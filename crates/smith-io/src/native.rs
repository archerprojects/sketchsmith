//! SketchSmith — native file format | v1.0.0 | Thursday 12 November 2026
//!
//! The native file is a binary glTF 2.0 file (`.glb`) with two layers:
//!
//! * **Viewer layer** — standard glTF that any viewer, Blender or browser shows:
//!   one mesh per context (the root and each definition) with a triangle
//!   primitive for faces and a line primitive for visible edges (hidden faces
//!   and edges, and anything on a layer switched off, are left out of this
//!   layer only; the model layer keeps everything), instances as
//!   nodes reusing their definition's mesh, two plain materials, and the
//!   current view as a camera. Mesh data is in mm, Z-up; a single root node
//!   scales by 0.001 and turns Z-up into Y-up, so consumers receive metres and
//!   Y-up. Positions here are `f32` and are never read back. Its export
//!   form (`plain_gltf`, for File → Export → glTF) draws each face once,
//!   double-sided, with no edges, camera or model layer.
//! * **Model layer** — the `SKETCHSMITH_model` extension, listed in
//!   `extensionsUsed` only, so other software ignores it. It holds the full
//!   topology, every curve kind, edge flags, groups and components, the drawing
//!   axes and the view. Bulk data (positions as `f64`, index lists) lives in its
//!   own bufferViews; small records are JSON. This layer alone rebuilds the
//!   model, exactly (`Document::from_data`).
//!
//! **Schema.** `schema` starts at 1. A file with a newer schema is read as far
//! as it can be and flagged [`Opened::newer`], so the app can warn before
//! saving over it; unknown fields are ignored, and a curve of an unknown kind
//! becomes plain edges.
//!
//! **Schema 2** adds layers: the document's `layers` list (name, colour,
//! visibility; Base first) and `active_layer` (the layer new geometry goes
//! on, by index; each layer's `number` is the one it was created with),
//! each instance's `layer`, `locked` and `hidden`, and per
//! context the bufferViews `edge_layers` and `face_layers` (one `u32` per
//! edge or face, an index into `layers`, 0 for Base) and `face_flags` (one
//! byte per face: 1 hidden). Each of these is written only when something is
//! off Base, hidden or locked. A schema 1 file reads with everything on Base.
//!
//! **Schema 11** adds the Present-mode look:
//! * `document.materials`, in creation order, only those some face side is
//!   painted with: `{"name", "color": [r, g, b] (linear), "opacity" (1 when
//!   absent), "metallic" (false when absent), "texture": {"image",
//!   "width_mm", "pixels": [w, h]}}`. `image` indexes the glTF `images`
//!   array, which holds each imported PNG or JPEG once, byte for byte, in its
//!   own bufferView; the viewer layer's textures use the same images.
//! * `document.smoothing_angle` in degrees (30 when absent).
//! * `present` on the extension root: the file was saved in Present mode
//!   and reopens in it (Edit when absent).
//! * A layer's `style`: `{"weight": mm (an ISO pen: 0.13, 0.18, 0.25, 0.35,
//!   0.5, 0.7 or 1.0), "pattern": "solid" | "dashed" | "dash_dot" |
//!   "dash_double_dot"}`.
//! * Per context: `wraps`, the projections faces are painted with
//!   (`{"projection": "box" | "cylinder" | "planar", "axis": "x" | "y" | "z",
//!   "origin", "basis": [u, v, w], "radius", "offset": [u, v], "rotation"}`,
//!   mm and radians, only those in use); `paints`, the distinct face paints
//!   (`{"material", "frame": {"origin", "u", "v"}}` or `{"material",
//!   "wrap"}`); the bufferView `face_paint`, two `u32` per face (front,
//!   back): 0 unpainted, else a `paints` index + 1; and the bufferView
//!   `edge_styles`, one byte per edge: 0 none, else 1 + weight × 4 +
//!   pattern (weight 0–6 thinnest first, pattern 0–3 as listed). Each is
//!   written only when something uses it.
//! * `edge_flags` gains 8 (marked Sharp) and 16 (marked Smooth).
//!
//! Older files open with no materials, no marks or styles, a 30° angle and
//! in Edit mode.
//!
//! **Schema 12** adds a layer's `locked` (written only when true): what is
//! on it cannot be selected or changed. Older files open with every layer
//! unlocked.
//!
//! **Schema 13** lets a dimension's or text's point be tied to an edge
//! (`edge` and `t`, 0–1 along it from its first end) or a face (`face` and
//! `uv`, mm in the face's frame: from its first corner along its first
//! edge, and across), besides a vertex. A build before it reads such a point
//! as fixed at its `at`. A leader's `label` is read from its anchor's `at`
//! (where the text stays), in every schema.
//!
//! **Schema 14** adds a loft recipe's `edges_only` (written only when true):
//! the loft is its rings and the lines along them, with no faces. Older
//! files open with every loft faced.
//!
//! **Schema 15** adds a loft recipe's `closed` (written only when true): its
//! path closes on itself and the loft runs round through every profile and
//! back into the first. Older files open with every loft path open.
//!
//! **Schema 16** adds a loft recipe's `twists` (radians, one per profile in
//! `profiles` order; written only when any is not 0): each profile's own
//! turn. Older files open with no profile turned.
//!
//! **Schema 17** adds the Skin recipe (Skinning): `sweep` as `{"kind":
//! "skin", "segments": [[[x, y, z], [x, y, z]], …], "sampling", and
//! "edges_only" (written only when true)}`. Builds before it cannot read
//! that kind and refuse the file; from it on, a recipe of a kind not known
//! is dropped with a warning and its group opens as plain geometry.
//!
//! **Bulk encodings** (all little-endian; every bufferView is 8-byte aligned):
//! * `positions` — `f64` x, y, z per vertex.
//! * a *ragged* list — `{offsets, items}`: `offsets` holds `n + 1` `u32`
//!   values and list `i` is `items[offsets[i]..offsets[i+1]]` (`u32`).
//! * `edge_vertices` — two `u32` per edge (start, end).
//! * `edge_flags` — one byte per edge: 1 soft, 2 smooth, 4 hidden, 8 marked
//!   Sharp, 16 marked Smooth.
//! * `edge_curves` — one `u32` per edge: 0 for none, else curve index + 1.
//! * `face_planes` — four `f64` per face: normal x, y, z and `d`.
//! * `face_loop_counts` — one `u32` per face: its outer loop plus holes.
//!   The loops themselves are a ragged list in face order, each item an edge
//!   use `edge << 1 | reversed`.
//!
//! **Extension root:** `schema`, `units` (always `"mm"`), `up` (always
//! `"Z"`), `generator`, `document`, and `view` and `axes` when present
//! (`axes` only when the drawing axes differ from the world axes).
//!
//! **`document`:** `root` (a context) and `definitions` (`name`, `kind`
//! `"group"` or `"component"`, `context`), in creation order; entities are
//! listed densely in creation order and referred to by index, so a re-save
//! is byte-identical. With schema 2, `layers` and `active_layer` as above.
//!
//! **A context:** `vertex_count`, `edge_count`, `face_count`; the bufferViews
//! above plus the ragged `vertex_edges`, `edge_faces` and `curve_edges`; the
//! JSON lists `curves` and `instances` (`definition`, a column-major 4 × 4
//! `f64` `matrix` in mm, `name`, and with schema 2 `layer`, `locked`,
//! `hidden`); `guides` (schema 3) and `annotations` (schema 4), both below.
//!
//! **Guides** (schema 3): a JSON list per context, in creation order, each
//! `{"kind": "line", "point": [x, y, z], "direction": [x, y, z]}` (an
//! endless line; `direction` a unit vector) or `{"kind": "point", "point":
//! [x, y, z]}`, in mm. A guide of an unknown kind is skipped with a warning;
//! a malformed one (non-finite, or a direction that is not a unit vector)
//! refuses the file. Older files have none.
//!
//! **Shape recipes** (schema 7): a definition made by the Shapes tool
//! carries `shape`, `{"kind": "box" | "cylinder" | "cone" | "sphere" |
//! "torus", …sizes}` (mm; see `RecipeExt`), until its geometry is edited by
//! hand. Older files have none.
//!
//! **Sweep recipes** (schema 8): a definition made by Sweep or Revolve
//! carries `sweep`, until its geometry is edited by hand. A sweep is
//! `{"kind": "sweep", "path": [[x, y, z], …], "closed", "profile", "up":
//! [x, y, z], "taper", "twist", "upright", "align"}` with `profile` one of
//! `{"kind": "outline", "points": [[x, y, z], …]}`, `{"kind": "round",
//! "radius", "sides"}` or `{"kind": "rect", "width", "height"}`; a revolve
//! is `{"kind": "revolve", "outline": [[x, y, z], …], "axis_point",
//! "axis_dir", "angle", "steps"}`. Points in mm in the definition's own
//! coordinates, angles in radians. Older files have none.
//!
//! **Loft recipes** (schema 9): a definition made by Loft along Path or
//! Loft carries `sweep` as `{"kind": "loft", "path": [[x, y, z],
//! …] or null, "profiles": [[[x, y, z], …], …], "up": [x, y, z],
//! "vertices", "start_shift", "segments", "twist"}`: along a path each
//! profile lies across it; with a null path (by spline) the profiles are in
//! loft order.
//!
//! **Second paths and bare edges** (schema 10): a sweep or a loft along a
//! path may carry `"path2": [[x, y, z], …]`, a second path its profiles
//! keep spanning; a sweep's profile may be `{"kind": "edges", "points":
//! [[x, y, z], …], "closed"}` (bare edges: a surface), and a loft's
//! profiles are bare edges when it carries `"edges"` (true for closed
//! chains, false for open ones). Older files have neither.
//!
//! **Section planes** (schema 6): `sections` per context, in creation
//! order, each `{"point": [x, y, z], "normal": [x, y, z], "active": true}`
//! (mm; the normal a unit vector toward the side cut away; `active` left out
//! when false, and true on at most one). A malformed plane, or two active,
//! refuses the file. A scene's `section` (schema 6) is the index of the
//! root's plane it makes active, and its `faces` (`wireframe`,
//! `hidden_line`, `shaded`, `monochrome`, `x_ray`; shaded when absent) and
//! `back_edges` the display style it shows. Scenes are called renders on
//! screen.
//!
//! **Scenes** (schema 5): `document.scenes`, in the order they were made,
//! each `{"name", "target": [x, y, z], "distance", "yaw", "pitch",
//! "perspective", "fov_y", "present", "faces", "back_edges",
//! "hidden_layers": [layer index, …]}`
//! (mm and radians; `present` and `hidden_layers` left out when false or
//! empty). A scene with an empty name, a camera that is not finite, or a
//! layer index past the end refuses the file. Older files have none.
//!
//! **Dimensions and text** (schema 4): a JSON list per context, in creation
//! order, each with a `kind`:
//! `linear` (`a`, `b`, `offset`), `radial` (`curve`, `diameter`, `centre`,
//! `radius`, `label`), `angular` (`centre`, `a`, `b`, `radius`), `leader`
//! (`anchor`, `label`, `text`) or `screen` (`at`, 0–1 across and down the
//! canvas, and `text`). A point they are tied to is `{"vertex": index,
//! "at": [x, y, z]}`, or `{"at": [x, y, z]}` when fixed; `at` is where the
//! vertex was when the annotation was made. Leader `text` is `{"show":
//! "text", "text"}`, `{"show": "length", "edge", "value"}`, `{"show":
//! "area", "face", "value"}` or `{"show": "coordinates"}`; `curve`, `edge`
//! and `face` are left out once that geometry is gone. Lengths in mm, areas
//! in mm². An unknown kind is skipped with a warning; a malformed one, or one
//! referring past the context's vertices, edges, faces or curves, refuses
//! the file.
//!
//! **Curve records:** `kind` plus its fields and `segments`. A `frame` is
//! `{center, x_axis, normal}`. `arc`: frame, radius, start and end angles
//! (radians); `circle`: frame, radius; `polygon`: frame, radius, sides;
//! `bezier`: control points (3n + 1); `polyline`: points, closed; `helix`:
//! frame, start and end radius, pitch, turns, left-handed.
//!
//! **`view`:** target, distance, yaw, pitch, projection, field of view (read
//! back), and the derived eye, up, half-height and aspect (for the glTF
//! camera only).
//!
//! **Exactness.** Every `f64`, in bufferViews and in JSON, reads back
//! bit-identical; planes and curve frames are restored as stored.
//!
//! **Reading checks.** Every index is checked against its list, then the
//! model's link invariants (vertex ↔ edge, edge ↔ face, loop continuity and
//! winding, curve ↔ edge, no definition containing itself). A file that
//! fails is refused as damaged; a broken model is never opened. Layers
//! without a `number` are numbered by position. A `.glb` without the
//! extension is not a SketchSmith model and is refused here (imports come
//! separately).
//!
//! **Fixtures.** `tests/fixtures/` holds five schema-1 files (kept as
//! written, to prove older files still open), one schema-2 file, one
//! schema-3 file with guides and one schema-4 file with dimensions and text;
//! each is checked against a freshly built model.
//!
//! The schema is laid out field by field in the encoder and decoder below.

use std::collections::BTreeMap;

use glam::{DMat3, DMat4, DQuat, DVec3};
use gltf_json as gj;
use gltf_json::validation::Checked::Valid;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use smith_geom::curve::{Arc, Bezier, Circle, Curve, CurveFrame, Helix, Polygon, Polyline};
use smith_model::{
    Anchor, Annotation, AnnotationData, Axis, CurveData, DefinitionData, DefinitionKind, Document,
    DocumentData, EdgeData, EdgeFlags, EdgeMark, EntitiesData, FaceData, FaceDisplay, Guide,
    ImageFormat, InstanceData, LayerData, LeaderText, LinePattern, LineStyle, LoftOptions, LoopUse,
    MappingData, Material, ModelLook, PaintData, PenWeight, Projection, SceneData, SceneStyle,
    SceneView, SectionPlane, ShapeSpec, SweepRecipe, SweepSection, Texture, TextureFrame,
    TextureImage, Wrap,
};

use crate::error::IoError;
use crate::glb;

/// Name of the model-layer extension.
pub const EXTENSION: &str = "SKETCHSMITH_model";

/// Schema version this build writes and fully understands.
pub const SCHEMA: u32 = 17;

/// The camera as the canvas keeps it, plus the derived pose used for the
/// glTF camera. Units are mm and radians, Z-up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedView {
    /// Point looked at and orbited around.
    pub target: DVec3,
    /// Eye-to-target distance.
    pub distance: f64,
    /// Rotation about +Z.
    pub yaw: f64,
    /// Elevation above the XY plane.
    pub pitch: f64,
    /// Perspective (`true`) or parallel projection.
    pub perspective: bool,
    /// Vertical field of view (perspective).
    pub fov_y: f64,
    /// Eye position (derived).
    pub eye: DVec3,
    /// Screen-up direction (derived).
    pub up: DVec3,
    /// Half the visible height at the target (parallel projection).
    pub half_height: f64,
    /// Canvas width / height when saved.
    pub aspect: f64,
}

/// Drawing axes: origin and three unit directions, mm, Z-up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedAxes {
    /// Origin.
    pub origin: DVec3,
    /// Red axis.
    pub x: DVec3,
    /// Green axis.
    pub y: DVec3,
    /// Blue axis.
    pub z: DVec3,
}

/// Everything a native file holds.
#[derive(Debug, Clone, Default)]
pub struct NativeFile {
    /// The model.
    pub document: Document,
    /// The view when saved.
    pub view: Option<SavedView>,
    /// The drawing axes, if moved from the world axes.
    pub axes: Option<SavedAxes>,
    /// Saved in Present mode: reopen in it.
    pub present: bool,
}

/// A file that was read, and how it may be used.
#[derive(Debug)]
pub struct Opened {
    /// Its contents.
    pub file: NativeFile,
    /// The file was written by a newer SketchSmith; saving over it may drop
    /// data this version does not know.
    pub newer: bool,
    /// Messages for the user (unknown curve kinds…).
    pub warnings: Vec<String>,
}

// ----- extension JSON --------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct ModelExt {
    schema: u32,
    units: String,
    up: String,
    #[serde(default)]
    generator: String,
    document: DocExt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    view: Option<ViewExt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    axes: Option<AxesExt>,
    /// Saved in Present mode (schema 11).
    #[serde(default, skip_serializing_if = "is_false")]
    present: bool,
}

#[derive(Serialize, Deserialize)]
struct DocExt {
    root: ContextExt,
    #[serde(default)]
    definitions: Vec<DefExt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    layers: Vec<LayerExt>,
    #[serde(default, skip_serializing_if = "is_zero")]
    active_layer: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    scenes: Vec<SceneExt>,
    /// Materials in use (schema 11).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    materials: Vec<MaterialExt>,
    /// Smoothing angle in degrees (schema 11); 30 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    smoothing_angle: Option<f64>,
}

/// A material as stored (schema 11).
#[derive(Serialize, Deserialize)]
struct MaterialExt {
    name: String,
    color: [f32; 3],
    #[serde(default = "opaque", skip_serializing_if = "is_opaque")]
    opacity: f32,
    #[serde(default, skip_serializing_if = "is_false")]
    metallic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    texture: Option<TextureExt>,
}

/// A material's texture: a glTF image index, its width and pixel size.
#[derive(Serialize, Deserialize)]
struct TextureExt {
    image: u32,
    width_mm: f64,
    pixels: [u32; 2],
}

fn opaque() -> f32 {
    1.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_opaque(v: &f32) -> bool {
    *v == 1.0
}

/// A line style as stored: the pen weight in mm and the pattern by name.
#[derive(Serialize, Deserialize, Clone, Copy)]
struct LineStyleExt {
    weight: f64,
    pattern: PatternExt,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum PatternExt {
    Solid,
    Dashed,
    DashDot,
    DashDoubleDot,
}

impl From<LineStyle> for LineStyleExt {
    fn from(s: LineStyle) -> Self {
        Self {
            weight: s.weight.mm(),
            pattern: match s.pattern {
                LinePattern::Solid => PatternExt::Solid,
                LinePattern::Dashed => PatternExt::Dashed,
                LinePattern::DashDot => PatternExt::DashDot,
                LinePattern::DashDoubleDot => PatternExt::DashDoubleDot,
            },
        }
    }
}

impl LineStyleExt {
    fn style(self) -> Result<LineStyle, IoError> {
        let weight = PenWeight::ALL
            .into_iter()
            .find(|w| (w.mm() - self.weight).abs() < 1e-9)
            .ok_or_else(|| IoError::Damaged(format!("{} mm is not a pen weight", self.weight)))?;
        Ok(LineStyle {
            weight,
            pattern: match self.pattern {
                PatternExt::Solid => LinePattern::Solid,
                PatternExt::Dashed => LinePattern::Dashed,
                PatternExt::DashDot => LinePattern::DashDot,
                PatternExt::DashDoubleDot => LinePattern::DashDoubleDot,
            },
        })
    }
}

/// A wrap as stored (schema 11).
#[derive(Serialize, Deserialize)]
struct WrapExt {
    projection: ProjectionExt,
    axis: AxisExt,
    origin: [f64; 3],
    basis: [[f64; 3]; 3],
    radius: f64,
    offset: [f64; 2],
    rotation: f64,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum ProjectionExt {
    Box,
    Cylinder,
    Planar,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum AxisExt {
    X,
    Y,
    Z,
}

impl From<&Wrap> for WrapExt {
    fn from(w: &Wrap) -> Self {
        Self {
            projection: match w.projection {
                Projection::Box => ProjectionExt::Box,
                Projection::Cylinder => ProjectionExt::Cylinder,
                Projection::Planar => ProjectionExt::Planar,
            },
            axis: match w.axis {
                Axis::X => AxisExt::X,
                Axis::Y => AxisExt::Y,
                Axis::Z => AxisExt::Z,
            },
            origin: arr(w.origin),
            basis: w.basis.map(arr),
            radius: w.radius,
            offset: w.offset.to_array(),
            rotation: w.rotation,
        }
    }
}

impl From<WrapExt> for Wrap {
    fn from(w: WrapExt) -> Self {
        Self {
            projection: match w.projection {
                ProjectionExt::Box => Projection::Box,
                ProjectionExt::Cylinder => Projection::Cylinder,
                ProjectionExt::Planar => Projection::Planar,
            },
            axis: match w.axis {
                AxisExt::X => Axis::X,
                AxisExt::Y => Axis::Y,
                AxisExt::Z => Axis::Z,
            },
            origin: vec3(w.origin),
            basis: w.basis.map(vec3),
            radius: w.radius,
            offset: glam::DVec2::from_array(w.offset),
            rotation: w.rotation,
        }
    }
}

/// A distinct face paint as stored: a material and either its own frame or
/// a wrap.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
struct PaintExt {
    material: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frame: Option<TexFrameExt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wrap: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
struct TexFrameExt {
    origin: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
}

impl From<PaintData> for PaintExt {
    fn from(p: PaintData) -> Self {
        match p.mapping {
            MappingData::Face(f) => Self {
                material: p.material,
                frame: Some(TexFrameExt {
                    origin: arr(f.origin),
                    u: arr(f.u),
                    v: arr(f.v),
                }),
                wrap: None,
            },
            MappingData::Wrap(w) => Self {
                material: p.material,
                frame: None,
                wrap: Some(w),
            },
        }
    }
}

impl PaintExt {
    fn paint(self) -> Result<PaintData, IoError> {
        let mapping = match (self.frame, self.wrap) {
            (Some(f), None) => MappingData::Face(TextureFrame {
                origin: vec3(f.origin),
                u: vec3(f.u),
                v: vec3(f.v),
            }),
            (None, Some(w)) => MappingData::Wrap(w),
            _ => {
                return Err(IoError::Damaged(
                    "a face paint needs a frame or a wrap".into(),
                ));
            }
        };
        Ok(PaintData {
            material: self.material,
            mapping,
        })
    }
}

/// A scene's face display as stored.
#[derive(Serialize, Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum FacesExt {
    Wireframe,
    HiddenLine,
    #[default]
    Shaded,
    Monochrome,
    XRay,
}

impl From<FaceDisplay> for FacesExt {
    fn from(f: FaceDisplay) -> Self {
        match f {
            FaceDisplay::Wireframe => FacesExt::Wireframe,
            FaceDisplay::HiddenLine => FacesExt::HiddenLine,
            FaceDisplay::Shaded => FacesExt::Shaded,
            FaceDisplay::Monochrome => FacesExt::Monochrome,
            FaceDisplay::XRay => FacesExt::XRay,
        }
    }
}

impl From<FacesExt> for FaceDisplay {
    fn from(f: FacesExt) -> Self {
        match f {
            FacesExt::Wireframe => FaceDisplay::Wireframe,
            FacesExt::HiddenLine => FaceDisplay::HiddenLine,
            FacesExt::Shaded => FaceDisplay::Shaded,
            FacesExt::Monochrome => FaceDisplay::Monochrome,
            FacesExt::XRay => FaceDisplay::XRay,
        }
    }
}

/// A scene as stored (schema 5): the camera in mm and radians, the mode,
/// and the layers it hides as indices into `layers`.
#[derive(Serialize, Deserialize)]
struct SceneExt {
    name: String,
    target: [f64; 3],
    distance: f64,
    yaw: f64,
    pitch: f64,
    perspective: bool,
    fov_y: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    present: bool,
    /// Display style (schema 6): `wireframe`, `hidden_line`, `shaded`,
    /// `monochrome` or `x_ray`; shaded when absent.
    #[serde(default)]
    faces: FacesExt,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    back_edges: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    hidden_layers: Vec<u32>,
    /// The root's section plane made active (schema 6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    section: Option<u32>,
}

#[derive(Serialize, Deserialize)]
struct LayerExt {
    name: String,
    /// Absent in files from before numbering: then the layer's position.
    #[serde(default)]
    number: Option<u32>,
    color: [u8; 3],
    #[serde(default = "shown")]
    visible: bool,
    /// Present-mode line style (schema 11).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    style: Option<LineStyleExt>,
    /// Locked (schema 12).
    #[serde(default, skip_serializing_if = "is_false")]
    locked: bool,
}

fn shown() -> bool {
    true
}

/// Whether `layers` is just the Base layer as a new document has it.
fn only_base(layers: &[LayerData]) -> bool {
    let base = smith_model::Layer::base();
    match layers {
        [] => true,
        [l] => {
            l.name == base.name
                && l.color == base.color
                && l.visible == base.visible
                && l.style.is_none()
                && !l.locked
        }
        _ => false,
    }
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

fn all_zero(v: &[f64]) -> bool {
    v.iter().all(|x| *x == 0.0)
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Serialize, Deserialize)]
struct DefExt {
    name: String,
    kind: String,
    /// The shape it was made as (schema 7), while not edited by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shape: Option<RecipeExt>,
    /// What it was made from by Sweep or Revolve (schema 8), while not
    /// edited by hand. A kind not known (a newer schema's) is dropped.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "known_sweep"
    )]
    sweep: Option<SweepExt>,
    context: ContextExt,
}

/// A definition's recipe, or none when it is of a kind this build does not
/// know (its group keeps its geometry).
fn known_sweep<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<SweepExt>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok(value.and_then(|v| match serde_json::from_value(v) {
        Ok(s) => Some(s),
        Err(e) => {
            log::warn!("recipe dropped: {e}");
            None
        }
    }))
}

/// A sweep or revolve recipe as stored (see the module documentation).
#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SweepExt {
    Sweep {
        path: Vec<[f64; 3]>,
        closed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path2: Option<Vec<[f64; 3]>>,
        profile: ProfileExt,
        up: [f64; 3],
        taper: f64,
        twist: f64,
        upright: bool,
        align: bool,
    },
    Loft {
        path: Option<Vec<[f64; 3]>>,
        profiles: Vec<Vec<[f64; 3]>>,
        up: [f64; 3],
        vertices: u32,
        start_shift: u32,
        segments: u32,
        twist: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path2: Option<Vec<[f64; 3]>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edges: Option<bool>,
        /// Edges Only (schema 14).
        #[serde(default, skip_serializing_if = "is_false")]
        edges_only: bool,
        /// The path closes on itself (schema 15).
        #[serde(default, skip_serializing_if = "is_false")]
        closed: bool,
        /// Each profile's own turn (schema 16).
        #[serde(default, skip_serializing_if = "all_zero")]
        twists: Vec<f64>,
    },
    Revolve {
        outline: Vec<[f64; 3]>,
        axis_point: [f64; 3],
        axis_dir: [f64; 3],
        angle: f64,
        steps: u32,
    },
    Skin {
        segments: Vec<[[f64; 3]; 2]>,
        sampling: u32,
        #[serde(default, skip_serializing_if = "is_false")]
        edges_only: bool,
    },
}

/// A sweep's profile as stored.
#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProfileExt {
    Outline { points: Vec<[f64; 3]> },
    Edges { points: Vec<[f64; 3]>, closed: bool },
    Round { radius: f64, sides: u32 },
    Rect { width: f64, height: f64 },
}

fn points_ext(pts: &[DVec3]) -> Vec<[f64; 3]> {
    pts.iter().map(|p| p.to_array()).collect()
}

fn points_of(pts: Vec<[f64; 3]>) -> Vec<DVec3> {
    pts.into_iter().map(DVec3::from_array).collect()
}

impl From<&SweepRecipe> for SweepExt {
    fn from(r: &SweepRecipe) -> Self {
        match r {
            SweepRecipe::Sweep {
                path,
                closed,
                path2,
                section,
                up,
                taper,
                twist,
                upright,
                align,
            } => SweepExt::Sweep {
                path: points_ext(path),
                closed: *closed,
                path2: path2.as_deref().map(points_ext),
                profile: match section {
                    SweepSection::Outline(pts) => ProfileExt::Outline {
                        points: points_ext(pts),
                    },
                    SweepSection::Edges { points, closed } => ProfileExt::Edges {
                        points: points_ext(points),
                        closed: *closed,
                    },
                    SweepSection::Round { radius, sides } => ProfileExt::Round {
                        radius: *radius,
                        sides: *sides,
                    },
                    SweepSection::Rect { width, height } => ProfileExt::Rect {
                        width: *width,
                        height: *height,
                    },
                },
                up: up.to_array(),
                taper: *taper,
                twist: *twist,
                upright: *upright,
                align: *align,
            },
            SweepRecipe::Loft {
                path,
                profiles,
                up,
                options,
                path2,
                edges,
                closed,
                twists,
            } => SweepExt::Loft {
                path: path.as_deref().map(points_ext),
                profiles: profiles.iter().map(|o| points_ext(o)).collect(),
                up: up.to_array(),
                vertices: options.vertices,
                start_shift: options.start_shift,
                segments: options.segments,
                twist: options.twist,
                path2: path2.as_deref().map(points_ext),
                edges: *edges,
                edges_only: options.edges_only,
                closed: *closed,
                twists: twists.clone(),
            },
            SweepRecipe::Revolve {
                outline,
                axis_point,
                axis_dir,
                angle,
                steps,
            } => SweepExt::Revolve {
                outline: points_ext(outline),
                axis_point: axis_point.to_array(),
                axis_dir: axis_dir.to_array(),
                angle: *angle,
                steps: *steps,
            },
            SweepRecipe::Skin {
                segments,
                sampling,
                edges_only,
            } => SweepExt::Skin {
                segments: segments
                    .iter()
                    .map(|[a, b]| [a.to_array(), b.to_array()])
                    .collect(),
                sampling: *sampling,
                edges_only: *edges_only,
            },
        }
    }
}

impl From<SweepExt> for SweepRecipe {
    fn from(r: SweepExt) -> Self {
        match r {
            SweepExt::Sweep {
                path,
                closed,
                path2,
                profile,
                up,
                taper,
                twist,
                upright,
                align,
            } => SweepRecipe::Sweep {
                path: points_of(path),
                closed,
                path2: path2.map(points_of),
                section: match profile {
                    ProfileExt::Outline { points } => SweepSection::Outline(points_of(points)),
                    ProfileExt::Edges { points, closed } => SweepSection::Edges {
                        points: points_of(points),
                        closed,
                    },
                    ProfileExt::Round { radius, sides } => SweepSection::Round { radius, sides },
                    ProfileExt::Rect { width, height } => SweepSection::Rect { width, height },
                },
                up: DVec3::from_array(up),
                taper,
                twist,
                upright,
                align,
            },
            SweepExt::Loft {
                path,
                profiles,
                up,
                vertices,
                start_shift,
                segments,
                twist,
                path2,
                edges,
                edges_only,
                closed,
                twists,
            } => SweepRecipe::Loft {
                path: path.map(points_of),
                profiles: profiles.into_iter().map(points_of).collect(),
                up: DVec3::from_array(up),
                options: LoftOptions {
                    vertices,
                    start_shift,
                    segments,
                    twist,
                    edges_only,
                },
                path2: path2.map(points_of),
                edges,
                closed,
                twists,
            },
            SweepExt::Revolve {
                outline,
                axis_point,
                axis_dir,
                angle,
                steps,
            } => SweepRecipe::Revolve {
                outline: points_of(outline),
                axis_point: DVec3::from_array(axis_point),
                axis_dir: DVec3::from_array(axis_dir),
                angle,
                steps,
            },
            SweepExt::Skin {
                segments,
                sampling,
                edges_only,
            } => SweepRecipe::Skin {
                segments: segments
                    .into_iter()
                    .map(|[a, b]| [DVec3::from_array(a), DVec3::from_array(b)])
                    .collect(),
                sampling,
                edges_only,
            },
        }
    }
}

/// A shape recipe as stored: `{"kind": "box", "width", "depth", "height"}`,
/// `{"kind": "cylinder" | "cone", "radius", "height", "sides"}`,
/// `{"kind": "sphere", "radius", "segments"}` or `{"kind": "torus", "ring",
/// "tube", "segments", "sides"}`, in mm.
#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RecipeExt {
    Box {
        width: f64,
        depth: f64,
        height: f64,
    },
    Cylinder {
        radius: f64,
        height: f64,
        sides: u32,
    },
    Cone {
        radius: f64,
        height: f64,
        sides: u32,
    },
    Sphere {
        radius: f64,
        segments: u32,
    },
    Torus {
        ring: f64,
        tube: f64,
        segments: u32,
        sides: u32,
    },
}

impl From<ShapeSpec> for RecipeExt {
    fn from(s: ShapeSpec) -> Self {
        match s {
            ShapeSpec::Box {
                width,
                depth,
                height,
            } => RecipeExt::Box {
                width,
                depth,
                height,
            },
            ShapeSpec::Cylinder {
                radius,
                height,
                sides,
            } => RecipeExt::Cylinder {
                radius,
                height,
                sides,
            },
            ShapeSpec::Cone {
                radius,
                height,
                sides,
            } => RecipeExt::Cone {
                radius,
                height,
                sides,
            },
            ShapeSpec::Sphere { radius, segments } => RecipeExt::Sphere { radius, segments },
            ShapeSpec::Torus {
                ring,
                tube,
                segments,
                sides,
            } => RecipeExt::Torus {
                ring,
                tube,
                segments,
                sides,
            },
        }
    }
}

impl From<RecipeExt> for ShapeSpec {
    fn from(s: RecipeExt) -> Self {
        match s {
            RecipeExt::Box {
                width,
                depth,
                height,
            } => ShapeSpec::Box {
                width,
                depth,
                height,
            },
            RecipeExt::Cylinder {
                radius,
                height,
                sides,
            } => ShapeSpec::Cylinder {
                radius,
                height,
                sides,
            },
            RecipeExt::Cone {
                radius,
                height,
                sides,
            } => ShapeSpec::Cone {
                radius,
                height,
                sides,
            },
            RecipeExt::Sphere { radius, segments } => ShapeSpec::Sphere { radius, segments },
            RecipeExt::Torus {
                ring,
                tube,
                segments,
                sides,
            } => ShapeSpec::Torus {
                ring,
                tube,
                segments,
                sides,
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct Ragged {
    offsets: u32,
    items: u32,
}

#[derive(Serialize, Deserialize, Default)]
struct ContextExt {
    vertex_count: u32,
    edge_count: u32,
    face_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    positions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vertex_edges: Option<Ragged>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edge_vertices: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edge_flags: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edge_curves: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edge_faces: Option<Ragged>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edge_layers: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face_layers: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face_flags: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face_planes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face_loop_counts: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    loops: Option<Ragged>,
    #[serde(default)]
    curves: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve_edges: Option<Ragged>,
    #[serde(default)]
    instances: Vec<InstanceExt>,
    /// Guide lines and guide points ([`GuideExt`]), kept as raw values so
    /// a kind from a newer schema is skipped rather than refusing the file.
    #[serde(default)]
    guides: Vec<Value>,
    /// Dimensions and text ([`AnnotationExt`]), kept as raw values so a
    /// kind from a newer schema is skipped rather than refusing the file.
    #[serde(default)]
    annotations: Vec<Value>,
    /// Section planes (schema 6).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sections: Vec<SectionExt>,
    /// Wraps in use (schema 11).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    wraps: Vec<WrapExt>,
    /// Distinct face paints (schema 11).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    paints: Vec<PaintExt>,
    /// Two `u32` per face: front and back paint, 0 none, else index + 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face_paint: Option<u32>,
    /// One byte per edge: 0 no style, else 1 + weight × 4 + pattern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edge_styles: Option<u32>,
}

/// A guide as stored: `{"kind": "line", "point", "direction"}` or
/// `{"kind": "point", "point"}`, in mm.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum GuideExt {
    Line {
        point: [f64; 3],
        direction: [f64; 3],
    },
    Point {
        point: [f64; 3],
    },
}

impl From<&Guide> for GuideExt {
    fn from(g: &Guide) -> Self {
        match g {
            Guide::Line { point, direction } => GuideExt::Line {
                point: point.to_array(),
                direction: direction.to_array(),
            },
            Guide::Point { point } => GuideExt::Point {
                point: point.to_array(),
            },
        }
    }
}

impl GuideExt {
    /// The guide, exactly as stored (a line's direction is not
    /// renormalised, so it reads back bit for bit).
    fn guide(&self) -> Guide {
        match self {
            GuideExt::Line { point, direction } => Guide::Line {
                point: DVec3::from_array(*point),
                direction: DVec3::from_array(*direction),
            },
            GuideExt::Point { point } => Guide::Point {
                point: DVec3::from_array(*point),
            },
        }
    }
}

/// A section plane as stored: a point (mm), the unit normal toward the side
/// cut away, and whether it is the context's active plane.
#[derive(Serialize, Deserialize)]
struct SectionExt {
    point: [f64; 3],
    normal: [f64; 3],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    active: bool,
}

/// Where an annotation is tied: `{"vertex": index, "at": [x, y, z]}`,
/// `{"edge": index, "t": t, "at": …}` or `{"face": index, "uv": [u, v],
/// "at": …}` (schema 13), or `{"at": [x, y, z]}`.
#[derive(Default, Serialize, Deserialize)]
struct AnchorExt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vertex: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edge: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    t: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uv: Option<[f64; 2]>,
    at: [f64; 3],
}

impl From<&Anchor<u32, u32, u32>> for AnchorExt {
    fn from(a: &Anchor<u32, u32, u32>) -> Self {
        let at = a.fallback().to_array();
        match *a {
            Anchor::Vertex(v, _) => AnchorExt {
                vertex: Some(v),
                at,
                ..AnchorExt::default()
            },
            Anchor::Edge(e, t, _) => AnchorExt {
                edge: Some(e),
                t: Some(t),
                at,
                ..AnchorExt::default()
            },
            Anchor::Face(f, uv, _) => AnchorExt {
                face: Some(f),
                uv: Some(uv.to_array()),
                at,
                ..AnchorExt::default()
            },
            Anchor::Point(_) => AnchorExt {
                at,
                ..AnchorExt::default()
            },
        }
    }
}

impl AnchorExt {
    fn anchor(&self) -> Anchor<u32, u32, u32> {
        let at = DVec3::from_array(self.at);
        if let Some(v) = self.vertex {
            return Anchor::Vertex(v, at);
        }
        if let (Some(e), Some(t)) = (self.edge, self.t) {
            return Anchor::Edge(e, t, at);
        }
        if let (Some(f), Some(uv)) = (self.face, self.uv) {
            return Anchor::Face(f, smith_geom::DVec2::from_array(uv), at);
        }
        Anchor::Point(at)
    }
}

/// What leader text shows.
#[derive(Serialize, Deserialize)]
#[serde(tag = "show", rename_all = "snake_case")]
enum LeaderTextExt {
    Text {
        text: String,
    },
    Length {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edge: Option<u32>,
        value: f64,
    },
    Area {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<u32>,
        value: f64,
    },
    Coordinates,
}

/// A dimension or text as stored, in mm (references are indices into the
/// context's vertices, edges, faces and curves).
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AnnotationExt {
    Linear {
        a: AnchorExt,
        b: AnchorExt,
        offset: [f64; 3],
    },
    Radial {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        curve: Option<u32>,
        diameter: bool,
        centre: [f64; 3],
        radius: f64,
        label: [f64; 3],
    },
    Angular {
        centre: AnchorExt,
        a: AnchorExt,
        b: AnchorExt,
        radius: f64,
    },
    Leader {
        anchor: AnchorExt,
        label: [f64; 3],
        text: LeaderTextExt,
    },
    Screen {
        at: [f64; 2],
        text: String,
    },
}

impl From<&AnnotationData> for AnnotationExt {
    fn from(a: &AnnotationData) -> Self {
        match a {
            Annotation::Linear { a, b, offset } => AnnotationExt::Linear {
                a: a.into(),
                b: b.into(),
                offset: offset.to_array(),
            },
            Annotation::Radial {
                curve,
                diameter,
                centre,
                radius,
                label,
            } => AnnotationExt::Radial {
                curve: *curve,
                diameter: *diameter,
                centre: centre.to_array(),
                radius: *radius,
                label: label.to_array(),
            },
            Annotation::Angular {
                centre,
                a,
                b,
                radius,
            } => AnnotationExt::Angular {
                centre: centre.into(),
                a: a.into(),
                b: b.into(),
                radius: *radius,
            },
            Annotation::Leader {
                anchor,
                label,
                text,
            } => AnnotationExt::Leader {
                anchor: anchor.into(),
                label: label.to_array(),
                text: match text {
                    LeaderText::Custom(t) => LeaderTextExt::Text { text: t.clone() },
                    LeaderText::Length { edge, value } => LeaderTextExt::Length {
                        edge: *edge,
                        value: *value,
                    },
                    LeaderText::Area { face, value } => LeaderTextExt::Area {
                        face: *face,
                        value: *value,
                    },
                    LeaderText::Coordinates => LeaderTextExt::Coordinates,
                },
            },
            Annotation::Screen { at, text } => AnnotationExt::Screen {
                at: *at,
                text: text.clone(),
            },
        }
    }
}

impl AnnotationExt {
    fn annotation(&self) -> AnnotationData {
        let v3 = DVec3::from_array;
        match self {
            AnnotationExt::Linear { a, b, offset } => Annotation::Linear {
                a: a.anchor(),
                b: b.anchor(),
                offset: v3(*offset),
            },
            AnnotationExt::Radial {
                curve,
                diameter,
                centre,
                radius,
                label,
            } => Annotation::Radial {
                curve: *curve,
                diameter: *diameter,
                centre: v3(*centre),
                radius: *radius,
                label: v3(*label),
            },
            AnnotationExt::Angular {
                centre,
                a,
                b,
                radius,
            } => Annotation::Angular {
                centre: centre.anchor(),
                a: a.anchor(),
                b: b.anchor(),
                radius: *radius,
            },
            AnnotationExt::Leader {
                anchor,
                label,
                text,
            } => Annotation::Leader {
                anchor: anchor.anchor(),
                label: v3(*label),
                text: match text {
                    LeaderTextExt::Text { text } => LeaderText::Custom(text.clone()),
                    LeaderTextExt::Length { edge, value } => LeaderText::Length {
                        edge: *edge,
                        value: *value,
                    },
                    LeaderTextExt::Area { face, value } => LeaderText::Area {
                        face: *face,
                        value: *value,
                    },
                    LeaderTextExt::Coordinates => LeaderText::Coordinates,
                },
            },
            AnnotationExt::Screen { at, text } => Annotation::Screen {
                at: *at,
                text: text.clone(),
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
struct FrameExt {
    center: [f64; 3],
    x_axis: [f64; 3],
    normal: [f64; 3],
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ShapeExt {
    Arc {
        frame: FrameExt,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    Circle {
        frame: FrameExt,
        radius: f64,
    },
    Polygon {
        frame: FrameExt,
        radius: f64,
        sides: u32,
    },
    Bezier {
        control_points: Vec<[f64; 3]>,
    },
    Polyline {
        points: Vec<[f64; 3]>,
        closed: bool,
    },
    Helix {
        frame: FrameExt,
        start_radius: f64,
        end_radius: f64,
        pitch: f64,
        turns: f64,
        left_handed: bool,
    },
}

#[derive(Serialize, Deserialize)]
struct CurveExt {
    #[serde(flatten)]
    shape: ShapeExt,
    segments: u32,
}

#[derive(Serialize, Deserialize)]
struct InstanceExt {
    definition: u32,
    /// Column-major 4 × 4, mm.
    matrix: [f64; 16],
    #[serde(default)]
    name: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    layer: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    locked: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    hidden: bool,
}

#[derive(Serialize, Deserialize)]
struct ViewExt {
    target: [f64; 3],
    distance: f64,
    yaw: f64,
    pitch: f64,
    projection: String,
    fov_y: f64,
    eye: [f64; 3],
    up: [f64; 3],
    half_height: f64,
    aspect: f64,
}

#[derive(Serialize, Deserialize)]
struct AxesExt {
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    z: [f64; 3],
}

// ----- binary payload --------------------------------------------------------

/// Builds the BIN chunk and its bufferViews.
#[derive(Default)]
struct Bin {
    data: Vec<u8>,
    views: Vec<gj::buffer::View>,
}

impl Bin {
    /// Adds one bufferView (8-byte aligned) and returns its index.
    fn view(&mut self, bytes: &[u8], target: Option<gj::buffer::Target>, name: &str) -> u32 {
        self.data.resize(self.data.len().div_ceil(8) * 8, 0);
        let offset = self.data.len();
        self.data.extend_from_slice(bytes);
        let index = u32::try_from(self.views.len()).unwrap_or(u32::MAX);
        self.views.push(gj::buffer::View {
            buffer: gj::Index::new(0),
            byte_length: bytes.len().into(),
            byte_offset: Some(offset.into()),
            byte_stride: None,
            name: Some(name.into()),
            target: target.map(Valid),
            extensions: None,
            extras: Default::default(),
        });
        index
    }

    fn f64s(&mut self, values: &[f64], name: &str) -> Option<u32> {
        (!values.is_empty()).then(|| {
            let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
            self.view(&bytes, None, name)
        })
    }

    fn u32s(&mut self, values: &[u32], name: &str) -> Option<u32> {
        (!values.is_empty()).then(|| {
            let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
            self.view(&bytes, None, name)
        })
    }

    fn ragged(&mut self, lists: &[Vec<u32>], name: &str) -> Option<Ragged> {
        if lists.is_empty() {
            return None;
        }
        let mut offsets = Vec::with_capacity(lists.len() + 1);
        let mut items = Vec::new();
        offsets.push(0u32);
        for l in lists {
            items.extend_from_slice(l);
            offsets.push(u32::try_from(items.len()).unwrap_or(u32::MAX));
        }
        let offsets = self.u32s(&offsets, &format!("{name} offsets"))?;
        // An all-empty ragged list still needs an items view; one zero word.
        let items = if items.is_empty() {
            self.view(&[0; 4], None, &format!("{name} items"))
        } else {
            self.u32s(&items, &format!("{name} items"))?
        };
        Some(Ragged { offsets, items })
    }
}

fn arr(v: DVec3) -> [f64; 3] {
    v.to_array()
}

fn vec3(a: [f64; 3]) -> DVec3 {
    DVec3::from_array(a)
}

fn frame_ext(f: &CurveFrame) -> FrameExt {
    FrameExt {
        center: arr(f.center()),
        x_axis: arr(f.x_axis()),
        normal: arr(f.normal()),
    }
}

fn shape_ext(c: &Curve) -> ShapeExt {
    match c {
        Curve::Arc(a) => ShapeExt::Arc {
            frame: frame_ext(a.frame()),
            radius: a.radius(),
            start_angle: a.start_angle(),
            end_angle: a.end_angle(),
        },
        Curve::Circle(c) => ShapeExt::Circle {
            frame: frame_ext(c.frame()),
            radius: c.radius(),
        },
        Curve::Polygon(p) => ShapeExt::Polygon {
            frame: frame_ext(p.frame()),
            radius: p.radius(),
            sides: p.sides(),
        },
        Curve::Bezier(b) => ShapeExt::Bezier {
            control_points: b.control_points().iter().map(|p| arr(*p)).collect(),
        },
        Curve::Polyline(p) => ShapeExt::Polyline {
            points: p.points().iter().map(|p| arr(*p)).collect(),
            closed: p.is_closed(),
        },
        Curve::Helix(h) => ShapeExt::Helix {
            frame: frame_ext(h.frame()),
            start_radius: h.start_radius(),
            end_radius: h.end_radius(),
            pitch: h.pitch(),
            turns: h.turns(),
            left_handed: h.left_handed(),
        },
    }
}

fn flag_byte(f: EdgeFlags) -> u8 {
    u8::from(f.soft)
        | (u8::from(f.smooth) << 1)
        | (u8::from(f.hidden) << 2)
        | (u8::from(f.mark == EdgeMark::Sharp) << 3)
        | (u8::from(f.mark == EdgeMark::Smooth) << 4)
}

fn style_byte(s: Option<LineStyle>) -> u8 {
    s.map_or(0, |s| 1 + s.weight.index() * 4 + s.pattern.index())
}

fn style_of(b: u8) -> Result<Option<LineStyle>, IoError> {
    if b == 0 {
        return Ok(None);
    }
    let i = b - 1;
    match (PenWeight::from_index(i / 4), LinePattern::from_index(i % 4)) {
        (Some(weight), Some(pattern)) => Ok(Some(LineStyle { weight, pattern })),
        _ => Err(IoError::Damaged(format!("unknown line style {b}"))),
    }
}

fn encode_use(u: LoopUse) -> u32 {
    (u.edge << 1) | u32::from(u.reversed)
}

fn context_ext(e: &EntitiesData, bin: &mut Bin, label: &str) -> Result<ContextExt, IoError> {
    let count = |n: usize| u32::try_from(n).map_err(|_| IoError::TooLarge);
    let positions: Vec<f64> = e.positions.iter().flat_map(|p| p.to_array()).collect();
    let edge_vertices: Vec<u32> = e.edges.iter().flat_map(|x| x.vertices).collect();
    let flags: Vec<u8> = e.edges.iter().map(|x| flag_byte(x.flags)).collect();
    let curves_of: Vec<u32> = e
        .edges
        .iter()
        .map(|x| x.curve.map_or(0, |c| c + 1))
        .collect();
    let edge_faces: Vec<Vec<u32>> = e.edges.iter().map(|x| x.faces.clone()).collect();
    let planes: Vec<f64> = e
        .faces
        .iter()
        .flat_map(|f| [f.normal.x, f.normal.y, f.normal.z, f.d])
        .collect();
    let mut loop_counts = Vec::with_capacity(e.faces.len());
    let mut loops = Vec::new();
    for f in &e.faces {
        loop_counts.push(count(1 + f.inner.len())?);
        loops.push(f.outer.iter().copied().map(encode_use).collect());
        for l in &f.inner {
            loops.push(l.iter().copied().map(encode_use).collect());
        }
    }
    let curve_edges: Vec<Vec<u32>> = e.curves.iter().map(|c| c.edges.clone()).collect();
    let edge_layers: Vec<u32> = e.edges.iter().map(|x| x.layer).collect();
    let face_layers: Vec<u32> = e.faces.iter().map(|f| f.layer).collect();
    let face_flags: Vec<u8> = e.faces.iter().map(|f| u8::from(f.hidden)).collect();
    let mut paints: Vec<PaintExt> = Vec::new();
    let mut slot = |p: Option<PaintData>| -> u32 {
        let Some(p) = p.map(PaintExt::from) else {
            return 0;
        };
        let i = paints.iter().position(|q| *q == p).unwrap_or_else(|| {
            paints.push(p);
            paints.len() - 1
        });
        u32::try_from(i + 1).unwrap_or(u32::MAX)
    };
    let face_paint: Vec<u32> = e
        .faces
        .iter()
        .flat_map(|f| [slot(f.front), slot(f.back)])
        .collect();
    let edge_styles: Vec<u8> = e.edges.iter().map(|x| style_byte(x.flags.style)).collect();
    let curves = e
        .curves
        .iter()
        .map(|c| {
            serde_json::to_value(CurveExt {
                shape: shape_ext(&c.curve),
                segments: c.segments,
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(ContextExt {
        vertex_count: count(e.positions.len())?,
        edge_count: count(e.edges.len())?,
        face_count: count(e.faces.len())?,
        positions: bin.f64s(&positions, &format!("{label} positions")),
        vertex_edges: bin.ragged(&e.vertex_edges, &format!("{label} vertex edges")),
        edge_vertices: bin.u32s(&edge_vertices, &format!("{label} edge vertices")),
        edge_flags: (!flags.is_empty())
            .then(|| bin.view(&flags, None, &format!("{label} edge flags"))),
        edge_curves: bin.u32s(&curves_of, &format!("{label} edge curves")),
        edge_faces: bin.ragged(&edge_faces, &format!("{label} edge faces")),
        edge_layers: edge_layers
            .iter()
            .any(|l| *l != 0)
            .then(|| bin.u32s(&edge_layers, &format!("{label} edge layers")))
            .flatten(),
        face_layers: face_layers
            .iter()
            .any(|l| *l != 0)
            .then(|| bin.u32s(&face_layers, &format!("{label} face layers")))
            .flatten(),
        face_flags: face_flags
            .iter()
            .any(|f| *f != 0)
            .then(|| bin.view(&face_flags, None, &format!("{label} face flags"))),
        face_planes: bin.f64s(&planes, &format!("{label} face planes")),
        face_loop_counts: bin.u32s(&loop_counts, &format!("{label} face loop counts")),
        loops: bin.ragged(&loops, &format!("{label} loops")),
        curves,
        curve_edges: bin.ragged(&curve_edges, &format!("{label} curve edges")),
        instances: e
            .instances
            .iter()
            .map(|i| InstanceExt {
                definition: i.definition,
                matrix: i.transform.to_cols_array(),
                name: i.name.clone(),
                layer: i.layer,
                locked: i.locked,
                hidden: i.hidden,
            })
            .collect(),
        guides: e
            .guides
            .iter()
            .map(|g| serde_json::to_value(GuideExt::from(g)))
            .collect::<Result<_, _>>()?,
        annotations: e
            .annotations
            .iter()
            .map(|a| serde_json::to_value(AnnotationExt::from(a)))
            .collect::<Result<_, _>>()?,
        sections: e
            .sections
            .iter()
            .map(|s| SectionExt {
                point: s.point.to_array(),
                normal: s.normal.to_array(),
                active: s.active,
            })
            .collect(),
        wraps: e.wraps.iter().map(WrapExt::from).collect(),
        face_paint: face_paint
            .iter()
            .any(|p| *p != 0)
            .then(|| bin.u32s(&face_paint, &format!("{label} face paint")))
            .flatten(),
        paints,
        edge_styles: edge_styles
            .iter()
            .any(|b| *b != 0)
            .then(|| bin.view(&edge_styles, None, &format!("{label} edge styles"))),
    })
}

// ----- viewer layer ----------------------------------------------------------

/// The plain face and the edge materials: always the first two.
const FACE_MATERIAL: u32 = 0;
const EDGE_MATERIAL: u32 = 1;
/// Plain face colours, linear (the canvas's front and, in Edit mode, back:
/// golden yellow, so a face turned the wrong way shows in any viewer).
const FACE_COLOR: [f32; 4] = [0.62, 0.60, 0.56, 1.0];
const FACE_BACK_COLOR: [f32; 4] = [0.791, 0.456, 0.007, 1.0];

/// The Z-up mm → Y-up m conversion: (x, y, z) mm becomes (x, z, −y) m.
fn to_gltf_world(p: DVec3) -> DVec3 {
    DVec3::new(p.x, p.z, -p.y)
}

fn root_conversion_matrix() -> [f32; 16] {
    let m = DMat4::from_cols_array(&[
        1.0, 0.0, 0.0, 0.0, // image of +X
        0.0, 0.0, -1.0, 0.0, // image of +Y
        0.0, 1.0, 0.0, 0.0, // image of +Z
        0.0, 0.0, 0.0, 1.0,
    ]) * DMat4::from_scale(DVec3::splat(0.001));
    m.to_cols_array().map(|v| v as f32)
}

struct Viewer {
    root: gj::Root,
    bin: Bin,
    /// Mesh of each context: index 0 is the root, `i + 1` definition `i`.
    meshes: Vec<Option<u32>>,
    /// Each saved material's index, by key.
    material_index: BTreeMap<smith_model::MaterialId, u32>,
    /// The saved materials, and the glTF texture of each one's image.
    materials: Vec<(Material, Option<u32>)>,
    /// glTF material of each slot, made when first used.
    slots: BTreeMap<Slot, u32>,
    /// The plain export form: each face once, double-sided, with the front's
    /// paint, else the back's; no edges.
    plain: bool,
}

/// Which glTF material a set of face triangles is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    /// Unpainted on both sides: the plain double-sided face material.
    Plain,
    /// The unpainted front of a face painted on its back.
    PlainFront,
    /// The unpainted back of a face painted on its front.
    PlainBack,
    /// A saved material (by index), double-sided or not.
    Painted(u32, bool),
}

/// One face primitive being built.
#[derive(Default)]
struct Prim {
    points: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    tris: Vec<u32>,
}

impl Viewer {
    fn accessor(
        &mut self,
        view: u32,
        count: usize,
        ty: gj::accessor::Type,
        component: gj::accessor::ComponentType,
        bounds: Option<(Value, Value)>,
    ) -> gj::Index<gj::Accessor> {
        let (min, max) = bounds.map_or((None, None), |(a, b)| (Some(a), Some(b)));
        gj::Index::push(
            &mut self.root.accessors,
            gj::Accessor {
                buffer_view: Some(gj::Index::new(view)),
                byte_offset: None,
                count: count.into(),
                component_type: Valid(gj::accessor::GenericComponentType(component)),
                extensions: None,
                extras: Default::default(),
                type_: Valid(ty),
                min,
                max,
                name: None,
                normalized: false,
                sparse: None,
            },
        )
    }

    fn positions(&mut self, points: &[[f32; 3]], name: &str) -> gj::Index<gj::Accessor> {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in points {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let bytes: Vec<u8> = points
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let view = self
            .bin
            .view(&bytes, Some(gj::buffer::Target::ArrayBuffer), name);
        self.accessor(
            view,
            points.len(),
            gj::accessor::Type::Vec3,
            gj::accessor::ComponentType::F32,
            Some((serde_json::json!(lo), serde_json::json!(hi))),
        )
    }

    fn vec3s(&mut self, values: &[[f32; 3]], name: &str) -> gj::Index<gj::Accessor> {
        let bytes: Vec<u8> = values
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let view = self
            .bin
            .view(&bytes, Some(gj::buffer::Target::ArrayBuffer), name);
        self.accessor(
            view,
            values.len(),
            gj::accessor::Type::Vec3,
            gj::accessor::ComponentType::F32,
            None,
        )
    }

    fn vec2s(&mut self, values: &[[f32; 2]], name: &str) -> gj::Index<gj::Accessor> {
        let bytes: Vec<u8> = values
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let view = self
            .bin
            .view(&bytes, Some(gj::buffer::Target::ArrayBuffer), name);
        self.accessor(
            view,
            values.len(),
            gj::accessor::Type::Vec2,
            gj::accessor::ComponentType::F32,
            None,
        )
    }

    /// The saved index of material `id`.
    fn material_index(&self, id: smith_model::MaterialId) -> u32 {
        self.material_index.get(&id).copied().unwrap_or(0)
    }

    /// One texture repeat in mm for material `id`, if it has a texture.
    fn texture_size(&self, id: smith_model::MaterialId) -> Option<glam::DVec2> {
        let (m, _) = self.materials.get(self.material_index(id) as usize)?;
        m.texture.as_ref().map(Texture::size_mm)
    }

    /// The glTF material for `slot`, made the first time it is needed.
    fn slot_material(&mut self, slot: Slot) -> u32 {
        if let Some(i) = self.slots.get(&slot) {
            return *i;
        }
        let made = match slot {
            Slot::Plain => return FACE_MATERIAL,
            Slot::PlainFront => gj::Material {
                double_sided: false,
                ..material("Face front", FACE_COLOR)
            },
            Slot::PlainBack => gj::Material {
                double_sided: false,
                ..material("Face back", FACE_BACK_COLOR)
            },
            Slot::Painted(i, double) => {
                let Some((m, texture)) = self.materials.get(i as usize) else {
                    return FACE_MATERIAL;
                };
                // glTF takes base colours within 0–1.
                let [r, g, b] = m.color.map(|c| c.min(1.0));
                let mut out = material(&m.name, [r, g, b, m.opacity]);
                out.double_sided = double;
                if m.opacity < 1.0 {
                    out.alpha_mode = Valid(gj::material::AlphaMode::Blend);
                }
                let pbr = &mut out.pbr_metallic_roughness;
                pbr.metallic_factor =
                    gj::material::StrengthFactor(if m.metallic { 1.0 } else { 0.0 });
                // No roughness control yet: a middle value.
                pbr.roughness_factor = gj::material::StrengthFactor(0.5);
                pbr.base_color_texture = texture.map(|t| gj::texture::Info {
                    index: gj::Index::new(t),
                    tex_coord: 0,
                    extensions: None,
                    extras: Default::default(),
                });
                out
            }
        };
        let i = u32::try_from(self.root.materials.len()).unwrap_or(u32::MAX);
        self.root.materials.push(made);
        self.slots.insert(slot, i);
        i
    }

    fn indices(&mut self, values: &[u32], name: &str) -> gj::Index<gj::Accessor> {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self
            .bin
            .view(&bytes, Some(gj::buffer::Target::ElementArrayBuffer), name);
        self.accessor(
            view,
            values.len(),
            gj::accessor::Type::Scalar,
            gj::accessor::ComponentType::U32,
            None,
        )
    }

    /// One mesh for a context: triangles for faces, lines for visible edges.
    fn mesh(&mut self, doc: &Document, def: Option<smith_model::DefinitionId>, name: &str) {
        let entities = match def {
            None => Some(doc.root()),
            Some(id) => doc.definition_entities(id),
        };
        let Some(e) = entities else {
            self.meshes.push(None);
            return;
        };
        self.mesh_of(doc, e, None, name);
    }

    /// One mesh for the faces of `e` (only those in `only`, when given).
    fn mesh_of(
        &mut self,
        doc: &Document,
        e: &smith_model::Entities,
        only: Option<&[smith_model::FaceId]>,
        name: &str,
    ) {
        let f32x3 = |p: DVec3| [p.x as f32, p.y as f32, p.z as f32];
        let mut primitives = Vec::new();

        // Faces, grouped into one primitive per glTF material. A face plain
        // on both sides, or painted the same on both, is drawn once with a
        // double-sided material; otherwise each side is drawn with its own
        // single-sided material (the back as reversed triangles), so viewers
        // show what Present mode shows. Normals are the smoothed ones.
        let angle = Some(doc.look().smoothing_angle_deg.to_radians().cos());
        let mut prims: BTreeMap<Slot, Prim> = BTreeMap::new();
        for f in e.face_ids() {
            let Some(face) = e.face(f) else { continue };
            if face.attributes.hidden
                || !doc.layer_visible(face.attributes.layer)
                || only.is_some_and(|o| !o.contains(&f))
            {
                continue;
            }
            let n = face.plane().normal();
            let normals = e.corner_normals(f, angle);
            let corners: Vec<smith_model::VertexId> =
                face.loops().flat_map(|l| e.loop_vertices(l)).collect();
            let points: Vec<DVec3> = corners
                .iter()
                .map(|v| {
                    e.vertex(*v)
                        .map_or(DVec3::ZERO, smith_model::Vertex::position)
                })
                .collect();
            let tris = e.face_triangles(f);
            let front = face.attributes.front;
            let back = face.attributes.back;
            // Each side drawn: its paint, whether its triangles are
            // reversed, its material, and whether its texture is laid out
            // for the back.
            let sides: Vec<(Option<smith_model::Paint>, bool, Slot, bool)> = if self.plain {
                let (paint, from_back) = match (front, back) {
                    (Some(p), _) => (Some(p), false),
                    (None, Some(p)) => (Some(p), true),
                    (None, None) => (None, false),
                };
                let slot = paint.map_or(Slot::Plain, |p| {
                    Slot::Painted(self.material_index(p.material), true)
                });
                vec![(paint, false, slot, from_back)]
            } else if front.is_none() && back.is_none() {
                vec![(None, false, Slot::Plain, false)]
            } else if front.is_some() && front == back {
                let m = front.map(|p| self.material_index(p.material)).unwrap_or(0);
                vec![(front, false, Slot::Painted(m, true), false)]
            } else {
                let slot = |p: Option<smith_model::Paint>, back: bool| match p {
                    Some(p) => Slot::Painted(self.material_index(p.material), false),
                    None if back => Slot::PlainBack,
                    None => Slot::PlainFront,
                };
                vec![
                    (front, false, slot(front, false), false),
                    (back, true, slot(back, true), true),
                ]
            };
            for (paint, reversed, slot, uv_back) in sides {
                let side_n = if uv_back { -n } else { n };
                // Texture coordinates for the whole face at once, so a wrap
                // round a cylinder stays continuous across it.
                let uvs: Option<Vec<[f32; 2]>> = paint.and_then(|p| {
                    let size = self.texture_size(p.material)?;
                    let mm = e.paint_uvs(&p, side_n, &points)?;
                    Some(
                        mm.iter()
                            .map(|t| [(t.x / size.x) as f32, (-t.y / size.y) as f32])
                            .collect(),
                    )
                });
                let prim = prims.entry(slot).or_default();
                let mut index: BTreeMap<smith_model::VertexId, u32> = BTreeMap::new();
                for t in &tris {
                    let order = if reversed { [0, 2, 1] } else { [0, 1, 2] };
                    for k in order {
                        let p = t[k];
                        let Some(ci) = points.iter().position(|q| q.to_array() == p.to_array())
                        else {
                            continue;
                        };
                        let v = corners[ci];
                        let i = *index.entry(v).or_insert_with(|| {
                            let nv = normals.get(&v).copied().unwrap_or(n);
                            let nv = if reversed { -nv } else { nv };
                            prim.points.push(f32x3(p));
                            prim.normals.push(f32x3(nv));
                            if let Some(u) = &uvs {
                                prim.uvs.push(u[ci]);
                            }
                            u32::try_from(prim.points.len() - 1).unwrap_or(u32::MAX)
                        });
                        prim.tris.push(i);
                    }
                }
                // Untextured faces in a textured primitive get 0, 0.
                prim.uvs.resize(
                    if prim.uvs.is_empty() {
                        0
                    } else {
                        prim.points.len()
                    },
                    [0.0; 2],
                );
            }
        }
        for (slot, prim) in prims {
            if prim.tris.is_empty() {
                continue;
            }
            let pos = self.positions(&prim.points, &format!("{name} face positions"));
            let nor = self.vec3s(&prim.normals, &format!("{name} face normals"));
            let idx = self.indices(&prim.tris, &format!("{name} face indices"));
            let mut attributes: BTreeMap<_, _> = [
                (Valid(gj::mesh::Semantic::Positions), pos),
                (Valid(gj::mesh::Semantic::Normals), nor),
            ]
            .into_iter()
            .collect();
            if prim.uvs.len() == prim.points.len() && !prim.uvs.is_empty() {
                let uv = self.vec2s(&prim.uvs, &format!("{name} face uvs"));
                attributes.insert(Valid(gj::mesh::Semantic::TexCoords(0)), uv);
            }
            let material = self.slot_material(slot);
            primitives.push(gj::mesh::Primitive {
                attributes,
                extensions: None,
                extras: Default::default(),
                indices: Some(idx),
                material: Some(gj::Index::new(material)),
                mode: Valid(gj::mesh::Mode::Triangles),
                targets: None,
            });
        }

        // Edges that are drawn (not soft, not hidden); none in the plain form.
        let mut vindex: BTreeMap<smith_model::VertexId, u32> = BTreeMap::new();
        let mut line_points = Vec::new();
        let mut lines = Vec::new();
        for id in e.edge_ids().filter(|_| !self.plain) {
            let Some(edge) = e.edge(id) else { continue };
            // As Present mode shows them: smoothed edges are not drawn.
            if edge.flags.soft
                || edge.flags.hidden
                || !doc.layer_visible(edge.flags.layer)
                || angle.is_some_and(|c| e.edge_is_smooth(id, c))
            {
                continue;
            }
            for v in edge.vertices() {
                let i = *vindex.entry(v).or_insert_with(|| {
                    let p = e
                        .vertex(v)
                        .map_or(DVec3::ZERO, smith_model::Vertex::position);
                    line_points.push(f32x3(p));
                    u32::try_from(line_points.len() - 1).unwrap_or(u32::MAX)
                });
                lines.push(i);
            }
        }
        if !lines.is_empty() {
            let pos = self.positions(&line_points, &format!("{name} edge positions"));
            let idx = self.indices(&lines, &format!("{name} edge indices"));
            primitives.push(gj::mesh::Primitive {
                attributes: [(Valid(gj::mesh::Semantic::Positions), pos)]
                    .into_iter()
                    .collect(),
                extensions: None,
                extras: Default::default(),
                indices: Some(idx),
                material: Some(gj::Index::new(EDGE_MATERIAL)),
                mode: Valid(gj::mesh::Mode::Lines),
                targets: None,
            });
        }

        if primitives.is_empty() {
            self.meshes.push(None);
            return;
        }
        let index = gj::Index::push(
            &mut self.root.meshes,
            gj::Mesh {
                extensions: None,
                extras: Default::default(),
                name: Some(name.into()),
                primitives,
                weights: None,
            },
        );
        self.meshes.push(Some(index.value() as u32));
    }

    fn node(&mut self, node: gj::Node) -> gj::Index<gj::Node> {
        gj::Index::push(&mut self.root.nodes, node)
    }

    /// Nodes for the instances in a context (recursively; models cannot
    /// contain themselves, so this ends).
    fn instance_nodes(
        &mut self,
        doc: &Document,
        def: Option<smith_model::DefinitionId>,
        def_index: &BTreeMap<smith_model::DefinitionId, usize>,
    ) -> Vec<gj::Index<gj::Node>> {
        let entities = match def {
            None => Some(doc.root()),
            Some(id) => doc.definition_entities(id),
        };
        let Some(e) = entities else {
            return Vec::new();
        };
        self.instance_nodes_of(doc, e, None, def_index)
    }

    /// Nodes for the instances of `e` (only those in `only`, when given).
    fn instance_nodes_of(
        &mut self,
        doc: &Document,
        e: &smith_model::Entities,
        only: Option<&[smith_model::InstanceId]>,
        def_index: &BTreeMap<smith_model::DefinitionId, usize>,
    ) -> Vec<gj::Index<gj::Node>> {
        let mut out = Vec::new();
        for id in e.instance_ids() {
            if only.is_some_and(|o| !o.contains(&id)) {
                continue;
            }
            let Some(inst) = e.instance(id) else { continue };
            if inst.hidden || !doc.layer_visible(inst.layer) {
                continue;
            }
            let children = self.instance_nodes(doc, Some(inst.definition), def_index);
            let mesh = def_index
                .get(&inst.definition)
                .and_then(|i| self.meshes[i + 1])
                .map(gj::Index::new);
            let name = (!inst.name.is_empty())
                .then(|| inst.name.clone())
                .or_else(|| doc.definition(inst.definition).map(|d| d.name.clone()));
            out.push(self.node(gj::Node {
                children: (!children.is_empty()).then_some(children),
                matrix: Some(inst.transform.to_cols_array().map(|v| v as f32)),
                mesh,
                name,
                ..empty_node()
            }));
        }
        out
    }
}

fn empty_node() -> gj::Node {
    gj::Node {
        camera: None,
        children: None,
        extensions: None,
        extras: Default::default(),
        matrix: None,
        mesh: None,
        name: None,
        rotation: None,
        scale: None,
        translation: None,
        skin: None,
        weights: None,
    }
}

fn material(name: &str, color: [f32; 4]) -> gj::Material {
    gj::Material {
        alpha_cutoff: None,
        alpha_mode: Valid(gj::material::AlphaMode::Opaque),
        double_sided: true,
        name: Some(name.into()),
        pbr_metallic_roughness: gj::material::PbrMetallicRoughness {
            base_color_factor: gj::material::PbrBaseColorFactor(color),
            base_color_texture: None,
            metallic_factor: gj::material::StrengthFactor(0.0),
            roughness_factor: gj::material::StrengthFactor(1.0),
            metallic_roughness_texture: None,
            extensions: None,
            extras: Default::default(),
        },
        normal_texture: None,
        occlusion_texture: None,
        emissive_texture: None,
        emissive_factor: gj::material::EmissiveFactor([0.0; 3]),
        extensions: None,
        extras: Default::default(),
    }
}

/// The view as a glTF camera node at scene level (metres, Y-up, no scale).
fn camera_node(v: &SavedView, root: &mut gj::Root) -> Option<gj::Node> {
    let forward = to_gltf_world((v.target - v.eye).try_normalize()?);
    let up = to_gltf_world(v.up).try_normalize()?;
    let right = forward.cross(up).try_normalize()?;
    let up = right.cross(forward);
    let q = DQuat::from_mat3(&DMat3::from_cols(right, up, -forward)).normalize();
    let eye = to_gltf_world(v.eye) * 0.001;
    let distance_m = (v.distance * 0.001).max(1e-6);
    let camera = if v.perspective {
        gj::Camera {
            name: Some("View".into()),
            orthographic: None,
            perspective: Some(gj::camera::Perspective {
                aspect_ratio: (v.aspect > 0.0).then_some(v.aspect as f32),
                yfov: v.fov_y as f32,
                zfar: None,
                znear: (distance_m * 1e-3).max(1e-5) as f32,
                extensions: None,
                extras: Default::default(),
            }),
            type_: Valid(gj::camera::Type::Perspective),
            extensions: None,
            extras: Default::default(),
        }
    } else {
        let ymag = (v.half_height * 0.001).max(1e-6);
        gj::Camera {
            name: Some("View".into()),
            orthographic: Some(gj::camera::Orthographic {
                xmag: (ymag * v.aspect.max(1e-3)) as f32,
                ymag: ymag as f32,
                zfar: (distance_m * 1000.0).max(1000.0) as f32,
                znear: 0.0,
                extensions: None,
                extras: Default::default(),
            }),
            perspective: None,
            type_: Valid(gj::camera::Type::Orthographic),
            extensions: None,
            extras: Default::default(),
        }
    };
    let cam = gj::Index::push(&mut root.cameras, camera);
    Some(gj::Node {
        camera: Some(cam),
        name: Some("View".into()),
        rotation: Some(gj::scene::UnitQuaternion([
            q.x as f32, q.y as f32, q.z as f32, q.w as f32,
        ])),
        translation: Some([eye.x as f32, eye.y as f32, eye.z as f32]),
        ..empty_node()
    })
}

// ----- writing ---------------------------------------------------------------

/// Encodes a native file as `.glb` bytes.
///
/// # Errors
/// [`IoError::TooLarge`] beyond the format's limits.
pub fn write_glb(file: &NativeFile) -> Result<Vec<u8>, IoError> {
    write_parts(
        &file.document,
        file.view.as_ref(),
        file.axes.as_ref(),
        file.present,
    )
}

/// The images a viewer layer refers to, each once.
type Images = Vec<std::sync::Arc<TextureImage>>;

/// Names image `i` as a file beside the exported file.
pub(crate) type ImageUri<'a> = &'a dyn Fn(usize, &TextureImage) -> String;

/// The start of a viewer layer for `data` (of `doc`): the plain face and
/// edge materials, each image used and each material. Images go in the
/// binary chunk, or, with `image_uri` (the plain export form), are named by
/// it as files beside the file.
fn viewer_start(
    doc: &Document,
    data: &DocumentData,
    image_uri: Option<ImageUri<'_>>,
) -> Result<(Viewer, Vec<MaterialExt>, Images), IoError> {
    let mut root = gj::Root {
        asset: gj::Asset {
            copyright: None,
            extensions: None,
            extras: Default::default(),
            generator: Some(generator()),
            min_version: None,
            version: "2.0".into(),
        },
        ..Default::default()
    };
    root.materials.push(material("Face", FACE_COLOR));
    root.materials.push(material("Edge", [0.0, 0.0, 0.0, 1.0]));
    let mut viewer = Viewer {
        root,
        bin: Bin::default(),
        meshes: Vec::new(),
        material_index: BTreeMap::new(),
        materials: Vec::new(),
        slots: BTreeMap::new(),
        plain: image_uri.is_some(),
    };

    // Images: each imported image once, shared by both layers.
    let mut images: Vec<std::sync::Arc<TextureImage>> = Vec::new();
    let mut materials = Vec::with_capacity(data.materials.len());
    for m in &data.materials {
        let texture = match &m.texture {
            Some(t) => {
                let i = images
                    .iter()
                    .position(|x| std::sync::Arc::ptr_eq(x, &t.image) || **x == *t.image)
                    .unwrap_or_else(|| {
                        images.push(t.image.clone());
                        images.len() - 1
                    });
                Some(TextureExt {
                    image: u32::try_from(i).map_err(|_| IoError::TooLarge)?,
                    width_mm: t.width_mm,
                    pixels: t.image.pixels,
                })
            }
            None => None,
        };
        materials.push(MaterialExt {
            name: m.name.clone(),
            color: m.color,
            opacity: m.opacity,
            metallic: m.metallic,
            texture,
        });
    }
    for (i, image) in images.iter().enumerate() {
        let (buffer_view, mime_type, uri) = match image_uri {
            Some(uri) => (None, None, Some(uri(i, image))),
            None => {
                let view = viewer.bin.view(&image.bytes, None, &format!("image {i}"));
                let mime = gj::image::MimeType(image.format.mime().into());
                (Some(gj::Index::new(view)), Some(mime), None)
            }
        };
        viewer.root.images.push(gj::Image {
            buffer_view,
            mime_type,
            name: Some(image.name.clone()),
            uri,
            extensions: None,
            extras: Default::default(),
        });
        // One texture per image, all repeating with mipmaps.
        viewer.root.textures.push(gj::Texture {
            name: None,
            sampler: Some(gj::Index::new(0)),
            source: gj::Index::new(u32::try_from(i).unwrap_or(u32::MAX)),
            extensions: None,
            extras: Default::default(),
        });
    }
    if !images.is_empty() {
        viewer.root.samplers.push(gj::texture::Sampler {
            mag_filter: Some(Valid(gj::texture::MagFilter::Linear)),
            min_filter: Some(Valid(gj::texture::MinFilter::LinearMipmapLinear)),
            name: None,
            wrap_s: Valid(gj::texture::WrappingMode::Repeat),
            wrap_t: Valid(gj::texture::WrappingMode::Repeat),
            extensions: None,
            extras: Default::default(),
        });
    }
    viewer.materials = data
        .materials
        .iter()
        .zip(&materials)
        .map(|(m, ext)| (m.clone(), ext.texture.as_ref().map(|t| t.image)))
        .collect();
    viewer.material_index = doc
        .material_ids()
        .filter(|id| {
            doc.material(*id)
                .is_some_and(|m| data.materials.iter().any(|d| d.name == m.name))
        })
        .enumerate()
        .map(|(i, id)| (id, u32::try_from(i).unwrap_or(u32::MAX)))
        .collect();

    Ok((viewer, materials, images))
}

/// Encodes a document, view, axes and mode (`present`: saved in Present
/// mode) as `.glb` bytes without first gathering them into a [`NativeFile`]
/// (saving from the application).
///
/// # Errors
/// [`IoError::TooLarge`] beyond the format's limits.
pub fn write_parts(
    doc: &Document,
    view: Option<&SavedView>,
    axes: Option<&SavedAxes>,
    present: bool,
) -> Result<Vec<u8>, IoError> {
    let data = doc.to_data();
    let (mut viewer, materials, _) = viewer_start(doc, &data, None)?;

    // Viewer layer.
    let def_ids: Vec<_> = doc.definition_ids().collect();
    let def_index: BTreeMap<_, _> = def_ids.iter().enumerate().map(|(i, d)| (*d, i)).collect();
    viewer.mesh(doc, None, "Model");
    for id in &def_ids {
        let name = doc
            .definition(*id)
            .map_or_else(String::new, |d| d.name.clone());
        viewer.mesh(doc, Some(*id), &name);
    }
    let mut children = Vec::new();
    if let Some(m) = viewer.meshes[0] {
        children.push(viewer.node(gj::Node {
            mesh: Some(gj::Index::new(m)),
            name: Some("Model".into()),
            ..empty_node()
        }));
    }
    children.extend(viewer.instance_nodes(doc, None, &def_index));
    let top = viewer.node(gj::Node {
        children: (!children.is_empty()).then_some(children),
        matrix: Some(root_conversion_matrix()),
        name: Some("SketchSmith model (mm, Z-up)".into()),
        ..empty_node()
    });
    let mut scene_nodes = vec![top];
    if let Some(v) = view
        && let Some(node) = camera_node(v, &mut viewer.root)
    {
        scene_nodes.push(viewer.node(node));
    }
    viewer.root.scenes.push(gj::Scene {
        extensions: None,
        extras: Default::default(),
        name: Some("Scene".into()),
        nodes: scene_nodes,
    });
    viewer.root.scene = Some(gj::Index::new(0));

    // Model layer.
    let ext = ModelExt {
        schema: SCHEMA,
        units: "mm".into(),
        up: "Z".into(),
        generator: generator(),
        document: DocExt {
            root: context_ext(&data.root, &mut viewer.bin, "root")?,
            definitions: data
                .definitions
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    Ok(DefExt {
                        name: d.name.clone(),
                        kind: match d.kind {
                            DefinitionKind::Group => "group",
                            DefinitionKind::Component => "component",
                        }
                        .into(),
                        shape: d.shape.map(RecipeExt::from),
                        sweep: d.sweep.as_ref().map(SweepExt::from),
                        context: context_ext(&d.entities, &mut viewer.bin, &format!("def {i}"))?,
                    })
                })
                .collect::<Result<_, IoError>>()?,
            active_layer: data.active_layer,
            scenes: data
                .scenes
                .iter()
                .map(|s| SceneExt {
                    name: s.name.clone(),
                    target: s.view.target.to_array(),
                    distance: s.view.distance,
                    yaw: s.view.yaw,
                    pitch: s.view.pitch,
                    perspective: s.view.perspective,
                    fov_y: s.view.fov_y,
                    present: s.present,
                    faces: s.style.faces.into(),
                    back_edges: s.style.back_edges,
                    hidden_layers: s.hidden_layers.clone(),
                    section: s.section,
                })
                .collect(),
            // Only Base, as a new document has it: nothing to write.
            layers: if only_base(&data.layers) {
                Vec::new()
            } else {
                data.layers
                    .iter()
                    .map(|l| LayerExt {
                        name: l.name.clone(),
                        number: Some(l.number),
                        color: l.color,
                        visible: l.visible,
                        style: l.style.map(LineStyleExt::from),
                        locked: l.locked,
                    })
                    .collect()
            },
            materials,
            smoothing_angle: (data.look != ModelLook::default())
                .then_some(data.look.smoothing_angle_deg),
        },
        view: view.map(|v| ViewExt {
            target: arr(v.target),
            distance: v.distance,
            yaw: v.yaw,
            pitch: v.pitch,
            projection: if v.perspective {
                "perspective"
            } else {
                "orthographic"
            }
            .into(),
            fov_y: v.fov_y,
            eye: arr(v.eye),
            up: arr(v.up),
            half_height: v.half_height,
            aspect: v.aspect,
        }),
        axes: axes.map(|a| AxesExt {
            origin: arr(a.origin),
            x: arr(a.x),
            y: arr(a.y),
            z: arr(a.z),
        }),
        present,
    };
    let mut others = serde_json::Map::new();
    others.insert(EXTENSION.into(), serde_json::to_value(ext)?);
    viewer.root.extensions = Some(gj::extensions::root::Root { others });
    viewer.root.extensions_used.push(EXTENSION.into());

    let Viewer { mut root, bin, .. } = viewer;
    if !bin.data.is_empty() {
        root.buffers.push(gj::Buffer {
            byte_length: glb::pad4(bin.data.len()).into(),
            name: None,
            uri: None,
            extensions: None,
            extras: Default::default(),
        });
    }
    root.buffer_views = bin.views;
    let json = serde_json::to_vec(&root)?;
    glb::write(&json, &bin.data)
}

/// The plain glTF of what `scope` exports, named `name`: the viewer layer
/// in its export form (each face once, double-sided, no edges), without the
/// model extension or a camera. Returns the JSON, which refers to the
/// binary data as `bin_uri` and to each image by `image_uri`, the binary
/// data, and the images.
///
/// # Errors
/// [`IoError::TooLarge`] beyond the format's limits.
pub(crate) fn plain_gltf(
    doc: &Document,
    scope: crate::export::Scope<'_>,
    name: &str,
    bin_uri: &str,
    image_uri: ImageUri<'_>,
) -> Result<(Vec<u8>, Vec<u8>, Images), IoError> {
    let data = doc.to_data();
    let (mut viewer, _, images) = viewer_start(doc, &data, Some(image_uri))?;
    let (entities, faces, instances, local) = match scope {
        crate::export::Scope::Model => (Some(doc.root()), None, None, DMat4::IDENTITY),
        crate::export::Scope::Selection {
            context,
            local,
            faces,
            instances,
        } => (
            match context {
                None => Some(doc.root()),
                Some(id) => doc.definition_entities(id),
            },
            Some(faces),
            Some(instances),
            local,
        ),
    };
    let def_ids: Vec<_> = doc.definition_ids().collect();
    let def_index: BTreeMap<_, _> = def_ids.iter().enumerate().map(|(i, d)| (*d, i)).collect();
    match entities {
        Some(e) => viewer.mesh_of(doc, e, faces, name),
        None => viewer.meshes.push(None),
    }
    for id in &def_ids {
        let def_name = doc
            .definition(*id)
            .map_or_else(String::new, |d| d.name.clone());
        viewer.mesh(doc, Some(*id), &def_name);
    }
    let mut children = Vec::new();
    if let Some(m) = viewer.meshes[0] {
        children.push(viewer.node(gj::Node {
            mesh: Some(gj::Index::new(m)),
            name: Some(name.into()),
            ..empty_node()
        }));
    }
    if let Some(e) = entities {
        children.extend(viewer.instance_nodes_of(doc, e, instances, &def_index));
    }
    // Metres and Y up, with the open context's placement for a selection.
    let to_gltf = DMat4::from_cols_array(&root_conversion_matrix().map(f64::from)) * local;
    let top = viewer.node(gj::Node {
        children: (!children.is_empty()).then_some(children),
        matrix: Some(to_gltf.to_cols_array().map(|v| v as f32)),
        name: Some(name.into()),
        ..empty_node()
    });
    viewer.root.scenes.push(gj::Scene {
        extensions: None,
        extras: Default::default(),
        name: Some("Scene".into()),
        nodes: vec![top],
    });
    viewer.root.scene = Some(gj::Index::new(0));

    let Viewer { mut root, bin, .. } = viewer;
    if !bin.data.is_empty() {
        root.buffers.push(gj::Buffer {
            byte_length: bin.data.len().into(),
            name: None,
            uri: Some(bin_uri.into()),
            extensions: None,
            extras: Default::default(),
        });
    }
    root.buffer_views = bin.views;
    let json = serde_json::to_vec_pretty(&root)?;
    Ok((json, bin.data, images))
}

pub(crate) fn generator() -> String {
    format!("SketchSmith {}", env!("CARGO_PKG_VERSION"))
}

// ----- reading ---------------------------------------------------------------

/// Reads bufferViews out of the BIN chunk.
struct Views<'a> {
    views: &'a [Value],
    bin: &'a [u8],
}

impl Views<'_> {
    fn bytes(&self, index: u32) -> Result<&[u8], IoError> {
        let v = self
            .views
            .get(index as usize)
            .ok_or_else(|| IoError::Damaged(format!("missing bufferView {index}")))?;
        let off = v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let len = v.get("byteLength").and_then(Value::as_u64).unwrap_or(0) as usize;
        if v.get("buffer").and_then(Value::as_u64).unwrap_or(0) != 0 {
            return Err(IoError::Damaged("model data outside the GLB buffer".into()));
        }
        off.checked_add(len)
            .and_then(|end| self.bin.get(off..end))
            .ok_or_else(|| IoError::Damaged(format!("bufferView {index} runs past the data")))
    }

    fn f64s(&self, index: Option<u32>, count: usize, what: &str) -> Result<Vec<f64>, IoError> {
        let Some(index) = index else {
            return expect_none(count, what).map(|()| Vec::new());
        };
        let b = self.bytes(index)?;
        if b.len() < count * 8 {
            return Err(IoError::Damaged(format!("{what} is too short")));
        }
        Ok(b[..count * 8]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| f64::from_le_bytes(*c))
            .collect())
    }

    fn u32s(&self, index: Option<u32>, count: usize, what: &str) -> Result<Vec<u32>, IoError> {
        let Some(index) = index else {
            return expect_none(count, what).map(|()| Vec::new());
        };
        let b = self.bytes(index)?;
        if b.len() < count * 4 {
            return Err(IoError::Damaged(format!("{what} is too short")));
        }
        Ok(b[..count * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect())
    }

    fn u8s(&self, index: Option<u32>, count: usize, what: &str) -> Result<Vec<u8>, IoError> {
        let Some(index) = index else {
            return expect_none(count, what).map(|()| Vec::new());
        };
        let b = self.bytes(index)?;
        b.get(..count)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| IoError::Damaged(format!("{what} is too short")))
    }

    fn ragged(
        &self,
        r: Option<Ragged>,
        count: usize,
        what: &str,
    ) -> Result<Vec<Vec<u32>>, IoError> {
        let Some(r) = r else {
            return expect_none(count, what).map(|()| Vec::new());
        };
        let offsets = self.u32s(Some(r.offsets), count + 1, what)?;
        let total = *offsets.last().unwrap_or(&0) as usize;
        let items = if total == 0 {
            Vec::new()
        } else {
            self.u32s(Some(r.items), total, what)?
        };
        offsets
            .windows(2)
            .map(|w| {
                items
                    .get(w[0] as usize..w[1] as usize)
                    .map(<[u32]>::to_vec)
                    .ok_or_else(|| IoError::Damaged(format!("{what} offsets are out of order")))
            })
            .collect()
    }
}

fn expect_none(count: usize, what: &str) -> Result<(), IoError> {
    if count == 0 {
        Ok(())
    } else {
        Err(IoError::Damaged(format!("{what} is missing")))
    }
}

fn frame(f: &FrameExt) -> Result<CurveFrame, IoError> {
    Ok(CurveFrame::from_stored(
        vec3(f.center),
        vec3(f.x_axis),
        vec3(f.normal),
    )?)
}

fn curve(shape: ShapeExt) -> Result<Curve, IoError> {
    let points = |v: Vec<[f64; 3]>| v.into_iter().map(vec3).collect::<Vec<_>>();
    Ok(match shape {
        ShapeExt::Arc {
            frame: f,
            radius,
            start_angle,
            end_angle,
        } => Curve::Arc(Arc::new(frame(&f)?, radius, start_angle, end_angle)?),
        ShapeExt::Circle { frame: f, radius } => Curve::Circle(Circle::new(frame(&f)?, radius)?),
        ShapeExt::Polygon {
            frame: f,
            radius,
            sides,
        } => Curve::Polygon(Polygon::new(frame(&f)?, radius, sides)?),
        ShapeExt::Bezier { control_points } => Curve::Bezier(Bezier::new(points(control_points))?),
        ShapeExt::Polyline { points: p, closed } => {
            Curve::Polyline(Polyline::new(points(p), closed)?)
        }
        ShapeExt::Helix {
            frame: f,
            start_radius,
            end_radius,
            pitch,
            turns,
            left_handed,
        } => Curve::Helix(Helix::new(
            frame(&f)?,
            start_radius,
            end_radius,
            pitch,
            turns,
            left_handed,
        )?),
    })
}

fn decode_use(v: u32) -> LoopUse {
    LoopUse {
        edge: v >> 1,
        reversed: v & 1 == 1,
    }
}

fn read_context(
    c: ContextExt,
    views: &Views<'_>,
    warnings: &mut Vec<String>,
) -> Result<EntitiesData, IoError> {
    let (nv, ne, nf) = (
        c.vertex_count as usize,
        c.edge_count as usize,
        c.face_count as usize,
    );
    let positions = views.f64s(c.positions, nv * 3, "vertex positions")?;
    let vertex_edges = views.ragged(c.vertex_edges, nv, "vertex edges")?;
    let edge_vertices = views.u32s(c.edge_vertices, ne * 2, "edge vertices")?;
    let flags = views.u8s(c.edge_flags, ne, "edge flags")?;
    let edge_curves = views.u32s(c.edge_curves, ne, "edge curves")?;
    let edge_faces = views.ragged(c.edge_faces, ne, "edge faces")?;
    let planes = views.f64s(c.face_planes, nf * 4, "face planes")?;
    let loop_counts = views.u32s(c.face_loop_counts, nf, "face loop counts")?;
    let loop_total: usize = loop_counts.iter().map(|n| *n as usize).sum();
    let mut loops = views.ragged(c.loops, loop_total, "face loops")?.into_iter();
    let curve_edges = views.ragged(c.curve_edges, c.curves.len(), "curve edges")?;
    // Schema 2; absent (schema 1, or nothing off Base) means all zero.
    let optional_u32s = |v: Option<u32>, n: usize, what: &str| -> Result<Vec<u32>, IoError> {
        match v {
            Some(_) => views.u32s(v, n, what),
            None => Ok(vec![0; n]),
        }
    };
    let edge_layers = optional_u32s(c.edge_layers, ne, "edge layers")?;
    let face_layers = optional_u32s(c.face_layers, nf, "face layers")?;
    let face_flags = match c.face_flags {
        Some(_) => views.u8s(c.face_flags, nf, "face flags")?,
        None => vec![0; nf],
    };
    // Schema 11; absent means no paint and no styles.
    let face_paint = optional_u32s(c.face_paint, nf * 2, "face paint")?;
    let edge_styles = match c.edge_styles {
        Some(_) => views.u8s(c.edge_styles, ne, "edge styles")?,
        None => vec![0; ne],
    };
    let paints = c
        .paints
        .iter()
        .map(|p| p.paint())
        .collect::<Result<Vec<_>, _>>()?;
    let paint_at = |i: u32| -> Result<Option<PaintData>, IoError> {
        match i {
            0 => Ok(None),
            n => paints
                .get(n as usize - 1)
                .copied()
                .map(Some)
                .ok_or_else(|| IoError::Damaged(format!("face paint {n} out of range"))),
        }
    };

    // Curves; an unknown kind (newer schema) is dropped and its edges kept plain.
    let mut curves = Vec::with_capacity(c.curves.len());
    let mut renumber = Vec::with_capacity(c.curves.len());
    for (value, edges) in c.curves.into_iter().zip(curve_edges) {
        match serde_json::from_value::<CurveExt>(value) {
            Ok(ext) => {
                renumber.push(Some(u32::try_from(curves.len()).unwrap_or(u32::MAX)));
                curves.push(CurveData {
                    curve: curve(ext.shape)?,
                    segments: ext.segments,
                    edges,
                });
            }
            Err(e) => {
                renumber.push(None);
                warnings.push(format!(
                    "A curve could not be read and is kept as plain edges ({e})."
                ));
            }
        }
    }

    let edges = (0..ne)
        .map(|i| {
            let b = flags[i];
            let curve = match edge_curves[i] {
                0 => None,
                n => renumber.get(n as usize - 1).copied().flatten(),
            };
            Ok(EdgeData {
                vertices: [edge_vertices[2 * i], edge_vertices[2 * i + 1]],
                faces: edge_faces[i].clone(),
                curve,
                flags: EdgeFlags {
                    soft: b & 1 != 0,
                    smooth: b & 2 != 0,
                    hidden: b & 4 != 0,
                    layer: None,
                    mark: if b & 8 != 0 {
                        EdgeMark::Sharp
                    } else if b & 16 != 0 {
                        EdgeMark::Smooth
                    } else {
                        EdgeMark::Auto
                    },
                    style: style_of(edge_styles[i])?,
                },
                layer: edge_layers[i],
            })
        })
        .collect::<Result<_, IoError>>()?;

    let mut faces = Vec::with_capacity(nf);
    for (i, n) in loop_counts.iter().enumerate() {
        if *n == 0 {
            return Err(IoError::Damaged("a face has no outer loop".into()));
        }
        let mut take = || -> Result<Vec<LoopUse>, IoError> {
            loops
                .next()
                .map(|l| l.into_iter().map(decode_use).collect())
                .ok_or_else(|| IoError::Damaged("face loops are missing".into()))
        };
        let outer = take()?;
        let inner = (1..*n).map(|_| take()).collect::<Result<_, _>>()?;
        faces.push(FaceData {
            outer,
            inner,
            normal: DVec3::new(planes[4 * i], planes[4 * i + 1], planes[4 * i + 2]),
            d: planes[4 * i + 3],
            layer: face_layers[i],
            hidden: face_flags[i] & 1 != 0,
            front: paint_at(face_paint[2 * i])?,
            back: paint_at(face_paint[2 * i + 1])?,
        });
    }

    // Guides; an unknown kind (newer schema) is skipped. A malformed guide
    // of a known kind refuses the file when the model is rebuilt.
    let mut guides = Vec::with_capacity(c.guides.len());
    for value in c.guides {
        match serde_json::from_value::<GuideExt>(value) {
            Ok(ext) => guides.push(ext.guide()),
            Err(e) => warnings.push(format!("A guide could not be read and is left out ({e}).")),
        }
    }

    let sections = c
        .sections
        .iter()
        .map(|s| SectionPlane {
            point: DVec3::from_array(s.point),
            normal: DVec3::from_array(s.normal),
            active: s.active,
        })
        .collect();

    // Dimensions and text; an unknown kind (newer schema) is skipped.
    let mut annotations = Vec::with_capacity(c.annotations.len());
    for value in c.annotations {
        match serde_json::from_value::<AnnotationExt>(value) {
            Ok(ext) => annotations.push(ext.annotation()),
            Err(e) => warnings.push(format!(
                "A dimension or text could not be read and is left out ({e})."
            )),
        }
    }

    Ok(EntitiesData {
        positions: positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| DVec3::from_array(*p))
            .collect(),
        vertex_edges,
        edges,
        faces,
        curves,
        instances: c
            .instances
            .into_iter()
            .map(|i| InstanceData {
                definition: i.definition,
                transform: DMat4::from_cols_array(&i.matrix),
                name: i.name,
                layer: i.layer,
                locked: i.locked,
                hidden: i.hidden,
            })
            .collect(),
        guides,
        annotations,
        sections,
        wraps: c.wraps.into_iter().map(Wrap::from).collect(),
    })
}

/// Decodes `.glb` bytes written by SketchSmith.
///
/// # Errors
/// [`IoError::NotGlb`], [`IoError::Foreign`] (no model layer) or
/// [`IoError::Damaged`].
pub fn read_glb(bytes: &[u8]) -> Result<Opened, IoError> {
    let (json, bin) = glb::read(bytes)?;
    let mut root: Value = serde_json::from_slice(json)?;
    let Some(ext) = root
        .get_mut("extensions")
        .and_then(|e| e.get_mut(EXTENSION))
        .map(Value::take)
    else {
        return Err(IoError::Foreign);
    };
    let schema = ext.get("schema").and_then(Value::as_u64).unwrap_or(0);
    let mut warnings = Vec::new();
    let newer = schema > u64::from(SCHEMA);
    if schema == 0 {
        return Err(IoError::Damaged("no model schema number".into()));
    }
    let ext: ModelExt = serde_json::from_value(ext)?;
    if ext.units != "mm" || ext.up != "Z" {
        return Err(IoError::Damaged(format!(
            "unsupported units or up axis ({}, {})",
            ext.units, ext.up
        )));
    }
    let empty = Vec::new();
    let views = Views {
        views: root
            .get("bufferViews")
            .and_then(Value::as_array)
            .unwrap_or(&empty),
        bin,
    };
    let images: &[Value] = root
        .get("images")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let mut shared: BTreeMap<u32, std::sync::Arc<TextureImage>> = BTreeMap::new();
    let mut materials = Vec::with_capacity(ext.document.materials.len());
    for m in ext.document.materials {
        let texture = match m.texture {
            None => None,
            Some(t) => {
                let image = match shared.get(&t.image) {
                    Some(i) => i.clone(),
                    None => {
                        let i = std::sync::Arc::new(read_image(images, t.image, t.pixels, &views)?);
                        shared.insert(t.image, i.clone());
                        i
                    }
                };
                Some(Texture {
                    image,
                    width_mm: t.width_mm,
                })
            }
        };
        materials.push(Material {
            name: m.name,
            color: m.color,
            opacity: m.opacity,
            metallic: m.metallic,
            texture,
        });
    }
    let data = DocumentData {
        materials,
        look: ext
            .document
            .smoothing_angle
            .map_or_else(ModelLook::default, |a| ModelLook {
                smoothing_angle_deg: a,
            }),
        root: read_context(ext.document.root, &views, &mut warnings)?,
        definitions: ext
            .document
            .definitions
            .into_iter()
            .map(|d| {
                Ok(DefinitionData {
                    name: d.name,
                    kind: if d.kind == "component" {
                        DefinitionKind::Component
                    } else {
                        DefinitionKind::Group
                    },
                    shape: d.shape.map(ShapeSpec::from),
                    sweep: d.sweep.map(SweepRecipe::from),
                    entities: read_context(d.context, &views, &mut warnings)?,
                })
            })
            .collect::<Result<_, IoError>>()?,
        active_layer: ext.document.active_layer,
        scenes: ext
            .document
            .scenes
            .into_iter()
            .map(|s| SceneData {
                name: s.name,
                view: SceneView {
                    target: DVec3::from_array(s.target),
                    distance: s.distance,
                    yaw: s.yaw,
                    pitch: s.pitch,
                    perspective: s.perspective,
                    fov_y: s.fov_y,
                },
                present: s.present,
                style: SceneStyle {
                    faces: s.faces.into(),
                    back_edges: s.back_edges,
                },
                hidden_layers: s.hidden_layers,
                section: s.section,
            })
            .collect(),
        layers: ext
            .document
            .layers
            .into_iter()
            .enumerate()
            .map(|(i, l)| {
                Ok(LayerData {
                    name: l.name,
                    number: l.number.unwrap_or(i as u32),
                    color: l.color,
                    visible: l.visible,
                    style: l.style.map(LineStyleExt::style).transpose()?,
                    locked: l.locked,
                })
            })
            .collect::<Result<_, IoError>>()?,
    };
    let document = Document::from_data(data)?;
    let view = ext.view.map(|v| SavedView {
        target: vec3(v.target),
        distance: v.distance,
        yaw: v.yaw,
        pitch: v.pitch,
        perspective: v.projection != "orthographic",
        fov_y: v.fov_y,
        eye: vec3(v.eye),
        up: vec3(v.up),
        half_height: v.half_height,
        aspect: v.aspect,
    });
    let axes = ext.axes.map(|a| SavedAxes {
        origin: vec3(a.origin),
        x: vec3(a.x),
        y: vec3(a.y),
        z: vec3(a.z),
    });
    Ok(Opened {
        file: NativeFile {
            document,
            view,
            axes,
            present: ext.present,
        },
        newer,
        warnings,
    })
}

/// Reads glTF image `index` (a PNG or JPEG in a bufferView) as stored.
fn read_image(
    images: &[Value],
    index: u32,
    pixels: [u32; 2],
    views: &Views<'_>,
) -> Result<TextureImage, IoError> {
    let image = images
        .get(index as usize)
        .ok_or_else(|| IoError::Damaged(format!("missing image {index}")))?;
    let view = image
        .get("bufferView")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| IoError::Damaged(format!("image {index} has no data")))?;
    let format = match image.get("mimeType").and_then(Value::as_str) {
        Some("image/png") => ImageFormat::Png,
        Some("image/jpeg") => ImageFormat::Jpeg,
        other => {
            return Err(IoError::Damaged(format!(
                "image {index} is not PNG or JPEG ({})",
                other.unwrap_or("no type")
            )));
        }
    };
    Ok(TextureImage {
        name: image
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into(),
        format,
        bytes: views.bytes(view)?.to_vec(),
        pixels,
    })
}
