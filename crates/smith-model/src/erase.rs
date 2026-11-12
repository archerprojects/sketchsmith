//! SketchSmith — erasing and healing | v1.0.0 | Thursday 12 November 2026
//!
//! [`Entities::erase_edge`] follows SketchUp:
//!
//! * **Heal** — if the edge separates exactly two coplanar faces, they merge into
//!   one face that keeps the first face's attributes and normal, and both faces'
//!   holes. If the edge belonged to a hole of one face (a circle segment, say),
//!   that hole is dissolved and the face inside it is removed.
//! * **Remove** — otherwise every face that used the edge is deleted.
//!
//! The edge is then removed, along with any vertex left with no edges. If the
//! edge belonged to a curve, the curve is exploded first — except one segment
//! of an unsplit circle: the rest stays a curve, as an arc, so it remains
//! editable (as SketchUp does).
//!
//! [`Entities::erase_vertex`] does not heal: the vertex goes with every
//! edge meeting at it and every face using one of those edges. What is left
//! is redrawn with Fill.

use std::f64::consts::TAU;

use smith_geom::curve::{Arc, Curve};
use smith_geom::plane::Plane;

use crate::entities::{EdgeUse, Entities};
use crate::error::ModelError;
use crate::faces::same_plane;
use crate::ids::{CurveId, EdgeId, FaceId};

impl Entities {
    /// Erases an edge, healing coplanar faces on either side.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] if the edge does not exist.
    pub fn erase_edge(&mut self, e: EdgeId) -> Result<(), ModelError> {
        let edge = self.edges.get(e).ok_or(ModelError::NoSuchEntity)?;
        let faces: Vec<FaceId> = edge.faces().to_vec();
        // Erasing part of a curve turns the rest into plain edges (see
        // curves.rs), except that a circle missing one segment becomes an arc.
        if let Some(c) = edge.curve()
            && !self.circle_to_arc(c, e)
        {
            self.explode_curve(c)?;
        }

        let healed = faces.len() == 2
            && same_plane(self.faces[faces[0]].plane(), self.faces[faces[1]].plane())
            && self.heal(e, faces[0], faces[1]);

        if !healed {
            // The faces on the edge now (a failed heal puts them back under
            // new keys).
            let now: Vec<FaceId> = self.edges[e].faces().to_vec();
            for f in now {
                self.remove_face(f);
            }
        }
        self.remove_bare_edge(e);
        Ok(())
    }

    /// Erases a vertex, every edge meeting at it (back to the vertex at its
    /// other end) and every face using one of those edges. Nothing heals.
    /// Curves through it become plain edges.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] if the vertex does not exist.
    pub fn erase_vertex(&mut self, v: crate::ids::VertexId) -> Result<(), ModelError> {
        let edges: Vec<EdgeId> = self
            .vertices
            .get(v)
            .ok_or(ModelError::NoSuchEntity)?
            .edges()
            .to_vec();
        let curves: Vec<CurveId> = edges
            .iter()
            .filter_map(|e| self.edges[*e].curve())
            .collect();
        for c in curves {
            if self.curves.contains_key(c) {
                self.explode_curve(c)?;
            }
        }
        let faces: Vec<FaceId> = edges
            .iter()
            .flat_map(|e| self.edges[*e].faces().to_vec())
            .collect();
        for f in faces {
            self.remove_face(f);
        }
        for e in edges {
            self.remove_bare_edge(e);
        }
        Ok(())
    }

    /// Merge Coplanar for one edge: if `e` lies between exactly two faces
    /// on the same plane, one on each side of it, and is not part of a
    /// curve, the faces join into one (the first keeps its attributes) and
    /// the edge goes. Returns whether they joined; otherwise nothing
    /// changes — unlike [`Entities::erase_edge`], faces are never deleted.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] if the edge does not exist.
    pub fn merge_faces_across(&mut self, e: EdgeId) -> Result<bool, ModelError> {
        let edge = self.edges.get(e).ok_or(ModelError::NoSuchEntity)?;
        let [f1, f2] = edge.faces()[..] else {
            return Ok(false);
        };
        if edge.curve().is_some()
            || f1 == f2
            || !same_plane(self.faces[f1].plane(), self.faces[f2].plane())
        {
            return Ok(false);
        }
        // Each face lies to the left of its loops' walk; the two must lie
        // on opposite sides of the edge (not one on top of the other).
        let (Some(s1), Some(s2)) = (self.inward(f1, e), self.inward(f2, e)) else {
            return Ok(false);
        };
        if s1.dot(s2) >= 0.0 {
            return Ok(false);
        }
        // Joining across a hole's edge fills the hole: its other edges are
        // left between nothing and go too.
        let hole: Vec<EdgeId> = [f1, f2]
            .iter()
            .flat_map(|f| self.faces[*f].inner())
            .find(|l| l.iter().any(|u| u.edge == e))
            .map(|l| l.iter().map(|u| u.edge).filter(|x| *x != e).collect())
            .unwrap_or_default();
        if !self.heal(e, f1, f2) {
            return Ok(false);
        }
        self.remove_bare_edge(e);
        for x in hole {
            if self
                .edges
                .get(x)
                .is_some_and(|edge| edge.faces().is_empty())
            {
                self.remove_bare_edge(x);
            }
        }
        Ok(true)
    }

    /// The direction from edge `e` into face `f`, in the face's plane.
    fn inward(&self, f: FaceId, e: EdgeId) -> Option<smith_geom::DVec3> {
        let face = self.faces.get(f)?;
        let u = face.loops().flatten().find(|u| u.edge == e)?;
        let [a, b] = self.edges[e].vertices();
        let (from, to) = if u.reversed { (b, a) } else { (a, b) };
        let d = self.vertices[to].position() - self.vertices[from].position();
        Some(face.plane().normal().cross(d))
    }

    /// Merges coplanar faces `f1` and `f2` across edge `e`. Returns `false` (and
    /// changes nothing) if the merged boundary cannot be traced.
    fn heal(&mut self, e: EdgeId, f1: FaceId, f2: FaceId) -> bool {
        let in_inner = |s: &Self, f: FaceId| {
            s.faces[f]
                .inner()
                .iter()
                .any(|l| l.iter().any(|u| u.edge == e))
        };

        // Case 1: e lies on a hole of one face; the other face fills that hole.
        for (host, filler) in [(f1, f2), (f2, f1)] {
            if in_inner(self, host) {
                let Some(fill) = self.remove_face(filler) else {
                    return false;
                };
                let Some(old) = self.remove_face(host) else {
                    return false;
                };
                let mut inner: Vec<Vec<EdgeUse>> = old
                    .inner()
                    .iter()
                    .filter(|l| !l.iter().any(|u| u.edge == e))
                    .cloned()
                    .collect();
                inner.extend(fill.inner().iter().cloned());
                let plane = *old.plane();
                let id = self.insert_face(old.outer().to_vec(), Vec::new(), plane, old.attributes);
                for l in inner {
                    self.add_inner_loop(id, l);
                }
                self.detach_edge_faces(e);
                return true;
            }
        }

        // Case 2: e is on both outer loops. Trace the merged outer boundary by a
        // left-most walk from an edge of f1's outer loop that is not e.
        let plane: Plane = *self.faces[f1].plane();
        let Some(start) = self.faces[f1].outer().iter().copied().find(|u| u.edge != e) else {
            return false;
        };
        let attrs = self.faces[f1].attributes.clone();
        let mut inner: Vec<Vec<EdgeUse>> = self.faces[f1].inner().to_vec();
        inner.extend(self.faces[f2].inner().iter().cloned());

        // Detach both faces and the edge before walking, so the walk cannot use e.
        let Some(face1) = self.remove_face(f1) else {
            return false;
        };
        let Some(face2) = self.remove_face(f2) else {
            return false;
        };
        let [a, b] = self.edges[e].vertices();
        self.vertices[a].edges_mut_remove(e);
        self.vertices[b].edges_mut_remove(e);

        let from = self.use_start(start);
        let to = self.edges[start.edge].other(from);
        let merged = self.walk_left(&plane, from, to, start.edge);

        // Restore e's vertex links; remove_bare_edge performs the final removal.
        self.vertices[a].edges_mut_push(e);
        self.vertices[b].edges_mut_push(e);

        let Some(merged) = merged.filter(|l| self.signed_area(l, &plane) > 0.0) else {
            // Untraceable: put both faces back as they were and report it
            // (Erase then removes them; Merge Coplanar leaves them).
            for face in [face1, face2] {
                let plane = *face.plane();
                self.insert_face(face.outer, face.inner, plane, face.attributes);
            }
            return false;
        };
        let id = self.insert_face(merged, Vec::new(), plane, attrs);
        for l in inner {
            self.add_inner_loop(id, l);
        }
        true
    }

    /// If `c` is an unsplit circle and `e` one of its segments, turns the
    /// circle into the arc of its other segments and returns `true`.
    fn circle_to_arc(&mut self, c: CurveId, e: EdgeId) -> bool {
        let Some(record) = self.curves.get(c) else {
            return false;
        };
        let Curve::Circle(circle) = record.curve() else {
            return false;
        };
        let n = record.segments();
        if n < 3 || record.edges().len() != n as usize {
            return false;
        }
        let Some((a, b)) = self.edge_points(e) else {
            return false;
        };
        let frame = *circle.frame();
        let angle = |p: smith_geom::DVec3| {
            let d = p - frame.center();
            d.dot(frame.y_axis())
                .atan2(d.dot(frame.x_axis()))
                .rem_euclid(TAU)
        };
        let step = TAU / f64::from(n);
        let (ta, tb) = (angle(a), angle(b));
        // The erased segment runs counter-clockwise from `from` to `to`.
        let from = if ((tb - ta).rem_euclid(TAU) - step).abs() < 1e-6 {
            ta
        } else {
            tb
        };
        let start = from + step;
        let Ok(arc) = Arc::new(frame, circle.radius(), start, start + TAU - step) else {
            return false;
        };
        let record = &mut self.curves[c];
        record.curve = Curve::Arc(arc);
        record.segments = n - 1;
        true
    }

    fn detach_edge_faces(&mut self, e: EdgeId) {
        let faces: Vec<FaceId> = self.edges[e].faces().to_vec();
        for f in faces {
            self.remove_face(f);
        }
    }
}
