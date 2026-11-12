//! SketchSmith — curves in the model | v1.0.0 | Thursday 12 November 2026
//!
//! A [`CurveRecord`] ties a true curve (`smith_geom::curve::Curve`) to the edges
//! generated from it. Because the analytic curve is kept, its
//! segment count can be changed at any time and the edges are rebuilt — the
//! basis for right-click → Edit on circles and arcs.
//!
//! Rules:
//! * **Regenerate** — the old edges are erased (healing faces as usual) and the
//!   curve is redrawn with the new count, so sticky behaviour recreates faces,
//!   holes and splits against whatever else is drawn. Each face rebuilt
//!   beside the curve takes back the attributes (layer, hidden, paint) of the
//!   face that lay where it lies.
//! * **Locked** — once a curve's edges bound a face that is not in the curve's
//!   plane (it has been extrudeed into a cylinder), the segment count can no
//!   longer change: the side faces depend on it. [`ModelError::CurveLocked`].
//! * **Split** — when another line crosses a curve, the split pieces stay members
//!   of the same curve, so it remains editable; regeneration re-splits at the
//!   crossing.
//! * **Erase** — erasing one of a curve's edges explodes the curve: the
//!   remaining edges become plain edges.
//! * **Explode** — [`Entities::explode_curve`] removes the curve and keeps its
//!   edges as plain edges.

use smith_geom::DVec3;
use smith_geom::curve::Curve;
use smith_geom::plane::Plane;
use smith_geom::tolerance::{TOL_MERGE, coincident};

use crate::entities::{Entities, FaceAttributes, point_in_polygon};
use crate::error::ModelError;
use crate::faces::same_plane;
use crate::ids::{CurveId, EdgeId, FaceId};

/// A face beside a curve before the curve is rebuilt: what it carried and
/// the region it covered.
struct FaceBefore {
    attributes: FaceAttributes,
    plane: Plane,
    outer: Vec<DVec3>,
    holes: Vec<Vec<DVec3>>,
}

impl FaceBefore {
    /// Whether `p` (on the plane) lay inside the face, holes excluded.
    fn covers(&self, p: DVec3) -> bool {
        let local = |pts: &[DVec3]| {
            pts.iter()
                .map(|x| self.plane.to_local(*x))
                .collect::<Vec<_>>()
        };
        let q = self.plane.to_local(p);
        point_in_polygon(q, &local(&self.outer))
            && !self.holes.iter().any(|h| point_in_polygon(q, &local(h)))
    }
}

/// A curve and the edges generated from it.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveRecord {
    pub(crate) curve: Curve,
    pub(crate) segments: u32,
    pub(crate) edges: Vec<EdgeId>,
}

impl CurveRecord {
    /// The analytic curve.
    pub fn curve(&self) -> &Curve {
        &self.curve
    }

    /// The segment count used to generate the edges.
    pub fn segments(&self) -> u32 {
        self.segments
    }

    /// The member edges (in no particular order once split).
    pub fn edges(&self) -> &[EdgeId] {
        &self.edges
    }
}

impl Entities {
    /// Number of curves.
    pub fn curve_count(&self) -> usize {
        self.curves.len()
    }

    /// A curve by key.
    pub fn curve(&self, id: CurveId) -> Option<&CurveRecord> {
        self.curves.get(id)
    }

    /// All curve keys.
    pub fn curve_ids(&self) -> impl Iterator<Item = CurveId> + '_ {
        self.curves.keys()
    }

    /// Draws a curve with full sticky behaviour and records it.
    ///
    /// # Errors
    /// [`ModelError::Geometry`] for an invalid segment count;
    /// otherwise as [`Entities::add_line`].
    pub fn add_curve(&mut self, curve: Curve, segments: u32) -> Result<CurveId, ModelError> {
        let edges = self.draw_curve_edges(&curve, segments)?;
        let id = self.curves.insert(CurveRecord {
            curve,
            segments,
            edges: Vec::new(),
        });
        self.attach_curve_edges(id, edges);
        Ok(id)
    }

    /// Changes a curve's segment count and rebuilds its edges.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`]; [`ModelError::Geometry`] for an invalid
    /// count; [`ModelError::CurveLocked`] if the curve bounds out-of-plane faces.
    pub fn set_curve_segments(&mut self, id: CurveId, segments: u32) -> Result<(), ModelError> {
        let record = self.curves.get(id).ok_or(ModelError::NoSuchEntity)?;
        let curve = record.curve.clone();
        curve.tessellate(segments).map_err(ModelError::Geometry)?;
        if self.curve_is_locked(id) {
            return Err(ModelError::CurveLocked);
        }
        let old = record.edges.clone();
        let before = self.faces_beside(&old);
        for e in &old {
            if let Some(edge) = self.edges.get_mut(*e) {
                edge.curve = None;
            }
        }
        for e in old {
            if self.edges.contains_key(e) {
                self.erase_edge(e)?;
            }
        }
        let edges = self.draw_curve_edges(&curve, segments)?;
        let record = &mut self.curves[id];
        record.segments = segments;
        record.edges.clear();
        self.attach_curve_edges(id, edges);
        let rebuilt = self.curves[id].edges.clone();
        self.restore_faces_beside(&rebuilt, &before);
        Ok(())
    }

    /// Every face using any of `edges`, each once.
    fn faces_using(&self, edges: &[EdgeId]) -> Vec<FaceId> {
        let mut out: Vec<FaceId> = Vec::new();
        for e in edges {
            if let Some(edge) = self.edges.get(*e) {
                for f in edge.faces() {
                    if !out.contains(f) {
                        out.push(*f);
                    }
                }
            }
        }
        out
    }

    /// The faces using any of `edges`, as they are now.
    fn faces_beside(&self, edges: &[EdgeId]) -> Vec<FaceBefore> {
        self.faces_using(edges)
            .into_iter()
            .filter_map(|f| self.faces.get(f))
            .map(|face| FaceBefore {
                attributes: face.attributes.clone(),
                plane: *face.plane(),
                outer: self.loop_points(face.outer()),
                holes: face.inner().iter().map(|l| self.loop_points(l)).collect(),
            })
            .collect()
    }

    /// Gives each face now using `edges` the attributes of the face in
    /// `before` that covered it: the one on its plane containing a point well
    /// inside it.
    fn restore_faces_beside(&mut self, edges: &[EdgeId], before: &[FaceBefore]) {
        for f in self.faces_using(edges) {
            let Some(face) = self.faces.get(f) else {
                continue;
            };
            let Some(inside) = self.point_inside(f) else {
                continue;
            };
            let plane = *face.plane();
            let Some(source) = before
                .iter()
                .find(|b| same_plane(&b.plane, &plane) && b.covers(inside))
            else {
                continue;
            };
            let mut attributes = source.attributes.clone();
            // The face may now face the other way to the one it replaces.
            if plane.normal().dot(source.plane.normal()) < 0.0 {
                std::mem::swap(&mut attributes.front, &mut attributes.back);
            }
            if self.faces[f].attributes != attributes {
                self.faces[f].attributes = attributes;
            }
        }
    }

    /// Replaces a curve's shape and segment count (Edit Curve) and
    /// rebuilds its edges with the normal sticky rules. The curve keeps its id.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`]; [`ModelError::Geometry`] if the new curve
    /// cannot be tessellated at `segments`; [`ModelError::CurveLocked`] if the
    /// curve bounds out-of-plane faces (its edges cannot be rebuilt in place).
    pub fn set_curve(
        &mut self,
        id: CurveId,
        curve: Curve,
        segments: u32,
    ) -> Result<(), ModelError> {
        if !self.curves.contains_key(id) {
            return Err(ModelError::NoSuchEntity);
        }
        curve.tessellate(segments).map_err(ModelError::Geometry)?;
        if self.curve_is_locked(id) {
            return Err(ModelError::CurveLocked);
        }
        let old = self.curves[id].edges.clone();
        let before = self.faces_beside(&old);
        for e in &old {
            if let Some(edge) = self.edges.get_mut(*e) {
                edge.curve = None;
            }
        }
        for e in old {
            if self.edges.contains_key(e) {
                self.erase_edge(e)?;
            }
        }
        let edges = self.draw_curve_edges(&curve, segments)?;
        let record = &mut self.curves[id];
        record.curve = curve;
        record.segments = segments;
        record.edges.clear();
        self.attach_curve_edges(id, edges);
        let rebuilt = self.curves[id].edges.clone();
        self.restore_faces_beside(&rebuilt, &before);
        Ok(())
    }

    /// Removes a curve, leaving its edges as plain edges.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] if the curve does not exist.
    pub fn explode_curve(&mut self, id: CurveId) -> Result<(), ModelError> {
        let record = self.curves.remove(id).ok_or(ModelError::NoSuchEntity)?;
        for e in record.edges {
            if let Some(edge) = self.edges.get_mut(e) {
                edge.curve = None;
            }
        }
        Ok(())
    }

    /// Whether a curve's segment count is locked.
    pub fn curve_is_locked(&self, id: CurveId) -> bool {
        let Some(record) = self.curves.get(id) else {
            return false;
        };
        let plane = curve_plane(&record.curve);
        record.edges.iter().any(|e| {
            self.edges.get(*e).is_some_and(|edge| {
                edge.faces().iter().any(|f| match &plane {
                    Some(p) => !same_plane(self.faces[*f].plane(), p),
                    None => true,
                })
            })
        })
    }

    fn draw_curve_edges(
        &mut self,
        curve: &Curve,
        segments: u32,
    ) -> Result<Vec<EdgeId>, ModelError> {
        let tess = curve.tessellate(segments).map_err(ModelError::Geometry)?;
        self.add_polyline(&tess.points, tess.closed)?;
        let n = tess.points.len();
        let spans = if tess.closed { n } else { n.saturating_sub(1) };
        let mut edges = Vec::new();
        for i in 0..spans {
            edges.extend(self.edges_along(tess.points[i], tess.points[(i + 1) % n]));
        }
        Ok(edges)
    }

    fn attach_curve_edges(&mut self, id: CurveId, edges: Vec<EdgeId>) {
        for e in &edges {
            if let Some(edge) = self.edges.get_mut(*e) {
                edge.curve = Some(id);
            }
        }
        self.curves[id].edges = edges;
    }

    /// The chain of existing edges running from the vertex at `a` to the vertex
    /// at `b` along the straight segment between them.
    pub(crate) fn edges_along(&self, a: DVec3, b: DVec3) -> Vec<EdgeId> {
        let (Some(va), Some(vb)) = (self.find_vertex(a), self.find_vertex(b)) else {
            return Vec::new();
        };
        let pb = self.vertices[vb].position();
        let mut out = Vec::new();
        let mut cur = va;
        for _ in 0..=self.edges.len() {
            if cur == vb {
                return out;
            }
            let pc = self.vertices[cur].position();
            let here = pc.distance(pb);
            let next = self.vertices[cur].edges().iter().copied().find(|e| {
                let w = self.vertices[self.edges[*e].other(cur)].position();
                let (_, q) = smith_geom::intersect::closest_point_on_segment(w, a, b);
                coincident(q, w) && w.distance(pb) < here - TOL_MERGE * 0.5
            });
            let Some(e) = next else { return out };
            out.push(e);
            cur = self.edges[e].other(cur);
        }
        out
    }
}

/// The plane a curve lies in, if it is planar.
fn curve_plane(curve: &Curve) -> Option<Plane> {
    match curve {
        Curve::Arc(a) => Plane::from_point_normal(a.frame().center(), a.frame().normal()).ok(),
        Curve::Circle(c) => Plane::from_point_normal(c.frame().center(), c.frame().normal()).ok(),
        Curve::Polygon(p) => Plane::from_point_normal(p.frame().center(), p.frame().normal()).ok(),
        Curve::Bezier(_) | Curve::Polyline(_) | Curve::Helix(_) => {
            let pts = curve
                .tessellate(curve.default_segments().max(1))
                .ok()?
                .points;
            smith_geom::plane::planar_loop(&pts).ok().flatten()
        }
    }
}
