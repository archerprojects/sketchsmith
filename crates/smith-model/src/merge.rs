//! SketchSmith — merging points | v1.0.0 | Thursday 12 November 2026
//!
//! [`Entities::merge_vertices`] joins points into one, as Blender's Merge by
//! Distance does. Points named together form groups (a point merged with a
//! second that is merged with a third makes one group of three). Each group
//! keeps one point where it is — the one with the most edges, so the least
//! geometry moves — and the others join it:
//!
//! * an edge between two points of one group has no length left and goes;
//! * two edges that now join the same two points become one;
//! * every face around a moved point is rebuilt through the kept points,
//!   with its layer, paint and holes. A face left with fewer than three
//!   corners goes; so does a hole.
//!
//! Faces stay flat: if a face would bend, nothing changes and the faces
//! are named ([`ModelError::NotFlat`]) — unless Auto-Tri is on for the
//! operation, when just those faces are folded into triangles first.
//! Curves through a moved point become plain edges, since their points no
//! longer follow the curve.

use std::collections::{HashMap, HashSet};

use smith_geom::DVec3;
use smith_geom::plane::Plane;
use smith_geom::tolerance::{extent, planar_tolerance};

use crate::entities::{EdgeFlags, EdgeUse, Entities, FaceAttributes, reverse_loop};
use crate::error::ModelError;
use crate::ids::{CurveId, FaceId, VertexId};

/// A face taken out to be rebuilt through the kept points.
struct Rebuild {
    outer: Vec<VertexId>,
    holes: Vec<Vec<VertexId>>,
    plane: Plane,
    attributes: FaceAttributes,
}

impl Entities {
    /// Merges the points within `distance` of each other, among `scope`
    /// (every point when `None`). Returns how many points were merged away.
    ///
    /// # Errors
    /// As [`Entities::merge_vertices`].
    pub fn merge_by_distance(
        &mut self,
        scope: Option<&[VertexId]>,
        distance: f64,
    ) -> Result<usize, ModelError> {
        if !distance.is_finite() || distance <= 0.0 {
            return Ok(0);
        }
        let mut points: Vec<(VertexId, DVec3)> = match scope {
            Some(ids) => ids
                .iter()
                .filter_map(|v| self.vertices.get(*v).map(|x| (*v, x.position())))
                .collect(),
            None => self
                .vertices
                .iter()
                .map(|(v, x)| (v, x.position()))
                .collect(),
        };
        // Sweep along X: only points this close in X can be this close.
        points.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
        let mut pairs = Vec::new();
        for (i, (a, pa)) in points.iter().enumerate() {
            for (b, pb) in &points[i + 1..] {
                if pb.x - pa.x > distance {
                    break;
                }
                if pa.distance(*pb) <= distance {
                    pairs.push((*a, *b));
                }
            }
        }
        self.merge_vertices(&pairs)
    }

    /// Merges each pair of points into one (see the module documentation).
    /// Returns how many points were merged away.
    ///
    /// # Errors
    /// [`ModelError::NotFlat`] naming the faces a merge would bend (nothing
    /// changes), unless Auto-Tri is on; otherwise as [`Entities::add_line`].
    pub fn merge_vertices(&mut self, pairs: &[(VertexId, VertexId)]) -> Result<usize, ModelError> {
        let keep = self.merge_groups(pairs);
        if keep.is_empty() {
            return Ok(0);
        }
        let moved: Vec<VertexId> = keep.keys().copied().collect();
        let to = |v: VertexId| keep.get(&v).copied().unwrap_or(v);

        // Every face around a moved point, as it would be: check it first.
        let mut rebuilds: Vec<(FaceId, Option<Rebuild>)> = Vec::new();
        let mut bent = Vec::new();
        for f in self.faces_at(&moved) {
            let face = &self.faces[f];
            let outer = squeeze(
                self.loop_vertices(face.outer())
                    .into_iter()
                    .map(to)
                    .collect(),
            );
            let holes: Vec<Vec<VertexId>> = face
                .inner()
                .iter()
                .map(|h| squeeze(self.loop_vertices(h).into_iter().map(to).collect()))
                .filter(|h| h.len() >= 3)
                .collect();
            if outer.len() < 3 {
                // Collapsed to a line or a point: the face goes.
                rebuilds.push((f, None));
                continue;
            }
            let pts = |l: &[VertexId]| -> Vec<DVec3> {
                l.iter().map(|v| self.vertices[*v].position()).collect()
            };
            let outer_pts = pts(&outer);
            let all: Vec<DVec3> = std::iter::once(outer_pts.clone())
                .chain(holes.iter().map(|h| pts(h)))
                .flatten()
                .collect();
            let pinched = has_repeat(&outer) || holes.iter().any(|h| has_repeat(h));
            let fitted = Plane::fit_loop(&outer_pts)
                .ok()
                .filter(|p| p.max_deviation(&all) <= planar_tolerance(extent(&all)));
            match fitted {
                Some(mut plane) if !pinched => {
                    if plane.normal().dot(face.plane().normal()) < 0.0 {
                        plane = plane.flipped();
                    }
                    rebuilds.push((
                        f,
                        Some(Rebuild {
                            outer,
                            holes,
                            plane,
                            attributes: face.attributes.clone(),
                        }),
                    ));
                }
                // A face whose corners now lie in a line has no area left.
                None if !pinched && line_like(&outer_pts) => rebuilds.push((f, None)),
                _ => bent.push(f),
            }
        }
        if !bent.is_empty() {
            if crate::autotri::auto_tri() {
                // Triangles always stay flat: fold just these and try again.
                self.fold_faces(&bent)?;
                return self.merge_vertices(pairs);
            }
            return Err(ModelError::NotFlat(bent));
        }

        // Curves through a moved point no longer follow their curve.
        let mut curves: Vec<CurveId> = Vec::new();
        for v in &moved {
            for e in self.vertices[*v].edges() {
                if let Some(c) = self.edges[*e].curve()
                    && !curves.contains(&c)
                {
                    curves.push(c);
                }
            }
        }
        for c in curves {
            self.explode_curve(c)?;
        }

        // Take out the faces, then the edges at the moved points.
        for (f, _) in &rebuilds {
            self.remove_face(*f);
        }
        let mut edges: Vec<(VertexId, VertexId, EdgeFlags)> = Vec::new();
        let mut seen = HashSet::new();
        for v in &moved {
            for e in self.vertices[*v].edges().to_vec() {
                if seen.insert(e) {
                    let [a, b] = self.edges[e].vertices();
                    edges.push((to(a), to(b), self.edges[e].flags));
                }
            }
        }
        for e in seen {
            // Every face at a moved point was taken out above. The edge's
            // other end stays even if this was its last edge: it gets the
            // edge back below.
            let Some(edge) = self.edges.remove(e) else {
                continue;
            };
            if !edge.faces().is_empty() {
                return Err(ModelError::InvalidData("merge left a face behind".into()));
            }
            for v in edge.vertices() {
                self.vertices[v].edges_mut_remove(e);
            }
        }
        for v in &moved {
            if let Some(gone) = self.vertices.remove(*v) {
                self.grid.remove(*v, gone.position());
            }
        }

        // Put the edges back between the kept points (the first of any
        // that now coincide keeps its flags), then the faces.
        for (a, b, flags) in edges {
            if a != b && self.edge_between(a, b).is_none() {
                let e = self.insert_edge(a, b);
                self.edges[e].flags = flags;
            }
        }
        for (_, r) in rebuilds {
            if let Some(r) = r {
                self.put_face_back(r)?;
            }
        }
        // A kept point whose every edge had no length left is left alone.
        let lone: Vec<VertexId> = keep
            .values()
            .copied()
            .filter(|v| self.vertices.get(*v).is_some_and(|x| x.edges().is_empty()))
            .collect();
        for v in lone {
            if let Some(gone) = self.vertices.remove(v) {
                self.grid.remove(v, gone.position());
            }
        }
        Ok(moved.len())
    }

    /// The point each merged point joins, from `pairs`: groups by
    /// union–find, each keeping its point with the most edges (then the
    /// lowest key). Points that don't exist, and pairs of a point with
    /// itself, are skipped.
    fn merge_groups(&self, pairs: &[(VertexId, VertexId)]) -> HashMap<VertexId, VertexId> {
        let mut parent: HashMap<VertexId, VertexId> = HashMap::new();
        fn root(parent: &mut HashMap<VertexId, VertexId>, v: VertexId) -> VertexId {
            let mut r = v;
            while let Some(p) = parent.get(&r).copied().filter(|p| *p != r) {
                r = p;
            }
            // Path compression.
            let mut x = v;
            while let Some(p) = parent.get(&x).copied().filter(|p| *p != x) {
                parent.insert(x, r);
                x = p;
            }
            r
        }
        for (a, b) in pairs {
            if a == b || !self.vertices.contains_key(*a) || !self.vertices.contains_key(*b) {
                continue;
            }
            parent.entry(*a).or_insert(*a);
            parent.entry(*b).or_insert(*b);
            let (ra, rb) = (root(&mut parent, *a), root(&mut parent, *b));
            if ra != rb {
                parent.insert(ra.max(rb), ra.min(rb));
            }
        }
        let mut groups: HashMap<VertexId, Vec<VertexId>> = HashMap::new();
        let members: Vec<VertexId> = parent.keys().copied().collect();
        for v in members {
            let r = root(&mut parent, v);
            groups.entry(r).or_default().push(v);
        }
        let mut keep = HashMap::new();
        for members in groups.values() {
            let Some(kept) = members.iter().copied().max_by(|a, b| {
                let (ea, eb) = (
                    self.vertices[*a].edges().len(),
                    self.vertices[*b].edges().len(),
                );
                ea.cmp(&eb).then(b.cmp(a))
            }) else {
                continue;
            };
            for v in members {
                if *v != kept {
                    keep.insert(*v, kept);
                }
            }
        }
        keep
    }

    /// Builds a face taken out by a merge again, through its edges.
    fn put_face_back(&mut self, r: Rebuild) -> Result<FaceId, ModelError> {
        let mut outer = self.uses_of(&r.outer)?;
        if self.signed_area(&outer, &r.plane) < 0.0 {
            reverse_loop(&mut outer);
        }
        let f = self.insert_face(outer, Vec::new(), r.plane, r.attributes);
        for h in &r.holes {
            let hole = self.uses_of(h)?;
            self.add_inner_loop(f, hole);
        }
        Ok(f)
    }

    /// The edge uses walking `points` round as a closed loop.
    fn uses_of(&self, points: &[VertexId]) -> Result<Vec<EdgeUse>, ModelError> {
        let n = points.len();
        (0..n)
            .map(|i| {
                let (a, b) = (points[i], points[(i + 1) % n]);
                let edge = self
                    .edge_between(a, b)
                    .ok_or_else(|| ModelError::InvalidData("merge lost an edge".into()))?;
                Ok(EdgeUse {
                    edge,
                    reversed: self.edges[edge].vertices()[0] != a,
                })
            })
            .collect()
    }
}

/// A closed loop with runs of the same point made one (the ends included).
fn squeeze(mut l: Vec<VertexId>) -> Vec<VertexId> {
    l.dedup();
    while l.len() > 1 && l.first() == l.last() {
        l.pop();
    }
    l
}

/// Whether a loop passes through one point twice.
fn has_repeat(l: &[VertexId]) -> bool {
    let mut seen = HashSet::new();
    l.iter().any(|v| !seen.insert(*v))
}

/// Whether points all lie along one line (no area between them).
fn line_like(points: &[DVec3]) -> bool {
    let Some(&a) = points.first() else {
        return true;
    };
    let Some(&b) = points
        .iter()
        .max_by(|p, q| p.distance(a).total_cmp(&q.distance(a)))
    else {
        return true;
    };
    let d = b - a;
    let len = d.length();
    if len <= f64::EPSILON {
        return true;
    }
    let tol = planar_tolerance(extent(points));
    points.iter().all(|p| d.cross(*p - a).length() / len <= tol)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    #[test]
    fn two_close_points_become_one_and_the_face_stays() {
        let mut m = Entities::new();
        // A square whose corner has a second point 0.05 mm away on a spur.
        m.add_polyline(
            &[
                v(0.0, 0.0, 0.0),
                v(10.0, 0.0, 0.0),
                v(10.0, 10.0, 0.0),
                v(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        m.add_line(v(10.0, 10.0, 0.0), v(10.05, 10.0, 0.0)).unwrap();
        assert_eq!(m.face_count(), 1);
        let n = m.merge_by_distance(None, 0.1).unwrap();
        assert_eq!(n, 1);
        assert_eq!(m.vertex_count(), 4);
        assert_eq!(m.edge_count(), 4);
        assert_eq!(m.face_count(), 1);
        m.validate().unwrap();
    }

    #[test]
    fn a_short_edge_collapses_and_its_faces_are_rebuilt() {
        let mut m = Entities::new();
        // A face with a 0.01 mm notch edge along one side.
        m.add_polyline(
            &[
                v(0.0, 0.0, 0.0),
                v(10.0, 0.0, 0.0),
                v(10.0, 5.0, 0.0),
                v(10.0, 5.01, 0.0),
                v(10.0, 10.0, 0.0),
                v(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        assert_eq!(m.face_count(), 1);
        let a = m.find_vertex(v(10.0, 5.0, 0.0)).unwrap();
        let b = m.find_vertex(v(10.0, 5.01, 0.0)).unwrap();
        assert_eq!(m.merge_vertices(&[(a, b)]).unwrap(), 1);
        assert_eq!(m.face_count(), 1);
        assert_eq!(m.vertex_count(), 5);
        m.validate().unwrap();
    }

    #[test]
    fn a_merge_that_bends_a_face_is_refused_then_folds_with_auto_tri() {
        let mut m = Entities::new();
        m.add_polyline(
            &[
                v(0.0, 0.0, 0.0),
                v(10.0, 0.0, 0.0),
                v(10.0, 10.0, 0.0),
                v(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        // A loose point just above one corner.
        m.add_line(v(10.0, 10.0, 0.05), v(20.0, 20.0, 5.0)).unwrap();
        let corner = m.find_vertex(v(10.0, 10.0, 0.0)).unwrap();
        let above = m.find_vertex(v(10.0, 10.0, 0.05)).unwrap();
        // The corner has more edges, so it would stay: no bend.
        assert_eq!(m.merge_vertices(&[(corner, above)]).unwrap(), 1);
        m.validate().unwrap();

        // Now force a moved corner: a square with a point off its plane
        // that has more edges than the corner.
        let mut m = Entities::new();
        m.add_polyline(
            &[
                v(0.0, 0.0, 0.0),
                v(10.0, 0.0, 0.0),
                v(10.0, 10.0, 0.0),
                v(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        let hub = v(10.0, 10.0, 0.05);
        for end in [v(30.0, 10.0, 5.0), v(10.0, 30.0, 5.0), v(30.0, 30.0, 5.0)] {
            m.add_line(hub, end).unwrap();
        }
        let corner = m.find_vertex(v(10.0, 10.0, 0.0)).unwrap();
        let hub = m.find_vertex(hub).unwrap();
        let before = m.clone();
        let err = m.merge_vertices(&[(corner, hub)]).unwrap_err();
        assert!(matches!(err, ModelError::NotFlat(_)));
        assert!(m == before, "a refused merge changes nothing");
        let n = crate::with_auto_tri(|| m.merge_vertices(&[(corner, hub)])).unwrap();
        assert_eq!(n, 1);
        assert_eq!(m.face_count(), 2);
        m.validate().unwrap();
    }

    #[test]
    fn a_face_squeezed_to_a_point_goes() {
        let mut m = Entities::new();
        m.add_polyline(
            &[v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(5.0, 0.05, 0.0)],
            true,
        )
        .unwrap();
        assert_eq!(m.face_count(), 1);
        m.merge_by_distance(None, 5.1).unwrap();
        assert_eq!(m.face_count(), 0);
        m.validate().unwrap();
    }
}
