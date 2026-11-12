//! SketchSmith — section planes | v1.0.0 | Thursday 12 November 2026
//!
//! A section plane cuts the view: while it is active, everything on the side
//! its normal points to is not drawn, and where the plane passes through
//! faces the cut is outlined and filled. Each context keeps its own section
//! planes, at most one of them active; a scene remembers which of the
//! root's planes is active.
//!
//! [`Entities::slice`] works out the cut itself: where the plane crosses
//! each face, as segments, and those segments joined into closed loops with
//! the holes sorted under the loops that hold them, ready to fill or to turn
//! into geometry (Create Group from Slice).

use std::collections::BTreeMap;

use smith_geom::{DVec2, DVec3};

use crate::entities::Entities;
use crate::error::ModelError;
use crate::ids::SectionId;

/// A section plane: through `point`, cutting away the side `normal`
/// points to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionPlane {
    /// A point on the plane.
    pub point: DVec3,
    /// Unit normal, toward the side cut away.
    pub normal: DVec3,
    /// The plane is the context's active one.
    pub active: bool,
}

impl SectionPlane {
    /// A plane through `point` cutting toward `normal` (normalised), not
    /// active. `None` for a zero or non-finite normal.
    pub fn new(point: DVec3, normal: DVec3) -> Option<Self> {
        let normal = normal.try_normalize()?;
        point.is_finite().then_some(Self {
            point,
            normal,
            active: false,
        })
    }

    /// Whether it is finite with a unit normal.
    pub fn is_valid(&self) -> bool {
        self.point.is_finite()
            && self.normal.is_finite()
            && (self.normal.length() - 1.0).abs() <= 1e-9
    }

    /// Signed distance of `p` from the plane, positive on the cut-away side.
    pub fn distance(&self, p: DVec3) -> f64 {
        (p - self.point).dot(self.normal)
    }

    /// The plane as `[nx, ny, nz, d]` with `n·p = d` on it.
    pub fn equation(&self) -> [f64; 4] {
        let n = self.normal;
        [n.x, n.y, n.z, n.dot(self.point)]
    }
}

/// The cut a section plane makes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slice {
    /// Every segment where the plane crosses a face.
    pub segments: Vec<[DVec3; 2]>,
    /// The segments joined into closed loops, each an outer loop and the
    /// holes directly inside it.
    pub regions: Vec<(Vec<DVec3>, Vec<Vec<DVec3>>)>,
}

impl Entities {
    /// Every section plane with its key, in creation order.
    pub fn sections(&self) -> impl Iterator<Item = (SectionId, &SectionPlane)> + '_ {
        self.sections.iter()
    }

    /// A section plane by key.
    pub fn section(&self, id: SectionId) -> Option<&SectionPlane> {
        self.sections.get(id)
    }

    /// The active section plane, if any.
    pub fn active_section(&self) -> Option<(SectionId, &SectionPlane)> {
        self.sections.iter().find(|(_, s)| s.active)
    }

    /// Adds a section plane and makes it the active one.
    ///
    /// # Errors
    /// [`ModelError::InvalidData`] for a malformed plane.
    pub fn add_section(&mut self, plane: SectionPlane) -> Result<SectionId, ModelError> {
        if !plane.is_valid() {
            return Err(ModelError::InvalidData("malformed section plane".into()));
        }
        let id = self.sections.insert(SectionPlane {
            active: false,
            ..plane
        });
        self.set_active_section(Some(id))?;
        Ok(id)
    }

    /// Moves or turns section plane `id`, keeping whether it is active.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`], or [`ModelError::InvalidData`].
    pub fn replace_section(
        &mut self,
        id: SectionId,
        plane: SectionPlane,
    ) -> Result<(), ModelError> {
        if !plane.is_valid() {
            return Err(ModelError::InvalidData("malformed section plane".into()));
        }
        let slot = self.sections.get_mut(id).ok_or(ModelError::NoSuchEntity)?;
        *slot = SectionPlane {
            active: slot.active,
            ..plane
        };
        Ok(())
    }

    /// Makes `id` the active section plane (`None`: none active).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] for a missing plane.
    pub fn set_active_section(&mut self, id: Option<SectionId>) -> Result<(), ModelError> {
        if let Some(id) = id
            && self.sections.get(id).is_none()
        {
            return Err(ModelError::NoSuchEntity);
        }
        let keys: Vec<SectionId> = self.sections.keys().collect();
        for k in keys {
            let want = Some(k) == id;
            if let Some(s) = self.sections.get(k)
                && s.active != want
            {
                let mut s = *s;
                s.active = want;
                if let Some(slot) = self.sections.get_mut(k) {
                    *slot = s;
                }
            }
        }
        Ok(())
    }

    /// Erases section plane `id`.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn remove_section(&mut self, id: SectionId) -> Result<SectionPlane, ModelError> {
        self.sections.remove(id).ok_or(ModelError::NoSuchEntity)
    }

    /// The cut `plane` makes through the faces that `shown` keeps.
    ///
    /// Each face's loops are crossed with the plane; the crossings along
    /// the line where the face's plane meets the section plane pair up
    /// inside the face (even–odd), so holes are respected. A point exactly
    /// on the plane counts as on the kept side, so a face lying in the plane
    /// gives nothing and shared corners are not counted twice.
    pub fn slice(&self, plane: &SectionPlane, shown: impl Fn(crate::ids::FaceId) -> bool) -> Slice {
        let mut segments = Vec::new();
        for f in self.face_ids().filter(|f| shown(*f)) {
            let Some(face) = self.face(f) else {
                continue;
            };
            let fn_ = face.plane().normal();
            let Some(along) = fn_.cross(plane.normal).try_normalize() else {
                continue;
            };
            let mut hits: Vec<(f64, DVec3)> = Vec::new();
            let loops = std::iter::once(face.outer()).chain(face.inner().iter().map(Vec::as_slice));
            for l in loops {
                let pts: Vec<DVec3> = l
                    .iter()
                    .filter_map(|u| {
                        let (a, b) = self.edge(u.edge)?.vertices().into();
                        let v = if u.reversed { b } else { a };
                        self.vertex(v).map(|x| x.position())
                    })
                    .collect();
                for i in 0..pts.len() {
                    let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                    let (da, db) = (plane.distance(a), plane.distance(b));
                    if (da > 0.0) != (db > 0.0) {
                        let p = a + (b - a) * (da / (da - db));
                        hits.push((p.dot(along), p));
                    }
                }
            }
            hits.sort_by(|x, y| x.0.total_cmp(&y.0));
            for [(_, a), (_, b)] in hits.as_chunks::<2>().0 {
                if a.distance(*b) > 1e-9 {
                    segments.push([*a, *b]);
                }
            }
        }
        let regions = regions(&segments, plane);
        Slice { segments, regions }
    }
}

/// Joins `segments` into closed loops and sorts them into regions: each
/// outer loop with the holes directly inside it. Ends within 1e-6 mm meet.
fn regions(segments: &[[DVec3; 2]], plane: &SectionPlane) -> Vec<(Vec<DVec3>, Vec<Vec<DVec3>>)> {
    // Snap ends to shared points.
    let mut points: Vec<DVec3> = Vec::new();
    let mut index = |p: DVec3| -> usize {
        match points.iter().position(|q| q.distance(p) <= 1e-6) {
            Some(i) => i,
            None => {
                points.push(p);
                points.len() - 1
            }
        }
    };
    let mut links: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for [a, b] in segments {
        let (i, j) = (index(*a), index(*b));
        if i != j {
            links.entry(i).or_default().push(j);
            links.entry(j).or_default().push(i);
        }
    }
    // Walk loops through points of degree two.
    let mut used: std::collections::BTreeSet<(usize, usize)> = Default::default();
    let mut loops: Vec<Vec<usize>> = Vec::new();
    for (&start, next) in &links {
        for &first in next {
            if used.contains(&(start.min(first), start.max(first))) {
                continue;
            }
            let mut ring = vec![start];
            let (mut prev, mut at) = (start, first);
            used.insert((start.min(first), start.max(first)));
            let mut closed = false;
            while ring.len() <= points.len() {
                if at == start {
                    closed = true;
                    break;
                }
                ring.push(at);
                let Some(&to) = links.get(&at).and_then(|n| {
                    n.iter()
                        .find(|&&x| x != prev && !used.contains(&(at.min(x), at.max(x))))
                }) else {
                    break;
                };
                used.insert((at.min(to), at.max(to)));
                prev = at;
                at = to;
            }
            if closed && ring.len() >= 3 {
                loops.push(ring);
            }
        }
    }
    // Flatten into the plane to sort outer loops from holes.
    let u = plane.normal.any_orthonormal_vector();
    let v = plane.normal.cross(u);
    let flat = |ring: &[usize]| -> Vec<DVec2> {
        ring.iter()
            .map(|&i| DVec2::new(points[i].dot(u), points[i].dot(v)))
            .collect()
    };
    let rings: Vec<Vec<DVec2>> = loops.iter().map(|r| flat(r)).collect();
    let area = |r: &[DVec2]| -> f64 {
        (0..r.len())
            .map(|i| r[i].perp_dot(r[(i + 1) % r.len()]))
            .sum::<f64>()
            .abs()
            * 0.5
    };
    let inside = |p: DVec2, r: &[DVec2]| -> bool {
        let mut c = false;
        for i in 0..r.len() {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                c = !c;
            }
        }
        c
    };
    // Depth = how many other loops contain it; even depth is an outer loop.
    let depth: Vec<usize> = (0..rings.len())
        .map(|i| {
            (0..rings.len())
                .filter(|&j| {
                    j != i && area(&rings[j]) > area(&rings[i]) && inside(rings[i][0], &rings[j])
                })
                .count()
        })
        .collect();
    let to3 = |ring: &[usize]| ring.iter().map(|&i| points[i]).collect::<Vec<_>>();
    let mut out = Vec::new();
    for i in (0..rings.len()).filter(|&i| depth[i].is_multiple_of(2)) {
        let holes = (0..rings.len())
            .filter(|&j| {
                depth[j] == depth[i] + 1
                    && area(&rings[i]) > area(&rings[j])
                    && inside(rings[j][0], &rings[i])
            })
            .map(|j| to3(&loops[j]))
            .collect();
        out.push((to3(&loops[i]), holes));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    /// A 10 mm cube with a 4 mm square hole straight down through it.
    fn holed_cube() -> Entities {
        let mut e = Entities::new();
        let sq = |s: f64, o: f64| {
            [
                DVec3::new(o, o, 0.0),
                DVec3::new(o + s, o, 0.0),
                DVec3::new(o + s, o + s, 0.0),
                DVec3::new(o, o + s, 0.0),
            ]
        };
        let up = DVec3::new(0.0, 0.0, 10.0);
        for ring in [sq(10.0, 0.0), sq(4.0, 3.0)] {
            for i in 0..4 {
                let (a, b) = (ring[i], ring[(i + 1) % 4]);
                e.add_line(a, b).unwrap();
                e.add_line(a + up, b + up).unwrap();
                e.add_line(a, a + up).unwrap();
            }
        }
        e
    }

    #[test]
    fn a_level_cut_through_a_holed_cube_is_a_square_with_a_hole() {
        let e = holed_cube();
        let plane = SectionPlane::new(DVec3::new(0.0, 0.0, 5.0), DVec3::Z).unwrap();
        let s = e.slice(&plane, |_| true);
        assert_eq!(s.segments.len(), 8);
        assert_eq!(s.regions.len(), 1);
        let (outer, holes) = &s.regions[0];
        assert_eq!(outer.len(), 4);
        assert_eq!(holes.len(), 1);
        assert!(outer.iter().all(|p| (p.z - 5.0).abs() < 1e-12));
    }

    #[test]
    fn one_plane_is_active_at_a_time_and_adding_undoes() {
        let mut doc = Document::new();
        let a = doc
            .transaction("Section", |d| {
                d.root_mut()
                    .add_section(SectionPlane::new(DVec3::ZERO, DVec3::Z).unwrap())
            })
            .unwrap();
        let b = doc
            .transaction("Section", |d| {
                d.root_mut()
                    .add_section(SectionPlane::new(DVec3::ZERO, DVec3::X).unwrap())
            })
            .unwrap();
        assert_eq!(doc.root().active_section().map(|(k, _)| k), Some(b));
        assert!(!doc.root().section(a).unwrap().active);
        assert!(doc.undo().is_some());
        assert_eq!(doc.root().active_section().map(|(k, _)| k), Some(a));
    }
}
