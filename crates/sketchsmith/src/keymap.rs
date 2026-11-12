//! SketchSmith — keymaps | v1.0.0 | Thursday 12 November 2026
//!
//! Keyboard bindings: the SketchSmith (default), SketchUp and Blender
//! presets, or Custom. The
//! presets are TOML files in `assets/keymaps/`, compiled into the binary so
//! the app always starts with a working keymap; they never change. Custom is
//! the user's own table, `keymap-custom.toml` in the config folder: changing
//! a key while a preset is chosen copies that preset into Custom, switches
//! to it and changes it there (Preferences → Keys).
//!
//! **The binding table** ([`ACTIONS`]): every action, each with its key (a
//! chord) or none, and its tap or none. A **tap** is a right-hand modifier
//! pressed and released alone, with no other key, click or wheel turn
//! between ([`TapDetector`]): Right Alt opens the view pie, Right Ctrl opens
//! or hides the panel, in every preset. A Left Ctrl tap can be bound (in
//! Custom); the drawing tools keep a lone Ctrl tap as their own switch.
//!
//! **Hold chords** ([`HoldKey`]): a letter pressed while Space or Tab is
//! held, written `Space+S`, `Tab+P`. The held key's own binding (Space's
//! Select, Tab's Edit / Present) runs when it is released with nothing
//! pressed while it was down. While text is being typed, Space and Tab type as usual.
//!
//! Mouse navigation is the same in both presets and is not rebindable:
//! middle drag orbits, Shift + middle drag pans, the wheel zooms to the cursor.
//!
//! **Matching order.** egui matches a chord while ignoring extra Shift and Alt,
//! so `Z` would also fire on `Shift+Z`. [`Keymap::ordered`] lists chords with
//! more modifiers first, so the most specific binding always wins.

use std::collections::BTreeMap;
use std::path::Path;

use egui::{Key, Modifiers};
use serde::Deserialize;

/// Built-in presets.
const SKETCHUP_TOML: &str = include_str!("../../../assets/keymaps/sketchup.toml");
const BLENDER_TOML: &str = include_str!("../../../assets/keymaps/blender.toml");
const SKETCHSMITH_TOML: &str = include_str!("../../../assets/keymaps/sketchsmith.toml");

/// The Custom table's file in the config folder.
pub const CUSTOM_FILE: &str = "keymap-custom.toml";

/// Which keymap is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    /// SketchSmith's own keys (default): Ctrl switches on, Shift off, Alt
    /// moves and reshapes, Space and Tab hold chords.
    #[default]
    SketchSmith,
    /// SketchUp-style keys.
    SketchUp,
    /// Blender-style keys.
    Blender,
    /// The user's own table.
    Custom,
}

impl Preset {
    /// Its file-name stem.
    pub fn stem(self) -> &'static str {
        match self {
            Preset::SketchSmith => "sketchsmith",
            Preset::SketchUp => "sketchup",
            Preset::Blender => "blender",
            Preset::Custom => "custom",
        }
    }

    /// Its name on screen.
    pub fn label(self) -> &'static str {
        match self {
            Preset::SketchSmith => "SketchSmith",
            Preset::SketchUp => "SketchUp",
            Preset::Blender => "Blender",
            Preset::Custom => "Custom",
        }
    }

    /// The built-in table, for the presets.
    fn builtin(self) -> Option<&'static str> {
        match self {
            Preset::SketchSmith => Some(SKETCHSMITH_TOML),
            Preset::SketchUp => Some(SKETCHUP_TOML),
            Preset::Blender => Some(BLENDER_TOML),
            Preset::Custom => None,
        }
    }
}

/// A key whose lone tap can be bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapKey {
    /// The right Alt key (AltGr on some layouts).
    RightAlt,
    /// The right Ctrl key.
    RightCtrl,
    /// The left Ctrl key (a tap: Ctrl+click and Ctrl+key are not taps).
    LeftCtrl,
}

impl TapKey {
    /// Every tap key, in the order lists show them.
    pub const ALL: [TapKey; 3] = [TapKey::RightAlt, TapKey::RightCtrl, TapKey::LeftCtrl];

    /// Its name on screen.
    pub fn label(self) -> &'static str {
        match self {
            TapKey::RightAlt => "Right Alt",
            TapKey::RightCtrl => "Right Ctrl",
            TapKey::LeftCtrl => "Left Ctrl",
        }
    }

    /// Its name in keymap files.
    fn file_name(self) -> &'static str {
        match self {
            TapKey::RightAlt => "RightAlt",
            TapKey::RightCtrl => "RightCtrl",
            TapKey::LeftCtrl => "LeftCtrl",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.file_name().eq_ignore_ascii_case(text.trim()))
    }
}

/// Finds lone taps in the stream of key and pointer events (the toolkit
/// does not tell left from right modifiers, so the window's own key events
/// are read).
#[derive(Debug, Default)]
pub struct TapDetector {
    /// The tap key held down with nothing else since, if any.
    down: Option<TapKey>,
}

impl TapDetector {
    /// Takes a key event: `key` is the tap key it is, or `None` for any
    /// other key. Returns the tap completed by this release, if any.
    pub fn key(&mut self, key: Option<TapKey>, pressed: bool, repeat: bool) -> Option<TapKey> {
        if repeat {
            return None;
        }
        if pressed {
            // Another key pressed while one is held spoils the tap.
            self.down = if self.down.is_none() { key } else { None };
            return None;
        }
        match key {
            Some(k) if self.down == Some(k) => self.down.take(),
            _ => {
                self.down = None;
                None
            }
        }
    }

    /// A click, wheel turn or focus change: whatever is held is no longer
    /// a tap.
    pub fn interrupt(&mut self) {
        self.down = None;
    }
}

/// Every bindable action (grows as tools are built).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// Activate the Orbit navigation tool (left drag orbits).
    OrbitTool,
    /// Activate the Pan navigation tool.
    PanTool,
    /// Activate the Zoom navigation tool.
    ZoomTool,
    /// Frame the whole model.
    ZoomExtents,
    /// Standard view: top.
    ViewTop,
    /// Standard view: bottom.
    ViewBottom,
    /// Standard view: front.
    ViewFront,
    /// Standard view: back.
    ViewBack,
    /// Standard view: left.
    ViewLeft,
    /// Standard view: right.
    ViewRight,
    /// Standard view: iso.
    ViewIso,
    /// Toggle perspective / parallel projection.
    ToggleProjection,
    /// Perspective projection on.
    Perspective,
    /// Parallel projection (perspective off).
    Parallel,
    /// Cancel the current tool.
    Cancel,
    /// The Select tool (left button selects).
    SelectTool,
    /// The Paint tool (Present mode).
    PaintTool,
    /// The Mark Edges tool (Present mode).
    MarkEdgesTool,
    /// The Line Style tool (Present mode).
    LineStyleTool,
    /// Select everything in the active context.
    SelectAll,
    /// Clear the selection.
    SelectNone,
    /// Switch between Edit and Present mode.
    ToggleMode,
    /// The Axes tool.
    AxesTool,
    /// Return the drawing axes to the world axes.
    ResetAxes,
    /// The view pie menu (Blender's `~` pie).
    ViewPie,
    /// Undo the last step.
    Undo,
    /// Redo the last undone step.
    Redo,
    /// Show the last shape's values in the Operation Inputs again (Blender F9).
    AdjustLast,
    /// The Line tool.
    LineTool,
    /// The Rectangle tool.
    RectangleTool,
    /// The Rotated Rectangle tool.
    RotatedRectangleTool,
    /// The Circle tool.
    CircleTool,
    /// The Arc tool.
    ArcTool,
    /// The Polygon tool.
    PolygonTool,
    /// The Bézier Spline tool.
    BezierTool,
    /// The Curve Maker (helix, conical helix, flat spiral).
    CurveMakerTool,
    /// The Extrude tool.
    ExtrudeTool,
    /// The Eraser tool.
    EraserTool,
    /// The Move tool.
    MoveTool,
    /// The Rotate tool.
    RotateTool,
    /// The Scale tool.
    ScaleTool,
    /// The Mirror tool.
    MirrorTool,
    /// The Offset (Inset) tool.
    OffsetTool,
    /// The Divide tool.
    DivideTool,
    /// The Loop Cut tool.
    LoopCutTool,
    /// The Knife tool.
    KnifeTool,
    /// The Slide tool.
    SlideTool,
    /// The Dissolve tool.
    DissolveTool,
    /// The Tape Measure tool.
    TapeMeasureTool,
    /// The Protractor tool.
    ProtractorTool,
    /// The Find Center tool.
    FindCenterTool,
    /// Erase every guide in the context being edited.
    DeleteGuides,
    /// The Dimensions tool.
    DimensionTool,
    /// The Text tool.
    TextTool,
    /// The Section Plane tool.
    SectionTool,
    /// The Sweep tool.
    SweepTool,
    /// Revolve.
    RevolveTool,
    /// Extrude Edges along Path.
    ExtrudeEdgesTool,
    /// Bevel.
    BevelTool,
    /// Fill.
    Fill,
    /// Fuse.
    FuseTool,
    /// Weld.
    WeldTool,
    /// Point at Percentage.
    PercentTool,
    /// Space.
    SpaceTool,
    /// Circle Loop.
    CircleLoopTool,
    /// Flatten.
    FlattenTool,
    /// Smooth.
    SmoothTool,
    /// Relax.
    RelaxTool,
    /// Intersect Faces.
    IntersectTool,
    /// Clean Up.
    CleanUpTool,
    /// Close Gaps.
    CloseGapsTool,
    /// Inspect Faces.
    InspectFacesTool,
    /// Inspect Manifold.
    InspectManifoldTool,
    /// Make Manifold.
    MakeManifoldTool,
    /// Trim / Extend.
    TrimTool,
    /// Bisector.
    BisectorTool,
    /// Extend to Face.
    ExtendFaceTool,
    /// Bridge.
    BridgeTool,
    /// Loft along Path.
    LoftTool,
    /// Loft by Spline.
    LoftSplineTool,
    /// Skinning.
    SkinTool,
    /// Copy Along Path.
    CopyAlongPathTool,
    /// The Shapes tool.
    ShapesTool,
    /// Turn the active section plane's cut into a group of edges.
    CreateGroupFromSlice,
    /// Proportional editing on or off.
    ToggleProportional,
    /// Proportional editing on.
    ProportionalOn,
    /// Proportional editing off.
    ProportionalOff,
    /// Reverse the selected faces.
    ReverseFaces,
    /// Turn faces connected to the selected face the same way.
    OrientFaces,
    /// Erase the selection.
    Delete,
    /// Hide the selected edges.
    Hide,
    /// Show the edges hidden last.
    UnhideLast,
    /// Show every hidden edge.
    UnhideAll,
    /// Add everything connected to the selection.
    SelectConnected,
    /// Select by Type: choose a type, layer and material to select.
    SelectByType,
    /// Select mode: Vertices.
    SelectVertices,
    /// Select mode: Edges.
    SelectEdges,
    /// Select mode: Faces.
    SelectFaces,
    /// Repeat the last copy's move, rotate and scale as a new copy.
    RepeatTransform,
    /// Copy the selection to the clipboard and erase it.
    Cut,
    /// Copy the selection to the clipboard.
    Copy,
    /// Paste the clipboard where it was copied from, then move it.
    Paste,
    /// Open or close the sidebar.
    ToggleSidebar,
    /// Make a group of the selection.
    MakeGroup,
    /// Show the selection again.
    Unhide,
    /// Hide everything but the selection.
    HideOthers,
    /// Put a group's or component's geometry back into the model.
    Explode,
    /// Lock the selected group or component.
    Lock,
    /// Unlock the selected group or component.
    Unlock,
    /// Make a component of the selection.
    MakeComponent,
    /// File → New.
    NewFile,
    /// File → Open.
    OpenFile,
    /// File → Save.
    SaveFile,
    /// File → Save As.
    SaveFileAs,
    /// Save Version: the next numbered file, as Blender's Save Incremental.
    SaveVersion,
    /// File → Quit.
    Quit,
}

impl Action {
    fn from_name(name: &str) -> Option<Self> {
        // The tool was named Push/Pull until 0.1.46; old files still load.
        let name = if name == "push_pull_tool" {
            "extrude_tool"
        } else {
            name
        };
        ACTIONS
            .iter()
            .find(|(_, n, _)| *n == name)
            .map(|(a, _, _)| *a)
    }

    /// Its name in keymap files.
    pub fn file_name(self) -> &'static str {
        ACTIONS
            .iter()
            .find(|(a, _, _)| *a == self)
            .map_or("", |(_, n, _)| n)
    }

    /// Its name on screen.
    pub fn label(self) -> &'static str {
        ACTIONS
            .iter()
            .find(|(a, _, _)| *a == self)
            .map_or("", |(_, _, l)| l)
    }
}

/// Every action: its name in keymap files and its name on screen, in the
/// order the Keys list shows them.
pub const ACTIONS: &[(Action, &str, &str)] = &[
    (Action::OrbitTool, "orbit_tool", "Orbit"),
    (Action::PanTool, "pan_tool", "Pan"),
    (Action::ZoomTool, "zoom_tool", "Zoom"),
    (Action::ZoomExtents, "zoom_extents", "Zoom Extents"),
    (Action::ViewTop, "view_top", "View Top"),
    (Action::ViewBottom, "view_bottom", "View Bottom"),
    (Action::ViewFront, "view_front", "View Front"),
    (Action::ViewBack, "view_back", "View Back"),
    (Action::ViewLeft, "view_left", "View Left"),
    (Action::ViewRight, "view_right", "View Right"),
    (Action::ViewIso, "view_iso", "View Iso"),
    (
        Action::ToggleProjection,
        "toggle_projection",
        "Perspective / Parallel",
    ),
    (Action::Perspective, "perspective", "Perspective"),
    (Action::Parallel, "parallel", "Parallel"),
    (Action::Cancel, "cancel", "Cancel"),
    (Action::SelectTool, "select_tool", "Select"),
    (Action::PaintTool, "paint_tool", "Paint"),
    (Action::MarkEdgesTool, "mark_edges_tool", "Mark Edges"),
    (Action::LineStyleTool, "line_style_tool", "Line Style"),
    (Action::SelectAll, "select_all", "Select All"),
    (Action::SelectNone, "select_none", "Select None"),
    (Action::ToggleMode, "toggle_mode", "Edit / Present"),
    (Action::AxesTool, "axes_tool", "Axes"),
    (Action::ResetAxes, "reset_axes", "Reset Axes"),
    (Action::ViewPie, "view_pie", "View Pie"),
    (Action::Undo, "undo", "Undo"),
    (Action::Redo, "redo", "Redo"),
    (Action::AdjustLast, "adjust_last", "Adjust Last Operation"),
    (Action::LineTool, "line_tool", "Line"),
    (Action::RectangleTool, "rectangle_tool", "Rectangle"),
    (
        Action::RotatedRectangleTool,
        "rotated_rectangle_tool",
        "Rotated Rectangle",
    ),
    (Action::CircleTool, "circle_tool", "Circle"),
    (Action::ArcTool, "arc_tool", "Arc"),
    (Action::PolygonTool, "polygon_tool", "Polygon"),
    (Action::BezierTool, "bezier_tool", "Bézier Spline"),
    (Action::CurveMakerTool, "curve_maker_tool", "Curve Maker"),
    (Action::ExtrudeTool, "extrude_tool", "Extrude"),
    (Action::EraserTool, "eraser_tool", "Eraser"),
    (Action::MoveTool, "move_tool", "Move"),
    (Action::RotateTool, "rotate_tool", "Rotate"),
    (Action::ScaleTool, "scale_tool", "Scale"),
    (Action::MirrorTool, "mirror_tool", "Mirror"),
    (Action::OffsetTool, "offset_tool", "Offset (Inset)"),
    (Action::DivideTool, "divide_tool", "Divide"),
    (Action::LoopCutTool, "loop_cut_tool", "Loop Cut"),
    (Action::KnifeTool, "knife_tool", "Knife"),
    (Action::SlideTool, "slide_tool", "Slide"),
    (Action::DissolveTool, "dissolve_tool", "Dissolve"),
    (Action::TapeMeasureTool, "tape_measure_tool", "Tape Measure"),
    (Action::ProtractorTool, "protractor_tool", "Protractor"),
    (Action::FindCenterTool, "find_center_tool", "Find Center"),
    (Action::DeleteGuides, "delete_guides", "Delete Guides"),
    (Action::DimensionTool, "dimension_tool", "Dimensions"),
    (Action::TextTool, "text_tool", "Text"),
    (Action::SectionTool, "section_tool", "Section Plane"),
    (Action::SweepTool, "sweep_tool", "Extrude along Path"),
    (Action::RevolveTool, "revolve_tool", "Revolve"),
    (
        Action::ExtrudeEdgesTool,
        "extrude_edges_tool",
        "Extrude Edges along Path",
    ),
    (Action::BevelTool, "bevel_tool", "Bevel"),
    (Action::Fill, "fill", "Fill"),
    (Action::FuseTool, "fuse_tool", "Fuse"),
    (Action::WeldTool, "weld_tool", "Weld"),
    (Action::PercentTool, "percent_tool", "Point at Percentage"),
    (Action::SpaceTool, "space_tool", "Space"),
    (Action::CircleLoopTool, "circle_loop_tool", "Circle Loop"),
    (Action::FlattenTool, "flatten_tool", "Flatten"),
    (Action::SmoothTool, "smooth_tool", "Smooth"),
    (Action::RelaxTool, "relax_tool", "Relax"),
    (Action::IntersectTool, "intersect_tool", "Intersect Faces"),
    (Action::CleanUpTool, "clean_up_tool", "Clean Up"),
    (Action::CloseGapsTool, "close_gaps_tool", "Close Gaps"),
    (
        Action::InspectFacesTool,
        "inspect_faces_tool",
        "Inspect Faces",
    ),
    (
        Action::InspectManifoldTool,
        "inspect_manifold_tool",
        "Inspect Manifold",
    ),
    (
        Action::MakeManifoldTool,
        "make_manifold_tool",
        "Make Manifold",
    ),
    (Action::TrimTool, "trim_tool", "Trim / Extend"),
    (Action::BisectorTool, "bisector_tool", "Bisector"),
    (Action::ExtendFaceTool, "extend_face_tool", "Extend to Face"),
    (Action::BridgeTool, "bridge_tool", "Bridge"),
    (Action::LoftTool, "loft_tool", "Loft along Path"),
    (Action::LoftSplineTool, "loft_spline_tool", "Loft"),
    (Action::SkinTool, "skin_tool", "Skinning"),
    (
        Action::CopyAlongPathTool,
        "copy_along_path_tool",
        "Copy Along Path",
    ),
    (Action::ShapesTool, "shapes_tool", "Shapes"),
    (
        Action::CreateGroupFromSlice,
        "create_group_from_slice",
        "Create Group from Slice",
    ),
    (
        Action::ToggleProportional,
        "toggle_proportional",
        "Proportional Editing",
    ),
    (
        Action::ProportionalOn,
        "proportional_on",
        "Proportional Editing On",
    ),
    (
        Action::ProportionalOff,
        "proportional_off",
        "Proportional Editing Off",
    ),
    (Action::ReverseFaces, "reverse_faces", "Reverse Faces"),
    (Action::OrientFaces, "orient_faces", "Orient Faces"),
    (Action::Delete, "delete", "Delete"),
    (Action::Hide, "hide", "Hide"),
    (Action::UnhideLast, "unhide_last", "Unhide Last"),
    (Action::UnhideAll, "unhide_all", "Unhide All"),
    (
        Action::SelectConnected,
        "select_connected",
        "Select Connected",
    ),
    (Action::SelectByType, "select_by_type", "Select by Type"),
    (Action::SelectVertices, "select_vertices", "Select Vertices"),
    (Action::SelectEdges, "select_edges", "Select Edges"),
    (Action::SelectFaces, "select_faces", "Select Faces"),
    (
        Action::RepeatTransform,
        "repeat_transform",
        "Repeat Transform",
    ),
    (Action::Cut, "cut", "Cut"),
    (Action::Copy, "copy", "Copy"),
    (Action::Paste, "paste", "Paste"),
    (Action::ToggleSidebar, "toggle_sidebar", "Open / Hide Panel"),
    (Action::MakeGroup, "make_group", "Make Group"),
    (Action::Unhide, "unhide", "Unhide"),
    (Action::HideOthers, "hide_others", "Hide Others"),
    (Action::Explode, "explode", "Explode"),
    (Action::Lock, "lock", "Lock"),
    (Action::Unlock, "unlock", "Unlock"),
    (Action::MakeComponent, "make_component", "Make Component"),
    (Action::NewFile, "new_file", "New"),
    (Action::OpenFile, "open_file", "Open…"),
    (Action::SaveFile, "save_file", "Save"),
    (Action::SaveFileAs, "save_file_as", "Save As…"),
    (Action::SaveVersion, "save_version", "Save Version"),
    (Action::Quit, "quit", "Quit"),
];

/// A key held down while another is pressed, as a modifier is (a hold
/// chord: `Space+S`, `Tab+P`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HoldKey {
    /// The space bar.
    Space,
    /// The Tab key.
    Tab,
}

impl HoldKey {
    /// Both.
    pub const ALL: [HoldKey; 2] = [HoldKey::Space, HoldKey::Tab];

    /// The key it is.
    pub fn key(self) -> Key {
        match self {
            HoldKey::Space => Key::Space,
            HoldKey::Tab => Key::Tab,
        }
    }

    /// Its name in files and on screen.
    pub fn name(self) -> &'static str {
        match self {
            HoldKey::Space => "Space",
            HoldKey::Tab => "Tab",
        }
    }

    /// The hold key that is `key`, if any.
    pub fn of(key: Key) -> Option<Self> {
        Self::ALL.into_iter().find(|h| h.key() == key)
    }
}

/// A key plus modifiers, and a key held with it, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    /// Required modifiers (matched logically, so Ctrl and Cmd are equivalent).
    pub modifiers: Modifiers,
    /// The key.
    pub key: Key,
    /// Space or Tab held while the key is pressed.
    pub hold: Option<HoldKey>,
}

impl Chord {
    /// A plain chord (no held key).
    pub fn new(modifiers: Modifiers, key: Key) -> Self {
        Self {
            modifiers,
            key,
            hold: None,
        }
    }

    /// Parses `"Ctrl+Shift+Z"`, `"Home"`, `"7"`, `"Space+S"`, `"Tab+P"` and
    /// similar. A held key comes first and stands alone (`Space+Ctrl+S` is
    /// not a chord).
    pub fn parse(text: &str) -> Option<Self> {
        let mut modifiers = Modifiers::NONE;
        let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let key_name = parts.pop()?;
        let key = Key::from_name(key_name)?;
        if let [held] = parts.as_slice()
            && let Some(h) = HoldKey::ALL
                .into_iter()
                .find(|h| h.name().eq_ignore_ascii_case(held))
        {
            if HoldKey::of(key).is_some() {
                return None;
            }
            return Some(Self {
                modifiers,
                key,
                hold: Some(h),
            });
        }
        for m in parts {
            modifiers |= match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => Modifiers::COMMAND,
                "shift" => Modifiers::SHIFT,
                "alt" => Modifiers::ALT,
                _ => return None,
            };
        }
        Some(Self::new(modifiers, key))
    }

    /// Its name on screen and in files: `Ctrl+Shift+Z`, `Space+S`.
    pub fn label(self) -> String {
        let mut s = String::new();
        if let Some(h) = self.hold {
            s.push_str(h.name());
            s.push('+');
        }
        if self.modifiers.command || self.modifiers.ctrl {
            s.push_str("Ctrl+");
        }
        if self.modifiers.shift {
            s.push_str("Shift+");
        }
        if self.modifiers.alt {
            s.push_str("Alt+");
        }
        s.push_str(self.key.name());
        s
    }
}

#[derive(Debug, Deserialize)]
struct KeymapFile {
    #[serde(default)]
    name: Option<String>,
    /// The preset a Custom table was copied from.
    #[serde(default)]
    based_on: Option<Preset>,
    #[serde(default)]
    keys: BTreeMap<String, String>,
    #[serde(default)]
    taps: BTreeMap<String, String>,
}

/// A resolved keymap: action → chord, action → tap.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    /// Display name of the preset.
    pub name: String,
    /// Bindings, in action order.
    pub bindings: BTreeMap<Action, Chord>,
    /// Taps, in action order.
    pub taps: BTreeMap<Action, TapKey>,
    /// For Custom: the preset it was copied from (what Reset goes back to).
    pub based_on: Preset,
    /// Problems found while loading (unknown actions, unparsable keys).
    pub warnings: Vec<String>,
}

/// What a key or tap is already doing, found before it is given to another
/// action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// A chord.
    Key(Chord),
    /// A lone tap.
    Tap(TapKey),
}

impl Keymap {
    /// Loads a keymap: a preset's built-in table, or Custom from
    /// `config_dir` (a copy of SketchUp's when there is none yet). Problems
    /// are collected as warnings, never fatal.
    pub fn load(preset: Preset, config_dir: &Path) -> Self {
        if let Some(builtin) = preset.builtin() {
            let mut map = Self {
                based_on: preset,
                ..Self::default()
            };
            map.merge(builtin, "built-in");
            return map;
        }
        let path = config_dir.join(CUSTOM_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let mut map = Self::default();
                map.merge(&text, &path.display().to_string());
                map.name = Preset::Custom.label().into();
                map
            }
            Err(_) => Self::load(Preset::SketchSmith, config_dir).into_custom(),
        }
    }

    /// This keymap as a Custom table copied from it.
    pub fn into_custom(mut self) -> Self {
        self.name = Preset::Custom.label().into();
        self
    }

    /// Writes this keymap as the Custom table in `config_dir`: every action
    /// listed, unbound ones as `""`.
    ///
    /// # Errors
    /// When the file cannot be written.
    pub fn save_custom(&self, config_dir: &Path) -> std::io::Result<()> {
        let mut out = format!(
            "# SketchSmith — Custom keymap (written by Preferences → Keys)\n\nname = \"Custom\"\nbased_on = \"{}\"\n\n[keys]\n",
            self.based_on.stem()
        );
        for (action, name, _) in ACTIONS {
            let key = self.label(*action).unwrap_or_default();
            out.push_str(&format!("{name} = \"{key}\"\n"));
        }
        out.push_str("\n[taps]\n");
        for (action, name, _) in ACTIONS {
            if let Some(t) = self.taps.get(action) {
                out.push_str(&format!("{name} = \"{}\"\n", t.file_name()));
            }
        }
        std::fs::create_dir_all(config_dir)?;
        std::fs::write(config_dir.join(CUSTOM_FILE), out)
    }

    /// The action `binding` is given to, if any.
    pub fn holder(&self, binding: Binding) -> Option<Action> {
        match binding {
            Binding::Key(c) => self
                .bindings
                .iter()
                .find(|(_, x)| **x == c)
                .map(|(a, _)| *a),
            Binding::Tap(t) => self.taps.iter().find(|(_, x)| **x == t).map(|(a, _)| *a),
        }
    }

    /// Gives `action` the key or tap `binding` (`None` clears the key, for
    /// a `Key`, or the tap). Any other action holding it gets what `action`
    /// had instead (a swap: nothing is left with two actions).
    pub fn assign(&mut self, action: Action, binding: Binding) {
        match binding {
            Binding::Key(c) => {
                let mine = self.bindings.get(&action).copied();
                if let Some(other) = self.holder(binding).filter(|o| *o != action) {
                    match mine {
                        Some(m) => self.bindings.insert(other, m),
                        None => self.bindings.remove(&other),
                    };
                }
                self.bindings.insert(action, c);
            }
            Binding::Tap(t) => {
                let mine = self.taps.get(&action).copied();
                if let Some(other) = self.holder(binding).filter(|o| *o != action) {
                    match mine {
                        Some(m) => self.taps.insert(other, m),
                        None => self.taps.remove(&other),
                    };
                }
                self.taps.insert(action, t);
            }
        }
    }

    /// Takes `action`'s key (`tap`: its tap) away.
    pub fn clear(&mut self, action: Action, tap: bool) {
        if tap {
            self.taps.remove(&action);
        } else {
            self.bindings.remove(&action);
        }
    }

    /// Puts `action`'s key and tap back as in the preset this was copied
    /// from (taking them from any action that has them now).
    pub fn reset(&mut self, action: Action) {
        let base = Self::load(self.based_on, Path::new(""));
        self.clear(action, false);
        self.clear(action, true);
        if let Some(c) = base.bindings.get(&action) {
            self.assign(action, Binding::Key(*c));
        }
        if let Some(t) = base.taps.get(&action) {
            self.assign(action, Binding::Tap(*t));
        }
    }

    /// The action tapping `key` runs, if any.
    pub fn tap_action(&self, key: TapKey) -> Option<Action> {
        self.holder(Binding::Tap(key))
    }

    fn merge(&mut self, text: &str, source: &str) {
        let file: KeymapFile = match toml::from_str(text) {
            Ok(f) => f,
            Err(e) => {
                self.warnings.push(format!("{source}: {e}"));
                return;
            }
        };
        if let Some(name) = file.name {
            self.name = name;
        }
        if let Some(base) = file.based_on.filter(|b| *b != Preset::Custom) {
            self.based_on = base;
        }
        for (action_name, tap_text) in file.taps {
            let Some(action) = Action::from_name(&action_name) else {
                self.warnings
                    .push(format!("{source}: unknown action `{action_name}`"));
                continue;
            };
            if tap_text.trim().is_empty() {
                self.taps.remove(&action);
                continue;
            }
            match TapKey::parse(&tap_text) {
                Some(t) => {
                    self.taps.insert(action, t);
                }
                None => self
                    .warnings
                    .push(format!("{source}: cannot parse tap `{tap_text}`")),
            }
        }
        for (action_name, key_text) in file.keys {
            let Some(action) = Action::from_name(&action_name) else {
                self.warnings
                    .push(format!("{source}: unknown action `{action_name}`"));
                continue;
            };
            if key_text.trim().is_empty() {
                self.bindings.remove(&action);
                continue;
            }
            match Chord::parse(&key_text) {
                Some(chord) => {
                    self.bindings.insert(action, chord);
                }
                None => self
                    .warnings
                    .push(format!("{source}: cannot parse key `{key_text}`")),
            }
        }
    }

    /// Whether any binding holds `hold` (its letters are then read while it
    /// is down, and its own binding runs on release).
    pub fn holds(&self, hold: HoldKey) -> bool {
        self.bindings.values().any(|c| c.hold == Some(hold))
    }

    /// The action `key` pressed while `hold` is held runs, if any.
    pub fn hold_action(&self, hold: HoldKey, key: Key) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(_, c)| c.hold == Some(hold) && c.key == key)
            .map(|(a, _)| *a)
    }

    /// The action bound to `key` alone (no modifier, nothing held), if any.
    pub fn plain_action(&self, key: Key) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(_, c)| c.hold.is_none() && c.key == key && c.modifiers.is_none())
            .map(|(a, _)| *a)
    }

    /// Bindings ordered for matching: chords with more modifiers first, so
    /// `Shift+Z` is tried before `Z` and `Ctrl+Shift+Z` before `Ctrl+Z`.
    /// Hold chords are matched apart (before the toolkit sees the keys) and
    /// are not listed.
    pub fn ordered(&self) -> Vec<(Action, Chord)> {
        let mut list: Vec<(Action, Chord)> = self
            .bindings
            .iter()
            .filter(|(_, c)| c.hold.is_none())
            .map(|(a, c)| (*a, *c))
            .collect();
        let weight = |c: &Chord| {
            let m = c.modifiers;
            u8::from(m.command || m.ctrl) + u8::from(m.shift) + u8::from(m.alt)
        };
        list.sort_by_key(|(a, c)| (std::cmp::Reverse(weight(c)), *a));
        list
    }

    /// The chord bound to an action, for menus and tooltips.
    pub fn label(&self, action: Action) -> Option<String> {
        self.bindings.get(&action).map(|c| c.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_loads_cleanly_without_duplicate_chords() {
        for preset in [Preset::SketchSmith, Preset::SketchUp, Preset::Blender] {
            let map = Keymap::load(preset, Path::new("/nonexistent"));
            assert!(map.warnings.is_empty(), "{preset:?}: {:?}", map.warnings);
            assert!(!map.bindings.is_empty());
            let chords: Vec<_> = map.bindings.values().collect();
            for (i, a) in chords.iter().enumerate() {
                assert!(
                    !chords[i + 1..].contains(a),
                    "{preset:?}: duplicate chord {a:?}"
                );
            }
        }
    }

    #[test]
    fn chord_parsing() {
        let c = Chord::parse("Shift+Z").unwrap();
        assert_eq!(c.key, Key::Z);
        assert!(c.modifiers.shift);
        assert_eq!(Chord::parse("Ctrl+7").unwrap().key, Key::Num7);
        assert_eq!(Chord::parse("Home").unwrap().key, Key::Home);
        assert!(Chord::parse("Hyper+Z").is_none());
        assert!(Chord::parse("NotAKey").is_none());
    }

    #[test]
    fn custom_starts_as_sketchsmith_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("sketchsmith-keymap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // No Custom file yet: a copy of SketchSmith's, the default.
        let mut custom = Keymap::load(Preset::Custom, &dir);
        let ss = Keymap::load(Preset::SketchSmith, &dir);
        assert_eq!(custom.bindings, ss.bindings);
        assert_eq!(custom.taps, ss.taps);
        assert_eq!(custom.name, "Custom");
        // Change, save, load: the same table back.
        custom.assign(Action::OrbitTool, Binding::Key(Chord::parse("Q").unwrap()));
        custom.clear(Action::PanTool, false);
        custom.save_custom(&dir).unwrap();
        let back = Keymap::load(Preset::Custom, &dir);
        assert!(back.warnings.is_empty(), "{:?}", back.warnings);
        assert_eq!(back.bindings, custom.bindings);
        assert_eq!(back.taps, custom.taps);
        assert_eq!(back.bindings[&Action::OrbitTool].key, Key::Q);
        assert!(!back.bindings.contains_key(&Action::PanTool));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_preset_chord_survives_writing() {
        let dir = std::env::temp_dir().join(format!("sketchsmith-keymap-w-{}", std::process::id()));
        for preset in [Preset::SketchSmith, Preset::SketchUp, Preset::Blender] {
            let map = Keymap::load(preset, Path::new("/nonexistent")).into_custom();
            map.save_custom(&dir).unwrap();
            let back = Keymap::load(Preset::Custom, &dir);
            assert!(back.warnings.is_empty(), "{preset:?}: {:?}", back.warnings);
            assert_eq!(back.bindings, map.bindings, "{preset:?}");
            assert_eq!(back.based_on, preset);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn taps_open_the_pie_and_the_panel() {
        for preset in [Preset::SketchSmith, Preset::SketchUp, Preset::Blender] {
            let map = Keymap::load(preset, Path::new("/nonexistent"));
            assert_eq!(map.tap_action(TapKey::RightAlt), Some(Action::ViewPie));
            assert_eq!(
                map.tap_action(TapKey::RightCtrl),
                Some(Action::ToggleSidebar)
            );
        }
    }

    #[test]
    fn a_taken_key_is_swapped_and_reset_puts_it_back() {
        let mut map = Keymap::load(Preset::SketchUp, Path::new("/nonexistent")).into_custom();
        let line = map.bindings[&Action::LineTool];
        let rect = map.bindings[&Action::RectangleTool];
        map.assign(Action::LineTool, Binding::Key(rect));
        assert_eq!(map.bindings[&Action::LineTool], rect);
        assert_eq!(map.bindings[&Action::RectangleTool], line);
        map.assign(Action::ViewPie, Binding::Tap(TapKey::RightCtrl));
        assert_eq!(map.tap_action(TapKey::RightCtrl), Some(Action::ViewPie));
        assert_eq!(
            map.tap_action(TapKey::RightAlt),
            Some(Action::ToggleSidebar)
        );
        map.reset(Action::LineTool);
        assert_eq!(map.bindings[&Action::LineTool], line);
        assert_eq!(map.holder(Binding::Key(line)), Some(Action::LineTool));
    }

    #[test]
    fn a_clean_tap_counts_and_anything_else_spoils_it() {
        let mut t = TapDetector::default();
        t.key(Some(TapKey::RightAlt), true, false);
        assert_eq!(t.key(Some(TapKey::RightAlt), true, true), None, "repeat");
        assert_eq!(
            t.key(Some(TapKey::RightAlt), false, false),
            Some(TapKey::RightAlt)
        );
        t.key(Some(TapKey::RightCtrl), true, false);
        t.key(None, true, false);
        t.key(None, false, false);
        assert_eq!(t.key(Some(TapKey::RightCtrl), false, false), None);
        t.key(Some(TapKey::RightCtrl), true, false);
        t.interrupt();
        assert_eq!(t.key(Some(TapKey::RightCtrl), false, false), None);
    }

    #[test]
    fn every_action_is_listed_once() {
        let mut names: Vec<&str> = ACTIONS.iter().map(|(_, n, _)| *n).collect();
        let len = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), len);
        for (a, n, l) in ACTIONS {
            assert_eq!(Action::from_name(n), Some(*a));
            assert!(!l.is_empty());
        }
        assert_eq!(
            Action::from_name("push_pull_tool"),
            Some(Action::ExtrudeTool)
        );
    }

    #[test]
    fn specific_chords_are_matched_first() {
        for preset in [Preset::SketchSmith, Preset::SketchUp, Preset::Blender] {
            let map = Keymap::load(preset, Path::new("/nonexistent"));
            let order: Vec<Action> = map.ordered().into_iter().map(|(a, _)| a).collect();
            let pos = |a: Action| order.iter().position(|x| *x == a).unwrap();
            assert!(pos(Action::Redo) < pos(Action::Undo), "{preset:?}");
        }
        let su = Keymap::load(Preset::SketchUp, Path::new("/nonexistent"));
        let order: Vec<Action> = su.ordered().into_iter().map(|(a, _)| a).collect();
        let pos = |a: Action| order.iter().position(|x| *x == a).unwrap();
        assert!(
            pos(Action::ZoomExtents) < pos(Action::ZoomTool),
            "Shift+Z before Z"
        );
        let bl = Keymap::load(Preset::Blender, Path::new("/nonexistent"));
        assert_eq!(bl.label(Action::ArcTool).as_deref(), Some("Shift+A"));
        assert_eq!(bl.label(Action::RectangleTool).as_deref(), Some("Shift+R"));
    }

    #[test]
    fn hold_chords_parse_match_and_fall_back() {
        let c = Chord::parse("Space+S").unwrap();
        assert_eq!(c.hold, Some(HoldKey::Space));
        assert_eq!(c.key, Key::S);
        assert_eq!(c.label(), "Space+S");
        assert_eq!(Chord::parse("Tab+P").unwrap().hold, Some(HoldKey::Tab));
        let map = Keymap::load(Preset::SketchSmith, Path::new("/nonexistent"));
        assert!(map.holds(HoldKey::Space) && map.holds(HoldKey::Tab));
        assert_eq!(
            map.hold_action(HoldKey::Space, Key::S),
            Some(Action::SpaceTool)
        );
        assert_eq!(
            map.hold_action(HoldKey::Tab, Key::P),
            Some(Action::ProtractorTool)
        );
        assert_eq!(map.hold_action(HoldKey::Space, Key::Z), None);
        // Released alone: the plain binding.
        assert_eq!(map.plain_action(Key::Space), Some(Action::SelectTool));
        assert_eq!(map.plain_action(Key::Tab), Some(Action::ToggleMode));
        // Hold chords never match as plain keys.
        assert!(map.ordered().iter().all(|(_, c)| c.hold.is_none()));
        // No bare digit carries a command, so typed values stay values.
        let digits = [
            Key::Num0,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        assert!(
            map.ordered()
                .iter()
                .all(|(_, c)| { !digits.contains(&c.key) || c.modifiers.any() })
        );
        // Mutually exclusive pairs: Ctrl switches on, Shift switches off.
        assert_eq!(map.label(Action::ProportionalOn).as_deref(), Some("Ctrl+E"));
        assert_eq!(
            map.label(Action::ProportionalOff).as_deref(),
            Some("Shift+E")
        );
        assert_eq!(map.label(Action::ExtrudeTool).as_deref(), Some("X"));
    }

    #[test]
    fn labels() {
        let map = Keymap::load(Preset::SketchUp, Path::new("/nonexistent"));
        assert_eq!(map.label(Action::ZoomExtents).as_deref(), Some("Shift+Z"));
    }
}
