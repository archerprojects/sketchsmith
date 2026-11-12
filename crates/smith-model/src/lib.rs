//! # smith-model
//!
//! SketchSmith — document model | v1.0.0 | Thursday 12 November 2026
//!
//! The SketchUp-style geometry model: sticky, non-manifold,
//! planar geometry stored in a radial-edge structure, with true curves,
//! definitions and instances, and transaction-based undo.
//!
//! * An [`Edge`] joins two [`Vertex`]es and keeps a *radial list* of every face
//!   that uses it — zero, one, two, or more (a fin hanging off a box edge gives
//!   three). Half-edge structures cannot represent this; SketchUp geometry needs it.
//! * A [`Face`] is planar and bounded by one outer loop and any number of inner
//!   loops (holes). A loop is an ordered list of [`EdgeUse`]s.
//! * [`Entities`] owns the geometry of one context and implements the sticky
//!   behaviour: vertex merge, edge splitting, auto-face, face splitting, holes,
//!   heal on erase, and curves ([`CurveRecord`]); and the clean-up primitives:
//!   merging points by distance, joining split edges and merging coplanar
//!   faces; and intersecting faces ([`FaceCuts`]), with fusing overlapping
//!   faces on one plane, and Fuse's booleans ([`BooleanOp`]).
//! * [`Guide`]s — guide lines and guide points — belong to a context too, but
//!   are construction geometry only: snapped to, never part of the model.
//! * [`Annotation`]s — dimensions and text — belong to a context as well and
//!   follow the geometry they are tied to; they are not geometry either.
//! * [`SectionPlane`]s belong to a context, at most one active; the active
//!   one cuts the view.
//! * [`Scene`]s — saved camera, mode and layer visibility — belong to the
//!   document.
//! * [`Material`]s belong to the document; a face side's [`Paint`] names one
//!   and places its texture, alone or through a context's [`Wrap`]. Edges
//!   carry an [`EdgeMark`] and an optional [`LineStyle`] for Present mode.
//! * [`Document`] owns the root context, [`Definition`]s for groups and
//!   components, and the undo/redo history.
//! * [`DocumentData`] is the document as plain, index-linked data, for the
//!   native file format (`smith-io`): [`Document::to_data`] and
//!   [`Document::from_data`] convert exactly in both directions.
//!
//! Dependency rule: depends only on `smith-geom`.

mod annotations;
mod arena;
mod autotri;
mod build;
mod copylook;
mod curves;
mod document;
mod edit;
mod entities;
mod erase;
mod error;
mod faces;
mod fuse;
mod grid;
mod guides;
mod ids;
mod import;
mod intersect;
mod journal;
mod look;
mod merge;
mod paint;
mod scenes;
mod sections;
mod selection;
mod shapes;
mod snapshot;
mod snapview;

pub use annotations::{Anchor, Annotation, AnnotationData, LeaderText};
pub use autotri::{auto_tri, with_auto_tri};
pub use copylook::CopiedLook;
pub use curves::CurveRecord;
pub use document::{
    Definition, DefinitionKind, Document, Instance, Layer, ModelLook, ON_LOCKED_LAYER, UNLOCK_LAYER,
};
pub use entities::{Edge, EdgeFlags, EdgeUse, Entities, Face, FaceAttributes, Vertex};
pub use error::ModelError;
pub use fuse::{BooleanOp, NOTHING_LEFT};
pub use guides::Guide;
pub use ids::{
    AnnotationId, CurveId, DefinitionId, EdgeId, FaceId, GuideId, InstanceId, LayerId, MaterialId,
    SceneId, SectionId, VertexId, WrapId,
};
pub use import::{ImportCounts, ImportedEdge};
pub use intersect::{Against, FaceCuts, FaceShape};
pub use look::{EdgeMark, LinePattern, LineStyle, PenWeight};
pub use paint::{
    Axis, ImageFormat, METAL_PRESETS, Mapping, Material, Paint, Projection, Side, Texture,
    TextureFrame, TextureImage, Wrap,
};
pub use scenes::{FaceDisplay, Scene, SceneData, SceneStyle, SceneView};
pub use sections::{SectionPlane, Slice};
pub use selection::{Pickable, SelectKind, SelectMode, Selection};
pub use shapes::{LoftOptions, ShapeSpec, SweepRecipe, SweepSection};
pub use snapshot::{
    CurveData, DefinitionData, DocumentData, EdgeData, EntitiesData, FaceData, InstanceData,
    LayerData, LoopUse, MappingData, PaintData,
};
pub use snapview::SnapView;
