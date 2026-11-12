//! SketchSmith — Fuse (booleans) | v1.0.0 | Thursday 12 November 2026
//!
//! Fuse combines a base with a target: Union, Difference (the target cut
//! out of the base) or Intersect (only what both hold).
//!
//! **Closed groups** ([`Document::fuse_groups`]): the target's faces are
//! laid into the base's definition where they really are, keeping their
//! paint and layers ([`Entities::lay_in`]). Intersect Faces then cuts every
//! place the two surfaces cross. Each piece is judged against the *other*
//! solid's surface as it was: inside, outside (a ray from the piece
//! counts the surface's crossings), or lying on it, facing the same way or
//! not. The pieces each operation keeps:
//!
//! | | base pieces | target pieces | pieces on both, same way | opposite |
//! |---|---|---|---|---|
//! | Union | outside the target | outside the base | one copy | none |
//! | Difference | outside the target | inside the base, turned | none | the base's |
//! | Intersect | inside the target | inside the base | one copy | none |
//!
//! The rest go, with the edges left without faces, and new edges between
//! two kept pieces on one plane are joined away. The target group goes; a
//! component used as the base is made unique first. Refusals change
//! nothing ([`ModelError::Refused`], with the reason as shown on screen).
//!
//! **Faces on one plane** ([`Entities::fuse_faces`]): the flat rebuild of
//! Intersect Faces, keeping the places the operation keeps, as faces with
//! the base's layer and paint.

use std::collections::{BTreeMap, BTreeSet};

use smith_geom::tolerance::TOL_MERGE;
use smith_geom::triangulate::triangulate_polygon;
use smith_geom::{DVec2, DVec3};

use crate::document::Document;
use crate::entities::{EdgeUse, Entities, FaceAttributes};
use crate::error::ModelError;
use crate::faces::same_plane;
use crate::ids::{DefinitionId, EdgeId, FaceId, InstanceId, VertexId};
use crate::intersect::{Against, FaceShape, Keep, carried};

/// What Fuse makes of a base and a target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BooleanOp {
    /// Both together.
    #[default]
    Union,
    /// The target cut out of the base.
    Difference,
    /// Only what both hold.
    Intersect,
}

impl BooleanOp {
    /// The operations, in drop-down order.
    pub const ALL: [BooleanOp; 3] = [Self::Union, Self::Difference, Self::Intersect];
    /// Their names, in [`BooleanOp::ALL`] order.
    pub const NAMES: [&'static str; 3] = ["Union", "Difference", "Intersect"];
}

/// Refused: nothing would be left.
pub const NOTHING_LEFT: &str = "Nothing Would Be Left";

/// A solid's surface as it was: its triangles, each with its face's normal.
struct Surface(Vec<([DVec3; 3], DVec3)>);

impl Surface {
    fn of_faces(e: &Entities, faces: &[FaceId]) -> Self {
        Self(
            faces
                .iter()
                .filter_map(|f| e.face(*f).map(|x| (*f, x.plane().normal())))
                .flat_map(|(f, n)| e.face_triangles(f).into_iter().map(move |t| (t, n)))
                .collect(),
        )
    }

    fn of_shapes(shapes: &[(FaceShape, FaceAttributes)]) -> Self {
        let mut tris = Vec::new();
        for (s, _) in shapes {
            let plane = s.plane;
            let local: Vec<Vec<DVec2>> = s
                .loops
                .iter()
                .map(|l| l.iter().map(|p| plane.to_local(*p)).collect())
                .collect();
            let all: Vec<DVec3> = s.loops.iter().flatten().copied().collect();
            for [a, b, c] in triangulate_polygon(&local[0], &local[1..]) {
                tris.push(([all[a], all[b], all[c]], plane.normal()));
            }
        }
        Self(tris)
    }

    /// The surface's normal where `p` lies on it, if it does.
    fn on(&self, p: DVec3) -> Option<DVec3> {
        self.0
            .iter()
            .find(|(t, _)| closest_on_triangle(p, *t).distance(p) <= TOL_MERGE)
            .map(|(_, n)| *n)
    }

    /// Whether `p` (off the surface) is inside it: the crossings of a ray
    /// in an unlikely direction, counted.
    fn holds(&self, p: DVec3) -> bool {
        let dir = DVec3::new(0.5773, 0.5779, 0.5768).normalize();
        self.0.iter().filter(|(t, _)| ray_hits(p, dir, *t)).count() % 2 == 1
    }
}

/// The point of triangle `t` closest to `p` (Ericson, *Real-Time Collision
/// Detection*, §5.1.5).
fn closest_on_triangle(p: DVec3, [a, b, c]: [DVec3; 3]) -> DVec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// Whether a ray from `o` along `d` crosses triangle `t` ahead of `o`
/// (Möller–Trumbore).
fn ray_hits(o: DVec3, d: DVec3, [a, b, c]: [DVec3; 3]) -> bool {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return false;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    e2.dot(q) * inv > TOL_MERGE
}

impl Entities {
    /// Lays `shapes` in as faces, without the sticky rules: points merge
    /// with points already there and edges between the same points are
    /// shared, but nothing is split. Each face takes its shape's
    /// attributes. Returns the faces made.
    pub fn lay_in(&mut self, shapes: &[(FaceShape, FaceAttributes)]) -> Vec<FaceId> {
        let mut made = Vec::new();
        for (shape, attributes) in shapes {
            let mut loops: Vec<Vec<EdgeUse>> = Vec::new();
            for l in &shape.loops {
                let ids: Vec<VertexId> = l.iter().map(|p| self.merge_vertex(*p)).collect();
                let mut uses = Vec::new();
                for i in 0..ids.len() {
                    let (a, b) = (ids[i], ids[(i + 1) % ids.len()]);
                    if a == b {
                        continue;
                    }
                    let edge = self
                        .edge_between(a, b)
                        .unwrap_or_else(|| self.insert_edge(a, b));
                    uses.push(EdgeUse {
                        edge,
                        reversed: self.edges[edge].vertices()[0] != a,
                    });
                }
                if uses.len() >= 3 {
                    loops.push(uses);
                }
            }
            let mut loops = loops.into_iter();
            let Some(outer) = loops.next() else {
                continue;
            };
            if let Some(f) = self.create_face(outer, Some(shape.plane.normal()), attributes.clone())
            {
                for hole in loops {
                    self.add_inner_loop(f, hole);
                }
                made.push(f);
            }
        }
        made
    }

    /// Fuse for a closed solid (every face here) and a target solid
    /// (`target`, already in this context's coordinates); see the module
    /// documentation.
    ///
    /// # Errors
    /// [`ModelError::Refused`] with [`NOTHING_LEFT`]; otherwise as
    /// [`Entities::intersect_faces`].
    pub fn fuse_solid(
        &mut self,
        target: &[(FaceShape, FaceAttributes)],
        op: BooleanOp,
    ) -> Result<(), ModelError> {
        let base_faces: Vec<FaceId> = self.face_ids().collect();
        let base = Surface::of_faces(self, &base_faces);
        let other = Surface::of_shapes(target);
        // Where each solid's own edges ran. An edge between two kept pieces
        // on one plane is a seam, joined away, unless it was an edge of just
        // one of them (a split drawn on purpose).
        let base_edges: Vec<(DVec3, DVec3)> = self
            .edge_ids()
            .filter_map(|e| self.edge_points(e))
            .collect();
        let mut target_edges: Vec<(DVec3, DVec3)> = Vec::new();
        for (s, _) in target {
            for l in &s.loops {
                for i in 0..l.len() {
                    target_edges.push((l[i], l[(i + 1) % l.len()]));
                }
            }
        }
        self.lay_in(target);
        let all: Vec<FaceId> = self.face_ids().collect();
        self.intersect_faces(&all, &Against::default())?;

        let mut keep: Vec<FaceId> = Vec::new();
        let mut turn: Vec<FaceId> = Vec::new();
        let mut copies: BTreeMap<Vec<VertexId>, FaceId> = BTreeMap::new();
        for f in self.face_ids().collect::<Vec<_>>() {
            let Some(face) = self.face(f) else {
                continue;
            };
            let n = face.plane().normal();
            let Some(p) = self.inside_sample(f) else {
                continue;
            };
            let on_base = base.on(p);
            let on_target = other.on(p);
            let kept = match (on_base, on_target) {
                (Some(_), None) => {
                    let inside = other.holds(p);
                    match op {
                        BooleanOp::Union | BooleanOp::Difference => !inside,
                        BooleanOp::Intersect => inside,
                    }
                }
                (None, Some(_)) => {
                    let inside = base.holds(p);
                    match op {
                        BooleanOp::Union => !inside,
                        BooleanOp::Difference => {
                            if inside {
                                turn.push(f);
                            }
                            inside
                        }
                        BooleanOp::Intersect => inside,
                    }
                }
                (Some(nb), Some(nt)) => {
                    let same = nb.dot(nt) > 0.0;
                    let wanted = match op {
                        BooleanOp::Union | BooleanOp::Intersect => same,
                        BooleanOp::Difference => !same,
                    };
                    // One copy, the one facing the base's way.
                    let mut key = self.loop_vertices(face.outer());
                    key.sort();
                    wanted && n.dot(nb) > 0.0 && copies.insert(key, f).is_none()
                }
                (None, None) => false,
            };
            if kept {
                keep.push(f);
            }
        }
        if keep.is_empty() {
            return Err(ModelError::Refused(NOTHING_LEFT.into()));
        }
        let kept: BTreeSet<FaceId> = keep.iter().copied().collect();
        for f in self.face_ids().collect::<Vec<_>>() {
            if !kept.contains(&f) {
                self.remove_face(f);
            }
        }
        for f in turn {
            self.reverse_face(f)?;
        }
        for e in self.edge_ids().collect::<Vec<_>>() {
            if self.edges.get(e).is_some_and(|x| x.faces().is_empty()) {
                self.remove_bare_edge(e);
            }
        }
        // New edges lying between two kept pieces on one plane are seams,
        // not edges of the result.
        loop {
            let seams: Vec<EdgeId> = self
                .edge_ids()
                .filter(|e| {
                    self.on_segments(*e, &base_edges) == self.on_segments(*e, &target_edges)
                })
                .filter(|e| {
                    self.edges[*e].faces().len() == 2 && {
                        let [a, b] = self.edges[*e].faces()[..] else {
                            return false;
                        };
                        same_plane(self.faces[a].plane(), self.faces[b].plane())
                    }
                })
                .collect();
            let mut joined = false;
            for e in seams {
                if self.edges.contains_key(e) && self.merge_faces_across(e)? {
                    joined = true;
                }
            }
            if !joined {
                break;
            }
        }
        Ok(())
    }

    /// Whether edge `e`'s middle lies on one of `segments`.
    fn on_segments(&self, e: EdgeId, segments: &[(DVec3, DVec3)]) -> bool {
        self.edge_points(e).is_some_and(|(a, b)| {
            let mid = (a + b) * 0.5;
            segments.iter().any(|(p, q)| {
                smith_geom::intersect::closest_point_on_segment(mid, *p, *q)
                    .1
                    .distance(mid)
                    <= TOL_MERGE
            })
        })
    }

    /// A point well inside face `f`: the middle of its largest triangle.
    fn inside_sample(&self, f: FaceId) -> Option<DVec3> {
        self.face_triangles(f)
            .into_iter()
            .max_by(|x, y| {
                let area = |[a, b, c]: [DVec3; 3]| (b - a).cross(c - a).length();
                area(*x).total_cmp(&area(*y))
            })
            .map(|[a, b, c]| (a + b + c) / 3.0)
    }

    /// Fuse for two faces on one plane (see the module documentation).
    ///
    /// # Errors
    /// [`ModelError::Refused`] when they are not on one plane, when nothing
    /// would be left, or when the pieces cannot be traced.
    pub fn fuse_faces(
        &mut self,
        base: FaceId,
        target: FaceId,
        op: BooleanOp,
    ) -> Result<(), ModelError> {
        let (Some(a), Some(b)) = (self.face(base), self.face(target)) else {
            return Err(ModelError::NoSuchEntity);
        };
        if !same_plane(a.plane(), b.plane()) {
            return Err(ModelError::Refused("The Faces Are Not on One Plane".into()));
        }
        let keep = match op {
            BooleanOp::Union => Keep::Either,
            BooleanOp::Difference => Keep::BaseOnly,
            BooleanOp::Intersect => Keep::Both,
        };
        match self.fuse_two_faces(base, target, keep) {
            None => Err(ModelError::Refused("Couldn't Fuse These Cleanly".into())),
            Some(made) if made.is_empty() => Err(ModelError::Refused(NOTHING_LEFT.into())),
            Some(_) => Ok(()),
        }
    }
}

impl Document {
    /// Fuse for two closed groups or components placed in context `ctx`:
    /// the base becomes the result, the target goes (see the module
    /// documentation).
    ///
    /// # Errors
    /// [`ModelError::Refused`] with the reason as shown on screen: a locked
    /// group, groups inside, a group that is not closed, or nothing left;
    /// otherwise as [`Entities::fuse_solid`].
    pub fn fuse_groups(
        &mut self,
        ctx: Option<DefinitionId>,
        base: InstanceId,
        target: InstanceId,
        op: BooleanOp,
    ) -> Result<(), ModelError> {
        let here = self.context(ctx).ok_or(ModelError::NoSuchEntity)?;
        let (Some(b), Some(t)) = (here.instance(base), here.instance(target)) else {
            return Err(ModelError::NoSuchEntity);
        };
        if base == target {
            return Err(ModelError::NoSuchEntity);
        }
        if b.locked || t.locked {
            return Err(ModelError::Refused("Unlock the Groups First".into()));
        }
        let (bd, td) = (b.definition, t.definition);
        let to_base = b.transform.inverse() * t.transform;
        for (def, which) in [(bd, "Base"), (td, "Target")] {
            let e = self.context(Some(def)).ok_or(ModelError::NoSuchEntity)?;
            if e.instance_ids().next().is_some() {
                return Err(ModelError::Refused(
                    "Explode the Groups Inside First".into(),
                ));
            }
            let closed = e.face_count() > 0
                && e.edge_ids()
                    .all(|x| e.edge(x).is_some_and(|edge| edge.faces().len() == 2));
            if !closed {
                return Err(ModelError::Refused(format!(
                    "The {which} Is Not Closed: Use Make Manifold"
                )));
            }
        }
        let shapes: Vec<(FaceShape, FaceAttributes)> = {
            let e = self.context(Some(td)).ok_or(ModelError::NoSuchEntity)?;
            e.face_ids()
                .filter_map(|f| e.face(f))
                .filter_map(|f| carried(e, f, &to_base).map(|s| (s, f.attributes.clone())))
                .collect()
        };
        let def = if self.instance_users(bd) > 1 {
            self.make_unique(ctx, base)?
        } else {
            bd
        };
        self.definition_entities_mut(def)
            .ok_or(ModelError::NoSuchEntity)?
            .fuse_solid(&shapes, op)?;
        self.context_entities_mut(ctx)
            .ok_or(ModelError::NoSuchEntity)?
            .erase_instance(target)?;
        Ok(())
    }
}
