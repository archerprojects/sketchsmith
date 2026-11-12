//! SketchSmith — edge marks and line styles | v1.0.0 | Thursday 12 November 2026
//!
//! Present-mode display data carried by edges and layers. Neither ever
//! changes geometry.
//!
//! * [`EdgeMark`]: how an edge takes part in smoothing. Auto follows the
//!   model's smoothing angle; Sharp and Smooth override it.
//!   [`Entities::edge_is_smooth`] decides, and [`Entities::corner_normals`]
//!   averages normals only across smooth edges (a fan of faces round each
//!   corner, stopped by sharp edges), as Blender's sharp edges do.
//! * [`LineStyle`]: an ISO 128 pen weight and line pattern. An edge's own
//!   style wins over its layer's; an edge with neither keeps the plain thin
//!   line.

use std::collections::{BTreeMap, BTreeSet};

use smith_geom::DVec3;

use crate::entities::Entities;
use crate::error::ModelError;
use crate::ids::{EdgeId, FaceId, VertexId};

/// How an edge takes part in smoothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum EdgeMark {
    /// Smooth when its faces meet at less than the smoothing angle.
    #[default]
    Auto,
    /// Always a crease, drawn.
    Sharp,
    /// Always shaded as one surface, not drawn.
    Smooth,
}

/// The ISO 128 pen weights, mm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum PenWeight {
    /// 0.13 mm.
    W013,
    /// 0.18 mm.
    W018,
    /// 0.25 mm.
    #[default]
    W025,
    /// 0.35 mm.
    W035,
    /// 0.5 mm.
    W050,
    /// 0.7 mm.
    W070,
    /// 1.0 mm.
    W100,
}

impl PenWeight {
    /// Every weight, thinnest first.
    pub const ALL: [PenWeight; 7] = [
        Self::W013,
        Self::W018,
        Self::W025,
        Self::W035,
        Self::W050,
        Self::W070,
        Self::W100,
    ];

    /// The weight in mm.
    pub fn mm(self) -> f64 {
        match self {
            Self::W013 => 0.13,
            Self::W018 => 0.18,
            Self::W025 => 0.25,
            Self::W035 => 0.35,
            Self::W050 => 0.5,
            Self::W070 => 0.7,
            Self::W100 => 1.0,
        }
    }

    /// Its position in [`PenWeight::ALL`] (the stored value).
    pub fn index(self) -> u8 {
        Self::ALL.iter().position(|w| *w == self).unwrap_or(0) as u8
    }

    /// The weight at `index` in [`PenWeight::ALL`].
    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(usize::from(index)).copied()
    }

    /// Width on screen in pixels (before display scaling): 6 px per mm of
    /// weight, never under 0.75 px, so each step reads heavier than the last
    /// at every zoom. True size is for exports.
    pub fn screen_pixels(self) -> f32 {
        (self.mm() * 6.0).max(0.75) as f32
    }
}

/// The ISO 128-20 line types offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum LinePattern {
    /// Type 01, continuous.
    #[default]
    Solid,
    /// Type 02, dashed: hidden outlines.
    Dashed,
    /// Type 04, long dash and dot: centre lines and cutting planes.
    DashDot,
    /// Type 05, long dash and two dots: adjacent parts, alternate positions.
    DashDoubleDot,
}

impl LinePattern {
    /// Every pattern, in the order offered.
    pub const ALL: [LinePattern; 4] = [
        Self::Solid,
        Self::Dashed,
        Self::DashDot,
        Self::DashDoubleDot,
    ];

    /// Its position in [`LinePattern::ALL`] (the stored value).
    pub fn index(self) -> u8 {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0) as u8
    }

    /// The pattern at `index` in [`LinePattern::ALL`].
    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(usize::from(index)).copied()
    }

    /// One period as alternating drawn and gap lengths, in multiples of the
    /// line width (ISO 128-20: dash 12, long dash 24, gap 3, dot 0.5).
    /// Empty for a solid line.
    pub fn elements(self) -> &'static [f32] {
        match self {
            Self::Solid => &[],
            Self::Dashed => &[12.0, 3.0],
            Self::DashDot => &[24.0, 3.0, 0.5, 3.0],
            Self::DashDoubleDot => &[24.0, 3.0, 0.5, 3.0, 0.5, 3.0],
        }
    }
}

/// A pen weight and a line pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct LineStyle {
    /// ISO pen weight.
    pub weight: PenWeight,
    /// ISO line type.
    pub pattern: LinePattern,
}

impl Entities {
    /// Marks edge `e` for smoothing.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn set_edge_mark(&mut self, e: EdgeId, mark: EdgeMark) -> Result<(), ModelError> {
        let edge = self.edges.get(e).ok_or(ModelError::NoSuchEntity)?;
        if edge.flags.mark != mark {
            self.edges[e].flags.mark = mark;
        }
        Ok(())
    }

    /// Whether the faces either side of edge `e` shade as one surface:
    /// exactly two faces, and either marked Smooth, or on Auto and folded by
    /// Auto-Tri or meeting at less than the angle whose cosine is
    /// `cos_limit` (face orientation ignored). Marked Sharp never is.
    pub fn edge_is_smooth(&self, e: EdgeId, cos_limit: f64) -> bool {
        let Some(edge) = self.edges.get(e) else {
            return false;
        };
        let [a, b] = edge.faces() else {
            return false;
        };
        match edge.flags.mark {
            EdgeMark::Sharp => false,
            EdgeMark::Smooth => true,
            EdgeMark::Auto => {
                edge.flags.smooth
                    || match (self.faces.get(*a), self.faces.get(*b)) {
                        (Some(fa), Some(fb)) => {
                            fa.plane().normal().dot(fb.plane().normal()).abs() >= cos_limit
                        }
                        _ => false,
                    }
            }
        }
    }

    /// The shading normal at each corner of face `f`: its own normal
    /// averaged with the faces reached round that corner across smooth
    /// edges (each turned to agree with `f`). `None` for `cos_limit` gives
    /// flat shading.
    pub fn corner_normals(&self, f: FaceId, cos_limit: Option<f64>) -> BTreeMap<VertexId, DVec3> {
        let mut out = BTreeMap::new();
        let Some(face) = self.faces.get(f) else {
            return out;
        };
        let n_f = face.plane().normal();
        let corners: Vec<VertexId> = face.loops().flat_map(|l| self.loop_vertices(l)).collect();
        for v in corners {
            let Some(cos) = cos_limit else {
                out.insert(v, n_f);
                continue;
            };
            let mut seen: BTreeSet<FaceId> = BTreeSet::from([f]);
            let mut queue = vec![f];
            let mut sum = DVec3::ZERO;
            while let Some(g) = queue.pop() {
                let Some(gf) = self.faces.get(g) else {
                    continue;
                };
                let n = gf.plane().normal();
                sum += if n.dot(n_f) < 0.0 { -n } else { n };
                for u in gf.loops().flatten() {
                    let Some(edge) = self.edges.get(u.edge) else {
                        continue;
                    };
                    if !edge.vertices().contains(&v) || !self.edge_is_smooth(u.edge, cos) {
                        continue;
                    }
                    for h in edge.faces() {
                        if seen.insert(*h) {
                            queue.push(*h);
                        }
                    }
                }
            }
            out.insert(v, sum.normalize_or(n_f));
        }
        out
    }

    /// Gives edge `e` its own line style (`None`: its layer's).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn set_edge_style(
        &mut self,
        e: EdgeId,
        style: Option<LineStyle>,
    ) -> Result<(), ModelError> {
        let edge = self.edges.get(e).ok_or(ModelError::NoSuchEntity)?;
        if edge.flags.style != style {
            self.edges[e].flags.style = style;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip() -> Entities {
        // Three faces in a row, the middle one folded up 20° and the last
        // up 70° from the first.
        let mut m = Entities::new();
        let (c20, s20) = (20f64.to_radians().cos(), 20f64.to_radians().sin());
        let (c70, s70) = (70f64.to_radians().cos(), 70f64.to_radians().sin());
        let a = DVec3::new(0.0, 0.0, 0.0);
        let b = DVec3::new(10.0, 0.0, 0.0);
        let c = b + DVec3::new(10.0 * c20, 0.0, 10.0 * s20);
        let d = c + DVec3::new(10.0 * c70, 0.0, 10.0 * s70);
        let y = DVec3::Y * 10.0;
        for (p, q) in [(a, b), (b, c), (c, d)] {
            m.add_polyline(&[p, q, q + y, p + y], true).unwrap();
        }
        m
    }

    #[test]
    fn smooth_edges_join_shading_sharp_ones_split_it_and_marks_override() {
        let mut m = strip();
        let cos30 = 30f64.to_radians().cos();
        let fold = m
            .edge_between(
                m.find_vertex(DVec3::new(10.0, 0.0, 0.0)).unwrap(),
                m.find_vertex(DVec3::new(10.0, 10.0, 0.0)).unwrap(),
            )
            .unwrap();
        assert!(m.edge_is_smooth(fold, cos30));
        let first = m.edges[fold].faces()[0];
        let v = m.find_vertex(DVec3::new(10.0, 0.0, 0.0)).unwrap();
        let shared = m.corner_normals(first, Some(cos30))[&v];
        let flat = m.corner_normals(first, None)[&v];
        assert!(shared.dot(flat) < 0.999);
        m.set_edge_mark(fold, EdgeMark::Sharp).unwrap();
        assert!(!m.edge_is_smooth(fold, cos30));
        let split = m.corner_normals(first, Some(cos30))[&v];
        assert!(split.dot(flat) > 0.999_999);
        // The 70° fold is sharp at 30° unless marked Smooth.
        let far = m
            .edge_ids()
            .find(|e| {
                let (a, b) = m.edge_points(*e).unwrap();
                a.x > 15.0 && b.x > 15.0 && (a.y - b.y).abs() > 1.0
            })
            .unwrap();
        assert!(!m.edge_is_smooth(far, cos30));
        m.set_edge_mark(far, EdgeMark::Smooth).unwrap();
        assert!(m.edge_is_smooth(far, cos30));
    }

    #[test]
    fn weights_and_patterns_round_trip_through_their_index() {
        for w in PenWeight::ALL {
            assert_eq!(PenWeight::from_index(w.index()), Some(w));
        }
        for p in LinePattern::ALL {
            assert_eq!(LinePattern::from_index(p.index()), Some(p));
        }
        assert_eq!(PenWeight::from_index(7), None);
    }

    #[test]
    fn each_weight_reads_heavier_than_the_last() {
        let px: Vec<f32> = PenWeight::ALL.iter().map(|w| w.screen_pixels()).collect();
        assert!(px.windows(2).all(|p| p[1] > p[0]));
        assert!((PenWeight::W050.screen_pixels() - 3.0).abs() < 1e-6);
    }
}
