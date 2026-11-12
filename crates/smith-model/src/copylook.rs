//! SketchSmith — what a copy keeps | v1.0.0 | Thursday 12 November 2026
//!
//! Copies of loose geometry are drawn afresh (new lines, faces filled as when
//! drawn), so nothing on the originals comes across by itself.
//! [`Entities::capture_look`] notes, before copying, what the chosen edges and
//! the faces they enclose carry: layer, the edge flags (soft, smooth,
//! hidden), edge marks and line styles, and paint. [`Entities::apply_look`] then finds each copied edge by its
//! carried end points and each copied face by a point well inside it, and
//! gives them the originals' values. Paint goes with the copy as it goes with
//! a move: face frames are carried, and each wrap gets a carried copy of its
//! own, so the copy can be repositioned without moving the original.
//!
//! [`Entities::place_copy`] copies a whole context into another as it is,
//! for an Explode clear of other geometry: nothing is redrawn, so large
//! meshes go across quickly and keep their holes, flags and paint.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use smith_geom::tolerance::TOL_MERGE;
use smith_geom::{DMat3, DMat4, DVec3};

use crate::entities::{EdgeFlags, Entities, FaceAttributes, point_in_polygon};
use crate::ids::{EdgeId, FaceId, VertexId, WrapId};
use crate::paint::{Mapping, Side, Wrap};

/// A face to be copied: a point well inside it, its normal and what it
/// carries.
#[derive(Debug, Clone)]
struct FaceLook {
    inside: DVec3,
    normal: DVec3,
    attributes: FaceAttributes,
}

/// What a set of edges and the faces they enclose carry, noted before they
/// are copied.
#[derive(Debug, Clone, Default)]
pub struct CopiedLook {
    edges: Vec<(DVec3, DVec3, EdgeFlags)>,
    faces: Vec<FaceLook>,
    wraps: BTreeMap<WrapId, Wrap>,
    /// Faces with keys up to this one existed before copying.
    last_face: u64,
}

impl Entities {
    /// Notes what `edges`, and every face whose loops use only them, carry.
    pub fn capture_look(&self, edges: &[EdgeId]) -> CopiedLook {
        let chosen: BTreeSet<EdgeId> = edges.iter().copied().collect();
        let mut look = CopiedLook {
            last_face: self.faces.keys().last().map_or(0, FaceId::raw),
            ..CopiedLook::default()
        };
        for e in &chosen {
            if let (Some(edge), Some((a, b))) = (self.edges.get(*e), self.edge_points(*e)) {
                look.edges.push((a, b, edge.flags));
            }
        }
        let mut faces: BTreeSet<FaceId> = BTreeSet::new();
        for e in &chosen {
            if let Some(edge) = self.edges.get(*e) {
                faces.extend(edge.faces().iter().copied());
            }
        }
        for f in faces {
            let Some(face) = self.faces.get(f) else {
                continue;
            };
            if !face.loops().flatten().all(|u| chosen.contains(&u.edge)) {
                continue;
            }
            let Some(inside) = self.point_inside(f) else {
                continue;
            };
            for side in [Side::Front, Side::Back] {
                if let Some(p) = face.attributes.paint(side)
                    && let Mapping::Wrap(w) = p.mapping
                    && let Some(wrap) = self.wraps.get(w)
                {
                    look.wraps.insert(w, *wrap);
                }
            }
            look.faces.push(FaceLook {
                inside,
                normal: face.plane().normal(),
                attributes: face.attributes.clone(),
            });
        }
        look
    }

    /// Gives the copy of what `look` noted, carried by `m`, the originals'
    /// values. Edges and faces that cannot be found (the copy merged into
    /// other geometry there) are left as drawn.
    pub fn apply_look(&mut self, look: &CopiedLook, m: &DMat4) {
        for (a, b, flags) in &look.edges {
            let (Some(va), Some(vb)) = (
                self.find_vertex(m.transform_point3(*a)),
                self.find_vertex(m.transform_point3(*b)),
            ) else {
                continue;
            };
            if let Some(e) = self.edge_between(va, vb)
                && self.edges[e].flags != *flags
            {
                self.edges[e].flags = *flags;
            }
        }
        let mut wraps: BTreeMap<WrapId, WrapId> = BTreeMap::new();
        for (old, wrap) in &look.wraps {
            wraps.insert(*old, self.wraps.insert(wrap.transformed(m)));
        }
        for face in &look.faces {
            let p = m.transform_point3(face.inside);
            let n = m.transform_vector3(face.normal).normalize_or(face.normal);
            let Some(target) = self.new_face_at(p, n, look.last_face) else {
                continue;
            };
            let mut attributes = face.attributes.clone();
            for side in [Side::Front, Side::Back] {
                if let Some(paint) = attributes.paint_mut(side) {
                    match &mut paint.mapping {
                        Mapping::Face(frame) => *frame = frame.transformed(m),
                        Mapping::Wrap(w) => {
                            if let Some(copy) = wraps.get(w) {
                                *w = *copy;
                            }
                        }
                    }
                }
            }
            // Drawn afresh, the copy may face the other way.
            if self.faces[target].plane().normal().dot(n) < 0.0 {
                std::mem::swap(&mut attributes.front, &mut attributes.back);
            }
            if self.faces[target].attributes != attributes {
                self.faces[target].attributes = attributes;
            }
        }
        // A wrap whose copied faces were not found is not kept.
        for copy in wraps.values() {
            if self.wrap_users(*copy).is_empty() {
                self.wraps.remove(*copy);
            }
        }
    }

    /// Copies every edge, face and curve of `src` into `self`, carried by
    /// `m`, as they are: faces keep their holes, front, layer, hidden flag
    /// and paint (frames carried, each wrap copied); edges keep their flags,
    /// marks and styles; curves stay curves (a transform that cannot carry
    /// one leaves its edges plain). Corners merge with vertices already at
    /// the same point, so pieces that share corners share edges; nothing is
    /// cut where it crosses other geometry, so this is for geometry clear
    /// of what is already here. A face repeating one here is skipped.
    pub fn place_copy(&mut self, src: &Entities, m: &DMat4) {
        let normals = DMat3::from_mat4(*m).inverse().transpose();
        let at = |p: DVec3| m.transform_point3(p);
        let mut vmap: HashMap<VertexId, VertexId> = HashMap::new();
        for (v, vertex) in src.vertices.iter() {
            vmap.insert(v, self.merge_vertex(at(vertex.position())));
        }
        let mut wraps: BTreeMap<WrapId, WrapId> = BTreeMap::new();
        for (w, wrap) in src.wraps.iter() {
            wraps.insert(w, self.wraps.insert(wrap.transformed(m)));
        }
        let ids = |l: &[crate::entities::EdgeUse]| -> Vec<VertexId> {
            src.loop_vertices(l).iter().map(|v| vmap[v]).collect()
        };
        for (_, face) in src.faces.iter() {
            let outer = ids(face.outer());
            if outer.len() < 3 || self.face_with_corners_any(&outer) {
                continue;
            }
            let mut attributes = face.attributes.clone();
            for side in [Side::Front, Side::Back] {
                if let Some(paint) = attributes.paint_mut(side) {
                    match &mut paint.mapping {
                        Mapping::Face(frame) => *frame = frame.transformed(m),
                        Mapping::Wrap(w) => {
                            if let Some(copy) = wraps.get(w) {
                                *w = *copy;
                            }
                        }
                    }
                }
            }
            let normal = (normals * face.plane().normal()).normalize_or(DVec3::Z);
            let Some(l) = self.loop_uses(&outer) else {
                continue;
            };
            let Some(f) = self.create_face(l, Some(normal), attributes) else {
                continue;
            };
            for hole in face.inner() {
                let h = ids(hole);
                if h.len() >= 3
                    && let Some(l) = self.loop_uses(&h)
                {
                    self.add_inner_loop(f, l);
                }
            }
        }
        for (_, edge) in src.edges.iter() {
            let [a, b] = edge.vertices();
            let (va, vb) = (vmap[&a], vmap[&b]);
            if va == vb {
                continue;
            }
            let e = match self.edge_between(va, vb) {
                Some(e) => e,
                None => self.insert_edge(va, vb),
            };
            self.edges[e].flags = edge.flags;
        }
        for (_, record) in src.curves.iter() {
            let Some(curve) = record.curve().transformed(m) else {
                continue;
            };
            let segments: Vec<[DVec3; 2]> = record
                .edges()
                .iter()
                .filter_map(|e| src.edge_points(*e))
                .map(|(a, b)| [at(a), at(b)])
                .collect();
            self.import_curve(curve, record.segments(), &segments);
        }
        // A wrap none of the copied faces use is not kept.
        for copy in wraps.values() {
            if self.wrap_users(*copy).is_empty() {
                self.wraps.remove(*copy);
            }
        }
    }

    /// The centre of face `f`'s largest triangle: a point well inside it.
    pub(crate) fn point_inside(&self, f: FaceId) -> Option<DVec3> {
        let area = |t: &[DVec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]).length();
        self.face_triangles(f)
            .into_iter()
            .max_by(|a, b| area(a).total_cmp(&area(b)))
            .map(|t| (t[0] + t[1] + t[2]) / 3.0)
    }

    /// A face made after face key `last` whose plane holds `p`, square to
    /// `n` either way, with `p` inside it (not in a hole).
    fn new_face_at(&self, p: DVec3, n: DVec3, last: u64) -> Option<FaceId> {
        self.faces
            .iter()
            .filter(|(f, _)| f.raw() > last)
            .find(|(f, face)| {
                let plane = face.plane();
                plane.normal().dot(n).abs() > 1.0 - 1e-9
                    && plane.signed_distance(p).abs() <= TOL_MERGE * 10.0
                    && self.outer_contains(*f, p)
                    && !face
                        .inner()
                        .iter()
                        .any(|l| point_in_polygon(plane.to_local(p), &self.loop_local(l, plane)))
            })
            .map(|(f, _)| f)
    }
}
