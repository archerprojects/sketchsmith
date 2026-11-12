//! SketchSmith — selection | v1.0.0 | Thursday 12 November 2026
//!
//! The set of selected vertices, edges, faces, groups and components in the
//! active context (a group or component is picked whole), plus the
//! selection rules that depend on topology:
//!
//! * double-click a face → the face and its edges;
//!   double-click an edge → the edge and the faces using it;
//! * triple-click → everything connected through shared vertices;
//! * Select ▸ Bounding Edges / Connected Faces (context menu).
//!
//! Selecting works in one of three modes ([`SelectKind`]), as in Blender:
//! a click or a box picks vertices, edges or faces. A box also takes what
//! its picks complete or contain ([`Entities::completed`]): picked vertices
//! bring the edges and faces whose vertices are all picked; picked edges
//! bring their vertices and the faces whose edges are all picked; picked
//! faces bring their edges and vertices. Switching mode converts the
//! selection ([`Entities::converted`]): going up keeps only complete items,
//! going down keeps every part.
//!
//! The selection remembers the order vertices were picked in
//! ([`Selection::vertices`]), for tools that join them in turn.
//!
//! The modifier rules (Alt adds, Shift toggles, Ctrl+Shift removes) live in
//! [`SelectMode`] and [`Selection::apply`]; Ctrl alone picks a path
//! ([`Entities::vertex_path`], [`Entities::edge_path`]), as in Blender: from
//! the last item picked ([`Selection::last`]) to the one clicked, every
//! vertex or edge on the shortest way along edges, in order. Selection is UI
//! state: it is not journaled for undo.

use std::collections::{BTreeSet, VecDeque};

use crate::entities::Entities;
use crate::ids::{EdgeId, FaceId, GuideId, InstanceId, VertexId};

/// A selectable entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Pickable {
    /// An edge.
    Edge(EdgeId),
    /// A face.
    Face(FaceId),
    /// A group or component, picked whole.
    Instance(InstanceId),
    /// A guide line or guide point. Guides are snapped to and erased, but
    /// never selected.
    Guide(GuideId),
    /// A vertex.
    Vertex(VertexId),
}

/// What a click or a box picks (the select mode).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SelectKind {
    /// Vertices.
    Vertices,
    /// Edges.
    #[default]
    Edges,
    /// Faces.
    Faces,
}

impl SelectKind {
    /// The modes, in icon order.
    pub const ALL: [SelectKind; 3] = [Self::Vertices, Self::Edges, Self::Faces];

    /// The mode's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Vertices => "Vertices",
            Self::Edges => "Edges",
            Self::Faces => "Faces",
        }
    }
}

/// How a pick combines with the existing selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectMode {
    /// Plain click: replace the selection.
    Replace,
    /// Ctrl: add (Ctrl switches on).
    Add,
    /// Toggle: in or out (no key gives it since 0.1.123).
    Toggle,
    /// Shift: remove (Shift switches off).
    Remove,
}

impl SelectMode {
    /// The mode for the given modifier keys: Ctrl adds, Shift removes.
    /// Ctrl + Shift (the path pick, for clicks) replaces, like a plain pick.
    /// Alt does not change a selection (desktops take Alt + drag).
    pub fn from_modifiers(_alt: bool, ctrl: bool, shift: bool) -> Self {
        match (ctrl, shift) {
            (true, false) => Self::Add,
            (false, true) => Self::Remove,
            _ => Self::Replace,
        }
    }
}

/// The current selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    items: BTreeSet<Pickable>,
    /// The selected vertices in the order they were picked.
    order: Vec<VertexId>,
    /// The last item picked that is still selected.
    last: Option<Pickable>,
}

impl Selection {
    /// An empty selection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Number of selected entities.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether `item` is selected.
    pub fn contains(&self, item: Pickable) -> bool {
        self.items.contains(&item)
    }

    /// Selected entities in key order.
    pub fn iter(&self) -> impl Iterator<Item = Pickable> + '_ {
        self.items.iter().copied()
    }

    /// Selected edges.
    pub fn edges(&self) -> impl Iterator<Item = EdgeId> + '_ {
        self.items.iter().filter_map(|p| match p {
            Pickable::Edge(e) => Some(*e),
            _ => None,
        })
    }

    /// Selected faces.
    pub fn faces(&self) -> impl Iterator<Item = FaceId> + '_ {
        self.items.iter().filter_map(|p| match p {
            Pickable::Face(f) => Some(*f),
            _ => None,
        })
    }

    /// Selected groups and components.
    pub fn instances(&self) -> impl Iterator<Item = InstanceId> + '_ {
        self.items.iter().filter_map(|p| match p {
            Pickable::Instance(i) => Some(*i),
            _ => None,
        })
    }

    /// Everything selected: the vertices first, in the order they were
    /// picked, then the rest in key order. What tools are given.
    pub fn ordered(&self) -> Vec<Pickable> {
        self.order
            .iter()
            .map(|v| Pickable::Vertex(*v))
            .chain(
                self.items
                    .iter()
                    .filter(|p| !matches!(p, Pickable::Vertex(_)))
                    .copied(),
            )
            .collect()
    }

    /// The last item picked that is still selected (where a path pick
    /// starts).
    pub fn last(&self) -> Option<Pickable> {
        self.last
    }

    /// Selected vertices, in the order they were picked.
    pub fn vertices(&self) -> impl Iterator<Item = VertexId> + '_ {
        self.order.iter().copied()
    }

    /// Clears the selection.
    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.last = None;
    }

    /// Combines `picked` with the selection according to `mode`. With
    /// [`SelectMode::Replace`] and nothing picked, the selection is cleared.
    pub fn apply(&mut self, picked: impl IntoIterator<Item = Pickable>, mode: SelectMode) {
        // Guides are never selected.
        let in_order: Vec<Pickable> = picked
            .into_iter()
            .filter(|p| !matches!(p, Pickable::Guide(_)))
            .collect();
        let picked: BTreeSet<Pickable> = in_order.iter().copied().collect();
        match mode {
            SelectMode::Replace => self.items = picked,
            SelectMode::Add => self.items.extend(picked),
            SelectMode::Remove => {
                for p in picked {
                    self.items.remove(&p);
                }
            }
            SelectMode::Toggle => {
                for p in picked {
                    if !self.items.remove(&p) {
                        self.items.insert(p);
                    }
                }
            }
        }
        // Keep the pick order: vertices still selected stay where they
        // were, newly selected ones follow in the order given.
        if mode == SelectMode::Replace {
            self.order.clear();
        }
        let items = &self.items;
        self.order.retain(|v| items.contains(&Pickable::Vertex(*v)));
        for p in &in_order {
            if let Pickable::Vertex(v) = p
                && self.items.contains(p)
                && !self.order.contains(v)
            {
                self.order.push(*v);
            }
        }
        if let Some(p) = in_order.last().filter(|p| self.items.contains(p)) {
            self.last = Some(*p);
        } else if self.last.is_some_and(|p| !self.items.contains(&p)) {
            self.last = None;
        }
    }

    /// Drops entities that no longer exist (after erase, undo or a new document).
    pub fn prune(&mut self, entities: &Entities) {
        self.items.retain(|p| match p {
            Pickable::Edge(e) => entities.edge(*e).is_some(),
            Pickable::Face(f) => entities.face(*f).is_some(),
            Pickable::Instance(i) => entities.instance(*i).is_some(),
            Pickable::Vertex(v) => entities.vertex(*v).is_some(),
            Pickable::Guide(_) => false,
        });
        let items = &self.items;
        self.order.retain(|v| items.contains(&Pickable::Vertex(*v)));
        if self.last.is_some_and(|p| !self.items.contains(&p)) {
            self.last = None;
        }
    }
}

impl Entities {
    /// Everything a double-click on `item` selects: a face with its edges, or an
    /// edge with the faces that use it.
    pub fn double_click_set(&self, item: Pickable) -> Vec<Pickable> {
        let mut out = vec![item];
        match item {
            Pickable::Face(f) => {
                if let Some(face) = self.face(f) {
                    out.extend(face.loops().flatten().map(|u| Pickable::Edge(u.edge)));
                }
            }
            Pickable::Edge(e) => {
                if let Some(edge) = self.edge(e) {
                    out.extend(edge.faces().iter().map(|f| Pickable::Face(*f)));
                }
            }
            Pickable::Vertex(_) | Pickable::Instance(_) | Pickable::Guide(_) => {}
        }
        out
    }

    /// Everything connected to `item` through shared vertices (triple-click).
    pub fn connected_set(&self, item: Pickable) -> Vec<Pickable> {
        let mut seen_v: BTreeSet<VertexId> = BTreeSet::new();
        let mut queue: VecDeque<VertexId> = VecDeque::new();
        let start_vertices: Vec<VertexId> = match item {
            Pickable::Edge(e) => self
                .edge(e)
                .map(|x| x.vertices().to_vec())
                .unwrap_or_default(),
            Pickable::Face(f) => self
                .face(f)
                .map(|face| self.loop_vertices(face.outer()))
                .unwrap_or_default(),
            Pickable::Vertex(v) => vec![v],
            Pickable::Instance(_) | Pickable::Guide(_) => return vec![item],
        };
        queue.extend(start_vertices);
        let mut edges: BTreeSet<EdgeId> = BTreeSet::new();
        while let Some(v) = queue.pop_front() {
            if !seen_v.insert(v) {
                continue;
            }
            let Some(vertex) = self.vertex(v) else {
                continue;
            };
            for e in vertex.edges() {
                if edges.insert(*e) {
                    queue.push_back(self.edges[*e].other(v));
                }
            }
        }
        let faces: BTreeSet<FaceId> = edges
            .iter()
            .flat_map(|e| self.edges[*e].faces().iter().copied())
            .collect();
        edges
            .into_iter()
            .map(Pickable::Edge)
            .chain(faces.into_iter().map(Pickable::Face))
            .collect()
    }

    /// The edges bounding the selected faces (Select ▸ Bounding Edges).
    pub fn bounding_edges(&self, selection: &Selection) -> Vec<Pickable> {
        selection
            .faces()
            .filter_map(|f| self.face(f))
            .flat_map(|face| {
                face.loops()
                    .flatten()
                    .map(|u| Pickable::Edge(u.edge))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The faces using the selected edges, plus selected faces' edge-neighbours
    /// (Select ▸ Connected Faces).
    pub fn connected_faces(&self, selection: &Selection) -> Vec<Pickable> {
        let mut out: BTreeSet<FaceId> = BTreeSet::new();
        for e in selection.edges() {
            if let Some(edge) = self.edge(e) {
                out.extend(edge.faces());
            }
        }
        for f in selection.faces() {
            if let Some(face) = self.face(f) {
                for u in face.loops().flatten() {
                    out.extend(self.edges[u.edge].faces());
                }
            }
        }
        out.into_iter().map(Pickable::Face).collect()
    }

    /// Every edge, face, group and component in the context (Select All).
    pub fn all_pickables(&self) -> Vec<Pickable> {
        self.edge_ids()
            .map(Pickable::Edge)
            .chain(self.face_ids().map(Pickable::Face))
            .chain(self.instance_ids().map(Pickable::Instance))
            .collect()
    }

    /// The vertices on the shortest way along edges from `from` to `to`,
    /// both included, in order; `None` when no edges join them.
    pub fn vertex_path(&self, from: VertexId, to: VertexId) -> Option<Vec<VertexId>> {
        use std::cmp::Reverse;
        use std::collections::{BTreeMap, BinaryHeap};
        self.vertex(from)?;
        self.vertex(to)?;
        // Dijkstra over edge lengths; distances ordered by their bits (all
        // non-negative, so bit order is number order).
        let mut dist: BTreeMap<VertexId, f64> = BTreeMap::from([(from, 0.0)]);
        let mut prev: BTreeMap<VertexId, VertexId> = BTreeMap::new();
        let mut heap = BinaryHeap::from([Reverse((0f64.to_bits(), from))]);
        while let Some(Reverse((d, v))) = heap.pop() {
            let d = f64::from_bits(d);
            if v == to {
                break;
            }
            if dist.get(&v).is_some_and(|best| d > *best) {
                continue;
            }
            for e in self.vertices[v].edges() {
                let w = self.edges[*e].other(v);
                let nd = d + self.edge_length(*e).unwrap_or(0.0);
                if dist.get(&w).is_none_or(|best| nd < *best) {
                    dist.insert(w, nd);
                    prev.insert(w, v);
                    heap.push(Reverse((nd.to_bits(), w)));
                }
            }
        }
        if from != to && !prev.contains_key(&to) {
            return None;
        }
        let mut path = vec![to];
        while let Some(p) = prev.get(path.last()?) {
            path.push(*p);
            if *p == from {
                break;
            }
        }
        path.reverse();
        Some(path)
    }

    /// The edges on the shortest way from edge `from` to edge `to`, both
    /// included, in order: through whichever of their ends are nearest
    /// along edges. `None` when no edges join them.
    pub fn edge_path(&self, from: EdgeId, to: EdgeId) -> Option<Vec<EdgeId>> {
        let (a, b) = (self.edge(from)?.vertices(), self.edge(to)?.vertices());
        if from == to {
            return Some(vec![from]);
        }
        let length = |p: &[VertexId]| -> f64 {
            p.windows(2)
                .filter_map(|w| self.edge_between(w[0], w[1]))
                .filter_map(|e| self.edge_length(e))
                .sum()
        };
        let best = a
            .iter()
            .flat_map(|x| b.iter().map(move |y| (*x, *y)))
            .filter_map(|(x, y)| self.vertex_path(x, y))
            .min_by(|p, q| length(p).total_cmp(&length(q)))?;
        let mut out = vec![from];
        for w in best.windows(2) {
            if let Some(e) = self.edge_between(w[0], w[1])
                && !out.contains(&e)
                && e != to
            {
                out.push(e);
            }
        }
        out.push(to);
        Some(out)
    }

    /// What picking `picked` with a box in `kind` mode selects: the picks
    /// of that kind, with what they complete or contain (see the module
    /// documentation). Groups and components pass through; picks of
    /// other kinds are dropped.
    pub fn completed(&self, picked: &[Pickable], kind: SelectKind) -> Vec<Pickable> {
        let mut vertices: BTreeSet<VertexId> = BTreeSet::new();
        let mut edges: BTreeSet<EdgeId> = BTreeSet::new();
        let mut faces: BTreeSet<FaceId> = BTreeSet::new();
        let mut out: Vec<Pickable> = picked
            .iter()
            .filter(|p| matches!(p, Pickable::Instance(_)))
            .copied()
            .collect();
        for p in picked {
            match (kind, p) {
                (SelectKind::Vertices, Pickable::Vertex(v)) if self.vertex(*v).is_some() => {
                    vertices.insert(*v);
                }
                (SelectKind::Edges, Pickable::Edge(e)) if self.edge(*e).is_some() => {
                    edges.insert(*e);
                }
                (SelectKind::Faces, Pickable::Face(f)) if self.face(*f).is_some() => {
                    faces.insert(*f);
                }
                _ => {}
            }
        }
        match kind {
            SelectKind::Vertices => {
                edges.extend(self.edge_ids().filter(|e| {
                    self.edges[*e]
                        .vertices()
                        .iter()
                        .all(|v| vertices.contains(v))
                }));
                faces.extend(self.face_ids().filter(|f| {
                    self.faces[*f]
                        .loops()
                        .all(|l| self.loop_vertices(l).iter().all(|v| vertices.contains(v)))
                }));
            }
            SelectKind::Edges => {
                vertices.extend(edges.iter().flat_map(|e| self.edges[*e].vertices()));
                faces.extend(self.face_ids().filter(|f| {
                    self.faces[*f]
                        .loops()
                        .flatten()
                        .all(|u| edges.contains(&u.edge))
                }));
            }
            SelectKind::Faces => {
                for f in &faces {
                    for u in self.faces[*f].loops().flatten() {
                        edges.insert(u.edge);
                        vertices.extend(self.edges[u.edge].vertices());
                    }
                }
            }
        }
        out.extend(vertices.into_iter().map(Pickable::Vertex));
        out.extend(edges.into_iter().map(Pickable::Edge));
        out.extend(faces.into_iter().map(Pickable::Face));
        out
    }

    /// `items` converted to `kind` mode: going up keeps only what is
    /// complete (an edge whose vertices are both selected, a face whose
    /// edges or vertices all are), going down keeps every part; then as
    /// [`Entities::completed`].
    pub fn converted(&self, items: &[Pickable], kind: SelectKind) -> Vec<Pickable> {
        let has = |p: Pickable| items.contains(&p);
        let mut picks: Vec<Pickable> = items
            .iter()
            .filter(|p| matches!(p, Pickable::Instance(_)))
            .copied()
            .collect();
        match kind {
            SelectKind::Vertices => {
                let mut vertices: BTreeSet<VertexId> = BTreeSet::new();
                for p in items {
                    match p {
                        Pickable::Vertex(v) => {
                            vertices.insert(*v);
                        }
                        Pickable::Edge(e) => {
                            if let Some(edge) = self.edge(*e) {
                                vertices.extend(edge.vertices());
                            }
                        }
                        Pickable::Face(f) => {
                            if let Some(face) = self.face(*f) {
                                vertices.extend(face.loops().flat_map(|l| self.loop_vertices(l)));
                            }
                        }
                        Pickable::Instance(_) | Pickable::Guide(_) => {}
                    }
                }
                picks.extend(vertices.into_iter().map(Pickable::Vertex));
            }
            SelectKind::Edges => {
                let mut edges: BTreeSet<EdgeId> = BTreeSet::new();
                for e in self.edge_ids() {
                    let [a, b] = self.edges[e].vertices();
                    if has(Pickable::Edge(e))
                        || (has(Pickable::Vertex(a)) && has(Pickable::Vertex(b)))
                        || self.edges[e]
                            .faces()
                            .iter()
                            .any(|f| has(Pickable::Face(*f)))
                    {
                        edges.insert(e);
                    }
                }
                picks.extend(edges.into_iter().map(Pickable::Edge));
            }
            SelectKind::Faces => {
                for f in self.face_ids() {
                    let face = &self.faces[f];
                    let all_edges = face.loops().flatten().all(|u| has(Pickable::Edge(u.edge)));
                    let all_vertices = face.loops().all(|l| {
                        self.loop_vertices(l)
                            .iter()
                            .all(|v| has(Pickable::Vertex(*v)))
                    });
                    if has(Pickable::Face(f)) || all_edges || all_vertices {
                        picks.push(Pickable::Face(f));
                    }
                }
            }
        }
        self.completed(&picks, kind)
    }

    /// Bounds of the selected entities, or `None` if nothing is selected.
    pub fn selection_bounds(
        &self,
        selection: &Selection,
    ) -> Option<(smith_geom::DVec3, smith_geom::DVec3)> {
        let mut pts = Vec::new();
        for p in selection.iter() {
            match p {
                Pickable::Edge(e) => {
                    if let Some((a, b)) = self.edge_points(e) {
                        pts.extend([a, b]);
                    }
                }
                Pickable::Face(f) => {
                    if let Some(face) = self.face(f) {
                        pts.extend(self.loop_points(face.outer()));
                    }
                }
                // Only its insertion point: the geometry is in its definition.
                Pickable::Instance(i) => {
                    if let Some(inst) = self.instance(i) {
                        pts.push(inst.transform.w_axis.truncate());
                    }
                }
                Pickable::Vertex(v) => {
                    if let Some(vertex) = self.vertex(v) {
                        pts.push(vertex.position());
                    }
                }
                Pickable::Guide(_) => {}
            }
        }
        let first = *pts.first()?;
        Some(
            pts.iter()
                .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_geom::DVec3;

    fn square(m: &mut Entities, x: f64) {
        let v = |a: f64, b: f64| DVec3::new(a, b, 0.0);
        m.add_polyline(
            &[v(x, 0.0), v(x + 1.0, 0.0), v(x + 1.0, 1.0), v(x, 1.0)],
            true,
        )
        .unwrap();
    }

    #[test]
    fn modifier_keys() {
        assert_eq!(
            SelectMode::from_modifiers(false, false, false),
            SelectMode::Replace
        );
        // Ctrl adds, Shift removes; Alt changes nothing.
        assert_eq!(
            SelectMode::from_modifiers(false, true, false),
            SelectMode::Add
        );
        assert_eq!(
            SelectMode::from_modifiers(false, false, true),
            SelectMode::Remove
        );
        assert_eq!(
            SelectMode::from_modifiers(true, false, false),
            SelectMode::Replace
        );
        // Ctrl + Shift is the path pick: as a mode, it replaces.
        assert_eq!(
            SelectMode::from_modifiers(false, true, true),
            SelectMode::Replace
        );
    }

    #[test]
    fn paths_follow_the_shortest_way_along_edges() {
        let mut m = Entities::new();
        // A row of five points along X, and a long way round from the first
        // to the last.
        let p = |x: f64, y: f64| smith_geom::DVec3::new(x, y, 0.0);
        m.add_polyline(
            &[
                p(0.0, 0.0),
                p(1.0, 0.0),
                p(2.0, 0.0),
                p(3.0, 0.0),
                p(4.0, 0.0),
            ],
            false,
        )
        .unwrap();
        m.add_polyline(&[p(0.0, 0.0), p(0.0, 9.0), p(4.0, 9.0), p(4.0, 0.0)], false)
            .unwrap();
        let at = |x: f64, y: f64| m.find_vertex(p(x, y)).unwrap();
        let path = m.vertex_path(at(0.0, 0.0), at(4.0, 0.0)).unwrap();
        assert_eq!(
            path,
            [
                at(0.0, 0.0),
                at(1.0, 0.0),
                at(2.0, 0.0),
                at(3.0, 0.0),
                at(4.0, 0.0)
            ]
        );
        let first = m.edge_between(at(0.0, 0.0), at(1.0, 0.0)).unwrap();
        let last = m.edge_between(at(3.0, 0.0), at(4.0, 0.0)).unwrap();
        assert_eq!(m.edge_path(first, last).unwrap().len(), 4);
        // Nothing joins a loose point.
        let start = at(0.0, 0.0);
        m.add_line(p(9.0, 9.0), p(9.0, 8.0)).unwrap();
        let loose = m.find_vertex(p(9.0, 9.0)).unwrap();
        assert!(m.vertex_path(start, loose).is_none());
    }

    #[test]
    fn the_last_pick_is_remembered() {
        let mut m = Entities::new();
        square(&mut m, 0.0);
        let vs: Vec<VertexId> = m.vertex_ids().collect();
        let mut s = Selection::new();
        s.apply([Pickable::Vertex(vs[1])], SelectMode::Replace);
        s.apply([Pickable::Vertex(vs[3])], SelectMode::Add);
        assert_eq!(s.last(), Some(Pickable::Vertex(vs[3])));
        s.apply([Pickable::Vertex(vs[3])], SelectMode::Toggle);
        assert_eq!(s.last(), None);
    }

    #[test]
    fn modifier_rules() {
        let mut s = Selection::new();
        let a = Pickable::Edge(<EdgeId as crate::ids::Key>::from_raw(1));
        let b = Pickable::Edge(<EdgeId as crate::ids::Key>::from_raw(2));
        s.apply([a], SelectMode::Replace);
        s.apply([b], SelectMode::Add);
        assert_eq!(s.len(), 2);
        s.apply([a], SelectMode::Toggle);
        assert!(!s.contains(a));
        s.apply([a], SelectMode::Toggle);
        assert!(s.contains(a));
        s.apply([b], SelectMode::Remove);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![a]);
        s.apply([], SelectMode::Replace);
        assert!(s.is_empty());
    }

    #[test]
    fn double_and_triple_click_sets() {
        let mut m = Entities::new();
        square(&mut m, 0.0);
        square(&mut m, 5.0);
        let f = m.face_ids().next().unwrap();
        assert_eq!(m.double_click_set(Pickable::Face(f)).len(), 5);
        let connected = m.connected_set(Pickable::Face(f));
        assert_eq!(connected.len(), 5, "the second square is not connected");
        let e = m.edge_ids().next().unwrap();
        assert_eq!(m.double_click_set(Pickable::Edge(e)).len(), 2);
    }

    fn kinds(items: &[Pickable]) -> (usize, usize, usize) {
        let count = |f: fn(&Pickable) -> bool| items.iter().filter(|p| f(p)).count();
        (
            count(|p| matches!(p, Pickable::Vertex(_))),
            count(|p| matches!(p, Pickable::Edge(_))),
            count(|p| matches!(p, Pickable::Face(_))),
        )
    }

    #[test]
    fn a_box_brings_what_its_picks_complete() {
        let mut m = Entities::new();
        square(&mut m, 0.0);
        let vs: Vec<Pickable> = m.vertex_ids().map(Pickable::Vertex).collect();
        // Three corners: the two edges between them, no face.
        assert_eq!(
            kinds(&m.completed(&vs[..3], SelectKind::Vertices)),
            (3, 2, 0)
        );
        // All four: every edge and the face.
        assert_eq!(kinds(&m.completed(&vs, SelectKind::Vertices)), (4, 4, 1));
        let es: Vec<Pickable> = m.edge_ids().map(Pickable::Edge).collect();
        assert_eq!(kinds(&m.completed(&es[..1], SelectKind::Edges)), (2, 1, 0));
        assert_eq!(kinds(&m.completed(&es, SelectKind::Edges)), (4, 4, 1));
        let f = Pickable::Face(m.face_ids().next().unwrap());
        assert_eq!(kinds(&m.completed(&[f], SelectKind::Faces)), (4, 4, 1));
        // Picks of another kind are dropped.
        assert_eq!(kinds(&m.completed(&[f], SelectKind::Edges)), (0, 0, 0));
    }

    #[test]
    fn switching_mode_keeps_complete_items_going_up_and_parts_going_down() {
        let mut m = Entities::new();
        square(&mut m, 0.0);
        let es: Vec<Pickable> = m.edge_ids().map(Pickable::Edge).collect();
        // One edge up to faces: no face is complete.
        let one = m.completed(&es[..1], SelectKind::Edges);
        assert_eq!(kinds(&m.converted(&one, SelectKind::Faces)), (0, 0, 0));
        // One edge down to vertices: its two ends.
        assert_eq!(kinds(&m.converted(&one, SelectKind::Vertices)), (2, 1, 0));
        // The face down to edges: all four, and the face still complete.
        let f = Pickable::Face(m.face_ids().next().unwrap());
        let face = m.completed(&[f], SelectKind::Faces);
        assert_eq!(kinds(&m.converted(&face, SelectKind::Edges)), (4, 4, 1));
    }

    #[test]
    fn vertices_keep_the_order_they_were_picked_in() {
        let mut m = Entities::new();
        square(&mut m, 0.0);
        let vs: Vec<VertexId> = m.vertex_ids().collect();
        let mut s = Selection::new();
        s.apply([Pickable::Vertex(vs[2])], SelectMode::Replace);
        s.apply([Pickable::Vertex(vs[0])], SelectMode::Add);
        s.apply([Pickable::Vertex(vs[3])], SelectMode::Add);
        assert_eq!(s.vertices().collect::<Vec<_>>(), [vs[2], vs[0], vs[3]]);
        s.apply([Pickable::Vertex(vs[0])], SelectMode::Toggle);
        assert_eq!(s.vertices().collect::<Vec<_>>(), [vs[2], vs[3]]);
        s.apply([Pickable::Vertex(vs[0])], SelectMode::Add);
        assert_eq!(s.vertices().collect::<Vec<_>>(), [vs[2], vs[3], vs[0]]);
        s.apply([Pickable::Vertex(vs[1])], SelectMode::Replace);
        assert_eq!(s.vertices().collect::<Vec<_>>(), [vs[1]]);
    }

    #[test]
    fn prune_drops_erased_entities() {
        let mut m = Entities::new();
        square(&mut m, 0.0);
        let mut s = Selection::new();
        s.apply(m.all_pickables(), SelectMode::Replace);
        assert_eq!(s.len(), 5);
        let e = m.edge_ids().next().unwrap();
        m.erase_edge(e).unwrap();
        s.prune(&m);
        assert_eq!(s.len(), 3);
    }
}
