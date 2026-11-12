//! SketchSmith — 3D canvas | v1.0.0 | Thursday 12 November 2026
//!
//! Hosts the document, camera, selection and renderer, and turns input into
//! camera moves and selection:
//!
//! * middle drag orbits; Shift + middle drag pans (both presets);
//! * the wheel zooms toward the surface under the cursor; during a move with
//!   proportional editing on, it sets the radius instead (wheel down larger),
//!   as in Blender;
//! * navigation tools (SketchUp `O` / `H` / `Z`) make left drag orbit, pan or zoom;
//! * with no navigation tool, the left button selects, exactly as SketchUp:
//!   click, double-click, triple-click, drag left→right (window) and
//!   right→left (crossing), with Ctrl add / Shift toggle / Ctrl+Shift remove;
//! * right-click opens the context menu for what is under the cursor.
//!
//! **Inference.** While a draw tool or the Axes tool is active,
//! the cursor snaps through `smith_infer::infer`. Shift holds the current
//! inference; the arrow keys lock red (Right), green (Left), blue (Up) or
//! parallel to the last hovered edge (Down); pressing the same arrow again
//! releases it. Hovering a point snap for 0.4 s tracks it, adding axis lines
//! from it. Typing goes to the measurement box; Enter applies it.
//!
//! **Draw tools.** The active `smith_tools::Tool` receives each
//! snapped point, click and typed value. A completed shape is built with
//! `smith_ops::draw::draw` inside one document transaction (one undo step). A
//! typed value after a draw *redefines* it: the last step is undone and the
//! changed shape built in its place, so it stays one undo step. Any other change
//! to the history (undo, redo, a new document, a failed draw) ends that.
//! With the Select tool, a drag that starts on selected geometry moves it
//! (the Move tool takes over for that one drag).
//!
//! **Groups and components** are drawn by their instance transforms and
//! picked whole: a click selects the nearest one under the cursor, and a
//! selection box takes those whose box it encloses (window) or reaches
//! (crossing). Make Group, Make Component and Explode are in the context
//! menu; Move, Rotate, Scale, Mirror, Flip Along and Erase work on them.
//!
//! **Edit context.** Double-clicking a group or component (or Edit Group /
//! Edit Component in the context menu) opens it for editing; Esc with
//! nothing else to cancel closes it. Everything outside is drawn faded and
//! nothing inside sticks to it. Picking, inference and the tools then work in
//! the definition's own coordinates: the cursor ray, the drawing axes and
//! the Pivot are carried in, and every point shown on screen is carried back
//! out, so drawing inside a moved or turned group lands where the cursor is.
//!
//! Points are placed by a click, or by each end of a press–drag–release, as in
//! SketchUp. A tap of Ctrl (pressed and released alone) switches the tool's
//! alternate mode, such as Circle through 3 points.
//!
//! **Guides.** The guide lines and guide points of the context being
//! edited are drawn dashed over the model in Edit mode while View → Guides
//! is on, and only then snapped to, together with the guides inside its
//! groups and, while a group is open, those outside it. A guide line is
//! endless: it is clipped to what the camera sees (in front of the eye, then
//! to the canvas), and the parts behind faces are left out.
//! Edit → Delete Guides erases them all as one step.
//!
//! **Section planes.** The root's active section plane cuts the whole
//! view (View → Section Cuts): the renderer leaves out what lies on the side
//! its normal points to, and the cut through the root's faces and its
//! groups' is filled and outlined. The planes of the context being edited
//! are drawn as square outlines (View → Section Planes), the active one in
//! orange; with the Section Plane tool a click on an outline turns that
//! plane on or off, and the Eraser erases one. Edit → Create Group from
//! Slice turns the active plane's cut through the edited context into a
//! group of edges.
//!
//! **Auto-Tri.** An operation that would bend faces is refused with a
//! message, and Operation Inputs offers Auto-Tri for that operation alone:
//! switched on, the operation is done again with only the faces it bends
//! folded into triangles (soft, smooth diagonals), as one undo step. The
//! offer goes with the next operation or tool, so nothing is ever folded
//! without asking.
//!
//! **Dimensions and text** of the context being edited are drawn over the
//! model in both modes, read afresh each frame from the geometry they are
//! tied to, in the display units. The Eraser erases one under the cursor
//! before it looks at edges.
//!
//! **Operation Inputs.** [`Canvas::operation_inputs`] draws the
//! active tool's values in their box: live while drawing, exact
//! (full precision) once the shape is made, and editable — a field takes
//! units and arithmetic. The field being edited shows in amber; it applies
//! when it loses focus (moving to the next field, or back to the model), and
//! the shape rebuilds — every change to a drawn shape stays one undo step. Esc
//! in a field discards its edit. Enter confirms: it applies the field it is
//! pressed in, then accepts the shape and returns to the Select tool.
//!
//! **Paused after a shape.** Once a shape is drawn (for Line, once its chain
//! ends), the tool pauses: clicks in the canvas no longer start a new shape,
//! so a stray click cannot begin one and lock in the last shape's values.
//! Navigation still works. Enter or Esc confirms the shape and returns to
//! Select; the tool's key or button starts the next shape. Typing over the canvas goes into the tool's primary field; where
//! that field is a count (segments, sides), a bare whole number sets the count.
//! Enter with nothing typed accepts the shape and resets the box; F9 (Edit →
//! Adjust Last Operation) shows the last shape's values again. While a
//! multi-point operation is in progress (the Bézier Spline), Enter finishes it.
//!
//! **Edit Curve.** Right-click → Edit Curve on a selected curve
//! loads its values into the Operation Inputs (`smith_tools::CurveEditTool`); Enter
//! accepts and returns to Select.
//!
//! **View pie (Blender `~`).** The view-pie key opens eight choices around the
//! cursor; moving toward one highlights it, and a click (or releasing the key
//! after moving) chooses it. Esc closes it.
//!
//! **Modes.** Edit mode shows the real geometry (every edge, flat
//! shading) in the chosen display style, with selection. Present mode is
//! view-only and shows the Look settings: smoothing by angle, shading, edges and
//! background. Each mode keeps its own settings; the camera is shared.
//!
//! **Files.** The shell saves and opens through [`Canvas::saved_view`],
//! [`Canvas::saved_axes`] and [`Canvas::load`]. Changes are tracked by the
//! document's revision plus [`Canvas::axes_serial`], which counts moves of
//! the drawing axes (they are saved with the model; the camera is saved too but
//! moving it is not a change). File keys (New, Open, Save, Save As, Quit) are
//! passed up to the shell through [`Canvas::take_file_actions`].
//!
//! The scene is rendered into an offscreen texture each frame and drawn as an
//! image filling the central area.

use egui::{
    Color32, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Stroke, TextEdit, Ui, Vec2, pos2,
};
use smith_geom::plane::Plane;
use smith_geom::{DMat3, DMat4, DVec2, DVec3};
use smith_infer::bvh::FaceBvh;
use smith_infer::infer::{
    Axis, DrawingAxes, Inference, Kind, Lock, OnlyKind, Query, SnapFilter, axis_lock, infer,
    lock_for,
};
use smith_infer::select::pick_rect;
use smith_infer::units::{
    LengthUnit, TypedValue, UnitSettings, format_area, format_exact, format_length, parse_length,
    parse_typed,
};
use smith_io::{SavedAxes, SavedView};
use smith_model::{
    Annotation, AnnotationId, DefinitionId, DefinitionKind, Document, Entities, FaceDisplay, Guide,
    Instance, InstanceId, LayerId, LeaderText, MaterialId, ModelError, Pickable, Scene, SceneId,
    SceneStyle, SceneView, SectionId, SectionPlane, SelectKind, SelectMode, Selection, SnapView,
};
use smith_ops::cleanup::ReportLine;
use smith_ops::draw::{DrawOp, draw};
use smith_render::camera::{Camera, Projection, StandardView};
use smith_render::mesh::{
    FaceVertex, LineVertex, PresentLook, PresentPaint, SceneMesh, Shown, StyledSegment, Visibility,
};

use crate::commands::{CONTEXT_MENU, CONTEXT_MENU_EMPTY, Command, Entry};
use smith_render::renderer::{CanvasRenderer, FaceStyle, RenderStyle, SECTION_FILL};
use smith_tools::{Ctx, CurveEditTool, DrawKind, Field, FieldKind, Input, Outcome, Preview, Tool};

use crate::keymap::{Action, Keymap};
use crate::paint_tool::{ApplyTo, PaintSettings, WrapTargets};

/// Navigation tool driven by the left mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavTool {
    /// No navigation tool: the left button selects.
    #[default]
    None,
    /// Left drag orbits.
    Orbit,
    /// Left drag pans.
    Pan,
    /// Left drag zooms toward the orbit target.
    Zoom,
}

/// Edit or Present mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Build geometry.
    #[default]
    Edit,
    /// Set the look; geometry cannot change.
    Present,
}

/// Scale's Around Centre field (after X, Y, Z and Lock Proportions).
const SCALE_AROUND_CENTRE: usize = 4;

/// A Yes / No question the canvas waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    /// An operation was refused for bending faces: do it with triangles?
    AutoTriangulate,
    /// Clean Up held this many unused materials back: remove them?
    RemoveMaterials(usize),
    /// Remove this layer and what is on it: its faces, groups and loose
    /// edges?
    RemoveLayer(smith_model::LayerId, usize, usize, usize),
}

/// Present-mode look: mesh-affecting settings plus face style and background.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Smoothing, shading normals and edge display.
    pub mesh: PresentLook,
    /// How faces are drawn (the display styles Edit mode uses too).
    pub faces: FaceStyle,
    /// Edges behind faces drawn dashed.
    pub back_edges: bool,
    /// Background colour (linear RGBA).
    pub background: [f32; 4],
}

impl Default for Look {
    fn default() -> Self {
        Self {
            mesh: PresentLook::default(),
            faces: FaceStyle::Shaded,
            back_edges: false,
            background: smith_render::renderer::BACKGROUND,
        }
    }
}

/// The working tool driven by the left button when no navigation tool is active.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum WorkTool {
    /// Selection.
    #[default]
    Select,
    /// A draw tool; its state lives in the canvas's tool object.
    Draw(DrawKind),
    /// Edit Curve on one curve; state in the tool object.
    EditCurve,
    /// Paint (Present mode); its values live in the canvas's paint settings.
    Paint,
    /// Mark Edges (Present mode): a click marks an edge for smoothing.
    MarkEdges,
    /// Line Style (Present mode): a click styles an edge, or the style is
    /// set on a layer.
    LineStyle,
    /// Axes tool: origin, then red, then green.
    Axes {
        /// Origin, once placed.
        origin: Option<DVec3>,
        /// Red-direction point, once placed.
        red: Option<DVec3>,
    },
}

/// Per-frame preferences from the shell.
pub struct ViewPrefs<'a> {
    /// Units and precision.
    pub units: &'a UnitSettings,
    /// Enabled snaps.
    pub snaps: &'a SnapFilter,
    /// Snap-only type for the held key.
    pub snap_only: OnlyKind,
    /// Guides are shown and snapped to.
    pub guides: bool,
    /// Section planes are drawn.
    pub sections: bool,
    /// The active section plane cuts the view.
    pub cuts: bool,
}

/// View pie layout, Blender's: W Left, E Right, S Bottom, N Top, NW Front,
/// NE Back, SW Iso, SE Zoom Extents. Angles in screen degrees, 0° = east,
/// counter-clockwise (y up).
const PIE: [(&str, f32, Action); 8] = [
    ("Right", 0.0, Action::ViewRight),
    ("Back", 45.0, Action::ViewBack),
    ("Top", 90.0, Action::ViewTop),
    ("Front", 135.0, Action::ViewFront),
    ("Left", 180.0, Action::ViewLeft),
    ("Iso", 225.0, Action::ViewIso),
    ("Bottom", 270.0, Action::ViewBottom),
    ("Zoom Extents", 315.0, Action::ZoomExtents),
];
/// Pie radius in points.
const PIE_RADIUS: f32 = 95.0;
/// Inner dead zone: the cursor must leave this circle to highlight a choice.
const PIE_DEAD: f32 = 22.0;

/// Seconds the cursor must rest on a point snap to track it.
const TRACK_DWELL: f64 = 0.4;
/// Most tracked points kept at once.
const MAX_TRACKED: usize = 2;

/// Wheel zoom sensitivity: factor = exp(−scroll_points × this).
const WHEEL_ZOOM: f64 = 0.0015;
/// Zoom-tool drag sensitivity, per point of vertical drag.
const DRAG_ZOOM: f64 = 0.01;
/// Selection rectangle colour (SketchUp blue).
const RECT_COLOR: Color32 = Color32::from_rgb(40, 90, 230);
/// Room left beside a Operation Inputs field for its label and unit, in points.
const SETTINGS_LABEL_ROOM: f32 = 100.0;
/// Narrowest a Operation Inputs field gets, in points.
const SETTINGS_FIELD_MIN: f32 = 60.0;
/// Text colour of a field edited but not yet applied (amber).
const PENDING_COLOR: Color32 = Color32::from_rgb(190, 110, 0);
/// Text colour of a field receiving typed input from the canvas.
const TYPING_COLOR: Color32 = Color32::from_rgb(40, 90, 230);
/// Draw-tool preview outline colour.
const PREVIEW_COLOR: Color32 = Color32::from_gray(45);
/// A tool's preview lines when it warns (a profile wider than a bend).
const WARN_COLOR: Color32 = Color32::from_rgb(220, 40, 40);
/// Tangent inference colour (cyan, distinct from the axes).
const TANGENT_COLOR: Color32 = Color32::from_rgb(0, 160, 200);

/// The canvas: document, camera, selection and GPU renderer.
pub struct Canvas {
    document: Document,
    /// Guides are shown and snapped to (View → Guides).
    show_guides: bool,
    /// Where the cursor is on the canvas, 0–1 across and down.
    cursor: DVec2,
    /// Units for dimensions and text, as shown this frame.
    units: UnitSettings,
    /// Section planes are drawn (View → Section Planes).
    show_sections: bool,
    /// The operation just refused because it would bend faces, offered again
    /// with Auto-Tri (and whether it was a typed change to the last one).
    auto_tri_offer: Option<(DrawOp, bool)>,
    /// The Clean Up just run with unused materials held back, and how many,
    /// offered again with them removed.
    material_offer: Option<(DrawOp, usize)>,
    /// A layer waiting on the Remove question.
    layer_offer: Option<smith_model::LayerId>,
    /// A window over the canvas (the file prompts) holds its input.
    on_hold: bool,
    /// What the Select tool picks in Edit mode: vertices, edges or faces.
    select_kind: SelectKind,
    /// The render (scene) last restored, offered first by Export Render.
    last_scene: Option<SceneId>,
    /// The size of overlays (dimension text, arrowheads, guide and section
    /// lines) against the screen's: 1 on screen, more for a larger picture.
    overlay_scale: f32,
    /// Faces turned the wrong way in the selection (or the open context),
    /// for Statistics; worked out when the model or selection changes.
    reversed: usize,
    /// The last run of the active operation recorded an undo step (a run
    /// that changed nothing records none, so a redo must not step back).
    last_step: bool,
    /// What the active repair tool found or did, one line each, shown
    /// under Statistics while the tool is active; a click on a line selects
    /// its items.
    report: Vec<ReportLine>,
    /// Select by Type's choices while it is open (the Select tool with its
    /// values in Operation Inputs).
    by_type: Option<ByType>,
    /// The active section plane cuts the view (View → Section Cuts).
    section_cuts: bool,
    selection: Selection,
    bvh: FaceBvh,
    /// What picking looks through besides `bvh`: the edges `bvh` indexes
    /// the faces of, and the open context's vertices (Vertices mode).
    pick_index: smith_infer::select::PickIndex,
    /// The active context with its groups' geometry laid in, for picking and
    /// inference; `None` when it has no groups.
    snap: Option<SnapView>,
    /// The connected pieces of loose geometry in the active context, for
    /// the Items panel; rebuilt after each change.
    pieces: Vec<Piece>,
    /// What Copy or Cut put on the clipboard, and where its context was
    /// placed in the model then (so Paste lands in the same place).
    clipboard: Option<(smith_ops::organize::Clip, DMat4)>,
    /// The sidebar key was pressed (the shell owns the sidebar).
    sidebar_toggle: bool,
    /// Box corners of each group and component in the active context.
    instance_boxes: smith_ops::modify::InstanceBoxes,
    /// Edges of each group and component in the active context, for previews.
    instance_lines: smith_ops::modify::InstanceLines,
    /// The groups and components opened for editing, outermost first.
    editing: Vec<InstanceId>,
    /// The definition being edited (`None`: the model root).
    ctx: Option<DefinitionId>,
    /// The edited definition's placement: its coordinates → world.
    local: DMat4,
    /// The drawing axes carried into the edited definition's coordinates.
    axes_ctx: DrawingAxes,
    /// The Paint tool's Operation Inputs.
    paint: PaintSettings,
    /// The last paint and the history revision right after it, while
    /// changing Offset or Rotation can still redo it.
    last_paint: Option<(u64, LastPaint)>,
    /// The revision right after the last value edit (a material, the
    /// smoothing angle, a layer's line style) and what it changed, so that a
    /// run of edits to one value stays one undo step.
    last_value_edit: Option<(u64, ValueEdit)>,
    /// Mark Edges' Operation Inputs: the mark a click gives.
    mark: smith_model::EdgeMark,
    /// Line Style's Operation Inputs.
    line: LineSettings,
    camera: Camera,
    render_state: Option<egui_wgpu::RenderState>,
    renderer: Option<CanvasRenderer>,
    texture: Option<egui::TextureId>,
    /// A mesh waiting to be uploaded (set when the scene or selection changes).
    pending_mesh: Option<SceneMesh>,
    /// The mesh last uploaded, kept so a selection change can re-mark it
    /// in place ([`SceneMesh::reselect`]) instead of rebuilding it.
    shown_mesh: Option<SceneMesh>,
    nav: NavTool,
    rect: Rect,
    /// Start of a left-button selection drag, in screen points.
    drag_start: Option<Pos2>,
    /// Bounds of what is shown, for zoom extents (also covers render-only scenes).
    shown_bounds: Option<(DVec3, DVec3)>,
    /// Set by the context menu's Entity Info item; read by the shell.
    mode: Mode,
    /// Edit-mode display style (Wireframe … X-Ray, back edges).
    edit_style: RenderStyle,
    /// Present-mode look.
    look: Look,
    /// Shown mesh is render-only (stress scene): never rebuilt from the document.
    render_only: bool,
    tool: WorkTool,
    axes: DrawingAxes,
    inference: Option<Inference>,
    lock: Lock,
    /// The arrow-key lock in force (kept until toggled off).
    arrow_lock: Option<Option<Axis>>,
    /// The plane an arrow key chose for a tool that takes one (Rotate).
    tool_axis: Option<Axis>,
    /// The Pivot (Rotate, Scale, Mirror and Flip Along work around it while
    /// Pivot Active is on).
    pivot: DVec3,
    /// The item the Pivot is tied to (Set Active Selection), in the context
    /// that was open then; it follows the item until the item is erased.
    pivot_tie: Option<(Option<DefinitionId>, Pickable)>,
    /// Pivot Active.
    pivot_active: bool,
    /// Faces the last refused change would have bent, drawn in red until the
    /// next change or tool.
    bent: Vec<smith_model::FaceId>,
    /// Proportional editing: on, and its reach and falloff.
    proportional_on: bool,
    proportional: smith_ops::modify::Proportional,
    /// Where the context menu was opened, on the model or the ground.
    menu_point: Option<DVec3>,
    /// The copy Repeat Transform repeats: what was copied, the combined
    /// transform from it to the latest copy (later moves, turns and scales of
    /// that copy are folded in), and how many steps exist so far. The chain
    /// as it was before the last operation is kept for redefining.
    chain: Option<Chain>,
    chain_before: Option<Chain>,
    /// A drag with the Select tool is moving the selection.
    drag_move: bool,
    /// The edges hidden last (for Unhide Last).
    last_hidden: Vec<Pickable>,
    tracked: Vec<DVec3>,
    /// A point snap under the cursor and when the cursor arrived on it.
    dwell: Option<(DVec3, f64)>,
    reference_edge: Option<smith_model::EdgeId>,
    /// Text typed into the measurement box.
    typed: String,
    /// One-line message for the status bar (results and errors), if any.
    message: Option<String>,
    /// Centre of the open view pie, if open.
    pie: Option<Pos2>,
    /// Set by the view-pie action; the pie opens at the cursor next frame.
    pie_requested: bool,
    /// The active draw tool's state, when the work tool is `Draw`.
    draw_tool: Option<Box<dyn Tool>>,
    /// The newest undo step is the draw tool's last shape, so a typed value may
    /// redefine it.
    redefinable: bool,
    /// Ctrl is held (for detecting a lone Ctrl tap).
    ctrl_held: bool,
    /// Something else happened while Ctrl was held, so its release is no tap.
    ctrl_used: bool,
    /// Text of each Operation Inputs field (kept while a field is being edited).
    field_text: Vec<String>,
    /// The edit in progress in a Operation Inputs field (applied when the field
    /// loses focus), by field.
    pending: Vec<Option<String>>,
    /// The tool the pending edits belong to.
    pending_owner: Option<&'static str>,
    /// The last confirmed shape's tool, kept so F9 can reopen it after Enter
    /// returned to Select; dropped once the history moves on.
    confirmed: Option<(WorkTool, Box<dyn Tool>)>,
    /// Counts changes to the drawing axes (for unsaved-change tracking).
    axes_serial: u64,
    /// File actions from the keymap, for the shell to perform.
    file_actions: Vec<Action>,
}

impl Canvas {
    /// Creates an empty canvas. `render_state` is `None` only if the GPU could
    /// not be initialised, in which case nothing is drawn.
    pub fn new(render_state: Option<egui_wgpu::RenderState>) -> Self {
        Self {
            document: Document::new(),
            show_guides: true,
            cursor: DVec2::ZERO,
            units: UnitSettings::default(),
            show_sections: true,
            auto_tri_offer: None,
            material_offer: None,
            layer_offer: None,
            on_hold: false,
            overlay_scale: 1.0,
            last_scene: None,
            last_step: false,
            select_kind: SelectKind::default(),
            reversed: 0,
            report: Vec::new(),
            by_type: None,
            section_cuts: true,
            selection: Selection::new(),
            bvh: FaceBvh::default(),
            pick_index: Default::default(),
            snap: None,
            pieces: Vec::new(),
            clipboard: None,
            sidebar_toggle: false,
            instance_boxes: Default::default(),
            instance_lines: Default::default(),
            editing: Vec::new(),
            ctx: None,
            local: DMat4::IDENTITY,
            axes_ctx: DrawingAxes::default(),
            camera: Camera::default(),
            render_state,
            renderer: None,
            texture: None,
            pending_mesh: Some(SceneMesh::default()),
            shown_mesh: None,
            nav: NavTool::None,
            rect: Rect::NOTHING,
            drag_start: None,
            shown_bounds: None,
            mode: Mode::Edit,
            edit_style: RenderStyle::default(),
            look: Look::default(),
            render_only: false,
            tool: WorkTool::Select,
            axes: DrawingAxes::default(),
            paint: PaintSettings::default(),
            last_paint: None,
            last_value_edit: None,
            mark: smith_model::EdgeMark::Sharp,
            line: LineSettings::default(),
            inference: None,
            lock: Lock::None,
            arrow_lock: None,
            tool_axis: None,
            pivot: DVec3::ZERO,
            pivot_tie: None,
            pivot_active: false,
            bent: Vec::new(),
            proportional_on: false,
            proportional: smith_ops::modify::Proportional {
                radius: 10.0,
                falloff: smith_ops::modify::Falloff::Smooth,
            },
            menu_point: None,
            chain: None,
            chain_before: None,
            drag_move: false,
            last_hidden: Vec::new(),
            tracked: Vec::new(),
            dwell: None,
            reference_edge: None,
            typed: String::new(),
            message: None,
            pie: None,
            pie_requested: false,
            draw_tool: None,
            redefinable: false,
            ctrl_held: false,
            ctrl_used: false,
            field_text: Vec::new(),
            pending: Vec::new(),
            pending_owner: None,
            confirmed: None,
            axes_serial: 0,
            file_actions: Vec::new(),
        }
    }

    /// The active navigation tool.
    pub fn nav_tool(&self) -> NavTool {
        self.nav
    }

    /// The active repair tool's report lines.
    pub fn report(&self) -> &[ReportLine] {
        &self.report
    }

    /// Selects the items of report line `i` (replacing the selection).
    pub fn select_report_line(&mut self, i: usize) {
        if let Some(line) = self.report.get(i) {
            let items: Vec<Pickable> = line
                .items
                .iter()
                .copied()
                .filter(|p| {
                    let e = self.active();
                    match p {
                        Pickable::Edge(x) => e.edge(*x).is_some(),
                        Pickable::Face(x) => e.face(*x).is_some(),
                        Pickable::Instance(x) => e.instance(*x).is_some(),
                        Pickable::Guide(x) => e.guide(*x).is_some(),
                        Pickable::Vertex(x) => e.vertex(*x).is_some(),
                    }
                })
                .collect();
            self.select(items, SelectMode::Replace);
        }
    }

    /// The current selection.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// The document.
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// The active work tool.
    pub fn tool(&self) -> WorkTool {
        self.tool
    }

    /// The status message, if any (typed-input results and errors).
    pub fn message(&self) -> Option<&str> {
        // A step refused for touching a locked layer says only that,
        // whatever words the tool wrapped round it.
        self.message.as_deref().map(|m| {
            if m.contains(smith_model::ON_LOCKED_LAYER) {
                smith_model::ON_LOCKED_LAYER
            } else if m.contains(smith_model::UNLOCK_LAYER) {
                smith_model::UNLOCK_LAYER
            } else {
                m
            }
        })
    }

    /// Puts `text` on the bottom line.
    pub fn set_message(&mut self, text: impl Into<String>) {
        self.message = Some(text.into());
    }

    /// The open context (`None`: the model root) and its placement: its
    /// coordinates → world.
    pub fn context(&self) -> (Option<DefinitionId>, DMat4) {
        (self.ctx, self.local)
    }

    /// Draws the Operation Inputs: the active operation's values, each editable
    /// where the tool allows. Called by the shell in the Operation Inputs box.
    pub fn operation_inputs(&mut self, ui: &mut Ui, units: &UnitSettings) {
        ui.strong("Operation Inputs");
        if self.by_type.is_some() {
            self.by_type_inputs(ui);
            return;
        }
        if self.mode == Mode::Present {
            match self.tool {
                WorkTool::Paint => self.paint_inputs(ui),
                WorkTool::MarkEdges => self.mark_inputs(ui),
                WorkTool::LineStyle => self.line_inputs(ui),
                _ => {}
            }
            return;
        }
        let Some(t) = self.draw_tool.as_ref() else {
            // Collapsed to the title when there is nothing to show.
            if !self.typed.is_empty() {
                ui.label(format!("Typed: {}", self.typed));
            }
            return;
        };
        let (name, fields, primary) = (t.name(), t.fields(), t.primary_field());
        ui.label(name);
        if self.pending_owner != Some(name) {
            self.pending.clear();
            self.pending_owner = Some(name);
        }
        self.pending.resize(fields.len(), None);
        self.field_text.resize(fields.len(), String::new());
        let field_width = (ui.available_width() - SETTINGS_LABEL_ROOM).max(SETTINGS_FIELD_MIN);
        let mut leaving = None;
        let mut switched = None;
        let mut confirm = false;
        egui::Grid::new("operation_inputs")
            .num_columns(3)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                for (i, f) in fields.iter().enumerate() {
                    ui.label(f.label);
                    let id = ui.make_persistent_id(("settings_field", name, i));
                    let focused = ui.memory(|m| m.has_focus(id));
                    let typing_here = i == primary && !self.typed.is_empty();
                    if !focused {
                        self.field_text[i] = match &self.pending[i] {
                            Some(text) => text.clone(),
                            None if typing_here => self.typed.clone(),
                            None => field_display(f, units),
                        };
                    }
                    // One of several: a drop-down, applied at once.
                    if let FieldKind::Choice(options) = f.kind {
                        let current = f.value.map_or(0, |v| v.max(0.0) as usize);
                        let mut chosen = current;
                        ui.add_enabled_ui(f.editable, |ui| {
                            egui::ComboBox::from_id_salt(id)
                                .selected_text(options.get(current).copied().unwrap_or(""))
                                .show_ui(ui, |ui| {
                                    for (k, o) in options.iter().enumerate() {
                                        ui.selectable_value(&mut chosen, k, *o);
                                    }
                                });
                        });
                        if chosen != current {
                            switched = Some((i, chosen as f64));
                        }
                        ui.label("");
                        ui.end_row();
                        continue;
                    }
                    if f.kind == FieldKind::Switch {
                        let stored = f.value.is_some_and(|v| v != 0.0);
                        let mut on = self.pending[i].as_deref().map_or(stored, |t| t == "1");
                        let resp =
                            ui.add_enabled(f.editable, egui::Checkbox::without_text(&mut on));
                        if resp.changed() {
                            switched = Some((i, if on { 1.0 } else { 0.0 }));
                        }
                        ui.label("");
                        ui.end_row();
                        continue;
                    }
                    let mut edit = TextEdit::singleline(&mut self.field_text[i])
                        .id(id)
                        .desired_width(field_width);
                    if self.pending[i].is_some() {
                        edit = edit.text_color(PENDING_COLOR);
                    } else if typing_here {
                        edit = edit.text_color(TYPING_COLOR);
                    }
                    let resp = ui.add_enabled(f.editable, edit);
                    if resp.gained_focus() && self.pending[i].is_none() {
                        self.field_text[i] = f
                            .value
                            .map_or_else(String::new, |v| exact_text(f, v, units));
                    }
                    if resp.changed() {
                        self.pending[i] = Some(self.field_text[i].clone());
                    }
                    if resp.lost_focus() {
                        let (enter, escape) = ui.input(|inp| {
                            (inp.key_pressed(Key::Enter), inp.key_pressed(Key::Escape))
                        });
                        if escape {
                            self.pending[i] = None;
                        } else if let Some(text) = self.pending[i].take() {
                            leaving = Some((i, f.kind, text));
                        }
                        confirm |= enter;
                    }
                    ui.label(match f.kind {
                        FieldKind::Length => unit_abbrev(units.unit),
                        _ => "",
                    });
                    ui.end_row();
                    if let Some(note) = f.note {
                        ui.label("");
                        ui.add(egui::Label::new(egui::RichText::new(note).small().weak()).wrap());
                        ui.end_row();
                    }
                }
            });
        if let Some((i, kind, text)) = leaving {
            self.submit_field(i, kind, &text, units);
        }
        if let Some((i, v)) = switched {
            self.apply_field(i, v);
        }
        if confirm {
            // This Enter confirms here; the canvas must not see it again.
            ui.input_mut(|inp| inp.consume_key(Modifiers::NONE, Key::Enter));
            self.accept_last();
        }
    }

    /// How many faces are turned the wrong way in the selection (or the
    /// open context when nothing is selected), for Statistics.
    pub fn reversed_count(&self) -> usize {
        self.reversed
    }

    /// Works out [`Canvas::reversed_count`] again.
    fn count_reversed(&mut self) {
        let e = self.active();
        let faces: Vec<_> = if self.selection.is_empty() {
            e.face_ids().collect()
        } else {
            let points: Vec<DVec3> = smith_ops::modify::item_vertices(e, &self.selection.ordered())
                .into_iter()
                .filter_map(|v| e.vertex(v).map(smith_model::Vertex::position))
                .collect();
            smith_ops::solid::faces_in(e, Some(&points))
        };
        self.reversed = smith_ops::solid::reversed_faces(e, &faces).len();
    }

    /// What the Select tool picks in Edit mode.
    pub fn select_kind(&self) -> SelectKind {
        self.select_kind
    }

    /// Sets what the Select tool picks, converting the selection: going up
    /// keeps only complete items, going down keeps every part. Vertices
    /// already selected keep their pick order.
    pub fn set_select_kind(&mut self, kind: SelectKind) {
        if kind == self.select_kind {
            return;
        }
        self.select_kind = kind;
        let before = self.selection.ordered();
        let after = self.active().converted(&before, kind);
        let order: Vec<Pickable> = before
            .iter()
            .filter(|p| matches!(p, Pickable::Vertex(_)) && after.contains(p))
            .chain(
                after
                    .iter()
                    .filter(|p| !before.contains(p) || !matches!(p, Pickable::Vertex(_))),
            )
            .copied()
            .collect();
        let order = self.unlocked(order);
        self.selection.apply(order, SelectMode::Replace);
        self.count_reversed();
    }

    /// Holds the canvas's input (keys, clicks, typed values) while a window
    /// over it waits for an answer.
    pub fn hold(&mut self, on: bool) {
        self.on_hold = on;
    }

    /// Whether the canvas takes no input this frame: a window over it, or
    /// its own question, waits for an answer.
    pub fn input_held(&self) -> bool {
        self.on_hold || self.dialog().is_some()
    }

    /// The question waiting for a Yes or No, if any; the canvas takes no
    /// input until it is answered.
    pub fn dialog(&self) -> Option<Dialog> {
        if self.auto_tri_offer.is_some() {
            Some(Dialog::AutoTriangulate)
        } else if let Some(id) = self.layer_offer {
            let (faces, groups, edges) = self.document.layer_content_count(id);
            Some(Dialog::RemoveLayer(id, faces, groups, edges))
        } else {
            self.material_offer
                .as_ref()
                .map(|(_, n)| Dialog::RemoveMaterials(*n))
        }
    }

    /// The answer to [`Canvas::dialog`]: Yes carries the offer out, No
    /// leaves things as they are.
    pub fn answer_dialog(&mut self, yes: bool) {
        if self.auto_tri_offer.is_some() {
            if yes {
                self.retry_auto_tri();
            } else {
                self.auto_tri_offer = None;
            }
        } else if let Some(id) = self.layer_offer.take() {
            if yes {
                self.layer_op("Remove Layer", |d| d.remove_layer(id));
                self.after_change();
            }
        } else if self.material_offer.is_some() {
            if yes {
                self.remove_materials();
            } else {
                self.material_offer = None;
                self.message = None;
            }
        }
    }

    /// Applies every pending Operation Inputs edit, in field order, and clears
    /// them. Returns `false` if there were none. All changes to a shape that
    /// is already drawn redefine it, so together they are one undo step.
    fn apply_pending(&mut self, units: &UnitSettings) -> bool {
        let edits: Vec<(usize, String)> = std::mem::take(&mut self.pending)
            .into_iter()
            .enumerate()
            .filter_map(|(i, p)| p.map(|t| (i, t)))
            .collect();
        if edits.is_empty() {
            return false;
        }
        for (i, text) in edits {
            let Some(kind) = self
                .draw_tool
                .as_ref()
                .and_then(|t| t.fields().get(i).map(|f| f.kind))
            else {
                continue;
            };
            if kind == FieldKind::Switch {
                self.apply_field(i, if text == "1" { 1.0 } else { 0.0 });
            } else {
                self.submit_field(i, kind, &text, units);
            }
        }
        true
    }

    /// Applies an edited Operation Inputs field.
    fn submit_field(&mut self, index: usize, kind: FieldKind, text: &str, units: &UnitSettings) {
        let plain = || parse_length(text, LengthUnit::Millimetre).ok();
        let value = match (kind, read_count(text), parse_typed(text, units.unit)) {
            (FieldKind::Count, Some(n), _) => f64::from(n),
            (FieldKind::Number, _, _) if plain().is_some() => plain().unwrap_or_default(),
            (FieldKind::Length, _, Ok(TypedValue::Length(v) | TypedValue::Radius(v))) => v,
            (_, _, Err(e)) => {
                self.message = Some(format!("Couldn't read “{text}”: {e}"));
                return;
            }
            _ => {
                self.message = Some(format!("“{text}” doesn't fit this field."));
                return;
            }
        };
        self.apply_field(index, value);
    }

    /// Sets Operation Inputs field `index` of the draw tool to `value`.
    fn apply_field(&mut self, index: usize, value: f64) {
        let pixel = self.inference.map_or(0.0, |inf| self.pixel_at(inf.point));
        let Some(t) = self.draw_tool.as_mut() else {
            return;
        };
        let ctx = Ctx {
            entities: ctx_of(&self.document, self.ctx),
            axes: &self.axes_ctx,
            instances: &self.instance_boxes,
            instance_lines: &self.instance_lines,
            pixel,
            cursor: self.cursor,
        };
        let outcome = t.edit(index, value, &ctx);
        self.run_outcome(outcome);
    }

    /// Enter with nothing typed: the shape is accepted and the box resets.
    /// Enter: confirms the shape and returns to the Select tool. The tool's key
    /// or button starts the next shape; F9 still reopens the last one.
    fn accept_last(&mut self) {
        self.pending.clear();
        if let Some(t) = self.draw_tool.as_mut() {
            t.accept();
        }
        if matches!(self.tool, WorkTool::Draw(_) | WorkTool::EditCurve) {
            let tool = self.tool;
            let still_open = self.redefinable;
            if let Some(t) = self.draw_tool.take()
                && still_open
            {
                self.confirmed = Some((tool, t));
            }
            self.set_tool(WorkTool::Select);
        }
    }

    /// Sends one input to the draw tool and carries out what it asks for.
    fn tool_input(&mut self, input: Input) {
        let pixel = self.inference.map_or(0.0, |inf| self.pixel_at(inf.point));
        let Some(t) = self.draw_tool.as_mut() else {
            return;
        };
        let ctx = Ctx {
            entities: ctx_of(&self.document, self.ctx),
            axes: &self.axes_ctx,
            instances: &self.instance_boxes,
            instance_lines: &self.instance_lines,
            pixel,
            cursor: self.cursor,
        };
        let outcome = t.handle(input, &ctx);
        self.run_outcome(outcome);
    }

    /// The curve of the selected edges, if they all belong to one curve.
    fn selected_curve(&self) -> Option<smith_model::CurveId> {
        let root = self.active();
        let mut curve = None;
        for p in self.selection.iter() {
            let Pickable::Edge(e) = p else { continue };
            let c = root.edge(e)?.curve()?;
            if curve.is_some_and(|k| k != c) {
                return None;
            }
            curve = Some(c);
        }
        curve
    }

    /// Starts Edit Curve on `id` (context menu).
    fn edit_curve(&mut self, id: smith_model::CurveId) {
        if self.mode != Mode::Edit {
            return;
        }
        let Some(tool) = CurveEditTool::new(id, self.active()) else {
            self.message = Some("This curve has nothing to edit.".into());
            return;
        };
        self.set_tool(WorkTool::Select);
        self.tool = WorkTool::EditCurve;
        self.draw_tool = Some(Box::new(tool));
    }

    /// F9 / Edit → Adjust Last Operation: the last shape's values come back.
    fn adjust_last(&mut self) {
        if !self.redefinable
            && let Some((tool, t)) = self.confirmed.take()
        {
            // Back into the confirmed shape's tool, still able to redefine it.
            self.set_tool(WorkTool::Select);
            self.tool = tool;
            self.draw_tool = Some(t);
            self.redefinable = true;
        }
        let reopened = self.redefinable && self.draw_tool.as_mut().is_some_and(|t| t.reopen());
        if !reopened {
            self.message = Some("There is no shape to adjust.".into());
        }
    }

    /// The active draw tool's status hint, if a draw tool is active.
    pub fn tool_status(&self) -> Option<String> {
        if self.paused() {
            return Some(
                "Adjust Values in Operation Inputs · Enter or Esc confirms, returns to Select"
                    .into(),
            );
        }
        let around = self.pivot_active
            && matches!(
                self.tool,
                WorkTool::Draw(DrawKind::Rotate | DrawKind::Scale | DrawKind::Mirror)
            );
        self.draw_tool.as_ref().map(|t| {
            if around {
                format!("{} · Around Pivot", t.status())
            } else {
                t.status()
            }
        })
    }

    /// A draw tool has made a shape that is still open for adjusting (and no
    /// new operation is in progress): canvas clicks are ignored until the
    /// shape is confirmed.
    fn paused(&self) -> bool {
        matches!(self.tool, WorkTool::Draw(_))
            && self.redefinable
            && self
                .draw_tool
                .as_ref()
                .is_some_and(|t| t.pauses() && t.last_point().is_none())
    }

    fn set_tool(&mut self, tool: WorkTool) {
        if let WorkTool::Draw(_) = tool
            && self.mode == Mode::Present
        {
            self.message = Some("Drawing works in Edit mode; press Tab to switch.".into());
            return;
        }
        if tool.is_look() && self.mode == Mode::Edit {
            self.message = Some("This works in Present mode; press Tab to switch.".into());
            return;
        }
        self.last_paint = None;
        self.pending.clear();
        self.draw_tool = match tool {
            WorkTool::Draw(kind) => Some(kind.create()),
            _ => None,
        };
        if let Some(t) = self.draw_tool.as_mut() {
            let items: Vec<Pickable> = self.selection.ordered();
            t.use_selection(&items);
            t.use_pivot(
                self.pivot_active
                    .then_some(self.local.inverse().transform_point3(self.pivot)),
            );
            t.use_proportional(self.proportional_on.then_some(self.proportional));
        }
        self.redefinable = false;
        self.message = None;
        self.bent.clear();
        self.auto_tri_offer = None;
        self.material_offer = None;
        self.report.clear();
        self.by_type = None;
        self.tool = tool;
        self.nav = NavTool::None;
        self.inference = None;
        self.lock = Lock::None;
        self.arrow_lock = None;
        self.tool_axis = None;
        self.tracked.clear();
        self.typed.clear();
        // A tool that acts as it starts (Inspect Faces looks at once).
        if let Some(t) = self.draw_tool.as_mut() {
            let ctx = Ctx {
                entities: ctx_of(&self.document, self.ctx),
                axes: &self.axes_ctx,
                instances: &self.instance_boxes,
                instance_lines: &self.instance_lines,
                pixel: 0.0,
                cursor: self.cursor,
            };
            let outcome = t.start(&ctx);
            self.run_outcome(outcome);
        }
    }

    /// The current mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Switches mode. The selection is kept, and shown and editable in both.
    pub fn set_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            if mode == Mode::Present {
                self.nav = NavTool::None;
                self.drag_start = None;
            }
            self.refresh_mesh();
        }
    }

    /// The Edit-mode display style.
    pub fn edit_style(&self) -> RenderStyle {
        self.edit_style
    }

    /// Sets the Edit-mode display style.
    pub fn set_edit_style(&mut self, style: RenderStyle) {
        self.edit_style = style;
    }

    /// The Present-mode look.
    pub fn look(&self) -> Look {
        self.look
    }

    /// Sets the Present-mode look, rebuilding the mesh if smoothing changed.
    pub fn set_look(&mut self, look: Look) {
        let mesh_changed = look.mesh != self.look.mesh;
        self.look = look;
        if mesh_changed {
            self.refresh_mesh();
        }
    }

    /// Replaces the document and frames it.
    pub fn set_document(&mut self, document: Document) {
        self.render_only = false;
        self.document = document;
        self.end_redefinition();
        self.selection.clear();
        self.editing.clear();
        self.resolve_context();
        self.shown_bounds = self.document.model_bounds();
        self.rebuild_snap();
        self.refresh_mesh();
        self.apply(Action::ZoomExtents);
    }

    /// Replaces the document with one read from a file, restoring its view and
    /// drawing axes. Without a saved view the model is framed.
    pub fn load(&mut self, document: Document, view: Option<SavedView>, axes: Option<SavedAxes>) {
        self.set_tool(WorkTool::Select);
        self.set_document(document);
        if let Some(v) = view {
            self.camera.target = v.target;
            self.camera.distance = v.distance;
            self.camera.yaw = v.yaw;
            self.camera.pitch = v.pitch;
            self.camera.fov_y = v.fov_y;
            self.camera.projection = if v.perspective {
                Projection::Perspective
            } else {
                Projection::Parallel
            };
        }
        self.set_axes(axes.map_or_else(DrawingAxes::default, |a| DrawingAxes {
            origin: a.origin,
            x: a.x,
            y: a.y,
            z: a.z,
        }));
        self.message = None;
    }

    // ----- renders (scenes) ------------------------------------------------
    //
    // On screen a scene is a render: Save Render keeps one version of how
    // the item is shown.

    /// The scenes, in order: key and name.
    pub fn scene_list(&self) -> Vec<(SceneId, String)> {
        self.document
            .scenes()
            .map(|(id, s)| (id, s.name.clone()))
            .collect()
    }

    /// What the camera, mode and layers are now, as a scene named `name`.
    fn scene_now(&self, name: String) -> Scene {
        let c = &self.camera;
        Scene {
            name,
            view: SceneView {
                target: c.target,
                distance: c.distance,
                yaw: c.yaw,
                pitch: c.pitch,
                perspective: c.projection == Projection::Perspective,
                fov_y: c.fov_y,
            },
            present: self.mode == Mode::Present,
            style: {
                let (faces, back_edges) = match self.mode {
                    Mode::Edit => (self.edit_style.faces, self.edit_style.back_edges),
                    Mode::Present => (self.look.faces, self.look.back_edges),
                };
                SceneStyle {
                    faces: face_display(faces),
                    back_edges,
                }
            },
            hidden_layers: self.document.hidden_layers(),
            section: self.document.root().active_section().map(|(k, _)| k),
        }
    }

    /// Save Render: adds a scene of the current view (one undo step).
    pub fn add_scene(&mut self) {
        let scene = self.scene_now(self.document.next_scene_name());
        match self
            .document
            .transaction("Save Render", |d| d.add_scene(scene))
        {
            Ok(_) => self.after_change(),
            Err(e) => self.message = Some(format!("Couldn't save the render: {e}.")),
        }
    }

    /// Restores scene `id`: its camera, mode and layers. Showing or hiding
    /// layers is one undo step, named after the scene.
    pub fn show_scene(&mut self, id: SceneId) {
        let Some(scene) = self.document.scene(id).cloned() else {
            return;
        };
        self.last_scene = Some(id);
        self.scene_camera(&scene);
        let faces = face_style(scene.style.faces);
        if scene.present {
            self.look.faces = faces;
            self.look.back_edges = scene.style.back_edges;
            self.set_mode(Mode::Present);
        } else {
            self.edit_style.faces = faces;
            self.edit_style.back_edges = scene.style.back_edges;
            self.set_mode(Mode::Edit);
        }
        match self
            .document
            .transaction(scene.name.clone(), |d| d.apply_scene_layers(id))
        {
            Ok(()) => self.after_change(),
            Err(e) => self.message = Some(format!("Couldn't show the render: {e}.")),
        }
    }

    /// Puts the camera where `scene` has it.
    fn scene_camera(&mut self, scene: &Scene) {
        let v = scene.view;
        self.camera.target = v.target;
        self.camera.distance = v.distance;
        self.camera.yaw = v.yaw;
        self.camera.pitch = v.pitch;
        self.camera.fov_y = v.fov_y;
        self.camera.projection = if v.perspective {
            Projection::Perspective
        } else {
            Projection::Parallel
        };
    }

    /// Adds an import as one undo step, `Import`: `add` puts it into the
    /// model in the open context, placed by the drawing axes, and returns
    /// its group and what it reports. The group is left selected.
    ///
    /// # Errors
    /// The model's error, with nothing changed.
    pub fn add_import<R>(
        &mut self,
        add: impl FnOnce(
            &mut Document,
            Option<DefinitionId>,
            DMat4,
        ) -> Result<(InstanceId, R), ModelError>,
    ) -> Result<R, ModelError> {
        let a = self.axes_ctx;
        let at = DMat4::from_cols(
            a.x.extend(0.0),
            a.y.extend(0.0),
            a.z.extend(0.0),
            a.origin.extend(1.0),
        );
        let ctx = self.ctx;
        let (id, out) = self.document.transaction("Import", |d| add(d, ctx, at))?;
        self.after_change();
        self.select(vec![Pickable::Instance(id)], SelectMode::Replace);
        Ok(out)
    }

    /// The render last restored, if it still exists.
    pub fn last_scene(&self) -> Option<SceneId> {
        self.last_scene
            .filter(|id| self.document.scene(*id).is_some())
    }

    /// Runs `f` on the canvas as render `id` shows the model (its camera,
    /// mode, display style, hidden layers and section plane), then puts the
    /// canvas back as it was. The render is applied to a copy of the model,
    /// so nothing is changed or recorded for undo. `None` without such a
    /// render.
    pub fn with_scene<R>(&mut self, id: SceneId, f: impl FnOnce(&mut Self) -> R) -> Option<R> {
        let scene = self.document.scene(id).cloned()?;
        let mut copy = self.document.clone();
        copy.transaction(scene.name.clone(), |d| d.apply_scene_layers(id))
            .ok()?;
        let saved = (self.camera, self.mode, self.edit_style, self.look);
        let original = std::mem::replace(&mut self.document, copy);
        self.scene_camera(&scene);
        let faces = face_style(scene.style.faces);
        if scene.present {
            self.look.faces = faces;
            self.look.back_edges = scene.style.back_edges;
            self.mode = Mode::Present;
        } else {
            self.edit_style.faces = faces;
            self.edit_style.back_edges = scene.style.back_edges;
            self.mode = Mode::Edit;
        }
        let out = f(self);
        self.document = original;
        (self.camera, self.mode, self.edit_style, self.look) = saved;
        Some(out)
    }

    /// Updates scene `id` to the current view, keeping its name.
    pub fn update_scene(&mut self, id: SceneId) {
        let Some(name) = self.document.scene(id).map(|s| s.name.clone()) else {
            return;
        };
        let scene = self.scene_now(name);
        match self
            .document
            .transaction("Update Render", |d| d.set_scene(id, scene))
        {
            Ok(()) => self.after_change(),
            Err(e) => self.message = Some(format!("Couldn't update the render: {e}.")),
        }
    }

    /// Renames scene `id`.
    pub fn rename_scene(&mut self, id: SceneId, name: &str) {
        let Some(mut scene) = self.document.scene(id).cloned() else {
            return;
        };
        if name.trim().is_empty() || name == scene.name {
            return;
        }
        scene.name = name.trim().to_string();
        match self
            .document
            .transaction("Rename Render", |d| d.set_scene(id, scene))
        {
            Ok(()) => self.after_change(),
            Err(e) => self.message = Some(format!("Couldn't rename the render: {e}.")),
        }
    }

    /// Removes scene `id`.
    pub fn remove_scene(&mut self, id: SceneId) {
        match self
            .document
            .transaction("Remove Render", |d| d.remove_scene(id))
        {
            Ok(_) => self.after_change(),
            Err(e) => self.message = Some(format!("Couldn't remove the render: {e}.")),
        }
    }

    /// The current view, for saving.
    pub fn saved_view(&self) -> SavedView {
        let c = &self.camera;
        SavedView {
            target: c.target,
            distance: c.distance,
            yaw: c.yaw,
            pitch: c.pitch,
            perspective: c.projection == Projection::Perspective,
            fov_y: c.fov_y,
            eye: c.eye(),
            up: c.up(),
            half_height: c.half_height(),
            aspect: self.aspect(),
        }
    }

    /// The drawing axes, for saving (`None` while they are the world axes).
    pub fn saved_axes(&self) -> Option<SavedAxes> {
        let a = self.axes;
        (a != DrawingAxes::default()).then_some(SavedAxes {
            origin: a.origin,
            x: a.x,
            y: a.y,
            z: a.z,
        })
    }

    /// Counts changes to the drawing axes; compared against the value at the
    /// last save to tell whether there are unsaved changes.
    pub fn axes_serial(&self) -> u64 {
        self.axes_serial
    }

    /// The document's revision (see `Document::revision`).
    pub fn revision(&self) -> u64 {
        self.document.revision()
    }

    /// File actions triggered by keys since the last call.
    pub fn take_file_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.file_actions)
    }

    fn set_axes(&mut self, axes: DrawingAxes) {
        if axes != self.axes {
            self.axes = axes;
            self.axes_serial += 1;
        }
    }

    /// Shows a render-only mesh (stress testing) with an empty document.
    pub fn set_render_only(&mut self, mesh: SceneMesh) {
        self.render_only = true;
        self.document = Document::new();
        self.end_redefinition();
        self.selection.clear();
        self.editing.clear();
        self.resolve_context();
        self.snap = None;
        self.bvh = FaceBvh::default();
        self.pick_index = Default::default();
        self.shown_bounds = mesh_bounds(&mesh);
        self.pending_mesh = Some(mesh);
        self.apply(Action::ZoomExtents);
    }

    /// Rebuilds the display mesh (after a selection or geometry change).
    fn refresh_mesh(&mut self) {
        if self.render_only {
            return;
        }
        self.shown_mesh = None;
        self.pending_mesh = Some(self.build_mesh(&self.selection));
    }

    /// Shows a new selection: the mesh re-marked in place when it can be
    /// (Edit mode, no group open: see [`SceneMesh::spans`]), else rebuilt.
    fn reselect_mesh(&mut self) {
        if self.render_only {
            return;
        }
        let mesh = self.pending_mesh.take().or_else(|| self.shown_mesh.take());
        match mesh {
            Some(mut m) if !m.spans.is_empty() => {
                m.reselect(&self.selection);
                self.pending_mesh = Some(m);
            }
            _ => self.refresh_mesh(),
        }
    }

    /// A picture of the canvas's view, `width` × `height` pixels, for PNG:
    /// what the canvas shows (its camera, mode, style and look, shown
    /// layers, section cut, dimensions and text, and guides and section
    /// planes while View shows them), without the selection, the axes or
    /// anything drawn only while working. Overlays grow with the picture.
    /// A transparent background leaves only the model and its marks.
    /// Returns RGBA pixels, sRGB, rows top first, straight alpha.
    ///
    /// # Errors
    /// A message when there is no GPU or the picture cannot be read back.
    pub fn picture(
        &mut self,
        ctx: &egui::Context,
        width: u32,
        height: u32,
        transparent: bool,
    ) -> Result<Vec<u8>, String> {
        let Some(rs) = self.render_state.clone() else {
            return Err("no GPU renderer".into());
        };
        self.picture_on(ctx, (&rs.device, &rs.queue), width, height, transparent)
    }

    /// [`Self::picture`] on a given GPU device and queue.
    fn picture_on(
        &mut self,
        ctx: &egui::Context,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        width: u32,
        height: u32,
        transparent: bool,
    ) -> Result<Vec<u8>, String> {
        let ppp = ctx.pixels_per_point();
        // Picture points per canvas point: overlays and lines grow with it.
        let k = width as f32 / ppp / self.rect.width().max(1.0);

        // The overlays, worked out on a canvas the picture's size.
        let saved = (self.rect, self.overlay_scale);
        self.rect = Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(width as f32 / ppp, height as f32 / ppp),
        );
        self.overlay_scale = k;
        let mut shapes = self.annotation_paint(ctx);
        if self.mode == Mode::Edit && self.show_sections {
            shapes.extend(self.section_paint());
        }
        if self.mode == Mode::Edit && self.show_guides {
            shapes.extend(self.guide_paint());
        }
        let full = self.rect;
        (self.rect, self.overlay_scale) = saved;
        let clipped: Vec<egui::epaint::ClippedShape> = shapes
            .into_iter()
            .map(|shape| egui::epaint::ClippedShape {
                clip_rect: full,
                shape,
            })
            .collect();
        let prims = ctx.tessellate(clipped, ppp);

        // The model, in a renderer of its own: the canvas's is untouched.
        let mut renderer = CanvasRenderer::new(device, queue);
        renderer.set_axes_shown(false);
        renderer.upload(
            device,
            queue,
            &self.build_mesh(&Selection::new()),
            &|image| smith_io::decode_for_display(image).ok().map(|d| d.levels),
        );
        let mut style = render_style(
            self.mode,
            &self.edit_style,
            &self.look,
            self.section_cuts,
            &self.document,
            ppp * k,
        );
        if transparent {
            style.background = [0.0; 4];
        }
        let out = renderer.render(device, queue, &self.camera, &style, width, height);

        // The overlays on top, through egui's own renderer and font atlas.
        let mut painter = egui_wgpu::Renderer::new(
            device,
            smith_render::renderer::COLOR_FORMAT,
            egui_wgpu::RendererOptions::default(),
        );
        painter.update_texture(
            device,
            queue,
            egui::TextureId::default(),
            &egui::epaint::ImageDelta::full(ctx.fonts(|f| f.image()), egui::TextureOptions::LINEAR),
        );
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: ppp,
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("picture overlays"),
        });
        let extra = painter.update_buffers(device, queue, &mut encoder, &prims, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("picture overlays"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: out.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            painter.render(&mut pass.forget_lifetime(), &prims, &screen);
        }
        queue.submit(extra.into_iter().chain([encoder.finish()]));

        let (_, _, mut px) = renderer
            .read_pixels(device, queue)
            .ok_or("the picture could not be read back")?;
        if transparent {
            unpremultiply(&mut px);
        }
        Ok(px)
    }

    /// The canvas's own size in pixels.
    pub fn pixel_size(&self, ppp: f32) -> [u32; 2] {
        [
            (self.rect.width() * ppp).round().max(1.0) as u32,
            (self.rect.height() * ppp).round().max(1.0) as u32,
        ]
    }

    /// The scene as the canvas shows it, with `selection` highlighted.
    fn build_mesh(&self, selection: &Selection) -> SceneMesh {
        let doc = &self.document;
        let root = doc.root();
        let vis = visibility(doc);
        let editing = self.mode == Mode::Edit && !self.editing.is_empty();
        let mut mesh = if editing {
            // Inside a group: everything outside faded, the edited geometry
            // with its selection.
            let mut m = SceneMesh {
                origin: doc
                    .model_bounds()
                    .map_or(DVec3::ZERO, |(lo, hi)| (lo + hi) * 0.5),
                visibility: vis.clone(),
                ..SceneMesh::default()
            };
            m.append(root, &DMat4::IDENTITY, Shown::Faded);
            append_path(
                &mut m,
                doc,
                root,
                &self.editing,
                &DMat4::IDENTITY,
                selection,
                self.section_cuts,
            );
            m
        } else {
            match self.mode {
                Mode::Edit => SceneMesh::build_shown(root, selection, &vis),
                Mode::Present => {
                    let paint = std::sync::Arc::new(PresentPaint {
                        smoothing_angle_deg: doc.look().smoothing_angle_deg,
                        materials: doc
                            .material_ids()
                            .filter_map(|id| doc.material(id).map(|m| (id, m.clone())))
                            .collect(),
                        layer_styles: doc
                            .layer_ids()
                            .filter_map(|id| {
                                let l = doc.layer(id)?;
                                // The Base layer is stored as `None` on edges.
                                let key = (Some(id) != doc.base_layer()).then_some(id);
                                l.style.map(|s| (key, s))
                            })
                            .collect(),
                    });
                    SceneMesh::build_present_shown(
                        root,
                        self.look.mesh,
                        &paint,
                        &vis,
                        Some(selection),
                    )
                }
            }
        };
        let cuts = self.section_cuts;
        if !editing {
            for id in root.instance_ids() {
                let shown = if self.mode == Mode::Edit && selection.contains(Pickable::Instance(id))
                {
                    Shown::Selected
                } else {
                    Shown::Plain
                };
                if let Some(inst) = root.instance(id) {
                    let start = (mesh.faces.len(), mesh.edges.len());
                    append_instance(&mut mesh, doc, inst, &DMat4::IDENTITY, shown, cuts, 0);
                    // Present mode has no spans: it is rebuilt to reselect.
                    if self.mode == Mode::Edit {
                        mesh.spans.push(smith_render::mesh::Span {
                            item: Pickable::Instance(id),
                            faces: start.0..mesh.faces.len(),
                            edges: start.1..mesh.edges.len(),
                        });
                    }
                }
            }
        }
        if let Some(plane) = self.cut_plane() {
            add_section_fill(&mut mesh, doc, &plane);
        }
        mesh
    }

    /// The root's active section plane, while cuts are on.
    fn cut_plane(&self) -> Option<SectionPlane> {
        if !self.section_cuts {
            return None;
        }
        self.document.root().active_section().map(|(_, p)| *p)
    }

    /// Each section plane of the context being edited as drawn on screen:
    /// its key, whether it is active, and its outline.
    fn section_outlines(&self) -> Vec<(SectionId, bool, Vec<[Pos2; 2]>)> {
        let size = self
            .document
            .model_bounds()
            .map_or(50.0, |(lo, hi)| lo.distance(hi) * 0.6)
            .max(10.0);
        let scr = |p: DVec3| self.to_screen(p).map(|s| pos2(s.x as f32, s.y as f32));
        self.active()
            .sections()
            .map(|(id, s)| {
                let u = s.normal.any_orthonormal_vector() * size * 0.5;
                let v = s.normal.cross(u.normalize()) * size * 0.5;
                let c = [
                    s.point - u - v,
                    s.point + u - v,
                    s.point + u + v,
                    s.point - u + v,
                ];
                let mut lines = Vec::new();
                for i in 0..4 {
                    if let (Some(a), Some(b)) = (scr(c[i]), scr(c[(i + 1) % 4])) {
                        lines.push([a, b]);
                    }
                    // Arrows at the corners, toward the side cut away.
                    let tip = c[i] + s.normal * size * 0.12;
                    if let (Some(a), Some(b)) = (scr(c[i]), scr(tip)) {
                        lines.push([a, b]);
                    }
                }
                (id, s.active, lines)
            })
            .collect()
    }

    /// The section planes of the context being edited: grey outlines, the
    /// active one orange.
    fn draw_sections(&self, ui: &Ui) {
        ui.painter()
            .with_clip_rect(self.rect)
            .extend(self.section_paint());
    }

    /// [`Self::draw_sections`]'s shapes.
    fn section_paint(&self) -> Vec<egui::Shape> {
        let mut out = Vec::new();
        for (_, active, lines) in self.section_outlines() {
            let colour = if active {
                Color32::from_rgb(230, 130, 20)
            } else {
                Color32::from_gray(120)
            };
            for l in lines {
                out.push(egui::Shape::line_segment(
                    l,
                    Stroke::new(1.5 * self.overlay_scale, colour),
                ));
            }
        }
        out
    }

    /// The section plane whose outline is under screen position `at`.
    fn section_at(&self, at: Pos2) -> Option<(SectionId, bool)> {
        self.section_outlines()
            .into_iter()
            .find(|(_, _, lines)| {
                lines.iter().any(|[a, b]| {
                    let ab = *b - *a;
                    let t = ((at - *a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                    (*a + ab * t).distance(at) <= 5.0
                })
            })
            .map(|(id, active, _)| (id, active))
    }

    /// Whether the context being edited has an active section plane.
    pub fn has_active_section(&self) -> bool {
        self.active().active_section().is_some()
    }

    /// Edit → Create Group from Slice: the active plane's cut through the
    /// context being edited, as a new group of edges (one undo step).
    pub fn create_group_from_slice(&mut self) {
        let Some((_, plane)) = self.active().active_section().map(|(k, p)| (k, *p)) else {
            self.message = Some("Turn a section plane on first.".into());
            return;
        };
        let doc = &self.document;
        let e = self.active();
        let slice = e.slice(&plane, |f| {
            e.face(f)
                .is_some_and(|x| !x.attributes.hidden && doc.layer_visible(x.attributes.layer))
        });
        if slice.segments.is_empty() {
            self.message = Some("The plane cuts nothing here.".into());
            return;
        }
        let ctx = self.ctx;
        let result = self.document.transaction("Create Group from Slice", |d| {
            let def = d.create_definition("Slice", DefinitionKind::Group);
            let g = d
                .context_entities_mut(Some(def))
                .ok_or(ModelError::NoSuchEntity)?;
            for [a, b] in &slice.segments {
                g.add_line(*a, *b)?;
            }
            d.add_instance(ctx, def, DMat4::IDENTITY, "")
        });
        match result {
            Ok(id) => {
                self.after_change();
                self.select(vec![Pickable::Instance(id)], SelectMode::Replace);
            }
            Err(e) => self.message = Some(format!("Couldn't make the slice: {e}.")),
        }
    }

    /// The geometry being edited: a group's or component's, or the root.
    fn active(&self) -> &Entities {
        ctx_of(&self.document, self.ctx)
    }

    /// Works out the edited definition and its placement from the path of
    /// open groups; a path broken by undo or erasing is cut back to what
    /// still exists. Carries the drawing axes into its coordinates.
    fn resolve_context(&mut self) {
        let mut ctx = None;
        let mut m = DMat4::IDENTITY;
        let mut kept = 0;
        for id in &self.editing {
            let Some(inst) = self.document.context(ctx).and_then(|c| c.instance(*id)) else {
                break;
            };
            ctx = Some(inst.definition);
            m *= inst.transform;
            kept += 1;
        }
        self.editing.truncate(kept);
        self.ctx = ctx;
        self.local = m;
        let inv = m.inverse();
        let dir = |v: DVec3| inv.transform_vector3(v).normalize_or(v);
        self.axes_ctx = DrawingAxes {
            origin: inv.transform_point3(self.axes.origin),
            x: dir(self.axes.x),
            y: dir(self.axes.y),
            z: dir(self.axes.z),
        };
    }

    /// Opens group or component `id` of the active context for editing.
    /// A group copied elsewhere is made unique first, so editing it leaves
    /// its copies alone (one undo step); a component stays shared.
    fn enter_instance(&mut self, id: InstanceId) {
        let Some(inst) = self.active().instance(id) else {
            return;
        };
        if inst.locked {
            self.message = Some("It is locked. Unlock it to edit it.".into());
            return;
        }
        if self.document.layer_locked(inst.layer) {
            self.message = Some(smith_model::ON_LOCKED_LAYER.into());
            return;
        }
        let def = inst.definition;
        let shared_group = self
            .document
            .definition(def)
            .is_some_and(|d| d.kind == DefinitionKind::Group)
            && self.document.instance_users(def) > 1;
        if shared_group {
            let ctx = self.ctx;
            if let Err(e) = self
                .document
                .transaction("Make Unique", |d| d.make_unique(ctx, id))
            {
                self.message = Some(format!("Couldn't open the group: {e}."));
                return;
            }
        }
        self.set_tool(WorkTool::Select);
        self.editing.push(id);
        self.resolve_context();
        self.after_change();
    }

    /// Closes the innermost open group or component; `false` at the root.
    fn leave_context(&mut self) -> bool {
        let Some(id) = self.editing.pop() else {
            return false;
        };
        self.resolve_context();
        self.after_change();
        self.select(vec![Pickable::Instance(id)], SelectMode::Replace);
        true
    }

    /// The cursor ray for `ndc`, in the edited definition's coordinates
    /// (the direction stays a unit vector).
    fn ray(&self, ndc: DVec2, aspect: f64) -> (DVec3, DVec3) {
        let (o, d) = self.camera.ray(ndc, aspect);
        let inv = self.local.inverse();
        (
            inv.transform_point3(o),
            inv.transform_vector3(d).normalize_or(d),
        )
    }

    fn select(&mut self, picked: Vec<Pickable>, mode: SelectMode) {
        let before = self.selection.clone();
        let picked = self.unlocked(picked);
        self.selection.apply(picked, mode);
        if self.selection != before {
            self.reselect_mesh();
            self.count_reversed();
        }
    }

    /// Whether `item`, in the open context, is on a locked layer: an edge,
    /// face or group on one, or a vertex an edge on one uses.
    fn on_locked_layer(&self, item: Pickable) -> bool {
        let doc = &self.document;
        let e = self.active();
        match item {
            Pickable::Edge(x) => e.edge(x).is_some_and(|x| doc.layer_locked(x.flags.layer)),
            Pickable::Face(x) => e
                .face(x)
                .is_some_and(|x| doc.layer_locked(x.attributes.layer)),
            Pickable::Instance(x) => e.instance(x).is_some_and(|x| doc.layer_locked(x.layer)),
            Pickable::Vertex(v) => e.vertex(v).is_some_and(|v| {
                v.edges()
                    .iter()
                    .any(|x| e.edge(*x).is_some_and(|x| doc.layer_locked(x.flags.layer)))
            }),
            Pickable::Guide(_) => false,
        }
    }

    /// `picked` without what is on a locked layer.
    fn unlocked(&self, picked: Vec<Pickable>) -> Vec<Pickable> {
        if !self
            .document
            .layer_ids()
            .any(|l| self.document.layer(l).is_some_and(|x| x.locked))
        {
            return picked;
        }
        picked
            .into_iter()
            .filter(|p| !self.on_locked_layer(*p))
            .collect()
    }

    fn aspect(&self) -> f64 {
        if self.rect.is_positive() {
            f64::from(self.rect.width() / self.rect.height())
        } else {
            1.0
        }
    }

    /// Performs a keymap or menu action.
    pub fn apply(&mut self, action: Action) {
        let view = |c: &mut Camera, v| c.set_view(v);
        match action {
            Action::OrbitTool => self.nav = NavTool::Orbit,
            Action::PanTool => self.nav = NavTool::Pan,
            Action::ZoomTool => self.nav = NavTool::Zoom,
            Action::SelectTool => self.set_tool(WorkTool::Select),
            Action::PaintTool => self.set_tool(WorkTool::Paint),
            Action::MarkEdgesTool => self.set_tool(WorkTool::MarkEdges),
            Action::LineStyleTool => self.set_tool(WorkTool::LineStyle),
            Action::Cancel => {
                // Esc: first clear typing, then the operation in progress, then the tool.
                if !self.typed.is_empty() {
                    self.typed.clear();
                } else if self.pending.iter().any(Option::is_some) {
                    self.pending.clear();
                } else if self.nav != NavTool::None {
                    self.nav = NavTool::None;
                } else {
                    self.arrow_lock = None;
                    self.tool_axis = None;
                    self.lock = Lock::None;
                    self.tracked.clear();
                    match self.tool {
                        WorkTool::Draw(_) => {
                            let busy = self.draw_tool.as_mut().is_some_and(|t| t.cancel());
                            if !busy {
                                // Like Enter: confirms a shape still open for
                                // adjusting, and returns to Select.
                                self.accept_last();
                            }
                        }
                        WorkTool::Axes {
                            origin: Some(_), ..
                        } => self.set_tool(WorkTool::Axes {
                            origin: None,
                            red: None,
                        }),
                        WorkTool::Select if !self.editing.is_empty() => {
                            self.leave_context();
                        }
                        _ => self.set_tool(WorkTool::Select),
                    }
                }
            }
            Action::Undo => self.undo(),
            Action::SelectVertices => self.set_select_kind(SelectKind::Vertices),
            Action::SelectEdges => self.set_select_kind(SelectKind::Edges),
            Action::SelectFaces => self.set_select_kind(SelectKind::Faces),
            Action::AdjustLast => self.adjust_last(),
            Action::Redo => self.redo(),
            Action::LineTool => self.set_tool(WorkTool::Draw(DrawKind::Line)),
            Action::RectangleTool => self.set_tool(WorkTool::Draw(DrawKind::Rectangle)),
            Action::RotatedRectangleTool => {
                self.set_tool(WorkTool::Draw(DrawKind::RotatedRectangle));
            }
            Action::CircleTool => self.set_tool(WorkTool::Draw(DrawKind::Circle)),
            Action::ArcTool => self.set_tool(WorkTool::Draw(DrawKind::Arc)),
            Action::PolygonTool => self.set_tool(WorkTool::Draw(DrawKind::Polygon)),
            Action::BezierTool => self.set_tool(WorkTool::Draw(DrawKind::Bezier)),
            Action::CurveMakerTool => self.set_tool(WorkTool::Draw(DrawKind::CurveMaker)),
            Action::ExtrudeTool => self.set_tool(WorkTool::Draw(DrawKind::Extrude)),
            Action::EraserTool => self.set_tool(WorkTool::Draw(DrawKind::Eraser)),
            Action::MoveTool => self.set_tool(WorkTool::Draw(DrawKind::Move)),
            Action::RotateTool => self.set_tool(WorkTool::Draw(DrawKind::Rotate)),
            Action::ScaleTool => self.set_tool(WorkTool::Draw(DrawKind::Scale)),
            Action::MirrorTool => self.set_tool(WorkTool::Draw(DrawKind::Mirror)),
            Action::OffsetTool => self.set_tool(WorkTool::Draw(DrawKind::Offset)),
            Action::DivideTool => self.set_tool(WorkTool::Draw(DrawKind::Divide)),
            Action::LoopCutTool => self.set_tool(WorkTool::Draw(DrawKind::LoopCut)),
            Action::KnifeTool => self.set_tool(WorkTool::Draw(DrawKind::Knife)),
            Action::SlideTool => self.set_tool(WorkTool::Draw(DrawKind::Slide)),
            Action::DissolveTool => self.set_tool(WorkTool::Draw(DrawKind::Dissolve)),
            Action::TapeMeasureTool => self.set_tool(WorkTool::Draw(DrawKind::TapeMeasure)),
            Action::ProtractorTool => self.set_tool(WorkTool::Draw(DrawKind::Protractor)),
            Action::FindCenterTool => self.set_tool(WorkTool::Draw(DrawKind::FindCenter)),
            Action::DimensionTool => self.set_tool(WorkTool::Draw(DrawKind::Dimension)),
            Action::TextTool => self.set_tool(WorkTool::Draw(DrawKind::Text)),
            Action::SectionTool => self.set_tool(WorkTool::Draw(DrawKind::Section)),
            Action::SweepTool => self.set_tool(WorkTool::Draw(DrawKind::Sweep)),
            Action::RevolveTool => self.set_tool(WorkTool::Draw(DrawKind::Revolve)),
            Action::ExtrudeEdgesTool => self.set_tool(WorkTool::Draw(DrawKind::ExtrudeEdges)),
            Action::BevelTool => self.set_tool(WorkTool::Draw(DrawKind::Bevel)),
            Action::WeldTool => self.set_tool(WorkTool::Draw(DrawKind::Weld)),
            Action::PercentTool => self.set_tool(WorkTool::Draw(DrawKind::Percent)),
            Action::SpaceTool => self.set_tool(WorkTool::Draw(DrawKind::Space)),
            Action::CircleLoopTool => self.set_tool(WorkTool::Draw(DrawKind::CircleLoop)),
            Action::FlattenTool => self.set_tool(WorkTool::Draw(DrawKind::Flatten)),
            Action::SmoothTool => self.set_tool(WorkTool::Draw(DrawKind::Smooth)),
            Action::RelaxTool => self.set_tool(WorkTool::Draw(DrawKind::Relax)),
            Action::IntersectTool => self.set_tool(WorkTool::Draw(DrawKind::Intersect)),
            Action::CleanUpTool => self.set_tool(WorkTool::Draw(DrawKind::CleanUp)),
            Action::CloseGapsTool => self.set_tool(WorkTool::Draw(DrawKind::CloseGaps)),
            Action::InspectFacesTool => self.set_tool(WorkTool::Draw(DrawKind::InspectFaces)),
            Action::InspectManifoldTool => self.set_tool(WorkTool::Draw(DrawKind::InspectManifold)),
            Action::MakeManifoldTool => self.set_tool(WorkTool::Draw(DrawKind::MakeManifold)),
            Action::FuseTool => self.set_tool(WorkTool::Draw(DrawKind::Fuse)),
            Action::TrimTool => self.set_tool(WorkTool::Draw(DrawKind::TrimExtend)),
            Action::BisectorTool => self.set_tool(WorkTool::Draw(DrawKind::Bisector)),
            Action::ExtendFaceTool => self.set_tool(WorkTool::Draw(DrawKind::ExtendToFace)),
            Action::BridgeTool => self.set_tool(WorkTool::Draw(DrawKind::Bridge)),
            Action::LoftTool => self.set_tool(WorkTool::Draw(DrawKind::Loft)),
            Action::SkinTool => self.set_tool(WorkTool::Draw(DrawKind::Skin)),
            Action::LoftSplineTool => self.set_tool(WorkTool::Draw(DrawKind::LoftSpline)),
            Action::CopyAlongPathTool => self.set_tool(WorkTool::Draw(DrawKind::CopyAlongPath)),
            Action::ShapesTool => self.set_tool(WorkTool::Draw(DrawKind::Shapes)),
            Action::CreateGroupFromSlice => self.create_group_from_slice(),
            Action::DeleteGuides => {
                if self.active().guide_count() == 0 {
                    self.message = Some("No guides to delete.".into());
                } else {
                    self.run_now(&DrawOp::DeleteGuides);
                }
            }
            Action::Fill => self.fill_selection(),
            Action::ReverseFaces => {
                let faces: Vec<_> = self.selection.faces().collect();
                if faces.is_empty() {
                    self.message = Some("Select faces to reverse.".into());
                } else {
                    self.run_now(&DrawOp::ReverseFaces { faces });
                }
            }
            Action::OrientFaces => {
                let faces: Vec<_> = self.selection.faces().collect();
                match faces[..] {
                    [face] => self.run_now(&DrawOp::OrientFaces { face }),
                    _ => self.message = Some("Select one face to orient from.".into()),
                }
            }
            Action::Delete => self.erase_selection(),
            Action::Hide => self.hide_selection(),
            Action::UnhideLast => {
                let edges = std::mem::take(&mut self.last_hidden);
                self.set_hidden(edges, false, "Unhide");
            }
            Action::UnhideAll => {
                let a = self.active();
                let items: Vec<Pickable> = a
                    .hidden_edges()
                    .into_iter()
                    .map(Pickable::Edge)
                    .chain(a.hidden_faces().into_iter().map(Pickable::Face))
                    .chain(a.hidden_instances().into_iter().map(Pickable::Instance))
                    .collect();
                self.last_hidden.clear();
                self.set_hidden(items, false, "Unhide");
            }
            Action::RepeatTransform => self.repeat_transform(),
            Action::Copy => {
                self.copy_selection();
            }
            Action::Cut => {
                if self.copy_selection() {
                    self.erase_selection();
                }
            }
            Action::Paste => self.paste(),
            Action::ToggleSidebar => self.sidebar_toggle = true,
            Action::Unhide => self.run_command(Command::Unhide),
            Action::HideOthers => self.run_command(Command::HideOthers),
            Action::Explode => {
                if self.command_applies(Command::Explode) {
                    self.run_command(Command::Explode);
                }
            }
            Action::Lock | Action::Unlock => {
                // Lock only an unlocked one, Unlock only a locked one.
                let want = action == Action::Lock;
                if self.single_instance().is_some() && self.single_locked() != want {
                    self.run_command(Command::Lock);
                }
            }
            Action::MakeGroup | Action::MakeComponent => {
                let c = if action == Action::MakeGroup {
                    Command::MakeGroup
                } else {
                    Command::MakeComponent
                };
                if self.command_applies(c) {
                    self.run_command(c);
                } else {
                    self.message = Some("Select edges or faces to group.".into());
                }
            }
            Action::ProportionalOn | Action::ProportionalOff => {
                let on = action == Action::ProportionalOn;
                self.set_proportional(on, self.proportional);
                self.message = Some(
                    if on {
                        "Proportional On"
                    } else {
                        "Proportional Off"
                    }
                    .into(),
                );
            }
            Action::ToggleProportional => {
                let on = !self.proportional_on;
                self.set_proportional(on, self.proportional);
                self.message = Some(
                    if on {
                        "Proportional On"
                    } else {
                        "Proportional Off"
                    }
                    .into(),
                );
            }
            Action::SelectByType => {
                self.set_tool(WorkTool::Select);
                self.by_type = Some(ByType {
                    scope: self.selection.iter().collect(),
                    ..ByType::default()
                });
            }
            Action::SelectConnected => {
                if self.selection.is_empty() {
                    self.message = Some("Select something first.".into());
                } else {
                    let root = self.active();
                    let all: Vec<Pickable> = self
                        .selection
                        .iter()
                        .flat_map(|p| root.connected_set(p))
                        .collect();
                    self.select(all, SelectMode::Add);
                }
            }
            Action::AxesTool => self.set_tool(WorkTool::Axes {
                origin: None,
                red: None,
            }),
            Action::NewFile
            | Action::OpenFile
            | Action::SaveFile
            | Action::SaveFileAs
            | Action::SaveVersion
            | Action::Quit => self.file_actions.push(action),
            Action::ResetAxes => {
                self.set_axes(DrawingAxes::default());
                self.message = Some("Drawing axes reset to the world axes.".into());
            }
            Action::SelectAll => {
                let all = self.active().all_pickables();
                self.select(all, SelectMode::Replace);
            }
            Action::SelectNone => self.select(Vec::new(), SelectMode::Replace),
            Action::ZoomExtents => {
                if let Some(b) = self.shown_bounds {
                    let aspect = self.aspect();
                    self.camera.zoom_extents(b, aspect);
                }
            }
            Action::ViewTop => view(&mut self.camera, StandardView::Top),
            Action::ViewBottom => view(&mut self.camera, StandardView::Bottom),
            Action::ViewFront => view(&mut self.camera, StandardView::Front),
            Action::ViewBack => view(&mut self.camera, StandardView::Back),
            Action::ViewLeft => view(&mut self.camera, StandardView::Left),
            Action::ViewRight => view(&mut self.camera, StandardView::Right),
            Action::ViewIso => view(&mut self.camera, StandardView::Iso),
            Action::ToggleProjection => self.camera.toggle_projection(),
            Action::Perspective => {
                self.camera.projection = Projection::Perspective;
            }
            Action::Parallel => {
                self.camera.projection = Projection::Parallel;
            }
            Action::ViewPie => self.pie_requested = true,
            Action::ToggleMode => self.set_mode(match self.mode {
                Mode::Edit => Mode::Present,
                Mode::Present => Mode::Edit,
            }),
        }
    }

    /// Whether the camera is in perspective.
    pub fn is_perspective(&self) -> bool {
        self.camera.projection == Projection::Perspective
    }

    /// Frames the current selection (Zoom Selection).
    pub fn zoom_selection(&mut self) {
        if let Some(b) = self.active().selection_bounds(&self.selection) {
            let aspect = self.aspect();
            self.camera.zoom_extents(b, aspect);
        }
    }

    // ----- history (undo / redo) and geometry changes -------------------------

    /// Whether there is a step to undo (the Undo icon).
    pub fn can_undo(&self) -> bool {
        self.mode == Mode::Edit && self.document.can_undo()
    }

    /// Whether there is a step to redo (Edit menu).
    pub fn can_redo(&self) -> bool {
        self.mode == Mode::Edit && self.document.can_redo()
    }

    /// Undoes the last step. Geometry never changes in Present mode.
    fn undo(&mut self) {
        if self.mode == Mode::Present {
            self.undo_look(true);
            return;
        }
        self.end_redefinition();
        self.message = Some(match self.document.undo() {
            Some(name) => format!("Undid {name}."),
            None => "Nothing to undo.".into(),
        });
        self.after_change();
    }

    /// Undo (or redo) in Present mode: only steps that change the look, so
    /// geometry never changes there.
    fn undo_look(&mut self, undo: bool) {
        let name = if undo {
            self.document.undo_name()
        } else {
            self.document.redo_name()
        };
        self.message = Some(match name {
            None => if undo {
                "Nothing to undo."
            } else {
                "Nothing to redo."
            }
            .into(),
            Some(n) if !LOOK_STEPS.contains(&n) => {
                format!("{n} changes geometry: press Tab and undo it in Edit mode.")
            }
            Some(_) => {
                let done = if undo {
                    self.document.undo()
                } else {
                    self.document.redo()
                };
                self.last_paint = None;
                self.last_value_edit = None;
                self.refresh_mesh();
                format!(
                    "{} {}.",
                    if undo { "Undid" } else { "Redid" },
                    done.unwrap_or_default()
                )
            }
        });
    }

    /// Whether Operation Inputs has something to show in the current mode.
    pub fn has_inputs(&self) -> bool {
        self.mode == Mode::Edit || self.tool.is_look() || self.by_type.is_some()
    }

    /// Whether Select by Type is open.
    pub fn selecting_by_type(&self) -> bool {
        self.by_type.is_some()
    }

    /// Select by Type's Operation Inputs: Type, Layer and Material, each
    /// "Any" until chosen. Every change selects what matches, within the
    /// selection it started with (or everything here when that was empty).
    fn by_type_inputs(&mut self, ui: &mut Ui) {
        let Some(mut choice) = self.by_type.clone() else {
            return;
        };
        ui.label("Select by Type");
        let layers: Vec<(Option<LayerId>, String)> = self
            .document
            .layer_ids()
            .filter_map(|id| {
                let l = self.document.layer(id)?;
                let key = (Some(id) != self.document.base_layer()).then_some(id);
                Some((key, l.name.clone()))
            })
            .collect();
        let materials: Vec<(MaterialId, String)> = self
            .document
            .material_ids()
            .filter_map(|id| Some((id, self.document.material(id)?.name.clone())))
            .collect();
        let name_of = |c: &Option<Option<String>>| match c {
            None => "Any".to_owned(),
            Some(None) => "None".to_owned(),
            Some(Some(n)) => n.clone(),
        };
        egui::Grid::new("by_type_inputs")
            .num_columns(2)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                ui.label("Type");
                egui::ComboBox::from_id_salt("by_type_kind")
                    .selected_text(BY_TYPE_KINDS[choice.kind])
                    .show_ui(ui, |ui| {
                        for (i, k) in BY_TYPE_KINDS.iter().enumerate() {
                            // Vertices are Edit mode's.
                            if i == BY_TYPE_VERTICES && self.mode == Mode::Present {
                                continue;
                            }
                            ui.selectable_value(&mut choice.kind, i, *k);
                        }
                    });
                ui.end_row();
                // Vertices have no layer or paint of their own.
                let filters = choice.kind != BY_TYPE_VERTICES;
                ui.label("Layer");
                let shown = choice.layer.map(|l| {
                    Some(
                        layers
                            .iter()
                            .find(|(k, _)| *k == l)
                            .map_or_else(String::new, |(_, n)| n.clone()),
                    )
                });
                ui.add_enabled_ui(filters, |ui| {
                    egui::ComboBox::from_id_salt("by_type_layer")
                        .selected_text(name_of(&shown))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut choice.layer, None, "Any");
                            for (k, n) in &layers {
                                ui.selectable_value(&mut choice.layer, Some(*k), n);
                            }
                        });
                });
                ui.end_row();
                ui.label("Material");
                let shown = choice.material.map(|m| {
                    m.map(|m| {
                        materials
                            .iter()
                            .find(|(k, _)| *k == m)
                            .map_or_else(String::new, |(_, n)| n.clone())
                    })
                });
                ui.add_enabled_ui(filters, |ui| {
                    egui::ComboBox::from_id_salt("by_type_material")
                        .selected_text(name_of(&shown))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut choice.material, None, "Any");
                            ui.selectable_value(&mut choice.material, Some(None), "None");
                            for (k, n) in &materials {
                                ui.selectable_value(&mut choice.material, Some(Some(*k)), n);
                            }
                        });
                });
                ui.end_row();
            });
        if Some(&choice) != self.by_type.as_ref() {
            let found = self.matching(&choice);
            self.by_type = Some(choice);
            self.select(found, SelectMode::Replace);
        }
    }

    /// What Select by Type's choices pick out.
    fn matching(&self, c: &ByType) -> Vec<Pickable> {
        let e = self.active();
        let pool = if c.scope.is_empty() {
            let mut all = e.all_pickables();
            all.extend(e.guide_ids().map(Pickable::Guide));
            all
        } else {
            c.scope.clone()
        };
        // Vertices: those of everything here, or of the selection it
        // started with. They have no layer or paint of their own.
        if c.kind == BY_TYPE_VERTICES {
            return smith_ops::modify::item_vertices(e, &pool)
                .into_iter()
                .chain(e.vertex_ids().filter(|_| c.scope.is_empty()))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(Pickable::Vertex)
                .collect();
        }
        pool.into_iter()
            .filter(|p| {
                let kind = match p {
                    // Any never takes vertices.
                    Pickable::Vertex(_) => return false,
                    Pickable::Edge(x) => match e.edge(*x) {
                        Some(edge) if edge.curve().is_some() => 3,
                        Some(_) => 2,
                        None => return false,
                    },
                    Pickable::Face(_) => 4,
                    Pickable::Instance(x) => {
                        let def = e
                            .instance(*x)
                            .and_then(|i| self.document.definition(i.definition));
                        match def.map(|d| d.kind) {
                            Some(DefinitionKind::Group) => 5,
                            Some(DefinitionKind::Component) => 6,
                            None => return false,
                        }
                    }
                    Pickable::Guide(_) => 7,
                };
                if c.kind != 0 && c.kind != kind {
                    return false;
                }
                let layer = match p {
                    Pickable::Edge(x) => e.edge(*x).map(|x| x.flags.layer),
                    Pickable::Face(x) => e.face(*x).map(|x| x.attributes.layer),
                    Pickable::Instance(x) => e.instance(*x).map(|x| x.layer),
                    // Guides are on no layer.
                    Pickable::Guide(_) | Pickable::Vertex(_) => None,
                };
                if let Some(want) = c.layer
                    && layer != Some(want)
                {
                    return false;
                }
                if let Some(want) = c.material {
                    let Pickable::Face(f) = p else {
                        return false;
                    };
                    let Some(face) = e.face(*f) else {
                        return false;
                    };
                    let (front, back) = (
                        face.attributes.front.map(|x| x.material),
                        face.attributes.back.map(|x| x.material),
                    );
                    let hit = match want {
                        None => front.is_none() && back.is_none(),
                        Some(m) => front == Some(m) || back == Some(m),
                    };
                    if !hit {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// The Paint tool's current material (`None`: unpaint).
    pub fn paint_material(&self) -> Option<MaterialId> {
        self.paint.material
    }

    /// Makes `m` the Paint tool's current material and picks up Paint.
    pub fn use_material(&mut self, m: Option<MaterialId>) {
        self.paint.material = m;
        if self.tool != WorkTool::Paint {
            self.set_tool(WorkTool::Paint);
        }
    }

    /// Adds `m` to the document (one undo step) and makes it current.
    pub fn add_material(&mut self, m: smith_model::Material) {
        match self.document.transaction("Material", |d| d.add_material(m)) {
            Ok(id) => self.use_material(Some(id)),
            Err(e) => self.message = Some(format!("Could not add the material: {e}.")),
        }
    }

    /// Replace with Current: every face painted with `old` takes the current
    /// material instead (one undo step).
    pub fn replace_with_current(&mut self, old: MaterialId) {
        let new = self.paint.material;
        match self
            .document
            .transaction("Material", |d| d.replace_material(old, new))
        {
            Ok(n) => {
                self.message = Some(format!("Replaced on {n} face sides."));
                self.refresh_mesh();
            }
            Err(e) => self.message = Some(format!("Could not replace: {e}.")),
        }
    }

    /// Changes material `id`; a run of changes to one material is one undo
    /// step.
    fn edit_material(&mut self, id: MaterialId, m: smith_model::Material) {
        self.edit_value(ValueEdit::Material(id), "Material", |d| {
            d.set_material(id, m)
        });
    }

    /// Runs `op` as the undo step `name`; when the step before it changed
    /// the same value (`key`) and nothing came between, that step is undone
    /// first, so a run of changes to one value is one undo step.
    fn edit_value(
        &mut self,
        key: ValueEdit,
        name: &str,
        op: impl FnOnce(&mut Document) -> Result<(), ModelError>,
    ) {
        if self.last_value_edit == Some((self.document.revision(), key)) {
            self.document.undo();
        }
        match self.document.transaction(name, op) {
            Ok(()) => {
                self.last_value_edit = Some((self.document.revision(), key));
                self.refresh_mesh();
            }
            Err(e) => self.message = Some(format!("Could not change it: {e}.")),
        }
    }

    /// The Paint tool's Operation Inputs: how it applies, then the current
    /// material's values. Changing Offset, Rotation, Projection or Axis
    /// right after painting paints again with them (one undo step).
    fn paint_inputs(&mut self, ui: &mut Ui) {
        use smith_model::{Axis, METAL_PRESETS, Projection};
        ui.label("Paint");
        let before = self.paint;
        egui::Grid::new("paint_inputs")
            .num_columns(2)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                ui.label("Apply To");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.paint.apply_to, ApplyTo::Face, "Face");
                    ui.radio_value(&mut self.paint.apply_to, ApplyTo::Wrap, "Wrap");
                });
                ui.end_row();
                if self.paint.apply_to == ApplyTo::Wrap {
                    ui.label("Projection");
                    ui.horizontal(|ui| {
                        for (p, name) in [
                            (Projection::Box, "Box"),
                            (Projection::Cylinder, "Cylinder"),
                            (Projection::Planar, "Planar"),
                        ] {
                            ui.radio_value(&mut self.paint.projection, p, name);
                        }
                    });
                    ui.end_row();
                    ui.label("Axis");
                    ui.add_enabled_ui(self.paint.projection != Projection::Box, |ui| {
                        ui.horizontal(|ui| {
                            for (a, name) in [(Axis::X, "X"), (Axis::Y, "Y"), (Axis::Z, "Z")] {
                                ui.radio_value(&mut self.paint.axis, a, name);
                            }
                        });
                    });
                    ui.end_row();
                }
                ui.label("Offset X");
                ui.add(
                    egui::DragValue::new(&mut self.paint.offset.x)
                        .speed(0.5)
                        .suffix(" mm"),
                );
                ui.end_row();
                ui.label("Offset Y");
                ui.add(
                    egui::DragValue::new(&mut self.paint.offset.y)
                        .speed(0.5)
                        .suffix(" mm"),
                );
                ui.end_row();
                ui.label("Rotation");
                ui.add(
                    egui::DragValue::new(&mut self.paint.rotation_deg)
                        .speed(1.0)
                        .suffix("°"),
                );
                ui.end_row();
            });
        if self.paint != before && self.paint.apply_to == before.apply_to {
            self.repaint_last();
        }
        ui.separator();
        let Some(id) = self.paint.material else {
            ui.label("Material: None (unpaints)");
            return;
        };
        let Some(mut m) = self.document.material(id).cloned() else {
            self.paint.material = None;
            return;
        };
        let was = m.clone();
        egui::Grid::new("material_inputs")
            .num_columns(2)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                ui.label("Material");
                ui.label(&m.name);
                ui.end_row();
                ui.label("Colour");
                // The picker works within 0–1; a measured metal above 1 is
                // kept until the colour is changed here.
                let mut rgb = m.color.map(|c| c.min(1.0));
                if ui.color_edit_button_rgb(&mut rgb).changed() {
                    m.color = rgb;
                }
                ui.end_row();
                ui.label("Metal");
                egui::ComboBox::from_id_salt("metal_preset")
                    .selected_text("Choose…")
                    .show_ui(ui, |ui| {
                        for (name, color) in METAL_PRESETS {
                            if ui.selectable_label(false, name).clicked() {
                                m.color = color;
                                m.metallic = true;
                            }
                        }
                    });
                ui.end_row();
                ui.label("Metallic");
                ui.checkbox(&mut m.metallic, "");
                ui.end_row();
                ui.label("Opacity");
                ui.add(egui::Slider::new(&mut m.opacity, 0.0..=1.0));
                ui.end_row();
                if let Some(t) = m.texture.as_mut() {
                    ui.label("Width");
                    ui.add(
                        egui::DragValue::new(&mut t.width_mm)
                            .range(0.01..=1.0e6)
                            .speed(0.5)
                            .suffix(" mm"),
                    );
                    ui.end_row();
                }
            });
        if m != was {
            self.edit_material(id, m);
        }
    }

    /// A Mark Edges or Line Style click at `pos`: marks or styles the edge
    /// there (one undo step).
    fn edge_click(&mut self, pos: Pos2) {
        let e = match self.pick_at(pos) {
            Some(Pickable::Edge(e)) => e,
            Some(Pickable::Instance(_)) => {
                self.message = Some("That edge is in a group: open the group first.".into());
                return;
            }
            _ => return,
        };
        let ctx = self.ctx;
        let result = match self.tool {
            WorkTool::MarkEdges => {
                let mark = self.mark;
                self.document.transaction("Mark Edges", |d| {
                    d.context_entities_mut(ctx)
                        .ok_or(ModelError::NoSuchEntity)?
                        .set_edge_mark(e, mark)
                })
            }
            WorkTool::LineStyle if !self.line.on_layer => {
                let style = self.line.style;
                self.document.transaction("Line Style", |d| {
                    d.context_entities_mut(ctx)
                        .ok_or(ModelError::NoSuchEntity)?
                        .set_edge_style(e, style)
                })
            }
            _ => return,
        };
        match result {
            Ok(()) => self.refresh_mesh(),
            Err(err) => self.message = Some(format!("Could not change the edge: {err}.")),
        }
    }

    /// Mark Edges' Operation Inputs: the mark a click gives, and the model's
    /// smoothing angle.
    fn mark_inputs(&mut self, ui: &mut Ui) {
        use smith_model::EdgeMark;
        ui.label("Mark Edges");
        let mut angle = self.document.look().smoothing_angle_deg;
        egui::Grid::new("mark_inputs")
            .num_columns(2)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                ui.label("Mark");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.mark, EdgeMark::Sharp, "Sharp");
                    ui.radio_value(&mut self.mark, EdgeMark::Smooth, "Smooth");
                    ui.radio_value(&mut self.mark, EdgeMark::Auto, "Auto");
                });
                ui.end_row();
                ui.label("Smoothing Angle");
                ui.add(
                    egui::DragValue::new(&mut angle)
                        .range(0.0..=180.0)
                        .speed(0.5)
                        .suffix("°"),
                );
                ui.end_row();
            });
        if angle != self.document.look().smoothing_angle_deg {
            self.edit_value(ValueEdit::Angle, "Smoothing Angle", |d| {
                d.set_smoothing_angle(angle)
            });
        }
    }

    /// Line Style's Operation Inputs: Apply To (Edges, or a layer), then the
    /// weight (None clears a style) and pattern. With a layer, changing them
    /// restyles the layer at once; picking the layer shows its style.
    fn line_inputs(&mut self, ui: &mut Ui) {
        use smith_model::{LinePattern, LineStyle, PenWeight};
        ui.label("Line Style");
        let before = self.line;
        let layers: Vec<(smith_model::LayerId, String)> = self
            .document
            .layer_ids()
            .filter_map(|id| self.document.layer(id).map(|l| (id, l.name.clone())))
            .collect();
        egui::Grid::new("line_inputs")
            .num_columns(2)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                ui.label("Apply To");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.line.on_layer, false, "Edges");
                    ui.radio_value(&mut self.line.on_layer, true, "Layer");
                });
                ui.end_row();
                if self.line.on_layer {
                    ui.label("Layer");
                    let shown = self
                        .line
                        .layer
                        .and_then(|id| layers.iter().find(|(l, _)| *l == id))
                        .map_or("Choose…", |(_, n)| n.as_str());
                    egui::ComboBox::from_id_salt("line_layer")
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for (id, name) in &layers {
                                ui.selectable_value(&mut self.line.layer, Some(*id), name);
                            }
                        });
                    ui.end_row();
                }
                ui.label("Weight");
                let weight = self
                    .line
                    .style
                    .map_or("None".to_owned(), |s| format!("{} mm", s.weight.mm()));
                egui::ComboBox::from_id_salt("line_weight")
                    .selected_text(weight)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(self.line.style.is_none(), "None")
                            .clicked()
                        {
                            self.line.style = None;
                        }
                        for w in PenWeight::ALL {
                            let on = self.line.style.is_some_and(|s| s.weight == w);
                            if ui.selectable_label(on, format!("{} mm", w.mm())).clicked() {
                                let pattern =
                                    self.line.style.map_or(LinePattern::Solid, |s| s.pattern);
                                self.line.style = Some(LineStyle { weight: w, pattern });
                            }
                        }
                    });
                ui.end_row();
                ui.label("Pattern");
                ui.add_enabled_ui(self.line.style.is_some(), |ui| {
                    let name = |p: LinePattern| match p {
                        LinePattern::Solid => "Solid",
                        LinePattern::Dashed => "Dashed",
                        LinePattern::DashDot => "Dash-Dot",
                        LinePattern::DashDoubleDot => "Dash-Double-Dot",
                    };
                    let current = self.line.style.map_or(LinePattern::Solid, |s| s.pattern);
                    egui::ComboBox::from_id_salt("line_pattern")
                        .selected_text(name(current))
                        .show_ui(ui, |ui| {
                            for p in LinePattern::ALL {
                                if ui.selectable_label(current == p, name(p)).clicked()
                                    && let Some(s) = self.line.style.as_mut()
                                {
                                    s.pattern = p;
                                }
                            }
                        });
                });
                ui.end_row();
            });
        if !self.line.on_layer {
            return;
        }
        let Some(id) = self.line.layer else { return };
        if self.line.layer != before.layer || !before.on_layer {
            // A layer just picked: show its style rather than change it.
            self.line.style = self.document.layer(id).and_then(|l| l.style);
        } else if self.line.style != before.style {
            let style = self.line.style;
            self.edit_value(ValueEdit::LayerStyle(id), "Line Style", |d| {
                let mut layer = d.layer(id).cloned().ok_or(ModelError::NoSuchEntity)?;
                layer.style = style;
                d.set_layer(id, layer)
            });
        }
    }

    /// A Paint click at `pos`: samples with `alt`, else paints the face
    /// clicked (Face) or the selection, piece or group clicked (Wrap).
    fn paint_click(&mut self, pos: Pos2, alt: bool) {
        let (o, d) = self.ray(self.ndc(pos), self.aspect());
        let hit = crate::paint_tool::deep_hit(&self.document, self.ctx, DMat4::IDENTITY, o, d);
        if alt {
            if let Some(h) = hit {
                self.paint.material = crate::paint_tool::sample(&self.document, &h);
                self.message = Some(match self.paint.material {
                    Some(m) => format!(
                        "Sampled {}.",
                        self.document.material(m).map_or("", |x| x.name.as_str())
                    ),
                    None => "Sampled None.".into(),
                });
            }
            return;
        }
        let result = match self.paint.apply_to {
            ApplyTo::Face => {
                let Some(h) = hit else { return };
                crate::paint_tool::paint_face(&mut self.document, &h, &self.paint)
                    .map(|()| LastPaint::Face(h))
            }
            ApplyTo::Wrap => {
                let items: Vec<Pickable> = self.selection.ordered();
                let targets = crate::paint_tool::wrap_targets(
                    &self.document,
                    self.ctx,
                    DMat4::IDENTITY,
                    &items,
                    hit.as_ref(),
                );
                if targets.is_empty() {
                    return;
                }
                let a = self.axes_ctx;
                crate::paint_tool::paint_wrap(
                    &mut self.document,
                    &targets,
                    [a.x, a.y, a.z],
                    &self.paint,
                )
                .map(|()| LastPaint::Wrap(targets))
            }
        };
        match result {
            Ok(last) => {
                self.last_paint = Some((self.document.revision(), last));
                self.refresh_mesh();
            }
            Err(e) => self.message = Some(format!("Could not paint: {e}.")),
        }
    }

    /// Paints again what the last paint painted, with the current values
    /// (Offset and Rotation changed right after painting): one undo step.
    fn repaint_last(&mut self) {
        let Some((rev, last)) = self.last_paint.clone() else {
            return;
        };
        if rev != self.document.revision() {
            self.last_paint = None;
            return;
        }
        self.document.undo();
        let result = match &last {
            LastPaint::Face(h) => crate::paint_tool::paint_face(&mut self.document, h, &self.paint),
            LastPaint::Wrap(t) => {
                let a = self.axes_ctx;
                crate::paint_tool::paint_wrap(&mut self.document, t, [a.x, a.y, a.z], &self.paint)
            }
        };
        if result.is_ok() {
            self.last_paint = Some((self.document.revision(), last));
        } else {
            self.last_paint = None;
        }
        self.refresh_mesh();
    }

    /// Redoes the last undone step.
    fn redo(&mut self) {
        if self.mode == Mode::Present {
            self.undo_look(false);
            return;
        }
        self.end_redefinition();
        self.message = Some(match self.document.redo() {
            Some(name) => format!("Redid {name}."),
            None => "Nothing to redo.".into(),
        });
        self.after_change();
    }

    /// The history changed outside the draw tool: its last shape can no longer
    /// be redefined, and any shape in progress is abandoned.
    fn end_redefinition(&mut self) {
        self.confirmed = None;
        if self.tool == WorkTool::EditCurve {
            self.tool = WorkTool::Select;
            self.draw_tool = None;
        }
        self.redefinable = false;
        if let Some(t) = self.draw_tool.as_mut() {
            t.cancel();
            t.forget();
        }
    }

    /// Folds a committed move, rotate or scale into the Repeat Transform
    /// chain: a copy starts a new chain; a change in place after a copy is
    /// taken to adjust that copy, so it joins the step.
    fn note_transform(&mut self, op: &DrawOp) {
        use smith_ops::modify::Transform;
        let (items, m, copy) = match op {
            DrawOp::Move {
                items, delta, copy, ..
            } => (items, DMat4::from_translation(*delta), *copy),
            DrawOp::Transform {
                items,
                transform,
                copy,
                ..
            } => {
                if matches!(transform, Transform::Matrix { .. }) {
                    return;
                }
                (items, transform.matrix(1.0), *copy)
            }
            _ => return,
        };
        if copy {
            self.chain = Some(Chain {
                items: items.clone(),
                step: m,
                count: 1,
            });
        } else if let Some(chain) = self.chain.as_mut()
            && chain.count == 1
        {
            chain.step = m * chain.step;
        }
    }

    /// Repeat Transform: one more copy, one more step along the chain.
    fn repeat_transform(&mut self) {
        let ctx = self.ctx;
        let Some(mut chain) = self.chain.clone() else {
            self.message = Some("Copy with Move, Rotate or Scale first.".into());
            return;
        };
        chain.count += 1;
        let op = DrawOp::Transform {
            items: chain.items.clone(),
            transform: smith_ops::modify::Transform::Matrix {
                m: smith_ops::modify::Transform::Matrix { m: chain.step }
                    .matrix(f64::from(chain.count)),
            },
            copy: true,
            copies: 1,
            divide: false,
        };
        match self
            .document
            .transaction("Repeat Transform", |d| run_draw(d, ctx, &op))
        {
            Ok(_) => {
                self.chain = Some(chain);
                self.message = None;
            }
            Err(e) => self.message = Some(format!("Couldn't repeat: {e}.")),
        }
        self.after_change();
    }

    /// Copies the selection to the clipboard; `false` when nothing is
    /// selected.
    fn copy_selection(&mut self) -> bool {
        let items: Vec<Pickable> = self.selection.ordered();
        match smith_ops::organize::copy_items(&self.document, self.ctx, &items) {
            Some(clip) => {
                self.clipboard = Some((clip, self.local));
                true
            }
            None => {
                self.message = Some("Select something to copy.".into());
                false
            }
        }
    }

    /// Pastes the clipboard where it was copied from in the model (into the
    /// group being edited, if any), selects it, and takes the Move tool so
    /// the next two clicks place it. One undo step for the paste.
    fn paste(&mut self) {
        let Some((clip, from)) = self.clipboard.as_ref() else {
            self.message = Some("Nothing to paste.".into());
            return;
        };
        let ctx = self.ctx;
        let m = self.local.inverse() * *from;
        let result = self.document.transaction("Paste", |d| {
            smith_ops::organize::paste_clip(d, ctx, clip, &m)
        });
        self.after_change();
        match result {
            Ok(made) if !made.is_empty() => {
                self.select(made, SelectMode::Replace);
                self.set_tool(WorkTool::Draw(DrawKind::Move));
                self.message = Some("Pasted in place: click two points to move it.".into());
            }
            Ok(_) => self.message = Some("Nothing could be pasted here.".into()),
            Err(e) => self.message = Some(format!("Couldn't paste: {e}.")),
        }
    }

    /// Makes a group or component of the selected faces and edges and selects
    /// it. One undo step.
    fn make_group(&mut self, kind: DefinitionKind) {
        let items: Vec<Pickable> = self
            .selection
            .iter()
            .filter(|p| !matches!(p, Pickable::Instance(_)))
            .collect();
        let name = match kind {
            DefinitionKind::Group => "Group".to_string(),
            DefinitionKind::Component => {
                let n = self
                    .document
                    .definition_ids()
                    .filter(|d| {
                        self.document
                            .definition(*d)
                            .is_some_and(|x| x.kind == DefinitionKind::Component)
                    })
                    .count();
                format!("Component#{}", n + 1)
            }
        };
        let label = match kind {
            DefinitionKind::Group => "Make Group",
            DefinitionKind::Component => "Make Component",
        };
        let ctx = self.ctx;
        let result = self.document.transaction(label, |d| {
            smith_ops::organize::make_group(d, ctx, &items, kind, &name)
        });
        self.after_change();
        match result {
            Ok(id) => self.select(vec![Pickable::Instance(id)], SelectMode::Replace),
            Err(e) => self.message = Some(format!("Couldn't {}: {e}.", label.to_lowercase())),
        }
    }

    /// The connected pieces of loose geometry in the context being edited.
    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// Whether every part of piece `i` is selected.
    pub fn piece_selected(&self, i: usize) -> bool {
        self.pieces
            .get(i)
            .is_some_and(|p| p.items().all(|x| self.selection.contains(x)))
    }

    /// Selects piece `i` (replacing the selection).
    pub fn select_piece(&mut self, i: usize) {
        if let Some(p) = self.pieces.get(i) {
            let items: Vec<Pickable> = p.items().collect();
            self.select(items, SelectMode::Replace);
        }
    }

    /// Hides or shows piece `i`. One undo step.
    pub fn set_piece_hidden(&mut self, i: usize, hidden: bool) {
        if let Some(p) = self.pieces.get(i) {
            let items: Vec<Pickable> = p.items().collect();
            self.set_hidden(items, hidden, if hidden { "Hide" } else { "Unhide" });
        }
    }

    /// The selection's shape, as the drawing tools name it, or a single
    /// group's or component's name.
    pub fn selection_shape(&self) -> Option<String> {
        let a = self.active();
        let items: Vec<Pickable> = self.selection.ordered();
        if let [Pickable::Instance(i)] = items[..] {
            let inst = a.instance(i)?;
            return Some(self.instance_name(inst));
        }
        let edges: Vec<smith_model::EdgeId> = self.selection.edges().collect();
        let faces: Vec<smith_model::FaceId> = self.selection.faces().collect();
        shape_name(a, &edges, &faces).map(str::to_string)
    }

    /// What a group or component is called: its own name, else its
    /// definition's; an unnamed plain group whose geometry is a closed
    /// solid is a "Solid".
    fn instance_name(&self, inst: &smith_model::Instance) -> String {
        if !inst.name.is_empty() {
            return inst.name.clone();
        }
        let Some(def) = self.document.definition(inst.definition) else {
            return String::new();
        };
        let plain = def.kind == DefinitionKind::Group && def.name == "Group";
        if plain
            && self
                .document
                .context(Some(inst.definition))
                .is_some_and(|e| smith_ops::modify::solid_volume(e).is_some())
        {
            return "Solid".into();
        }
        def.name.clone()
    }

    /// The volume of a group or component that is a closed solid, mm³, as
    /// placed (its own geometry; nested groups are not counted).
    pub fn instance_volume(&self, id: InstanceId) -> Option<f64> {
        let inst = self.active().instance(id)?;
        let e = self.document.context(Some(inst.definition))?;
        let scale = DMat3::from_mat4(inst.transform).determinant().abs();
        smith_ops::modify::solid_volume(e).map(|v| v * scale)
    }

    /// The selection's size along the drawing axes' X, Y and Z.
    pub fn selection_size(&self) -> Option<DVec3> {
        let items: Vec<Pickable> = self.selection.ordered();
        let a = self.axes_ctx;
        let frame = smith_geom::DMat3::from_cols(a.x, a.y, a.z);
        smith_ops::modify::items_bounds_in(self.active(), &items, &self.instance_boxes, frame)
            .map(|(lo, hi)| hi - lo)
    }

    /// Shows what is selected again.
    fn unhide_selection(&mut self) {
        let items: Vec<Pickable> = self.selection.ordered();
        self.set_hidden(items, false, "Unhide");
    }

    /// Hides everything in the context being edited except the selection.
    fn hide_others(&mut self) {
        let a = self.active();
        let keep: std::collections::BTreeSet<Pickable> = self.selection.iter().collect();
        let others: Vec<Pickable> = a
            .all_pickables()
            .into_iter()
            .filter(|p| !keep.contains(p))
            .collect();
        if others.is_empty() {
            return;
        }
        self.last_hidden.clone_from(&others);
        self.set_hidden(others, true, "Hide Others");
    }

    /// Whether anything selected is hidden (shown again by Unhide).
    fn selection_has_hidden(&self) -> bool {
        let a = self.active();
        self.selection.iter().any(|p| match p {
            Pickable::Edge(e) => a.edge(e).is_some_and(|x| x.flags.hidden),
            Pickable::Face(f) => a.face(f).is_some_and(|x| x.attributes.hidden),
            Pickable::Instance(i) => a.instance(i).is_some_and(|x| x.hidden),
            Pickable::Guide(_) | Pickable::Vertex(_) => false,
        })
    }

    /// The values of the one curve selected, when it is a circle, arc or
    /// polygon: for the Statistics overlay (changed only with Edit Curve).
    pub fn curve_values(&self) -> Option<CurveValues> {
        let id = self.selected_curve()?;
        let rec = self.active().curve(id)?;
        let (kind, radius) = match rec.curve() {
            smith_geom::curve::Curve::Circle(c) => (CurveKind::Circle, c.radius()),
            smith_geom::curve::Curve::Arc(a) => (CurveKind::Arc, a.radius()),
            smith_geom::curve::Curve::Polygon(p) => (CurveKind::Polygon, p.radius()),
            _ => return None,
        };
        let segments = match rec.curve() {
            smith_geom::curve::Curve::Polygon(p) => p.sides(),
            _ => rec.segments(),
        };
        Some(CurveValues {
            kind,
            segments,
            radius,
            locked: self.active().curve_is_locked(id),
        })
    }

    /// The area of every face inside group or component `id` of the active
    /// context (nested ones too), measured as placed, so scaling counts.
    pub fn instance_area(&self, id: InstanceId) -> f64 {
        fn area(doc: &Document, def: DefinitionId, m: &DMat4, depth: u32) -> f64 {
            let Some(e) = doc.context(Some(def)) else {
                return 0.0;
            };
            let own: f64 = e
                .face_ids()
                .flat_map(|f| e.face_triangles(f))
                .map(|[a, b, c]| {
                    let (a, b, c) = (
                        m.transform_point3(a),
                        m.transform_point3(b),
                        m.transform_point3(c),
                    );
                    (b - a).cross(c - a).length() * 0.5
                })
                .sum();
            let nested: f64 = if depth < 64 {
                e.instance_ids()
                    .filter_map(|i| e.instance(i))
                    .map(|i| area(doc, i.definition, &(*m * i.transform), depth + 1))
                    .sum()
            } else {
                0.0
            };
            own + nested
        }
        self.active()
            .instance(id)
            .map_or(0.0, |i| area(&self.document, i.definition, &i.transform, 0))
    }

    /// The total area of the faces on `layer` (`None`: Base) in the active
    /// context.
    pub fn layer_area(&self, layer: Option<smith_model::LayerId>) -> f64 {
        let e = self.active();
        e.face_ids()
            .filter(|f| e.face(*f).is_some_and(|x| x.attributes.layer == layer))
            .filter_map(|f| e.face_area(f))
            .sum()
    }

    /// The Outliner's rows: every group and component in the model, depth
    /// first, each with the path of instances that leads to it from the root.
    pub fn outliner(&self) -> Vec<OutlinerRow> {
        let mut rows = Vec::new();
        self.outline(None, &mut Vec::new(), &mut rows);
        rows
    }

    fn outline(
        &self,
        ctx: Option<DefinitionId>,
        path: &mut Vec<InstanceId>,
        rows: &mut Vec<OutlinerRow>,
    ) {
        let Some(e) = self.document.context(ctx) else {
            return;
        };
        if path.len() > 64 {
            return;
        }
        for id in e.instance_ids() {
            let Some(inst) = e.instance(id) else {
                continue;
            };
            let def = self.document.definition(inst.definition);
            let kind = def.map_or(DefinitionKind::Group, |d| d.kind);
            let name = self.instance_name(inst);
            path.push(id);
            rows.push(OutlinerRow {
                path: path.clone(),
                name,
                component: kind == DefinitionKind::Component,
                locked: inst.locked,
                hidden: inst.hidden,
                selected: path.len() == self.editing.len() + 1
                    && path[..self.editing.len()] == self.editing[..]
                    && self.selection.contains(Pickable::Instance(id)),
                open: self.editing.starts_with(path),
            });
            self.outline(Some(inst.definition), path, rows);
            path.pop();
        }
    }

    /// Selects the group or component at `path`, opening the groups around
    /// it for editing (and closing any others).
    pub fn outliner_select(&mut self, path: &[InstanceId]) {
        let Some((last, parents)) = path.split_last() else {
            return;
        };
        if self.editing != parents {
            while !self.editing.is_empty() {
                self.leave_context();
            }
            for p in parents {
                self.enter_instance(*p);
            }
            if self.editing != parents {
                return;
            }
        }
        self.select(vec![Pickable::Instance(*last)], SelectMode::Replace);
    }

    /// The context holding the last instance of `path`, and that instance.
    fn path_target(&self, path: &[InstanceId]) -> Option<(Option<DefinitionId>, InstanceId)> {
        let (last, parents) = path.split_last()?;
        let mut ctx = None;
        for p in parents {
            ctx = Some(self.document.context(ctx)?.instance(*p)?.definition);
        }
        Some((ctx, *last))
    }

    /// Renames, locks or hides the group or component at `path`. One undo step.
    pub fn outliner_change(&mut self, path: &[InstanceId], change: OutlinerChange) {
        let Some((ctx, id)) = self.path_target(path) else {
            return;
        };
        let name = match &change {
            OutlinerChange::Rename(_) => "Rename",
            OutlinerChange::Lock(true) => "Lock",
            OutlinerChange::Lock(false) => "Unlock",
            OutlinerChange::Hide(true) => "Hide",
            OutlinerChange::Hide(false) => "Unhide",
        };
        self.layer_op(name, |d| {
            let e = active_mut(d, ctx)?;
            match change {
                OutlinerChange::Rename(n) => e.set_instance_name(id, &n),
                OutlinerChange::Lock(on) => e.set_instance_locked(id, on),
                OutlinerChange::Hide(on) => e.set_instance_hidden(id, on),
            }
        });
    }

    /// The geometry being edited (the open group or component, or the root),
    /// for the panels.
    pub fn active_entities(&self) -> &Entities {
        self.active()
    }

    /// Runs a change to the document's layers (or to what is on them) as an
    /// undo step named `name`, keeping the selection, since no entity is
    /// added or removed.
    pub fn layer_op(
        &mut self,
        name: &str,
        op: impl FnOnce(&mut Document) -> Result<(), ModelError>,
    ) {
        if let Err(e) = self.document.transaction(name, op) {
            self.message = Some(format!("Couldn't {}: {e}.", name.to_lowercase()));
        }
        self.resolve_context();
        self.rebuild_snap();
        self.refresh_mesh();
    }

    /// Whether the sidebar key was pressed since the last call.
    pub fn take_sidebar_toggle(&mut self) -> bool {
        std::mem::take(&mut self.sidebar_toggle)
    }

    /// Makes `layer` (`None`: Base) the one new geometry goes on; a locked
    /// layer cannot be.
    pub fn set_active_layer(&mut self, layer: Option<smith_model::LayerId>) {
        if self.document.layer_locked(layer) {
            self.message = Some(LOCKED_NOT_ACTIVE.into());
            return;
        }
        self.document.set_active_layer(layer);
    }

    /// Adds a layer, named by its number, just above the active one, and
    /// makes it the active one. One undo step.
    pub fn add_layer(&mut self) {
        let above = self
            .document
            .active_layer()
            .and_then(|l| self.document.layer_position(l))
            .map_or(0, |p| p + 1);
        match self.document.transaction("Add Layer", |d| {
            let id = d.add_numbered_layer([90, 140, 200]);
            d.reorder_layer(id, above)?;
            Ok(id)
        }) {
            Ok(id) => self.document.set_active_layer(Some(id)),
            Err(e) => self.message = Some(format!("Couldn't add a layer: {e}.")),
        }
        self.rebuild_snap();
        self.refresh_mesh();
    }

    /// Selects what is on layer `layer` (`None`: Base) in the open context:
    /// the select mode's items (its faces, its edges, or the vertices its
    /// edges use) and its groups, whole. A hidden layer is shown first (one
    /// undo step); a locked one gives nothing.
    /// Scale Layer: selects everything on `layer` (`None`: Base) in the open
    /// context, edges, faces and groups whatever the select mode, and starts
    /// Scale about their centre, Operation Inputs showing their X, Y and Z
    /// sizes. Loose geometry stretches as Scale stretches it.
    pub fn scale_layer(&mut self, layer: Option<smith_model::LayerId>) {
        if self.document.layer_locked(layer) {
            self.message = Some(smith_model::UNLOCK_LAYER.into());
            return;
        }
        let (edges, faces, groups) = self.document.layer_contents(self.ctx, layer);
        let items: Vec<Pickable> = edges
            .into_iter()
            .map(Pickable::Edge)
            .chain(faces.into_iter().map(Pickable::Face))
            .chain(groups.into_iter().map(Pickable::Instance))
            .collect();
        if items.is_empty() {
            self.message = Some("Nothing on This Layer Here".into());
            return;
        }
        if !self.document.layer_visible(layer)
            && let Some(id) = layer.or_else(|| self.document.base_layer())
            && let Some(mut l) = self.document.layer(id).cloned()
        {
            l.visible = true;
            self.layer_op("Change Layer", |d| d.set_layer(id, l));
        }
        self.selection.clear();
        self.select(items, SelectMode::Replace);
        self.set_tool(WorkTool::Draw(DrawKind::Scale));
        // Around Centre: from the contents' centre; it also measures them,
        // so their sizes show at once.
        self.apply_field(SCALE_AROUND_CENTRE, 1.0);
    }

    pub fn select_layer_contents(&mut self, layer: Option<smith_model::LayerId>) {
        if self.document.layer_locked(layer) {
            self.message = Some(smith_model::UNLOCK_LAYER.into());
            return;
        }
        if !self.document.layer_visible(layer)
            && let Some(id) = layer.or_else(|| self.document.base_layer())
            && let Some(mut l) = self.document.layer(id).cloned()
        {
            l.visible = true;
            self.layer_op("Change Layer", |d| d.set_layer(id, l));
        }
        let (edges, faces, groups) = self.document.layer_contents(self.ctx, layer);
        let (mut picked, what): (Vec<Pickable>, (&str, &str)) = match self.select_kind {
            SelectKind::Faces => (
                faces.into_iter().map(Pickable::Face).collect(),
                ("Face", "Faces"),
            ),
            SelectKind::Edges => (
                edges.into_iter().map(Pickable::Edge).collect(),
                ("Edge", "Edges"),
            ),
            SelectKind::Vertices => {
                let e = self.active();
                let mut seen = std::collections::BTreeSet::new();
                let vertices: Vec<Pickable> = edges
                    .iter()
                    .filter_map(|x| e.edge(*x))
                    .flat_map(|x| x.vertices())
                    .filter(|v| seen.insert(*v))
                    .map(Pickable::Vertex)
                    .collect();
                (vertices, ("Vertex", "Vertices"))
            }
        };
        let items = picked.len();
        let wholes = groups.len();
        picked.extend(groups.into_iter().map(Pickable::Instance));
        self.select(picked, SelectMode::Replace);
        self.message = Some(if items == 0 && wholes == 0 {
            "Nothing on This Layer".into()
        } else {
            let mut parts = Vec::new();
            if items > 0 {
                parts.push(counted(items, what.0, what.1));
            }
            if wholes > 0 {
                parts.push(counted(wholes, "Group", "Groups"));
            }
            format!("Selected {}", parts.join(", "))
        });
    }

    /// Duplicates layer `id` (see `Document::duplicate_layer`) and makes
    /// the copy active. One undo step.
    pub fn duplicate_layer(&mut self, id: smith_model::LayerId) {
        match self
            .document
            .transaction("Duplicate Layer", |d| d.duplicate_layer(id))
        {
            Ok((copy, loose)) => {
                self.document.set_active_layer(Some(copy));
                self.message =
                    (loose > 0).then(|| "Loose Geometry Not Copied: Group It First".into());
            }
            Err(e) => self.message = Some(format!("Couldn't duplicate the layer: {e}.")),
        }
        self.after_change();
    }

    /// Remove: layer `id` and everything on it are deleted, after the
    /// Remove question when anything is on it. One undo step.
    pub fn remove_layer(&mut self, id: smith_model::LayerId) {
        if self.document.layer_locked(Some(id)) {
            self.message = Some(smith_model::UNLOCK_LAYER.into());
            return;
        }
        if self.document.layer_content_count(id) == (0, 0, 0) {
            self.layer_op("Remove Layer", |d| d.remove_layer(id));
            self.after_change();
        } else {
            self.layer_offer = Some(id);
        }
    }

    /// Anchor to Base: what is on layer `id` moves to Base and the layer is
    /// deleted. One undo step.
    pub fn anchor_layer(&mut self, id: smith_model::LayerId) {
        self.layer_op("Anchor to Base", |d| d.anchor_layer(id));
        self.after_change();
    }

    /// Merge Down: what is on layer `id` moves to the layer below and `id`
    /// is deleted. One undo step.
    pub fn merge_layer_down(&mut self, id: smith_model::LayerId) {
        self.layer_op("Merge Down", |d| d.merge_layer_down(id));
        self.after_change();
    }

    /// Merge Visible: every shown layer into the lowest shown one. One undo
    /// step.
    pub fn merge_visible_layers(&mut self) {
        self.layer_op("Merge Visible", |d| d.merge_visible_layers().map(|_| ()));
        self.after_change();
    }

    /// Flatten: everything onto Base, every other layer deleted. One undo
    /// step.
    pub fn flatten_layers(&mut self) {
        self.layer_op("Flatten", |d| d.flatten_layers().map(|_| ()));
        self.after_change();
    }

    /// Moves layer `id` to `position` in the stack above Base (0 just above
    /// it). One undo step; nothing when it is already there.
    pub fn reorder_layer(&mut self, id: smith_model::LayerId, position: usize) {
        if self.document.layer_position(id) == Some(position) {
            return;
        }
        self.layer_op("Move Layer", |d| d.reorder_layer(id, position));
    }

    /// Puts the selection on `layer` (`None`: Base). One undo step.
    pub fn assign_layer(&mut self, layer: Option<smith_model::LayerId>) {
        let items: Vec<Pickable> = self.selection.ordered();
        if items.is_empty() {
            return;
        }
        let ctx = self.ctx;
        self.layer_op("Set Layer", |d| {
            active_mut(d, ctx)?.set_layer(&items, layer)
        });
    }

    /// Runs one change to groups or components as an undo step named
    /// `name`, reporting a failure.
    fn instance_op(
        &mut self,
        name: &str,
        op: impl FnOnce(&mut Document) -> Result<(), ModelError>,
    ) {
        if let Err(e) = self.document.transaction(name, op) {
            self.message = Some(format!("Couldn't {}: {e}.", name.to_lowercase()));
        }
        self.after_change();
    }

    /// Explodes a group or component back into the model. One undo step.
    fn explode(&mut self, id: InstanceId) {
        let ctx = self.ctx;
        let result = self
            .document
            .transaction("Explode", |d| smith_ops::organize::explode(d, ctx, id));
        if let Err(e) = result {
            self.message = Some(format!("Couldn't explode: {e}."));
        }
        self.after_change();
    }

    /// Erases the selection (Delete, Edit → Delete, context menu Erase):
    /// selected faces go, their edges stay; selected edges are erased with
    /// healing, as the Eraser does. One undo step.
    fn erase_selection(&mut self) {
        let ctx = self.ctx;
        let vertices: Vec<_> = self.selection.vertices().collect();
        let faces: Vec<_> = self.selection.faces().collect();
        let edges: Vec<_> = self.selection.edges().collect();
        let all: Vec<_> = self.selection.instances().collect();
        let instances: Vec<_> = all
            .iter()
            .copied()
            .filter(|i| self.active().instance(*i).is_some_and(|x| !x.locked))
            .collect();
        if instances.len() < all.len() {
            self.message = Some("Locked groups and components are not erased.".into());
        }
        if vertices.is_empty() && faces.is_empty() && edges.is_empty() && instances.is_empty() {
            return;
        }
        let result = self.document.transaction("Erase", |d| {
            let root = active_mut(d, ctx)?;
            for i in &instances {
                root.erase_instance(*i)?;
            }
            // A vertex goes with its edges and their faces; nothing heals.
            for v in &vertices {
                if root.vertex(*v).is_some() {
                    root.erase_vertex(*v)?;
                }
            }
            for f in &faces {
                if root.face(*f).is_some() {
                    root.erase_face(*f)?;
                }
            }
            smith_ops::modify::erase(root, &edges, false)
        });
        if let Err(e) = result {
            self.message = Some(format!("Couldn't erase: {e}."));
        }
        self.after_change();
    }

    /// Fill on the selection: selected edges' flat loops, else the selected
    /// vertices in pick order. One undo step; off one plane, the Auto
    /// Triangulate question.
    fn fill_selection(&mut self) {
        let edges: Vec<_> = self.selection.edges().collect();
        let vertices: Vec<_> = self.selection.vertices().collect();
        if edges.is_empty() && vertices.len() < 2 {
            self.message = Some("Select Vertices or Edges to Fill".into());
            return;
        }
        let op = DrawOp::Fill { vertices, edges };
        let ctx = self.ctx;
        let result = self
            .document
            .transaction(op.name(), |d| run_draw(d, ctx, &op));
        match result {
            Ok(_) => self.message = None,
            Err(smith_model::ModelError::Refused(why)) => self.message = Some(why),
            Err(e) => {
                self.message = Some(format!("Couldn't fill: {e}."));
                self.offer_auto_tri(&e, op, false);
            }
        }
        self.after_change();
    }

    /// Aligns the selection's end points to their average along a drawing
    /// axis (0 red, 1 green, 2 blue). One undo step.
    fn align_selection(&mut self, axis: usize) {
        let items: Vec<Pickable> = self.selection.ordered();
        let root = self.active();
        let dir = [self.axes.x, self.axes.y, self.axes.z][axis.min(2)];
        let values: Vec<f64> = smith_ops::modify::item_edges(root, &items)
            .into_iter()
            .filter_map(|e| root.edge_points(e))
            .flat_map(|(a, b)| [a.dot(dir), b.dot(dir)])
            .collect();
        if values.is_empty() {
            self.message = Some("Select Edges or Faces: Open a Group to Work Inside It".into());
            return;
        }
        let value = values.iter().sum::<f64>() / values.len() as f64;
        self.run_now(&DrawOp::Align {
            items,
            axis: dir,
            value,
        });
    }

    /// Runs `op` at once as one undo step, reporting a failure; one that
    /// would bend faces is offered Auto-Tri.
    fn run_now(&mut self, op: &DrawOp) {
        let ctx = self.ctx;
        if let Err(e) = self
            .document
            .transaction(op.name(), |d| run_draw(d, ctx, op))
        {
            self.message = Some(format!("Couldn't {}: {e}.", op.name().to_lowercase()));
            self.note_bent(&e);
            self.offer_auto_tri(&e, op.clone(), false);
        }
        self.after_change();
    }

    /// Flips the selection in place along a drawing axis (0 red, 1 green,
    /// 2 blue), about the centre of its bounding box. One undo step.
    fn flip_selection(&mut self, axis: usize) {
        let ctx = self.ctx;
        let items: Vec<Pickable> = self.selection.ordered();
        let Some((lo, hi)) =
            smith_ops::modify::items_bounds(self.active(), &items, &self.instance_boxes)
        else {
            return;
        };
        let normal = [self.axes.x, self.axes.y, self.axes.z][axis.min(2)];
        let point = if self.pivot_active {
            self.pivot
        } else {
            (lo + hi) * 0.5
        };
        let op = DrawOp::Transform {
            items,
            transform: smith_ops::modify::Transform::Mirror { point, normal },
            copy: false,
            copies: 1,
            divide: false,
        };
        let before = self.active().clone();
        let result = self
            .document
            .transaction("Flip Along", |d| run_draw(d, ctx, &op));
        let name = ["X", "Y", "Z"][axis.min(2)];
        if let Err(e) = &result {
            self.note_bent(e);
        }
        self.message = Some(match &result {
            Err(e) => format!("Couldn't flip: {e}."),
            Ok(_) if *self.active() == before => {
                format!("Flipped along {name}: the shape is symmetric, so it looks the same.")
            }
            Ok(_) => format!("Flipped along {name}."),
        });
        // Bending faces: the Auto-Tri message and offer.
        if let Err(e) = result {
            self.offer_auto_tri(&e, op, false);
        }
        self.after_change();
    }

    /// Hides the selected edges (context menu Hide). One undo step.
    fn hide_selection(&mut self) {
        let items: Vec<Pickable> = self.selection.ordered();
        if items.is_empty() {
            self.message = Some("Select something to hide.".into());
            return;
        }
        self.last_hidden.clone_from(&items);
        self.set_hidden(items, true, "Hide");
    }

    /// Hides or shows the edges and faces among `items` as one undo step
    /// named `name`.
    fn set_hidden(&mut self, items: Vec<Pickable>, hidden: bool, name: &str) {
        let ctx = self.ctx;
        if items.is_empty() {
            return;
        }
        let result = self.document.transaction(name, |d| {
            let root = active_mut(d, ctx)?;
            for p in &items {
                match *p {
                    Pickable::Edge(e) if root.edge(e).is_some() => {
                        root.set_edge_hidden(e, hidden)?
                    }
                    Pickable::Face(f) if root.face(f).is_some() => {
                        root.set_face_hidden(f, hidden)?
                    }
                    _ => {}
                }
            }
            Ok(())
        });
        if let Err(e) = result {
            self.message = Some(format!("Couldn't {}: {e}.", name.to_lowercase()));
        }
        self.after_change();
    }

    /// Refreshes everything derived from the geometry. The selection is cleared,
    /// since splitting and undo can remove the entities it referred to.
    fn after_change(&mut self) {
        self.selection.clear();
        self.resolve_context();
        self.follow_pivot();
        self.rebuild_snap();
        self.shown_bounds = self.document.model_bounds();
        self.refresh_mesh();
        self.count_reversed();
    }

    /// Rebuilds what picking and inference look at: the active context, with
    /// its groups' geometry laid in and what cannot be seen left out, when
    /// either applies (see `SnapView`).
    fn rebuild_snap(&mut self) {
        self.pieces = loose_pieces(self.active());
        self.snap = if !self.editing.is_empty() {
            // Inside a group: the rest of the model is there to snap to.
            self.document.snap_view_in(&self.editing)
        } else if self.document.needs_snap_view(self.ctx) {
            self.document.snap_view(self.ctx)
        } else {
            None
        };
        let seen = ctx_snap(&self.document, self.ctx, self.snap.as_ref());
        self.bvh = FaceBvh::build(seen);
        self.pick_index = smith_infer::select::PickIndex {
            edges: smith_infer::bvh::SegmentBvh::edges(seen),
            vertices: smith_infer::bvh::SegmentBvh::vertices(self.active()),
        };
        self.instance_boxes = self.document.instance_boxes(self.ctx);
        self.instance_lines = match self.snap.as_ref() {
            Some(s) => self
                .active()
                .instance_ids()
                .map(|i| (i, s.owned_lines(i)))
                .collect(),
            None => Default::default(),
        };
    }

    /// What the cursor sees for `p` from the snapping view: the group it
    /// belongs to, or `p` itself.
    /// `None` for geometry outside the group being edited, which can be
    /// snapped to but not picked.
    fn owner_of(&self, p: Pickable) -> Option<Pickable> {
        match self.snap.as_ref() {
            Some(s) if s.is_outside(p) => None,
            Some(s) => Some(s.owner(p).map_or(p, Pickable::Instance)),
            None => Some(p),
        }
    }

    /// `op`, with Intersect Faces told which groups are open.
    fn placed(&self, op: DrawOp) -> DrawOp {
        match op {
            DrawOp::Intersect(mut i) => {
                i.path.clone_from(&self.editing);
                DrawOp::Intersect(i)
            }
            op => op,
        }
    }

    /// Carries out what the draw tool asked for.
    fn run_outcome(&mut self, outcome: Outcome) {
        let ctx = self.ctx;
        let outcome = match outcome {
            Outcome::Commit(op) => Outcome::Commit(self.placed(op)),
            Outcome::Redefine(op) => Outcome::Redefine(self.placed(op)),
            other => other,
        };
        match outcome {
            Outcome::Nothing => {}
            Outcome::Message(m) => self.message = Some(m),
            Outcome::Commit(op) => {
                let before = self.document.revision();
                let result = self
                    .document
                    .transaction(op.name(), |d| run_draw(d, ctx, &op));
                self.last_step = self.document.revision() != before;
                match result {
                    Ok(made) => {
                        self.report = made.report;
                        // A new shape: the previously confirmed one can no
                        // longer be redefined.
                        self.confirmed = None;
                        self.redefinable = true;
                        self.message = made.hint.map(str::to_owned);
                        self.chain_before = self.chain.clone();
                        self.note_transform(&op);
                        self.bent.clear();
                        self.auto_tri_offer = None;
                        self.offer_materials(&op);
                        if made.offer_auto_tri {
                            self.message = Some(AUTO_TRI_MESSAGE.into());
                            self.auto_tri_offer = Some((op, true));
                        }
                    }
                    Err(e) => {
                        self.end_redefinition();
                        self.note_bent(&e);
                        self.message = Some(match (&op, &e) {
                            // Fuse says why in its own words.
                            (DrawOp::Fuse(_), ModelError::Refused(why)) => why.clone(),
                            _ => format!("Couldn't draw that {}: {e}.", op.name()),
                        });
                        self.offer_auto_tri(&e, op, false);
                    }
                }
                self.after_change();
            }
            Outcome::Redefine(op) => {
                if !self.redefinable {
                    if let Some(t) = self.draw_tool.as_mut() {
                        t.forget();
                    }
                    self.message = Some(format!(
                        "The last {} can no longer be changed by typing.",
                        op.name()
                    ));
                    return;
                }
                let stepped = self.step_back();
                let before = self.document.revision();
                let result = self
                    .document
                    .transaction(op.name(), |d| run_draw(d, ctx, &op));
                self.last_step = self.document.revision() != before;
                let hint = result.as_ref().ok().and_then(|made| made.hint);
                let offer = result.as_ref().is_ok_and(|made| made.offer_auto_tri);
                if let Ok(made) = &result {
                    self.report.clone_from(&made.report);
                }
                if let Err(e) = result {
                    // Put the previous shape back and stop redefining.
                    if stepped {
                        self.document.redo();
                        self.last_step = true;
                    }
                    self.end_redefinition();
                    self.note_bent(&e);
                    self.message = Some(match (&op, &e) {
                        (DrawOp::Fuse(_), ModelError::Refused(why)) => why.clone(),
                        _ => format!("Couldn't change the {}: {e}.", op.name()),
                    });
                    self.offer_auto_tri(&e, op, true);
                } else {
                    self.message = hint.map(str::to_owned);
                    self.chain = self.chain_before.clone();
                    self.note_transform(&op);
                    self.offer_materials(&op);
                    if offer {
                        self.message = Some(AUTO_TRI_MESSAGE.into());
                        self.auto_tri_offer = Some((op, true));
                    }
                }
                self.after_change();
            }
        }
    }

    /// World size of one screen pixel near `p` (for tool-level snapping).
    fn pixel_at(&self, p: DVec3) -> f64 {
        let Some(s0) = self.to_screen(p) else {
            return 0.0;
        };
        let px_per_mm = [DVec3::X, DVec3::Y, DVec3::Z]
            .into_iter()
            .filter_map(|d| self.to_screen(p + d).map(|s| s.distance(s0)))
            .fold(0.0, f64::max);
        if px_per_mm > 0.0 {
            1.0 / px_per_mm
        } else {
            0.0
        }
    }

    /// Detects a lone Ctrl tap (pressed and released with nothing else) and
    /// passes it to the draw tool as its mode switch.
    fn ctrl_tap(&mut self, ui: &Ui) {
        let (ctrl, other) = ui.input(|i| {
            let other = i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key { pressed: true, .. }
                        | egui::Event::PointerButton { pressed: true, .. }
                        | egui::Event::MouseWheel { .. }
                )
            });
            (i.modifiers.ctrl || i.modifiers.command, other)
        });
        if ctrl {
            if !self.ctrl_held {
                self.ctrl_held = true;
                self.ctrl_used = false;
            }
            self.ctrl_used |= other;
        } else if std::mem::take(&mut self.ctrl_held)
            && !self.ctrl_used
            && let Some(t) = self.draw_tool.as_mut()
        {
            let ctx = Ctx {
                entities: ctx_of(&self.document, self.ctx),
                axes: &self.axes_ctx,
                instances: &self.instance_boxes,
                instance_lines: &self.instance_lines,
                pixel: 0.0,
                cursor: self.cursor,
            };
            let outcome = t.handle(Input::Toggle, &ctx);
            self.run_outcome(outcome);
        }
    }

    /// Lays out, handles input for, and draws the canvas filling `ui`.
    pub fn ui(&mut self, ui: &mut Ui, keymap: &Keymap, prefs: &ViewPrefs<'_>) {
        self.resolve_context();
        self.show_guides = prefs.guides;
        self.units = *prefs.units;
        if (self.show_sections, self.section_cuts) != (prefs.sections, prefs.cuts) {
            self.show_sections = prefs.sections;
            self.section_cuts = prefs.cuts;
            self.refresh_mesh();
        }
        let rect = ui.max_rect();
        self.rect = rect;
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        // A question on screen holds every canvas input until answered.
        let held = self.input_held();
        if self.mode == Mode::Edit && !held {
            self.ctrl_tap(ui);
        }
        let pie_open = !held && self.pie_input(ui);
        if !pie_open && !held {
            let typing = self.handle_typing(ui, prefs);
            self.handle_input(ui, &response, keymap, typing);
            if self.tool != WorkTool::Select && self.mode == Mode::Edit {
                self.handle_inference(ui, &response, prefs);
            }
        }
        self.draw(ui, rect);
        self.draw_annotations(ui);
        if self.mode == Mode::Edit && self.show_sections {
            self.draw_sections(ui);
        }
        if self.mode == Mode::Edit {
            if self.show_guides {
                self.draw_guides(ui);
            }
            self.draw_selection_rect(ui);
            self.draw_vertices(ui);
            self.draw_inference(ui);
            self.draw_pivot(ui);
            self.draw_bent(ui);
            if !pie_open {
                response.context_menu(|ui| self.context_menu(ui));
            }
        }
        self.draw_pie(ui);
    }

    // ----- input -------------------------------------------------------------

    fn handle_input(&mut self, ui: &Ui, response: &egui::Response, keymap: &Keymap, typing: bool) {
        let (shift, ctrl, alt) = ui.input(|i| {
            (
                i.modifiers.shift,
                i.modifiers.command || i.modifiers.ctrl,
                i.modifiers.alt,
            )
        });
        let delta = response.drag_delta();
        let (dx, dy) = (f64::from(delta.x), f64::from(delta.y));
        let height = f64::from(self.rect.height());

        // Camera navigation.
        if response.dragged_by(PointerButton::Middle) {
            if shift {
                self.camera.pan(dx, dy, height);
            } else {
                self.camera.orbit(dx, dy);
            }
        } else if response.dragged_by(PointerButton::Primary) {
            match self.nav {
                NavTool::Orbit => self.camera.orbit(dx, dy),
                NavTool::Pan => self.camera.pan(dx, dy, height),
                NavTool::Zoom => {
                    let target = self.camera.target;
                    self.camera.zoom_toward(target, (dy * DRAG_ZOOM).exp());
                }
                NavTool::None => {}
            }
        }

        // Paint with the left button (Present mode, Paint tool); Alt samples.
        if self.nav == NavTool::None
            && self.mode == Mode::Present
            && self.tool == WorkTool::Paint
            && response.clicked_by(PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            let alt = response.ctx.input(|i| i.modifiers.alt);
            self.paint_click(pos, alt);
        }
        // Mark Edges and Line Style: a click on an edge.
        if self.nav == NavTool::None
            && self.mode == Mode::Present
            && matches!(self.tool, WorkTool::MarkEdges | WorkTool::LineStyle)
            && response.clicked_by(PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.edge_click(pos);
        }

        // Selection with the left button, Select tool, in either mode.
        if self.nav == NavTool::None && self.tool == WorkTool::Select {
            let mode = SelectMode::from_modifiers(alt, ctrl, shift);
            if response.drag_started_by(PointerButton::Primary) {
                let start = response.interact_pointer_pos();
                let on_selected = start.is_some_and(|p| {
                    self.pick_select(p)
                        .into_iter()
                        .chain(self.pick_at(p))
                        .any(|x| self.selection.contains(x))
                });
                // In Present mode geometry cannot change: no move.
                if on_selected && self.mode == Mode::Edit {
                    // A drag that starts on selected geometry moves it: the
                    // Move tool takes over for this drag (its start point is
                    // placed below, this frame) and hands back to Select.
                    self.drag_move = true;
                    self.set_tool(WorkTool::Draw(DrawKind::Move));
                } else {
                    self.drag_start = start;
                }
            }
            if response.drag_stopped_by(PointerButton::Primary)
                && let (Some(start), Some(end)) =
                    (self.drag_start.take(), response.interact_pointer_pos())
            {
                // Left → right: window. Right → left: crossing.
                let crossing = end.x < start.x;
                let picked = self.rect_select(start, end, crossing);
                self.select(picked, mode);
            } else if let Some(pos) = response.interact_pointer_pos() {
                if response.triple_clicked() {
                    if let Some(p) = self.pick_at(pos) {
                        let set = self.active().connected_set(p);
                        let set = self.in_kind(set);
                        self.select(set, mode);
                    }
                } else if response.double_clicked() {
                    if let Some(Pickable::Instance(id)) = self.pick_at(pos) {
                        self.enter_instance(id);
                    } else if let Some(p) = self.pick_at(pos) {
                        let set = self.active().double_click_set(p);
                        let set = self.in_kind(set);
                        self.select(set, mode);
                    }
                } else if response.clicked_by(PointerButton::Primary) {
                    let picked = self.pick_select(pos);
                    // Ctrl + Shift: the path pick.
                    if ctrl
                        && shift
                        && let Some(p) = picked
                    {
                        self.select_path(p);
                    } else {
                        self.select(picked.into_iter().collect(), mode);
                    }
                }
            }
        }

        // Where the context menu opens (for Set Pivot Here).
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.menu_point = Some(self.surface_point(pos));
        }

        // Right-click selects what is under the cursor unless it is already
        // selected, so the context menu acts on it (SketchUp behaviour).
        if self.mode == Mode::Edit
            && response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
            && let Some(p) = self.pick_at(pos)
            && !self.selection.contains(p)
        {
            self.select(vec![p], SelectMode::Replace);
        }

        // Wheel zoom toward the cursor; during a proportional move, the
        // wheel sets the radius instead.
        if response.hovered() {
            let scroll = f64::from(ui.input(|i| i.smooth_scroll_delta.y));
            if scroll != 0.0 && self.proportional_moving() {
                let mut p = self.proportional;
                p.radius = (p.radius * (-scroll * WHEEL_ZOOM).exp()).clamp(0.001, 1.0e6);
                self.set_proportional(true, p);
            } else if scroll != 0.0
                && let Some(pos) = ui.input(|i| i.pointer.hover_pos())
            {
                self.zoom_at(pos, (-scroll * WHEEL_ZOOM).exp());
            }
        }

        // Keymap actions (suspended while typing into the measurement box, so
        // letters such as `m` in `5mm` never trigger tools).
        if !typing && !ui.ctx().egui_wants_keyboard_input() {
            let triggered: Vec<Action> = keymap
                .ordered()
                .into_iter()
                .filter(|(_, chord)| ui.input_mut(|i| i.consume_key(chord.modifiers, chord.key)))
                .map(|(action, _)| action)
                .collect();
            for action in triggered {
                self.apply(action);
            }
        }
    }

    /// Screen point (logical px) → normalised device coordinates.
    fn ndc(&self, pos: Pos2) -> DVec2 {
        let r = self.rect;
        DVec2::new(
            f64::from((pos.x - r.min.x) / r.width()) * 2.0 - 1.0,
            1.0 - f64::from((pos.y - r.min.y) / r.height()) * 2.0,
        )
    }

    /// World → screen (logical px), `None` behind the camera.
    fn to_screen(&self, p: DVec3) -> Option<DVec2> {
        let ndc = self
            .camera
            .project(self.local.transform_point3(p), self.aspect())?;
        let r = self.rect;
        Some(DVec2::new(
            f64::from(r.min.x) + (ndc.x + 1.0) * 0.5 * f64::from(r.width()),
            f64::from(r.min.y) + (1.0 - ndc.y) * 0.5 * f64::from(r.height()),
        ))
    }

    /// The entity under screen position `pos`.
    fn pick_at(&self, pos: Pos2) -> Option<Pickable> {
        let aspect = self.aspect();
        let (origin, dir) = self.ray(self.ndc(pos), aspect);
        let height = f64::from(self.rect.height());
        let camera = self.camera;
        let px_at = move |t: f64| match camera.projection {
            Projection::Perspective => 2.0 * t * (camera.fov_y * 0.5).tan() / height.max(1.0),
            Projection::Parallel => camera.world_per_pixel(height),
        };
        smith_infer::select::pick_click_in(&self.pick_index.edges, &self.bvh, origin, dir, px_at)
            .and_then(|p| self.owner_of(p))
    }

    /// The face under `pos`, or the group it belongs to.
    fn face_or_group_at(&self, pos: Pos2) -> Option<Pickable> {
        let aspect = self.aspect();
        let (origin, dir) = self.ray(self.ndc(pos), aspect);
        self.bvh
            .ray_cast(origin, dir)
            .and_then(|hit| self.owner_of(Pickable::Face(hit.face)))
    }

    /// Ctrl+click: adds the vertices or edges on the shortest way along
    /// edges from the last one picked to `to`, in order (Edit mode,
    /// Vertices or Edges). Without a last pick of the same kind, or with
    /// no way between them, `to` is just added.
    fn select_path(&mut self, to: Pickable) {
        let e = self.active();
        let path: Vec<Pickable> = match (self.mode, self.selection.last(), to) {
            (Mode::Edit, Some(Pickable::Vertex(a)), Pickable::Vertex(b)) => e
                .vertex_path(a, b)
                .map(|p| p.into_iter().map(Pickable::Vertex).collect())
                .unwrap_or_default(),
            (Mode::Edit, Some(Pickable::Edge(a)), Pickable::Edge(b)) => e
                .edge_path(a, b)
                .map(|p| p.into_iter().map(Pickable::Edge).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let picked = if path.is_empty() { vec![to] } else { path };
        self.select(picked, SelectMode::Add);
    }

    /// What a Select click at `pos` picks: in Edit mode, only an item of
    /// the select mode (or a group, picked whole); in Present mode, as
    /// before.
    fn pick_select(&self, pos: Pos2) -> Option<Pickable> {
        if self.mode == Mode::Present {
            return self.pick_at(pos);
        }
        let aspect = self.aspect();
        let (origin, dir) = self.ray(self.ndc(pos), aspect);
        let height = f64::from(self.rect.height());
        let camera = self.camera;
        let px_at = move |t: f64| match camera.projection {
            Projection::Perspective => 2.0 * t * (camera.fov_y * 0.5).tan() / height.max(1.0),
            Projection::Parallel => camera.world_per_pixel(height),
        };
        let found = if self.select_kind == SelectKind::Vertices {
            // Vertices come from the context itself.
            smith_infer::select::pick_vertex_in(
                &self.pick_index.vertices,
                &self.bvh,
                origin,
                dir,
                px_at,
            )
            .map(Pickable::Vertex)
        } else {
            smith_infer::select::pick_kind_in(
                &self.pick_index,
                &self.bvh,
                origin,
                dir,
                px_at,
                self.select_kind,
            )
            .and_then(|p| self.owner_of(p))
        };
        // Groups and components are picked whole in every mode.
        found.or_else(|| {
            self.pick_at(pos)
                .filter(|p| matches!(p, Pickable::Instance(_)))
        })
    }

    /// `items` as the select mode gives them (Edit mode only).
    fn in_kind(&self, items: Vec<Pickable>) -> Vec<Pickable> {
        if self.mode == Mode::Present {
            return items;
        }
        self.active().converted(&items, self.select_kind)
    }

    /// What a Select box picks: in Edit mode, the select mode's items with
    /// what they complete or contain, and groups by their boxes; in Present
    /// mode, as before.
    fn rect_select(&self, a: Pos2, b: Pos2, crossing: bool) -> Vec<Pickable> {
        let picked = self.rect_pick(a, b, crossing);
        if self.mode == Mode::Present {
            return picked;
        }
        let mut picked = picked;
        if self.select_kind == SelectKind::Vertices {
            let to = |p: Pos2| DVec2::new(f64::from(p.x), f64::from(p.y));
            picked.extend(smith_infer::select::pick_rect_vertices(
                self.active(),
                to(a),
                to(b),
                |p| self.to_screen(p),
            ));
        }
        self.active().completed(&picked, self.select_kind)
    }

    fn rect_pick(&self, a: Pos2, b: Pos2, crossing: bool) -> Vec<Pickable> {
        let to = |p: Pos2| DVec2::new(f64::from(p.x), f64::from(p.y));
        let mut picked = pick_rect(self.active(), to(a), to(b), crossing, |p| self.to_screen(p));
        // Groups and components by their boxes: window takes those wholly
        // inside, crossing those with any corner inside.
        let (lo, hi) = (to(a).min(to(b)), to(a).max(to(b)));
        for (id, corners) in &self.instance_boxes {
            if self
                .active()
                .instance(*id)
                .is_none_or(|i| i.hidden || !self.document.layer_visible(i.layer))
            {
                continue;
            }
            let inside: Vec<bool> = corners
                .iter()
                .map(|c| {
                    self.to_screen(*c)
                        .is_some_and(|s| s.x >= lo.x && s.x <= hi.x && s.y >= lo.y && s.y <= hi.y)
                })
                .collect();
            let take = if crossing {
                inside.iter().any(|x| *x)
            } else {
                inside.iter().all(|x| *x)
            };
            if take {
                picked.push(Pickable::Instance(*id));
            }
        }
        picked
    }

    /// Zooms by `factor` toward whatever is under screen position `pos`.
    fn zoom_at(&mut self, pos: Pos2, factor: f64) {
        let point = self.surface_point(pos);
        self.camera.zoom_toward(point, factor);
    }

    /// The model surface under `pos`, or the camera's target plane.
    fn surface_point(&self, pos: Pos2) -> DVec3 {
        let ndc = self.ndc(pos);
        let aspect = self.aspect();
        let (origin, dir) = self.ray(ndc, aspect);
        self.bvh
            .ray_cast(origin, dir)
            .map(|hit| self.local.transform_point3(hit.point))
            .unwrap_or_else(|| self.camera.point_on_target_plane(ndc, aspect))
    }

    /// Whether a move with proportional editing is under way (between its
    /// start and end clicks), so the wheel sets the radius.
    fn proportional_moving(&self) -> bool {
        self.proportional_on
            && self
                .draw_tool
                .as_ref()
                .is_some_and(|t| t.name() == "Move" && t.last_point().is_some())
    }

    /// Proportional editing: whether it is on, and its settings.
    pub fn proportional(&self) -> (bool, smith_ops::modify::Proportional) {
        (self.proportional_on, self.proportional)
    }

    /// Sets proportional editing; the active tool follows at once.
    pub fn set_proportional(&mut self, on: bool, p: smith_ops::modify::Proportional) {
        self.proportional_on = on;
        self.proportional = p;
        if let Some(t) = self.draw_tool.as_mut() {
            t.use_proportional(on.then_some(p));
        }
    }

    /// Whether Pivot Active is on.
    pub fn pivot_active(&self) -> bool {
        self.pivot_active
    }

    /// Turns Pivot Active on or off; the active tool follows at once.
    pub fn set_pivot_active(&mut self, on: bool) {
        self.pivot_active = on;
        let pivot = on.then_some(self.local.inverse().transform_point3(self.pivot));
        if let Some(t) = self.draw_tool.as_mut() {
            t.use_pivot(pivot);
        }
    }

    /// Places the Pivot and turns Pivot Active on.
    fn set_pivot(&mut self, p: DVec3) {
        self.pivot = p;
        self.pivot_tie = None;
        self.set_pivot_active(true);
    }

    /// Ties the Pivot to the one selected item (Set Active Selection): an
    /// edge's midpoint (a curve's true centre when the edge is part of a
    /// circle, arc or polygon), a face's centre, or a group's or component's
    /// box centre. It follows the item from then on.
    fn tie_pivot(&mut self) {
        let items: Vec<Pickable> = self.selection.iter().take(2).collect();
        let [item] = items[..] else {
            self.message = Some("Select one edge, face or group to tie the Pivot to.".into());
            return;
        };
        self.pivot_tie = Some((self.ctx, item));
        self.follow_pivot();
        self.set_pivot_active(true);
    }

    /// Moves a tied Pivot to where its item now is; an item that no longer
    /// exists leaves the Pivot where it was, untied.
    fn follow_pivot(&mut self) {
        let Some((ctx, item)) = self.pivot_tie else {
            return;
        };
        let Some(e) = self.document.context(ctx) else {
            self.pivot_tie = None;
            return;
        };
        let local = match item {
            Pickable::Edge(id) => e.edge(id).and_then(|edge| {
                let centre = edge
                    .curve()
                    .and_then(|c| e.curve(c))
                    .and_then(|r| match r.curve() {
                        smith_geom::curve::Curve::Arc(a) => Some(a.frame().center()),
                        smith_geom::curve::Curve::Circle(c) => Some(c.frame().center()),
                        smith_geom::curve::Curve::Polygon(p) => Some(p.frame().center()),
                        _ => None,
                    });
                centre.or_else(|| e.edge_points(id).map(|(a, b)| (a + b) * 0.5))
            }),
            Pickable::Face(id) => e.face(id).map(|f| {
                let pts = e.loop_points(f.outer());
                pts.iter().copied().sum::<DVec3>() / pts.len().max(1) as f64
            }),
            Pickable::Instance(id) => self
                .document
                .instance_boxes(ctx)
                .get(&id)
                .map(|c| c.iter().copied().sum::<DVec3>() / 8.0),
            Pickable::Guide(id) => e.guide(id).map(Guide::anchor),
            Pickable::Vertex(id) => e.vertex(id).map(smith_model::Vertex::position),
        };
        match local {
            Some(p) => {
                // The tie's context may be a group placed elsewhere.
                let mut m = DMat4::IDENTITY;
                let mut c = None;
                for id in &self.editing {
                    if c == ctx {
                        break;
                    }
                    if let Some(inst) = self.document.context(c).and_then(|x| x.instance(*id)) {
                        m *= inst.transform;
                        c = Some(inst.definition);
                    }
                }
                self.pivot = m.transform_point3(p);
            }
            None => self.pivot_tie = None,
        }
    }

    /// After an operation is refused because it would bend faces: says so,
    /// and asks whether to do it with Auto-Tri, for that operation alone.
    fn offer_auto_tri(&mut self, e: &ModelError, op: DrawOp, was_redefine: bool) {
        if matches!(e, ModelError::NotFlat(_)) {
            self.message = Some(AUTO_TRI_MESSAGE.into());
            self.auto_tri_offer = Some((op, was_redefine));
        }
    }

    /// Yes to Auto Triangulate: the refused operation again, folding only
    /// the faces it bends into triangles (one undo step). The offer goes
    /// with it, so the next operation starts without Auto-Tri.
    fn retry_auto_tri(&mut self) {
        let Some((op, was_redefine)) = self.auto_tri_offer.take() else {
            return;
        };
        let ctx = self.ctx;
        let stepped = was_redefine && self.step_back();
        let before = self.document.revision();
        let result = smith_model::with_auto_tri(|| {
            self.document
                .transaction(op.name(), |d| run_draw(d, ctx, &op))
        });
        self.last_step = self.document.revision() != before;
        match result {
            Ok(_) => {
                self.message = None;
                self.bent.clear();
                self.note_transform(&op);
            }
            Err(e) => {
                if stepped {
                    self.document.redo();
                    self.last_step = true;
                }
                self.note_bent(&e);
                self.message = Some(format!(
                    "Couldn't do the {} even with triangles: {e}.",
                    op.name()
                ));
            }
        }
        self.after_change();
    }

    /// After a Clean Up that held unused materials back: says so, and asks
    /// whether to remove them. Nothing when there are none.
    fn offer_materials(&mut self, op: &DrawOp) {
        self.material_offer = None;
        let DrawOp::CleanUp(c) = op else {
            return;
        };
        if !c.passes.purge_unused || c.remove_materials {
            return;
        }
        let n = self.document.unused_material_count();
        if n > 0 {
            self.message = Some("You Have Unused Materials".into());
            self.material_offer = Some((op.clone(), n));
        }
    }

    /// Yes to removing unused materials: the Clean Up again with them
    /// removed, in place of the run that held them back (one undo step).
    fn remove_materials(&mut self) {
        let Some((DrawOp::CleanUp(mut c), _)) = self.material_offer.take() else {
            return;
        };
        c.remove_materials = true;
        let op = DrawOp::CleanUp(c);
        let ctx = self.ctx;
        let stepped = self.step_back();
        let before = self.document.revision();
        let result = self
            .document
            .transaction(op.name(), |d| run_draw(d, ctx, &op));
        self.last_step = self.document.revision() != before;
        match result {
            Ok(made) => {
                self.report = made.report;
                self.message = None;
            }
            Err(e) => {
                if stepped {
                    self.document.redo();
                    self.last_step = true;
                }
                self.message = Some(format!("Couldn't remove the materials: {e}."));
            }
        }
        self.after_change();
    }

    /// Undoes the last run of the active operation, before it is run again,
    /// if it recorded a step. Returns whether it did.
    fn step_back(&mut self) -> bool {
        let stepped = self.last_step;
        if stepped {
            self.document.undo();
            self.last_step = false;
        }
        stepped
    }

    /// Keeps the faces a refused change would have bent, for the red outline.
    fn note_bent(&mut self, e: &smith_model::ModelError) {
        if let smith_model::ModelError::NotFlat(faces) = e {
            self.bent.clone_from(faces);
        }
    }

    /// Outlines in red the faces the last refused change would have bent.
    fn draw_bent(&self, ui: &Ui) {
        let root = self.active();
        let stroke = Stroke::new(3.0, Color32::from_rgb(220, 30, 30));
        let painter = ui.painter();
        for f in &self.bent {
            let Some(face) = root.face(*f) else {
                continue;
            };
            for l in face.loops() {
                let pts: Vec<Pos2> = root
                    .loop_points(l)
                    .into_iter()
                    .filter_map(|p| self.to_screen(p))
                    .map(|s| pos2(s.x as f32, s.y as f32))
                    .collect();
                for i in 0..pts.len() {
                    painter.line_segment([pts[i], pts[(i + 1) % pts.len()]], stroke);
                }
            }
        }
    }

    /// Draws the Pivot marker while Pivot Active is on.
    /// In Edit mode's Vertices mode, every vertex of the open context as a
    /// small dark square; selected vertices as filled squares; and with the
    /// Select tool, a hollow square on the vertex a click would pick.
    fn draw_vertices(&self, ui: &Ui) {
        let [r, g, b, _] = smith_render::mesh::SELECTED_EDGE_COLOR.map(|c| (c * 255.0) as u8);
        let colour = Color32::from_rgb(r, g, b);
        let painter = ui.painter();
        let at = |p: DVec3| self.to_screen(p).map(|s| pos2(s.x as f32, s.y as f32));
        if self.mode == Mode::Edit && self.select_kind == SelectKind::Vertices {
            let rect = self.rect;
            let dark = Color32::from_gray(40);
            for v in self.active().vertex_ids() {
                if let Some(c) = self.active().vertex(v).and_then(|x| at(x.position()))
                    && rect.contains(c)
                {
                    painter.rect_filled(
                        egui::Rect::from_center_size(c, Vec2::splat(4.0)),
                        0.0,
                        dark,
                    );
                }
            }
        }
        for v in self.selection.vertices() {
            if let Some(c) = self.active().vertex(v).and_then(|x| at(x.position())) {
                painter.rect_filled(
                    egui::Rect::from_center_size(c, Vec2::splat(7.0)),
                    0.0,
                    colour,
                );
            }
        }
        if self.tool == WorkTool::Select
            && self.nav == NavTool::None
            && self.select_kind == SelectKind::Vertices
            && let Some(pos) = ui.input(|i| i.pointer.hover_pos())
            && self.rect.contains(pos)
            && let Some(Pickable::Vertex(v)) = self.pick_select(pos)
            && let Some(c) = self.active().vertex(v).and_then(|x| at(x.position()))
        {
            painter.rect_stroke(
                egui::Rect::from_center_size(c, Vec2::splat(11.0)),
                0.0,
                Stroke::new(1.5, colour),
                egui::StrokeKind::Middle,
            );
        }
    }

    fn draw_pivot(&self, ui: &Ui) {
        if !self.pivot_active {
            return;
        }
        let Some(s) = self.to_screen(self.pivot) else {
            return;
        };
        let c = pos2(s.x as f32, s.y as f32);
        let colour = Color32::from_rgb(230, 120, 0);
        let stroke = Stroke::new(2.0, colour);
        let painter = ui.painter();
        painter.circle_stroke(c, 7.0, stroke);
        painter.line_segment([c - Vec2::new(11.0, 0.0), c + Vec2::new(11.0, 0.0)], stroke);
        painter.line_segment([c - Vec2::new(0.0, 11.0), c + Vec2::new(0.0, 11.0)], stroke);
    }

    // ----- context menu ------------------------------------------

    fn context_menu(&mut self, ui: &mut Ui) {
        let entries = if self.selection.is_empty() {
            CONTEXT_MENU_EMPTY
        } else {
            CONTEXT_MENU
        };
        self.menu_entries(ui, entries);
    }

    /// Renders `entries`: commands that apply, submenus with at least one
    /// command that applies, and separators only between shown items.
    pub fn menu_entries(&mut self, ui: &mut Ui, entries: &[Entry]) {
        let shown: Vec<Entry> = entries
            .iter()
            .copied()
            .filter(|e| match e {
                Entry::Item(c) => self.command_applies(*c),
                Entry::Sub(_, cs) => cs.iter().any(|c| self.command_applies(*c)),
                Entry::Separator => true,
            })
            .collect();
        let mut pending_separator = false;
        let mut any = false;
        for e in shown {
            match e {
                Entry::Separator => pending_separator = any,
                Entry::Item(c) => {
                    if pending_separator {
                        ui.separator();
                        pending_separator = false;
                    }
                    any = true;
                    if ui.button(self.command_label(c)).clicked() {
                        self.run_command(c);
                        ui.close();
                    }
                }
                Entry::Sub(title, cs) => {
                    if pending_separator {
                        ui.separator();
                        pending_separator = false;
                    }
                    any = true;
                    ui.menu_button(title, |ui| {
                        for c in cs {
                            if self.command_applies(*c)
                                && ui.button(self.command_label(*c)).clicked()
                            {
                                self.run_command(*c);
                                ui.close();
                            }
                        }
                    });
                }
            }
        }
    }

    /// The single selected group or component, if exactly one is selected
    /// and nothing else.
    /// The single selected group, when it is a shape still: its
    /// definition and sizes.
    fn selected_shape(&self) -> Option<(DefinitionId, smith_model::ShapeSpec)> {
        let i = self.single_instance()?;
        let def = self.active().instance(i)?.definition;
        Some((def, self.document.definition(def)?.shape?))
    }

    /// The single selected group, when it is a sweep or revolve still: its
    /// definition, recipe and placement.
    fn selected_sweep(&self) -> Option<(DefinitionId, smith_model::SweepRecipe, DMat4)> {
        let i = self.single_instance()?;
        let inst = self.active().instance(i)?;
        let recipe = self.document.definition(inst.definition)?.sweep.clone()?;
        Some((inst.definition, recipe, inst.transform))
    }

    fn single_instance(&self) -> Option<InstanceId> {
        let mut i = self.selection.instances();
        match (i.next(), i.next(), self.selection.iter().count()) {
            (Some(x), None, 1) => Some(x),
            _ => None,
        }
    }

    /// The single selected face, if exactly one face is selected.
    fn single_face(&self) -> Option<smith_model::FaceId> {
        let mut f = self.selection.faces();
        match (f.next(), f.next()) {
            (Some(x), None) => Some(x),
            _ => None,
        }
    }

    /// Whether a group or component in the selection is locked.
    fn single_locked(&self) -> bool {
        self.single_instance()
            .and_then(|i| self.active().instance(i))
            .is_some_and(|i| i.locked)
    }

    /// A command's label for the current selection.
    pub fn command_label(&self, c: Command) -> String {
        const AXES: [&str; 3] = ["X", "Y", "Z"];
        match c {
            Command::Erase => "Erase".into(),
            Command::Hide => "Hide".into(),
            Command::Unhide => "Unhide".into(),
            Command::HideOthers => "Hide Others".into(),
            Command::UnhideLast => "Unhide Last".into(),
            Command::UnhideAll => "Unhide All".into(),
            Command::EditGroup => {
                let component = self
                    .single_instance()
                    .and_then(|i| self.active().instance(i))
                    .and_then(|i| self.document.definition(i.definition))
                    .is_some_and(|d| d.kind == DefinitionKind::Component);
                if component {
                    "Edit Component"
                } else {
                    "Edit Group"
                }
                .into()
            }
            Command::EditCurve => "Edit Curve".into(),
            Command::SelectConnected => "All Connected".into(),
            Command::SelectBoundingEdges => "Bounding Edges".into(),
            Command::SelectConnectedFaces => "Connected Faces".into(),
            Command::SelectAll => "Select All".into(),
            Command::SetPivotHere => "Set Pivot Here".into(),
            Command::PivotToOrigin => "Pivot to Origin".into(),
            Command::PivotToSelection => "Set Active Selection".into(),
            Command::AlignTo(a) => format!("Align to {}", AXES[usize::from(a.min(2))]),
            Command::FlipAlong(a) => format!("Flip Along {}", AXES[usize::from(a.min(2))]),
            Command::ReverseFaces => "Reverse Faces".into(),
            Command::OrientFaces => "Orient Faces".into(),
            Command::Divide => "Divide".into(),
            Command::AlignView => "Align View".into(),
            Command::ZoomSelection => "Zoom Selection".into(),
            Command::ZoomExtents => "Zoom Extents".into(),
            Command::Paste => "Paste".into(),
            Command::MakeGroup => "Make Group".into(),
            Command::MakeComponent => "Make Component".into(),
            Command::Explode => "Explode".into(),
            Command::MakeUnique => "Make Unique".into(),
            Command::Lock => if self.single_locked() {
                "Unlock"
            } else {
                "Lock"
            }
            .into(),
            Command::ResetScale => "Reset Scale".into(),
            Command::ChangeAxes => "Change Axes".into(),
            Command::EditShape => "Edit Shape".into(),
            // Named for what the selected group was made by.
            Command::EditSweep => match self.selected_sweep() {
                Some((_, smith_model::SweepRecipe::Loft { .. }, _)) => "Edit Loft".into(),
                Some((_, smith_model::SweepRecipe::Revolve { .. }, _)) => "Edit Revolve".into(),
                Some((_, smith_model::SweepRecipe::Skin { .. }, _)) => "Edit Skin".into(),
                _ => "Edit Extrude".into(),
            },
        }
    }

    /// Whether `c` applies to the current selection.
    pub fn command_applies(&self, c: Command) -> bool {
        let has = !self.selection.is_empty();
        let geometry =
            self.selection.faces().next().is_some() || self.selection.edges().next().is_some();
        let unlocked_one = self.single_instance().is_some() && !self.single_locked();
        match c {
            Command::Erase | Command::ZoomSelection => has,
            Command::Hide => has && !self.selection_has_hidden(),
            Command::Unhide => self.selection_has_hidden(),
            Command::HideOthers => has,
            Command::UnhideLast => !self.last_hidden.is_empty(),
            Command::UnhideAll => {
                let a = self.active();
                !a.hidden_edges().is_empty()
                    || !a.hidden_faces().is_empty()
                    || !a.hidden_instances().is_empty()
            }
            Command::EditGroup | Command::Explode | Command::ResetScale | Command::ChangeAxes => {
                unlocked_one
            }
            Command::EditCurve => self.selected_curve().is_some(),
            Command::EditShape => unlocked_one && self.selected_shape().is_some(),
            Command::EditSweep => unlocked_one && self.selected_sweep().is_some(),
            Command::SelectConnected => has,
            Command::SelectBoundingEdges => {
                !self.active().bounding_edges(&self.selection).is_empty()
            }
            Command::SelectConnectedFaces => {
                !self.active().connected_faces(&self.selection).is_empty()
            }
            Command::SelectAll | Command::ZoomExtents => true,
            Command::SetPivotHere => self.menu_point.is_some(),
            Command::PivotToOrigin => true,
            Command::PivotToSelection => self.selection.iter().count() == 1,
            Command::AlignTo(_) | Command::FlipAlong(_) => has,
            Command::ReverseFaces => self.selection.faces().next().is_some(),
            Command::OrientFaces | Command::AlignView => self.single_face().is_some(),
            Command::Divide => self.selection.edges().next().is_some(),
            Command::Paste => self.clipboard.is_some(),
            Command::MakeGroup | Command::MakeComponent => geometry,
            Command::MakeUnique => {
                unlocked_one
                    && self
                        .single_instance()
                        .and_then(|i| self.active().instance(i))
                        .is_some_and(|i| self.document.instance_users(i.definition) > 1)
            }
            Command::Lock => self.single_instance().is_some(),
        }
    }

    /// Runs `c` on the current selection.
    pub fn run_command(&mut self, c: Command) {
        let ctx = self.ctx;
        match c {
            Command::Erase => self.erase_selection(),
            Command::Hide => self.hide_selection(),
            Command::Unhide => self.unhide_selection(),
            Command::HideOthers => self.hide_others(),
            Command::UnhideLast => self.apply(Action::UnhideLast),
            Command::UnhideAll => self.apply(Action::UnhideAll),
            Command::EditGroup => {
                if let Some(i) = self.single_instance() {
                    self.enter_instance(i);
                }
            }
            Command::EditCurve => {
                if let Some(cv) = self.selected_curve() {
                    self.edit_curve(cv);
                }
            }
            Command::SelectConnected => {
                let root = self.active();
                let all: Vec<Pickable> = self
                    .selection
                    .iter()
                    .flat_map(|p| root.connected_set(p))
                    .collect();
                self.select(all, SelectMode::Add);
            }
            Command::SelectBoundingEdges => {
                let b = self.active().bounding_edges(&self.selection);
                self.select(b, SelectMode::Add);
            }
            Command::SelectConnectedFaces => {
                let f = self.active().connected_faces(&self.selection);
                self.select(f, SelectMode::Add);
            }
            Command::SelectAll => self.apply(Action::SelectAll),
            Command::SetPivotHere => {
                if let Some(p) = self.menu_point {
                    self.set_pivot(p);
                }
            }
            Command::PivotToOrigin => {
                let origin = self.axes.origin;
                self.set_pivot(origin);
            }
            Command::PivotToSelection => self.tie_pivot(),
            Command::AlignTo(a) => self.align_selection(usize::from(a.min(2))),
            Command::FlipAlong(a) => self.flip_selection(usize::from(a.min(2))),
            Command::ReverseFaces => self.apply(Action::ReverseFaces),
            Command::OrientFaces => self.apply(Action::OrientFaces),
            Command::Divide => self.apply(Action::DivideTool),
            Command::AlignView => {
                if let Some(f) = self.single_face() {
                    self.align_view(f);
                }
            }
            Command::ZoomSelection => self.zoom_selection(),
            Command::ZoomExtents => self.apply(Action::ZoomExtents),
            Command::Paste => self.apply(Action::Paste),
            Command::MakeGroup => self.make_group(DefinitionKind::Group),
            Command::MakeComponent => self.make_group(DefinitionKind::Component),
            Command::Explode => {
                if let Some(i) = self.single_instance() {
                    self.explode(i);
                }
            }
            Command::MakeUnique => {
                if let Some(i) = self.single_instance() {
                    self.instance_op("Make Unique", |d| d.make_unique(ctx, i).map(|_| ()));
                }
            }
            Command::Lock => {
                if let Some(i) = self.single_instance() {
                    let locked = self.single_locked();
                    let name = if locked { "Unlock" } else { "Lock" };
                    self.instance_op(name, |d| {
                        active_mut(d, ctx)?.set_instance_locked(i, !locked)
                    });
                    self.select(vec![Pickable::Instance(i)], SelectMode::Replace);
                }
            }
            Command::EditShape => {
                if let Some((def, spec)) = self.selected_shape() {
                    self.set_tool(WorkTool::Draw(DrawKind::Shapes));
                    if let Some(t) = self.draw_tool.as_mut() {
                        t.use_shape(def, spec);
                    }
                }
            }
            Command::EditSweep => {
                if let Some((def, recipe, placed)) = self.selected_sweep() {
                    // Each reopens in the tool that made it.
                    let kind = match recipe {
                        // Bare edges: Extrude Edges along Path.
                        smith_model::SweepRecipe::Loft { edges: Some(_), .. }
                        | smith_model::SweepRecipe::Sweep {
                            section: smith_model::SweepSection::Edges { .. },
                            ..
                        } => DrawKind::ExtrudeEdges,
                        smith_model::SweepRecipe::Loft { path: Some(_), .. } => DrawKind::Loft,
                        smith_model::SweepRecipe::Loft { path: None, .. } => DrawKind::LoftSpline,
                        smith_model::SweepRecipe::Revolve { .. } => DrawKind::Revolve,
                        smith_model::SweepRecipe::Sweep { .. } => DrawKind::Sweep,
                        smith_model::SweepRecipe::Skin { .. } => DrawKind::Skin,
                    };
                    self.set_tool(WorkTool::Draw(kind));
                    if let Some(t) = self.draw_tool.as_mut() {
                        t.use_sweep(def, recipe, placed);
                    }
                }
            }
            Command::ResetScale => {
                if let Some(i) = self.single_instance() {
                    self.instance_op("Reset Scale", |d| {
                        smith_ops::organize::reset_scale(d, ctx, i)
                    });
                }
            }
            Command::ChangeAxes => {
                if let Some(i) = self.single_instance() {
                    let a = self.axes_ctx;
                    let frame = DMat4::from_cols(
                        a.x.extend(0.0),
                        a.y.extend(0.0),
                        a.z.extend(0.0),
                        a.origin.extend(1.0),
                    );
                    self.instance_op("Change Axes", |d| {
                        smith_ops::organize::change_axes(d, ctx, i, frame)
                    });
                }
            }
        }
    }

    /// Looks straight at a face in parallel projection, framing it.
    fn align_view(&mut self, f: smith_model::FaceId) {
        let root = self.active();
        let Some(face) = root.face(f) else { return };
        let normal = face.plane().normal();
        let pts = root.loop_points(face.outer());
        let Some(first) = pts.first().copied() else {
            return;
        };
        let bounds = pts
            .iter()
            .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        self.camera.look_from(normal);
        self.camera.projection = Projection::Parallel;
        let aspect = self.aspect();
        self.camera.zoom_extents(bounds, aspect);
    }

    // ----- view pie ------------------------------------------------------------

    /// Which pie choice the cursor points at, if it is outside the dead zone.
    fn pie_choice(centre: Pos2, cursor: Pos2) -> Option<usize> {
        let d = cursor - centre;
        if d.length() < PIE_DEAD {
            return None;
        }
        let angle = (-d.y).atan2(d.x).to_degrees().rem_euclid(360.0);
        Some((((angle + 22.5) / 45.0).floor() as usize) % PIE.len())
    }

    /// Opens, drives and closes the view pie. Returns `true` while it is open,
    /// so the canvas ignores other input.
    fn pie_input(&mut self, ui: &Ui) -> bool {
        if std::mem::take(&mut self.pie_requested) {
            let at = ui
                .input(|i| i.pointer.hover_pos())
                .filter(|p| self.rect.contains(*p));
            self.pie = Some(at.unwrap_or_else(|| self.rect.center()));
        }
        let Some(centre) = self.pie else { return false };
        let cursor = ui.input(|i| i.pointer.hover_pos()).unwrap_or(centre);
        let choice = Self::pie_choice(centre, cursor);
        let clicked = ui.input(|i| i.pointer.primary_clicked());
        let released = ui.input(|i| i.key_released(Key::Backtick));
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.pie = None;
        } else if clicked || (released && choice.is_some()) {
            self.pie = None;
            if let Some(i) = choice {
                self.apply(PIE[i].2);
            }
        }
        true
    }

    fn draw_pie(&self, ui: &Ui) {
        let Some(centre) = self.pie else { return };
        let painter = ui.painter();
        let cursor = ui.input(|i| i.pointer.hover_pos()).unwrap_or(centre);
        let choice = Self::pie_choice(centre, cursor);
        painter.circle_stroke(centre, PIE_DEAD, Stroke::new(1.0, Color32::from_gray(120)));
        let font = egui::FontId::proportional(14.0);
        for (i, (label, deg, _)) in PIE.iter().enumerate() {
            let a = deg.to_radians();
            let at = centre + egui::vec2(a.cos(), -a.sin()) * PIE_RADIUS;
            let galley = painter.layout_no_wrap((*label).to_owned(), font.clone(), Color32::BLACK);
            let box_rect = Rect::from_center_size(at, galley.size() + egui::vec2(18.0, 10.0));
            let selected = choice == Some(i);
            let fill = if selected {
                Color32::from_rgb(60, 110, 230)
            } else {
                Color32::from_gray(248)
            };
            let text = if selected {
                Color32::WHITE
            } else {
                Color32::BLACK
            };
            painter.rect_filled(box_rect, 6.0, fill);
            painter.rect_stroke(
                box_rect,
                6.0,
                Stroke::new(1.0, Color32::from_gray(110)),
                egui::StrokeKind::Inside,
            );
            painter.text(at, egui::Align2::CENTER_CENTER, *label, font.clone(), text);
        }
        if let Some(i) = choice {
            let a = PIE[i].1.to_radians();
            painter.line_segment(
                [
                    centre,
                    centre + egui::vec2(a.cos(), -a.sin()) * (PIE_RADIUS - 30.0),
                ],
                Stroke::new(2.0, Color32::from_rgb(60, 110, 230)),
            );
        }
    }

    // ----- typing ------------------------------------------------------------------

    /// Collects typed text for the measurement box. Returns `true` while typing,
    /// so tool shortcuts are suspended.
    fn handle_typing(&mut self, ui: &Ui, prefs: &ViewPrefs<'_>) -> bool {
        if ui.ctx().egui_wants_keyboard_input() || self.mode != Mode::Edit {
            return false;
        }
        let starts = |c: char| c.is_ascii_digit() || ".-[<*/(".contains(c);
        let texts: Vec<String> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(t) => Some(t.clone()),
                    _ => None,
                })
                .collect()
        });
        for t in texts {
            if self.typed.is_empty() && !t.chars().next().is_some_and(starts) {
                continue;
            }
            self.typed.push_str(&t);
        }
        if self.typed.is_empty() {
            // Enter with nothing typed finishes an operation in progress (the
            // Bézier Spline, a loft's picked profiles), or accepts the shape
            // in the Operation Inputs.
            if self.draw_tool.is_some()
                && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
                && !self.apply_pending(prefs.units)
            {
                let busy = self
                    .draw_tool
                    .as_ref()
                    .is_some_and(|t| t.last_point().is_some() || t.wants_finish());
                if busy {
                    self.tool_input(Input::Finish);
                } else {
                    self.accept_last();
                }
            }
            return false;
        }
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::Backspace) {
                self.typed.pop();
            }
        });
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            let text = std::mem::take(&mut self.typed);
            self.apply_pending(prefs.units);
            self.submit_typed(&text, prefs);
        }
        true
    }

    /// Applies a measurement-box entry to the active tool.
    ///
    /// When the box shows a count (segments or sides), a bare whole number is
    /// that count, as in SketchUp: `48` sets 48 segments, not a 48 mm length.
    fn submit_typed(&mut self, text: &str, prefs: &ViewPrefs<'_>) {
        // Words for a tool that takes them (Text), as typed.
        if self.draw_tool.as_ref().is_some_and(|t| t.wants_text()) {
            let pixel = self.inference.map_or(0.0, |inf| self.pixel_at(inf.point));
            if let Some(t) = self.draw_tool.as_mut() {
                let ctx = Ctx {
                    entities: ctx_of(&self.document, self.ctx),
                    axes: &self.axes_ctx,
                    instances: &self.instance_boxes,
                    instance_lines: &self.instance_lines,
                    pixel,
                    cursor: self.cursor,
                };
                let outcome = t.text(text, &ctx);
                self.run_outcome(outcome);
            }
            return;
        }
        // A plain number typed for a Number field (turns) goes straight to it.
        let primary = self.draw_tool.as_ref().and_then(|t| {
            t.fields()
                .get(t.primary_field())
                .map(|f| (t.primary_field(), f.kind))
        });
        if let Some((index, FieldKind::Number)) = primary {
            self.submit_field(index, FieldKind::Number, text, prefs.units);
            return;
        }
        let shows_count = matches!(primary, Some((_, FieldKind::Count)));
        let bare = text.trim();
        let count = (shows_count && !bare.is_empty() && bare.chars().all(|c| c.is_ascii_digit()))
            .then(|| bare.parse::<u32>().ok())
            .flatten();
        let parsed = match count {
            Some(n) => Ok(TypedValue::Segments(n)),
            None => parse_typed(text, prefs.units.unit),
        };
        let value = match parsed {
            Ok(v) => v,
            Err(e) => {
                self.message = Some(format!("Couldn't read “{text}”: {e}"));
                return;
            }
        };
        let pixel = self.inference.map_or(0.0, |inf| self.pixel_at(inf.point));
        let Some(t) = self.draw_tool.as_mut() else {
            self.message = Some(format!("Read “{text}” as {}", describe(value, prefs.units)));
            return;
        };
        let ctx = Ctx {
            entities: ctx_of(&self.document, self.ctx),
            axes: &self.axes_ctx,
            instances: &self.instance_boxes,
            instance_lines: &self.instance_lines,
            pixel,
            cursor: self.cursor,
        };
        let outcome = t.handle(Input::Typed(value), &ctx);
        self.arrow_lock = None;
        self.tool_axis = None;
        self.tracked.clear();
        self.run_outcome(outcome);
    }

    // ----- inference -----------------------------------------------

    fn handle_inference(&mut self, ui: &Ui, response: &egui::Response, prefs: &ViewPrefs<'_>) {
        let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        if !self.rect.contains(pos) {
            return;
        }
        let last = self.last_point();

        // Arrow-key locks (Right red, Left green, Up blue, Down parallel).
        for (key, axis) in [
            (Key::ArrowRight, Some(Axis::Red)),
            (Key::ArrowLeft, Some(Axis::Green)),
            (Key::ArrowUp, Some(Axis::Blue)),
            (Key::ArrowDown, None),
        ] {
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                // A tool may take the key as a plane choice (Rotate's
                // protractor); otherwise it locks a direction.
                let toggled = if self.tool_axis == axis { None } else { axis };
                let dir = match toggled {
                    Some(Axis::Red) => Some(self.axes_ctx.x),
                    Some(Axis::Green) => Some(self.axes_ctx.y),
                    Some(Axis::Blue) => Some(self.axes_ctx.z),
                    _ => None,
                };
                if self
                    .draw_tool
                    .as_mut()
                    .is_some_and(|t| t.use_axis_lock(dir))
                {
                    self.tool_axis = toggled;
                    continue;
                }
                self.arrow_lock = if self.arrow_lock == Some(axis) {
                    None
                } else {
                    Some(axis)
                };
            }
        }
        let shift = ui.input(|i| i.modifiers.shift);
        let only = ui.input(|i| i.modifiers.alt).then_some(prefs.snap_only);

        // Locks: an arrow lock wins; Shift holds the inference shown when pressed.
        self.lock = match (self.arrow_lock, last) {
            (Some(Some(axis)), Some(l)) => axis_lock(axis, l, &self.axes),
            (Some(None), Some(l)) => self
                .reference_edge
                .and_then(|e| self.active().edge_points(e))
                .and_then(|(a, b)| (b - a).try_normalize())
                .map_or(Lock::None, |dir| Lock::Line {
                    origin: l,
                    dir,
                    kind: Kind::Parallel,
                }),
            _ if shift => match (self.lock, self.inference) {
                (Lock::None, Some(inf)) => {
                    let plane = inf.entity.and_then(|p| match p {
                        Pickable::Face(f) => self.active().face(f).map(|f| *f.plane()),
                        _ => inf
                            .normal
                            .and_then(|n| Plane::from_point_normal(inf.point, n).ok()),
                    });
                    lock_for(&inf, last, self.active(), plane)
                }
                (held, _) => held,
            },
            _ => Lock::None,
        };
        // A tool that holds the cursor to a line (Extrude) wins.
        if let Some(l) = self.draw_tool.as_ref().and_then(|t| t.lock()) {
            self.lock = l;
        }

        let aspect = self.aspect();
        let (origin, dir) = self.ray(self.ndc(pos), aspect);
        let fallback = self
            .local
            .inverse()
            .transform_point3(self.camera.point_on_target_plane(self.ndc(pos), aspect));
        let to_screen = |p: DVec3| self.to_screen(p);
        let tangent = self.draw_tool.as_ref().and_then(|t| {
            t.tangent(&Ctx {
                entities: ctx_of(&self.document, self.ctx),
                axes: &self.axes_ctx,
                instances: &self.instance_boxes,
                instance_lines: &self.instance_lines,
                pixel: 0.0,
                cursor: self.cursor,
            })
        });
        // Centres of the groups and components shown in this context.
        let box_centres: Vec<(DVec3, Pickable)> = self
            .instance_boxes
            .iter()
            .filter(|(id, _)| {
                self.active()
                    .instance(**id)
                    .is_some_and(|i| !i.hidden && self.document.layer_visible(i.layer))
            })
            .map(|(id, c)| {
                (
                    c.iter().copied().sum::<DVec3>() / 8.0,
                    Pickable::Instance(*id),
                )
            })
            .collect();
        let q = Query {
            entities: ctx_snap(&self.document, self.ctx, self.snap.as_ref()),
            bvh: &self.bvh,
            origin,
            dir,
            cursor: DVec2::new(f64::from(pos.x), f64::from(pos.y)),
            to_screen: &to_screen,
            last,
            axes: &self.axes_ctx,
            reference_edge: self.reference_edge,
            tracked: &self.tracked,
            tangent,
            lock: self.lock,
            filter: prefs.snaps,
            only,
            fallback,
            box_centres: &box_centres,
            guides: self.show_guides,
        };
        let mut inf = infer(&q);
        // A snap onto a group's geometry names the group, and carries the
        // face's normal so tools can still turn or lock to it.
        if let Some(e) = inf.entity
            && let Some(s) = self.snap.as_ref()
            && !matches!(e, Pickable::Instance(_))
        {
            let outside = s.is_outside(e);
            let owner = s.owner(e);
            if (outside || owner.is_some())
                && let Pickable::Face(f) = e
            {
                inf.normal = s.entities.face(f).map(|x| x.plane().normal());
            }
            // Outside the group being edited: a place to snap to, nothing
            // to take or cut.
            if outside {
                inf.entity = None;
            } else if let Some(o) = owner {
                inf.entity = Some(Pickable::Instance(o));
            }
        }
        // The inference shown on the previous frame: where the button went down
        // when a drag has just started.
        let before = self.inference;

        // Remember the last hovered edge for parallel/perpendicular.
        if let (Kind::OnEdge | Kind::Midpoint | Kind::Endpoint, Some(Pickable::Edge(e))) =
            (inf.kind, inf.entity)
        {
            self.reference_edge = Some(e);
        }
        // Dwell tracking on point snaps.
        let now = ui.input(|i| i.time);
        let is_point = matches!(
            inf.kind,
            Kind::Endpoint
                | Kind::Midpoint
                | Kind::Intersection
                | Kind::Centre
                | Kind::Quadrant
                | Kind::FaceCentre
                | Kind::BoxCentre
                | Kind::PerpendicularPoint
                | Kind::TangentPoint
        );
        self.dwell = match self.dwell {
            Some((p, since)) if is_point && p == inf.point => {
                if now - since >= TRACK_DWELL && !self.tracked.contains(&p) && Some(p) != last {
                    self.tracked.push(p);
                    if self.tracked.len() > MAX_TRACKED {
                        self.tracked.remove(0);
                    }
                }
                Some((p, since))
            }
            _ if is_point => Some((inf.point, now)),
            _ => None,
        };
        self.inference = Some(inf);

        // A click places a point, and so does each end of a press–drag–release,
        // as in SketchUp. (egui reports a press with even slight movement as a
        // drag, so clicks alone would miss many real clicks.)
        let free = self.nav == NavTool::None && !self.paused();
        let placed = if !free {
            None
        } else if response.clicked_by(PointerButton::Primary)
            || response.drag_stopped_by(PointerButton::Primary)
        {
            Some(inf)
        } else if response.drag_started_by(PointerButton::Primary) {
            Some(before.unwrap_or(inf))
        } else {
            None
        };
        // A snap to a point names no entity. Move, Rotate, Scale and Mirror
        // with nothing selected take what is under the cursor, as SketchUp
        // does (a group's corner takes the group).
        let takes_items = self
            .draw_tool
            .as_ref()
            .is_some_and(|t| matches!(t.name(), "Move" | "Rotate" | "Scale" | "Mirror"));
        let fusing = self.draw_tool.as_ref().is_some_and(|t| t.name() == "Fuse");
        let placed = placed.map(|mut p| {
            if p.entity.is_none() && takes_items && self.selection.is_empty() {
                p.entity = self.pick_at(pos);
            }
            // Fuse takes the face (or group) under the cursor, not the
            // edge or point an inference snapped to.
            if fusing {
                p.entity = self.face_or_group_at(pos);
            }
            p
        });
        if let Some(at) = ui.input(|i| i.pointer.hover_pos()) {
            let r = self.rect;
            self.cursor = DVec2::new(
                f64::from((at.x - r.left()) / r.width().max(1.0)),
                f64::from((at.y - r.top()) / r.height().max(1.0)),
            );
        }
        // The Section Plane tool turns a plane on or off by its outline; the
        // Eraser takes a plane under the cursor.
        let tool_name = self.draw_tool.as_ref().map(|t| t.name());
        let placed = match (placed, tool_name) {
            (Some(_), Some(name @ ("Section Plane" | "Eraser"))) if self.show_sections => {
                match self.section_at(pos) {
                    Some((id, active)) => {
                        let op = if name == "Eraser" {
                            DrawOp::EraseSection { section: id }
                        } else if active {
                            DrawOp::ActivateSection { section: None }
                        } else {
                            DrawOp::ActivateSection { section: Some(id) }
                        };
                        self.run_now(&op);
                        None
                    }
                    None => placed,
                }
            }
            (p, _) => p,
        };
        // The Eraser takes a dimension or text under the cursor first.
        let placed = match placed {
            Some(_)
                if self
                    .draw_tool
                    .as_ref()
                    .is_some_and(|t| t.name() == "Eraser") =>
            {
                match self.annotation_at(pos) {
                    Some(id) => {
                        self.run_now(&DrawOp::EraseAnnotation { annotation: id });
                        None
                    }
                    None => placed,
                }
            }
            _ => placed,
        };
        let pixel = self.pixel_at(inf.point);
        let view = smith_ops::cut::View {
            eye: self.camera.eye(),
            forward: self.camera.forward(),
            parallel: self.camera.projection == smith_render::camera::Projection::Parallel,
        };
        if let Some(t) = self.draw_tool.as_mut() {
            t.use_view(view);
            let ctx = Ctx {
                entities: ctx_of(&self.document, self.ctx),
                axes: &self.axes_ctx,
                instances: &self.instance_boxes,
                instance_lines: &self.instance_lines,
                pixel,
                cursor: self.cursor,
            };
            t.handle(Input::Hover(inf), &ctx);
            if let Some(at) = placed {
                let outcome = t.handle(Input::Click(at), &ctx);
                self.arrow_lock = None;
                self.tool_axis = None;
                self.tracked.clear();
                self.run_outcome(outcome);
                // A drag-move with the Select tool ends with the drag.
                if response.double_clicked_by(PointerButton::Primary)
                    && let Some(t) = self.draw_tool.as_mut()
                {
                    let ctx = Ctx {
                        entities: ctx_of(&self.document, self.ctx),
                        axes: &self.axes_ctx,
                        instances: &self.instance_boxes,
                        instance_lines: &self.instance_lines,
                        pixel,
                        cursor: self.cursor,
                    };
                    let repeat = t.repeat(&ctx);
                    self.run_outcome(repeat);
                }
                if self.drag_move
                    && self
                        .draw_tool
                        .as_ref()
                        .is_none_or(|t| t.last_point().is_none())
                {
                    self.drag_move = false;
                    self.set_tool(WorkTool::Select);
                }
            }
        } else if let Some(at) = placed {
            self.click_point(at.point);
        }
        // Keep repainting while dwelling so tracking triggers without mouse motion.
        if self.dwell.is_some() {
            ui.ctx().request_repaint();
        }
    }

    /// The previous point of the operation in progress, for inference.
    fn last_point(&self) -> Option<DVec3> {
        match self.tool {
            WorkTool::Draw(_) => self.draw_tool.as_ref().and_then(|t| t.last_point()),
            WorkTool::Axes { origin, red } => red.or(origin),
            WorkTool::Select
            | WorkTool::EditCurve
            | WorkTool::Paint
            | WorkTool::MarkEdges
            | WorkTool::LineStyle => None,
        }
    }

    /// A left click with the Axes tool.
    fn click_point(&mut self, p: DVec3) {
        self.arrow_lock = None;
        self.tool_axis = None;
        match self.tool {
            WorkTool::Axes { origin: None, .. } => {
                self.tool = WorkTool::Axes {
                    origin: Some(p),
                    red: None,
                };
                self.message = Some("Axes: click the red direction.".into());
            }
            WorkTool::Axes {
                origin: Some(o),
                red: None,
            } => {
                self.tool = WorkTool::Axes {
                    origin: Some(o),
                    red: Some(p),
                };
                self.message = Some("Axes: click toward the green direction.".into());
            }
            WorkTool::Axes {
                origin: Some(o),
                red: Some(r),
            } => match DrawingAxes::from_points(o, r, p) {
                Some(axes) => {
                    self.set_axes(axes);
                    self.set_tool(WorkTool::Select);
                    self.message = Some("Drawing axes placed.".into());
                }
                None => self.message = Some("Those points don't define axes; try again.".into()),
            },
            WorkTool::Draw(_)
            | WorkTool::Select
            | WorkTool::EditCurve
            | WorkTool::Paint
            | WorkTool::MarkEdges
            | WorkTool::LineStyle => {}
        }
    }

    fn draw_inference(&self, ui: &Ui) {
        let Some(inf) = self.inference else { return };
        if self.tool == WorkTool::Select {
            return;
        }
        let painter = ui.painter();
        let Some(s) = self.to_screen(inf.point) else {
            return;
        };
        let at = |v: DVec2| pos2(v.x as f32, v.y as f32);
        let pt = at(s);
        let axis_color = |k: Kind| match k {
            Kind::OnAxis(Axis::Red) => Color32::from_rgb(210, 20, 20),
            Kind::OnAxis(Axis::Green) => Color32::from_rgb(20, 140, 30),
            Kind::OnAxis(Axis::Blue) => Color32::from_rgb(20, 40, 210),
            Kind::Parallel | Kind::Perpendicular | Kind::PerpendicularPoint => {
                Color32::from_rgb(200, 0, 180)
            }
            Kind::Tangent | Kind::TangentPoint => TANGENT_COLOR,
            _ => Color32::from_gray(30),
        };
        let width = if inf.locked { 2.5 } else { 1.5 };

        let last = self.last_point();
        let mut note = None;
        if let Some(t) = self.draw_tool.as_ref() {
            // The draw tool's preview: the shape outline, and the rubber band in
            // the inference colour.
            let ctx = Ctx {
                entities: ctx_of(&self.document, self.ctx),
                axes: &self.axes_ctx,
                instances: &self.instance_boxes,
                instance_lines: &self.instance_lines,
                pixel: self.pixel_at(inf.point),
                cursor: self.cursor,
            };
            let Preview {
                lines,
                band,
                note: n,
            } = t.preview(&ctx);
            note = n;
            // Red when the tool warns the result will go wrong.
            let colour = if t.warns() { WARN_COLOR } else { PREVIEW_COLOR };
            for [a, b] in lines {
                if let (Some(sa), Some(sb)) = (self.to_screen(a), self.to_screen(b)) {
                    painter.line_segment([at(sa), at(sb)], Stroke::new(1.5, colour));
                }
            }
            if let Some([a, b]) = band
                && let (Some(sa), Some(sb)) = (self.to_screen(a), self.to_screen(b))
            {
                painter.line_segment([at(sa), at(sb)], Stroke::new(width, axis_color(inf.kind)));
            }
        } else if let Some(l) = last.and_then(|l| self.to_screen(l)) {
            // Axes tool: rubber band from the origin.
            painter.line_segment([at(l), pt], Stroke::new(width, axis_color(inf.kind)));
        }
        // Dotted guides from tracked points (two for a From Points crossing).
        for from in [inf.from, inf.from2]
            .into_iter()
            .flatten()
            .filter(|f| Some(*f) != last)
            .filter_map(|f| self.to_screen(f))
        {
            painter.extend(egui::Shape::dotted_line(
                &[at(from), pt],
                axis_color(inf.kind),
                6.0,
                1.2,
            ));
        }
        for t in &self.tracked {
            if let Some(ts) = self.to_screen(*t) {
                painter.circle_stroke(at(ts), 6.0, Stroke::new(1.0, Color32::from_gray(90)));
            }
        }

        // Marker shape and colour by kind, tuned for a white background.
        let ring = |c: Color32| {
            painter.circle_filled(pt, 5.0, c);
            painter.circle_stroke(pt, 5.0, Stroke::new(1.0, Color32::BLACK));
        };
        match inf.kind {
            Kind::Endpoint => ring(Color32::from_rgb(30, 180, 40)),
            Kind::Midpoint => ring(Color32::from_rgb(0, 190, 210)),
            Kind::Intersection => {
                let d = 5.0;
                let st = Stroke::new(2.0, Color32::BLACK);
                painter.line_segment([pt + egui::vec2(-d, -d), pt + egui::vec2(d, d)], st);
                painter.line_segment([pt + egui::vec2(-d, d), pt + egui::vec2(d, -d)], st);
            }
            Kind::Centre => {
                painter.circle_stroke(pt, 6.0, Stroke::new(2.0, Color32::from_rgb(170, 0, 170)));
                painter.circle_filled(pt, 2.0, Color32::from_rgb(170, 0, 170));
            }
            Kind::Quadrant => {
                let d = 5.0;
                let pts = vec![
                    pt + egui::vec2(0.0, -d),
                    pt + egui::vec2(d, 0.0),
                    pt + egui::vec2(0.0, d),
                    pt + egui::vec2(-d, 0.0),
                ];
                painter.add(egui::Shape::convex_polygon(
                    pts,
                    Color32::from_rgb(240, 140, 0),
                    Stroke::new(1.0, Color32::BLACK),
                ));
            }
            Kind::FaceCentre => {
                painter.rect_filled(
                    Rect::from_center_size(pt, egui::vec2(8.0, 8.0)),
                    0.0,
                    Color32::from_rgb(40, 80, 230),
                );
            }
            Kind::BoxCentre => {
                painter.rect_stroke(
                    Rect::from_center_size(pt, egui::vec2(10.0, 10.0)),
                    0.0,
                    Stroke::new(2.0, Color32::from_rgb(40, 80, 230)),
                    egui::StrokeKind::Middle,
                );
            }
            Kind::OnEdge => ring(Color32::from_rgb(220, 30, 30)),
            Kind::OnGuide => ring(GUIDE_COLOR),
            Kind::GuidePoint => {
                painter.circle_stroke(pt, 6.0, Stroke::new(2.0, GUIDE_COLOR));
                painter.extend(guide_cross(pt, 4.0, Stroke::new(1.5, GUIDE_COLOR)));
            }
            Kind::OnFace => {
                painter.circle_filled(pt, 3.5, Color32::from_rgb(40, 80, 230));
            }
            Kind::PerpendicularPoint => {
                painter.rect_stroke(
                    Rect::from_center_size(pt, egui::vec2(9.0, 9.0)),
                    0.0,
                    Stroke::new(2.0, axis_color(inf.kind)),
                    egui::StrokeKind::Middle,
                );
            }
            Kind::TangentPoint => {
                painter.circle_stroke(pt, 5.0, Stroke::new(2.0, TANGENT_COLOR));
                painter.circle_filled(pt, 2.0, TANGENT_COLOR);
            }
            Kind::Crossing => {
                painter.circle_stroke(pt, 5.0, Stroke::new(1.5, Color32::from_gray(30)));
                painter.circle_filled(pt, 2.0, Color32::from_gray(30));
            }
            Kind::OnAxis(_) | Kind::Parallel | Kind::Perpendicular | Kind::Tangent => {
                painter.circle_filled(pt, 4.0, axis_color(inf.kind));
            }
            Kind::Free => {
                painter.circle_stroke(pt, 3.0, Stroke::new(1.0, Color32::from_gray(60)));
            }
        }
        let mut label = note.map_or_else(|| inf.kind.label(), str::to_owned);
        if inf.locked && !label.is_empty() {
            label.push_str(" (locked)");
        }
        if !label.is_empty() {
            let galley_pos = pt + egui::vec2(12.0, 10.0);
            let font = egui::FontId::proportional(12.0);
            let rect = painter.text(
                galley_pos,
                egui::Align2::LEFT_TOP,
                &label,
                font.clone(),
                Color32::BLACK,
            );
            painter.rect_filled(
                rect.expand(3.0),
                3.0,
                Color32::from_rgba_unmultiplied(255, 255, 230, 235),
            );
            painter.text(
                galley_pos,
                egui::Align2::LEFT_TOP,
                &label,
                font,
                Color32::BLACK,
            );
        }
    }

    // ----- drawing -------------------------------------------------------------

    /// Each dimension and text of the context being edited as drawn on
    /// screen: its lines, and its label (centre and words).
    fn annotation_shapes(&self) -> Vec<AnnotationShape> {
        let e = self.active();
        let scr = |p: DVec3| self.to_screen(p).map(|s| pos2(s.x as f32, s.y as f32));
        let u = &self.units;
        let mut out = Vec::new();
        for (id, a) in e.annotations() {
            let mut lines: Vec<[Pos2; 2]> = Vec::new();
            let seg = |p: DVec3, q: DVec3, lines: &mut Vec<[Pos2; 2]>| {
                if let (Some(a), Some(b)) = (scr(p), scr(q)) {
                    lines.push([a, b]);
                }
            };
            let label = match a {
                Annotation::Linear { a, b, offset } => {
                    let (pa, pb) = (a.position(e), b.position(e));
                    let (qa, qb) = (pa + *offset, pb + *offset);
                    seg(pa, qa, &mut lines);
                    seg(pb, qb, &mut lines);
                    seg(qa, qb, &mut lines);
                    // Arrowheads, drawn on screen.
                    if let (Some(sa), Some(sb)) = (scr(qa), scr(qb)) {
                        arrowhead(sa, sb, self.overlay_scale, &mut lines);
                        arrowhead(sb, sa, self.overlay_scale, &mut lines);
                    }
                    scr((qa + qb) * 0.5).map(|c| (c, format_length(pa.distance(pb), u)))
                }
                Annotation::Radial {
                    curve,
                    diameter,
                    centre,
                    radius,
                    label,
                } => {
                    let (c, r, n) = curve
                        .and_then(|k| e.curve(k))
                        .and_then(|rec| match rec.curve() {
                            smith_geom::curve::Curve::Circle(x) => {
                                Some((x.frame().center(), x.radius(), x.frame().normal()))
                            }
                            smith_geom::curve::Curve::Arc(x) => {
                                Some((x.frame().center(), x.radius(), x.frame().normal()))
                            }
                            _ => None,
                        })
                        .unwrap_or((*centre, *radius, DVec3::Z));
                    let l = c + *label;
                    let d = (*label - n * label.dot(n)).normalize_or(DVec3::X);
                    let rim = c + d * r;
                    let start = if *diameter { c - d * r } else { c };
                    seg(start, rim, &mut lines);
                    seg(rim, l, &mut lines);
                    if let (Some(sr), Some(ss)) = (scr(rim), scr(start)) {
                        arrowhead(sr, ss, self.overlay_scale, &mut lines);
                    }
                    let text = if *diameter {
                        format!("Ø {}", format_length(2.0 * r, u))
                    } else {
                        format!("R {}", format_length(r, u))
                    };
                    scr(l).map(|p| (p, text))
                }
                Annotation::Angular {
                    centre,
                    a,
                    b,
                    radius,
                } => {
                    let c = centre.position(e);
                    let (da, db) = (a.position(e) - c, b.position(e) - c);
                    let n = da.cross(db);
                    match (da.try_normalize(), db.try_normalize(), n.try_normalize()) {
                        (Some(ua), Some(ub), Some(n)) => {
                            let angle = ua.cross(ub).dot(n).atan2(ua.dot(ub));
                            let steps = 24;
                            let at =
                                |t: f64| c + DMat3::from_axis_angle(n, angle * t) * ua * *radius;
                            for i in 0..steps {
                                let (t0, t1) =
                                    (i as f64 / steps as f64, (i + 1) as f64 / steps as f64);
                                seg(at(t0), at(t1), &mut lines);
                            }
                            seg(c, c + ua * *radius, &mut lines);
                            seg(c, c + ub * *radius, &mut lines);
                            scr(at(0.5)).map(|p| (p, format_angle(angle.to_degrees())))
                        }
                        _ => None,
                    }
                }
                Annotation::Leader {
                    anchor,
                    label,
                    text,
                } => {
                    // The text stays where it was placed; the leader
                    // stretches to the anchor.
                    let p = anchor.position(e);
                    let l = anchor.fallback() + *label;
                    seg(p, l, &mut lines);
                    if let (Some(sp), Some(sl)) = (scr(p), scr(l)) {
                        arrowhead(sp, sl, self.overlay_scale, &mut lines);
                    }
                    let words = match text {
                        LeaderText::Custom(t) => t.clone(),
                        LeaderText::Length { edge, value } => format_length(
                            edge.and_then(|k| e.edge_points(k))
                                .map_or(*value, |(a, b)| a.distance(b)),
                            u,
                        ),
                        LeaderText::Area { face, value } => {
                            format_area(face.and_then(|f| e.face_area(f)).unwrap_or(*value), u)
                        }
                        LeaderText::Coordinates => format!(
                            "{}, {}, {}",
                            format_length(p.x, u),
                            format_length(p.y, u),
                            format_length(p.z, u)
                        ),
                    };
                    scr(l).map(|q| (q, words))
                }
                Annotation::Screen { at, text } => {
                    let r = self.rect;
                    Some((
                        pos2(
                            r.left() + at[0] as f32 * r.width(),
                            r.top() + at[1] as f32 * r.height(),
                        ),
                        text.clone(),
                    ))
                }
            };
            out.push((id, lines, label));
        }
        out
    }

    /// The dimensions and text of the context being edited: thin dark lines
    /// with arrowheads, and labels on a light ground.
    fn draw_annotations(&self, ui: &Ui) {
        ui.painter()
            .with_clip_rect(self.rect)
            .extend(self.annotation_paint(ui.ctx()));
    }

    /// [`Self::draw_annotations`]'s shapes.
    fn annotation_paint(&self, ctx: &egui::Context) -> Vec<egui::Shape> {
        let k = self.overlay_scale;
        let stroke = Stroke::new(k, ANNOTATION_COLOR);
        let mut out = Vec::new();
        for (_, lines, label) in self.annotation_shapes() {
            for l in lines {
                out.push(egui::Shape::line_segment(l, stroke));
            }
            if let Some((at, words)) = label {
                let galley = ctx.fonts_mut(|f| {
                    f.layout_no_wrap(
                        words,
                        egui::FontId::proportional(12.0 * k),
                        ANNOTATION_COLOR,
                    )
                });
                let rect = egui::Align2::CENTER_CENTER.anchor_size(at, galley.size());
                out.push(egui::Shape::rect_filled(
                    rect.expand(2.0 * k),
                    2.0 * k,
                    Color32::from_white_alpha(220),
                ));
                out.push(egui::Shape::galley(rect.min, galley, ANNOTATION_COLOR));
            }
        }
        out
    }

    /// The dimension or text under screen position `at`: within 5 points of
    /// one of its lines, or on its label.
    fn annotation_at(&self, at: Pos2) -> Option<AnnotationId> {
        self.annotation_shapes()
            .into_iter()
            .rev()
            .find(|(_, lines, label)| {
                lines.iter().any(|[a, b]| {
                    let ab = *b - *a;
                    let t = ((at - *a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                    (*a + ab * t).distance(at) <= 5.0
                }) || label.as_ref().is_some_and(|(c, words)| {
                    let half = egui::vec2(3.5 * words.chars().count() as f32 + 4.0, 10.0);
                    egui::Rect::from_center_size(*c, half * 2.0).contains(at)
                })
            })
            .map(|(id, _, _)| id)
    }

    /// Number of guides in the context being edited.
    pub fn guide_count(&self) -> usize {
        self.active().guide_count()
    }

    /// The guides in view: the edited context's own, its groups' and, inside
    /// a group, those outside it — guide lines dashed across the canvas,
    /// guide points as small crosses. Whatever lies behind a face is not
    /// drawn: each guide line is tested every [`GUIDE_STEP_PX`] along its
    /// stretch on screen.
    fn draw_guides(&self, ui: &Ui) {
        ui.painter()
            .with_clip_rect(self.rect)
            .extend(self.guide_paint());
    }

    /// [`Self::draw_guides`]'s shapes.
    fn guide_paint(&self) -> Vec<egui::Shape> {
        let k = self.overlay_scale;
        let stroke = Stroke::new(k, GUIDE_COLOR);
        let seen = ctx_snap(&self.document, self.ctx, self.snap.as_ref());
        let mut out = Vec::new();
        for (_, g) in seen.guides() {
            match *g {
                Guide::Line { point, direction } => {
                    let Some([a, b]) = self.guide_on_screen(point, direction) else {
                        continue;
                    };
                    for run in self.unhidden_runs(a, b, point, direction) {
                        out.extend(egui::Shape::dashed_line(&run, stroke, 6.0 * k, 4.0 * k));
                    }
                }
                Guide::Point { point } => {
                    if let Some(s) = self.to_screen(point) {
                        let at = pos2(s.x as f32, s.y as f32);
                        if !self.behind_face(at, |_, _| point) {
                            out.extend(guide_cross(at, 4.0 * k, Stroke::new(1.5 * k, GUIDE_COLOR)));
                        }
                    }
                }
            }
        }
        out
    }

    /// Whether the point that `on_ray` picks on the cursor ray through
    /// screen position `at` (given the ray's origin and direction, in the
    /// edited context's coordinates) lies behind a face. Within two pixels
    /// of the face counts as in front, so guides lying on faces show.
    fn behind_face(&self, at: Pos2, on_ray: impl Fn(DVec3, DVec3) -> DVec3) -> bool {
        let (o, d) = self.ray(self.ndc(at), self.aspect());
        let Some(hit) = self.bvh.ray_cast(o, d) else {
            return false;
        };
        let t = (on_ray(o, d) - o).dot(d);
        let px = match self.camera.projection {
            Projection::Perspective => {
                2.0 * t.max(0.0) * (self.camera.fov_y * 0.5).tan()
                    / f64::from(self.rect.height()).max(1.0)
            }
            Projection::Parallel => self.camera.world_per_pixel(f64::from(self.rect.height())),
        };
        hit.t < t - 2.0 * px - 1e-6
    }

    /// The stretches of screen segment `a`–`b`, the guide line through
    /// `point` along `direction`, that no face hides.
    fn unhidden_runs(&self, a: Pos2, b: Pos2, point: DVec3, direction: DVec3) -> Vec<Vec<Pos2>> {
        let steps = ((a.distance(b) / GUIDE_STEP_PX).ceil() as usize).clamp(1, 4000);
        let mut runs: Vec<Vec<Pos2>> = Vec::new();
        let mut open = false;
        for i in 0..=steps {
            let at = a.lerp(b, i as f32 / steps as f32);
            let shown = !self.behind_face(at, |o, d| closest_on_line(point, direction, o, d));
            match (shown, open) {
                (true, true) => runs.last_mut().expect("run").push(at),
                (true, false) => runs.push(vec![at]),
                _ => {}
            }
            open = shown;
        }
        runs.retain(|r| r.len() > 1);
        runs
    }

    /// The visible stretch of the endless guide line through `point` along
    /// `direction` (edited-context coordinates), in screen points: the line
    /// is cut where it passes behind the camera's near distance
    /// (perspective), then clipped to the canvas (Liang–Barsky).
    fn guide_on_screen(&self, point: DVec3, direction: DVec3) -> Option<[Pos2; 2]> {
        let d = direction.try_normalize()?;
        // How far either way to reach: well past anything in view.
        let world_p = self.local.transform_point3(point);
        let reach = (self.camera.distance + world_p.distance(self.camera.target)) * 1e3 + 1e4;
        let (mut t0, mut t1) = (-reach, reach);
        if self.camera.projection == Projection::Perspective {
            // Depth along the view direction is affine in t.
            let f = self.camera.forward();
            let near = (self.camera.distance * 1e-3).max(1e-5);
            let a = (world_p - self.camera.eye()).dot(f);
            let b = self.local.transform_vector3(d).dot(f);
            if b.abs() < 1e-12 {
                if a < near {
                    return None;
                }
            } else {
                let at = (near - a) / b;
                if b > 0.0 {
                    t0 = t0.max(at);
                } else {
                    t1 = t1.min(at);
                }
            }
            if t0 >= t1 {
                return None;
            }
        }
        let sa = self.to_screen(point + d * t0)?;
        let sb = self.to_screen(point + d * t1)?;
        let r = self.rect.expand(2.0);
        let (lo, hi) = (
            DVec2::new(f64::from(r.min.x), f64::from(r.min.y)),
            DVec2::new(f64::from(r.max.x), f64::from(r.max.y)),
        );
        let [a, b] = clip_segment(sa, sb, lo, hi)?;
        Some([pos2(a.x as f32, a.y as f32), pos2(b.x as f32, b.y as f32)])
    }

    fn draw_selection_rect(&self, ui: &Ui) {
        let (Some(start), Some(now)) = (self.drag_start, ui.input(|i| i.pointer.hover_pos()))
        else {
            return;
        };
        let rect = Rect::from_two_pos(start, now);
        let painter = ui.painter();
        if now.x < start.x {
            // Crossing: dashed outline, as in SketchUp.
            let c = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
                rect.left_top(),
            ];
            painter.extend(egui::Shape::dashed_line(
                &c,
                Stroke::new(1.0, RECT_COLOR),
                6.0,
                4.0,
            ));
        } else {
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, RECT_COLOR),
                egui::StrokeKind::Inside,
            );
        }
    }

    fn draw(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(rs) = self.render_state.as_ref() else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No GPU renderer available",
                egui::FontId::proportional(14.0),
                Color32::DARK_RED,
            );
            return;
        };
        let renderer = self
            .renderer
            .get_or_insert_with(|| CanvasRenderer::new(&rs.device, &rs.queue));
        if let Some(mesh) = self.pending_mesh.take() {
            renderer.upload(&rs.device, &rs.queue, &mesh, &|image| {
                match smith_io::decode_for_display(image) {
                    Ok(d) => Some(d.levels),
                    Err(e) => {
                        log::warn!("texture {}: {e}", image.name);
                        None
                    }
                }
            });
            self.shown_mesh = Some(mesh);
        }
        let ppp = ui.ctx().pixels_per_point();
        let width = (rect.width() * ppp).round().max(1.0) as u32;
        let height = (rect.height() * ppp).round().max(1.0) as u32;
        let style = render_style(
            self.mode,
            &self.edit_style,
            &self.look,
            self.section_cuts,
            &self.document,
            ppp,
        );
        let out = renderer.render(&rs.device, &rs.queue, &self.camera, &style, width, height);

        let mut egui_renderer = rs.renderer.write();
        let id = match self.texture {
            None => egui_renderer.register_native_texture(
                &rs.device,
                out.view,
                wgpu::FilterMode::Linear,
            ),
            Some(id) => {
                if out.recreated {
                    egui_renderer.update_egui_texture_from_wgpu_texture(
                        &rs.device,
                        out.view,
                        wgpu::FilterMode::Linear,
                        id,
                    );
                }
                id
            }
        };
        drop(egui_renderer);
        self.texture = Some(id);
        ui.painter().image(
            id,
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

/// A display style as a scene records it.
fn face_display(f: FaceStyle) -> FaceDisplay {
    match f {
        FaceStyle::Wireframe => FaceDisplay::Wireframe,
        FaceStyle::HiddenLine => FaceDisplay::HiddenLine,
        FaceStyle::Shaded => FaceDisplay::Shaded,
        FaceStyle::Monochrome => FaceDisplay::Monochrome,
        FaceStyle::XRay => FaceDisplay::XRay,
    }
}

/// The display style a scene records, for the renderer.
fn face_style(f: FaceDisplay) -> FaceStyle {
    match f {
        FaceDisplay::Wireframe => FaceStyle::Wireframe,
        FaceDisplay::HiddenLine => FaceStyle::HiddenLine,
        FaceDisplay::Shaded => FaceStyle::Shaded,
        FaceDisplay::Monochrome => FaceStyle::Monochrome,
        FaceDisplay::XRay => FaceStyle::XRay,
    }
}

/// Adds the cut `plane` makes through the root and its groups to `mesh`:
/// filled regions (face vertices marked [`SECTION_FILL`]) and dark outline
/// edges, so the cut reads as solid.
fn add_section_fill(mesh: &mut SceneMesh, doc: &Document, plane: &SectionPlane) {
    section_fill_walk(mesh, doc, doc.root(), &DMat4::IDENTITY, plane, 0);
}

/// The cut faces and outlines of `plane` (in model coordinates) through
/// `e` placed by `m`, and through the groups inside it.
fn section_fill_walk(
    mesh: &mut SceneMesh,
    doc: &Document,
    e: &Entities,
    m: &DMat4,
    plane: &SectionPlane,
    depth: u32,
) {
    // The plane in this context's coordinates.
    let inv = m.inverse();
    let local = SectionPlane::new(
        inv.transform_point3(plane.point),
        m.transpose().transform_vector3(plane.normal),
    );
    if let Some(local) = local {
        let slice = e.slice(&local, |f| {
            e.face(f)
                .is_some_and(|x| !x.attributes.hidden && doc.layer_visible(x.attributes.layer))
        });
        let o = mesh.origin;
        let n = plane.normal.as_vec3().to_array();
        let rel = |p: DVec3| (m.transform_point3(p) - o).as_vec3().to_array();
        let u = local.normal.any_orthonormal_vector();
        let v = local.normal.cross(u);
        let flat = |p: &DVec3| DVec2::new(p.dot(u), p.dot(v));
        for (outer, holes) in &slice.regions {
            let o2: Vec<DVec2> = outer.iter().map(flat).collect();
            let h2: Vec<Vec<DVec2>> = holes.iter().map(|h| h.iter().map(flat).collect()).collect();
            let all: Vec<DVec3> = outer
                .iter()
                .chain(holes.iter().flatten())
                .copied()
                .collect();
            for tri in smith_geom::triangulate::triangulate_polygon(&o2, &h2) {
                for i in tri {
                    mesh.faces
                        .push(FaceVertex::plain(rel(all[i]), n, SECTION_FILL));
                }
            }
        }
        for [a, b] in &slice.segments {
            for p in [a, b] {
                mesh.edges.push(LineVertex {
                    position: rel(*p),
                    color: smith_render::mesh::EDGE_COLOR,
                    along: 0.0,
                    dashed: 0.0,
                });
            }
        }
    }
    if depth >= 32 {
        return;
    }
    for id in e.instance_ids() {
        if let Some(inst) = e.instance(id)
            && !inst.hidden
            && doc.layer_visible(inst.layer)
            && let Some(g) = doc.definition_entities(inst.definition)
        {
            section_fill_walk(mesh, doc, g, &(*m * inst.transform), plane, depth + 1);
        }
    }
}

/// A dimension or text on screen: its key, its lines, and its label
/// (centre and words).
type AnnotationShape = (AnnotationId, Vec<[Pos2; 2]>, Option<(Pos2, String)>);

/// "`n` thing" or "`n` things", digits grouped in threes ("1 657 Faces").
fn counted(n: usize, one: &str, many: &str) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    format!("{out} {}", if n == 1 { one } else { many })
}

/// The refusal to draw on a locked layer.
const LOCKED_NOT_ACTIVE: &str = "Unlock the Layer to Draw on It";

/// Said when an operation is refused because it would bend faces.
const AUTO_TRI_MESSAGE: &str = "This action will cause your geometry to triangulate.";

/// Colour of dimensions and text.
const ANNOTATION_COLOR: Color32 = Color32::from_gray(40);

/// Two short strokes making an arrowhead at `tip`, pointing away from
/// `from`.
fn arrowhead(tip: Pos2, from: Pos2, size: f32, lines: &mut Vec<[Pos2; 2]>) {
    let Some(d) = (tip - from)
        .normalized()
        .is_finite()
        .then(|| (tip - from).normalized())
    else {
        return;
    };
    if (tip - from).length() < 1.0 {
        return;
    }
    let side = egui::vec2(-d.y, d.x) * size;
    lines.push([tip, tip - d * 8.0 * size + side * 3.0]);
    lines.push([tip, tip - d * 8.0 * size - side * 3.0]);
}

/// An angle in degrees for display: to a tenth, with `~` when rounded.
pub(crate) fn format_angle(deg: f64) -> String {
    let shown = (deg * 10.0).round() / 10.0;
    let text = if shown.fract() == 0.0 {
        format!("{shown:.0}°")
    } else {
        format!("{shown:.1}°")
    };
    if (deg - shown).abs() < 1e-9 {
        text
    } else {
        format!("~ {text}")
    }
}

/// Colour of guides and of their snap markers.
const GUIDE_COLOR: Color32 = Color32::from_gray(95);
/// Spacing, in screen points, of the hidden-behind-a-face test along a
/// guide line.
const GUIDE_STEP_PX: f32 = 4.0;

/// The point of the endless line `p + s·u` (`u` a unit vector) nearest the
/// ray `o + t·d` (`d` a unit vector).
fn closest_on_line(p: DVec3, u: DVec3, o: DVec3, d: DVec3) -> DVec3 {
    let w = p - o;
    let b = u.dot(d);
    let denom = 1.0 - b * b;
    if denom < 1e-12 {
        // Parallel: any point will do; take the one level with the origin.
        return p - u * w.dot(u);
    }
    let s = (b * w.dot(d) - w.dot(u)) / denom;
    p + u * s
}

/// A small × at `at`, `r` points from centre to tip.
/// How the canvas renders in `mode`, with lines `ppp` pixels per point; the
/// root's active section plane cuts while `cuts` is on.
fn render_style(
    mode: Mode,
    edit_style: &RenderStyle,
    look: &Look,
    cuts: bool,
    doc: &Document,
    ppp: f32,
) -> RenderStyle {
    let mut style = match mode {
        Mode::Edit => RenderStyle {
            pixels_per_point: ppp,
            ..*edit_style
        },
        Mode::Present => RenderStyle {
            faces: look.faces,
            back_edges: look.back_edges,
            background: look.background,
            section: None,
            paint: true,
            pixels_per_point: ppp,
        },
    };
    style.section = cuts
        .then(|| doc.root().active_section().map(|(_, p)| p.equation()))
        .flatten();
    style
}

/// Straight alpha from premultiplied, worked in linear light: a picture on
/// a transparent background comes out of the GPU premultiplied (its edges
/// blended with nothing), and PNG wants the colour itself.
fn unpremultiply(px: &mut [u8]) {
    let to_linear = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let to_srgb = |l: f32| {
        let l = l.clamp(0.0, 1.0);
        let c = if l <= 0.003_130_8 {
            12.92 * l
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        (c * 255.0).round() as u8
    };
    for p in px.as_chunks_mut::<4>().0 {
        let a = f32::from(p[3]) / 255.0;
        if a > 0.0 && a < 1.0 {
            for c in &mut p[..3] {
                *c = to_srgb(to_linear(*c) / a);
            }
        }
    }
}

fn guide_cross(at: Pos2, r: f32, stroke: Stroke) -> [egui::Shape; 2] {
    [
        egui::Shape::line_segment([at + egui::vec2(-r, -r), at + egui::vec2(r, r)], stroke),
        egui::Shape::line_segment([at + egui::vec2(-r, r), at + egui::vec2(r, -r)], stroke),
    ]
}

/// Segment `a`–`b` clipped to the box `lo`–`hi` (Liang–Barsky), or `None`
/// when none of it is inside.
fn clip_segment(a: DVec2, b: DVec2, lo: DVec2, hi: DVec2) -> Option<[DVec2; 2]> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-d.x, a.x - lo.x),
        (d.x, hi.x - a.x),
        (-d.y, a.y - lo.y),
        (d.y, hi.y - a.y),
    ] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 <= t1).then(|| [a + d * t0, a + d * t1])
}

/// A short description of a typed value, for the status line.
fn describe(v: TypedValue, units: &UnitSettings) -> String {
    let l = |mm: f64| format_length(mm, units);
    match v {
        TypedValue::Length(a) => l(a),
        TypedValue::Pair(a, b) => format!("{} × {}", l(a), l(b)),
        TypedValue::Delta(d) => format!("Δ {}, {}, {}", l(d.x), l(d.y), l(d.z)),
        TypedValue::Absolute(p) => format!("point [{}, {}, {}]", l(p.x), l(p.y), l(p.z)),
        TypedValue::Relative(p) => format!("offset <{}, {}, {}>", l(p.x), l(p.y), l(p.z)),
        TypedValue::Copies(n) => format!("{n} copies"),
        TypedValue::Divide(n) => format!("divide into {n}"),
        TypedValue::Segments(n) => format!("{n} segments"),
        TypedValue::Radius(r) => format!("radius {}", l(r)),
    }
}

/// The copy Repeat Transform repeats.
#[derive(Debug, Clone)]
struct Chain {
    /// What was copied.
    items: Vec<Pickable>,
    /// One step: from the original to the first copy.
    step: DMat4,
    /// Steps made so far.
    count: u32,
}

/// Bounds of a mesh's face and edge vertices (relative positions + origin).
fn mesh_bounds(mesh: &SceneMesh) -> Option<(DVec3, DVec3)> {
    let mut it = mesh
        .faces
        .iter()
        .map(|v| v.position)
        .chain(mesh.edges.iter().map(|v| v.position))
        .map(|p| DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])) + mesh.origin);
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
}

/// A Operation Inputs field's text when not being edited: live values rounded to
/// the display precision, stored values exact.
fn field_display(f: &Field, units: &UnitSettings) -> String {
    match (f.value, f.kind) {
        (None, _) => String::new(),
        (Some(v), FieldKind::Count | FieldKind::Switch) => format!("{v:.0}"),
        (Some(v), FieldKind::Choice(options)) => options
            .get(v.max(0.0) as usize)
            .map_or_else(String::new, |o| (*o).to_string()),
        (Some(v), FieldKind::Number) => format_number(v),
        (Some(v), FieldKind::Length) if f.live => live_text(v, units),
        (Some(v), FieldKind::Length) => format_exact(v, units),
    }
}

/// A live length rounded to the display precision, in the display unit with
/// no suffix (the unit is shown beside the field); `~` marks rounding.
fn live_text(mm: f64, units: &UnitSettings) -> String {
    match units.unit {
        LengthUnit::FractionalInch | LengthUnit::FeetInches => format_length(mm, units),
        unit => {
            let x = mm / unit.mm_per_unit();
            let text = format!("{x:.*}", usize::from(units.decimals));
            if text.parse::<f64>().is_ok_and(|r| r == x) {
                text
            } else {
                format!("~{text}")
            }
        }
    }
}

/// A plain number at full precision, trailing zeros trimmed.
fn format_number(v: f64) -> String {
    let text = format!("{v:.10}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}

/// A field's value at full precision, for editing.
fn exact_text(f: &Field, v: f64, units: &UnitSettings) -> String {
    match f.kind {
        FieldKind::Count | FieldKind::Switch | FieldKind::Choice(_) => format!("{v:.0}"),
        FieldKind::Number => format_number(v),
        FieldKind::Length => format_exact(v, units),
    }
}

/// The unit shown beside length fields.
fn unit_abbrev(unit: LengthUnit) -> &'static str {
    match unit {
        LengthUnit::Millimetre => "mm",
        LengthUnit::Centimetre => "cm",
        LengthUnit::Metre => "m",
        LengthUnit::DecimalInch | LengthUnit::FractionalInch | LengthUnit::FeetInches => "in",
    }
}

/// A count typed into a count field: a bare whole number or `Ns`.
fn read_count(text: &str) -> Option<u32> {
    let t = text.trim();
    let t = t.strip_suffix('s').unwrap_or(t).trim();
    t.parse().ok()
}

/// Which kind of curve [`CurveValues`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveKind {
    /// A full circle.
    Circle,
    /// An arc.
    Arc,
    /// A regular polygon (its count is sides).
    Polygon,
}

/// A selected curve's values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveValues {
    /// What it is.
    pub kind: CurveKind,
    /// Segments (a polygon's sides).
    pub segments: u32,
    /// Radius, mm.
    pub radius: f64,
    /// The count cannot change: the curve bounds faces out of its plane.
    pub locked: bool,
}

/// One row of the Outliner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlinerRow {
    /// The instances from the root down to this one.
    pub path: Vec<InstanceId>,
    /// The instance's name, or its definition's.
    pub name: String,
    /// A component (else a group).
    pub component: bool,
    /// Locked.
    pub locked: bool,
    /// Hidden.
    pub hidden: bool,
    /// Selected in the context being edited.
    pub selected: bool,
    /// Open for editing (on the path of open groups).
    pub open: bool,
}

/// A change made from the Outliner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutlinerChange {
    /// A new instance name.
    Rename(String),
    /// Lock (`true`) or unlock.
    Lock(bool),
    /// Hide (`true`) or show.
    Hide(bool),
}

/// What picking and inference look at: the snapping view when there is one,
/// else the context itself.
fn ctx_snap<'a>(
    d: &'a Document,
    ctx: Option<DefinitionId>,
    snap: Option<&'a SnapView>,
) -> &'a Entities {
    snap.map_or_else(|| ctx_of(d, ctx), |s| &s.entities)
}

/// The geometry of context `ctx` (`None`: the root). A free function, so it
/// borrows only the document while a tool is borrowed mutably.
fn ctx_of(d: &Document, ctx: Option<DefinitionId>) -> &Entities {
    d.context(ctx).unwrap_or_else(|| d.root())
}

/// The geometry of context `ctx` (`None`: the root), for an operation.
fn active_mut(d: &mut Document, ctx: Option<DefinitionId>) -> Result<&mut Entities, ModelError> {
    d.context_entities_mut(ctx).ok_or(ModelError::NoSuchEntity)
}

/// Runs `op` in context `ctx`. A shape, sweep, loft or revolve becomes a
/// new group there, named after it and keeping its recipe; Copy Along Path
/// adds instances there; everything else works on the context's own
/// geometry.
fn run_draw(
    d: &mut Document,
    ctx: Option<DefinitionId>,
    op: &DrawOp,
) -> Result<smith_ops::draw::DrawReport, ModelError> {
    if let DrawOp::Fuse(f) = op {
        smith_ops::fuse::fuse(d, ctx, f)?;
        return Ok(smith_ops::draw::DrawReport::default());
    }
    if let DrawOp::Intersect(i) = op {
        // What it cuts against is gathered from the document: the open
        // context's loose faces, and with Model those outside it.
        let against = match i.with {
            smith_ops::intersect::With::Selection => smith_model::Against::default(),
            smith_ops::intersect::With::Context => {
                d.intersect_against(&i.path, false).unwrap_or_default()
            }
            smith_ops::intersect::With::Model => {
                d.intersect_against(&i.path, true).unwrap_or_default()
            }
        };
        let (report, hint) = smith_ops::intersect::intersect(active_mut(d, ctx)?, i, &against)?;
        return Ok(smith_ops::draw::DrawReport {
            report,
            hint,
            ..Default::default()
        });
    }
    if let DrawOp::CleanUp(c) = op {
        // The geometry passes in the context, then Purge Unused on the
        // whole document.
        let mut made = smith_ops::draw::draw(active_mut(d, ctx)?, op)?;
        if c.passes.purge_unused {
            let groups = d.purge_unused_groups()?;
            made.report.push(ReportLine::count(
                "Unused Groups Removed",
                groups,
                Vec::new(),
            ));
            // Unused materials are held back until asked.
            if c.remove_materials {
                let materials = d.remove_unused_materials();
                made.report.push(ReportLine::count(
                    "Unused Materials Removed",
                    materials,
                    Vec::new(),
                ));
            }
        }
        return Ok(made);
    }
    let made = match op {
        DrawOp::Sweep { .. }
        | DrawOp::Loft { .. }
        | DrawOp::ExtrudeEdges { .. }
        | DrawOp::Skin { .. } => smith_ops::draw::recipe_of(active_mut(d, ctx)?, op).transpose()?,
        DrawOp::Revolve {
            face,
            axis_point,
            axis_dir,
            angle,
            steps,
        } => Some(smith_ops::sweep::revolve_recipe(
            active_mut(d, ctx)?,
            *face,
            *axis_point,
            *axis_dir,
            *angle,
            *steps,
        )?),
        _ => None,
    };
    if let Some(recipe) = made {
        // The group's origin sits at the path's start (a revolve's on its
        // axis); its recipe is kept in the group's own coordinates. The
        // picked face stays where it is.
        let origin = match &recipe {
            smith_model::SweepRecipe::Sweep { path, .. }
            | smith_model::SweepRecipe::Loft {
                path: Some(path), ..
            } => path.first().copied().unwrap_or(DVec3::ZERO),
            // By spline: at the first profile's centre.
            smith_model::SweepRecipe::Loft { profiles, .. } => profiles
                .first()
                .map(|o| o.iter().copied().sum::<DVec3>() / o.len().max(1) as f64)
                .unwrap_or(DVec3::ZERO),
            smith_model::SweepRecipe::Revolve { axis_point, .. } => *axis_point,
            // At the frame's first point.
            smith_model::SweepRecipe::Skin { segments, .. } => {
                segments.first().map_or(DVec3::ZERO, |s| s[0])
            }
        };
        let recipe = recipe.transformed(&DMat4::from_translation(-origin));
        let def = d.create_definition(recipe.name(), DefinitionKind::Group);
        let g = d
            .context_entities_mut(Some(def))
            .ok_or(ModelError::NoSuchEntity)?;
        smith_ops::sweep::build_sweep(g, &recipe)?;
        // Lines of a skin's frame in no cell are said on the bottom line.
        let hint = match &recipe {
            smith_model::SweepRecipe::Skin { segments, .. }
                if smith_ops::skin::frame_cells(segments).unused > 0 =>
            {
                Some("Some Lines Are in No Cell of Three or Four Sides")
            }
            _ => None,
        };
        d.set_definition_sweep(def, Some(recipe))?;
        d.add_instance(ctx, def, DMat4::from_translation(origin), "")?;
        return Ok(smith_ops::draw::DrawReport {
            hint,
            ..smith_ops::draw::DrawReport::default()
        });
    }
    if let DrawOp::CopyAlongPath {
        instance,
        path,
        closed,
        count,
        spacing,
        up,
    } = op
    {
        // Copies of the group or component, as instances of its definition,
        // each its original carried and turned by the path's frames.
        let (def, placed) = {
            let i = active_mut(d, ctx)?
                .instance(*instance)
                .ok_or(ModelError::NoSuchEntity)?;
            (i.definition, i.transform)
        };
        let total = smith_ops::sweep::path_length(path, *closed);
        let stations = smith_ops::sweep::copy_stations(total, *closed, *count, *spacing);
        let moves = smith_ops::sweep::path_placements(path, *closed, *up, &stations)?;
        for m in moves {
            d.add_instance(ctx, def, m * placed, "")?;
        }
        return Ok(smith_ops::draw::DrawReport::default());
    }
    if let DrawOp::EditSweep { definition, recipe } = op {
        // The group's geometry is replaced by the sweep from its new recipe.
        let g = d
            .context_entities_mut(Some(*definition))
            .ok_or(ModelError::NoSuchEntity)?;
        let edges: Vec<smith_model::EdgeId> = g.edge_ids().collect();
        for e in edges {
            if g.edge(e).is_some() {
                g.erase_edge(e)?;
            }
        }
        smith_ops::sweep::build_sweep(g, recipe)?;
        d.set_definition_sweep(*definition, Some(recipe.clone()))?;
        return Ok(smith_ops::draw::DrawReport::default());
    }
    if let DrawOp::Shape { spec, transform } = op {
        let def = d.create_definition(spec.name(), DefinitionKind::Group);
        let g = d
            .context_entities_mut(Some(def))
            .ok_or(ModelError::NoSuchEntity)?;
        smith_ops::shapes::build_shape(g, *spec)?;
        d.set_definition_shape(def, Some(*spec))?;
        d.add_instance(ctx, def, *transform, "")?;
        return Ok(smith_ops::draw::DrawReport::default());
    }
    if let DrawOp::EditShape { definition, spec } = op {
        // The group's geometry is replaced by the shape at its new sizes.
        let g = d
            .context_entities_mut(Some(*definition))
            .ok_or(ModelError::NoSuchEntity)?;
        let edges: Vec<smith_model::EdgeId> = g.edge_ids().collect();
        for e in edges {
            if g.edge(e).is_some() {
                g.erase_edge(e)?;
            }
        }
        smith_ops::shapes::build_shape(g, *spec)?;
        d.set_definition_shape(*definition, Some(*spec))?;
        return Ok(smith_ops::draw::DrawReport::default());
    }
    draw(active_mut(d, ctx)?, op)
}

/// Adds the groups and components of `entities` (placed at `outer`) to
/// `mesh` while a group is being edited: those on the open `path` are
/// entered, the innermost drawn with `selection`, the rest faded.
fn append_path(
    mesh: &mut SceneMesh,
    doc: &Document,
    entities: &Entities,
    path: &[InstanceId],
    outer: &DMat4,
    selection: &Selection,
    cuts: bool,
) {
    for id in entities.instance_ids() {
        let Some(inst) = entities.instance(id) else {
            continue;
        };
        if path.first() != Some(&id) {
            append_instance(mesh, doc, inst, outer, Shown::Faded, cuts, 0);
            continue;
        }
        let Some(inside) = doc.context(Some(inst.definition)) else {
            continue;
        };
        let m = *outer * inst.transform;
        let rest = &path[1..];
        let start = (mesh.faces.len(), mesh.edges.len(), mesh.styled.len());
        if rest.is_empty() {
            mesh.append_with(inside, &m, Shown::Plain, Some(selection));
            for nid in inside.instance_ids() {
                if let Some(n) = inside.instance(nid) {
                    let shown = if selection.contains(Pickable::Instance(nid)) {
                        Shown::Selected
                    } else {
                        Shown::Plain
                    };
                    append_instance(mesh, doc, n, &m, shown, cuts, 0);
                }
            }
        } else {
            mesh.append(inside, &m, Shown::Faded);
            append_path(mesh, doc, inside, rest, &m, selection, cuts);
        }
        if cuts {
            cut_group(mesh, doc, inside, &m, start);
        }
    }
}

/// The layers switched off in `doc`, for the mesh builders.
fn visibility(doc: &Document) -> Visibility {
    let base = doc.base_layer();
    Visibility {
        hidden_layers: doc
            .layer_ids()
            .filter(|l| doc.layer(*l).is_some_and(|x| !x.visible))
            .map(|l| if Some(l) == base { None } else { Some(l) })
            .collect(),
    }
}

/// Adds instance `inst` (placed inside a context at `outer`) and the
/// instances nested in its definition to `mesh`.
fn append_instance(
    mesh: &mut SceneMesh,
    doc: &Document,
    inst: &Instance,
    outer: &DMat4,
    shown: Shown,
    cuts: bool,
    depth: u32,
) {
    if inst.hidden || !mesh.visibility.shows(inst.layer) {
        return;
    }
    let Some(inside) = doc.context(Some(inst.definition)) else {
        return;
    };
    let m = *outer * inst.transform;
    let start = (mesh.faces.len(), mesh.edges.len(), mesh.styled.len());
    mesh.append(inside, &m, shown);
    if depth < 64 {
        for id in inside.instance_ids() {
            if let Some(n) = inside.instance(id) {
                append_instance(mesh, doc, n, &m, shown, cuts, depth + 1);
            }
        }
    }
    if cuts {
        cut_group(mesh, doc, inside, &m, start);
    }
}

/// A group's own active section plane cuts just its contents (and the
/// groups inside it): what was appended from `start` on is clipped to the
/// kept side, and the cut is filled.
fn cut_group(
    mesh: &mut SceneMesh,
    doc: &Document,
    inside: &Entities,
    m: &DMat4,
    start: (usize, usize, usize),
) {
    let Some((_, local)) = inside.active_section() else {
        return;
    };
    let normal = m.inverse().transpose().transform_vector3(local.normal);
    let Some(plane) = SectionPlane::new(m.transform_point3(local.point), normal) else {
        return;
    };
    clip_mesh_from(mesh, start, &plane);
    section_fill_walk(mesh, doc, inside, m, &plane, 0);
}

/// Clips the faces and edges appended from `start` to the side of `plane`
/// that is kept (away from its normal).
fn clip_mesh_from(mesh: &mut SceneMesh, start: (usize, usize, usize), plane: &SectionPlane) {
    let o = mesh.origin;
    let side = |p: [f32; 3]| {
        let w = DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])) + o;
        (w - plane.point).dot(plane.normal)
    };
    let lerp3 = |a: [f32; 3], b: [f32; 3], t: f32| {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };
    // Triangles: Sutherland–Hodgman against the one plane.
    let tris: Vec<FaceVertex> = mesh.faces.split_off(start.0);
    for t in tris.as_chunks::<3>().0 {
        let d: Vec<f64> = t.iter().map(|v| side(v.position)).collect();
        if d.iter().all(|x| *x <= 0.0) {
            mesh.faces.extend_from_slice(t);
            continue;
        }
        if d.iter().all(|x| *x > 0.0) {
            continue;
        }
        let mut poly: Vec<FaceVertex> = Vec::new();
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            let (da, db) = (d[i], d[(i + 1) % 3]);
            if da <= 0.0 {
                poly.push(a);
            }
            if (da <= 0.0) != (db <= 0.0) {
                let k = (da / (da - db)) as f32;
                poly.push(FaceVertex {
                    position: lerp3(a.position, b.position, k),
                    uv: [
                        a.uv[0] + (b.uv[0] - a.uv[0]) * k,
                        a.uv[1] + (b.uv[1] - a.uv[1]) * k,
                    ],
                    ..a
                });
            }
        }
        for i in 1..poly.len().saturating_sub(1) {
            mesh.faces
                .extend_from_slice(&[poly[0], poly[i], poly[i + 1]]);
        }
    }
    // Edges.
    let lines: Vec<LineVertex> = mesh.edges.split_off(start.1);
    for l in lines.as_chunks::<2>().0 {
        let [a, b] = *l;
        let (da, db) = (side(a.position), side(b.position));
        match (da <= 0.0, db <= 0.0) {
            (true, true) => mesh.edges.extend_from_slice(l),
            (false, false) => {}
            (keep_a, _) => {
                let k = (da / (da - db)) as f32;
                let cut = LineVertex {
                    position: lerp3(a.position, b.position, k),
                    along: a.along + (b.along - a.along) * k,
                    ..a
                };
                if keep_a {
                    mesh.edges.extend_from_slice(&[a, cut]);
                } else {
                    mesh.edges.extend_from_slice(&[cut, b]);
                }
            }
        }
    }
    // Styled edges: kept, dropped, or shortened to the kept side.
    let styled = mesh.styled.split_off(start.2);
    for s in styled {
        let (da, db) = (side(s.a), side(s.b));
        match (da <= 0.0, db <= 0.0) {
            (true, true) => mesh.styled.push(s),
            (false, false) => {}
            (keep_a, _) => {
                let cut = lerp3(s.a, s.b, (da / (da - db)) as f32);
                mesh.styled.push(if keep_a {
                    StyledSegment { b: cut, ..s }
                } else {
                    StyledSegment { a: cut, ..s }
                });
            }
        }
    }
}

/// A connected piece of loose geometry: the edges reachable from each other
/// through shared points, and the faces they bound.
#[derive(Debug, Clone)]
pub struct Piece {
    /// Its shape's name ("Rectangle", "Circle"…), or "Geometry".
    pub name: String,
    /// Its edges.
    pub edges: Vec<smith_model::EdgeId>,
    /// Its faces.
    pub faces: Vec<smith_model::FaceId>,
    /// Every part of it is hidden.
    pub hidden: bool,
}

impl Piece {
    /// Its edges and faces, as pickables.
    pub fn items(&self) -> impl Iterator<Item = Pickable> + '_ {
        self.edges
            .iter()
            .map(|e| Pickable::Edge(*e))
            .chain(self.faces.iter().map(|f| Pickable::Face(*f)))
    }
}

/// The connected pieces of `e`'s loose geometry, in creation order.
fn loose_pieces(e: &Entities) -> Vec<Piece> {
    use std::collections::BTreeMap;
    // Union-find over vertices, joined by edges.
    let mut parent: BTreeMap<smith_model::VertexId, smith_model::VertexId> = BTreeMap::new();
    fn find(
        parent: &mut BTreeMap<smith_model::VertexId, smith_model::VertexId>,
        v: smith_model::VertexId,
    ) -> smith_model::VertexId {
        let mut r = v;
        while let Some(p) = parent.get(&r).copied().filter(|p| *p != r) {
            r = p;
        }
        parent.insert(v, r);
        r
    }
    for id in e.edge_ids() {
        if let Some(edge) = e.edge(id) {
            let [a, b] = edge.vertices();
            parent.entry(a).or_insert(a);
            parent.entry(b).or_insert(b);
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                parent.insert(ra, rb);
            }
        }
    }
    let mut order: Vec<smith_model::VertexId> = Vec::new();
    let mut groups: BTreeMap<
        smith_model::VertexId,
        (Vec<smith_model::EdgeId>, Vec<smith_model::FaceId>),
    > = BTreeMap::new();
    for id in e.edge_ids() {
        if let Some(edge) = e.edge(id) {
            let r = find(&mut parent, edge.vertices()[0]);
            if !groups.contains_key(&r) {
                order.push(r);
            }
            groups.entry(r).or_default().0.push(id);
        }
    }
    for f in e.face_ids() {
        let first = e.face(f).and_then(|x| x.outer().first().map(|u| u.edge));
        if let Some(v) = first.and_then(|ed| e.edge(ed)).map(|x| x.vertices()[0]) {
            let r = find(&mut parent, v);
            groups.entry(r).or_default().1.push(f);
        }
    }
    order
        .into_iter()
        .filter_map(|r| groups.remove(&r))
        .map(|(edges, faces)| {
            let hidden = edges
                .iter()
                .all(|x| e.edge(*x).is_some_and(|y| y.flags.hidden))
                && faces
                    .iter()
                    .all(|x| e.face(*x).is_some_and(|y| y.attributes.hidden));
            let name = shape_name(e, &edges, &faces)
                .unwrap_or("Geometry")
                .to_string();
            Piece {
                name,
                edges,
                faces,
                hidden,
            }
        })
        .collect()
}

/// The name the drawing tools give the shape `edges` and `faces` make: a
/// line, a whole curve, or a rectangle or square face.
fn shape_name(
    e: &Entities,
    edges: &[smith_model::EdgeId],
    faces: &[smith_model::FaceId],
) -> Option<&'static str> {
    use smith_geom::curve::Curve;
    // One whole curve (with or without the face it closes).
    let curves: Vec<_> = edges
        .iter()
        .map(|x| e.edge(*x).and_then(|y| y.curve()))
        .collect();
    if let Some(Some(c)) = curves.first()
        && curves.iter().all(|x| *x == Some(*c))
        && faces.len() <= 1
        && let Some(rec) = e.curve(*c)
        && rec.edges().len() == edges.len()
    {
        return match rec.curve() {
            Curve::Circle(_) => Some("Circle"),
            Curve::Arc(_) => Some("Arc"),
            Curve::Polygon(_) => Some("Polygon"),
            Curve::Bezier(_) => Some("Bézier Spline"),
            Curve::Helix(_) => Some("Curve Maker"),
            Curve::Polyline(_) => None,
        };
    }
    if faces.is_empty() && edges.len() == 1 {
        return Some("Line");
    }
    // A four-sided face with right angles.
    if let [f] = faces
        && edges.len() == 4
        && let Some(face) = e.face(*f)
        && face.inner().is_empty()
    {
        let p = e.loop_points(face.outer());
        if p.len() == 4 {
            let side = |i: usize| p[(i + 1) % 4] - p[i];
            let square_corners = (0..4).all(|i| {
                let (a, b) = (side(i), side((i + 1) % 4));
                a.normalize_or_zero().dot(b.normalize_or_zero()).abs() < 1e-9
            });
            if square_corners {
                let (w, h) = (side(0).length(), side(1).length());
                return Some(if (w - h).abs() <= 1e-9 * w.max(h) {
                    "Square"
                } else {
                    "Rectangle"
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GPU device of this machine's, if it has one (tests that need one
    /// pass without it).
    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::default();
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    #[test]
    fn a_picture_shows_the_model_its_marks_and_no_axes() {
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter: picture not rendered");
            return;
        };
        let mut c = Canvas::new(None);
        let r = c.document.root_mut();
        let s = [
            DVec3::ZERO,
            DVec3::new(50.0, 0.0, 0.0),
            DVec3::new(50.0, 50.0, 0.0),
            DVec3::new(0.0, 50.0, 0.0),
        ];
        let up = DVec3::new(0.0, 0.0, 50.0);
        for i in 0..4 {
            let (a, b) = (s[i], s[(i + 1) % 4]);
            r.add_line(a, b).unwrap();
            r.add_line(a + up, b + up).unwrap();
            r.add_line(a, a + up).unwrap();
        }
        r.add_annotation(Annotation::Linear {
            a: smith_model::Anchor::Point(DVec3::ZERO),
            b: smith_model::Anchor::Point(DVec3::new(50.0, 0.0, 0.0)),
            offset: DVec3::new(0.0, -20.0, 0.0),
        })
        .unwrap();
        c.rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(300.0, 200.0));
        let bounds = c.document.model_bounds().unwrap();
        c.camera.zoom_extents(bounds, 1.5);
        let ctx = egui::Context::default();
        // One frame loads the fonts; its texture changes are not needed.
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let (w, h) = (600, 400);
        let px = c.picture_on(&ctx, (&device, &queue), w, h, false).unwrap();
        assert_eq!(px.len(), (w * h * 4) as usize);
        let at = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        // The corner is background; the middle is the box.
        let corner = at(2, 2);
        assert_eq!(corner[3], 255);
        assert_ne!(at(w / 2, h / 2), corner);
        if let Some(out) = std::env::var_os("SKETCHSMITH_PICTURE_OUT") {
            smith_io::export::write_png(std::path::Path::new(&out), w, h, &px).unwrap();
        }
        // Transparent: the corner clear, the box solid.
        let clear = c.picture_on(&ctx, (&device, &queue), w, h, true).unwrap();
        assert_eq!(clear[3], 0);
        let mid = ((h / 2 * w + w / 2) * 4 + 3) as usize;
        assert_eq!(clear[mid], 255);
        // The canvas's own state is as it was.
        assert_eq!(c.rect.width(), 300.0);
        assert!((c.overlay_scale - 1.0).abs() < f32::EPSILON);
    }

    /// A canvas with a 50 mm box on Base and a second box on layer `Far`,
    /// and a Render that hides `Far` and looks down from above. The canvas
    /// itself shows everything, from the default view.
    #[test]
    fn a_locked_layer_cannot_be_selected_drawn_on_or_entered() {
        let mut c = Canvas::new(None);
        let (kept, group) = c
            .document
            .transaction("setup", |d| {
                let kept = d.add_layer("Kept", [1, 2, 3]);
                d.set_active_layer(Some(kept));
                d.root_mut().add_polyline(
                    &[
                        DVec3::ZERO,
                        DVec3::X * 10.0,
                        DVec3::new(10.0, 10.0, 0.0),
                        DVec3::Y * 10.0,
                    ],
                    true,
                )?;
                d.set_active_layer(None);
                d.root_mut().add_line(DVec3::X * 20.0, DVec3::X * 30.0)?;
                let def = d.create_definition("Group", DefinitionKind::Group);
                let group = d.add_instance(None, def, DMat4::IDENTITY, "")?;
                d.root_mut()
                    .set_layer(&[Pickable::Instance(group)], Some(kept))?;
                Ok((kept, group))
            })
            .unwrap();
        let mut l = c.document.layer(kept).unwrap().clone();
        l.locked = true;
        c.layer_op("Change Layer", |d| d.set_layer(kept, l));
        let face = c.document.root().face_ids().next().unwrap();
        let loose = c
            .document
            .root()
            .edge_ids()
            .find(|e| {
                let x = c.document.root().edge(*e).unwrap();
                x.flags.layer.is_none()
            })
            .unwrap();
        let corner = c.document.root().face(face).unwrap().outer()[0].edge;
        let vertex = c.document.root().edge(corner).unwrap().vertices()[0];
        // Only what is off the locked layer is taken.
        c.select(
            vec![
                Pickable::Face(face),
                Pickable::Edge(loose),
                Pickable::Vertex(vertex),
                Pickable::Instance(group),
            ],
            SelectMode::Replace,
        );
        assert_eq!(c.selection.ordered(), [Pickable::Edge(loose)]);
        // A locked layer cannot be made active, nor its group opened.
        c.set_active_layer(Some(kept));
        assert_eq!(c.document.active_layer(), None);
        assert_eq!(c.message(), Some("Unlock the Layer to Draw on It"));
        c.enter_instance(group);
        assert!(c.editing.is_empty());
        assert_eq!(c.message(), Some(smith_model::ON_LOCKED_LAYER));
        // A refused step says only why, whatever the tool's words.
        c.message = Some(format!(
            "Couldn't draw that Line: {}.",
            smith_model::ON_LOCKED_LAYER
        ));
        assert_eq!(c.message(), Some(smith_model::ON_LOCKED_LAYER));
        // Remove refuses while it is locked.
        c.layer_op("Remove Layer", |d| d.remove_layer(kept));
        assert!(c.document.layer(kept).is_some());
        assert_eq!(c.message(), Some(smith_model::UNLOCK_LAYER));
    }

    #[test]
    fn scale_layer_scales_the_contents_about_their_centre() {
        let mut c = Canvas::new(None);
        let kept = c
            .document
            .transaction("setup", |d| {
                let kept = d.add_layer("Kept", [1, 2, 3]);
                d.set_active_layer(Some(kept));
                d.root_mut().add_polyline(
                    &[
                        DVec3::ZERO,
                        DVec3::X * 10.0,
                        DVec3::new(10.0, 10.0, 0.0),
                        DVec3::Y * 10.0,
                    ],
                    true,
                )?;
                d.set_active_layer(None);
                Ok(kept)
            })
            .unwrap();
        c.after_change();
        c.select_kind = SelectKind::Faces;
        c.scale_layer(Some(kept));
        // Faces mode or not, the edges and the face are taken.
        assert_eq!(c.selection.len(), 5);
        assert_eq!(c.tool, WorkTool::Draw(DrawKind::Scale));
        let fields = c.draw_tool.as_ref().unwrap().fields();
        assert!((fields[0].value.unwrap() - 10.0).abs() < 1e-9);
        assert!((fields[1].value.unwrap() - 10.0).abs() < 1e-9);
        // 20 wide, proportions locked: twice the size about the centre (5, 5).
        c.apply_field(0, 20.0);
        let (lo, hi) = c.document.root().bounds().unwrap();
        assert!((lo - DVec3::new(-5.0, -5.0, 0.0)).length() < 1e-9, "{lo:?}");
        assert!((hi - DVec3::new(15.0, 15.0, 0.0)).length() < 1e-9, "{hi:?}");
    }

    #[test]
    fn remove_asks_before_deleting_what_is_on_a_layer() {
        let mut c = Canvas::new(None);
        let (kept, empty) = c
            .document
            .transaction("setup", |d| {
                let kept = d.add_layer("Kept", [1, 2, 3]);
                let empty = d.add_layer("Empty", [1, 2, 3]);
                d.set_active_layer(Some(kept));
                d.root_mut().add_polyline(
                    &[
                        DVec3::ZERO,
                        DVec3::X * 10.0,
                        DVec3::new(10.0, 10.0, 0.0),
                        DVec3::Y * 10.0,
                    ],
                    true,
                )?;
                d.set_active_layer(None);
                Ok((kept, empty))
            })
            .unwrap();
        // An empty layer goes at once.
        c.remove_layer(empty);
        assert!(c.document.layer(empty).is_none());
        assert_eq!(c.dialog(), None);
        // One with a face on it asks; No leaves it.
        c.remove_layer(kept);
        assert_eq!(c.dialog(), Some(Dialog::RemoveLayer(kept, 1, 0, 0)));
        c.answer_dialog(false);
        assert_eq!(c.dialog(), None);
        assert!(c.document.layer(kept).is_some());
        assert_eq!(c.document.root().face_count(), 1);
        // Yes deletes the layer and its square, one undo step.
        c.remove_layer(kept);
        c.answer_dialog(true);
        assert!(c.document.layer(kept).is_none());
        assert_eq!(c.document.root().face_count(), 0);
        assert_eq!(c.document.root().edge_count(), 0);
        c.document.undo();
        assert_eq!(c.document.root().face_count(), 1);
    }

    #[test]
    fn select_contents_takes_the_modes_items_and_groups() {
        let mut c = Canvas::new(None);
        let (kept, group) = c
            .document
            .transaction("setup", |d| {
                let kept = d.add_layer("Kept", [1, 2, 3]);
                d.set_active_layer(Some(kept));
                d.root_mut().add_polyline(
                    &[
                        DVec3::ZERO,
                        DVec3::X * 10.0,
                        DVec3::new(10.0, 10.0, 0.0),
                        DVec3::Y * 10.0,
                    ],
                    true,
                )?;
                d.set_active_layer(None);
                d.root_mut().add_line(DVec3::X * 20.0, DVec3::X * 30.0)?;
                let def = d.create_definition("Group", DefinitionKind::Group);
                let group = d.add_instance(None, def, DMat4::IDENTITY, "")?;
                d.root_mut()
                    .set_layer(&[Pickable::Instance(group)], Some(kept))?;
                Ok((kept, group))
            })
            .unwrap();
        c.select_kind = SelectKind::Faces;
        c.select_layer_contents(Some(kept));
        assert_eq!(c.selection.len(), 2);
        assert!(c.selection.contains(Pickable::Instance(group)));
        assert_eq!(c.message(), Some("Selected 1 Face, 1 Group"));
        c.select_kind = SelectKind::Edges;
        c.select_layer_contents(Some(kept));
        assert_eq!(c.message(), Some("Selected 4 Edges, 1 Group"));
        c.select_kind = SelectKind::Vertices;
        c.select_layer_contents(Some(kept));
        assert_eq!(c.message(), Some("Selected 4 Vertices, 1 Group"));
        // Base holds the loose line only.
        c.select_kind = SelectKind::Edges;
        c.select_layer_contents(None);
        assert_eq!(c.message(), Some("Selected 1 Edge"));
        // A hidden layer is shown first.
        let mut l = c.document.layer(kept).unwrap().clone();
        l.visible = false;
        c.layer_op("Change Layer", |d| d.set_layer(kept, l));
        c.select_layer_contents(Some(kept));
        assert!(c.document.layer_visible(Some(kept)));
        assert_eq!(c.selection.len(), 5);
        // A locked one gives nothing.
        let mut l = c.document.layer(kept).unwrap().clone();
        l.locked = true;
        c.layer_op("Change Layer", |d| d.set_layer(kept, l));
        c.select(Vec::new(), SelectMode::Replace);
        c.select_layer_contents(Some(kept));
        assert!(c.selection.is_empty());
        assert_eq!(c.message(), Some(smith_model::UNLOCK_LAYER));
        // Duplicate: the copy is active; the square stays, and says so.
        c.duplicate_layer(kept);
        let copy = c.document.active_layer().unwrap();
        assert_eq!(c.document.layer(copy).unwrap().name, "Kept copy");
        assert_eq!(
            c.message(),
            Some("Loose Geometry Not Copied: Group It First")
        );
        assert_eq!(c.document.undo_name(), Some("Duplicate Layer"));
        // The copy holds the group's copy, on it.
        c.select_layer_contents(Some(copy));
        assert_eq!(c.message(), Some("Selected 1 Group"));
        // An empty layer says so.
        c.add_layer();
        c.select_layer_contents(c.document.active_layer());
        assert_eq!(c.message(), Some("Nothing on This Layer"));
    }

    #[test]
    fn a_new_layer_goes_just_above_the_active_one() {
        let mut c = Canvas::new(None);
        let names = |c: &Canvas| -> Vec<String> {
            c.document
                .layer_ids()
                .filter_map(|l| c.document.layer(l).map(|x| x.name.clone()))
                .collect()
        };
        // On Base: the first goes just above it.
        c.add_layer();
        c.add_layer();
        assert_eq!(names(&c), ["Base", "1", "2"]);
        // With 1 active, the next goes between 1 and 2, and is active.
        let one = c.document.layer_ids().nth(1).unwrap();
        c.set_active_layer(Some(one));
        c.add_layer();
        assert_eq!(names(&c), ["Base", "1", "3", "2"]);
        let three = c.document.layer_ids().nth(2).unwrap();
        assert_eq!(c.document.active_layer(), Some(three));
        // One undo step takes it away again.
        assert_eq!(c.document.undo().as_deref(), Some("Add Layer"));
        assert_eq!(names(&c), ["Base", "1", "2"]);
        // A move to where it already is is no step.
        let revision = c.document.revision();
        c.reorder_layer(one, 0);
        assert_eq!(c.document.revision(), revision);
        c.reorder_layer(one, 1);
        assert_eq!(names(&c), ["Base", "2", "1"]);
        assert_eq!(c.document.undo_name(), Some("Move Layer"));
    }

    fn with_a_render() -> (Canvas, SceneId) {
        let mut c = Canvas::new(None);
        let far = c.document.add_layer("Far", [200, 30, 30]);
        let cube = |r: &mut Entities, at: DVec3| {
            let s = [
                at,
                at + DVec3::new(50.0, 0.0, 0.0),
                at + DVec3::new(50.0, 50.0, 0.0),
                at + DVec3::new(0.0, 50.0, 0.0),
            ];
            let up = DVec3::new(0.0, 0.0, 50.0);
            for i in 0..4 {
                let (a, b) = (s[i], s[(i + 1) % 4]);
                r.add_line(a, b).unwrap();
                r.add_line(a + up, b + up).unwrap();
                r.add_line(a, a + up).unwrap();
            }
        };
        cube(c.document.root_mut(), DVec3::ZERO);
        c.document.root_mut().set_draw_layer(Some(far));
        cube(c.document.root_mut(), DVec3::new(200.0, 0.0, 0.0));
        c.document.root_mut().set_draw_layer(None);
        let mut scene = c.scene_now("Top".into());
        scene.view.pitch = std::f64::consts::FRAC_PI_2 - 0.01;
        scene.view.target = DVec3::new(25.0, 25.0, 25.0);
        scene.view.perspective = false;
        scene.hidden_layers = vec![far];
        let id = c
            .document
            .transaction("Save Render", |d| d.add_scene(scene))
            .unwrap();
        c.rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(300.0, 200.0));
        (c, id)
    }

    #[test]
    fn a_render_is_drawn_as_it_shows_and_the_canvas_is_left_alone() {
        let (mut c, id) = with_a_render();
        let (camera, revision) = (c.camera, c.document.revision());
        let drawn = |c: &Canvas| {
            let v = c.saved_view();
            let view = smith_io::export::DrawingView {
                centre: v.target,
                direction: v.target - v.eye,
                up: v.up,
            };
            smith_io::export::draw(c.document(), smith_io::export::Scope::Model, &view, None)
        };
        let all = drawn(&c);
        let render = c.with_scene(id, |c| drawn(c)).unwrap();
        // The Render hides the far box: only one box's edges remain.
        let span = |d: &smith_io::export::Drawing| {
            let (lo, hi) = d.bounds().unwrap();
            hi.x - lo.x
        };
        assert!(span(&all) > 200.0);
        assert!((span(&render) - 50.0).abs() < 1e-6);
        // Nothing of the canvas changed, and nothing went into undo.
        assert_eq!(c.camera, camera);
        assert_eq!(c.document.revision(), revision);
        let far = c.document.layer_ids().nth(1).unwrap();
        assert!(c.document.layer(far).unwrap().visible);
        // A Render that is gone gives nothing.
        c.document
            .transaction("Remove Render", |d| d.remove_scene(id))
            .unwrap();
        assert!(c.with_scene(id, |_| ()).is_none());
    }

    #[test]
    fn a_render_picture_differs_from_the_canvas() {
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter: picture not rendered");
            return;
        };
        let (mut c, id) = with_a_render();
        let bounds = c.document.model_bounds().unwrap();
        c.camera.zoom_extents(bounds, 1.5);
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let canvas = c
            .picture_on(&ctx, (&device, &queue), 300, 200, false)
            .unwrap();
        let render = c
            .with_scene(id, |c| {
                c.picture_on(&ctx, (&device, &queue), 300, 200, false)
            })
            .unwrap()
            .unwrap();
        assert_ne!(canvas, render);
        // Rendering it twice gives the same picture: nothing was left changed.
        let again = c
            .picture_on(&ctx, (&device, &queue), 300, 200, false)
            .unwrap();
        assert_eq!(canvas, again);
    }

    #[test]
    fn premultiplied_edges_get_their_colour_back() {
        // Half-covered pure red on nothing: premultiplied it reads as dark.
        let half = (0.5f32.powf(1.0 / 2.4) * 1.055 - 0.055) * 255.0;
        let mut px = vec![half.round() as u8, 0, 0, 128];
        unpremultiply(&mut px);
        assert!(px[0] >= 253);
        let mut solid = vec![10, 20, 30, 255];
        unpremultiply(&mut solid);
        assert_eq!(solid, [10, 20, 30, 255]);
    }

    /// A canvas with one painted square and `unused` materials painted on
    /// nothing, and the Clean Up run on it with the default passes.
    fn cleaned(unused: usize) -> Canvas {
        let mut c = Canvas::new(None);
        let red = c
            .document
            .add_material(smith_model::Material::default())
            .unwrap();
        for _ in 0..unused {
            c.document
                .add_material(smith_model::Material::default())
                .unwrap();
        }
        let m = c.document.root_mut();
        m.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::X * 10.0,
                DVec3::new(10.0, 10.0, 0.0),
                DVec3::Y * 10.0,
            ],
            true,
        )
        .unwrap();
        let f = m.face_ids().next().unwrap();
        let paint = smith_model::Paint {
            material: red,
            mapping: smith_model::Mapping::Face(smith_model::TextureFrame::for_side(
                DVec3::Z,
                smith_geom::DVec2::ZERO,
                0.0,
            )),
        };
        m.set_face_paint(f, smith_model::Side::Front, Some(paint))
            .unwrap();
        c.after_change();
        c.run_outcome(Outcome::Commit(DrawOp::CleanUp(
            smith_ops::cleanup::CleanUp::default(),
        )));
        c
    }

    fn report_has(c: &Canvas, start: &str) -> bool {
        c.report.iter().any(|l| l.text.starts_with(start))
    }

    #[test]
    fn clean_up_holds_unused_materials_back_and_asks() {
        let c = cleaned(2);
        assert_eq!(c.dialog(), Some(Dialog::RemoveMaterials(2)));
        assert_eq!(c.message(), Some("You Have Unused Materials"));
        assert_eq!(c.document.material_ids().count(), 3);
        assert!(report_has(&c, "Unused Groups Removed"));
        assert!(!report_has(&c, "Unused Materials Removed"));
    }

    #[test]
    fn yes_removes_them_in_the_same_undo_step() {
        let mut c = cleaned(2);
        c.answer_dialog(true);
        assert_eq!(c.dialog(), None);
        assert_eq!(c.message(), None);
        assert_eq!(c.document.material_ids().count(), 1);
        assert!(
            c.report
                .iter()
                .any(|l| l.text == "Unused Materials Removed: 2")
        );
        // One undo takes the whole Clean Up back, materials included.
        c.document.undo();
        assert_eq!(c.document.material_ids().count(), 3);
        assert!(!c.document.can_undo());
    }

    #[test]
    fn no_keeps_them() {
        let mut c = cleaned(1);
        assert_eq!(c.dialog(), Some(Dialog::RemoveMaterials(1)));
        c.answer_dialog(false);
        assert_eq!(c.dialog(), None);
        assert_eq!(c.message(), None);
        assert_eq!(c.document.material_ids().count(), 2);
        assert!(!report_has(&c, "Unused Materials Removed"));
    }

    #[test]
    fn nothing_is_asked_without_unused_materials() {
        let c = cleaned(0);
        assert_eq!(c.dialog(), None);
        assert_eq!(c.message(), None);
    }

    /// A canvas with one 10 mm square at the origin, nothing selected.
    fn with_square() -> Canvas {
        let mut c = Canvas::new(None);
        c.document
            .root_mut()
            .add_polyline(
                &[
                    DVec3::ZERO,
                    DVec3::X * 10.0,
                    DVec3::new(10.0, 10.0, 0.0),
                    DVec3::Y * 10.0,
                ],
                true,
            )
            .unwrap();
        c.after_change();
        c
    }

    fn counts(c: &Canvas) -> (usize, usize, usize) {
        let s = c.selection();
        (s.vertices().count(), s.edges().count(), s.faces().count())
    }

    #[test]
    fn switching_select_mode_converts_the_selection() {
        let mut c = with_square();
        assert_eq!(c.select_kind(), SelectKind::Edges);
        let e = c.document.root().edge_ids().next().unwrap();
        c.select(vec![Pickable::Edge(e)], SelectMode::Replace);
        // Down to vertices: the edge's two ends (and the edge they complete).
        c.set_select_kind(SelectKind::Vertices);
        assert_eq!(counts(&c), (2, 1, 0));
        // Up to faces: one edge completes no face.
        c.set_select_kind(SelectKind::Faces);
        assert_eq!(counts(&c), (0, 0, 0));
        let f = c.document.root().face_ids().next().unwrap();
        c.select(vec![Pickable::Face(f)], SelectMode::Replace);
        c.set_select_kind(SelectKind::Edges);
        assert_eq!(counts(&c), (4, 4, 1));
    }

    #[test]
    fn delete_on_a_selected_vertex_takes_its_edges_and_face() {
        let mut c = with_square();
        let corner = c.document.root().find_vertex(DVec3::ZERO).unwrap();
        c.set_select_kind(SelectKind::Vertices);
        c.select(vec![Pickable::Vertex(corner)], SelectMode::Replace);
        c.erase_selection();
        let root = c.document.root();
        assert_eq!((root.edge_count(), root.face_count()), (2, 0));
        c.document.undo();
        assert_eq!(c.document.root().face_count(), 1);
    }

    #[test]
    fn select_by_type_finds_vertices_and_any_leaves_them_out() {
        let c = with_square();
        let vertices = ByType {
            kind: BY_TYPE_VERTICES,
            ..ByType::default()
        };
        let found = c.matching(&vertices);
        assert_eq!(found.len(), 4);
        assert!(found.iter().all(|p| matches!(p, Pickable::Vertex(_))));
        let any = c.matching(&ByType::default());
        assert!(any.iter().all(|p| !matches!(p, Pickable::Vertex(_))));
        assert_eq!(any.len(), 5);
    }

    #[test]
    fn fill_joins_picked_vertices_in_one_undo_step() {
        let mut c = with_square();
        // Take the face away, keeping its outline.
        let f = c.document.root().face_ids().next().unwrap();
        c.document
            .transaction("Erase", |d| d.root_mut().erase_face(f))
            .unwrap();
        c.after_change();
        // Nothing selected: the hint.
        c.fill_selection();
        assert_eq!(c.message(), Some("Select Vertices or Edges to Fill"));
        c.set_select_kind(SelectKind::Vertices);
        let picks: Vec<Pickable> = [
            DVec3::ZERO,
            DVec3::X * 10.0,
            DVec3::new(10.0, 10.0, 0.0),
            DVec3::Y * 10.0,
        ]
        .iter()
        .map(|p| Pickable::Vertex(c.document.root().find_vertex(*p).unwrap()))
        .collect();
        for p in &picks {
            c.select(vec![*p], SelectMode::Add);
        }
        c.fill_selection();
        assert_eq!(c.document.root().face_count(), 1);
        c.document.undo();
        assert_eq!(c.document.root().face_count(), 0);
    }

    #[test]
    fn fill_off_one_plane_asks_auto_triangulate() {
        let mut c = Canvas::new(None);
        let pts = [
            DVec3::ZERO,
            DVec3::X * 10.0,
            DVec3::new(10.0, 10.0, 3.0),
            DVec3::Y * 10.0,
        ];
        for p in pts {
            c.document
                .root_mut()
                .add_line(p, p - DVec3::Z * 20.0)
                .unwrap();
        }
        c.after_change();
        c.set_select_kind(SelectKind::Vertices);
        for p in pts {
            let v = c.document.root().find_vertex(p).unwrap();
            c.select(vec![Pickable::Vertex(v)], SelectMode::Add);
        }
        c.fill_selection();
        assert_eq!(c.dialog(), Some(Dialog::AutoTriangulate));
        c.answer_dialog(true);
        let triangles = c
            .document
            .root()
            .face_ids()
            .filter(|f| c.document.root().face(*f).unwrap().outer().len() == 3)
            .count();
        assert_eq!(triangles, 2);
    }

    #[test]
    fn ctrl_click_picks_the_path_in_order() {
        let mut c = Canvas::new(None);
        let p = |x: f64| DVec3::new(x, 0.0, 0.0);
        c.document
            .root_mut()
            .add_polyline(&[p(0.0), p(1.0), p(2.0), p(3.0), p(4.0)], false)
            .unwrap();
        c.after_change();
        c.set_select_kind(SelectKind::Vertices);
        let at = |c: &Canvas, x: f64| c.document.root().find_vertex(p(x)).unwrap();
        c.select(vec![Pickable::Vertex(at(&c, 4.0))], SelectMode::Replace);
        c.select_path(Pickable::Vertex(at(&c, 0.0)));
        let order: Vec<_> = c.selection().vertices().collect();
        let want: Vec<_> = [4.0, 3.0, 2.0, 1.0, 0.0]
            .iter()
            .map(|x| at(&c, *x))
            .collect();
        assert_eq!(order, want);
        // Edges mode: the edges between, in order.
        c.set_select_kind(SelectKind::Edges);
        let edge = |c: &Canvas, a: f64, b: f64| {
            let r = c.document.root();
            r.edge_between(r.find_vertex(p(a)).unwrap(), r.find_vertex(p(b)).unwrap())
                .unwrap()
        };
        c.select(
            vec![Pickable::Edge(edge(&c, 0.0, 1.0))],
            SelectMode::Replace,
        );
        c.select_path(Pickable::Edge(edge(&c, 3.0, 4.0)));
        assert_eq!(c.selection().edges().count(), 4);
    }

    #[test]
    fn changing_gap_redoes_close_gaps_in_one_undo_step() {
        use smith_ops::gaps::CloseGaps;
        let mut c = Canvas::new(None);
        c.document
            .transaction("Lines", |d| {
                let m = d.root_mut();
                m.add_line(DVec3::ZERO, DVec3::X * 10.0)?;
                m.add_line(DVec3::new(5.0, 2.0, 0.0), DVec3::new(5.0, 8.0, 0.0))
                    .map(|_| ())
            })
            .unwrap();
        c.after_change();
        let run = |gap| {
            DrawOp::CloseGaps(CloseGaps {
                gap,
                ..CloseGaps::default()
            })
        };
        // 1 mm: the 2 mm gap stays open (no step recorded).
        c.run_outcome(Outcome::Commit(run(1.0)));
        assert_eq!(c.report[0].text, "Gaps Closed: 0");
        // 3 mm, changed right after: it closes.
        c.run_outcome(Outcome::Redefine(run(3.0)));
        assert_eq!(c.report[0].text, "Gaps Closed: 1");
        assert_eq!(c.document.root().edge_count(), 3);
        c.document.undo();
        assert_eq!(c.document.root().edge_count(), 2);
        // The lines are still there: only the Close Gaps step went.
        assert!(c.document.can_undo());
    }

    #[test]
    fn inspect_faces_reports_on_start_and_changes_nothing() {
        let mut c = with_square();
        c.document
            .transaction("Line", |d| {
                d.root_mut()
                    .add_line(DVec3::new(20.0, 0.0, 0.0), DVec3::new(30.0, 0.0, 0.0))
                    .map(|_| ())
            })
            .unwrap();
        c.after_change();
        let before = c.document.root().clone();
        c.set_tool(WorkTool::Draw(DrawKind::InspectFaces));
        let texts: Vec<&str> = c.report.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts.len(), 7);
        assert_eq!(texts[0], "Open Ends: 2");
        // Clicking the line selects the two open ends.
        c.select_report_line(0);
        assert_eq!(c.selection().vertices().count(), 2);
        assert_eq!(c.document.root(), &before);
        // The only undo step is the line drawn before.
        c.document.undo();
        assert!(!c.document.can_undo());
    }

    #[test]
    fn statistics_counts_reversed_faces_and_inspect_solid_finds_them() {
        let mut c = Canvas::new(None);
        let (lo, hi) = (DVec3::ZERO, DVec3::splat(10.0));
        let corner = |x: bool, y: bool, z: bool| {
            DVec3::new(
                if x { hi.x } else { lo.x },
                if y { hi.y } else { lo.y },
                if z { hi.z } else { lo.z },
            )
        };
        let m = c.document.root_mut();
        for quad in [
            [
                corner(false, false, false),
                corner(true, false, false),
                corner(true, true, false),
                corner(false, true, false),
            ],
            [
                corner(false, false, true),
                corner(true, false, true),
                corner(true, true, true),
                corner(false, true, true),
            ],
            [
                corner(false, false, false),
                corner(true, false, false),
                corner(true, false, true),
                corner(false, false, true),
            ],
            [
                corner(false, true, false),
                corner(true, true, false),
                corner(true, true, true),
                corner(false, true, true),
            ],
            [
                corner(false, false, false),
                corner(false, true, false),
                corner(false, true, true),
                corner(false, false, true),
            ],
            [
                corner(true, false, false),
                corner(true, true, false),
                corner(true, true, true),
                corner(true, false, true),
            ],
        ] {
            m.add_polyline(&quad, true).unwrap();
        }
        // Every face outward, then one turned in.
        let centre = DVec3::splat(5.0);
        let faces: Vec<_> = m.face_ids().collect();
        for f in &faces {
            let face = m.face(*f).unwrap();
            let p = m.loop_points(face.outer())[0];
            if face.plane().normal().dot(p - centre) < 0.0 {
                m.reverse_face(*f).unwrap();
            }
        }
        c.after_change();
        assert_eq!(c.reversed_count(), 0);
        let m = c.document.root_mut();
        m.reverse_face(faces[0]).unwrap();
        c.after_change();
        assert_eq!(c.reversed_count(), 1);
        c.set_tool(WorkTool::Draw(DrawKind::InspectManifold));
        assert_eq!(c.report[2].text, "Reversed Faces: 1");
        assert!(!c.document.can_undo());
    }

    #[test]
    fn make_manifold_offers_auto_triangulate_for_a_bent_hole() {
        let mut c = Canvas::new(None);
        let corner = |x: bool, y: bool, z: bool| {
            DVec3::new(
                if x { 10.0 } else { 0.0 },
                if y { 10.0 } else { 0.0 },
                if z { 10.0 } else { 0.0 },
            )
        };
        let m = c.document.root_mut();
        for quad in [
            [
                corner(false, false, false),
                corner(true, false, false),
                corner(true, true, false),
                corner(false, true, false),
            ],
            [
                corner(false, false, true),
                corner(true, false, true),
                corner(true, true, true),
                corner(false, true, true),
            ],
            [
                corner(false, false, false),
                corner(true, false, false),
                corner(true, false, true),
                corner(false, false, true),
            ],
            [
                corner(false, true, false),
                corner(true, true, false),
                corner(true, true, true),
                corner(false, true, true),
            ],
            [
                corner(false, false, false),
                corner(false, true, false),
                corner(false, true, true),
                corner(false, false, true),
            ],
            [
                corner(true, false, false),
                corner(true, true, false),
                corner(true, true, true),
                corner(true, false, true),
            ],
        ] {
            m.add_polyline(&quad, true).unwrap();
        }
        // Take the top away and lift one of its corners: a bent hole.
        let top = m
            .face_ids()
            .find(|f| {
                m.face(*f).unwrap().plane().normal().z.abs() > 0.9
                    && m.loop_points(m.face(*f).unwrap().outer())[0].z > 5.0
            })
            .unwrap();
        m.erase_face(top).unwrap();
        let lift = m.find_vertex(corner(true, true, true)).unwrap();
        smith_model::with_auto_tri(|| m.move_vertices(&[lift], DVec3::Z * 2.0)).unwrap();
        c.after_change();
        c.run_outcome(Outcome::Commit(DrawOp::MakeManifold(
            smith_ops::manifold::MakeManifold::default(),
        )));
        assert_eq!(c.dialog(), Some(Dialog::AutoTriangulate));
        assert_eq!(c.message(), Some(AUTO_TRI_MESSAGE));
        let open = c.document.root().face_count();
        c.answer_dialog(true);
        assert!(c.document.root().face_count() > open);
        let edges: Vec<_> = c.document.root().edge_ids().collect();
        assert!(
            edges
                .iter()
                .all(|e| c.document.root().edge(*e).unwrap().faces().len() == 2)
        );
        // One undo takes the whole run back.
        c.document.undo();
        assert_eq!(c.document.root().face_count(), open);
    }

    #[test]
    fn fuse_groups_in_one_undo_step_and_refusals_say_why() {
        use smith_model::{BooleanOp, DefinitionKind};
        use smith_ops::fuse::Fuse;
        let mut c = Canvas::new(None);
        let corner = |x: bool, y: bool, z: bool| {
            DVec3::new(
                if x { 10.0 } else { 0.0 },
                if y { 10.0 } else { 0.0 },
                if z { 10.0 } else { 0.0 },
            )
        };
        let mut groups = Vec::new();
        for at in [DVec3::ZERO, DVec3::splat(5.0)] {
            let def = c.document.create_definition("Box", DefinitionKind::Group);
            let m = c.document.definition_entities_mut(def).unwrap();
            for q in [
                [
                    corner(false, false, false),
                    corner(true, false, false),
                    corner(true, true, false),
                    corner(false, true, false),
                ],
                [
                    corner(false, false, true),
                    corner(true, false, true),
                    corner(true, true, true),
                    corner(false, true, true),
                ],
                [
                    corner(false, false, false),
                    corner(true, false, false),
                    corner(true, false, true),
                    corner(false, false, true),
                ],
                [
                    corner(false, true, false),
                    corner(true, true, false),
                    corner(true, true, true),
                    corner(false, true, true),
                ],
                [
                    corner(false, false, false),
                    corner(false, true, false),
                    corner(false, true, true),
                    corner(false, false, true),
                ],
                [
                    corner(true, false, false),
                    corner(true, true, false),
                    corner(true, true, true),
                    corner(true, false, true),
                ],
            ] {
                m.add_polyline(&q, true).unwrap();
            }
            for f in m.face_ids().collect::<Vec<_>>() {
                let face = m.face(f).unwrap();
                let p = m.loop_points(face.outer())[0];
                if face.plane().normal().dot(p - DVec3::splat(5.0)) < 0.0 {
                    m.reverse_face(f).unwrap();
                }
            }
            groups.push(
                c.document
                    .add_instance(None, def, DMat4::from_translation(at), "")
                    .unwrap(),
            );
        }
        c.after_change();
        let run = |op| {
            DrawOp::Fuse(Fuse {
                base: Pickable::Instance(groups[0]),
                target: Pickable::Instance(groups[1]),
                op,
            })
        };
        c.run_outcome(Outcome::Commit(run(BooleanOp::Union)));
        assert_eq!(c.document.root().instance_count(), 1);
        c.run_outcome(Outcome::Redefine(run(BooleanOp::Intersect)));
        assert_eq!(c.document.root().instance_count(), 1);
        c.document.undo();
        assert_eq!(c.document.root().instance_count(), 2);
        // Locked: refused, in its own words.
        c.document
            .root_mut()
            .set_instance_locked(groups[0], true)
            .unwrap();
        c.after_change();
        c.run_outcome(Outcome::Commit(run(BooleanOp::Union)));
        assert_eq!(c.message(), Some("Unlock the Groups First"));
        assert_eq!(c.document.root().instance_count(), 2);
    }

    #[test]
    fn a_window_over_the_canvas_holds_its_input() {
        let mut c = Canvas::new(None);
        assert!(!c.input_held());
        c.hold(true);
        assert!(c.input_held());
        c.hold(false);
        assert!(!c.input_held());
        c.offer_auto_tri(
            &ModelError::NotFlat(Vec::new()),
            DrawOp::CleanUp(smith_ops::cleanup::CleanUp::default()),
            false,
        );
        assert!(c.input_held());
    }

    #[test]
    fn intersect_faces_reports_and_hints_at_overlaps_in_one_undo_step() {
        let mut c = Canvas::new(None);
        let m = c.document.root_mut();
        m.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::X * 10.0,
                DVec3::new(10.0, 10.0, 0.0),
                DVec3::Y * 10.0,
            ],
            true,
        )
        .unwrap();
        // A second square lying over the first, moved there so it isn't
        // split on the way.
        m.add_polyline(
            &[
                DVec3::new(5.0, 5.0, 3.0),
                DVec3::new(15.0, 5.0, 3.0),
                DVec3::new(15.0, 15.0, 3.0),
                DVec3::new(5.0, 15.0, 3.0),
            ],
            true,
        )
        .unwrap();
        let up: Vec<_> = m
            .vertex_ids()
            .filter(|v| m.vertex(*v).unwrap().position().z > 1.0)
            .collect();
        m.move_vertices(&up, DVec3::new(0.0, 0.0, -3.0)).unwrap();
        let faces: Vec<smith_model::FaceId> = m.face_ids().collect();
        c.after_change();
        c.run_outcome(Outcome::Commit(DrawOp::Intersect(
            smith_ops::intersect::IntersectFaces {
                faces,
                ..Default::default()
            },
        )));
        let texts: Vec<&str> = c.report.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts[2], "Overlaps Found: 1");
        assert_eq!(c.message(), Some(smith_ops::intersect::COMBINE_HINT));
        assert_eq!(c.document.root().face_count(), 4);
        c.document.undo();
        assert_eq!(c.document.root().face_count(), 2);
        assert!(!c.document.can_undo());
    }

    #[test]
    fn changing_with_redoes_intersect_faces_in_one_undo_step() {
        use smith_ops::intersect::{IntersectFaces, With};
        let mut c = Canvas::new(None);
        let m = c.document.root_mut();
        m.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::X * 10.0,
                DVec3::new(10.0, 10.0, 0.0),
                DVec3::Y * 10.0,
            ],
            true,
        )
        .unwrap();
        let floor = m.face_ids().next().unwrap();
        // A wall across the floor, moved there so it hasn't split it.
        m.add_polyline(
            &[
                DVec3::new(5.0, -2.0, 18.0),
                DVec3::new(5.0, 12.0, 18.0),
                DVec3::new(5.0, 12.0, 23.0),
                DVec3::new(5.0, -2.0, 23.0),
            ],
            true,
        )
        .unwrap();
        let up: Vec<_> = m
            .vertex_ids()
            .filter(|v| m.vertex(*v).unwrap().position().z > 10.0)
            .collect();
        m.move_vertices(&up, DVec3::new(0.0, 0.0, -20.0)).unwrap();
        // An earlier step of the user's, which no redo may take back.
        c.document
            .transaction("Line", |d| {
                d.root_mut()
                    .add_line(DVec3::new(50.0, 0.0, 0.0), DVec3::new(60.0, 0.0, 0.0))
                    .map(|_| ())
            })
            .unwrap();
        c.after_change();
        let run = |with| {
            DrawOp::Intersect(IntersectFaces {
                with,
                faces: vec![floor],
                path: Vec::new(),
            })
        };
        // Selection alone changes nothing, so it records no step.
        c.run_outcome(Outcome::Commit(run(With::Selection)));
        assert_eq!(c.report[0].text, "Cut Lines Added: 0");
        c.run_outcome(Outcome::Redefine(run(With::Context)));
        assert_eq!(c.report[0].text, "Cut Lines Added: 1");
        assert_eq!(c.document.root().face_count(), 3);
        assert_eq!(c.document.root().edge_count(), 12);
        // Model redoes it in place of Context: still one step.
        c.run_outcome(Outcome::Redefine(run(With::Model)));
        assert_eq!(c.document.root().face_count(), 3);
        c.document.undo();
        assert_eq!(c.document.root().face_count(), 2);
        // The earlier line is still there, one undo further back.
        assert_eq!(c.document.root().edge_count(), 9);
        c.document.undo();
        assert_eq!(c.document.root().edge_count(), 8);
        assert!(!c.document.can_undo());
    }

    #[test]
    fn a_refusal_for_bending_asks_auto_triangulate() {
        let mut c = Canvas::new(None);
        // A stray edge, so the run has something to do.
        c.document
            .root_mut()
            .add_line(DVec3::ZERO, DVec3::X * 10.0)
            .unwrap();
        c.after_change();
        let op = DrawOp::CleanUp(smith_ops::cleanup::CleanUp::default());
        c.offer_auto_tri(&ModelError::NotFlat(Vec::new()), op.clone(), false);
        assert_eq!(c.dialog(), Some(Dialog::AutoTriangulate));
        assert_eq!(
            c.message(),
            Some("This action will cause your geometry to triangulate.")
        );
        // No: still refused, the message stays.
        c.answer_dialog(false);
        assert_eq!(c.dialog(), None);
        assert_eq!(c.message(), Some(AUTO_TRI_MESSAGE));
        // Yes: the operation runs again (one undo step).
        c.offer_auto_tri(&ModelError::NotFlat(Vec::new()), op, false);
        c.answer_dialog(true);
        assert_eq!(c.dialog(), None);
        assert_eq!(c.document.root().edge_count(), 0);
        assert!(c.document.can_undo());
        // Any other error is not offered.
        c.offer_auto_tri(
            &ModelError::NoSuchEntity,
            DrawOp::CleanUp(smith_ops::cleanup::CleanUp::default()),
            false,
        );
        assert_eq!(c.dialog(), None);
    }

    #[test]
    fn segments_clip_to_the_box() {
        let (lo, hi) = (DVec2::ZERO, DVec2::new(100.0, 50.0));
        let [a, b] = clip_segment(DVec2::new(-1e9, 25.0), DVec2::new(1e9, 25.0), lo, hi).unwrap();
        assert!((a - DVec2::new(0.0, 25.0)).length() < 1e-6);
        assert!((b - DVec2::new(100.0, 25.0)).length() < 1e-6);
        assert!(clip_segment(DVec2::new(-5.0, 60.0), DVec2::new(200.0, 60.0), lo, hi).is_none());
        let inside = clip_segment(DVec2::new(10.0, 10.0), DVec2::new(20.0, 20.0), lo, hi).unwrap();
        assert_eq!(inside, [DVec2::new(10.0, 10.0), DVec2::new(20.0, 20.0)]);
    }

    #[test]
    fn pie_directions_match_blender_layout() {
        let c = pos2(100.0, 100.0);
        let pick =
            |dx: f32, dy: f32| Canvas::pie_choice(c, c + egui::vec2(dx, dy)).map(|i| PIE[i].0);
        assert_eq!(pick(60.0, 0.0), Some("Right"));
        assert_eq!(pick(-60.0, 0.0), Some("Left"));
        assert_eq!(pick(0.0, -60.0), Some("Top"), "screen up");
        assert_eq!(pick(0.0, 60.0), Some("Bottom"));
        assert_eq!(pick(-50.0, -50.0), Some("Front"));
        assert_eq!(pick(50.0, -50.0), Some("Back"));
        assert_eq!(pick(5.0, 5.0), None, "dead zone");
    }

    #[test]
    fn a_groups_own_plane_clips_what_was_appended_after_the_start() {
        let fv = |x: f32, y: f32| FaceVertex::plain([x, y, 0.0], [0.0, 0.0, 1.0], 0.0);
        let lv = |x: f32| LineVertex {
            position: [x, 0.0, 0.0],
            color: [0.0; 4],
            along: x,
            dashed: 0.0,
        };
        let mut mesh = SceneMesh::default();
        // One triangle before the start (untouched), one across the plane x = 1.
        mesh.faces
            .extend([fv(5.0, 0.0), fv(6.0, 0.0), fv(5.0, 1.0)]);
        mesh.faces
            .extend([fv(0.0, 0.0), fv(2.0, 0.0), fv(0.0, 2.0)]);
        mesh.edges.extend([lv(0.0), lv(2.0)]);
        let plane = SectionPlane::new(DVec3::new(1.0, 0.0, 0.0), DVec3::X).unwrap();
        clip_mesh_from(&mut mesh, (3, 0, 0), &plane);
        // The first triangle stays as it was.
        assert_eq!(mesh.faces[0].position, [5.0, 0.0, 0.0]);
        // The crossing one is cut to x <= 1 (a quad: two triangles).
        let cut = &mesh.faces[3..];
        assert_eq!(cut.len(), 6);
        assert!(cut.iter().all(|v| v.position[0] <= 1.0 + 1e-6));
        // The edge is cut at x = 1.
        assert_eq!(mesh.edges.len(), 2);
        assert!((mesh.edges[1].position[0] - 1.0).abs() < 1e-6);
    }
}

/// The undo steps that only change the look: the ones Present mode may
/// undo and redo.
const LOOK_STEPS: [&str; 5] = [
    "Paint",
    "Material",
    "Mark Edges",
    "Line Style",
    "Smoothing Angle",
];

/// A value whose run of changes is kept as one undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueEdit {
    /// A material's values.
    Material(MaterialId),
    /// The model's smoothing angle.
    Angle,
    /// A layer's line style.
    LayerStyle(smith_model::LayerId),
}

/// Line Style's Operation Inputs.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct LineSettings {
    /// Apply To: a layer (else edges clicked).
    on_layer: bool,
    /// The layer styled.
    layer: Option<smith_model::LayerId>,
    /// The style given (`None` clears).
    style: Option<smith_model::LineStyle>,
}

impl WorkTool {
    /// A Present-mode tool, which sets the look.
    pub fn is_look(&self) -> bool {
        matches!(self, Self::Paint | Self::MarkEdges | Self::LineStyle)
    }
}

/// Select by Type's kinds, as offered (the index is [`ByType::kind`]).
const BY_TYPE_KINDS: &[&str] = &[
    "Any",
    "Vertices",
    "Edges",
    "Curves",
    "Faces",
    "Groups",
    "Components",
    "Guides",
];

/// [`BY_TYPE_KINDS`]' index of Vertices.
const BY_TYPE_VERTICES: usize = 1;

/// Select by Type's choices.
#[derive(Debug, Clone, Default, PartialEq)]
struct ByType {
    /// Index into [`BY_TYPE_KINDS`].
    kind: usize,
    /// `None`: any layer; `Some(None)`: Base.
    layer: Option<Option<LayerId>>,
    /// `None`: any; `Some(None)`: unpainted faces.
    material: Option<Option<MaterialId>>,
    /// The selection it started with (empty: everything here).
    scope: Vec<Pickable>,
}

/// What the last paint painted.
#[derive(Debug, Clone)]
enum LastPaint {
    /// One face side.
    Face(crate::paint_tool::Hit),
    /// A wrap's faces.
    Wrap(WrapTargets),
}
