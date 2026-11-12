//! # smith-tools
//!
//! SketchSmith — interactive tools | v1.0.0 | Thursday 12 November 2026
//!
//! The tool state-machine trait and the interactive tools. A tool turns snapped points ([`Inference`]) and typed values
//! ([`TypedValue`]) into a [`DrawOp`]; the canvas runs the op as one undo
//! transaction and draws the tool's [`Preview`].
//!
//! The crate knows nothing about the UI toolkit: input arrives as [`Input`],
//! output leaves as an [`Outcome`] plus preview geometry, status text and the
//! Operation Inputs [`Field`]s.
//!
//! **Operation Inputs.** Each tool lists its values as fields:
//! live while drawing, the exact stored values once the shape is made. Editing
//! a field ([`Tool::edit`]) has the same effect as typing that value. After a
//! draw the box shows the shape's values until it is accepted ([`Tool::accept`],
//! Enter); [`Tool::reopen`] (F9) brings them back while the shape can still be
//! redefined.
//!
//! **Typed redefinition.** After a draw completes, typing a value
//! returns [`Outcome::Redefine`] with the changed op. The canvas undoes the
//! last step and runs the new op, so the shape is rebuilt and stays a single
//! undo step. Once anything else changes the history, the canvas calls
//! [`Tool::forget`] and the tool stops offering redefinition.
//!
//! Dependency rule: uses `smith-geom`, `smith-model`, `smith-ops`, `smith-infer`.

mod annotate;
mod arc;
mod bevel_tool;
mod bezier;
mod circle;
mod cleanup_tool;
mod copy_path_tool;
mod curve_maker;
mod edges_tool;
mod edit_curve;
mod eraser;
mod extend_tool;
mod extrude;
mod fuse_tool;
mod gaps_tool;
mod inspect_tool;
mod intersect_tool;
mod line;
mod loft_tool;
mod manifold_tool;
mod measure;
mod move_tool;
mod polygon;
mod rectangle;
mod section;
mod shape_tools;
mod shapes_tool;
mod skin_tool;
mod solid_tool;
mod sweep_tool;
mod transform_tools;
mod tweak_tool;
mod weld_tool;

use smith_geom::{DVec2, DVec3};
use smith_infer::infer::{DrawingAxes, Inference, Lock};
use smith_infer::units::TypedValue;
use smith_model::{Entities, Pickable};
use smith_ops::draw::DrawOp;

pub use annotate::{DimensionTool, TextTool};
pub use arc::ArcTool;
pub use bevel_tool::BevelTool;
pub use bezier::BezierTool;
pub use circle::CircleTool;
pub use cleanup_tool::CleanUpTool;
pub use copy_path_tool::CopyAlongPathTool;
pub use curve_maker::CurveMakerTool;
pub use edges_tool::EdgesTool;
pub use edit_curve::CurveEditTool;
pub use eraser::EraserTool;
pub use extend_tool::{BridgeTool, ExtendFaceTool, PairKind, PairTool};
pub use extrude::ExtrudeTool;
pub use fuse_tool::FuseTool;
pub use gaps_tool::GapsTool;
pub use inspect_tool::InspectTool;
pub use intersect_tool::IntersectTool;
pub use line::LineTool;
pub use loft_tool::LoftTool;
pub use manifold_tool::ManifoldTool;
pub use measure::{FindCenterTool, ProtractorTool, TapeMeasureTool};
pub use move_tool::MoveTool;
pub use polygon::PolygonTool;
pub use rectangle::{RectangleTool, RotatedRectangleTool};
pub use section::SectionTool;
pub use shape_tools::{DissolveTool, DivideTool, KnifeTool, LoopCutTool, OffsetTool, SlideTool};
pub use shapes_tool::ShapesTool;
pub use skin_tool::SkinTool;
pub use solid_tool::SolidTool;
pub use sweep_tool::SweepTool;
pub use transform_tools::{MirrorTool, RotateTool, ScaleTool};
pub use tweak_tool::TweakTool;
pub use weld_tool::{PercentTool, WeldTool};

/// Snap distance for tool-level snaps (such as the tangent arc), in pixels.
pub const TOOL_SNAP_PX: f64 = 8.0;

/// What a tool needs to know about the scene.
pub struct Ctx<'a> {
    /// Geometry of the active context.
    pub entities: &'a Entities,
    /// Drawing axes.
    pub axes: &'a DrawingAxes,
    /// The box corners of each group and component in the context, for
    /// sizing and centring.
    pub instances: &'a smith_ops::modify::InstanceBoxes,
    /// The edges of each group and component in the context, for previews.
    pub instance_lines: &'a smith_ops::modify::InstanceLines,
    /// World size of one screen pixel near the cursor, mm.
    pub pixel: f64,
    /// Where the cursor is on the canvas, 0–1 across and down from the top
    /// left (for text fixed to the canvas).
    pub cursor: DVec2,
}

/// Input to a tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    /// The cursor moved; this is the snapped point under it.
    Hover(Inference),
    /// Left click at the snapped point.
    Click(Inference),
    /// A measurement-box entry.
    Typed(TypedValue),
    /// The tool's alternate mode key (Ctrl tap), e.g. Circle ↔ 3 points.
    Toggle,
    /// Enter with nothing typed while an operation is in progress (e.g. the
    /// Bézier Spline's last point).
    Finish,
}

/// What the tool wants done after an input.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Nothing for the canvas to do.
    Nothing,
    /// Build this op as a new undo step.
    Commit(DrawOp),
    /// Replace the tool's last committed op with this one (undo + build).
    Redefine(DrawOp),
    /// Show a one-line message (input the tool can't use, degenerate shapes).
    Message(String),
}

/// What a Operation Inputs field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// A length in mm, shown in the display unit.
    Length,
    /// A whole number (segments, sides).
    Count,
    /// A plain number with no unit (turns).
    Number,
    /// On or off (1 or 0), shown as a check box.
    Switch,
    /// One of several (the value is its index), shown as a drop-down.
    Choice(&'static [&'static str]),
}

/// One value in the Operation Inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    /// Label, e.g. "Radius".
    pub label: &'static str,
    /// Length or count.
    pub kind: FieldKind,
    /// Current value (mm, or the count), if there is one yet.
    pub value: Option<f64>,
    /// The value follows the cursor (shown rounded); otherwise it is the
    /// stored value (shown at full precision).
    pub live: bool,
    /// Whether the field can be edited now.
    pub editable: bool,
    /// A note shown under the field (e.g. why it is disabled).
    pub note: Option<&'static str>,
}

impl Field {
    /// A length field.
    pub fn length(label: &'static str, value: Option<f64>, live: bool, editable: bool) -> Self {
        Self {
            label,
            kind: FieldKind::Length,
            value,
            live,
            editable,
            note: None,
        }
    }

    /// A count field (never live: counts change only when set).
    pub fn count(label: &'static str, value: u32, editable: bool) -> Self {
        Self {
            label,
            kind: FieldKind::Count,
            value: Some(f64::from(value)),
            live: false,
            editable,
            note: None,
        }
    }

    /// A plain-number field.
    pub fn number(label: &'static str, value: Option<f64>, editable: bool) -> Self {
        Self {
            label,
            kind: FieldKind::Number,
            value,
            live: false,
            editable,
            note: None,
        }
    }

    /// An on/off field.
    pub fn switch(label: &'static str, on: bool, editable: bool) -> Self {
        Self {
            label,
            kind: FieldKind::Switch,
            value: Some(if on { 1.0 } else { 0.0 }),
            live: false,
            editable,
            note: None,
        }
    }

    /// One of `options` (the value is the index chosen).
    pub fn choice(
        label: &'static str,
        options: &'static [&'static str],
        index: usize,
        editable: bool,
    ) -> Self {
        Self {
            label,
            kind: FieldKind::Choice(options),
            value: Some(index as f64),
            live: false,
            editable,
            note: None,
        }
    }

    /// The same field with a note under it.
    pub fn with_note(self, note: &'static str) -> Self {
        Self {
            note: Some(note),
            ..self
        }
    }
}

/// Geometry the canvas draws over the model while a tool works.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Preview {
    /// Outline of the shape being drawn.
    pub lines: Vec<[DVec3; 2]>,
    /// The rubber band from the last point to the cursor, drawn in the
    /// inference colour.
    pub band: Option<[DVec3; 2]>,
    /// A tool-level snap to label at the cursor (e.g. "Tangent to Edge").
    pub note: Option<&'static str>,
}

/// An interactive tool.
pub trait Tool {
    /// Display name.
    fn name(&self) -> &'static str;
    /// Handles one input.
    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome;
    /// Esc: abandons the operation in progress. Returns `false` if nothing was
    /// in progress (the canvas then leaves the tool).
    fn cancel(&mut self) -> bool;
    /// The previous point of the operation in progress (for inference).
    fn last_point(&self) -> Option<DVec3>;
    /// Enter with nothing typed finishes an operation in progress that has
    /// no previous point (picking faces, say): the canvas then sends
    /// [`Input::Finish`] rather than leaving the tool.
    fn wants_finish(&self) -> bool {
        false
    }
    /// The preview should be drawn as a warning (red): what it shows will
    /// go wrong as it stands.
    fn warns(&self) -> bool {
        false
    }
    /// The selection when the tool starts (Move works on it).
    fn use_selection(&mut self, _items: &[Pickable]) {}
    /// Runs as the tool starts, after [`Tool::use_selection`] (Inspect
    /// Faces looks at once).
    fn start(&mut self, _ctx: &Ctx<'_>) -> Outcome {
        Outcome::Nothing
    }
    /// A double-click (Extrude repeats its last distance on the face).
    fn repeat(&mut self, _ctx: &Ctx<'_>) -> Outcome {
        Outcome::Nothing
    }
    /// How the view looks at the model, every frame (the Knife uses it).
    fn use_view(&mut self, _view: smith_ops::cut::View) {}
    /// Proportional editing, while it is on (Move uses it).
    fn use_proportional(&mut self, _p: Option<smith_ops::modify::Proportional>) {}
    /// The Pivot, while Pivot Active is on (Rotate, Scale and Mirror work
    /// around it).
    fn use_pivot(&mut self, _pivot: Option<DVec3>) {}
    /// Edit Shape: start editing the shape group with definition
    /// `definition`, made with `spec` (Shapes only).
    fn use_shape(&mut self, _definition: smith_model::DefinitionId, _spec: smith_model::ShapeSpec) {
    }
    /// Edit Sweep: start editing the Sweep or Revolve group with definition
    /// `definition`, made from `recipe` (in the group's coordinates) and
    /// placed by `placed` (group to context) (Sweep only).
    fn use_sweep(
        &mut self,
        _definition: smith_model::DefinitionId,
        _recipe: smith_model::SweepRecipe,
        _placed: smith_geom::DMat4,
    ) {
    }

    /// The arrow-key axis lock (`None`: released). A tool that takes it as a
    /// plane choice (Rotate's protractor) returns `true`; otherwise the
    /// canvas uses it to lock a direction.
    fn use_axis_lock(&mut self, _axis: Option<DVec3>) -> bool {
        false
    }
    /// A line or plane the cursor is held to while the tool works (Extrude
    /// holds it to the face normal). Overrides Shift and arrow-key locks.
    fn lock(&self) -> Option<Lock> {
        None
    }
    /// Whether the tool pauses after an operation, so the next click starts
    /// nothing until Enter (shapes). The Eraser does not.
    fn pauses(&self) -> bool {
        true
    }
    /// A direction to offer as Tangent to Edge from the last point.
    fn tangent(&self, _ctx: &Ctx<'_>) -> Option<DVec3> {
        None
    }
    /// Preview geometry for the current state.
    fn preview(&self, ctx: &Ctx<'_>) -> Preview;
    /// Status-line hint.
    fn status(&self) -> String;
    /// The Operation Inputs fields for the current state.
    fn fields(&self) -> Vec<Field>;
    /// The field that typing goes into.
    fn primary_field(&self) -> usize {
        0
    }
    /// Whether typing is words rather than a value (Text): typed entries go
    /// to [`Tool::text`] as they are.
    fn wants_text(&self) -> bool {
        false
    }
    /// Typed words, for a tool that [`Tool::wants_text`].
    fn text(&mut self, _text: &str, _ctx: &Ctx<'_>) -> Outcome {
        Outcome::Nothing
    }
    /// Sets field `index` to `value` (mm, or a count), as if typed.
    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome;
    /// Enter: the last shape's values are accepted and leave the box (they can
    /// still come back with [`Tool::reopen`]).
    fn accept(&mut self);
    /// F9: shows the last shape's values again. `false` if there is none.
    fn reopen(&mut self) -> bool;
    /// The last committed op can no longer be redefined (history changed).
    fn forget(&mut self);
}

/// A field value as a count (rounded, at least 0).
pub(crate) fn as_count(value: f64) -> u32 {
    value.round().clamp(0.0, f64::from(u32::MAX)) as u32
}

/// The draw tools, for menus, keys and the Geometry panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DrawKind {
    /// Line.
    Line,
    /// Rectangle.
    Rectangle,
    /// Rotated Rectangle.
    RotatedRectangle,
    /// Circle (centre–radius, or through 3 points).
    Circle,
    /// Arc (2 points plus bulge).
    Arc,
    /// Polygon.
    Polygon,
    /// Bézier Spline.
    Bezier,
    /// Curve Maker (helix, conical helix, flat spiral).
    CurveMaker,
    /// Extrude.
    Extrude,
    /// Eraser.
    Eraser,
    /// Move.
    Move,
    /// Rotate.
    Rotate,
    /// Scale.
    Scale,
    /// Mirror.
    Mirror,
    /// Offset (Inset).
    Offset,
    /// Divide.
    Divide,
    /// Loop Cut.
    LoopCut,
    /// Knife.
    Knife,
    /// Vertex/Edge Slide.
    Slide,
    /// Dissolve Vertex.
    Dissolve,
    /// Tape Measure.
    TapeMeasure,
    /// Protractor.
    Protractor,
    /// Find Center.
    FindCenter,
    /// Dimensions.
    Dimension,
    /// Text.
    Text,
    /// Section Plane.
    Section,
    /// Extrude along Path.
    Sweep,
    /// Revolve.
    Revolve,
    /// Extrude Edges along Path.
    ExtrudeEdges,
    /// Skinning.
    Skin,
    /// Bevel.
    Bevel,
    /// Weld.
    Weld,
    /// Point at Percentage.
    Percent,
    /// Space.
    Space,
    /// Circle (LoopTools).
    CircleLoop,
    /// Flatten.
    Flatten,
    /// Smooth.
    Smooth,
    /// Relax.
    Relax,
    /// Intersect Faces.
    Intersect,
    /// Clean Up.
    CleanUp,
    /// Close Gaps.
    CloseGaps,
    /// Inspect Faces.
    InspectFaces,
    /// Inspect Manifold.
    InspectManifold,
    /// Make Manifold.
    MakeManifold,
    /// Fuse.
    Fuse,
    /// Trim / Extend.
    TrimExtend,
    /// Bisector.
    Bisector,
    /// Extend to Face.
    ExtendToFace,
    /// Bridge.
    Bridge,
    /// Loft along Path.
    Loft,
    /// Loft (by spline: no path).
    LoftSpline,
    /// Copy Along Path.
    CopyAlongPath,
    /// Shapes.
    Shapes,
}

impl DrawKind {
    /// Display name, matching the panel button.
    pub fn name(self) -> &'static str {
        match self {
            DrawKind::Line => "Line",
            DrawKind::Rectangle => "Rectangle",
            DrawKind::RotatedRectangle => "Rotated Rectangle",
            DrawKind::Circle => "Circle",
            DrawKind::Arc => "Arc",
            DrawKind::Polygon => "Polygon",
            DrawKind::Bezier => "Bézier Spline",
            DrawKind::CurveMaker => "Curve Maker",
            DrawKind::Extrude => "Extrude",
            DrawKind::Eraser => "Eraser",
            DrawKind::Move => "Move",
            DrawKind::Rotate => "Rotate",
            DrawKind::Scale => "Scale",
            DrawKind::Mirror => "Mirror",
            DrawKind::Offset => "Offset (Inset)",
            DrawKind::Divide => "Divide",
            DrawKind::LoopCut => "Loop Cut",
            DrawKind::Knife => "Knife",
            DrawKind::Slide => "Slide",
            DrawKind::Dissolve => "Dissolve",
            DrawKind::TapeMeasure => "Tape Measure",
            DrawKind::Protractor => "Protractor",
            DrawKind::FindCenter => "Find Center",
            DrawKind::Dimension => "Dimensions",
            DrawKind::Text => "Text",
            DrawKind::Section => "Section Plane",
            DrawKind::Sweep => "Extrude along Path",
            DrawKind::Revolve => "Revolve",
            DrawKind::ExtrudeEdges => "Extrude Edges along Path",
            DrawKind::Skin => "Skinning",
            DrawKind::Bevel => "Bevel",
            DrawKind::Weld => "Weld",
            DrawKind::Percent => "Point at Percentage",
            DrawKind::Space => "Space",
            DrawKind::CircleLoop => "Circle Loop",
            DrawKind::Flatten => "Flatten",
            DrawKind::Smooth => "Smooth",
            DrawKind::Relax => "Relax",
            DrawKind::Intersect => "Intersect Faces",
            DrawKind::CleanUp => "Clean Up",
            DrawKind::CloseGaps => "Close Gaps",
            DrawKind::InspectFaces => "Inspect Faces",
            DrawKind::InspectManifold => "Inspect Manifold",
            DrawKind::MakeManifold => "Make Manifold",
            DrawKind::Fuse => "Fuse",
            DrawKind::TrimExtend => "Trim / Extend",
            DrawKind::Bisector => "Bisector",
            DrawKind::ExtendToFace => "Extend to Face",
            DrawKind::Bridge => "Bridge",
            DrawKind::Loft => "Loft along Path",
            DrawKind::LoftSpline => "Loft",
            DrawKind::CopyAlongPath => "Copy Along Path",
            DrawKind::Shapes => "Shapes",
        }
    }

    /// A fresh tool of this kind.
    pub fn create(self) -> Box<dyn Tool> {
        match self {
            DrawKind::Line => Box::new(LineTool::default()),
            DrawKind::Rectangle => Box::new(RectangleTool::default()),
            DrawKind::RotatedRectangle => Box::new(RotatedRectangleTool::default()),
            DrawKind::Circle => Box::new(CircleTool::default()),
            DrawKind::Arc => Box::new(ArcTool::default()),
            DrawKind::Polygon => Box::new(PolygonTool::default()),
            DrawKind::Bezier => Box::new(BezierTool::default()),
            DrawKind::CurveMaker => Box::new(CurveMakerTool::default()),
            DrawKind::Extrude => Box::new(ExtrudeTool::default()),
            DrawKind::Eraser => Box::new(EraserTool::default()),
            DrawKind::Move => Box::new(MoveTool::default()),
            DrawKind::Rotate => Box::new(RotateTool::default()),
            DrawKind::Scale => Box::new(ScaleTool::default()),
            DrawKind::Mirror => Box::new(MirrorTool::default()),
            DrawKind::Offset => Box::new(OffsetTool::default()),
            DrawKind::Divide => Box::new(DivideTool::default()),
            DrawKind::LoopCut => Box::new(LoopCutTool::default()),
            DrawKind::Knife => Box::new(KnifeTool::default()),
            DrawKind::Slide => Box::new(SlideTool::default()),
            DrawKind::Dissolve => Box::new(DissolveTool),
            DrawKind::TapeMeasure => Box::new(TapeMeasureTool::default()),
            DrawKind::Protractor => Box::new(ProtractorTool::default()),
            DrawKind::FindCenter => Box::new(FindCenterTool),
            DrawKind::Dimension => Box::new(DimensionTool::default()),
            DrawKind::Text => Box::new(TextTool::default()),
            DrawKind::Section => Box::new(SectionTool::default()),
            DrawKind::Sweep => Box::new(SweepTool::default()),
            DrawKind::Revolve => Box::new(SweepTool::revolve()),
            DrawKind::ExtrudeEdges => Box::new(EdgesTool::default()),
            DrawKind::Skin => Box::new(SkinTool::default()),
            DrawKind::Bevel => Box::new(BevelTool::default()),
            DrawKind::Weld => Box::new(WeldTool::default()),
            DrawKind::Percent => Box::new(PercentTool::default()),
            DrawKind::Space => Box::new(TweakTool::new(smith_ops::tweak::TweakKind::Space)),
            DrawKind::CircleLoop => Box::new(TweakTool::new(smith_ops::tweak::TweakKind::Circle)),
            DrawKind::Flatten => Box::new(TweakTool::new(smith_ops::tweak::TweakKind::Flatten)),
            DrawKind::Smooth => Box::new(TweakTool::new(smith_ops::tweak::TweakKind::Smooth)),
            DrawKind::Relax => Box::new(TweakTool::new(smith_ops::tweak::TweakKind::Relax)),
            DrawKind::Intersect => Box::new(IntersectTool::new()),
            DrawKind::CleanUp => Box::new(CleanUpTool::new()),
            DrawKind::CloseGaps => Box::new(GapsTool::new()),
            DrawKind::InspectFaces => Box::new(InspectTool::new()),
            DrawKind::InspectManifold => Box::new(SolidTool::new()),
            DrawKind::MakeManifold => Box::new(ManifoldTool::new()),
            DrawKind::Fuse => Box::new(FuseTool::new()),
            DrawKind::TrimExtend => Box::new(PairTool::new(PairKind::Trim)),
            DrawKind::Bisector => Box::new(PairTool::new(PairKind::Bisector)),
            DrawKind::ExtendToFace => Box::new(ExtendFaceTool::default()),
            DrawKind::Bridge => Box::new(BridgeTool::default()),
            DrawKind::Loft => Box::new(LoftTool::along_path()),
            DrawKind::LoftSpline => Box::new(LoftTool::by_spline()),
            DrawKind::CopyAlongPath => Box::new(CopyAlongPathTool::default()),
            DrawKind::Shapes => Box::new(ShapesTool::default()),
        }
    }
}

/// The normal of the face an inference landed on, if any.
/// The lines that show `items`: loose edges, and the edges of the groups and
/// components among them.
pub(crate) fn item_lines(ctx: &Ctx<'_>, items: &[Pickable]) -> Vec<[DVec3; 2]> {
    let mut lines: Vec<[DVec3; 2]> = smith_ops::modify::item_edges(ctx.entities, items)
        .into_iter()
        .filter_map(|e| ctx.entities.edge_points(e))
        .map(|(a, b)| [a, b])
        .collect();
    for p in items {
        if let Pickable::Instance(i) = p
            && let Some(l) = ctx.instance_lines.get(i)
        {
            lines.extend(l.iter().copied());
        }
    }
    lines
}

pub(crate) fn face_normal(inf: &Inference, entities: &Entities) -> Option<DVec3> {
    if inf.normal.is_some() {
        return inf.normal;
    }
    match inf.entity {
        Some(Pickable::Face(f)) => entities.face(f).map(|f| f.plane().normal()),
        _ => None,
    }
}

/// A typed offset in drawing-axis coordinates, as a world vector.
pub(crate) fn axes_vector(axes: &DrawingAxes, d: DVec3) -> DVec3 {
    axes.x * d.x + axes.y * d.y + axes.z * d.z
}

/// A typed absolute point in drawing-axis coordinates, as a world point.
pub(crate) fn axes_point(axes: &DrawingAxes, p: DVec3) -> DVec3 {
    axes.origin + axes_vector(axes, p)
}

/// `v` with its component along unit `n` removed.
pub(crate) fn in_plane(v: DVec3, n: DVec3) -> DVec3 {
    v - n * v.dot(n)
}

/// The red axis flattened into the plane with normal `n` (green if red is
/// along `n`): the default first direction for curves drawn in that plane.
pub(crate) fn plane_x(axes: &DrawingAxes, n: DVec3) -> DVec3 {
    in_plane(axes.x, n)
        .try_normalize()
        .or_else(|| in_plane(axes.y, n).try_normalize())
        .unwrap_or(DVec3::X)
}

/// Outline segments of a closed polygon through `pts`.
pub(crate) fn closed_outline(pts: &[DVec3]) -> Vec<[DVec3; 2]> {
    (0..pts.len())
        .map(|i| [pts[i], pts[(i + 1) % pts.len()]])
        .collect()
}

/// Points on a circle or arc, for previews.
pub(crate) fn arc_points(
    at: &smith_ops::draw::Placement,
    radius: f64,
    sweep: f64,
    segments: u32,
) -> Vec<DVec3> {
    let y = at.normal.cross(at.x_axis);
    let n = segments.max(1);
    (0..=n)
        .map(|i| {
            let a = sweep * f64::from(i) / f64::from(n);
            at.center + (at.x_axis * a.cos() + y * a.sin()) * radius
        })
        .collect()
}

/// Open polyline segments through `pts`.
pub(crate) fn open_outline(pts: &[DVec3]) -> Vec<[DVec3; 2]> {
    pts.windows(2).map(|w| [w[0], w[1]]).collect()
}

#[cfg(test)]
pub(crate) mod testing {
    //! Helpers for driving tools in tests.
    use super::*;
    use smith_infer::infer::Kind;

    /// A free inference at `p`.
    pub fn at(p: DVec3) -> Inference {
        Inference {
            point: p,
            kind: Kind::Free,
            from: None,
            from2: None,
            entity: None,
            locked: false,
            normal: None,
        }
    }

    /// No groups or components.
    static NO_BOXES: smith_ops::modify::InstanceBoxes = std::collections::BTreeMap::new();
    /// No group edges.
    static NO_LINES: smith_ops::modify::InstanceLines = std::collections::BTreeMap::new();

    /// A context over `m` with default axes and 0.01 mm pixels.
    pub fn ctx<'a>(m: &'a Entities, axes: &'a DrawingAxes) -> Ctx<'a> {
        Ctx {
            entities: m,
            axes,
            instances: &NO_BOXES,
            instance_lines: &NO_LINES,
            pixel: 0.01,
            cursor: smith_geom::DVec2::ZERO,
        }
    }

    /// Hover then click at `p`.
    pub fn click(tool: &mut dyn Tool, p: DVec3, ctx: &Ctx<'_>) -> Outcome {
        tool.handle(Input::Hover(at(p)), ctx);
        tool.handle(Input::Click(at(p)), ctx)
    }

    /// Shorthand vector.
    pub fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }
}
