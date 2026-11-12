//! SketchSmith — entities container | v1.0.0 | Thursday 12 November 2026
//!
//! Entity types, read-only queries, and the low-level mutation primitives that
//! keep back-references consistent. Higher-level sticky operations live in
//! `build.rs` (adding lines), `faces.rs` (finding and creating faces) and
//! `erase.rs` (erasing and healing).
//!
//! Invariants maintained by every public operation (checked by
//! [`Entities::validate`]):
//! * every edge's two vertices exist, are distinct, and list the edge;
//! * at most one edge joins any pair of vertices;
//! * no two vertices lie within `TOL_MERGE` of each other;
//! * every face loop is closed and connected, and every edge in a face loop
//!   lists that face in its radial list (and only faces that use it).

use smallvec::SmallVec;
use smith_geom::plane::Plane;
use smith_geom::tolerance::TOL_MERGE;
use smith_geom::{DVec2, DVec3};

use crate::curves::CurveRecord;
use crate::document::Instance;

use crate::annotations::Annotation;
use crate::arena::Arena;
use crate::error::ModelError;
use crate::grid::VertexGrid;
use crate::guides::Guide;
use crate::ids::{
    AnnotationId, CurveId, EdgeId, FaceId, GuideId, InstanceId, LayerId, SectionId, VertexId,
    WrapId,
};
use crate::look::{EdgeMark, LineStyle};
use crate::paint::{Paint, Side, Wrap};
use crate::sections::SectionPlane;

/// A point in the model, with the edges that meet at it.
#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    position: DVec3,
    edges: SmallVec<[EdgeId; 4]>,
}

impl Vertex {
    /// A vertex with its edge list, as read from a file (`snapshot.rs`).
    pub(crate) fn from_parts(position: DVec3, edges: SmallVec<[EdgeId; 4]>) -> Self {
        Self { position, edges }
    }

    /// Position in mm.
    pub fn position(&self) -> DVec3 {
        self.position
    }

    /// Edges meeting at this vertex.
    pub fn edges(&self) -> &[EdgeId] {
        &self.edges
    }

    pub(crate) fn set_position(&mut self, p: DVec3) {
        self.position = p;
    }

    pub(crate) fn edges_mut_remove(&mut self, e: EdgeId) {
        self.edges.retain(|x| *x != e);
    }

    pub(crate) fn edges_mut_push(&mut self, e: EdgeId) {
        if !self.edges.contains(&e) {
            self.edges.push(e);
        }
    }
}

/// Display flags, Present-mode look and the layer of an edge (kept by both
/// pieces when the edge splits).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EdgeFlags {
    /// Soft: not drawn, and transparent to selection between faces.
    pub soft: bool,
    /// Smooth: adjacent faces are shaded as one smooth surface.
    pub smooth: bool,
    /// Hidden by the user.
    pub hidden: bool,
    /// The layer the edge is on; `None` is the Base layer.
    pub layer: Option<LayerId>,
    /// How it takes part in Present-mode smoothing.
    pub mark: EdgeMark,
    /// Its own Present-mode line style (`None`: its layer's, if any).
    pub style: Option<LineStyle>,
}

impl EdgeFlags {
    /// The flags with the Present-mode look (mark and style) cleared: what
    /// counts when telling a geometry edit from a look change.
    pub(crate) fn without_look(self) -> Self {
        Self {
            mark: EdgeMark::Auto,
            style: None,
            ..self
        }
    }
}

/// A straight edge between two vertices, with its radial face list.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    vertices: [VertexId; 2],
    faces: SmallVec<[FaceId; 2]>,
    /// The curve this edge belongs to, if any.
    pub(crate) curve: Option<CurveId>,
    /// Display flags.
    pub flags: EdgeFlags,
}

impl Edge {
    /// Whether `self` and `other` differ only in their Present-mode look.
    pub(crate) fn same_but_look(&self, other: &Self) -> bool {
        self.vertices == other.vertices
            && self.faces == other.faces
            && self.curve == other.curve
            && self.flags.without_look() == other.flags.without_look()
    }

    /// An edge as read from a file (`snapshot.rs`).
    pub(crate) fn from_parts(
        vertices: [VertexId; 2],
        faces: SmallVec<[FaceId; 2]>,
        curve: Option<CurveId>,
        flags: EdgeFlags,
    ) -> Self {
        Self {
            vertices,
            faces,
            curve,
            flags,
        }
    }

    /// The two end vertices, `[start, end]`.
    pub fn vertices(&self) -> [VertexId; 2] {
        self.vertices
    }

    /// Every face that uses this edge (the radial list). May be empty.
    pub fn faces(&self) -> &[FaceId] {
        &self.faces
    }

    /// The faces using this edge, to change (the faces' loops must agree).
    pub(crate) fn faces_mut(&mut self) -> &mut SmallVec<[FaceId; 2]> {
        &mut self.faces
    }

    /// The curve this edge belongs to, if any.
    pub fn curve(&self) -> Option<CurveId> {
        self.curve
    }

    /// The vertex at the other end from `v`.
    pub fn other(&self, v: VertexId) -> VertexId {
        if self.vertices[0] == v {
            self.vertices[1]
        } else {
            self.vertices[0]
        }
    }
}

/// One edge within a loop, and the direction it is walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeUse {
    /// The edge.
    pub edge: EdgeId,
    /// `false`: walked start → end. `true`: walked end → start.
    pub reversed: bool,
}

/// Attributes a face carries that must survive splitting and healing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaceAttributes {
    /// The layer the face is on; `None` is the Base layer.
    pub layer: Option<LayerId>,
    /// Hidden by the user.
    pub hidden: bool,
    /// Paint on the front side.
    pub front: Option<Paint>,
    /// Paint on the back side.
    pub back: Option<Paint>,
}

impl FaceAttributes {
    /// The paint on `side`.
    pub fn paint(&self, side: Side) -> Option<&Paint> {
        match side {
            Side::Front => self.front.as_ref(),
            Side::Back => self.back.as_ref(),
        }
    }

    /// The paint slot for `side`.
    pub(crate) fn paint_mut(&mut self, side: Side) -> &mut Option<Paint> {
        match side {
            Side::Front => &mut self.front,
            Side::Back => &mut self.back,
        }
    }
}

/// A planar face: one outer loop, counter-clockwise about the normal, and any
/// number of inner loops (holes), clockwise about the normal.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub(crate) outer: Vec<EdgeUse>,
    pub(crate) inner: Vec<Vec<EdgeUse>>,
    pub(crate) plane: Plane,
    /// Inherited attributes.
    pub attributes: FaceAttributes,
}

impl Face {
    /// Whether `self` and `other` differ only in paint: the test that tells
    /// painting from editing geometry by hand.
    pub(crate) fn same_but_paint(&self, other: &Self) -> bool {
        self.outer == other.outer
            && self.inner == other.inner
            && self.plane == other.plane
            && self.attributes.layer == other.attributes.layer
            && self.attributes.hidden == other.attributes.hidden
    }

    /// A face as read from a file (`snapshot.rs`).
    pub(crate) fn from_parts(
        outer: Vec<EdgeUse>,
        inner: Vec<Vec<EdgeUse>>,
        plane: Plane,
        attributes: FaceAttributes,
    ) -> Self {
        Self {
            outer,
            inner,
            plane,
            attributes,
        }
    }

    /// The outer loop.
    pub fn outer(&self) -> &[EdgeUse] {
        &self.outer
    }

    /// The inner loops (holes).
    pub fn inner(&self) -> &[Vec<EdgeUse>] {
        &self.inner
    }

    /// The face plane; its normal is the face's front direction.
    pub fn plane(&self) -> &Plane {
        &self.plane
    }

    /// Every loop: outer first, then inner.
    pub fn loops(&self) -> impl Iterator<Item = &[EdgeUse]> {
        std::iter::once(self.outer.as_slice()).chain(self.inner.iter().map(Vec::as_slice))
    }
}

/// All geometry of one editing context.
#[derive(Debug, Clone, Default)]
pub struct Entities {
    pub(crate) vertices: Arena<VertexId, Vertex>,
    pub(crate) edges: Arena<EdgeId, Edge>,
    pub(crate) faces: Arena<FaceId, Face>,
    pub(crate) curves: Arena<CurveId, CurveRecord>,
    pub(crate) instances: Arena<InstanceId, Instance>,
    pub(crate) guides: Arena<GuideId, Guide>,
    pub(crate) annotations: Arena<AnnotationId, Annotation>,
    pub(crate) sections: Arena<SectionId, SectionPlane>,
    /// The projections Wrap painted faces in this context with.
    pub(crate) wraps: Arena<WrapId, Wrap>,
    /// Derived index over vertex positions; rebuilt incrementally, never journaled.
    pub(crate) grid: VertexGrid,
    /// The layer new edges and new faces go on (the document's active
    /// layer; `None` is Base). Pieces of a split edge or face keep the
    /// original's layer. A setting, not geometry: never journaled or compared.
    pub(crate) draw_layer: Option<LayerId>,
}

impl PartialEq for Entities {
    /// Structural equality of geometry (the undo test). The merge grid
    /// is derived data and is not compared.
    fn eq(&self, other: &Self) -> bool {
        self.vertices == other.vertices
            && self.edges == other.edges
            && self.faces == other.faces
            && self.curves == other.curves
            && self.instances == other.instances
            && self.guides == other.guides
            && self.annotations == other.annotations
            && self.sections == other.sections
            && self.wraps == other.wraps
    }
}

impl Entities {
    /// An empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the layer new edges and faces go on (`None`: Base).
    pub fn set_draw_layer(&mut self, layer: Option<LayerId>) {
        self.draw_layer = layer;
    }

    // ----- queries ---------------------------------------------------------

    /// Number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Number of edges.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Number of faces.
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// A vertex by key.
    pub fn vertex(&self, id: VertexId) -> Option<&Vertex> {
        self.vertices.get(id)
    }

    /// An edge by key.
    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(id)
    }

    /// A face by key.
    pub fn face(&self, id: FaceId) -> Option<&Face> {
        self.faces.get(id)
    }

    /// All vertex keys.
    pub fn vertex_ids(&self) -> impl Iterator<Item = VertexId> + '_ {
        self.vertices.keys()
    }

    /// All edge keys.
    pub fn edge_ids(&self) -> impl Iterator<Item = EdgeId> + '_ {
        self.edges.keys()
    }

    /// All face keys.
    pub fn face_ids(&self) -> impl Iterator<Item = FaceId> + '_ {
        self.faces.keys()
    }

    /// The edge joining `a` and `b`, if one exists.
    pub fn edge_between(&self, a: VertexId, b: VertexId) -> Option<EdgeId> {
        let va = self.vertices.get(a)?;
        va.edges
            .iter()
            .copied()
            .find(|e| self.edges[*e].other(a) == b)
    }

    /// The vertex within `TOL_MERGE` of `p`, if any.
    pub fn find_vertex(&self, p: DVec3) -> Option<VertexId> {
        self.grid.find(p, &self.vertices)
    }

    /// Endpoint positions of an edge.
    pub fn edge_points(&self, e: EdgeId) -> Option<(DVec3, DVec3)> {
        let edge = self.edges.get(e)?;
        Some((
            self.vertices[edge.vertices[0]].position,
            self.vertices[edge.vertices[1]].position,
        ))
    }

    /// Length of an edge in mm.
    pub fn edge_length(&self, e: EdgeId) -> Option<f64> {
        self.edge_points(e).map(|(a, b)| a.distance(b))
    }

    /// The vertex a use starts from.
    pub(crate) fn use_start(&self, u: EdgeUse) -> VertexId {
        let v = self.edges[u.edge].vertices;
        if u.reversed { v[1] } else { v[0] }
    }

    /// The vertex sequence of a loop, in walk order.
    pub fn loop_vertices(&self, l: &[EdgeUse]) -> Vec<VertexId> {
        l.iter().map(|u| self.use_start(*u)).collect()
    }

    /// The positions of a loop's vertices, in walk order.
    pub fn loop_points(&self, l: &[EdgeUse]) -> Vec<DVec3> {
        l.iter()
            .map(|u| self.vertices[self.use_start(*u)].position)
            .collect()
    }

    /// Signed area of a loop in a plane's 2D coordinates (positive =
    /// counter-clockwise about the plane normal).
    pub(crate) fn signed_area(&self, l: &[EdgeUse], plane: &Plane) -> f64 {
        signed_area_2d(&self.loop_local(l, plane))
    }

    pub(crate) fn loop_local(&self, l: &[EdgeUse], plane: &Plane) -> Vec<DVec2> {
        self.loop_points(l)
            .into_iter()
            .map(|p| plane.to_local(p))
            .collect()
    }

    /// Area of a face in mm²: outer loop minus holes.
    pub fn face_area(&self, f: FaceId) -> Option<f64> {
        let face = self.faces.get(f)?;
        let outer = self.signed_area(&face.outer, &face.plane).abs();
        let holes: f64 = face
            .inner
            .iter()
            .map(|l| self.signed_area(l, &face.plane).abs())
            .sum();
        Some(outer - holes)
    }

    /// World-space triangles of a face, wound counter-clockwise about the face
    /// normal (so their front side is the face's front). Used for display,
    /// picking and mesh export; the model itself stores n-gons only.
    pub fn face_triangles(&self, f: FaceId) -> Vec<[DVec3; 3]> {
        let Some(face) = self.faces.get(f) else {
            return Vec::new();
        };
        let plane = face.plane;
        let mut points = self.loop_points(&face.outer);
        let outer: Vec<DVec2> = points.iter().map(|p| plane.to_local(*p)).collect();
        let holes: Vec<Vec<DVec2>> = face
            .inner
            .iter()
            .map(|l| {
                let pts = self.loop_points(l);
                let local = pts.iter().map(|p| plane.to_local(*p)).collect();
                points.extend(pts);
                local
            })
            .collect();
        smith_geom::triangulate::triangulate_polygon(&outer, &holes)
            .into_iter()
            .map(|[a, b, c]| [points[a], points[b], points[c]])
            .collect()
    }

    /// Axis-aligned bounds of all vertices, or `None` if the context is empty.
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut it = self.vertices.iter().map(|(_, v)| v.position);
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// Whether `p` (assumed on the face plane) lies strictly inside the face:
    /// inside the outer loop and outside every hole.
    pub(crate) fn face_contains_point(&self, f: FaceId, p: DVec3) -> bool {
        let face = &self.faces[f];
        let q = face.plane.to_local(p);
        point_in_polygon(q, &self.loop_local(&face.outer, &face.plane))
            && !face
                .inner
                .iter()
                .any(|l| point_in_polygon(q, &self.loop_local(l, &face.plane)))
    }

    // ----- low-level mutation ---------------------------------------------

    /// Returns the vertex at `p`, merging with an existing vertex within
    /// `TOL_MERGE` or creating a new one.
    pub(crate) fn merge_vertex(&mut self, p: DVec3) -> VertexId {
        if let Some(v) = self.grid.find(p, &self.vertices) {
            return v;
        }
        let id = self.vertices.insert(Vertex {
            position: p,
            edges: SmallVec::new(),
        });
        self.grid.insert(id, p);
        id
    }

    /// Creates an edge between two distinct vertices that are not yet joined.
    pub(crate) fn insert_edge(&mut self, a: VertexId, b: VertexId) -> EdgeId {
        debug_assert!(a != b && self.edge_between(a, b).is_none());
        let id = self.edges.insert(Edge {
            vertices: [a, b],
            faces: SmallVec::new(),
            flags: EdgeFlags {
                layer: self.draw_layer,
                ..EdgeFlags::default()
            },
            curve: None,
        });
        self.vertices[a].edges.push(id);
        self.vertices[b].edges.push(id);
        id
    }

    /// Inserts a face and registers it on every edge it uses.
    pub(crate) fn insert_face(
        &mut self,
        outer: Vec<EdgeUse>,
        inner: Vec<Vec<EdgeUse>>,
        plane: Plane,
        attributes: FaceAttributes,
    ) -> FaceId {
        let id = self.faces.insert(Face {
            outer,
            inner,
            plane,
            attributes,
        });
        let uses: Vec<EdgeUse> = self.faces[id].loops().flatten().copied().collect();
        for u in uses {
            let list = &mut self.edges[u.edge].faces;
            if !list.contains(&id) {
                list.push(id);
            }
        }
        id
    }

    /// Adds a hole loop to a face, oriented clockwise about the face normal.
    pub(crate) fn add_inner_loop(&mut self, f: FaceId, mut l: Vec<EdgeUse>) {
        let plane = self.faces[f].plane;
        if self.signed_area(&l, &plane) > 0.0 {
            reverse_loop(&mut l);
        }
        for u in &l {
            let list = &mut self.edges[u.edge].faces;
            if !list.contains(&f) {
                list.push(f);
            }
        }
        self.faces[f].inner.push(l);
    }

    /// Moves the holes of face `from` that lie inside face `to`'s outline to
    /// `to`.
    pub(crate) fn move_enclosed_holes(&mut self, from: FaceId, to: FaceId) {
        let enclosed: Vec<usize> = self.faces[from]
            .inner()
            .iter()
            .enumerate()
            .filter(|(_, h)| {
                let sample = self.vertices[self.use_start(h[0])].position();
                self.outer_contains(to, sample)
            })
            .map(|(i, _)| i)
            .collect();
        for i in enclosed.into_iter().rev() {
            let hole = self.faces[from].inner.remove(i);
            for u in &hole {
                if let Some(edge) = self.edges.get_mut(u.edge) {
                    edge.faces.retain(|x| *x != from);
                }
            }
            self.add_inner_loop(to, hole);
        }
    }

    /// Removes a face, unregistering it from its edges. Edges and vertices stay.
    pub(crate) fn remove_face(&mut self, f: FaceId) -> Option<Face> {
        let face = self.faces.remove(f)?;
        for u in face.loops().flatten() {
            if let Some(edge) = self.edges.get_mut(u.edge) {
                edge.faces.retain(|x| *x != f);
            }
        }
        Some(face)
    }

    /// Removes an edge (which must have no faces) and any vertex left without edges.
    pub(crate) fn remove_bare_edge(&mut self, e: EdgeId) {
        let Some(edge) = self.edges.remove(e) else {
            return;
        };
        debug_assert!(edge.faces.is_empty());
        if let Some(c) = edge.curve
            && let Some(record) = self.curves.get_mut(c)
        {
            record.edges.retain(|x| *x != e);
        }
        for v in edge.vertices {
            let vertex = &mut self.vertices[v];
            vertex.edges.retain(|x| *x != e);
            if vertex.edges.is_empty() {
                let p = vertex.position;
                self.vertices.remove(v);
                self.grid.remove(v, p);
            }
        }
    }

    /// Splits edge `e` at point `p` (which must lie on it). Face loops using the
    /// edge get the new vertex inserted in walk order. Returns the vertex at `p`;
    /// if `p` merges with an endpoint, nothing is split and that endpoint returns.
    pub(crate) fn split_edge(&mut self, e: EdgeId, p: DVec3) -> VertexId {
        let [v0, v1] = self.edges[e].vertices;
        let vm = self.merge_vertex(p);
        if vm == v0 || vm == v1 {
            return vm;
        }
        // e becomes v0–vm; the new edge e2 is vm–v1.
        self.edges[e].vertices[1] = vm;
        self.vertices[v1].edges.retain(|x| *x != e);
        self.vertices[vm].edges.push(e);
        let flags = self.edges[e].flags;
        let curve = self.edges[e].curve;
        let faces = self.edges[e].faces.clone();
        let e2 = self.edges.insert(Edge {
            vertices: [vm, v1],
            faces: faces.clone(),
            flags,
            curve,
        });
        self.vertices[vm].edges.push(e2);
        self.vertices[v1].edges.push(e2);
        // The new piece stays part of the same curve (see curves.rs).
        if let Some(c) = curve {
            self.curves[c].edges.push(e2);
        }

        for f in faces {
            let face = &mut self.faces[f];
            let loops = std::iter::once(&mut face.outer).chain(face.inner.iter_mut());
            for l in loops {
                let mut out = Vec::with_capacity(l.len() + 1);
                for u in l.iter() {
                    if u.edge != e {
                        out.push(*u);
                    } else if !u.reversed {
                        // v0 → vm → v1
                        out.push(EdgeUse {
                            edge: e,
                            reversed: false,
                        });
                        out.push(EdgeUse {
                            edge: e2,
                            reversed: false,
                        });
                    } else {
                        // v1 → vm → v0
                        out.push(EdgeUse {
                            edge: e2,
                            reversed: true,
                        });
                        out.push(EdgeUse {
                            edge: e,
                            reversed: true,
                        });
                    }
                }
                *l = out;
            }
        }
        vm
    }

    /// Removes vertex `v` where exactly two edges meet in a straight line,
    /// joining them into one edge (Dissolve Vertex). Faces keep their shape;
    /// the joined edge stays in a curve both pieces belonged to.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`]; [`ModelError::DoesNotFit`] unless the
    /// vertex joins exactly two edges in a straight line whose far ends are
    /// not already joined.
    pub fn dissolve_vertex(&mut self, v: VertexId) -> Result<(), ModelError> {
        self.join_at_vertex(v, 0.0)
    }

    /// As [`Entities::dissolve_vertex`], but the two edges may also bend by
    /// up to `max_angle` (radians) at the vertex — Join Split Edges with an
    /// Angle. Faces stay flat, since a face only loses a corner.
    ///
    /// # Errors
    /// As [`Entities::dissolve_vertex`].
    pub fn join_at_vertex(&mut self, v: VertexId, max_angle: f64) -> Result<(), ModelError> {
        let vertex = self.vertices.get(v).ok_or(ModelError::NoSuchEntity)?;
        let [e1, e2] = vertex.edges[..] else {
            return Err(ModelError::DoesNotFit("dissolve"));
        };
        let a = self.edges[e1].other(v);
        let c = self.edges[e2].other(v);
        let (pa, pv, pc) = (
            self.vertices[a].position,
            vertex.position,
            self.vertices[c].position,
        );
        let straight = (pv - pa).cross(pc - pv).length() <= TOL_MERGE * (pc - pa).length().max(1.0)
            || (max_angle > 0.0 && (pv - pa).angle_between(pc - pv) <= max_angle);
        let same_faces = {
            let mut f1 = self.edges[e1].faces.to_vec();
            let mut f2 = self.edges[e2].faces.to_vec();
            f1.sort();
            f2.sort();
            f1 == f2
        };
        if a == c || !straight || !same_faces || self.edge_between(a, c).is_some() {
            return Err(ModelError::DoesNotFit("dissolve"));
        }
        let curve = self.edges[e1]
            .curve
            .filter(|c| self.edges[e2].curve == Some(*c));
        let flags = self.edges[e1].flags;
        let faces = self.edges[e1].faces.clone();
        let e = self.insert_edge(a, c);
        self.edges[e].flags = flags;
        self.edges[e].curve = curve;
        self.edges[e].faces = faces.clone();
        if let Some(cv) = curve {
            self.curves[cv].edges.push(e);
        }
        // In each loop, the two uses (in either order) become one use of `e`.
        for f in &faces {
            let face = &mut self.faces[*f];
            let loops = std::iter::once(&mut face.outer).chain(face.inner.iter_mut());
            for l in loops {
                let Some(i) = l.iter().position(|u| u.edge == e1 || u.edge == e2) else {
                    continue;
                };
                let k = l.len();
                // The pair may straddle the loop's end.
                let (first, second) = if l[(i + 1) % k].edge == e1 || l[(i + 1) % k].edge == e2 {
                    (i, (i + 1) % k)
                } else {
                    ((i + k - 1) % k, i)
                };
                // Which end the walk enters from: the start of the first use.
                let u = l[first];
                let [s0, s1] = self.edges[u.edge].vertices;
                let enter = if u.reversed { s1 } else { s0 };
                let reversed = enter != a;
                let merged = EdgeUse { edge: e, reversed };
                if second > first {
                    l.splice(first..=second, [merged]);
                } else {
                    // Wraps: drop the last use, replace the first.
                    l.remove(first);
                    l[0] = merged;
                }
            }
        }
        for old in [e1, e2] {
            self.edges[old].faces.clear();
            self.remove_bare_edge(old);
        }
        Ok(())
    }

    /// Checks every structural invariant listed in the module documentation.
    ///
    /// # Errors
    /// A description of the first broken invariant. Intended for tests and debug
    /// assertions; a correct model never fails.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_links()?;
        let ids: Vec<_> = self.vertices.iter().collect();
        for (i, (a, va)) in ids.iter().enumerate() {
            for (b, vb) in &ids[i + 1..] {
                if va.position.distance(vb.position) <= TOL_MERGE {
                    return Err(format!("vertices {a:?} and {b:?} within TOL_MERGE"));
                }
            }
        }
        Ok(())
    }

    /// The linear-time part of [`Entities::validate`]: every check except the
    /// all-pairs vertex distance test (the merge grid still catches a vertex
    /// shadowed by a near neighbour). Used when reading files, where the
    /// quadratic test would be too slow for large models.
    ///
    /// # Errors
    /// A description of the first broken invariant.
    pub fn validate_links(&self) -> Result<(), String> {
        for (vid, v) in self.vertices.iter() {
            for e in &v.edges {
                let edge = self.edges.get(*e).ok_or("vertex lists missing edge")?;
                if !edge.vertices.contains(&vid) {
                    return Err("vertex lists an edge that does not use it".into());
                }
            }
            if self.grid.find(v.position, &self.vertices) != Some(vid) {
                return Err("vertex missing from merge grid or shadowed".into());
            }
        }
        for (eid, edge) in self.edges.iter() {
            let [a, b] = edge.vertices;
            if a == b {
                return Err("edge with identical endpoints".into());
            }
            for v in [a, b] {
                let vx = self.vertices.get(v).ok_or("edge uses missing vertex")?;
                if !vx.edges.contains(&eid) {
                    return Err("edge not listed on its vertex".into());
                }
            }
            if self.vertices[a]
                .edges
                .iter()
                .filter(|x| self.edges[**x].other(a) == b)
                .count()
                != 1
            {
                return Err("duplicate edges between one vertex pair".into());
            }
            for f in &edge.faces {
                let face = self.faces.get(*f).ok_or("edge lists missing face")?;
                if !face.loops().flatten().any(|u| u.edge == eid) {
                    return Err("edge lists a face that does not use it".into());
                }
            }
        }
        for (fid, face) in self.faces.iter() {
            for l in face.loops() {
                if l.len() < 3 {
                    return Err("face loop with fewer than 3 edges".into());
                }
                for (i, u) in l.iter().enumerate() {
                    let edge = self.edges.get(u.edge).ok_or("face uses missing edge")?;
                    if !edge.faces.contains(&fid) {
                        return Err("face edge does not list the face".into());
                    }
                    let end = edge.vertices[if u.reversed { 0 } else { 1 }];
                    let next = l[(i + 1) % l.len()];
                    if self.use_start(next) != end {
                        return Err(format!("face {fid:?} loop is not connected"));
                    }
                }
            }
            if self.signed_area(&face.outer, &face.plane) <= 0.0 {
                return Err(format!("face {fid:?} outer loop is not counter-clockwise"));
            }
            for l in &face.inner {
                if self.signed_area(l, &face.plane) >= 0.0 {
                    return Err(format!("face {fid:?} inner loop is not clockwise"));
                }
            }
        }
        Ok(())
    }

    /// Rejects non-finite input.
    pub(crate) fn check_finite(p: DVec3) -> Result<(), ModelError> {
        if p.is_finite() {
            Ok(())
        } else {
            Err(ModelError::NonFinite)
        }
    }
}

/// Reverses a loop's walk direction.
pub(crate) fn reverse_loop(l: &mut [EdgeUse]) {
    l.reverse();
    for u in l.iter_mut() {
        u.reversed = !u.reversed;
    }
}

/// Shoelace signed area of a 2D polygon.
pub(crate) fn signed_area_2d(pts: &[DVec2]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| pts[i].perp_dot(pts[(i + 1) % n]))
        .sum::<f64>()
        * 0.5
}

/// Even-odd point-in-polygon test (crossing number). Points exactly on the
/// boundary may go either way; callers sample strictly interior points.
pub(crate) fn point_in_polygon(q: DVec2, poly: &[DVec2]) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        if (a.y > q.y) != (b.y > q.y) {
            let x = a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if q.x < x {
                inside = !inside;
            }
        }
    }
    inside
}
