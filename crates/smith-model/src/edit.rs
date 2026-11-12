//! SketchSmith — editing primitives | v1.0.0 | Thursday 12 November 2026
//!
//! The model-level operations the modify tools are built from:
//!
//! * [`Entities::erase_face`] removes a face and keeps its edges.
//! * [`Entities::reverse_face`] swaps a face's front and back.
//! * [`Entities::set_edge_hidden`] hides or shows an edge.
//! * [`Entities::move_vertices`] moves a set of vertices by one vector,
//!   stretching only the faces attached to them. Every changed face must stay
//!   flat within the planarity tolerance; if any would bend, nothing moves and
//!   the faces that would bend are returned ([`ModelError::NotFlat`]) —
//!   unless the operation runs with Auto-Tri (`autotri`), which folds just
//!   those faces into triangles and moves. A moved
//!   vertex may not land on a vertex that stays put ([`ModelError::Collides`]).
//!   A curve whose vertices all move is carried along whole and stays editable;
//!   a curve only partly moved loses its true shape and becomes plain edges.
//! * [`Entities::split_edge_at`] inserts points along an edge (Divide); the
//!   pieces stay members of the edge's curve.
//! * [`Entities::orient_faces_from`] turns every face connected to one face
//!   the same way as it.
//! * [`Entities::fill_faces`] makes the faces that existing edges close, as
//!   drawing those edges would have done.
//! * [`Entities::fill`] (Fill) makes faces from edges (refused when they
//!   close no flat loop, so no face is made), or joins vertices in
//!   the order given: two make an edge; three or more on one plane make a
//!   face round them, reusing edges already there. Off one plane they are
//!   refused ([`ModelError::NotFlat`]) unless Auto-Tri is on, when the
//!   outline is fanned into triangles from the first vertex with soft,
//!   smooth diagonals.
//! * [`Entities::set_layer`] puts edges, faces, groups and components on a
//!   layer; [`Entities::set_face_hidden`] hides or shows a face;
//!   [`Entities::set_instance_locked`] locks or unlocks a group or component;
//!   [`Entities::set_instance_hidden`] and [`Entities::set_instance_name`]
//!   hide and name one.

use std::collections::HashSet;

use smith_geom::plane::Plane;
use smith_geom::tolerance::{coincident, extent, planar_tolerance};
use smith_geom::{DMat3, DMat4, DVec3};

use crate::entities::{Entities, reverse_loop};
use crate::error::ModelError;
use crate::ids::{CurveId, EdgeId, FaceId, InstanceId, LayerId, VertexId};
use crate::selection::Pickable;

impl Entities {
    /// Erases a face. Its edges and vertices stay.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] if the face does not exist.
    pub fn erase_face(&mut self, f: FaceId) -> Result<(), ModelError> {
        self.remove_face(f)
            .map(|_| ())
            .ok_or(ModelError::NoSuchEntity)
    }

    /// Swaps a face's front and back: its loops are walked the other way and
    /// its plane's normal is flipped. Paint stays on the physical side it was
    /// on, so the front's paint becomes the back's and the other way round.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] if the face does not exist.
    pub fn reverse_face(&mut self, f: FaceId) -> Result<(), ModelError> {
        let face = self.faces.get_mut(f).ok_or(ModelError::NoSuchEntity)?;
        reverse_loop(&mut face.outer);
        for l in &mut face.inner {
            reverse_loop(l);
        }
        face.plane = face.plane.flipped();
        let a = &mut face.attributes;
        std::mem::swap(&mut a.front, &mut a.back);
        Ok(())
    }

    /// Hides or shows an edge.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] if the edge does not exist.
    pub fn set_edge_hidden(&mut self, e: EdgeId, hidden: bool) -> Result<(), ModelError> {
        let edge = self.edges.get_mut(e).ok_or(ModelError::NoSuchEntity)?;
        edge.flags.hidden = hidden;
        Ok(())
    }

    /// Moves each vertex in `targets` to its own new position (proportional
    /// editing). Every face touched must stay flat; a vertex may not land on
    /// one that stays; curves touched become plain edges, since their points
    /// no longer follow the curve.
    ///
    /// # Errors
    /// As [`Entities::transform_vertices`]. On any error nothing changes.
    pub fn reposition_vertices(&mut self, targets: &[(VertexId, DVec3)]) -> Result<(), ModelError> {
        let moving: std::collections::HashMap<VertexId, DVec3> = targets.iter().copied().collect();
        for (v, p) in &moving {
            Self::check_finite(*p)?;
            if !self.vertices.contains_key(*v) {
                return Err(ModelError::NoSuchEntity);
            }
        }
        let new_pos = |s: &Self, v: VertexId| {
            moving
                .get(&v)
                .copied()
                .unwrap_or_else(|| s.vertices[v].position())
        };
        for p in moving.values() {
            let hit = self
                .vertices
                .iter()
                .any(|(id, other)| !moving.contains_key(&id) && coincident(other.position(), *p));
            if hit {
                return Err(ModelError::Collides);
            }
        }
        let ids: Vec<VertexId> = moving.keys().copied().collect();
        let mut planes = Vec::new();
        let mut bent = Vec::new();
        for f in self.faces_at(&ids) {
            let face = &self.faces[f];
            let all: Vec<DVec3> = face
                .loops()
                .flat_map(|l| self.loop_vertices(l))
                .map(|v| new_pos(self, v))
                .collect();
            let outer: Vec<DVec3> = self
                .loop_vertices(face.outer())
                .into_iter()
                .map(|v| new_pos(self, v))
                .collect();
            let fitted = Plane::fit_loop(&outer)
                .ok()
                .filter(|plane| plane.max_deviation(&all) <= planar_tolerance(extent(&all)));
            match fitted {
                Some(mut plane) => {
                    if plane.normal().dot(face.plane().normal()) < 0.0 {
                        plane = plane.flipped();
                    }
                    planes.push((f, plane));
                }
                None => bent.push(f),
            }
        }
        if !bent.is_empty() {
            if crate::autotri::auto_tri() {
                // Auto-Tri for this operation: fold just these faces, then
                // try again (triangles always stay flat).
                self.fold_faces(&bent)?;
                return self.reposition_vertices(targets);
            }
            return Err(ModelError::NotFlat(bent));
        }
        let mut curves: Vec<CurveId> = Vec::new();
        for v in &ids {
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
        for (v, p) in moving {
            let old = self.vertices[v].position();
            self.grid.remove(v, old);
            self.vertices[v].set_position(p);
            self.grid.insert(v, p);
        }
        for (f, plane) in planes {
            self.faces[f].plane = plane;
        }
        Ok(())
    }

    /// Splits edge `e` at each point in `points` (each must lie on it),
    /// keeping faces and curve membership. Returns the edges the original
    /// became, in order from its start.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`]; [`ModelError::NonFinite`].
    pub fn split_edge_at(
        &mut self,
        e: EdgeId,
        points: &[DVec3],
    ) -> Result<Vec<EdgeId>, ModelError> {
        let (a, _) = self.edge_points(e).ok_or(ModelError::NoSuchEntity)?;
        for p in points {
            Self::check_finite(*p)?;
        }
        let mut sorted = points.to_vec();
        sorted.sort_by(|p, q| p.distance(a).total_cmp(&q.distance(a)));
        // Split from the far end back, so `e` always keeps the start piece
        // and each split lands on the piece that still contains it.
        let [start, end] = self.edges[e].vertices();
        let mut chain = vec![start];
        for p in sorted.iter().rev() {
            let v = self.split_edge(e, *p);
            if v != start && v != end && !chain.contains(&v) {
                chain.push(v);
            }
        }
        // The pieces, in order from the start.
        chain[1..].reverse();
        chain.push(end);
        Ok(chain
            .windows(2)
            .filter_map(|w| self.edge_between(w[0], w[1]))
            .collect())
    }

    /// Turns every face connected to `start` through edges shared by exactly
    /// two faces so that each uses the shared edge the opposite way to its
    /// neighbour — the same way out as `start`. Returns how many turned.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] if `start` does not exist.
    pub fn orient_faces_from(&mut self, start: FaceId) -> Result<usize, ModelError> {
        if !self.faces.contains_key(start) {
            return Err(ModelError::NoSuchEntity);
        }
        let mut seen = HashSet::from([start]);
        let mut queue = vec![start];
        let mut turned = 0;
        while let Some(f) = queue.pop() {
            let uses: Vec<(EdgeId, bool)> = self.faces[f]
                .loops()
                .flatten()
                .map(|u| (u.edge, u.reversed))
                .collect();
            for (e, reversed) in uses {
                let neighbours = self.edges[e].faces();
                if neighbours.len() != 2 {
                    continue;
                }
                let g = if neighbours[0] == f {
                    neighbours[1]
                } else {
                    neighbours[0]
                };
                if !seen.insert(g) {
                    continue;
                }
                let theirs = self.faces[g]
                    .loops()
                    .flatten()
                    .find(|u| u.edge == e)
                    .map(|u| u.reversed);
                if theirs == Some(reversed) {
                    self.reverse_face(g)?;
                    turned += 1;
                }
                queue.push(g);
            }
        }
        Ok(turned)
    }

    /// Makes the faces that `edges` close, as drawing them would: a loop in
    /// empty space becomes a face, and a loop inside a face splits it or fills
    /// its hole. Edges already gone are skipped.
    pub fn fill_faces(&mut self, edges: &[EdgeId]) {
        for e in edges {
            if self.edges.get(*e).is_some() {
                self.resolve_faces_for_edge(*e);
            }
        }
    }

    /// Fill: with `edges`, the faces their flat loops close; otherwise the
    /// `vertices` joined in order (see the module documentation).
    ///
    /// # Errors
    /// [`ModelError::Refused`] when `edges` close no flat loop (no face is
    /// made); [`ModelError::NoSuchEntity`] for a vertex that does not exist or
    /// fewer than two; [`ModelError::Refused`] when three or more lie in a
    /// line; [`ModelError::NotFlat`] (no faces named) when they are off one
    /// plane and Auto-Tri is off; otherwise as [`Entities::add_line`].
    pub fn fill(&mut self, vertices: &[VertexId], edges: &[EdgeId]) -> Result<(), ModelError> {
        if !edges.is_empty() {
            let before = self.face_count();
            self.fill_faces(edges);
            if self.face_count() == before {
                return Err(ModelError::Refused("These Edges Close No Flat Loop".into()));
            }
            return Ok(());
        }
        let mut pts: Vec<DVec3> = Vec::new();
        for v in vertices {
            let p = self.vertex(*v).ok_or(ModelError::NoSuchEntity)?.position();
            if !pts.contains(&p) {
                pts.push(p);
            }
        }
        match pts.len() {
            0 | 1 => Err(ModelError::NoSuchEntity),
            2 => self.add_line(pts[0], pts[1]).map(|_| ()),
            n => {
                let Ok(plane) = Plane::fit_loop(&pts) else {
                    return Err(ModelError::Refused("These Vertices Lie in a Line".into()));
                };
                let flat = plane.contains_all(&pts);
                if !flat && !crate::autotri::auto_tri() {
                    return Err(ModelError::NotFlat(Vec::new()));
                }
                let mut outline = Vec::new();
                for i in 0..n {
                    let (a, b) = (pts[i], pts[(i + 1) % n]);
                    match self.add_line(a, b) {
                        Ok(made) => outline.extend(made),
                        Err(ModelError::ZeroLength) => {}
                        Err(e) => return Err(e),
                    }
                    if let (Some(va), Some(vb)) = (self.find_vertex(a), self.find_vertex(b))
                        && let Some(e) = self.edge_between(va, vb)
                    {
                        outline.push(e);
                    }
                }
                if flat {
                    // Edges already there make no face on their own.
                    self.fill_faces(&outline);
                } else {
                    // Auto-Tri: a fan of triangles from the first vertex.
                    let mut folds = Vec::new();
                    for p in &pts[2..n - 1] {
                        folds.extend(self.add_line(pts[0], *p)?);
                    }
                    self.soften_edges(&folds);
                }
                Ok(())
            }
        }
    }

    /// Puts `items` on `layer` (`None`: Base).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`] for an item that does not exist; then
    /// nothing changes.
    pub fn set_layer(
        &mut self,
        items: &[Pickable],
        layer: Option<LayerId>,
    ) -> Result<(), ModelError> {
        let exists = |p: &Pickable| match p {
            Pickable::Edge(e) => self.edges.get(*e).is_some(),
            Pickable::Face(f) => self.faces.get(*f).is_some(),
            Pickable::Instance(i) => self.instances.get(*i).is_some(),
            Pickable::Guide(g) => self.guides.get(*g).is_some(),
            Pickable::Vertex(v) => self.vertices.get(*v).is_some(),
        };
        if !items.iter().all(exists) {
            return Err(ModelError::NoSuchEntity);
        }
        for p in items {
            match p {
                Pickable::Edge(e) => {
                    if let Some(x) = self.edges.get_mut(*e) {
                        x.flags.layer = layer;
                    }
                }
                Pickable::Face(f) => {
                    if let Some(x) = self.faces.get_mut(*f) {
                        x.attributes.layer = layer;
                    }
                }
                Pickable::Instance(i) => {
                    if let Some(x) = self.instances.get_mut(*i) {
                        x.layer = layer;
                    }
                }
                // Guides and vertices are on no layer.
                Pickable::Guide(_) | Pickable::Vertex(_) => {}
            }
        }
        Ok(())
    }

    /// Moves everything on layer `from` to layer `to` (`None`: Base).
    pub fn move_layer(&mut self, from: LayerId, to: Option<LayerId>) {
        let edges: Vec<EdgeId> = self
            .edges
            .iter()
            .filter(|(_, e)| e.flags.layer == Some(from))
            .map(|(k, _)| k)
            .collect();
        for e in edges {
            if let Some(x) = self.edges.get_mut(e) {
                x.flags.layer = to;
            }
        }
        let faces: Vec<FaceId> = self
            .faces
            .iter()
            .filter(|(_, f)| f.attributes.layer == Some(from))
            .map(|(k, _)| k)
            .collect();
        for f in faces {
            if let Some(x) = self.faces.get_mut(f) {
                x.attributes.layer = to;
            }
        }
        let instances: Vec<InstanceId> = self
            .instances
            .iter()
            .filter(|(_, i)| i.layer == Some(from))
            .map(|(k, _)| k)
            .collect();
        for i in instances {
            if let Some(x) = self.instances.get_mut(i) {
                x.layer = to;
            }
        }
    }

    /// Hides or shows face `f`.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn set_face_hidden(&mut self, f: FaceId, hidden: bool) -> Result<(), ModelError> {
        self.faces
            .get_mut(f)
            .ok_or(ModelError::NoSuchEntity)?
            .attributes
            .hidden = hidden;
        Ok(())
    }

    /// Every hidden face.
    pub fn hidden_faces(&self) -> Vec<FaceId> {
        self.faces
            .iter()
            .filter(|(_, f)| f.attributes.hidden)
            .map(|(k, _)| k)
            .collect()
    }

    /// Locks or unlocks group or component `id`.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn set_instance_locked(&mut self, id: InstanceId, locked: bool) -> Result<(), ModelError> {
        self.instances
            .get_mut(id)
            .ok_or(ModelError::NoSuchEntity)?
            .locked = locked;
        Ok(())
    }

    /// Hides or shows group or component `id`.
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn set_instance_hidden(&mut self, id: InstanceId, hidden: bool) -> Result<(), ModelError> {
        self.instances
            .get_mut(id)
            .ok_or(ModelError::NoSuchEntity)?
            .hidden = hidden;
        Ok(())
    }

    /// Names group or component `id` (the instance, not its definition).
    ///
    /// # Errors
    /// [`ModelError::NoSuchEntity`].
    pub fn set_instance_name(&mut self, id: InstanceId, name: &str) -> Result<(), ModelError> {
        self.instances
            .get_mut(id)
            .ok_or(ModelError::NoSuchEntity)?
            .name = name.to_string();
        Ok(())
    }

    /// Every hidden group and component.
    pub fn hidden_instances(&self) -> Vec<InstanceId> {
        self.instances
            .iter()
            .filter(|(_, i)| i.hidden)
            .map(|(k, _)| k)
            .collect()
    }

    /// Every hidden edge.
    pub fn hidden_edges(&self) -> Vec<EdgeId> {
        self.edges
            .iter()
            .filter(|(_, e)| e.flags.hidden)
            .map(|(id, _)| id)
            .collect()
    }

    /// The faces that use any of `vertices`.
    pub fn faces_at(&self, vertices: &[VertexId]) -> Vec<FaceId> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for v in vertices {
            let Some(vertex) = self.vertices.get(*v) else {
                continue;
            };
            for e in vertex.edges() {
                for f in self.edges[*e].faces() {
                    if seen.insert(*f) {
                        out.push(*f);
                    }
                }
            }
        }
        out
    }

    /// Moves `vertices` by `delta`, stretching the attached faces (see
    /// [`Entities::transform_vertices`]).
    ///
    /// # Errors
    /// As [`Entities::transform_vertices`].
    pub fn move_vertices(&mut self, vertices: &[VertexId], delta: DVec3) -> Result<(), ModelError> {
        Self::check_finite(delta)?;
        self.transform_vertices(vertices, &DMat4::from_translation(delta))
    }

    /// Applies `m` to `vertices` (a move, rotation, scale or mirror),
    /// stretching only the faces attached to them. A face whose vertices all
    /// move turns with them (and is turned the right way out after a mirror);
    /// any other face they touch must stay flat.
    ///
    /// # Errors
    /// [`ModelError::NonFinite`]; [`ModelError::NoSuchEntity`] for a vertex
    /// that does not exist; [`ModelError::NotFlat`] with the faces that would
    /// bend; [`ModelError::Collides`] if a moved vertex would land on one that
    /// stays. On any error nothing changes.
    pub fn transform_vertices(
        &mut self,
        vertices: &[VertexId],
        m: &DMat4,
    ) -> Result<(), ModelError> {
        if !m.is_finite() {
            return Err(ModelError::NonFinite);
        }
        let moving: HashSet<VertexId> = vertices.iter().copied().collect();
        for v in &moving {
            if !self.vertices.contains_key(*v) {
                return Err(ModelError::NoSuchEntity);
            }
        }
        let new_pos = |s: &Self, v: VertexId| {
            let p = s.vertices[v].position();
            if moving.contains(&v) {
                m.transform_point3(p)
            } else {
                p
            }
        };
        let mirror = DMat3::from_mat4(*m).determinant() < 0.0;

        // A transform that only swaps the points among themselves (flipping a
        // symmetric shape about its own centre) leaves the shape as it is.
        // Moving the points would twist the faces beside them, so nothing
        // moves. A face that lands on itself with its front reversed (a flat
        // face flipped along its normal) is turned over.
        let lands_among = |pts: &[DVec3]| {
            pts.iter().all(|p| {
                let q = m.transform_point3(*p);
                pts.iter().any(|o| coincident(*o, q))
            })
        };
        let old: Vec<DVec3> = moving
            .iter()
            .map(|v| self.vertices[*v].position())
            .collect();
        if lands_among(&old) {
            for f in self.faces_at(vertices) {
                let face = &self.faces[f];
                let pts: Vec<DVec3> = face.loops().flat_map(|l| self.loop_points(l)).collect();
                let n = face.plane().normal();
                if lands_among(&pts) && m.transform_vector3(n).dot(n) < 0.0 {
                    self.reverse_face(f)?;
                }
            }
            return Ok(());
        }

        // Collisions with vertices that stay.
        for v in &moving {
            let target = new_pos(self, *v);
            let hit = self
                .vertices
                .iter()
                .any(|(id, other)| !moving.contains(&id) && coincident(other.position(), target));
            if hit {
                return Err(ModelError::Collides);
            }
        }

        // Every face touching a moved vertex must stay flat. A face moved
        // whole keeps its front with the transform; a stretched face keeps
        // its old front side.
        let touched = self.faces_at(vertices);
        let mut planes = Vec::with_capacity(touched.len());
        let mut bent = Vec::new();
        // Faces moved whole carry their paint; stretched faces leave theirs
        // where it lies.
        let mut carried = Vec::new();
        for f in &touched {
            let face = &self.faces[*f];
            let ids: Vec<VertexId> = face.loops().flat_map(|l| self.loop_vertices(l)).collect();
            let whole = ids.iter().all(|v| moving.contains(v));
            if whole {
                carried.push(*f);
            }
            let all: Vec<DVec3> = ids.iter().map(|v| new_pos(self, *v)).collect();
            let outer: Vec<DVec3> = self
                .loop_vertices(face.outer())
                .into_iter()
                .map(|v| new_pos(self, v))
                .collect();
            let fitted = Plane::fit_loop(&outer)
                .ok()
                .filter(|plane| plane.max_deviation(&all) <= planar_tolerance(extent(&all)));
            match fitted {
                Some(mut plane) => {
                    let front = if whole {
                        m.transform_vector3(face.plane().normal())
                    } else {
                        face.plane().normal()
                    };
                    if plane.normal().dot(front) < 0.0 {
                        plane = plane.flipped();
                    }
                    planes.push((*f, plane, whole && mirror));
                }
                None => bent.push(*f),
            }
        }
        if !bent.is_empty() {
            if crate::autotri::auto_tri() {
                self.fold_faces(&bent)?;
                return self.transform_vertices(vertices, m);
            }
            return Err(ModelError::NotFlat(bent));
        }

        // Curves: carried whole when the transform keeps their shape, else
        // exploded to plain edges.
        let mut curves: Vec<CurveId> = Vec::new();
        for v in &moving {
            for e in self.vertices[*v].edges() {
                if let Some(c) = self.edges[*e].curve()
                    && !curves.contains(&c)
                {
                    curves.push(c);
                }
            }
        }
        for c in curves {
            let whole = self.curves[c].edges.iter().all(|e| {
                self.edges
                    .get(*e)
                    .is_some_and(|edge| edge.vertices().iter().all(|v| moving.contains(v)))
            });
            match self.curves[c].curve.transformed(m).filter(|_| whole) {
                Some(moved) => self.curves[c].curve = moved,
                None => self.explode_curve(c)?,
            }
        }

        // Apply.
        for v in &moving {
            let old = self.vertices[*v].position();
            self.grid.remove(*v, old);
            let p = m.transform_point3(old);
            self.vertices[*v].set_position(p);
            self.grid.insert(*v, p);
        }
        for (f, plane, reverse) in planes {
            let face = &mut self.faces[f];
            face.plane = plane;
            if reverse {
                // A mirror reverses each loop's turn about the face normal.
                reverse_loop(&mut face.outer);
                for l in &mut face.inner {
                    reverse_loop(l);
                }
            }
        }
        self.carry_paint(&carried, m);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use smith_geom::DVec3;

    use super::*;

    fn square(e: &mut Entities, z: f64) {
        let p = [
            DVec3::new(0.0, 0.0, z),
            DVec3::new(10.0, 0.0, z),
            DVec3::new(10.0, 10.0, z),
            DVec3::new(0.0, 10.0, z),
        ];
        e.add_polyline(&p, true).unwrap();
    }

    #[test]
    fn moving_a_whole_face_keeps_it_flat() {
        let mut e = Entities::new();
        square(&mut e, 0.0);
        let vs: Vec<VertexId> = e.vertex_ids().collect();
        e.move_vertices(&vs, DVec3::new(0.0, 0.0, 5.0)).unwrap();
        let f = e.face_ids().next().unwrap();
        assert!((e.face(f).unwrap().plane().signed_distance(DVec3::ZERO) + 5.0).abs() < 1e-9);
        e.validate().unwrap();
    }

    #[test]
    fn bending_a_face_is_refused_and_changes_nothing() {
        let mut e = Entities::new();
        square(&mut e, 0.0);
        let before = e.clone();
        let corner = e.find_vertex(DVec3::new(10.0, 10.0, 0.0)).unwrap();
        let err = e
            .move_vertices(&[corner], DVec3::new(0.0, 0.0, 1.0))
            .unwrap_err();
        assert!(matches!(err, ModelError::NotFlat(ref fs) if fs.len() == 1));
        assert_eq!(e, before);
    }

    #[test]
    fn moving_onto_a_vertex_that_stays_is_refused() {
        let mut e = Entities::new();
        square(&mut e, 0.0);
        let corner = e.find_vertex(DVec3::new(10.0, 0.0, 0.0)).unwrap();
        let err = e
            .move_vertices(&[corner], DVec3::new(-10.0, 0.0, 0.0))
            .unwrap_err();
        assert_eq!(err, ModelError::Collides);
    }

    #[test]
    fn reversing_a_face_flips_its_normal() {
        let mut e = Entities::new();
        square(&mut e, 0.0);
        let f = e.face_ids().next().unwrap();
        let n = e.face(f).unwrap().plane().normal();
        e.reverse_face(f).unwrap();
        assert!((e.face(f).unwrap().plane().normal() + n).length() < 1e-12);
        e.validate().unwrap();
    }

    #[test]
    fn dividing_an_edge_keeps_its_face() {
        let mut e = Entities::new();
        square(&mut e, 0.0);
        let edge = e
            .edge_between(
                e.find_vertex(DVec3::ZERO).unwrap(),
                e.find_vertex(DVec3::new(10.0, 0.0, 0.0)).unwrap(),
            )
            .unwrap();
        let pts: Vec<DVec3> = (1..4)
            .map(|i| DVec3::new(2.5 * f64::from(i), 0.0, 0.0))
            .collect();
        let pieces = e.split_edge_at(edge, &pts).unwrap();
        assert_eq!(pieces.len(), 4);
        for piece in &pieces {
            assert!((e.edge_length(*piece).unwrap() - 2.5).abs() < 1e-12);
        }
        assert_eq!(e.face_count(), 1);
        e.validate().unwrap();
    }

    #[test]
    fn orient_faces_turns_the_odd_one() {
        let mut e = Entities::new();
        square(&mut e, 0.0);
        e.add_polyline(
            &[
                DVec3::new(10.0, 0.0, 0.0),
                DVec3::new(20.0, 0.0, 0.0),
                DVec3::new(20.0, 10.0, 0.0),
                DVec3::new(10.0, 10.0, 0.0),
            ],
            false,
        )
        .unwrap();
        let ids: Vec<FaceId> = e.face_ids().collect();
        assert_eq!(ids.len(), 2);
        e.reverse_face(ids[1]).unwrap();
        assert_eq!(e.orient_faces_from(ids[0]).unwrap(), 1);
        let n0 = e.face(ids[0]).unwrap().plane().normal();
        let n1 = e.face(ids[1]).unwrap().plane().normal();
        assert!(n0.dot(n1) > 0.99);
        e.validate().unwrap();
    }

    #[test]
    fn dissolving_a_point_on_a_straight_side() {
        let mut e = Entities::new();
        e.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(5.0, 0.0, 0.0),
                DVec3::new(10.0, 0.0, 0.0),
                DVec3::new(10.0, 10.0, 0.0),
                DVec3::new(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        assert_eq!(e.edge_count(), 5);
        let mid = e.find_vertex(DVec3::new(5.0, 0.0, 0.0)).unwrap();
        e.dissolve_vertex(mid).unwrap();
        assert_eq!(e.edge_count(), 4);
        assert_eq!(e.face_count(), 1);
        e.validate().unwrap();
        // A corner cannot be dissolved.
        let corner = e.find_vertex(DVec3::new(10.0, 10.0, 0.0)).unwrap();
        assert_eq!(
            e.dissolve_vertex(corner),
            Err(ModelError::DoesNotFit("dissolve"))
        );
    }

    #[test]
    fn hiding_an_edge_is_recorded() {
        let mut e = Entities::new();
        square(&mut e, 0.0);
        let edge = e.edge_ids().next().unwrap();
        e.set_edge_hidden(edge, true).unwrap();
        assert_eq!(e.hidden_edges(), vec![edge]);
    }
}
