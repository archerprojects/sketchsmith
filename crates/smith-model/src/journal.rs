//! SketchSmith — per-context undo journal | v1.0.0 | Thursday 12 November 2026
//!
//! Collects the arena journals of one [`Entities`] into a single change set,
//! and applies change sets for undo and redo. The vertex merge grid is derived
//! data: it is updated alongside every vertex change rather than journaled.

use crate::annotations::Annotation;
use crate::arena::SharedChange;
use crate::curves::CurveRecord;
use crate::document::Instance;
use crate::entities::{Edge, Entities, Face, Vertex};
use crate::guides::Guide;
use crate::ids::{
    AnnotationId, CurveId, EdgeId, FaceId, GuideId, InstanceId, LayerId, SectionId, VertexId,
    WrapId,
};
use crate::paint::Wrap;
use crate::sections::SectionPlane;

/// Every entity changed in one context by one transaction.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct EntitiesChange {
    vertices: Vec<SharedChange<VertexId, Vertex>>,
    edges: Vec<SharedChange<EdgeId, Edge>>,
    faces: Vec<SharedChange<FaceId, Face>>,
    curves: Vec<SharedChange<CurveId, CurveRecord>>,
    instances: Vec<SharedChange<InstanceId, Instance>>,
    guides: Vec<SharedChange<GuideId, Guide>>,
    annotations: Vec<SharedChange<AnnotationId, Annotation>>,
    sections: Vec<SharedChange<SectionId, SectionPlane>>,
    wraps: Vec<SharedChange<WrapId, Wrap>>,
}

impl EntitiesChange {
    pub(crate) fn is_empty(&self) -> bool {
        self.vertices.is_empty()
            && self.edges.is_empty()
            && self.faces.is_empty()
            && self.curves.is_empty()
            && self.instances.is_empty()
            && self.guides.is_empty()
            && self.annotations.is_empty()
            && self.sections.is_empty()
            && self.wraps.is_empty()
    }
}

impl Entities {
    pub(crate) fn begin_journal(&mut self) {
        self.vertices.begin_journal();
        self.edges.begin_journal();
        self.faces.begin_journal();
        self.curves.begin_journal();
        self.instances.begin_journal();
        self.guides.begin_journal();
        self.annotations.begin_journal();
        self.sections.begin_journal();
        self.wraps.begin_journal();
    }

    pub(crate) fn journal_open(&self) -> bool {
        self.vertices.journal_open()
    }

    /// Whether the geometry (points, edges, faces, curves, groups) has been
    /// changed in the open transaction. Paint, edge marks and line styles are
    /// Present-mode look, not geometry, and do not count.
    pub(crate) fn geometry_touched(&self) -> bool {
        self.vertices.journal_touched()
            || self.edges.journal_changed_beyond(Edge::same_but_look)
            || self.faces.journal_changed_beyond(Face::same_but_paint)
            || self.curves.journal_touched()
            || self.instances.journal_touched()
    }

    /// Whether the open journal changes anything on a layer `locked` says is
    /// locked (`None`: Base): an edge, a face or a group, as it was or as it
    /// is now, or a vertex an edge on such a layer uses.
    pub(crate) fn journal_touches(&self, locked: &dyn Fn(Option<LayerId>) -> bool) -> bool {
        let edge_locked = |e: &EdgeId| {
            self.edges.get(*e).is_some_and(|x| locked(x.flags.layer))
                || self
                    .edges
                    .journal_before(*e)
                    .is_some_and(|x| locked(x.flags.layer))
        };
        self.edges
            .journal_changes()
            .any(|(b, a)| b.into_iter().chain(a).any(|x| locked(x.flags.layer)))
            || self
                .faces
                .journal_changes()
                .any(|(b, a)| b.into_iter().chain(a).any(|x| locked(x.attributes.layer)))
            || self
                .instances
                .journal_changes()
                .any(|(b, a)| b.into_iter().chain(a).any(|x| locked(x.layer)))
            || self.vertices.journal_changes().any(|(b, a)| {
                b.into_iter()
                    .chain(a)
                    .any(|v| v.edges().iter().any(edge_locked))
            })
    }

    pub(crate) fn take_journal(&mut self) -> EntitiesChange {
        EntitiesChange {
            vertices: self.vertices.take_journal(),
            edges: self.edges.take_journal(),
            faces: self.faces.take_journal(),
            curves: self.curves.take_journal(),
            instances: self.instances.take_journal(),
            guides: self.guides.take_journal(),
            annotations: self.annotations.take_journal(),
            sections: self.sections.take_journal(),
            wraps: self.wraps.take_journal(),
        }
    }

    /// Discards everything recorded since the journal began.
    pub(crate) fn rollback_journal(&mut self) {
        let change = self.take_journal();
        self.apply_change(&change, true);
    }

    /// Applies one side of a change set: `undo = true` restores the befores.
    pub(crate) fn apply_change(&mut self, change: &EntitiesChange, undo: bool) {
        for c in &change.vertices {
            if let Some(v) = self.vertices.get(c.key) {
                let p = v.position();
                self.grid.remove(c.key, p);
            }
        }
        self.vertices.apply(&change.vertices, undo);
        for c in &change.vertices {
            if let Some(v) = self.vertices.get(c.key) {
                let p = v.position();
                self.grid.insert(c.key, p);
            }
        }
        self.edges.apply(&change.edges, undo);
        self.faces.apply(&change.faces, undo);
        self.curves.apply(&change.curves, undo);
        self.instances.apply(&change.instances, undo);
        self.guides.apply(&change.guides, undo);
        self.annotations.apply(&change.annotations, undo);
        self.sections.apply(&change.sections, undo);
        self.wraps.apply(&change.wraps, undo);
    }
}
