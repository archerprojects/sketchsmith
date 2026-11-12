//! SketchSmith — snapping view | v1.0.0 | Thursday 12 November 2026
//!
//! A [`SnapView`] is what the cursor sees in one context: a copy of the
//! context's own geometry, with the geometry of every group and component
//! placed in it (nested ones too) laid in at its placement. It exists so
//! inference can snap to a group's points, edges and faces from outside it,
//! and so a click on a group finds its surface.
//!
//! The copy keeps the context's own entities under their own keys, so a snap
//! to loose geometry names the real entity. Group geometry is inserted as it
//! is, without the sticky rules: nothing splits or merges, and the copy is
//! never edited or saved. Each inserted edge and face records the group (the
//! top-level instance in this context) it came from; [`SnapView::owner`]
//! answers which.
//!
//! While a group is open for editing ([`Document::snap_view_in`]), the
//! geometry outside it — the rest of the model and the levels between — is
//! laid in too, carried into the open group's coordinates and marked as
//! outside ([`SnapView::is_outside`]): it can be snapped to, never picked.
//!
//! The context's own guides come with the copy; guides inside groups, and
//! outside the group being edited, are laid in moved into place, there to
//! see and snap to but not to erase.
//!
//! What cannot be seen is left out: hidden faces, faces and edges on layers
//! switched off, hidden edges, and groups that are hidden or on layers
//! switched off. An edge
//! that still bounds a face that is shown stays, since the face's outline
//! needs it.

use std::collections::BTreeMap;

use smallvec::SmallVec;
use smith_geom::plane::Plane;
use smith_geom::{DMat3, DMat4};

use crate::curves::CurveRecord;
use crate::document::Document;
use crate::entities::{Edge, EdgeUse, Entities, Face, Vertex};
use crate::ids::{CurveId, DefinitionId, EdgeId, FaceId, GuideId, InstanceId, VertexId};
use crate::selection::Pickable;

/// A context's geometry with its groups' geometry laid in (see the module
/// documentation).
#[derive(Debug, Clone)]
pub struct SnapView {
    /// The combined geometry.
    pub entities: Entities,
    edge_owner: BTreeMap<EdgeId, InstanceId>,
    face_owner: BTreeMap<FaceId, InstanceId>,
    outside_edges: std::collections::BTreeSet<EdgeId>,
    outside_faces: std::collections::BTreeSet<FaceId>,
    /// Guides laid in from groups and from outside the group being edited:
    /// shown and snapped to, never erased from here.
    laid_guides: std::collections::BTreeSet<GuideId>,
}

impl SnapView {
    /// The group or component that `p` belongs to, when it came from one.
    pub fn owner(&self, p: Pickable) -> Option<InstanceId> {
        match p {
            Pickable::Edge(e) => self.edge_owner.get(&e).copied(),
            Pickable::Face(f) => self.face_owner.get(&f).copied(),
            Pickable::Instance(i) => Some(i),
            // Vertices are picked from the context itself, never the view.
            Pickable::Guide(_) | Pickable::Vertex(_) => None,
        }
    }

    /// Whether `p` is geometry outside the group being edited: there to snap
    /// to, not to pick.
    pub fn is_outside(&self, p: Pickable) -> bool {
        match p {
            Pickable::Edge(e) => self.outside_edges.contains(&e),
            Pickable::Face(f) => self.outside_faces.contains(&f),
            Pickable::Instance(_) | Pickable::Vertex(_) => false,
            Pickable::Guide(g) => self.laid_guides.contains(&g),
        }
    }

    /// The edges laid in for group `owner`, as end points.
    pub fn owned_lines(&self, owner: InstanceId) -> Vec<[smith_geom::DVec3; 2]> {
        self.edge_owner
            .iter()
            .filter(|(_, o)| **o == owner)
            .filter_map(|(e, _)| self.entities.edge_points(*e))
            .map(|(a, b)| [a, b])
            .collect()
    }
}

impl Document {
    /// The snapping view of context `ctx` (`None` = the root); `None` when
    /// the context does not exist.
    pub fn snap_view(&self, ctx: Option<DefinitionId>) -> Option<SnapView> {
        let here = self.context(ctx)?;
        let mut view = SnapView {
            entities: here.clone(),
            edge_owner: BTreeMap::new(),
            face_owner: BTreeMap::new(),
            outside_edges: Default::default(),
            outside_faces: Default::default(),
            laid_guides: Default::default(),
        };
        strip_unseen(&mut view.entities, &|l| self.layer_visible(l));
        for (id, inst) in here.instances.iter() {
            if !inst.hidden && self.layer_visible(inst.layer) {
                self.lay_in(&mut view, inst.definition, &inst.transform, Some(id), 0);
            }
        }
        Some(view)
    }

    /// The snapping view of the group at the end of `path` (instances from
    /// the root down, the last one open for editing), with everything
    /// outside it laid in as outside geometry, in its coordinates. An empty
    /// path is the root's view.
    pub fn snap_view_in(&self, path: &[InstanceId]) -> Option<SnapView> {
        // The contexts along the path, and each level's placement in world.
        let mut ctxs: Vec<Option<DefinitionId>> = vec![None];
        let mut placed: Vec<DMat4> = vec![DMat4::IDENTITY];
        for id in path {
            let here = *ctxs.last()?;
            let inst = self.context(here)?.instance(*id)?;
            ctxs.push(Some(inst.definition));
            placed.push(*placed.last()? * inst.transform);
        }
        let inner = *ctxs.last()?;
        let mut view = self.snap_view(inner)?;
        let to_inner = placed.last()?.inverse();
        // Each level above the open group: its own geometry, and its groups
        // other than the one on the path.
        for (level, id) in path.iter().enumerate() {
            let here = ctxs[level];
            let m = to_inner * placed[level];
            let Some(full) = self.context(here) else {
                continue;
            };
            self.lay_in_entities(&mut view, full, &m, None, 0, Some(*id));
        }
        Some(view)
    }

    /// Whether context `ctx` has anything the snapping view would differ in:
    /// groups, or anything hidden or on a layer switched off.
    pub fn needs_snap_view(&self, ctx: Option<DefinitionId>) -> bool {
        let Some(e) = self.context(ctx) else {
            return false;
        };
        e.instances.iter().next().is_some()
            || e.faces
                .iter()
                .any(|(_, f)| f.attributes.hidden || !self.layer_visible(f.attributes.layer))
            || e.edges
                .iter()
                .any(|(_, x)| x.flags.hidden || !self.layer_visible(x.flags.layer))
    }

    /// Lays `definition`'s geometry (and its nested groups') into `view` at
    /// `m`, owned by `owner`.
    fn lay_in(
        &self,
        view: &mut SnapView,
        definition: DefinitionId,
        m: &DMat4,
        owner: Option<InstanceId>,
        depth: u32,
    ) {
        let Some(full) = self.context(Some(definition)) else {
            return;
        };
        self.lay_in_entities(view, full, m, owner, depth, None);
    }

    /// Lays `full`'s geometry (what can be seen) and its groups' (all but
    /// `skip`) into `view` at `m`, owned by `owner`, or as outside geometry
    /// for `None`.
    fn lay_in_entities(
        &self,
        view: &mut SnapView,
        full: &Entities,
        m: &DMat4,
        owner: Option<InstanceId>,
        depth: u32,
        skip: Option<InstanceId>,
    ) {
        let mut src = full.clone();
        strip_unseen(&mut src, &|l| self.layer_visible(l));
        let src = &src;

        // Guides, moved into place.
        for (_, g) in src.guides.iter() {
            let id = view.entities.guides.insert(g.transformed(m));
            view.laid_guides.insert(id);
        }

        let dst = &mut view.entities;

        // Vertices first, with their edge lists filled in once the edges exist.
        let mut vmap: BTreeMap<VertexId, VertexId> = BTreeMap::new();
        for (k, v) in src.vertices.iter() {
            let p = m.transform_point3(v.position());
            let nk = dst.vertices.insert(Vertex::from_parts(p, SmallVec::new()));
            dst.grid.insert(nk, p);
            vmap.insert(k, nk);
        }
        // Curves a transform can carry stay curves; others become plain edges.
        let mut cmap: BTreeMap<CurveId, CurveId> = BTreeMap::new();
        for (k, c) in src.curves.iter() {
            if let Some(curve) = c.curve.transformed(m) {
                let nk = dst.curves.insert(CurveRecord {
                    curve,
                    segments: c.segments,
                    edges: Vec::new(),
                });
                cmap.insert(k, nk);
            }
        }
        let mut emap: BTreeMap<EdgeId, EdgeId> = BTreeMap::new();
        let mut vertex_edges: BTreeMap<VertexId, SmallVec<[EdgeId; 4]>> = BTreeMap::new();
        for (k, e) in src.edges.iter() {
            let [a, b] = e.vertices();
            let (Some(na), Some(nb)) = (vmap.get(&a), vmap.get(&b)) else {
                continue;
            };
            let curve = e.curve.and_then(|c| cmap.get(&c).copied());
            let nk = dst.edges.insert(Edge::from_parts(
                [*na, *nb],
                SmallVec::new(),
                curve,
                e.flags,
            ));
            if let Some(c) = curve {
                dst.curves[c].edges.push(nk);
            }
            vertex_edges.entry(*na).or_default().push(nk);
            vertex_edges.entry(*nb).or_default().push(nk);
            emap.insert(k, nk);
            match owner {
                Some(o) => {
                    view.edge_owner.insert(nk, o);
                }
                None => {
                    view.outside_edges.insert(nk);
                }
            }
        }
        for (v, list) in vertex_edges {
            let p = dst.vertices[v].position();
            dst.vertices[v] = Vertex::from_parts(p, list);
        }

        // Faces, with planes carried by the transform (normals by the
        // inverse transpose).
        let normals = DMat3::from_mat4(*m).inverse().transpose();
        let mut edge_faces: BTreeMap<EdgeId, SmallVec<[FaceId; 2]>> = BTreeMap::new();
        for (_, f) in src.faces.iter() {
            let carry = |l: &[EdgeUse]| -> Option<Vec<EdgeUse>> {
                l.iter()
                    .map(|u| {
                        emap.get(&u.edge).map(|e| EdgeUse {
                            edge: *e,
                            reversed: u.reversed,
                        })
                    })
                    .collect()
            };
            let Some(outer) = carry(&f.outer) else {
                continue;
            };
            let Some(inner) = f.inner.iter().map(|l| carry(l)).collect::<Option<Vec<_>>>() else {
                continue;
            };
            let point = m.transform_point3(f.plane.origin());
            let Ok(plane) = Plane::from_point_normal(point, normals * f.plane.normal()) else {
                continue;
            };
            let edges: Vec<EdgeId> = outer
                .iter()
                .chain(inner.iter().flatten())
                .map(|u| u.edge)
                .collect();
            let nk = dst
                .faces
                .insert(Face::from_parts(outer, inner, plane, f.attributes.clone()));
            for e in edges {
                edge_faces.entry(e).or_default().push(nk);
            }
            match owner {
                Some(o) => {
                    view.face_owner.insert(nk, o);
                }
                None => {
                    view.outside_faces.insert(nk);
                }
            }
        }
        for (e, list) in edge_faces {
            let edge = &dst.edges[e];
            let rebuilt = Edge::from_parts(edge.vertices(), list, edge.curve, edge.flags);
            dst.edges[e] = rebuilt;
        }

        if depth < 64 {
            let nested: Vec<_> = src
                .instances
                .iter()
                .filter(|(k, i)| Some(*k) != skip && !i.hidden && self.layer_visible(i.layer))
                .map(|(_, i)| (i.definition, *m * i.transform))
                .collect();
            for (d, t) in nested {
                self.lay_in(view, d, &t, owner, depth + 1);
            }
        }
    }
}

/// Removes from `e`, without the sticky rules, the faces that cannot be
/// seen (hidden, or on a layer `shows` rejects), then the edges that cannot
/// be seen and bound no face left.
fn strip_unseen(e: &mut Entities, shows: &dyn Fn(Option<crate::ids::LayerId>) -> bool) {
    let faces: Vec<FaceId> = e
        .faces
        .iter()
        .filter(|(_, f)| f.attributes.hidden || !shows(f.attributes.layer))
        .map(|(k, _)| k)
        .collect();
    for f in faces {
        if let Some(face) = e.faces.remove(f) {
            for u in face.outer.iter().chain(face.inner.iter().flatten()) {
                if let Some(edge) = e.edges.get(u.edge) {
                    let list: SmallVec<[FaceId; 2]> =
                        edge.faces().iter().copied().filter(|x| *x != f).collect();
                    let rebuilt = Edge::from_parts(edge.vertices(), list, edge.curve, edge.flags);
                    e.edges[u.edge] = rebuilt;
                }
            }
        }
    }
    let edges: Vec<EdgeId> = e
        .edges
        .iter()
        .filter(|(_, x)| (x.flags.hidden || !shows(x.flags.layer)) && x.faces().is_empty())
        .map(|(k, _)| k)
        .collect();
    for k in edges {
        let Some(edge) = e.edges.remove(k) else {
            continue;
        };
        if let Some(c) = edge.curve
            && let Some(rec) = e.curves.get_mut(c)
        {
            rec.edges.retain(|x| *x != k);
        }
        for v in edge.vertices() {
            if let Some(vert) = e.vertices.get(v) {
                let list: SmallVec<[EdgeId; 4]> =
                    vert.edges().iter().copied().filter(|x| *x != k).collect();
                e.vertices[v] = Vertex::from_parts(vert.position(), list);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use smith_geom::DVec3;

    use super::*;
    use crate::document::DefinitionKind;

    #[test]
    fn a_group_is_seen_at_its_placement_and_owned() {
        let mut doc = Document::new();
        let def = doc.create_definition("Box", DefinitionKind::Group);
        doc.context_entities_mut(Some(def))
            .unwrap()
            .add_polyline(
                &[
                    DVec3::ZERO,
                    DVec3::new(10.0, 0.0, 0.0),
                    DVec3::new(10.0, 10.0, 0.0),
                    DVec3::new(0.0, 10.0, 0.0),
                ],
                true,
            )
            .unwrap();
        doc.root_mut()
            .add_line(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(-5.0, 5.0, 0.0))
            .unwrap();
        let loose = doc.root().edge_ids().next().unwrap();
        let i = doc
            .add_instance(None, def, DMat4::from_translation(DVec3::Z * 3.0), "")
            .unwrap();
        let view = doc.snap_view(None).unwrap();
        let e = &view.entities;
        assert_eq!(e.edge_count(), 5);
        assert_eq!(e.face_count(), 1);
        // Loose geometry keeps its key and has no owner.
        assert!(e.edge(loose).is_some());
        assert_eq!(view.owner(Pickable::Edge(loose)), None);
        // The group's corner is at its placement, and its face is owned.
        assert!(e.find_vertex(DVec3::new(10.0, 10.0, 3.0)).is_some());
        let f = e.face_ids().next().unwrap();
        assert_eq!(view.owner(Pickable::Face(f)), Some(i));
        assert!((e.face(f).unwrap().plane().origin().z - 3.0).abs() < 1e-12);
        assert_eq!(view.owned_lines(i).len(), 4);
        e.validate().unwrap();
    }

    #[test]
    fn inside_a_group_the_outside_is_seen_but_not_owned() {
        let mut doc = Document::new();
        let def = doc.create_definition("G", DefinitionKind::Group);
        doc.context_entities_mut(Some(def))
            .unwrap()
            .add_line(DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0))
            .unwrap();
        // A loose line in the root, and the group placed 10 mm up.
        doc.root_mut()
            .add_line(DVec3::new(0.0, 5.0, 0.0), DVec3::new(4.0, 5.0, 0.0))
            .unwrap();
        let i = doc
            .add_instance(None, def, DMat4::from_translation(DVec3::Z * 10.0), "")
            .unwrap();
        let view = doc.snap_view_in(&[i]).unwrap();
        let e = &view.entities;
        // The group's own line, plus the root's line carried into the
        // group's coordinates (10 mm down).
        assert_eq!(e.edge_count(), 2);
        let outside = e
            .edge_ids()
            .find(|x| view.is_outside(Pickable::Edge(*x)))
            .unwrap();
        let (a, _) = e.edge_points(outside).unwrap();
        assert!((a.z + 10.0).abs() < 1e-12);
        assert_eq!(view.owner(Pickable::Edge(outside)), None);
    }

    #[test]
    fn what_is_hidden_is_not_seen() {
        let mut doc = Document::new();
        let off = doc.add_layer("Off", [0, 0, 0]);
        let r = doc.root_mut();
        r.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(5.0, 0.0, 0.0),
                DVec3::new(5.0, 5.0, 0.0),
                DVec3::new(0.0, 5.0, 0.0),
            ],
            true,
        )
        .unwrap();
        let stray = r
            .add_line(DVec3::new(10.0, 0.0, 0.0), DVec3::new(10.0, 5.0, 0.0))
            .unwrap()[0];
        let f = r.face_ids().next().unwrap();
        r.set_face_hidden(f, true).unwrap();
        r.set_layer(&[Pickable::Edge(stray)], Some(off)).unwrap();
        let mut l = doc.layer(off).unwrap().clone();
        l.visible = false;
        doc.set_layer(off, l).unwrap();
        assert!(doc.needs_snap_view(None));
        let view = doc.snap_view(None).unwrap();
        // The hidden face is gone; its four (visible) edges stay; the stray
        // edge on the switched-off layer is gone.
        assert_eq!(view.entities.face_count(), 0);
        assert_eq!(view.entities.edge_count(), 4);
        assert!(view.entities.edge(stray).is_none());
    }
}
