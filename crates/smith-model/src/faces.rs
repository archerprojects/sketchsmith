//! SketchSmith — face finding and creation | v1.0.0 | Thursday 12 November 2026
//!
//! When an edge is added, every plane it shares with a neighbouring edge is
//! examined. In each plane the smallest closed loop on each side of the edge is
//! found by a *left-most turn walk*: from the edge's end, always take the
//! in-plane edge that turns furthest left. A walk that returns to its start
//! with positive (counter-clockwise) area has traced the smallest region to the
//! left of the edge; a negative area means it went round an outside boundary
//! and is discarded.
//!
//! Each loop found is then resolved against existing coplanar faces:
//!
//! * **Split** — both sides of the edge lie inside one existing face: the face
//!   is replaced by the two new faces, which inherit its attributes, normal
//!   direction, and holes (each hole goes to the new face that contains it).
//! * **Hole** — one closed loop lies inside an existing face without touching its
//!   boundary (a circle drawn on a face): the loop becomes a new face and a hole
//!   in the existing one. Holes of the existing face that the loop surrounds
//!   move to the new face (a rectangle drawn round a hole keeps the hole).
//! * **Auto-face** — the loop lies in empty space: it becomes a new face, and
//!   any separate coplanar faces it encloses become its holes.
//!
//! New faces are oriented with the normal pointing up (then +Y, then +X) unless
//! they inherit a normal. SketchUp leaves ground faces pointing down; SketchSmith
//! does not. Orient Faces and Reverse Faces are not built yet.

use std::collections::HashSet;

use smith_geom::DVec3;
use smith_geom::plane::{Plane, planar_loop};
use smith_geom::tolerance::TOL_MERGE;

use crate::entities::{EdgeUse, Entities, FaceAttributes, point_in_polygon, reverse_loop};
use crate::ids::{EdgeId, FaceId, VertexId};

/// Two planes whose normals differ by less than this (|cos| above 1 − ε) count as parallel.
const PARALLEL_COS_EPS: f64 = 1e-9;

impl Entities {
    /// Runs face resolution for a newly created edge.
    pub(crate) fn resolve_faces_for_edge(&mut self, e: EdgeId) {
        for plane in self.candidate_planes(e) {
            let [u, v] = self.edges[e].vertices();
            let mut loops = Vec::new();
            for (from, to) in [(u, v), (v, u)] {
                if let Some(l) = self.walk_left(&plane, from, to, e)
                    && self.signed_area(&l, &plane) > 0.0
                    && !self.loop_is_existing_face(&l)
                {
                    loops.push(l);
                }
            }
            self.resolve_loops(&plane, e, loops);
        }
    }

    /// Planes through edge `e` and each non-collinear neighbouring edge, deduplicated.
    fn candidate_planes(&self, e: EdgeId) -> Vec<Plane> {
        let [u, v] = self.edges[e].vertices();
        let pu = self.vertices[u].position();
        let pv = self.vertices[v].position();
        let mut planes: Vec<Plane> = Vec::new();
        for end in [u, v] {
            for &f in self.vertices[end].edges() {
                if f == e {
                    continue;
                }
                let w = self.edges[f].other(end);
                let pw = self.vertices[w].position();
                let Ok(plane) = Plane::from_three_points(pu, pv, pw) else {
                    continue; // collinear neighbour
                };
                // Reject near-collinear neighbours: w must be clearly off the line.
                let along = (pv - pu).normalize();
                let off = (pw - pu) - along * (pw - pu).dot(along);
                if off.length() <= TOL_MERGE {
                    continue;
                }
                if !planes.iter().any(|q| same_plane(q, &plane)) {
                    planes.push(plane);
                }
            }
        }
        planes
    }

    /// Left-most turn walk in `plane`, starting along edge `start` from `from` to `to`.
    /// Returns the loop if the walk closes as a simple cycle.
    pub(crate) fn walk_left(
        &self,
        plane: &Plane,
        from: VertexId,
        to: VertexId,
        start: EdgeId,
    ) -> Option<Vec<EdgeUse>> {
        let local = |v: VertexId| plane.to_local(self.vertices[v].position());
        let in_plane =
            |v: VertexId| plane.signed_distance(self.vertices[v].position()).abs() <= TOL_MERGE;

        let mut uses = Vec::new();
        let mut seen: HashSet<VertexId> = HashSet::from([from]);
        let (mut prev, mut cur, mut edge) = (from, to, start);

        for _ in 0..=self.edges.len() {
            uses.push(EdgeUse {
                edge,
                reversed: self.edges[edge].vertices()[0] != prev,
            });
            if cur == from {
                return Some(uses);
            }
            if !seen.insert(cur) {
                return None; // revisited a vertex: not a simple cycle
            }
            let d_in = local(cur) - local(prev);
            let mut best: Option<(f64, EdgeId, VertexId)> = None;
            for &cand in self.vertices[cur].edges() {
                if cand == edge {
                    continue;
                }
                let w = self.edges[cand].other(cur);
                if !in_plane(w) {
                    continue;
                }
                let d_out = local(w) - local(cur);
                let turn = d_in.perp_dot(d_out).atan2(d_in.dot(d_out));
                if best.is_none_or(|(b, _, _)| turn > b) {
                    best = Some((turn, cand, w));
                }
            }
            let (_, next_edge, next) = best?;
            prev = cur;
            cur = next;
            edge = next_edge;
        }
        None
    }

    /// Whether a face already has exactly this loop as its outer loop.
    fn loop_is_existing_face(&self, l: &[EdgeUse]) -> bool {
        let edges: HashSet<EdgeId> = l.iter().map(|u| u.edge).collect();
        self.edges[l[0].edge].faces().iter().any(|f| {
            let outer = self.faces[*f].outer();
            outer.len() == l.len() && outer.iter().all(|u| edges.contains(&u.edge))
        })
    }

    /// A point strictly inside the loop, just to the left of edge `e`'s midpoint.
    fn interior_sample(&self, l: &[EdgeUse], e: EdgeId, plane: &Plane) -> DVec3 {
        let u = l.iter().find(|u| u.edge == e).copied().unwrap_or(l[0]);
        let [a, b] = self.edges[u.edge].vertices();
        let (pa, pb) = if u.reversed {
            (self.vertices[b].position(), self.vertices[a].position())
        } else {
            (self.vertices[a].position(), self.vertices[b].position())
        };
        let dir = pb - pa;
        let left = plane.normal().cross(dir).normalize_or_zero();
        // Far enough to clear rounding, close enough to stay inside thin loops.
        let offset = (dir.length() * 1e-3).min(TOL_MERGE * 0.5);
        (pa + pb) * 0.5 + left * offset
    }

    /// The existing face coplanar with `plane` that strictly contains `p`, if any.
    fn coplanar_face_containing(
        &self,
        plane: &Plane,
        p: DVec3,
        exclude: &[FaceId],
    ) -> Option<FaceId> {
        self.faces.keys().find(|f| {
            !exclude.contains(f)
                && same_plane(self.faces[*f].plane(), plane)
                && self.face_contains_point(*f, p)
        })
    }

    /// Applies the split / hole / auto-face rules to loops found for edge `e`.
    fn resolve_loops(&mut self, plane: &Plane, e: EdgeId, loops: Vec<Vec<EdgeUse>>) {
        let containers: Vec<Option<FaceId>> = loops
            .iter()
            .map(|l| {
                let s = self.interior_sample(l, e, plane);
                self.coplanar_face_containing(plane, s, &[])
            })
            .collect();

        // Split: both sides of the edge fall in the same existing face.
        if loops.len() == 2 && containers[0].is_some() && containers[0] == containers[1] {
            let Some(g) = containers[0] else { return };
            let Some(old) = self.remove_face(g) else {
                return;
            };
            let normal = old.plane().normal();
            let mut created = Vec::new();
            for l in loops {
                if let Some(f) = self.create_face(l, Some(normal), old.attributes.clone()) {
                    created.push(f);
                }
            }
            for hole in old.inner() {
                let sample = self.vertices[self.use_start(hole[0])].position();
                if let Some(f) = created
                    .iter()
                    .copied()
                    .find(|f| self.outer_contains(*f, sample))
                {
                    self.add_inner_loop(f, hole.clone());
                }
            }
            return;
        }

        for (l, container) in loops.into_iter().zip(containers) {
            match container {
                Some(g) if !self.shares_vertex_with_outer(&l, g) => {
                    // Hole: a closed loop inside g, clear of g's boundary.
                    let normal = self.faces[g].plane().normal();
                    let attrs = self.faces[g].attributes.clone();
                    if let Some(f) = self.create_face(l.clone(), Some(normal), attrs) {
                        self.move_enclosed_holes(g, f);
                        self.add_inner_loop(g, l);
                    }
                }
                Some(_) => {
                    // A loop inside a face but touching its boundary at a vertex:
                    // not a valid simple split. Leave the edges as drawn (a known limit).
                }
                None => {
                    let attrs = FaceAttributes {
                        layer: self.draw_layer,
                        ..FaceAttributes::default()
                    };
                    if let Some(f) = self.create_face(l, None, attrs) {
                        self.adopt_enclosed_faces(f);
                    }
                }
            }
        }
    }

    /// Creates a face from a loop if it is planar. With `normal`, the face is
    /// oriented to agree with it; otherwise it points up (then +Y, then +X).
    pub(crate) fn create_face(
        &mut self,
        mut l: Vec<EdgeUse>,
        normal: Option<DVec3>,
        attributes: FaceAttributes,
    ) -> Option<FaceId> {
        let pts = self.loop_points(&l);
        let mut plane = planar_loop(&pts).ok().flatten()?;
        let flip = match normal {
            Some(n) => plane.normal().dot(n) < 0.0,
            None => !points_up(plane.normal()),
        };
        if flip {
            reverse_loop(&mut l);
            plane = plane.flipped();
        }
        Some(self.insert_face(l, Vec::new(), plane, attributes))
    }

    /// Whether `p` lies inside face `f`'s outer loop (ignoring holes).
    pub(crate) fn outer_contains(&self, f: FaceId, p: DVec3) -> bool {
        let face = &self.faces[f];
        point_in_polygon(
            face.plane().to_local(p),
            &self.loop_local(face.outer(), face.plane()),
        )
    }

    fn shares_vertex_with_outer(&self, l: &[EdgeUse], g: FaceId) -> bool {
        let outer: HashSet<VertexId> = self
            .loop_vertices(self.faces[g].outer())
            .into_iter()
            .collect();
        self.loop_vertices(l).iter().any(|v| outer.contains(v))
    }

    /// After auto-facing `f`, makes every separate coplanar face lying inside it
    /// a hole of `f` — but only the outermost ones (faces inside another
    /// enclosed face's holes are that face's business).
    fn adopt_enclosed_faces(&mut self, f: FaceId) {
        let plane = *self.faces[f].plane();
        let f_vertices: HashSet<VertexId> = self
            .loop_vertices(self.faces[f].outer())
            .into_iter()
            .collect();
        let candidates: Vec<FaceId> = self
            .faces
            .keys()
            .filter(|h| {
                *h != f && same_plane(self.faces[*h].plane(), &plane) && {
                    let hv = self.loop_vertices(self.faces[*h].outer());
                    hv.iter().all(|v| !f_vertices.contains(v))
                        && self.outer_contains(f, self.vertices[hv[0]].position())
                }
            })
            .collect();
        let outermost: Vec<FaceId> = candidates
            .iter()
            .copied()
            .filter(|h| {
                let p = self.vertices[self.use_start(self.faces[*h].outer()[0])].position();
                !candidates
                    .iter()
                    .any(|k| k != h && self.outer_contains(*k, p))
            })
            .collect();
        for h in outermost {
            let l = self.faces[h].outer().to_vec();
            self.add_inner_loop(f, l);
        }
    }
}

/// Whether two planes coincide within `TOL_MERGE` (either orientation).
pub(crate) fn same_plane(a: &Plane, b: &Plane) -> bool {
    a.normal().dot(b.normal()).abs() >= 1.0 - PARALLEL_COS_EPS
        && a.signed_distance(b.origin()).abs() <= TOL_MERGE
}

/// The default facing rule: +Z, then +Y, then +X.
fn points_up(n: DVec3) -> bool {
    const EPS: f64 = 1e-12;
    if n.z.abs() > EPS {
        n.z > 0.0
    } else if n.y.abs() > EPS {
        n.y > 0.0
    } else {
        n.x > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facing_rule() {
        assert!(points_up(DVec3::Z));
        assert!(!points_up(-DVec3::Z));
        assert!(points_up(DVec3::Y));
        assert!(points_up(DVec3::X));
        assert!(!points_up(-DVec3::X));
    }

    #[test]
    fn same_plane_ignores_orientation() {
        let a = Plane::from_point_normal(DVec3::ZERO, DVec3::Z).unwrap();
        let b = Plane::from_point_normal(DVec3::new(5.0, 5.0, 0.0), -DVec3::Z).unwrap();
        let c = Plane::from_point_normal(DVec3::new(0.0, 0.0, 1.0), DVec3::Z).unwrap();
        assert!(same_plane(&a, &b));
        assert!(!same_plane(&a, &c));
    }
}
