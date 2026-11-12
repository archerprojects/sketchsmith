//! SketchSmith — UI layout shell | v1.0.0 | Thursday 12 November 2026
//!
//! The application layout:
//!
//! * **Top bar** — rarely used menus (File, Edit, View, Window, Help), opening
//!   downward.
//! * **Bottom bar** — the work menus (Draw, Modify, Camera, Tools) opening
//!   upward, the status line, the Edit/Present switch, the Proportional /
//!   Snapping box.
//! * **Operations Bar** — a slim strip on the right or left window edge (View
//!   menu), in the canvas's background colour, with one tab per panel:
//!   Geometry, Items and Layers in Edit mode; Styles and Render in Present
//!   mode. Each mode remembers its own open tab. A tab opens its panel over the canvas edge when the pointer rests
//!   on it or it is clicked; one panel shows at a time.
//! * **Operation Inputs** — a fixed box in the canvas's bottom corner on the
//!   Operations Bar's side. **Save status** — a dot (red never saved, yellow unsaved
//!   changes, green saved), the file name and Save Version, at the right end
//!   of the top bar.
//!
//! Commands not built yet are shown greyed out, with the hover text "Not
//! available yet".
//!
//! The shell also holds: the draw tools in the Geometry panel, the Draw menu
//! and the keymaps; Undo / Redo; Operation Inputs; Edit → Adjust Last Operation (F9); customisable toolbars
//! (View → Customise Toolbars: drag buttons between groups or to the Hidden
//! tray, move groups up and down, float groups over the canvas as toolbars
//! and dock them back; each mode keeps its own arrangement in `settings.toml`
//! through `panel_layout`); File → New, Open, Save, Save As with the desktop's
//! file dialogs and the unsaved-changes prompt; backups kept when saving
//! over a file (Preferences → Save Versions, as Blender); Save Version
//! (`Ctrl+Alt+S`), which saves the next numbered file as Blender's Save
//! Incremental does; a prompt before saving over a file from a newer
//! SketchSmith; the file name with `•` in the
//! window title and the top bar; autosave at the interval set in
//! Window → Preferences (`autosave.rs`), the recovery prompt at start, and
//! File → Open Recent (`recent.rs`); File → Export, a format, then its
//! choices in a window over the canvas, then the save dialog; File → Import
//! the same way, with the open dialog and an Import Report.

use std::path::{Path, PathBuf};
use std::time::Instant;

use egui::{Align, Button, CentralPanel, Layout, Panel, Popup, RectAlign, ScrollArea, Ui, Vec2};
use smith_infer::infer::OnlyKind;
use smith_infer::units::{LengthUnit, format_area, format_length};
use smith_model::Document;
use smith_render::mesh::EdgeDisplay;
use smith_render::renderer::FaceStyle;

use crate::APP_NAME;
use crate::autosave::{self, Autosave, Recovery};
use crate::canvas::{Canvas, Dialog, Mode, NavTool, ViewPrefs, WorkTool};
use crate::files::{Discard, Files, Purpose, SaveStatus, Waker, next_version};
use crate::icons;
use crate::keymap::{Action, Binding, Chord, HoldKey, Keymap, Preset, TapKey};
use crate::panel_layout::{DefaultGroup, GroupLayout, PanelLayout};
use crate::paths::AppPaths;
use crate::recent::Recent;
use crate::scenes;
use crate::settings::{PanelSide, Settings};
use smith_io::export::{
    Background, Drawing, DrawingView, DxfContent, ExportOptions, FileUnit, Format, HiddenLines,
    MAX_SIDE, Measure, Orientation, Paper, PictureSize, RenderFormat, Scope, UnitPreset,
};
use smith_io::import::{ImportFormat, ImportOptions};
use smith_model::SelectKind;
use smith_ops::modify::Falloff;
use smith_tools::DrawKind;

/// One tool button: short label and full name.
struct ToolSlot {
    short: &'static str,
    name: &'static str,
}

/// Shorthand constructor for the static tool tables.
const fn tool(short: &'static str, name: &'static str) -> ToolSlot {
    ToolSlot { short, name }
}

/// A titled group of tool buttons in the Geometry panel.
struct ToolGroup {
    title: &'static str,
    tools: &'static [ToolSlot],
}

/// The display styles both modes offer, on the top bar (each mode keeps its
/// own choice).
const DISPLAY_STYLES: &[(&str, FaceStyle)] = &[
    ("Wireframe", FaceStyle::Wireframe),
    ("Hidden Line", FaceStyle::HiddenLine),
    ("Shaded", FaceStyle::Shaded),
    ("Monochrome", FaceStyle::Monochrome),
    ("X-Ray", FaceStyle::XRay),
];

/// The Select row, repeated at the top of the Items panel.
const SELECT_ROW: &[&str] = &[
    "Select",
    "Select Vertices",
    "Select Edges",
    "Select Faces",
    "Select All",
    "Select None",
    "Select Connected",
    "Select by Type",
    "Cut",
    "Copy",
    "Paste",
    "Undo",
    "Redo",
];

/// The select mode's number in the settings ([`SelectKind::ALL`] order).
fn select_mode_number(kind: SelectKind) -> u8 {
    match kind {
        SelectKind::Vertices => 0,
        SelectKind::Edges => 1,
        SelectKind::Faces => 2,
    }
}

/// The Select row's buttons Present mode leaves out: they change geometry.
const EDIT_ONLY: &[&str] = &["Cut", "Copy", "Paste"];

/// The select-mode icons in the Select row.
const SELECT_MODES: &[&str] = &["Select Vertices", "Select Edges", "Select Faces"];

/// Edit-mode panel groups.
const EDIT_GROUPS: &[ToolGroup] = &[
    ToolGroup {
        title: "Select",
        tools: &[
            tool("Sel", "Select"),
            tool("SV", "Select Vertices"),
            tool("SE", "Select Edges"),
            tool("SF", "Select Faces"),
            tool("SA", "Select All"),
            tool("SN", "Select None"),
            tool("SC", "Select Connected"),
            tool("ST", "Select by Type"),
            tool("Ct", "Cut"),
            tool("Cy", "Copy"),
            tool("Pa", "Paste"),
            tool("Un", "Undo"),
            tool("Re", "Redo"),
        ],
    },
    ToolGroup {
        title: "Draw",
        tools: &[
            tool("Ln", "Line"),
            tool("Rc", "Rectangle"),
            tool("RR", "Rotated Rectangle"),
            tool("Ci", "Circle"),
            tool("Ar", "Arc"),
            tool("Pg", "Polygon"),
            tool("Bz", "Bézier Spline"),
            tool("CM", "Curve Maker"),
            tool("Sh", "Shapes"),
        ],
    },
    ToolGroup {
        title: "Modify",
        tools: &[
            tool("Mv", "Move"),
            tool("Ro", "Rotate"),
            tool("Sc", "Scale"),
            tool("Of", "Offset (Inset)"),
            tool("Er", "Eraser"),
            tool("Mi", "Mirror"),
            tool("LC", "Loop Cut"),
            tool("Kn", "Knife"),
            tool("Sl", "Slide"),
            tool("Ds", "Dissolve"),
            tool("Dv", "Divide"),
            tool("CP", "Copy Along Path"),
            tool("Bv", "Bevel"),
            tool("Fi", "Fill"),
            tool("Fu", "Fuse"),
            tool("Wd", "Weld"),
            tool("TE", "Trim / Extend"),
            tool("XF", "Extend to Face"),
        ],
    },
    ToolGroup {
        title: "Tweak",
        tools: &[
            tool("Sp", "Space"),
            tool("CL", "Circle Loop"),
            tool("Fl", "Flatten"),
            tool("Sm", "Smooth"),
            tool("Rx", "Relax"),
        ],
    },
    ToolGroup {
        title: "Repair",
        tools: &[
            tool("IM", "Inspect Manifold"),
            tool("IFc", "Inspect Faces"),
            tool("IF", "Intersect Faces"),
            tool("CU", "Clean Up"),
            tool("CG", "Close Gaps"),
            tool("MM", "Make Manifold"),
        ],
    },
    ToolGroup {
        title: "Generate",
        tools: &[
            tool("PP", "Extrude"),
            tool("EP", "Extrude along Path"),
            tool("EE", "Extrude Edges along Path"),
            tool("Rv", "Revolve"),
            tool("Lf", "Loft"),
            tool("LP", "Loft along Path"),
            tool("Sk", "Skinning"),
            tool("Br", "Bridge"),
        ],
    },
    ToolGroup {
        title: "Measure",
        tools: &[
            tool("Tp", "Tape Measure"),
            tool("Pr", "Protractor"),
            tool("FC", "Find Center"),
            tool("P%", "Point at Percentage"),
            tool("Bi", "Bisector"),
            tool("Dm", "Dimensions"),
            tool("Tx", "Text"),
            tool("SP", "Section Plane"),
            tool("Ax", "Axes"),
        ],
    },
    ToolGroup {
        title: "Camera",
        tools: &[
            tool("Ob", "Orbit"),
            tool("Pn", "Pan"),
            tool("Zm", "Zoom"),
            tool("ZE", "Zoom Extents"),
        ],
    },
];

/// Present-mode panel groups.
const PRESENT_GROUPS: &[ToolGroup] = &[
    ToolGroup {
        title: "Paint",
        tools: &[tool("Pt", "Paint")],
    },
    ToolGroup {
        title: "Marks",
        tools: &[tool("ME", "Mark Edges"), tool("LS", "Line Style")],
    },
    ToolGroup {
        title: "Section",
        tools: &[tool("SP", "Section Plane")],
    },
    ToolGroup {
        title: "Camera",
        tools: &[
            tool("Ob", "Orbit"),
            tool("Pn", "Pan"),
            tool("Zm", "Zoom"),
            tool("ZE", "Zoom Extents"),
        ],
    },
    ToolGroup {
        title: "Render",
        tools: &[tool("+R", "Save Render")],
    },
];

/// Side length of a panel tool button, in points.
const TOOL_BUTTON_SIZE: f32 = 34.0;
/// Where a newly floated toolbar appears (points), staggered per group.
const FLOAT_X: f32 = 120.0;
/// See [`FLOAT_X`].
const FLOAT_Y: f32 = 80.0;
/// Offset between newly floated toolbars, per group index.
const FLOAT_STAGGER: f32 = 24.0;
/// Width of a floating toolbar, in points.
const FLOAT_WIDTH: f32 = 200.0;

/// UI state for the layout shell.
pub struct Shell {
    paths: AppPaths,
    settings: Settings,
    keymap: Keymap,
    canvas: Canvas,
    show_preferences: bool,
    show_about: bool,
    quit_requested: bool,
    /// Actions taken from raw input before egui saw it (see `intercept_keys`).
    queued: Vec<Action>,
    /// The action waiting for a key to be pressed (Preferences → Keys).
    key_capture: Option<Action>,
    /// Space or Tab held down for a hold chord, and whether a key was
    /// pressed while it was held.
    held: Option<(HoldKey, bool)>,
    /// A key or tap another action has: the action, the binding, its holder.
    key_conflict: Option<(Action, Binding, Action)>,
    /// Panel arrangements: Edit mode, then Present mode.
    layouts: [PanelLayout; 2],
    /// View → Customise Toolbars is on.
    customising: bool,
    /// The open file and its saved state.
    files: Files,
    /// This session's autosave copy.
    autosave: Autosave,
    /// Model state (revision, axes count) when last autosaved.
    autosaved: Option<(u64, u64)>,
    /// When the next autosave is due, once the model has changed.
    autosave_due: Option<Instant>,
    /// Work left by sessions that crashed, waiting for the recovery prompt.
    recover: Vec<Recovery>,
    /// File → Open Recent.
    recent: Recent,
    /// The Operations Bar's rectangle this frame.
    strip: egui::Rect,
    /// Each tab's rectangle and label this frame, in tab order.
    tab_layout: Vec<(egui::Rect, std::sync::Arc<egui::Galley>)>,
    /// The tab whose panel is showing or sliding away.
    shown_tab: u8,
    /// The tab under the pointer and since when (seconds), for opening on
    /// a resting pointer.
    tab_hover: Option<(u8, f64)>,
    /// When a tab was last opened (seconds).
    tab_opened_at: f64,
    /// The scene being renamed, and the name typed so far.
    renaming_scene: Option<(smith_model::SceneId, String)>,
    /// The import window, while it is open: the format and the choices.
    import: Option<(ImportFormat, ImportOptions)>,
    /// A `.glb` another program made, met by Open: asks to import it.
    foreign: Option<PathBuf>,
    /// The export window, while it is open.
    export: Option<ExportWindow>,
}

/// File → Export's window: the format, the choices so far, and how many
/// faces with holes OBJ would triangulate (whole model, then selection).
struct ExportWindow {
    format: Format,
    options: ExportOptions,
    holed: [usize; 2],
    /// Export Render: the saved Render chosen (`None`: a plain export).
    render: Option<smith_model::SceneId>,
}

/// What is dragged while customising: a tool, by name.
#[derive(Debug, Clone)]
struct DragTool(String);

/// A change to a panel layout, collected while drawing and applied after.
enum PanelChange {
    /// Move a tool into a group, before a button (or at the end).
    Move(String, usize, Option<usize>),
    /// Hide a tool.
    Hide(String),
    /// Move a group up (`-1`) or down (`+1`).
    Shift(usize, isize),
    /// Float a group at a position, or dock it back in the panel.
    Float(usize, Option<[f32; 2]>),
}

impl Shell {
    /// Creates the shell. `render_state` is the GPU context for the 3D canvas;
    /// `window` parents the file dialogs and `wake` wakes the event loop when
    /// one closes. `open` is a file named on the command line.
    pub fn new(
        paths: AppPaths,
        render_state: Option<egui_wgpu::RenderState>,
        window: Option<std::sync::Arc<winit::window::Window>>,
        wake: Waker,
        open: Option<PathBuf>,
    ) -> Self {
        let settings = Settings::load(&paths.config);
        let keymap = Keymap::load(settings.keymap, &paths.config);
        for w in &keymap.warnings {
            log::warn!("keymap: {w}");
        }
        let layouts = [Mode::Edit, Mode::Present].map(|mode| {
            let saved = match mode {
                Mode::Edit => settings.panel_edit.clone(),
                Mode::Present => settings.panel_present.clone(),
            };
            with_defaults(mode, |d| {
                let mut l = saved.unwrap_or_else(|| PanelLayout::from_defaults(d));
                l.reconcile(d);
                l
            })
        });
        let paths_state = paths.state.clone();
        let mut shell = Self {
            paths,
            settings,
            keymap,
            canvas: Canvas::new(render_state),
            show_preferences: false,
            show_about: false,
            quit_requested: false,
            queued: Vec::new(),
            key_capture: None,
            held: None,
            key_conflict: None,
            layouts,
            customising: false,
            files: Files::new(window, wake),
            autosave: Autosave::start(&paths_state),
            autosaved: None,
            autosave_due: None,
            recover: Vec::new(),
            recent: Recent::load(&paths_state),
            strip: egui::Rect::NOTHING,
            tab_layout: Vec::new(),
            shown_tab: 0,
            tab_hover: None,
            tab_opened_at: f64::NEG_INFINITY,
            renaming_scene: None,
            export: None,
            import: None,
            foreign: None,
        };
        if let Some(path) = open {
            shell.open_path(&path);
        }
        shell.recover = shell.autosave.crashed();
        let (on, p) = shell.saved_proportional();
        shell.canvas.set_proportional(on, p);
        let kind = SelectKind::ALL
            .get(usize::from(shell.settings.select_mode))
            .copied()
            .unwrap_or_default();
        shell.canvas.set_select_kind(kind);
        shell
    }

    /// Proportional editing as saved in the settings.
    fn saved_proportional(&self) -> (bool, smith_ops::modify::Proportional) {
        let falloff = match self.settings.proportional_falloff {
            1 => Falloff::Linear,
            2 => Falloff::Sharp,
            _ => Falloff::Smooth,
        };
        (
            self.settings.proportional,
            smith_ops::modify::Proportional {
                radius: self.settings.proportional_radius,
                falloff,
            },
        )
    }

    /// Saves the select mode when it changed.
    fn keep_select_kind(&mut self) {
        let now = select_mode_number(self.canvas.select_kind());
        if now != self.settings.select_mode {
            self.settings.select_mode = now;
            self.settings.save(&self.paths.config);
        }
    }

    /// Saves proportional editing when it changed (in the Snap box or by key).
    fn keep_proportional(&mut self) {
        let now = self.canvas.proportional();
        if now != self.saved_proportional() {
            let (on, p) = now;
            self.settings.proportional = on;
            self.settings.proportional_radius = p.radius;
            self.settings.proportional_falloff = match p.falloff {
                Falloff::Smooth => 0,
                Falloff::Linear => 1,
                Falloff::Sharp => 2,
            };
            self.settings.save(&self.paths.config);
        }
    }

    // ----- files -------------------------------------------------------------

    /// Whether the model differs from its file.
    fn dirty(&self) -> bool {
        self.files
            .dirty(self.canvas.revision(), self.canvas.axes_serial())
    }

    /// The window title: file name, `•` for unsaved changes, app name.
    pub fn window_title(&self) -> String {
        self.files.title(self.dirty())
    }

    /// Closing the window: asks about unsaved changes first.
    pub fn request_close(&mut self) {
        self.request(Discard::Quit);
    }

    /// Runs an action that replaces the model, asking first if there are
    /// unsaved changes. Ignored while a file dialog is open.
    fn request(&mut self, action: Discard) {
        if self.files.busy() || self.files.confirm.is_some() {
            return;
        }
        if self.dirty() {
            self.files.confirm = Some(action);
        } else {
            self.carry_out(action);
        }
    }

    /// Runs an action that replaces the model (no questions asked).
    fn carry_out(&mut self, action: Discard) {
        match action {
            Discard::New => {
                self.canvas.load(Document::new(), None, None);
                self.reset_file(None);
            }
            Discard::Open => self.files.start_dialog(Purpose::Open),
            Discard::OpenPath(path) => self.open_path(&path),
            Discard::Quit => self.quit_requested = true,
            Discard::TestScene => match scenes::test_scene() {
                Ok(doc) => {
                    self.canvas.load(doc, None, None);
                    self.reset_file(None);
                }
                Err(e) => log::error!("test scene: {e}"),
            },
            Discard::StressScene => {
                self.canvas.set_render_only(scenes::stress_mesh(100_000));
                self.reset_file(None);
            }
        }
    }

    /// Points the file state at `path` with the current model as saved. The
    /// autosave copy, if any, is no longer needed.
    fn reset_file(&mut self, path: Option<PathBuf>) {
        self.files.path = path;
        self.files.name = None;
        self.files.newer = false;
        self.files
            .mark_saved(self.canvas.revision(), self.canvas.axes_serial());
        self.autosave.clear();
        self.autosaved = None;
        self.autosave_due = None;
    }

    /// The model state autosave and the unsaved-changes dot follow.
    fn model_state(&self) -> (u64, u64) {
        (self.canvas.revision(), self.canvas.axes_serial())
    }

    /// Writes this session's autosave copy now.
    fn autosave_now(&mut self) {
        let view = self.canvas.saved_view();
        let axes = self.canvas.saved_axes();
        if self.autosave.write(
            self.canvas.document(),
            Some(&view),
            axes.as_ref(),
            self.canvas.mode() == Mode::Present,
            self.files.path.as_deref(),
        ) {
            self.autosaved = Some(self.model_state());
        }
        self.autosave_due = None;
    }

    /// Autosave: once the model has unsaved changes not yet copied, a copy is
    /// due one interval later; changes in between do not postpone it. With no
    /// unsaved changes (saved, or undone back to the saved state) there is no
    /// copy. Asks egui to wake the loop when the copy falls due.
    fn autosave_tick(&mut self, ctx: &egui::Context) {
        if !self.dirty() {
            if self.autosaved.is_some() {
                self.autosave.clear();
                self.autosaved = None;
            }
            self.autosave_due = None;
            return;
        }
        if self.autosaved == Some(self.model_state()) {
            self.autosave_due = None;
            return;
        }
        let Some(every) = autosave::interval(self.settings.autosave_minutes) else {
            self.autosave_due = None;
            return;
        };
        let now = Instant::now();
        let due = *self.autosave_due.get_or_insert(now + every);
        if now >= due {
            self.autosave_now();
        } else {
            ctx.request_repaint_after(due - now);
        }
    }

    /// Restores the newest crashed session's work as the open model. It is
    /// marked unsaved (it differs from any file) and copied again at once, so
    /// it is never unprotected; the old copy is then deleted.
    fn recover_first(&mut self) {
        let r = self.recover.remove(0);
        match smith_io::open(&r.model) {
            Ok(opened) => {
                let f = opened.file;
                self.canvas.load(f.document, f.view, f.axes);
                // A file reopens in the mode it was saved in.
                self.canvas
                    .set_mode(if f.present { Mode::Present } else { Mode::Edit });
                self.reset_file(r.meta.source.clone());
                self.files.mark_saved(u64::MAX, u64::MAX);
                self.autosave_now();
                self.autosave.forget(&r);
                log::info!("recovered work from {}", r.model.display());
            }
            Err(e) => self.files.notify(
                "Couldn't recover",
                format!(
                    "The copy of “{}” could not be read and is kept for now.\n\n{e}",
                    r.display_name()
                ),
            ),
        }
    }

    /// Save: to the open file, or through Save As when there is none. A file
    /// from a newer SketchSmith asks first. `then` runs after a successful save.
    fn save(&mut self, then: Option<Discard>) {
        match self.files.path.clone() {
            Some(path) => self.save_over(path, then),
            None => self.files.start_dialog(Purpose::SaveAs(then)),
        }
    }

    /// Saves to `path`, asking first when it is the open file and a newer
    /// SketchSmith wrote it. `then` runs after a successful save.
    fn save_over(&mut self, path: PathBuf, then: Option<Discard>) {
        if self.files.newer && self.files.path.as_deref() == Some(path.as_path()) {
            self.files.newer_prompt = Some((path, then));
            return;
        }
        self.save_now(&path, then);
    }

    /// Writes the model to `path`, then carries on with `then`.
    fn save_now(&mut self, path: &Path, then: Option<Discard>) {
        if self.write_to(path)
            && let Some(action) = then
        {
            self.carry_out(action);
        }
    }

    /// Save Version: saves the next numbered file beside the open one (as
    /// Blender's Save Incremental) and carries on in it. A model never saved
    /// goes through Save As.
    fn save_version(&mut self) {
        let Some(path) = self.files.path.clone() else {
            self.files.start_dialog(Purpose::SaveAs(None));
            return;
        };
        match next_version(&path) {
            Some(next) => {
                self.write_to(&next);
            }
            None => self.files.notify(
                "Couldn't save version",
                format!("No free numbered name was found for {}.", path.display()),
            ),
        }
    }

    /// Writes the model to `path`. Returns whether it worked; a failure is
    /// shown to the user and the model stays unsaved.
    fn write_to(&mut self, path: &Path) -> bool {
        let view = self.canvas.saved_view();
        let axes = self.canvas.saved_axes();
        match smith_io::save_parts_with_backups(
            path,
            self.canvas.document(),
            Some(&view),
            axes.as_ref(),
            self.canvas.mode() == Mode::Present,
            self.settings.save_versions,
        ) {
            Ok(()) => {
                self.reset_file(Some(path.to_path_buf()));
                self.recent.touch(path);
                self.recent.save(&self.paths.state);
                log::info!("saved {}", path.display());
                true
            }
            Err(e) => {
                self.files.notify(
                    "Couldn't save",
                    format!("{} could not be saved.\n\n{e}", path.display()),
                );
                false
            }
        }
    }

    /// Opens `path`, replacing the model. Errors and warnings are shown.
    fn open_path(&mut self, path: &Path) {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("skp"))
        {
            self.open_skp(path);
            return;
        }
        match smith_io::open(path) {
            Ok(opened) => {
                let f = opened.file;
                self.canvas.load(f.document, f.view, f.axes);
                // A file reopens in the mode it was saved in.
                self.canvas
                    .set_mode(if f.present { Mode::Present } else { Mode::Edit });
                self.reset_file(Some(path.to_path_buf()));
                self.files.newer = opened.newer;
                if !opened.warnings.is_empty() {
                    self.files
                        .notify("Opened with warnings", opened.warnings.join("\n\n"));
                }
                self.recent.touch(path);
                self.recent.save(&self.paths.state);
                log::info!("opened {}", path.display());
            }
            // Another program's .glb is offered as an import instead.
            Err(smith_io::IoError::Foreign) => self.foreign = Some(path.to_path_buf()),
            Err(e) => {
                if !path.exists() {
                    self.recent.remove(path);
                    self.recent.save(&self.paths.state);
                }
                self.files.notify(
                    "Couldn't open",
                    format!("{} could not be opened.\n\n{e}", path.display()),
                );
            }
        }
    }

    /// Opens an SKP as a new, unsaved model named after it, imported with
    /// the SKP import's last choices (Save then asks for a `.glb`). When it
    /// cannot be read, the model stays as it was and the bottom line says
    /// why.
    fn open_skp(&mut self, path: &Path) {
        let options = self.settings.imports.get(ImportFormat::Skp);
        let data = match smith_io::import::read_import(path, ImportFormat::Skp, &options) {
            Ok(data) => data,
            Err(e) => {
                if !path.exists() {
                    self.recent.remove(path);
                    self.recent.save(&self.paths.state);
                }
                self.canvas.set_message(format!("Couldn't Import: {e}"));
                return;
            }
        };
        self.canvas.load(Document::new(), None, None);
        self.canvas.set_mode(Mode::Edit);
        self.reset_file(None);
        self.files.name = Some(data.name.clone());
        self.place_import(path, &data);
        self.recent.touch(path);
        self.recent.save(&self.paths.state);
    }

    /// File keys and menu items (routed through the canvas's actions), and
    /// the answer from a file dialog that has closed.
    fn file_events(&mut self, ctx: &egui::Context) {
        for action in self.canvas.take_file_actions() {
            match action {
                Action::NewFile => self.request(Discard::New),
                Action::OpenFile => self.request(Discard::Open),
                Action::SaveFile if !self.files.busy() => self.save(None),
                Action::SaveFileAs if !self.files.busy() => {
                    self.files.start_dialog(Purpose::SaveAs(None));
                }
                Action::SaveVersion if !self.files.busy() => self.save_version(),
                Action::Quit => self.request(Discard::Quit),
                _ => {}
            }
        }
        if let Some((purpose, answer)) = self.files.poll() {
            match (purpose, answer) {
                (Purpose::Open, Some(path)) => self.open_path(&path),
                (Purpose::LoadImage, Some(path)) => self.load_image(&path),
                (Purpose::SaveAs(then), Some(path)) => self.save_over(path, then),
                (Purpose::Export(format, options), Some(path)) => {
                    self.export_to(&path, format, &options, ctx);
                }
                (Purpose::ExportRender(id, format, options, _), Some(path)) => {
                    self.export_render(&path, id, format, &options, ctx);
                }
                (Purpose::Import(format, options), Some(path)) => {
                    self.import_from(&path, format, &options);
                }
                // Cancelled: nothing changes (a pending New / Open / Quit is dropped).
                (_, None) => {}
            }
        }
    }

    /// File → Open Recent: the last files opened or saved, newest first.
    fn recent_menu(&mut self, ui: &mut Ui, idle: bool) {
        ui.add_enabled_ui(idle && !self.recent.files.is_empty(), |ui| {
            ui.menu_button("Open Recent", |ui| {
                let mut chosen = None;
                for path in &self.recent.files {
                    let name = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |n| n.to_string_lossy().into(),
                    );
                    if ui
                        .button(name)
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        chosen = Some(path.clone());
                    }
                }
                ui.separator();
                if ui.button("Clear Recent Files").clicked() {
                    self.recent.files.clear();
                    self.recent.save(&self.paths.state);
                }
                if let Some(path) = chosen {
                    ui.close();
                    self.request(Discard::OpenPath(path));
                }
            });
        });
    }

    /// Whether a file prompt or message is over the canvas.
    fn file_window_open(&self) -> bool {
        self.files.confirm.is_some()
            || !self.recover.is_empty()
            || self.files.newer_prompt.is_some()
            || self.files.notice.is_some()
            || self.export.is_some()
            || self.import.is_some()
            || self.foreign.is_some()
    }

    /// The unsaved-changes prompt, the recovery prompt and the message window.
    fn file_windows(&mut self, ui: &mut Ui) {
        if self.files.confirm.is_none()
            && let Some(r) = self.recover.first()
        {
            let (name, age) = (r.display_name(), r.age());
            let more = self.recover.len() - 1;
            let mut choice = None;
            egui::Modal::new(egui::Id::new("recover_work")).show(ui.ctx(), |ui| {
                ui.set_max_width(400.0);
                ui.heading("Recover unsaved work?");
                ui.add_space(6.0);
                ui.label(format!(
                    "{APP_NAME} closed without saving “{name}”. A copy from {age} was kept."
                ));
                if more > 0 {
                    ui.weak(format!("{more} more copy waiting after this one."));
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Recover").clicked() {
                        choice = Some(0);
                    }
                    if ui.button("Discard").clicked() {
                        choice = Some(1);
                    }
                    if ui.button("Later").clicked() {
                        choice = Some(2);
                    }
                });
            });
            match choice {
                Some(0) => self.recover_first(),
                Some(1) => {
                    let r = self.recover.remove(0);
                    self.autosave.forget(&r);
                }
                Some(_) => {
                    self.recover.remove(0);
                }
                None => {}
            }
        }
        if self.files.newer_prompt.is_some() {
            let mut choice = None;
            egui::Modal::new(egui::Id::new("newer_version")).show(ui.ctx(), |ui| {
                ui.set_max_width(400.0);
                ui.label("THIS FILE WAS CREATED BY A NEWER VERSION OF SKETCHSMITH. Saving it with your current version may remove some features designed by the newer version.");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            });
            if let Some(save) = choice
                && let Some((path, then)) = self.files.newer_prompt.take()
                && save
            {
                self.save_now(&path, then);
            }
        }
        if let Some(action) = self.files.confirm.clone() {
            let name = self.files.display_name();
            let mut choice = None;
            let modal = egui::Modal::new(egui::Id::new("unsaved_changes")).show(ui.ctx(), |ui| {
                ui.set_max_width(380.0);
                ui.heading("Save changes?");
                ui.add_space(6.0);
                ui.label(format!(
                    "“{name}” has unsaved changes. Save them before {}?",
                    action.verb()
                ));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        choice = Some(0);
                    }
                    if ui.button("Don't Save").clicked() {
                        choice = Some(1);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(2);
                    }
                });
            });
            if modal.should_close() && choice.is_none() {
                choice = Some(2);
            }
            if let Some(c) = choice {
                self.files.confirm = None;
                match c {
                    0 => self.save(Some(action)),
                    1 => self.carry_out(action),
                    _ => {}
                }
            }
        }
        if let Some(notice) = &self.files.notice {
            let (title, text) = (notice.title.clone(), notice.text.clone());
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::new("file_notice")).show(ui.ctx(), |ui| {
                ui.set_max_width(420.0);
                ui.heading(title);
                ui.add_space(6.0);
                ui.label(text);
                ui.add_space(10.0);
                close = ui.button("OK").clicked();
            });
            if close || modal.should_close() {
                self.files.notice = None;
            }
        }
        self.export_window(ui);
        self.import_window(ui);
        self.foreign_window(ui);
    }

    /// Open met a `.glb` another program made: Yes imports it into this
    /// model as meshes, with the glTF import's last choices.
    fn foreign_window(&mut self, ui: &mut Ui) {
        let Some(path) = &self.foreign else {
            return;
        };
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("foreign_glb")).show(ui.ctx(), |ui| {
            ui.set_max_width(400.0);
            ui.heading("Made by Another Program");
            ui.add_space(6.0);
            ui.label(format!(
                "{name} was made by another program. Import it into this model as meshes?"
            ));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Yes").clicked() {
                    choice = Some(true);
                }
                if ui.button("No").clicked() {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        if let Some(yes) = choice
            && let Some(path) = self.foreign.take()
            && yes
        {
            let options = self.settings.imports.get(ImportFormat::Gltf);
            self.import_from(&path, ImportFormat::Gltf, &options);
        }
    }

    /// The import window: the format's choices, then Import (on to the open
    /// dialog) or Cancel.
    fn import_window(&mut self, ui: &mut Ui) {
        let Some((format, o)) = self.import.as_mut() else {
            return;
        };
        let format = *format;
        let mut choice = None;
        egui::Modal::new(egui::Id::new("import_options")).show(ui.ctx(), |ui| {
            ui.set_max_width(340.0);
            ui.heading(format!("Import {}", format.name()));
            ui.add_space(6.0);
            // A file naming its own unit offers From File first.
            let own = format.has_own_units();
            let from_file = own && o.from_file;
            ui.horizontal(|ui| {
                ui.label("Units");
                egui::ComboBox::from_id_salt("import_units")
                    .selected_text(if from_file {
                        "From File"
                    } else {
                        o.units.name()
                    })
                    .show_ui(ui, |ui| {
                        if own && ui.selectable_label(from_file, "From File").clicked() {
                            o.from_file = true;
                        }
                        // A drawing offers its own size or Custom only.
                        for u in UnitPreset::ALL {
                            if format.file_or_custom() && u != UnitPreset::Custom {
                                continue;
                            }
                            if ui
                                .selectable_label(!from_file && o.units == u, u.name())
                                .clicked()
                            {
                                o.units = u;
                                o.from_file = false;
                            }
                        }
                    });
            });
            if !from_file && o.units == UnitPreset::Custom {
                ui.horizontal(|ui| {
                    ui.label("Unit");
                    egui::ComboBox::from_id_salt("import_unit")
                        .selected_text(o.unit.name())
                        .show_ui(ui, |ui| {
                            for u in FileUnit::ALL {
                                ui.selectable_value(&mut o.unit, u, u.name());
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Scale 1:");
                    ui.add(
                        egui::DragValue::new(&mut o.scale)
                            .range(0.001..=100_000.0)
                            .speed(1.0),
                    );
                    if o.scale > 0.0 && o.scale < 1.0 {
                        ui.weak(format!("({}:1)", ratio(1.0 / o.scale)));
                    }
                });
            }
            if format.has_shells() {
                ui.checkbox(&mut o.separate_shells, "Separate Shells");
            }
            if format.is_mesh() {
                ui.checkbox(&mut o.merge_coplanar, "Merge Coplanar Faces");
                ui.checkbox(&mut o.soften, "Soften Curved Surfaces");
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Import").clicked() {
                    choice = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(false);
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                choice = Some(false);
            }
        });
        let Some(go) = choice else {
            return;
        };
        let Some((format, options)) = self.import.take() else {
            return;
        };
        if self.settings.imports.get(format) != options {
            self.settings.imports.set(format, options);
            self.settings.save(&self.paths.config);
        }
        if go {
            self.files.start_dialog(Purpose::Import(format, options));
        }
    }

    /// Imports the file at `path`: one group, selected, one Undo step;
    /// the result on the bottom line and, when there is something to tell,
    /// the Import Report.
    fn import_from(&mut self, path: &Path, format: ImportFormat, options: &ImportOptions) {
        match smith_io::import::read_import(path, format, options) {
            Ok(data) => self.place_import(path, &data),
            Err(e) => self.canvas.set_message(format!("Couldn't Import: {e}")),
        }
    }

    /// Places `data`, read from `path`, as [`Shell::import_from`] says.
    fn place_import(&mut self, path: &Path, data: &smith_io::import::Imported) {
        let result = self
            .canvas
            .add_import(|doc, ctx, at| smith_io::import::place(doc, ctx, at, data))
            .map_err(|e| e.to_string());
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        match result {
            Ok(report) => {
                self.canvas.set_message(report.summary(&name));
                let lines = report.lines();
                if !lines.is_empty() {
                    self.files.notify("Import Report", lines.join("\n"));
                }
                log::info!("imported {}", path.display());
            }
            Err(e) => self.canvas.set_message(format!("Couldn't Import: {e}")),
        }
    }

    /// File → Export → a format: opens its window with the last choices.
    fn open_export(&mut self, format: Format) {
        let holed = if format == Format::Obj {
            let doc = self.canvas.document();
            [
                smith_io::export::gather(doc, Scope::Model).holed_faces(),
                self.export_selection(|scope| {
                    smith_io::export::gather(self.canvas.document(), scope).holed_faces()
                }),
            ]
        } else {
            [0, 0]
        };
        self.export = Some(ExportWindow {
            format,
            options: self.settings.exports.get(format),
            holed,
            render: None,
        });
    }

    /// File → Export → Render…: opens its window on the Render last
    /// restored (else the first), with the last choices.
    fn open_render_export(&mut self) {
        let first = self.canvas.scene_list().first().map(|(id, _)| *id);
        let Some(id) = self.canvas.last_scene().or(first) else {
            return;
        };
        let options = self.settings.exports.render;
        self.export = Some(ExportWindow {
            format: options.render_format.format(),
            options,
            holed: [0, 0],
            render: Some(id),
        });
    }

    /// Runs `f` on the selection's export scope.
    fn export_selection<R>(&self, f: impl FnOnce(Scope<'_>) -> R) -> R {
        let (context, local) = self.canvas.context();
        let selection = self.canvas.selection();
        let faces: Vec<_> = selection.faces().collect();
        let instances: Vec<_> = selection.instances().collect();
        f(Scope::Selection {
            context,
            local,
            faces: &faces,
            instances: &instances,
        })
    }

    /// The export window: the format's choices, then Export (on to the save
    /// dialog) or Cancel.
    fn export_window(&mut self, ui: &mut Ui) {
        let Some(w) = self.export.as_mut() else {
            return;
        };
        let selection = self.canvas.selection();
        let can_select =
            selection.faces().next().is_some() || selection.instances().next().is_some();
        // Export Render shows the fields of the format chosen in it.
        let format = if w.render.is_some() {
            w.options.render_format.format()
        } else {
            w.format
        };
        let scenes = self.canvas.scene_list();
        let mut choice = None;
        egui::Modal::new(egui::Id::new("export_options")).show(ui.ctx(), |ui| {
            ui.set_max_width(340.0);
            if w.render.is_some() {
                ui.heading("Export Render");
            } else {
                ui.heading(format!("Export {}", format.name()));
            }
            ui.add_space(6.0);
            let o = &mut w.options;
            if let Some(id) = w.render.as_mut() {
                let current = scenes
                    .iter()
                    .find(|(k, _)| k == id)
                    .map_or_else(String::new, |(_, n)| n.clone());
                ui.horizontal(|ui| {
                    ui.label("Render");
                    egui::ComboBox::from_id_salt("export_render")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (k, name) in &scenes {
                                ui.selectable_value(id, *k, name);
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Format");
                    egui::ComboBox::from_id_salt("export_render_format")
                        .selected_text(o.render_format.format().name())
                        .show_ui(ui, |ui| {
                            for f in RenderFormat::ALL {
                                ui.selectable_value(&mut o.render_format, f, f.format().name());
                            }
                        });
                });
            }
            if format == Format::Dxf {
                ui.horizontal(|ui| {
                    ui.label("Content");
                    egui::ComboBox::from_id_salt("export_content")
                        .selected_text(o.content.name())
                        .show_ui(ui, |ui| {
                            for c in DxfContent::ALL {
                                ui.selectable_value(&mut o.content, c, c.name());
                            }
                        });
                });
            }
            if matches!(format, Format::Pdf | Format::Svg) {
                ui.horizontal(|ui| {
                    ui.label("Paper");
                    egui::ComboBox::from_id_salt("export_paper")
                        .selected_text(o.paper.name())
                        .show_ui(ui, |ui| {
                            for p in Paper::ALL {
                                ui.selectable_value(&mut o.paper, p, p.name());
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Orientation");
                    egui::ComboBox::from_id_salt("export_orientation")
                        .selected_text(o.orientation.name())
                        .show_ui(ui, |ui| {
                            for r in Orientation::ALL {
                                ui.selectable_value(&mut o.orientation, r, r.name());
                            }
                        });
                });
            }
            if format.has_units() {
                ui.horizontal(|ui| {
                    ui.label("Units");
                    egui::ComboBox::from_id_salt("export_units")
                        .selected_text(o.units.name())
                        .show_ui(ui, |ui| {
                            for u in UnitPreset::ALL {
                                ui.selectable_value(&mut o.units, u, u.name());
                            }
                        });
                });
                if o.units == UnitPreset::Custom {
                    ui.horizontal(|ui| {
                        ui.label("Unit");
                        egui::ComboBox::from_id_salt("export_unit")
                            .selected_text(o.unit.name())
                            .show_ui(ui, |ui| {
                                for u in FileUnit::ALL {
                                    ui.selectable_value(&mut o.unit, u, u.name());
                                }
                            });
                    });
                }
            }
            if format.shows_scale(o) {
                ui.horizontal(|ui| {
                    ui.label("Scale 1:");
                    ui.add(
                        egui::DragValue::new(&mut o.scale)
                            .range(0.001..=100_000.0)
                            .speed(1.0),
                    );
                    // Under 1 enlarges: say so plainly.
                    if o.scale > 0.0 && o.scale < 1.0 {
                        ui.weak(format!("({}:1)", ratio(1.0 / o.scale)));
                    }
                });
            }
            if format.is_drawing(o) {
                ui.horizontal(|ui| {
                    ui.label("Hidden Lines");
                    egui::ComboBox::from_id_salt("export_hidden")
                        .selected_text(o.hidden_lines.name())
                        .show_ui(ui, |ui| {
                            for h in HiddenLines::ALL {
                                ui.selectable_value(&mut o.hidden_lines, h, h.name());
                            }
                        });
                });
            }
            if format == Format::Png {
                ui.horizontal(|ui| {
                    ui.label("Size");
                    egui::ComboBox::from_id_salt("export_size")
                        .selected_text(o.picture.name())
                        .show_ui(ui, |ui| {
                            for p in PictureSize::ALL {
                                ui.selectable_value(&mut o.picture, p, p.name());
                            }
                        });
                });
                if o.picture == PictureSize::Custom {
                    ui.horizontal(|ui| {
                        ui.label("Width");
                        ui.add(egui::DragValue::new(&mut o.pixels[0]).range(1..=MAX_SIDE));
                        ui.label("Height");
                        ui.add(egui::DragValue::new(&mut o.pixels[1]).range(1..=MAX_SIDE));
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Background");
                    egui::ComboBox::from_id_salt("export_background")
                        .selected_text(o.background.name())
                        .show_ui(ui, |ui| {
                            for b in Background::ALL {
                                ui.selectable_value(&mut o.background, b, b.name());
                            }
                        });
                });
            }
            // A picture, and a Render, is of the whole view.
            let whole = format == Format::Png || w.render.is_some();
            let mut only = o.selection_only && can_select && !whole;
            if !whole
                && ui
                    .add_enabled(can_select, egui::Checkbox::new(&mut only, "Selection Only"))
                    .changed()
            {
                o.selection_only = only;
            }
            let holed = w.holed[usize::from(only)];
            if format == Format::Obj && holed > 0 {
                ui.add_space(4.0);
                ui.weak(if holed == 1 {
                    "1 Face with Holes Will Be Triangulated".to_owned()
                } else {
                    format!("{holed} Faces with Holes Will Be Triangulated")
                });
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Export").clicked() {
                    choice = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(false);
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                choice = Some(false);
            }
        });
        let Some(go) = choice else {
            return;
        };
        let Some(w) = self.export.take() else {
            return;
        };
        if let Some(id) = w.render {
            if self.settings.exports.render != w.options {
                self.settings.exports.render = w.options;
                self.settings.save(&self.paths.config);
            }
            let name = scenes
                .iter()
                .find(|(k, _)| *k == id)
                .map_or_else(|| "Render".to_owned(), |(_, n)| n.clone());
            if go {
                let mut options = w.options;
                options.selection_only = false;
                let format = options.render_format.format();
                self.files
                    .start_dialog(Purpose::ExportRender(id, format, options, name));
            }
            return;
        }
        if self.settings.exports.get(w.format) != w.options {
            self.settings.exports.set(w.format, w.options);
            self.settings.save(&self.paths.config);
        }
        if go {
            let mut options = w.options;
            options.selection_only &= can_select;
            self.files.start_dialog(Purpose::Export(w.format, options));
        }
    }

    /// Exports to the file chosen as `chosen`, telling the result on the
    /// bottom line.
    fn export_to(
        &mut self,
        chosen: &Path,
        format: Format,
        options: &ExportOptions,
        ctx: &egui::Context,
    ) {
        let Some(path) = smith_io::export::target(chosen, format) else {
            self.canvas
                .set_message("Existing SketchSmith Model: Choose Another Name");
            return;
        };
        if format.is_drawing(options) {
            self.export_drawing(&path, format, options);
            return;
        }
        if format == Format::Png {
            self.export_png(&path, options, ctx);
            return;
        }
        let gathered = if options.selection_only {
            self.export_selection(|scope| smith_io::export::gather(self.canvas.document(), scope))
        } else {
            smith_io::export::gather(self.canvas.document(), Scope::Model)
        };
        if gathered.is_empty() {
            self.canvas.set_message("Nothing to Export");
            return;
        }
        // Pieces a STEP export wrote as surfaces, to tell.
        let mut open_pieces = 0;
        let result = match format {
            Format::Obj => {
                smith_io::export::write_obj(&path, self.canvas.document(), &gathered, options)
            }
            Format::Stl => smith_io::export::write_stl(&path, &gathered, options),
            Format::Dxf => smith_io::export::write_dxf(&path, &gathered, options),
            Format::Step => smith_io::export::write_step(&path, &gathered).map(|r| {
                open_pieces = r.open_pieces;
            }),
            Format::ThreeMf => {
                smith_io::export::write_3mf(&path, self.canvas.document(), &gathered, options)
            }
            Format::Gltf => {
                if options.selection_only {
                    self.export_selection(|scope| {
                        smith_io::export::write_gltf(&path, self.canvas.document(), scope)
                    })
                } else {
                    smith_io::export::write_gltf(&path, self.canvas.document(), Scope::Model)
                }
            }
            // Drawings and pictures went above.
            Format::Pdf | Format::Svg | Format::Png => return,
        };
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        match result {
            Ok(()) => {
                self.canvas.set_message(match open_pieces {
                    0 => format!("Exported {name}"),
                    1 => format!("Exported {name}: 1 Open Piece as a Surface"),
                    n => format!("Exported {name}: {n} Open Pieces as Surfaces"),
                });
                log::info!("exported {}", path.display());
            }
            Err(e) => self.canvas.set_message(format!("Couldn't Export: {e}")),
        }
    }

    /// Exports a picture of the canvas's view to `path` as PNG.
    fn export_png(&mut self, path: &Path, options: &ExportOptions, ctx: &egui::Context) {
        let canvas = self.canvas.pixel_size(ctx.pixels_per_point());
        let [w, h] = options.picture.pixels(canvas, options.pixels);
        let transparent = options.background == Background::Transparent;
        let result = self
            .canvas
            .picture(ctx, w, h, transparent)
            .map_err(|e| e.to_string())
            .and_then(|px| smith_io::export::write_png(path, w, h, &px).map_err(|e| e.to_string()));
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        match result {
            Ok(()) => {
                self.canvas.set_message(format!("Exported {name}"));
                log::info!("exported {} ({w} × {h})", path.display());
            }
            Err(e) => self.canvas.set_message(format!("Couldn't Export: {e}")),
        }
    }

    /// Exports a drawing of the canvas's view (always parallel) to `path`:
    /// PDF, SVG or 2D DXF. View → Section Cuts decides whether the active
    /// section plane cuts it.
    fn export_drawing(&mut self, path: &Path, format: Format, options: &ExportOptions) {
        let (units, cuts) = (self.settings.units, self.settings.section_cuts);
        let (drawing, perspective) = if options.selection_only {
            self.export_selection(|scope| drawing_of(&self.canvas, units, cuts, scope))
        } else {
            drawing_of(&self.canvas, units, cuts, Scope::Model)
        };
        self.write_drawing(path, format, options, &drawing, perspective);
    }

    /// File → Export → Render…: saved Render `id` exported to the file
    /// chosen as `chosen` in `format` (PNG, PDF or SVG), the canvas left as
    /// it is.
    fn export_render(
        &mut self,
        chosen: &Path,
        id: smith_model::SceneId,
        format: Format,
        options: &ExportOptions,
        ctx: &egui::Context,
    ) {
        let Some(path) = smith_io::export::target(chosen, format) else {
            self.canvas
                .set_message("Existing SketchSmith Model: Choose Another Name");
            return;
        };
        let (units, cuts) = (self.settings.units, self.settings.section_cuts);
        if format == Format::Png {
            let canvas = self.canvas.pixel_size(ctx.pixels_per_point());
            let [w, h] = options.picture.pixels(canvas, options.pixels);
            let transparent = options.background == Background::Transparent;
            let result = self
                .canvas
                .with_scene(id, |c| c.picture(ctx, w, h, transparent))
                .unwrap_or_else(|| Err("the Render is gone".into()))
                .and_then(|px| {
                    smith_io::export::write_png(&path, w, h, &px).map_err(|e| e.to_string())
                });
            self.tell_export(&path, result, &[]);
            return;
        }
        match self
            .canvas
            .with_scene(id, |c| drawing_of(c, units, cuts, Scope::Model))
        {
            Some((drawing, perspective)) => {
                self.write_drawing(&path, format, options, &drawing, perspective);
            }
            None => self
                .canvas
                .set_message("Couldn't Export: the Render is gone"),
        }
    }

    /// Puts an export's outcome on the bottom line, with any `notes`.
    fn tell_export(&mut self, path: &Path, result: Result<(), String>, notes: &[String]) {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        match result {
            Ok(()) => {
                self.canvas.set_message(if notes.is_empty() {
                    format!("Exported {name}")
                } else {
                    format!("Exported {name}: {}", notes.join("; "))
                });
                log::info!("exported {}", path.display());
            }
            Err(e) => self.canvas.set_message(format!("Couldn't Export: {e}")),
        }
    }

    /// Writes `drawing` (of a `perspective` view or not) to `path` as PDF,
    /// SVG or 2D DXF.
    fn write_drawing(
        &mut self,
        path: &Path,
        format: Format,
        options: &ExportOptions,
        drawing: &Drawing,
        perspective: bool,
    ) {
        if drawing.lines.is_empty() && drawing.marks.is_empty() {
            self.canvas.set_message("Nothing to Export");
            return;
        }
        let mut notes = Vec::new();
        if perspective {
            notes.push("Drawn in Parallel Projection".to_owned());
        }
        let result = match format {
            Format::Pdf | Format::Svg => {
                let sheet = smith_io::export::sheet(drawing, options);
                if !sheet.fits {
                    notes.push(format!(
                        "Drawing Is Larger Than the Page at 1:{}",
                        ratio(options.scale)
                    ));
                }
                if format == Format::Pdf {
                    smith_io::export::write_pdf(path, &sheet).map(|missing| match missing {
                        0 => {}
                        1 => notes.push("1 Character Not in the PDF Font, Shown as ?".to_owned()),
                        n => notes.push(format!("{n} Characters Not in the PDF Font, Shown as ?")),
                    })
                } else {
                    smith_io::export::write_svg(path, &sheet)
                }
            }
            _ => smith_io::export::write_dxf_drawing(path, drawing, options),
        };
        self.tell_export(path, result.map_err(|e| e.to_string()), &notes);
    }

    /// The canvas's Yes / No question, over the canvas until answered;
    /// closing it counts as No.
    fn canvas_dialog(&mut self, ui: &mut Ui) {
        let Some(dialog) = self.canvas.dialog() else {
            return;
        };
        let (id, heading, text) = match dialog {
            Dialog::AutoTriangulate => ("auto_triangulate", "Auto Triangulate", None),
            Dialog::RemoveMaterials(n) => (
                "remove_materials",
                "Confirm Remove",
                Some(if n == 1 {
                    "1 unused material will be removed.".to_owned()
                } else {
                    format!("{n} unused materials will be removed.")
                }),
            ),
            Dialog::RemoveLayer(id, faces, groups, edges) => {
                let name = self
                    .canvas
                    .document()
                    .layer(id)
                    .map_or_else(|| "the Layer".to_owned(), |l| l.name.clone());
                let count = |n: usize, one: &str, many: &str| match n {
                    0 => None,
                    1 => Some(format!("1 {one}")),
                    n => Some(format!("{n} {many}")),
                };
                let what: Vec<String> = [
                    count(faces, "Face", "Faces"),
                    count(groups, "Group", "Groups"),
                    count(edges, "Loose Edge", "Loose Edges"),
                ]
                .into_iter()
                .flatten()
                .collect();
                (
                    "remove_layer",
                    "Remove Layer",
                    Some(format!(
                        "Remove {name} and Its Contents? {} Will Be Deleted.",
                        what.join(", ")
                    )),
                )
            }
        };
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new(id)).show(ui.ctx(), |ui| {
            ui.set_max_width(380.0);
            ui.heading(heading);
            if let Some(text) = text {
                ui.add_space(6.0);
                ui.label(text);
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Yes").clicked() {
                    choice = Some(true);
                }
                if ui.button("No").clicked() {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        if let Some(yes) = choice {
            self.canvas.answer_dialog(yes);
        }
    }

    /// Applies a panel change to the current mode's layout and saves it.
    fn change_panel(&mut self, change: PanelChange) {
        let mode = self.canvas.mode();
        let layout = &mut self.layouts[mode_index(mode)];
        match change {
            PanelChange::Move(tool, group, before) => layout.move_tool(&tool, group, before),
            PanelChange::Hide(tool) => layout.hide(&tool),
            PanelChange::Shift(group, by) => {
                if let Some(to) = group.checked_add_signed(by) {
                    layout.move_group(group, to);
                }
            }
            PanelChange::Float(group, pos) => layout.set_floating(group, pos),
        }
        self.save_layouts();
    }

    /// Stores both panel layouts in the settings file.
    fn save_layouts(&mut self) {
        self.settings.panel_edit = Some(self.layouts[0].clone());
        self.settings.panel_present = Some(self.layouts[1].clone());
        self.settings.save(&self.paths.config);
    }

    /// Restores the current mode's default Geometry panel.
    fn reset_panel(&mut self) {
        let mode = self.canvas.mode();
        self.layouts[mode_index(mode)] = with_defaults(mode, PanelLayout::from_defaults);
        self.save_layouts();
    }

    /// Returns whether Quit was chosen since the last call, and clears the
    /// request. Quitting ends the autosave session, removing its copy: by now
    /// the work was saved or the user chose not to save it.
    pub fn take_quit_request(&mut self) -> bool {
        let quit = std::mem::take(&mut self.quit_requested);
        if quit {
            self.autosave.finish();
        }
        quit
    }

    /// Removes the Edit/Present toggle key (Tab by default) from the raw input
    /// and queues the action, unless a text field is being typed in.
    ///
    /// egui uses Tab to move keyboard focus between widgets, which would step
    /// through menus instead of switching modes. A mouse-driven drawing app
    /// doesn't need that, so Tab behaves as in Blender.
    pub fn intercept_keys(&mut self, raw: &mut egui::RawInput, typing: bool) {
        // Waiting for a key to bind: every key goes to Preferences.
        if typing || self.key_capture.is_some() || !raw.focused {
            self.held = None;
            return;
        }
        self.hold_chords(raw);
        let Some(chord) = self.keymap.bindings.get(&Action::ToggleMode).copied() else {
            return;
        };
        let mut toggle = false;
        let mut clipboard = Vec::new();
        raw.events.retain(|e| match e {
            egui::Event::Key {
                key,
                pressed,
                modifiers,
                ..
            } if *key == chord.key && modifiers.matches_logically(chord.modifiers) => {
                if *pressed {
                    toggle = true;
                }
                false
            }
            // The toolkit turns Ctrl+X / C / V into clipboard events rather
            // than key presses; outside a text field they are ours.
            egui::Event::Cut => {
                clipboard.push(Action::Cut);
                false
            }
            egui::Event::Copy => {
                clipboard.push(Action::Copy);
                false
            }
            egui::Event::Paste(_) => {
                clipboard.push(Action::Paste);
                false
            }
            _ => true,
        });
        if toggle {
            self.queued.push(Action::ToggleMode);
        }
        self.queued.extend(clipboard);
    }

    /// Hold chords: Space or Tab held while a key is pressed. The held key
    /// is taken out of the input (its spaces too); a key pressed while it is
    /// down runs its hold chord, if any; released with nothing pressed, it
    /// runs its own binding (Space: Select; Tab: Edit / Present). Only keys the keymap
    /// holds are read this way.
    fn hold_chords(&mut self, raw: &mut egui::RawInput) {
        let keymap = &self.keymap;
        let holds: Vec<HoldKey> = HoldKey::ALL
            .into_iter()
            .filter(|h| keymap.holds(*h))
            .collect();
        if holds.is_empty() {
            self.held = None;
            return;
        }
        let mut held = self.held;
        let mut queued = Vec::new();
        raw.events.retain(|e| match e {
            egui::Event::Key {
                key,
                pressed,
                repeat,
                ..
            } if HoldKey::of(*key).is_some_and(|h| holds.contains(&h)) => {
                let h = HoldKey::of(*key).unwrap_or(HoldKey::Space);
                if *repeat {
                    return false;
                }
                if *pressed {
                    held = Some((h, false));
                } else {
                    if held == Some((h, false))
                        && let Some(a) = keymap.plain_action(*key)
                    {
                        queued.push(a);
                    }
                    held = None;
                }
                false
            }
            egui::Event::Key {
                key, pressed: true, ..
            } if held.is_some() => {
                if let Some((h, _)) = held {
                    if let Some(a) = keymap.hold_action(h, *key) {
                        queued.push(a);
                    }
                    held = Some((h, true));
                }
                false
            }
            egui::Event::Key { pressed: false, .. } | egui::Event::Text(_) if held.is_some() => {
                false
            }
            egui::Event::Text(t) if t == " " && holds.contains(&HoldKey::Space) => false,
            _ => true,
        });
        self.held = held;
        self.queued.extend(queued);
    }

    /// Builds the whole window UI for one frame. The first panel added is
    /// outermost; the central canvas comes last.
    pub fn ui(&mut self, ui: &mut Ui) {
        for action in std::mem::take(&mut self.queued) {
            self.run(ui.ctx(), action);
        }
        Panel::top("top_bar")
            .resizable(false)
            .show(ui, |ui| self.top_bar(ui));
        Panel::bottom("bottom_bar")
            .resizable(false)
            .show(ui, |ui| self.bottom_bar(ui));

        let bar = match self.settings.panel_side {
            PanelSide::Right => Panel::right("tab_bar"),
            PanelSide::Left => Panel::left("tab_bar"),
        };
        let strip = egui::Frame::new().fill(self.canvas_background());
        bar.resizable(false)
            .show_separator_line(false)
            .frame(strip)
            .exact_size(TAB_BAR_WIDTH)
            .show(ui, |ui| self.tab_bar(ui));

        let prefs = ViewPrefs {
            units: &self.settings.units,
            snaps: &self.settings.snaps,
            snap_only: self.settings.snap_only,
            guides: self.settings.show_guides,
            sections: self.settings.show_section_planes,
            cuts: self.settings.section_cuts,
        };
        self.canvas
            .hold(self.file_window_open() || self.key_capture.is_some());
        let view = CentralPanel::no_frame()
            .show(ui, |ui| self.canvas.ui(ui, &self.keymap, &prefs))
            .response
            .rect;
        if self.canvas.take_sidebar_toggle() {
            self.settings.sidebar_open = !self.settings.sidebar_open;
            self.settings.save(&self.paths.config);
        }
        // The overlay steps aside while the Items panel shows the same
        // figures.
        let items_open = self.settings.sidebar_open && self.settings.sidebar_tab == 1;
        let statistics_shown =
            self.canvas.mode() == Mode::Edit && self.settings.show_statistics && !items_open;
        let mut below = 0;
        if statistics_shown {
            self.statistics(ui, view);
            below = self.statistics_lines().len();
        }
        self.report_overlay(ui, view, below);
        let inputs_top = self.operation_inputs_box(ui, view);
        self.side_panel(ui, view, inputs_top);
        self.tabs_overlay(ui.ctx());
        self.floating_toolbars(ui);
        self.file_events(&ui.ctx().clone());
        self.file_windows(ui);
        self.canvas_dialog(ui);
        self.autosave_tick(ui.ctx());
        self.keep_proportional();
        self.keep_select_kind();

        self.preferences_window(ui);
        self.about_window(ui);
    }

    // ----- top bar -----------------------------------------------------------

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.menu_button("File", |ui| {
                let idle = !self.files.busy();
                self.action_item_enabled(ui, "New", Action::NewFile, idle);
                self.action_item_enabled(ui, "Open…", Action::OpenFile, idle);
                self.recent_menu(ui, idle);
                self.action_item_enabled(ui, "Save", Action::SaveFile, idle);
                self.action_item_enabled(ui, "Save As…", Action::SaveFileAs, idle);
                ui.separator();
                ui.menu_button("Import", |ui| {
                    for format in ImportFormat::ALL {
                        let label = format!("{}…", format.name());
                        if ui.add_enabled(idle, Button::new(label)).clicked() {
                            self.import = Some((format, self.settings.imports.get(format)));
                        }
                    }
                });
                ui.menu_button("Export", |ui| {
                    for format in Format::ALL {
                        // The CAD formats below the model formats.
                        if format == Format::Step {
                            ui.separator();
                        }
                        let label = format!("{}…", format.name());
                        if ui.add_enabled(idle, Button::new(label)).clicked() {
                            self.open_export(format);
                        }
                    }
                    ui.separator();
                    let renders = !self.canvas.scene_list().is_empty();
                    if ui
                        .add_enabled(idle && renders, Button::new("Render…"))
                        .clicked()
                    {
                        self.open_render_export();
                    }
                });
                ui.separator();
                self.action_item(ui, "Quit", Action::Quit);
            });
            ui.menu_button("Edit", |ui| {
                // Undo, Redo, Unhide All, Cut, Copy, Paste, Select All and
                // Select None have icons.
                let edit = self.canvas.mode() == Mode::Edit;
                self.action_item_enabled(ui, "Adjust Last Operation", Action::AdjustLast, edit);
                ui.separator();
                self.action_item(ui, "Delete", Action::Delete);
                ui.separator();
                self.action_item(ui, "Unhide Last", Action::UnhideLast);
                ui.separator();
                let guides = edit && self.canvas.guide_count() > 0;
                self.action_item_enabled(ui, "Delete Guides", Action::DeleteGuides, guides);
                let slice = edit && self.canvas.has_active_section();
                self.action_item_enabled(
                    ui,
                    "Create Group from Slice",
                    Action::CreateGroupFromSlice,
                    slice,
                );
                ui.separator();
                self.action_item(ui, "Repeat Transform", Action::RepeatTransform);
            });
            ui.menu_button("View", |ui| self.view_menu(ui));
            ui.menu_button("Window", |ui| {
                if ui.button("Preferences…").clicked() {
                    self.show_preferences = true;
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Load Test Scene").clicked() {
                    self.request(Discard::TestScene);
                }
                if ui.button("Load Stress Scene (100k edges)").clicked() {
                    self.request(Discard::StressScene);
                }
                ui.separator();
                if ui.button(format!("About {APP_NAME}")).clicked() {
                    self.show_about = true;
                }
            });
            ui.separator();
            self.style_row(ui);
            // The file and its save state, always in view.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.save_status(ui)
            });
        });
    }

    /// The display styles along the top bar, for the mode in use (each mode
    /// keeps its own): one click each, and Back Edges.
    fn style_row(&mut self, ui: &mut Ui) {
        let edit = self.canvas.mode() == Mode::Edit;
        let (mut faces, mut back) = if edit {
            let s = self.canvas.edit_style();
            (s.faces, s.back_edges)
        } else {
            let l = self.canvas.look();
            (l.faces, l.back_edges)
        };
        for (label, value) in DISPLAY_STYLES {
            ui.selectable_value(&mut faces, *value, *label);
        }
        ui.toggle_value(&mut back, "Back Edges");
        if edit {
            let mut s = self.canvas.edit_style();
            (s.faces, s.back_edges) = (faces, back);
            self.canvas.set_edit_style(s);
        } else {
            let mut l = self.canvas.look();
            (l.faces, l.back_edges) = (faces, back);
            self.canvas.set_look(l);
        }
    }

    fn view_menu(&mut self, ui: &mut Ui) {
        let before = (
            self.settings.show_statistics,
            self.settings.sidebar_open,
            self.settings.show_guides,
            self.settings.show_section_planes,
            self.settings.section_cuts,
        );
        ui.checkbox(&mut self.settings.show_statistics, "Statistics");
        ui.checkbox(&mut self.settings.show_guides, "Guides");
        ui.checkbox(&mut self.settings.show_section_planes, "Section Planes");
        ui.checkbox(&mut self.settings.section_cuts, "Section Cuts");
        let mut open = self.settings.sidebar_open;
        let label = match self.keymap.label(Action::ToggleSidebar) {
            Some(k) => format!("Sidebar ({k})"),
            None => "Sidebar".to_string(),
        };
        ui.checkbox(&mut open, label);
        self.settings.sidebar_open = open;
        if (
            self.settings.show_statistics,
            self.settings.sidebar_open,
            self.settings.show_guides,
            self.settings.show_section_planes,
            self.settings.section_cuts,
        ) != before
        {
            self.settings.save(&self.paths.config);
        }
        // Mode is the bottom bar's switch; display styles are on the top
        // bar.
        ui.separator();
        ui.label("Operations Bar Side");
        self.panel_side_radios(ui);
        ui.separator();
        ui.checkbox(&mut self.customising, "Customise Toolbars")
            .on_hover_text("Drag buttons and groups; each mode keeps its own arrangement");
        if ui
            .button("Reset Toolbars")
            .on_hover_text("Restore this mode's default Geometry panel")
            .clicked()
        {
            self.reset_panel();
        }
    }

    // ----- bottom bar --------------------------------------------------------

    fn bottom_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // Only what has no icon in the Operations Bar: an icon or a key
            // is enough for the rest.
            upward_menu(ui, "Camera", |ui| self.camera_menu(ui));
            ui.separator();
            let mut mode = self.canvas.mode();
            ui.selectable_value(&mut mode, Mode::Edit, "Edit");
            ui.selectable_value(&mut mode, Mode::Present, "Present");
            self.canvas.set_mode(mode);
            ui.separator();
            self.snapping_box(ui);
            ui.separator();
            // The layer new geometry goes on; a click opens the Layers tab.
            let doc = self.canvas.document();
            let active = doc
                .active_layer()
                .or_else(|| doc.base_layer())
                .and_then(|l| doc.layer(l))
                .map_or_else(String::new, |l| l.name.clone());
            if ui
                .button(format!("Layer: {active}"))
                .on_hover_text("The layer new geometry goes on · click for the Layers tab")
                .clicked()
            {
                // The Layers tab is Edit mode's.
                self.settings.sidebar_tab = 2;
                if self.canvas.mode() == Mode::Edit {
                    self.settings.sidebar_open = true;
                }
                self.settings.save(&self.paths.config);
            }
            ui.separator();
            let mut pivot = self.canvas.pivot_active();
            if ui
                .toggle_value(&mut pivot, "Pivot Active")
                .on_hover_text(
                    "Rotate, Scale, Mirror and Flip Along work around the Pivot · \
                     right-click to place it",
                )
                .changed()
            {
                self.canvas.set_pivot_active(pivot);
            }
            ui.separator();
            let status = self
                .canvas
                .message()
                .map_or_else(|| self.status_text(), str::to_owned);
            ui.label(status);
        });
    }

    fn camera_menu(&mut self, ui: &mut Ui) {
        for (label, action) in [
            ("Top", Action::ViewTop),
            ("Bottom", Action::ViewBottom),
            ("Front", Action::ViewFront),
            ("Back", Action::ViewBack),
            ("Left", Action::ViewLeft),
            ("Right", Action::ViewRight),
            ("Iso", Action::ViewIso),
        ] {
            self.action_item(ui, label, action);
        }
        ui.separator();
        let label = if self.canvas.is_perspective() {
            "Parallel Projection"
        } else {
            "Perspective"
        };
        self.action_item(ui, label, Action::ToggleProjection);
        ui.separator();
        self.action_item(ui, "View Pie Menu", Action::ViewPie);
        self.action_item(ui, "Reset Axes", Action::ResetAxes);
    }

    /// The Proportional / Snapping box. Proportional
    /// editing is not built yet.
    fn snapping_box(&mut self, ui: &mut Ui) {
        let response = ui.button("Snap ▾");
        Popup::menu(&response)
            .align(RectAlign::TOP_END)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                let before = (self.settings.snaps, self.settings.snap_only);
                let s = &mut self.settings.snaps;
                ui.label("Snap to");
                ui.checkbox(&mut s.endpoint, "Endpoint");
                ui.checkbox(&mut s.midpoint, "Midpoint");
                ui.checkbox(&mut s.intersection, "Intersection");
                ui.checkbox(&mut s.centre, "Centre");
                ui.checkbox(&mut s.quadrant, "Quadrant");
                ui.checkbox(&mut s.face_centre, "Face centre");
                ui.checkbox(&mut s.on_edge, "On edge");
                ui.checkbox(&mut s.on_face, "On face");
                ui.checkbox(&mut s.axes, "Axis directions");
                ui.checkbox(&mut s.parallel, "Parallel / perpendicular");
                ui.checkbox(&mut s.perpendicular, "Perpendicular foot");
                ui.checkbox(&mut s.tangent, "Tangent");
                ui.checkbox(&mut s.guides, "Guides");
                ui.separator();
                ui.label("Hold Alt to snap only to:");
                for (label, kind) in [
                    ("Centre", OnlyKind::Centre),
                    ("Endpoint", OnlyKind::Endpoint),
                    ("Midpoint", OnlyKind::Midpoint),
                    ("Quadrant", OnlyKind::Quadrant),
                    ("Face centre", OnlyKind::FaceCentre),
                ] {
                    ui.radio_value(&mut self.settings.snap_only, kind, label);
                }
                ui.separator();
                ui.label("Proportional editing");
                let (mut on, mut p) = self.canvas.proportional();
                let was = (on, p);
                ui.checkbox(&mut on, "Proportional");
                ui.horizontal(|ui| {
                    ui.label("Radius");
                    ui.add(
                        egui::DragValue::new(&mut p.radius)
                            .range(0.01..=10_000.0)
                            .suffix(" mm"),
                    );
                });
                for (label, f) in [
                    ("Smooth", Falloff::Smooth),
                    ("Linear", Falloff::Linear),
                    ("Sharp", Falloff::Sharp),
                ] {
                    ui.radio_value(&mut p.falloff, f, label);
                }
                if (on, p) != was {
                    self.canvas.set_proportional(on, p);
                }
                if (self.settings.snaps, self.settings.snap_only) != before {
                    self.settings.save(&self.paths.config);
                }
            });
    }

    // ----- operations panel ----------------------------------------------------------------

    /// The save status at the top bar's right end: the dot, the file name and
    /// the Save Version button.
    fn save_status(&mut self, ui: &mut Ui) {
        let status = self.files.status(self.dirty());
        let full = self
            .files
            .path
            .as_deref()
            .map_or_else(|| self.files.display_name(), |p| p.display().to_string());
        // Drawn right to left at the top bar's end: Save Version, the name,
        // then the dot.
        let clicked = ui
            .add_enabled(!self.files.busy(), Button::new("Save Version"))
            .clicked();
        ui.add(egui::Label::new(self.files.display_name()).truncate())
            .on_hover_text(full);
        let size = ui.spacing().interact_size.y * 0.8;
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), size * 0.5, status_colour(status));
        response.on_hover_text(status.label());
        if clicked {
            self.save_version();
        }
    }

    /// The Geometry panel's tool groups.
    fn panel_contents(&mut self, ui: &mut Ui) {
        let mode = self.canvas.mode();
        ScrollArea::vertical().show(ui, |ui| {
            if self.customising {
                ui.horizontal(|ui| {
                    ui.strong("Customising toolbars");
                    if ui.button("Done").clicked() {
                        self.customising = false;
                    }
                });
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "Drag buttons to reorder or move them; drag to Hidden to remove.",
                        )
                        .small(),
                    )
                    .wrap(),
                );
                ui.add_space(4.0);
            }
            let mut change = None;
            let groups = self.layouts[mode_index(mode)].groups.clone();
            let count = groups.len();
            for (gi, group) in groups.iter().enumerate() {
                if group.floating.is_some() || (group.tools.is_empty() && !self.customising) {
                    continue;
                }
                self.group_header(ui, gi, count, group, &mut change);
                self.group_buttons(ui, gi, group, &mut change);
                ui.add_space(4.0);
            }
            if self.customising {
                self.hidden_tray(ui, mode, &mut change);
            }
            if let Some(c) = change {
                self.change_panel(c);
            }
        });
    }

    /// A group's title, with Up / Down / Float while customising.
    fn group_header(
        &mut self,
        ui: &mut Ui,
        gi: usize,
        count: usize,
        group: &GroupLayout,
        change: &mut Option<PanelChange>,
    ) {
        if !self.customising {
            group_title(ui, &group.title);
            return;
        }
        ui.horizontal(|ui| {
            group_title(ui, &group.title);
            if ui.add_enabled(gi > 0, Button::new("Up").small()).clicked() {
                *change = Some(PanelChange::Shift(gi, -1));
            }
            if ui
                .add_enabled(gi + 1 < count, Button::new("Down").small())
                .clicked()
            {
                *change = Some(PanelChange::Shift(gi, 1));
            }
            if ui
                .add_enabled(!group.tools.is_empty(), Button::new("Float").small())
                .on_hover_text("Float this group over the canvas as a toolbar")
                .clicked()
            {
                let step = gi as f32 * FLOAT_STAGGER;
                *change = Some(PanelChange::Float(
                    gi,
                    Some([FLOAT_X + step, FLOAT_Y + step]),
                ));
            }
        });
    }

    /// A group's buttons. While customising, each button can be dragged, and
    /// dropping a button on another inserts it before that one; dropping on the
    /// group's empty space appends it.
    fn group_buttons(
        &mut self,
        ui: &mut Ui,
        gi: usize,
        group: &GroupLayout,
        change: &mut Option<PanelChange>,
    ) {
        let area = ui
            .horizontal_wrapped(|ui| {
                if group.tools.is_empty() {
                    ui.weak("(drop buttons here)");
                }
                for (ti, name) in group.tools.iter().enumerate() {
                    let Some(slot) = find_slot(name) else {
                        continue;
                    };
                    if self.customising {
                        let resp = self.draggable_button(ui, slot);
                        if let Some(p) = resp.dnd_release_payload::<DragTool>()
                            && p.0 != *name
                        {
                            *change = Some(PanelChange::Move(p.0.clone(), gi, Some(ti)));
                        }
                    } else {
                        self.tool_button(ui, slot);
                    }
                }
            })
            .response;
        if self.customising
            && change.is_none()
            && let Some(p) = area.dnd_release_payload::<DragTool>()
        {
            *change = Some(PanelChange::Move(p.0.clone(), gi, None));
        }
    }

    /// The tray of hidden buttons shown while customising: drop a button here
    /// to hide it, drag one out to add it to a group.
    fn hidden_tray(&mut self, ui: &mut Ui, mode: Mode, change: &mut Option<PanelChange>) {
        ui.small("Hidden");
        let hidden = self.layouts[mode_index(mode)].hidden.clone();
        let frame = egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_size(Vec2::new(ui.available_width(), TOOL_BUTTON_SIZE));
            ui.horizontal_wrapped(|ui| {
                if hidden.is_empty() {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new("Drag buttons here to hide them.").weak(),
                        )
                        .wrap(),
                    );
                }
                for name in &hidden {
                    if let Some(slot) = find_slot(name) {
                        self.draggable_button(ui, slot);
                    }
                }
            });
        });
        if change.is_none()
            && let Some(p) = frame.response.dnd_release_payload::<DragTool>()
        {
            *change = Some(PanelChange::Hide(p.0.clone()));
        }
    }

    /// A tool button that can be dragged (customising): drawn like the Geometry panel
    /// button but not clickable. While dragged, its icon follows the cursor.
    fn draggable_button(&mut self, ui: &mut Ui, slot: &ToolSlot) -> egui::Response {
        let has_icon = icons::NAMES_SET.contains(&slot.name);
        let label = if has_icon { "" } else { slot.short };
        let size = Vec2::splat(TOOL_BUTTON_SIZE);
        let resp = ui.add(Button::new(label).min_size(size).sense(egui::Sense::drag()));
        let color = ui.visuals().widgets.inactive.fg_stroke.color;
        if has_icon {
            icons::paint(ui.painter(), resp.rect, slot.name, color);
        }
        resp.dnd_set_drag_payload(DragTool(slot.name.to_owned()));
        if resp.dragged()
            && let Some(at) = ui.ctx().pointer_interact_pos()
        {
            let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("panel_drag_ghost"));
            let painter = ui.ctx().layer_painter(layer);
            let ghost = egui::Rect::from_center_size(at, size);
            painter.rect_filled(ghost, 4.0, ui.visuals().widgets.hovered.bg_fill);
            if has_icon {
                icons::paint(&painter, ghost, slot.name, color);
            } else {
                painter.text(
                    ghost.center(),
                    egui::Align2::CENTER_CENTER,
                    slot.short,
                    egui::FontId::proportional(12.0),
                    color,
                );
            }
        }
        resp.on_hover_text(slot.name)
    }

    /// Groups floating over the canvas, as small toolbars. Their positions
    /// are saved when moved; while customising they also take dropped buttons
    /// and can be docked back.
    fn floating_toolbars(&mut self, ui: &mut Ui) {
        let mode = self.canvas.mode();
        let mi = mode_index(mode);
        let ctx = ui.ctx().clone();
        let groups = self.layouts[mi].groups.clone();
        let mut change = None;
        let mut moved = None;
        for (gi, group) in groups.iter().enumerate() {
            let Some(pos) = group.floating else { continue };
            let shown = egui::Window::new(&group.title)
                .id(egui::Id::new(("floating_group", mi, &group.title)))
                .default_pos(egui::pos2(pos[0], pos[1]))
                .resizable(false)
                .collapsible(false)
                .default_width(FLOAT_WIDTH)
                .show(&ctx, |ui| {
                    ui.set_max_width(FLOAT_WIDTH);
                    if self.customising && ui.small_button("Dock").clicked() {
                        change = Some(PanelChange::Float(gi, None));
                    }
                    self.group_buttons(ui, gi, group, &mut change);
                });
            if let Some(shown) = shown {
                let at = shown.response.rect.min;
                let released = ctx.input(|i| i.pointer.any_released());
                if released && ((at.x - pos[0]).abs() > 0.5 || (at.y - pos[1]).abs() > 0.5) {
                    moved = Some((gi, [at.x, at.y]));
                }
            }
        }
        if let Some(c) = change {
            self.change_panel(c);
        } else if let Some((gi, p)) = moved {
            self.change_panel(PanelChange::Float(gi, Some(p)));
        }
    }

    /// A panel button showing the tool's icon (or its short label if it has none).
    fn tool_button(&mut self, ui: &mut Ui, slot: &ToolSlot) {
        let size = Vec2::splat(TOOL_BUTTON_SIZE);
        let action = panel_action(slot.name);
        let nav = self.canvas.nav_tool();
        let tool = self.canvas.tool();
        let active = match action {
            Some(Action::SelectTool) => {
                nav == NavTool::None && tool == WorkTool::Select && !self.canvas.selecting_by_type()
            }
            Some(Action::SelectByType) => nav == NavTool::None && self.canvas.selecting_by_type(),
            Some(Action::SelectVertices) => self.canvas.select_kind() == SelectKind::Vertices,
            Some(Action::SelectEdges) => self.canvas.select_kind() == SelectKind::Edges,
            Some(Action::SelectFaces) => self.canvas.select_kind() == SelectKind::Faces,
            Some(Action::PaintTool) => nav == NavTool::None && tool == WorkTool::Paint,
            Some(Action::MarkEdgesTool) => nav == NavTool::None && tool == WorkTool::MarkEdges,
            Some(Action::LineStyleTool) => nav == NavTool::None && tool == WorkTool::LineStyle,
            Some(Action::OrbitTool) => nav == NavTool::Orbit,
            Some(Action::PanTool) => nav == NavTool::Pan,
            Some(Action::ZoomTool) => nav == NavTool::Zoom,
            Some(Action::AxesTool) => matches!(tool, WorkTool::Axes { .. }),
            Some(a) => draw_kind(a).is_some_and(|k| tool == WorkTool::Draw(k)),
            None => false,
        };
        let has_icon = icons::NAMES_SET.contains(&slot.name);
        let label = if has_icon { "" } else { slot.short };
        let enabled = match action {
            Some(Action::Undo) => self.canvas.can_undo(),
            Some(Action::Redo) => self.canvas.can_redo(),
            // The select modes are Edit mode's.
            Some(Action::SelectVertices | Action::SelectEdges | Action::SelectFaces) => {
                self.canvas.mode() == Mode::Edit
            }
            Some(_) => true,
            None => false,
        };
        let response = ui.add_enabled(enabled, Button::new(label).min_size(size).selected(active));
        if has_icon {
            let base = ui.visuals().widgets.inactive.fg_stroke.color;
            let color = if enabled {
                base
            } else {
                base.gamma_multiply(0.4)
            };
            icons::paint(ui.painter(), response.rect, slot.name, color);
        }
        match action {
            Some(action) => {
                let tip = match self.keymap.label(action) {
                    Some(k) => format!("{} ({k})", slot.name),
                    None => slot.name.to_owned(),
                };
                if response
                    .on_hover_text(tip.clone())
                    .on_disabled_hover_text(tip)
                    .clicked()
                {
                    let ctx = ui.ctx().clone();
                    self.run(&ctx, action);
                }
            }
            None => {
                response.on_disabled_hover_text(format!("{} — not available yet", slot.name));
            }
        }
    }

    /// The Statistics overlay: a read-only snapshot of the selection, in
    /// the canvas's top corner away from the Operations Bar (so an open panel never
    /// covers it). It shows the shape as the drawing tools name it (or a
    /// group's name), its size along X, Y and Z, a curve's segments and
    /// radius, the counts, length and areas — or the context's totals when
    /// nothing is selected. Changed only by operations.
    /// The Statistics overlay, top-left of the canvas (top-right with the
    /// Operations Bar on the left).
    fn statistics(&self, ui: &Ui, view: egui::Rect) {
        let lines = self.statistics_lines();
        self.statistics_paint(ui, view, lines);
    }

    /// What is selected, by name: the one item's name (a group's, or the
    /// shape the drawing tools would call it); "3 Boxes" for several of one
    /// kind, "3 Items" for a mix; for parts of loose geometry, "Face",
    /// "3 Faces" or "3 Edges", else "Geometry". With nothing selected, the group being edited, or
    /// "Model".
    fn selection_name(&self) -> String {
        if self.canvas.selection().is_empty() {
            return self
                .canvas
                .outliner()
                .iter()
                .filter(|r| r.open)
                .max_by_key(|r| r.path.len())
                .map_or_else(|| "Model".to_string(), outliner_name);
        }
        if let Some(name) = self.canvas.selection_shape() {
            return name;
        }
        let mut names: Vec<String> = self
            .canvas
            .pieces()
            .iter()
            .enumerate()
            .filter(|(i, _)| self.canvas.piece_selected(*i))
            .map(|(_, p)| p.name.clone())
            .collect();
        names.extend(
            self.canvas
                .outliner()
                .iter()
                .filter(|r| r.selected)
                .map(outliner_name),
        );
        let sel = self.canvas.selection();
        let (edges, faces) = (sel.edges().count(), sel.faces().count());
        match names.as_slice() {
            // Parts of a piece, not a whole one.
            [] if edges == 0 && faces == 1 => "Face".into(),
            [] if edges == 0 && faces > 1 => format!("{faces} Faces"),
            [] if faces == 0 && edges > 1 => format!("{edges} Edges"),
            [] => "Geometry".into(),
            [one] => one.clone(),
            [first, rest @ ..] if rest.iter().all(|n| n == first) => {
                format!("{} {}", names.len(), plural(first))
            }
            _ => format!("{} Items", names.len()),
        }
    }

    /// The Statistics figures, one line each, under the selection's name:
    /// for the whole context with nothing selected, otherwise for the
    /// selection.
    fn statistics_lines(&self) -> Vec<String> {
        let sel = self.canvas.selection();
        let root = self.canvas.active_entities();
        let doc = self.canvas.document();
        let units = &self.settings.units;
        let mut lines: Vec<String> = vec![self.selection_name()];
        // Faces turned the wrong way, whenever there are any.
        let reversed = self.canvas.reversed_count();
        if reversed > 0 {
            lines.push(format!("Reversed Faces: {reversed}"));
        }
        if sel.is_empty() {
            lines.push(format!(
                "Edges {} · Faces {} · Groups {}",
                root.edge_count(),
                root.face_count(),
                root.instance_count()
            ));
        } else {
            if let Some(cv) = self.canvas.curve_values() {
                let count = if cv.kind == crate::canvas::CurveKind::Polygon {
                    "Sides"
                } else {
                    "Segments"
                };
                lines.push(format!("{count} {}", cv.segments));
                lines.push(format!("Radius {}", format_length(cv.radius, units)));
            }
            if let Some(size) = self.canvas.selection_size() {
                lines.push(format!("X {}", format_length(size.x, units)));
                lines.push(format!("Y {}", format_length(size.y, units)));
                lines.push(format!("Z {}", format_length(size.z, units)));
            }
            let edges: Vec<_> = sel.edges().collect();
            let faces: Vec<_> = sel.faces().collect();
            let groups: Vec<_> = sel.instances().collect();
            lines.push(format!(
                "Edges {} · Faces {} · Groups {}",
                edges.len(),
                faces.len(),
                groups.len()
            ));
            if !edges.is_empty() {
                let length: f64 = edges.iter().filter_map(|e| root.edge_length(*e)).sum();
                lines.push(format!("Length {}", format_length(length, units)));
            }
            if !faces.is_empty() {
                let area: f64 = faces.iter().filter_map(|f| root.face_area(*f)).sum();
                lines.push(format!("Area {}", format_area(area, units)));
                let on: Vec<_> = faces
                    .iter()
                    .filter_map(|f| root.face(*f).map(|x| x.attributes.layer))
                    .collect();
                if let Some(l) = on.first().copied().filter(|l| on.iter().all(|x| x == l)) {
                    let name = l
                        .or_else(|| doc.base_layer())
                        .and_then(|k| doc.layer(k))
                        .map_or_else(String::new, |x| x.name.clone());
                    lines.push(format!(
                        "Area on layer {name} {}",
                        format_area(self.canvas.layer_area(l), units)
                    ));
                }
            }
            if !groups.is_empty() {
                let area: f64 = groups.iter().map(|i| self.canvas.instance_area(*i)).sum();
                lines.push(format!("Group area {}", format_area(area, units)));
                // Closed solids: their volume.
                let volumes: Vec<f64> = groups
                    .iter()
                    .filter_map(|i| self.canvas.instance_volume(*i))
                    .collect();
                if !volumes.is_empty() {
                    let v: f64 = volumes.iter().sum();
                    lines.push(format!(
                        "Volume {}",
                        smith_infer::units::format_volume(v, units)
                    ));
                }
            }
        }
        lines
    }

    fn statistics_paint(&self, ui: &Ui, view: egui::Rect, lines: Vec<String>) {
        // On the background order, above the canvas but below the panels,
        // windows and menus, so a dropdown opened over it stays solid.
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("statistics"),
        ));
        let font = egui::FontId::proportional(12.0);
        let right_side = self.settings.panel_side == PanelSide::Left;
        let (x, align) = if right_side {
            (view.right() - 10.0, egui::Align2::RIGHT_TOP)
        } else {
            (view.left() + 10.0, egui::Align2::LEFT_TOP)
        };
        let mut y = view.top() + 8.0;
        for line in lines {
            painter.text(
                egui::pos2(x, y),
                align,
                line,
                font.clone(),
                egui::Color32::from_gray(60),
            );
            y += 16.0;
        }
    }

    /// The report of the active repair tool, under the Statistics overlay
    /// (`below` lines of it are showing) in the same corner and style.
    /// Lines with items to select show a pointing hand, and a click selects
    /// them.
    fn report_overlay(&mut self, ui: &Ui, view: egui::Rect, below: usize) {
        if self.canvas.report().is_empty() {
            return;
        }
        let right_side = self.settings.panel_side == PanelSide::Left;
        let (x, pivot) = if right_side {
            (view.right() - 10.0, egui::Align2::RIGHT_TOP)
        } else {
            (view.left() + 10.0, egui::Align2::LEFT_TOP)
        };
        let gap = if below > 0 { 8.0 } else { 0.0 };
        let y = view.top() + 8.0 + below as f32 * 16.0 + gap;
        let lines: Vec<(String, bool)> = self
            .canvas
            .report()
            .iter()
            .map(|l| (l.text.clone(), !l.items.is_empty()))
            .collect();
        let mut clicked = None;
        egui::Area::new(egui::Id::new("report_overlay"))
            .fixed_pos(egui::pos2(x, y))
            .pivot(pivot)
            .order(egui::Order::Middle)
            .show(ui.ctx(), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let layout = if right_side {
                    egui::Layout::top_down(egui::Align::Max)
                } else {
                    egui::Layout::top_down(egui::Align::Min)
                };
                ui.with_layout(layout, |ui| {
                    for (i, (text, selectable)) in lines.iter().enumerate() {
                        let rich = egui::RichText::new(text)
                            .size(12.0)
                            .color(egui::Color32::from_gray(60));
                        if *selectable {
                            let r = ui.add(egui::Label::new(rich).sense(egui::Sense::click()));
                            let r = if r.hovered() {
                                r.on_hover_cursor(egui::CursorIcon::PointingHand)
                            } else {
                                r
                            };
                            if r.clicked() {
                                clicked = Some(i);
                            }
                        } else {
                            ui.add(egui::Label::new(rich).selectable(false));
                        }
                    }
                });
            });
        if let Some(i) = clicked {
            self.canvas.select_report_line(i);
        }
    }

    /// The Operations Bar: a slim strip on the window edge, in the canvas's own
    /// background colour. It only lays the tabs out ([`Shell::tab_layout`]);
    /// they are drawn over the canvas by [`Shell::tabs_overlay`], since they
    /// reach past the strip onto it.
    fn tab_bar(&mut self, ui: &mut Ui) {
        let strip = ui.max_rect();
        let right = self.settings.panel_side == PanelSide::Right;
        // From the window edge to TAB_PROTRUDE past the strip's canvas edge.
        let (x0, x1) = if right {
            (strip.left() - TAB_PROTRUDE, strip.right())
        } else {
            (strip.left(), strip.right() + TAB_PROTRUDE)
        };
        let mut y = strip.top() + TAB_GAP;
        self.strip = strip;
        self.tab_layout = self
            .tabs()
            .iter()
            .map(|name| {
                let galley = ui.painter().layout_no_wrap(
                    (*name).to_string(),
                    egui::FontId::proportional(13.0),
                    egui::Color32::PLACEHOLDER,
                );
                let rect = egui::Rect::from_min_max(
                    egui::pos2(x0, y),
                    egui::pos2(x1, y + galley.size().x + 2.0 * TAB_TEXT_PAD),
                );
                y = rect.bottom() + TAB_GAP;
                (rect, galley)
            })
            .collect();
        // A click on the empty strip below the tabs closes the open panel.
        let below = egui::Rect::from_min_max(egui::pos2(strip.left(), y), strip.right_bottom());
        if below.height() > 0.0
            && ui
                .interact(
                    below,
                    ui.id().with("strip_below_tabs"),
                    egui::Sense::click(),
                )
                .clicked()
            && self.settings.sidebar_open
        {
            self.settings.sidebar_open = false;
            self.settings.save(&self.paths.config);
        }
    }

    /// The tabs, drawn over the canvas edge: each runs from the window edge
    /// and reaches [`TAB_PROTRUDE`] onto the canvas; its corners are square
    /// on the window edge, where it attaches, and rounded on the canvas
    /// side. Closed tabs are a mid grey on the strip's white, darker under
    /// the pointer; the open tab takes its panel's colour, with no border
    /// on the canvas side, so it runs into the panel. Text reads top to
    /// bottom with the bar on the right and bottom to top on the left.
    ///
    /// Resting the pointer on a tab opens its panel (after
    /// [`TAB_HOVER_SECS`], so crossing the strip opens nothing); clicking a
    /// tab opens it at once, and clicking the open tab (or N) closes it.
    fn tabs_overlay(&mut self, ctx: &egui::Context) {
        if self.tab_layout.is_empty() {
            return;
        }
        let open = self.settings.sidebar_open.then_some(self.tab());
        let right = self.settings.panel_side == PanelSide::Right;
        let now = ctx.input(|i| i.time);
        let layout = self.tab_layout.clone();
        let mut hovered_tab = None;
        let mut clicked = None;
        // Painted on a layer of its own, with clicks read straight from the
        // pointer: an Area here would claim the pointer over its whole extent
        // and swallow clicks meant for the panel beside the tabs. The middle
        // order puts it over the panels but under the menus.
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("tab_overlay"),
        ));
        let (hover_pos, clicked_at) = ctx.input(|i| {
            (
                i.pointer.hover_pos(),
                i.pointer
                    .primary_clicked()
                    .then(|| i.pointer.interact_pos())
                    .flatten(),
            )
        });
        // Only when nothing above the tabs (a menu, a popup) has the pointer.
        let free = |p: egui::Pos2| {
            ctx.layer_id_at(p)
                .is_none_or(|l| l.order <= egui::Order::Middle)
        };
        {
            let visuals = ctx.global_style().visuals.clone();
            let panel_fill = visuals.panel_fill;
            let border = visuals.widgets.noninteractive.bg_stroke;
            let r = f32::from(TAB_RADIUS);
            let radius = egui::CornerRadius {
                nw: if right { TAB_RADIUS } else { 0 },
                sw: if right { TAB_RADIUS } else { 0 },
                ne: if right { 0 } else { TAB_RADIUS },
                se: if right { 0 } else { TAB_RADIUS },
            };
            for (i, (rect, galley)) in layout.into_iter().enumerate() {
                let tab = i as u8;
                let active = open == Some(tab);
                let hovered = hover_pos.is_some_and(|p| rect.contains(p) && free(p));
                if hovered {
                    hovered_tab = Some(tab);
                }
                if clicked_at.is_some_and(|p| rect.contains(p) && free(p)) {
                    clicked = Some((tab, active));
                }
                if active {
                    painter.rect_filled(rect, radius, panel_fill);
                    // Border along the top and bottom and round the
                    // canvas-side corners; open along the tip, where the
                    // panel joins, and at the window edge.
                    let (edge, tip) = if right {
                        (rect.right(), rect.left() + 0.5)
                    } else {
                        (rect.left(), rect.right() - 0.5)
                    };
                    let inward = if right { 1.0 } else { -1.0 };
                    let cx = tip + inward * r;
                    let mut top = vec![egui::pos2(edge, rect.top() + 0.5)];
                    top.extend(tip_arc(
                        egui::pos2(cx, rect.top() + r),
                        r - 0.5,
                        right,
                        true,
                    ));
                    painter.add(egui::Shape::line(top, border));
                    let mut bottom = vec![egui::pos2(edge, rect.bottom() - 0.5)];
                    bottom.extend(tip_arc(
                        egui::pos2(cx, rect.bottom() - r),
                        r - 0.5,
                        right,
                        false,
                    ));
                    painter.add(egui::Shape::line(bottom, border));
                } else {
                    let fill = if hovered { TAB_HOVER } else { TAB_FILL };
                    painter.rect_filled(rect, radius, fill);
                }
                let colour = if active {
                    egui::Color32::from_gray(20)
                } else {
                    TAB_TEXT
                };
                // Read top to bottom on the right (a quarter turn
                // clockwise), bottom to top on the left (anticlockwise).
                let h = galley.size().y;
                let (pos, angle) = if right {
                    (
                        egui::pos2(rect.center().x + h * 0.5, rect.top() + TAB_TEXT_PAD),
                        std::f32::consts::FRAC_PI_2,
                    )
                } else {
                    (
                        egui::pos2(rect.center().x - h * 0.5, rect.bottom() - TAB_TEXT_PAD),
                        -std::f32::consts::FRAC_PI_2,
                    )
                };
                painter.add(egui::epaint::TextShape::new(pos, galley, colour).with_angle(angle));
            }
        }
        if let Some((tab, active)) = clicked {
            // A click just after the pointer opened this tab keeps it open.
            let just_opened = active && now - self.tab_opened_at < TAB_CLICK_GRACE_SECS;
            if active && !just_opened {
                self.settings.sidebar_open = false;
            } else {
                self.open_tab_panel(tab, now);
            }
            self.settings.save(&self.paths.config);
        }
        // Opening on a resting pointer.
        match (hovered_tab, self.tab_hover) {
            (Some(tab), Some((t, since))) if t == tab => {
                let waited = now - since;
                if open != Some(tab) {
                    if waited >= TAB_HOVER_SECS {
                        self.open_tab_panel(tab, now);
                        self.settings.save(&self.paths.config);
                    } else {
                        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                            TAB_HOVER_SECS - waited,
                        ));
                    }
                }
            }
            (Some(tab), _) => {
                self.tab_hover = Some((tab, now));
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(TAB_HOVER_SECS));
            }
            (None, _) => self.tab_hover = None,
        }
    }

    /// Opens tab `tab`'s panel (swapping out any other), noting when.
    fn open_tab_panel(&mut self, tab: u8, now: f64) {
        self.settings.sidebar_open = true;
        self.set_tab(tab);
        self.tab_opened_at = now;
    }

    /// The canvas's background colour in the current mode, for the tab
    /// strip beside it.
    fn canvas_background(&self) -> egui::Color32 {
        let bg = match self.canvas.mode() {
            Mode::Edit => self.canvas.edit_style().background,
            Mode::Present => self.canvas.look().background,
        };
        egui::Color32::from(egui::Rgba::from_rgb(bg[0], bg[1], bg[2]))
    }

    /// The open tab of the current mode: each mode remembers its own, so
    /// switching back returns to the tab left there.
    fn tab(&self) -> u8 {
        match self.canvas.mode() {
            Mode::Edit => self.settings.sidebar_tab,
            Mode::Present => self.settings.present_tab,
        }
    }

    /// Sets the open tab of the current mode.
    fn set_tab(&mut self, tab: u8) {
        match self.canvas.mode() {
            Mode::Edit => self.settings.sidebar_tab = tab,
            Mode::Present => self.settings.present_tab = tab,
        }
    }

    /// The tabs for the current mode.
    fn tabs(&self) -> &'static [&'static str] {
        match self.canvas.mode() {
            Mode::Edit => &["Geometry", "Items", "Layers"],
            Mode::Present => &["Styles", "Materials", "Render"],
        }
    }

    /// The open tab's panel. It slides out from under its tab's tip over
    /// the canvas ([`PANEL_SLIDE_SECS`]), and back in when it closes; it
    /// runs from the top of the canvas down to `bottom` (above Operation
    /// Inputs), and at least to the foot of its tab. Its border is left open
    /// where the tab joins it. It stays open until its tab is clicked or N
    /// is pressed.
    fn side_panel(&mut self, ui: &mut Ui, view: egui::Rect, bottom: f32) {
        let open = self.settings.sidebar_open && usize::from(self.tab()) < self.tabs().len();
        if open {
            self.shown_tab = self.tab();
        }
        let out =
            ui.ctx()
                .animate_bool_with_time(egui::Id::new("panel_slide"), open, PANEL_SLIDE_SECS);
        let tab = usize::from(self.shown_tab);
        if out <= 0.0 || tab >= self.tabs().len() {
            return;
        }
        let right = self.settings.panel_side == PanelSide::Right;
        let joint = self.tab_layout.get(tab).map(|(r, _)| *r);
        // The panel's edge meets the tab's tip.
        let tip = match (joint, right) {
            (Some(j), true) => j.left(),
            (Some(j), false) => j.right(),
            (None, true) => view.right(),
            (None, false) => view.left(),
        };
        let travel = (1.0 - out) * PANEL_WIDTH;
        let (x, clip) = if right {
            (
                tip - PANEL_WIDTH + travel,
                egui::Rect::from_x_y_ranges(view.left()..=tip, view.y_range()),
            )
        } else {
            (
                tip - travel,
                egui::Rect::from_x_y_ranges(tip..=view.right(), view.y_range()),
            )
        };
        let reach = joint.map_or(0.0, |j| j.bottom() + TAB_GAP);
        let height = (bottom - view.top() - 20.0)
            .max(80.0)
            .max(reach - view.top());
        let mode = self.canvas.mode();
        let fill = ui.visuals().panel_fill;
        let border = ui.visuals().widgets.noninteractive.bg_stroke;
        egui::Area::new(egui::Id::new("side_panel"))
            .fixed_pos(egui::pos2(x, view.top()))
            .order(egui::Order::Middle)
            .show(ui.ctx(), |ui| {
                ui.set_clip_rect(clip);
                let shown = egui::Frame::new()
                    .fill(fill)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.set_width(PANEL_WIDTH - 12.0);
                        ui.set_height(height);
                        match (mode, tab) {
                            (Mode::Edit, 0) => self.panel_contents(ui),
                            (Mode::Edit, 1) => {
                                ScrollArea::vertical().show(ui, |ui| self.items_panel(ui));
                            }
                            (Mode::Edit, _) => {
                                ScrollArea::vertical().show(ui, |ui| self.layers_panel(ui));
                            }
                            (Mode::Present, 0) => {
                                ScrollArea::vertical().show(ui, |ui| self.styles_panel(ui));
                            }
                            (Mode::Present, 1) => {
                                ScrollArea::vertical().show(ui, |ui| self.materials_panel(ui));
                            }
                            (Mode::Present, _) => {
                                ScrollArea::vertical().show(ui, |ui| self.render_panel(ui));
                            }
                        }
                    });
                // The border, inside the panel's edge, open along the tab.
                let r = shown.response.rect.shrink(0.5);
                let painter = ui.painter().clone();
                let (near, far) = if right {
                    (r.right(), r.left())
                } else {
                    (r.left(), r.right())
                };
                painter.line_segment([r.left_top(), r.right_top()], border);
                painter.line_segment([r.left_bottom(), r.right_bottom()], border);
                painter.line_segment(
                    [egui::pos2(far, r.top()), egui::pos2(far, r.bottom())],
                    border,
                );
                let gap = joint.map(|j| {
                    (
                        j.top() + f32::from(TAB_RADIUS),
                        j.bottom() - f32::from(TAB_RADIUS),
                    )
                });
                match gap {
                    Some((a, b)) => {
                        painter
                            .line_segment([egui::pos2(near, r.top()), egui::pos2(near, a)], border);
                        painter.line_segment(
                            [egui::pos2(near, b), egui::pos2(near, r.bottom())],
                            border,
                        );
                    }
                    None => {
                        painter.line_segment(
                            [egui::pos2(near, r.top()), egui::pos2(near, r.bottom())],
                            border,
                        );
                    }
                }
            });
    }

    /// Operation Inputs: a fixed box in the canvas's bottom corner on the
    /// panel side, apart from the panels so it shows with every panel
    /// closed; collapsed to its title when there is nothing to show. Returns
    /// the box's top edge.
    fn operation_inputs_box(&mut self, ui: &mut Ui, view: egui::Rect) -> f32 {
        if !self.canvas.has_inputs() {
            return view.bottom();
        }
        let right = self.settings.panel_side == PanelSide::Right;
        let units = self.settings.units;
        let (pivot, corner) = if right {
            (egui::Align2::RIGHT_BOTTOM, view.right_bottom())
        } else {
            (egui::Align2::LEFT_BOTTOM, view.left_bottom())
        };
        let r = egui::Area::new(egui::Id::new("operation_inputs_box"))
            .fixed_pos(corner)
            .pivot(pivot)
            .order(egui::Order::Middle)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.set_width(PANEL_WIDTH - 14.0);
                        self.canvas.operation_inputs(ui, &units);
                    });
            });
        r.response.rect.top()
    }

    /// The Items panel: the selection's name and figures, the Select row,
    /// its commands, then every item in the context being
    /// edited — each connected piece of loose geometry (named by shape), then
    /// each group and component with what is nested in it indented beneath.
    /// Each row has an eye (dark: shown, grey: hidden); a click selects;
    /// double-clicking a group's or component's name renames it (applied
    /// when the field is left); "…" holds the row's commands. Hidden items
    /// stay listed, so they can be selected and shown again.
    fn items_panel(&mut self, ui: &mut Ui) {
        use crate::canvas::OutlinerChange;
        use crate::commands::{Command, ITEMS_PANEL};
        // The selection's name first, then the Statistics figures, as in the
        // overlay (which steps aside while this panel is open).
        let lines = self.statistics_lines();
        if let Some((name, figures)) = lines.split_first() {
            ui.label(egui::RichText::new(name).size(14.0).strong());
            for line in figures {
                ui.label(egui::RichText::new(line).size(12.0));
            }
        }
        ui.separator();
        // The Select row, as in the Geometry panel, so selecting needs no
        // change of tab.
        ui.horizontal_wrapped(|ui| {
            for name in SELECT_ROW {
                if let Some(slot) = find_slot(name) {
                    self.tool_button(ui, slot);
                }
            }
        });
        ui.separator();
        // Commands as icons; Hide and Unhide show as one, whichever applies
        // (as do Edit Shape and Edit Extrude / Loft / Revolve),
        // and Lock follows the selection's state. Hover names each.
        let mut run = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for c in ITEMS_PANEL {
                let applies = self.canvas.command_applies(*c);
                let skip = match c {
                    Command::Hide => self.canvas.command_applies(Command::Unhide),
                    Command::Unhide => !applies,
                    // Edit Shape and Edit Extrude / Loft / Revolve show as
                    // one, whichever applies.
                    Command::EditShape => self.canvas.command_applies(Command::EditSweep),
                    Command::EditSweep => !applies,
                    _ => false,
                };
                if skip {
                    continue;
                }
                let label = self.canvas.command_label(*c);
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
                let visuals = ui.visuals();
                let fill = if applies && response.hovered() {
                    visuals.widgets.hovered.weak_bg_fill
                } else {
                    visuals.widgets.inactive.weak_bg_fill
                };
                let colour = if applies {
                    visuals.text_color()
                } else {
                    visuals.weak_text_color()
                };
                ui.painter().rect_filled(rect, 3.0, fill);
                crate::icons::paint(ui.painter(), rect, &label, colour);
                let response = response.on_hover_text(label.clone());
                if applies && response.clicked() {
                    run = Some(*c);
                }
            }
        });
        if let Some(c) = run {
            self.canvas.run_command(c);
        }
        ui.separator();

        enum Act {
            SelectPiece(usize),
            PieceHidden(usize, bool),
            Select(Vec<smith_model::InstanceId>),
            Change(Vec<smith_model::InstanceId>, OutlinerChange),
        }
        let mut act: Option<Act> = None;
        let pieces: Vec<(String, bool, bool)> = self
            .canvas
            .pieces()
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name.clone(), p.hidden, self.canvas.piece_selected(i)))
            .collect();
        let rows = self.canvas.outliner();
        if pieces.is_empty() && rows.is_empty() {
            ui.weak("Nothing drawn yet.");
            return;
        }
        for (i, (name, hidden, selected)) in pieces.iter().enumerate() {
            ui.horizontal(|ui| {
                if eye(ui, !hidden, false) {
                    act = Some(Act::PieceHidden(i, !hidden));
                }
                let mut text = egui::RichText::new(name);
                if *hidden {
                    text = text.weak();
                }
                if ui.selectable_label(*selected, text).clicked() {
                    act = Some(Act::SelectPiece(i));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.menu_button("…", |ui| {
                        let label = if *hidden { "Unhide" } else { "Hide" };
                        if ui.button(label).clicked() {
                            act = Some(Act::PieceHidden(i, !hidden));
                            ui.close();
                        }
                    });
                });
            });
        }
        let editing_key = egui::Id::new("items_renaming");
        let renaming: Option<Vec<smith_model::InstanceId>> =
            ui.data_mut(|d| d.get_temp(editing_key));
        for row in &rows {
            ui.horizontal(|ui| {
                ui.add_space(12.0 * (row.path.len() - 1) as f32);
                if eye(ui, !row.hidden, false) {
                    act = Some(Act::Change(
                        row.path.clone(),
                        OutlinerChange::Hide(!row.hidden),
                    ));
                }
                let label = if row.name.is_empty() {
                    if row.component { "Component" } else { "Group" }.to_string()
                } else {
                    row.name.clone()
                };
                if renaming.as_ref() == Some(&row.path) {
                    let key = egui::Id::new(("items_name", row.path.clone()));
                    let mut text = ui
                        .data_mut(|d| d.get_temp::<String>(key))
                        .unwrap_or_else(|| row.name.clone());
                    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0));
                    r.request_focus();
                    ui.data_mut(|d| d.insert_temp(key, text.clone()));
                    if r.lost_focus() {
                        ui.data_mut(|d| {
                            d.remove::<String>(key);
                            d.remove::<Vec<smith_model::InstanceId>>(editing_key);
                        });
                        if text.trim() != row.name {
                            act = Some(Act::Change(
                                row.path.clone(),
                                OutlinerChange::Rename(text.trim().to_string()),
                            ));
                        }
                    }
                } else {
                    let mut text = egui::RichText::new(label);
                    if row.component {
                        text = text.italics();
                    }
                    if row.hidden {
                        text = text.weak();
                    }
                    if row.open {
                        text = text.strong();
                    }
                    let r = ui.selectable_label(row.selected, text);
                    if r.double_clicked() {
                        let path = row.path.clone();
                        ui.data_mut(|d| d.insert_temp(editing_key, path));
                    } else if r.clicked() {
                        act = Some(Act::Select(row.path.clone()));
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.menu_button("…", |ui| {
                        let hide = if row.hidden { "Unhide" } else { "Hide" };
                        if ui.button(hide).clicked() {
                            act = Some(Act::Change(
                                row.path.clone(),
                                OutlinerChange::Hide(!row.hidden),
                            ));
                            ui.close();
                        }
                        let lock = if row.locked { "Unlock" } else { "Lock" };
                        if ui.button(lock).clicked() {
                            act = Some(Act::Change(
                                row.path.clone(),
                                OutlinerChange::Lock(!row.locked),
                            ));
                            ui.close();
                        }
                    });
                });
            });
        }
        match act {
            Some(Act::SelectPiece(i)) => self.canvas.select_piece(i),
            Some(Act::PieceHidden(i, h)) => self.canvas.set_piece_hidden(i, h),
            Some(Act::Select(path)) => self.canvas.outliner_select(&path),
            Some(Act::Change(path, c)) => self.canvas.outliner_change(&path, c),
            None => {}
        }
    }

    /// The Layers panel, GIMP style: one full-width row per layer, in stack
    /// order, top first, Base fixed at the bottom. Each row but Base's has a
    /// grip: drag it onto another row to move the layer there (above or
    /// below that row by the half dropped on; onto Base, just above it), one
    /// undo step. Beside the eye, a padlock locks the layer: what is on it
    /// cannot be selected or changed, and it cannot be made active; the
    /// active layer cannot be locked. A right-click on a layer's name opens
    /// its menu: Select Contents, Duplicate, Scale Layer, Merge Down and Anchor to Base
    /// (not on Base), Merge Visible and Flatten; Remove deletes the layer and
    /// what is on it, asking first when anything is. The active layer's row (where new geometry goes) is filled
    /// with a strong selection blue; Base's row is shaded 30% darker when it is
    /// not active. Each row has an eye (dark: shown, grey: hidden; the active
    /// layer cannot be hidden), its name and Remove. A click on
    /// the name makes the layer active; a double-click renames it (applied
    /// when the field is left). Add Layer adds a layer named by its number
    /// just above the active one and makes it active. Each change but the choice of active layer is one
    /// undo step.
    fn layers_panel(&mut self, ui: &mut Ui) {
        enum Change {
            Set(smith_model::LayerId, smith_model::Layer),
            Remove(smith_model::LayerId),
            Activate(Option<smith_model::LayerId>),
            MoveHere(Option<smith_model::LayerId>),
            Reorder(smith_model::LayerId, usize),
            SelectContents(Option<smith_model::LayerId>),
            Duplicate(smith_model::LayerId),
            Anchor(smith_model::LayerId),
            Scale(Option<smith_model::LayerId>),
            MergeDown(smith_model::LayerId),
            MergeVisible,
            Flatten,
            Add,
        }
        let doc = self.canvas.document();
        let base = doc.base_layer();
        let active = doc.active_layer();
        // Top of the stack first; Base, at the bottom, last.
        let mut rows: Vec<(smith_model::LayerId, smith_model::Layer)> = doc
            .layer_ids()
            .filter_map(|l| doc.layer(l).map(|x| (l, x.clone())))
            .collect();
        rows.reverse();
        // The stack above Base, bottom up, for working out where a drop
        // lands.
        let stack: Vec<smith_model::LayerId> =
            doc.layer_ids().filter(|l| Some(*l) != base).collect();
        let dragging = egui::DragAndDrop::has_payload_of_type::<u64>(ui.ctx());
        let renaming_key = egui::Id::new("layer_renaming");
        let renaming: Option<u64> = ui.data_mut(|d| d.get_temp(renaming_key));
        // A strong blue, so the active row's white name reads clearly.
        let selection_fill = egui::Color32::from_rgb(52, 112, 222);
        let panel = ui.visuals().panel_fill;
        let base_fill = egui::Color32::from_rgb(
            (f32::from(panel.r()) * 0.7) as u8,
            (f32::from(panel.g()) * 0.7) as u8,
            (f32::from(panel.b()) * 0.7) as u8,
        );
        let mut change: Option<Change> = None;
        // Put the selection on the active layer.
        let has_selection = !self.canvas.selection().is_empty();
        if has_selection
            && ui
                .button("Move Selection Here")
                .on_hover_text("Puts the selection on the active layer")
                .clicked()
        {
            change = Some(Change::MoveHere(active));
        }
        ui.add_space(4.0);
        for (id, layer) in rows {
            let key = if Some(id) == base { None } else { Some(id) };
            let is_active = key == active;
            let fill = if is_active {
                selection_fill
            } else if key.is_none() {
                base_fill
            } else {
                egui::Color32::TRANSPARENT
            };
            let text_colour = if is_active {
                egui::Color32::WHITE
            } else {
                ui.visuals().text_color()
            };
            let row = egui::Frame::new()
                .fill(fill)
                .corner_radius(3.0)
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if key.is_some() {
                            let drag = egui::Id::new(("layer_drag", id.raw()));
                            ui.dnd_drag_source(drag, id.raw(), |ui| grip(ui, text_colour))
                                .response
                                .on_hover_text("Drag to move the layer");
                        } else {
                            ui.add_space(GRIP_WIDTH + ui.spacing().item_spacing.x);
                        }
                        let mut l = layer.clone();
                        if eye(ui, l.visible, is_active) {
                            l.visible = !l.visible;
                            change = Some(Change::Set(id, l.clone()));
                        }
                        // The active layer cannot be locked (it can be
                        // unlocked, should a file have it locked).
                        if padlock(ui, l.locked, is_active && !l.locked) {
                            l.locked = !l.locked;
                            change = Some(Change::Set(id, l.clone()));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if key.is_some()
                                && ui
                                    .small_button("Remove")
                                    .on_hover_text("The layer and what is on it are deleted")
                                    .clicked()
                            {
                                change = Some(Change::Remove(id));
                            }
                            if renaming == Some(id.raw()) && key.is_some() {
                                let tkey = egui::Id::new(("layer_name", id.raw()));
                                let mut text = ui
                                    .data_mut(|d| d.get_temp::<String>(tkey))
                                    .unwrap_or_else(|| layer.name.clone());
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut text)
                                        .desired_width(ui.available_width()),
                                );
                                r.request_focus();
                                ui.data_mut(|d| d.insert_temp(tkey, text.clone()));
                                if r.lost_focus() {
                                    ui.data_mut(|d| {
                                        d.remove::<String>(tkey);
                                        d.remove::<u64>(renaming_key);
                                    });
                                    let name = text.trim();
                                    if !name.is_empty() && name != layer.name {
                                        change = Some(Change::Set(
                                            id,
                                            smith_model::Layer {
                                                name: name.to_string(),
                                                ..l.clone()
                                            },
                                        ));
                                    }
                                }
                            } else {
                                // The name fills the rest of the row: a click
                                // anywhere there makes the layer active.
                                let (rect, r) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 18.0),
                                    egui::Sense::click(),
                                );
                                ui.painter().text(
                                    rect.left_center(),
                                    egui::Align2::LEFT_CENTER,
                                    &layer.name,
                                    egui::FontId::proportional(13.0),
                                    text_colour,
                                );
                                let r = r.on_hover_text(
                                    "Click: draw on this layer · Double-click: rename · \
                                     Right-click: more",
                                );
                                r.context_menu(|ui| {
                                    if ui.button("Select Contents").clicked() {
                                        change = Some(Change::SelectContents(key));
                                        ui.close();
                                    }
                                    if ui.button("Duplicate").clicked() {
                                        change = Some(Change::Duplicate(id));
                                        ui.close();
                                    }
                                    if ui.button("Scale Layer").clicked() {
                                        change = Some(Change::Scale(key));
                                        ui.close();
                                    }
                                    ui.separator();
                                    // Base has nothing below it to go to.
                                    if key.is_some() {
                                        if ui.button("Merge Down").clicked() {
                                            change = Some(Change::MergeDown(id));
                                            ui.close();
                                        }
                                        if ui.button("Anchor to Base").clicked() {
                                            change = Some(Change::Anchor(id));
                                            ui.close();
                                        }
                                    }
                                    if ui.button("Merge Visible").clicked() {
                                        change = Some(Change::MergeVisible);
                                        ui.close();
                                    }
                                    if ui.button("Flatten").clicked() {
                                        change = Some(Change::Flatten);
                                        ui.close();
                                    }
                                });
                                if r.double_clicked() && key.is_some() {
                                    ui.data_mut(|d| d.insert_temp(renaming_key, id.raw()));
                                } else if r.clicked() {
                                    change = Some(Change::Activate(key));
                                }
                            }
                        });
                    });
                });
            // A drop on this row: above or below it by the half the
            // pointer is in; on Base, just above Base.
            let r = &row.response;
            let upper = r
                .hover_pos()
                .or_else(|| ui.ctx().pointer_interact_pos())
                .is_some_and(|p| p.y < r.rect.center().y);
            let landing = |moved: u64| -> Option<(smith_model::LayerId, usize)> {
                let dragged = stack.iter().copied().find(|l| l.raw() == moved)?;
                if dragged == id {
                    return None;
                }
                let others: Vec<_> = stack.iter().copied().filter(|l| *l != dragged).collect();
                let at = match others.iter().position(|l| *l == id) {
                    Some(k) if upper => k + 1,
                    Some(k) => k,
                    None => 0,
                };
                Some((dragged, at))
            };
            if dragging
                && let Some(moved) = r.dnd_hover_payload::<u64>()
                && landing(*moved).is_some()
            {
                let y = if upper && key.is_some() {
                    r.rect.top()
                } else if key.is_some() {
                    r.rect.bottom()
                } else {
                    r.rect.top()
                };
                ui.painter()
                    .hline(r.rect.x_range(), y, egui::Stroke::new(2.0, selection_fill));
            }
            if let Some(moved) = r.dnd_release_payload::<u64>()
                && let Some((dragged, at)) = landing(*moved)
            {
                change = Some(Change::Reorder(dragged, at));
            }
        }
        ui.add_space(4.0);
        if ui.button("Add Layer").clicked() {
            change = Some(Change::Add);
        }
        match change {
            Some(Change::Set(id, l)) => {
                self.canvas.layer_op("Change Layer", |d| d.set_layer(id, l))
            }
            Some(Change::Remove(id)) => self.canvas.remove_layer(id),
            Some(Change::Activate(l)) => self.canvas.set_active_layer(l),
            Some(Change::Add) => self.canvas.add_layer(),
            Some(Change::MoveHere(l)) => self.canvas.assign_layer(l),
            Some(Change::Reorder(id, at)) => self.canvas.reorder_layer(id, at),
            Some(Change::SelectContents(l)) => self.canvas.select_layer_contents(l),
            Some(Change::Duplicate(id)) => self.canvas.duplicate_layer(id),
            Some(Change::Anchor(id)) => self.canvas.anchor_layer(id),
            Some(Change::Scale(l)) => self.canvas.scale_layer(l),
            Some(Change::MergeDown(id)) => self.canvas.merge_layer_down(id),
            Some(Change::MergeVisible) => self.canvas.merge_visible_layers(),
            Some(Change::Flatten) => self.canvas.flatten_layers(),
            None => {}
        }
    }

    /// Present-mode Look settings. Changes apply immediately and
    /// never alter the geometry.
    /// The Render tab: Save Render, then the saved renders in order. Each
    /// is one version of how the item is shown — camera, mode, display
    /// style, layers and section plane. Click one to show it; double-click
    /// its name to rename it; its menu updates it to the current view or
    /// removes it.
    fn render_panel(&mut self, ui: &mut Ui) {
        if ui
            .button("Save Render")
            .on_hover_text(
                "Saves the camera, the mode and style, the layers shown and the section plane",
            )
            .clicked()
        {
            self.canvas.add_scene();
        }
        ui.add_space(4.0);
        let scenes = self.canvas.scene_list();
        if scenes.is_empty() {
            ui.weak("No renders saved.");
        }
        for (id, name) in scenes {
            ui.horizontal(|ui| {
                if self.renaming_scene.as_ref().is_some_and(|(r, _)| *r == id) {
                    let mut done = false;
                    if let Some((_, text)) = self.renaming_scene.as_mut() {
                        let edit = ui.text_edit_singleline(text);
                        edit.request_focus();
                        done = edit.lost_focus();
                    }
                    if done && let Some((_, text)) = self.renaming_scene.take() {
                        self.canvas.rename_scene(id, &text);
                    }
                    return;
                }
                let row = ui
                    .selectable_label(false, &name)
                    .on_hover_text("Click: show · Double-click: rename");
                if row.double_clicked() {
                    self.renaming_scene = Some((id, name.clone()));
                } else if row.clicked() {
                    self.canvas.show_scene(id);
                }
                ui.menu_button("…", |ui| {
                    if ui.button("Update to This Render").clicked() {
                        self.canvas.update_scene(id);
                        ui.close();
                    }
                    if ui.button("Remove").clicked() {
                        self.canvas.remove_scene(id);
                        ui.close();
                    }
                });
            });
        }
    }

    /// The Materials tab (Present mode): the file's materials, None first. A
    /// click makes one the Paint tool's current material (its values show in
    /// Operation Inputs); right-click offers Replace with Current. New Colour
    /// adds a plain material; Load Image… adds one from a PNG or JPEG.
    fn materials_panel(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("New Colour").clicked() {
                self.canvas.add_material(smith_model::Material::default());
            }
            if ui.button("Load Image…").clicked() {
                self.files.start_dialog(Purpose::LoadImage);
            }
        });
        ui.separator();
        let current = self.canvas.paint_material();
        let doc = self.canvas.document();
        let rows: Vec<(smith_model::MaterialId, String, [f32; 3], bool)> = doc
            .material_ids()
            .filter_map(|id| {
                let m = doc.material(id)?;
                Some((id, m.name.clone(), m.color, m.texture.is_some()))
            })
            .collect();
        if ui.selectable_label(current.is_none(), "None").clicked() {
            self.canvas.use_material(None);
        }
        let mut replace = None;
        for (id, name, color, textured) in rows {
            ui.horizontal(|ui| {
                // A swatch of the colour (shown in sRGB).
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                let c =
                    egui::Rgba::from_rgb(color[0].min(1.0), color[1].min(1.0), color[2].min(1.0));
                ui.painter().rect_filled(rect, 2.0, egui::Color32::from(c));
                let label = if textured {
                    format!("{name} (image)")
                } else {
                    name
                };
                let r = ui.selectable_label(current == Some(id), label);
                if r.clicked() {
                    self.canvas.use_material(Some(id));
                }
                r.context_menu(|ui| {
                    if ui.button("Replace with Current").clicked() {
                        replace = Some(id);
                        ui.close();
                    }
                });
            });
        }
        if let Some(old) = replace {
            self.canvas.replace_with_current(old);
        }
    }

    /// Adds the PNG or JPEG at `path` as a new material, 100 mm wide, and
    /// makes it current.
    fn load_image(&mut self, path: &std::path::Path) {
        match smith_io::load_image(path) {
            Ok(image) => {
                let name = path
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.canvas.add_material(smith_model::Material {
                    name,
                    color: [1.0, 1.0, 1.0],
                    texture: Some(smith_model::Texture {
                        image: std::sync::Arc::new(image),
                        width_mm: 100.0,
                    }),
                    ..smith_model::Material::default()
                });
            }
            Err(e) => self.files.notify("Load Image", e.to_string()),
        }
    }

    /// The Styles tab (Present mode): the Select row (Select, Select
    /// Connected, Select by Type, Undo, Redo), the look tools (Paint, Mark
    /// Edges, Line Style), then shading, edges and background, display choices
    /// only. The display style is on the top bar; the smoothing angle
    /// belongs to the model and is set with Mark Edges.
    fn styles_panel(&mut self, ui: &mut Ui) {
        // The Select row, as in Edit mode (without its modes, which are
        // Edit mode's, nor Cut, Copy and Paste, which change geometry), then
        // the look tools, as icons.
        ui.horizontal_wrapped(|ui| {
            for name in SELECT_ROW
                .iter()
                .filter(|n| !SELECT_MODES.contains(n) && !EDIT_ONLY.contains(n))
            {
                if let Some(slot) = find_slot(name) {
                    self.tool_button(ui, slot);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            for name in ["Paint", "Mark Edges", "Line Style"] {
                if let Some(slot) = find_slot(name) {
                    self.tool_button(ui, slot);
                }
            }
        });
        ui.separator();
        let mut look = self.canvas.look();
        egui::Grid::new("look").num_columns(2).show(ui, |ui| {
            ui.label("Shading");
            ui.horizontal(|ui| {
                ui.radio_value(&mut look.mesh.smooth_shading, true, "Smooth");
                ui.radio_value(&mut look.mesh.smooth_shading, false, "Flat");
            });
            ui.end_row();
            ui.label("Edges");
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(&mut look.mesh.edges, EdgeDisplay::Sharp, "Sharp");
                ui.radio_value(&mut look.mesh.edges, EdgeDisplay::All, "All");
                ui.radio_value(&mut look.mesh.edges, EdgeDisplay::None, "None");
            });
            ui.end_row();
            ui.label("Background");
            let mut rgb = [look.background[0], look.background[1], look.background[2]];
            ui.color_edit_button_rgb(&mut rgb);
            look.background = [rgb[0], rgb[1], rgb[2], 1.0];
            ui.end_row();
        });
        self.canvas.set_look(look);
    }

    // ----- windows -------------------------------------------------------------

    fn preferences_window(&mut self, ui: &mut Ui) {
        let mut open = self.show_preferences;
        egui::Window::new("Preferences")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.heading("Keys");
                self.preset_radios(ui);
                for w in &self.keymap.warnings {
                    ui.colored_label(egui::Color32::from_rgb(170, 90, 0), w);
                }
                ui.add_space(6.0);
                ui.heading("Operations Bar Side");
                self.panel_side_radios(ui);
                ui.add_space(6.0);
                ui.heading("Units");
                let before = self.settings.units;
                let u = &mut self.settings.units;
                egui::ComboBox::from_id_salt("unit")
                    .selected_text(u.unit.label())
                    .show_ui(ui, |ui| {
                        for unit in LengthUnit::ALL {
                            ui.selectable_value(&mut u.unit, unit, unit.label());
                        }
                    });
                match u.unit {
                    LengthUnit::FractionalInch | LengthUnit::FeetInches => {
                        ui.horizontal(|ui| {
                            ui.label("Precision 1/");
                            let mut denom = 1u32 << u.fraction_bits;
                            egui::ComboBox::from_id_salt("frac")
                                .selected_text(denom.to_string())
                                .show_ui(ui, |ui| {
                                    for bits in 1..=6u8 {
                                        ui.selectable_value(
                                            &mut denom,
                                            1 << bits,
                                            (1u32 << bits).to_string(),
                                        );
                                    }
                                });
                            u.fraction_bits = denom.trailing_zeros() as u8;
                            ui.label("inch");
                        });
                    }
                    _ => {
                        ui.horizontal(|ui| {
                            ui.label("Decimal places");
                            ui.add(egui::DragValue::new(&mut u.decimals).range(0..=6));
                        });
                    }
                }
                ui.weak(
                    "Values are stored at full precision; this only changes how they are shown.",
                );
                if self.settings.units != before {
                    self.settings.save(&self.paths.config);
                }
                ui.add_space(6.0);
                ui.heading("Autosave");
                ui.horizontal(|ui| {
                    ui.label("Every");
                    let before = self.settings.autosave_minutes;
                    ui.add(
                        egui::DragValue::new(&mut self.settings.autosave_minutes)
                            .range(0..=60)
                            .suffix(" min"),
                    );
                    if self.settings.autosave_minutes != before {
                        self.settings.save(&self.paths.config);
                        self.autosave_due = None;
                    }
                });
                ui.weak("Unsaved work is copied to the State folder. 0 turns autosave off.");
                ui.add_space(6.0);
                ui.heading("Save Versions");
                let before = self.settings.save_versions;
                ui.add(egui::DragValue::new(&mut self.settings.save_versions).range(0..=32))
                    .on_hover_text(
                        "The number of old versions to keep beside the file when saving over it",
                    );
                if self.settings.save_versions != before {
                    self.settings.save(&self.paths.config);
                }
                ui.add_space(6.0);
                ui.heading("Folders");
                for (label, path) in [
                    ("Config", &self.paths.config),
                    ("Data", &self.paths.data),
                    ("State", &self.paths.state),
                ] {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("{label}:"));
                        ui.monospace(path.display().to_string());
                    });
                }
            });
        self.show_preferences = open;
    }

    /// About window.
    fn about_window(&mut self, ui: &mut Ui) {
        egui::Window::new(format!("About {APP_NAME}"))
            .open(&mut self.show_about)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.heading(APP_NAME);
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("Sketch ideas in 3D and hand them off.");
                ui.add_space(6.0);
                ui.label("Developer: archerprojects");
                ui.hyperlink_to(
                    "archer.projects@proton.me",
                    "mailto:archer.projects@proton.me",
                );
                ui.add_space(6.0);
                ui.label("Licence: GPL-3.0-or-later");
                ui.hyperlink(env!("CARGO_PKG_REPOSITORY"));
                ui.add_space(6.0);
                ui.small("Not affiliated with or endorsed by Trimble Inc.");
            });
    }

    // ----- shared helpers ------------------------------------------------------

    /// A menu item that performs a canvas action and shows its key binding.
    fn action_item(&mut self, ui: &mut Ui, label: &str, action: Action) {
        self.action_item_enabled(ui, label, action, true);
    }

    /// A menu item for an action, greyed out unless `enabled`.
    fn action_item_enabled(&mut self, ui: &mut Ui, label: &str, action: Action, enabled: bool) {
        let mut button = Button::new(label);
        if let Some(keys) = self.keymap.label(action) {
            button = button.shortcut_text(keys);
        }
        if ui.add_enabled(enabled, button).clicked() {
            let ctx = ui.ctx().clone();
            self.run(&ctx, action);
        }
    }

    /// Runs `action`. Copy and Cut also put a short marker on the system
    /// clipboard: the toolkit only reports Ctrl+V when the system clipboard
    /// holds text, so this keeps Paste working.
    fn run(&mut self, ctx: &egui::Context, action: Action) {
        self.canvas.apply(action);
        if matches!(action, Action::Copy | Action::Cut) {
            ctx.copy_text(format!("{APP_NAME} selection"));
        }
    }

    /// Keymap choice and the Keys list; a change is saved immediately.
    fn preset_radios(&mut self, ui: &mut Ui) {
        let before = self.settings.keymap;
        for p in [
            Preset::SketchSmith,
            Preset::SketchUp,
            Preset::Blender,
            Preset::Custom,
        ] {
            ui.radio_value(&mut self.settings.keymap, p, p.label());
        }
        if self.settings.keymap != before {
            self.keymap = Keymap::load(self.settings.keymap, &self.paths.config);
            self.key_capture = None;
            self.key_conflict = None;
            self.settings.save(&self.paths.config);
        }
        if self.settings.keymap == Preset::Custom
            && ui
                .button(format!("Reset All to {}", self.keymap.based_on.label()))
                .clicked()
        {
            let base = self.keymap.based_on;
            let fresh = Keymap::load(base, &self.paths.config).into_custom();
            self.keymap = fresh;
            self.save_custom_keys();
        }
        self.keys_list(ui);
    }

    /// Every action with its key and tap. A click on a key waits for the
    /// next key pressed; a key or tap another action has asks to swap.
    /// Changing a preset copies it into Custom first.
    fn keys_list(&mut self, ui: &mut Ui) {
        if let Some(action) = self.key_capture {
            ui.colored_label(
                egui::Color32::from_rgb(40, 90, 200),
                format!("Press a Key for {}: Esc Cancels", action.label()),
            );
            if let Some(got) = ui.input(captured_chord) {
                self.key_capture = None;
                if let Some(chord) = got {
                    self.offer_binding(action, Binding::Key(chord));
                }
            }
        }
        if let Some((action, binding, holder)) = self.key_conflict {
            let what = match binding {
                Binding::Key(c) => chord_label(c),
                Binding::Tap(t) => t.label().to_owned(),
            };
            ui.colored_label(
                egui::Color32::from_rgb(170, 90, 0),
                format!("{what} Is Used by {}", holder.label()),
            );
            ui.horizontal(|ui| {
                if ui.button("Swap").clicked() {
                    self.key_conflict = None;
                    self.change_keys(|k| k.assign(action, binding));
                }
                if ui.button("Cancel").clicked() {
                    self.key_conflict = None;
                }
            });
        }
        egui::CollapsingHeader::new("Keys")
            .default_open(false)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        egui::Grid::new("keys_grid")
                            .striped(true)
                            .num_columns(5)
                            .show(ui, |ui| {
                                ui.strong("Action");
                                ui.strong("Key");
                                ui.strong("Tap");
                                ui.label("");
                                ui.label("");
                                ui.end_row();
                                for (action, _, label) in crate::keymap::ACTIONS {
                                    let action = *action;
                                    ui.label(*label);
                                    let key =
                                        self.keymap.label(action).unwrap_or_else(|| "—".into());
                                    let waiting = self.key_capture == Some(action);
                                    if ui
                                        .add(
                                            Button::new(if waiting { "…" } else { key.as_str() })
                                                .selected(waiting),
                                        )
                                        .on_hover_text("Click, Then Press the Key")
                                        .clicked()
                                    {
                                        self.key_capture = Some(action);
                                        self.key_conflict = None;
                                    }
                                    let tap = self.keymap.taps.get(&action).copied();
                                    let mut chosen = tap;
                                    egui::ComboBox::from_id_salt(("tap", action.file_name()))
                                        .selected_text(tap.map_or("—", TapKey::label))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut chosen, None, "—");
                                            for t in TapKey::ALL {
                                                ui.selectable_value(
                                                    &mut chosen,
                                                    Some(t),
                                                    t.label(),
                                                );
                                            }
                                        });
                                    if chosen != tap {
                                        match chosen {
                                            Some(t) => self.offer_binding(action, Binding::Tap(t)),
                                            None => self.change_keys(|k| k.clear(action, true)),
                                        }
                                    }
                                    if ui.small_button("Clear").clicked() {
                                        self.change_keys(|k| k.clear(action, false));
                                    }
                                    if ui.small_button("Reset").clicked() {
                                        self.change_keys(|k| k.reset(action));
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
    }

    /// Gives `action` `binding`, or asks to swap when another action has it.
    fn offer_binding(&mut self, action: Action, binding: Binding) {
        match self.keymap.holder(binding) {
            Some(holder) if holder != action => self.key_conflict = Some((action, binding, holder)),
            Some(_) => {}
            None => self.change_keys(|k| k.assign(action, binding)),
        }
    }

    /// Changes the keys: a preset is copied into Custom first, and Custom is
    /// switched to and saved.
    fn change_keys(&mut self, change: impl FnOnce(&mut Keymap)) {
        if self.settings.keymap != Preset::Custom {
            self.keymap = self.keymap.clone().into_custom();
            self.settings.keymap = Preset::Custom;
            self.settings.save(&self.paths.config);
        }
        change(&mut self.keymap);
        self.save_custom_keys();
    }

    fn save_custom_keys(&mut self) {
        if let Err(e) = self.keymap.save_custom(&self.paths.config) {
            log::warn!("keymap: couldn't save the Custom keys: {e}");
            self.canvas
                .set_message(format!("Couldn't Save the Keys: {e}"));
        }
    }

    /// A lone tap of a right-hand modifier: the action it is bound to.
    pub fn key_tap(&mut self, key: TapKey) {
        if self.key_capture.is_some() || self.file_window_open() {
            return;
        }
        if let Some(action) = self.keymap.tap_action(key) {
            self.queued.push(action);
        }
    }

    /// Operations Bar Side radio buttons; a change is saved immediately.
    fn panel_side_radios(&mut self, ui: &mut Ui) {
        let before = self.settings.panel_side;
        ui.radio_value(&mut self.settings.panel_side, PanelSide::Right, "Right");
        ui.radio_value(&mut self.settings.panel_side, PanelSide::Left, "Left");
        if self.settings.panel_side != before {
            self.settings.save(&self.paths.config);
        }
    }

    /// The status/hint line for the current state.
    fn status_text(&self) -> String {
        if self.canvas.mode() == Mode::Present {
            match self.canvas.tool() {
                WorkTool::Paint => return "Click A Face · Alt-Click Samples".to_owned(),
                WorkTool::MarkEdges => return "Click Edges".to_owned(),
                WorkTool::LineStyle => return "Click Edges · Or Choose A Layer".to_owned(),
                _ => {}
            }
            return "Present mode: set the look · Tab returns to Edit".to_owned();
        }
        if self.customising {
            return "Customising toolbars: drag buttons within or between groups, or to Hidden · Done when finished".to_owned();
        }
        match self.canvas.tool() {
            WorkTool::Draw(_) | WorkTool::EditCurve => {
                if let Some(status) = self.canvas.tool_status() {
                    return status;
                }
            }
            WorkTool::Axes { origin: None, .. } => return "Axes: click the new origin".to_owned(),
            WorkTool::Axes { .. } => {
                return "Axes: click the red direction, then toward green".to_owned();
            }
            WorkTool::Select | WorkTool::Paint | WorkTool::MarkEdges | WorkTool::LineStyle => {}
        }
        match self.canvas.nav_tool() {
            NavTool::Orbit => "Orbit: drag with the left button. Esc to stop.".to_owned(),
            NavTool::Pan => "Pan: drag with the left button. Esc to stop.".to_owned(),
            NavTool::Zoom => "Zoom: drag up or down with the left button. Esc to stop.".to_owned(),
            NavTool::None => {
                let n = self.canvas.selection().len();
                let sel = if n == 0 {
                    String::new()
                } else {
                    format!("{n} selected · ")
                };
                format!("{sel}Click or drag to select · Middle drag: orbit · Wheel: zoom to cursor")
            }
        }
    }
}

/// A menu button whose popup opens *above* the button (bottom bar).
/// A tool group's heading in the panel: 2 pt above the small text size.
fn group_title(ui: &mut Ui, title: &str) {
    let size = ui
        .style()
        .text_styles
        .get(&egui::TextStyle::Small)
        .map_or(10.0, |f| f.size)
        + 2.0;
    ui.label(egui::RichText::new(title).size(size));
}

/// An Items row's name: the instance's or definition's, else its kind.
fn outliner_name(row: &crate::canvas::OutlinerRow) -> String {
    if !row.name.is_empty() {
        row.name.clone()
    } else if row.component {
        "Component".into()
    } else {
        "Group".into()
    }
}

/// The plural of an item's name, for "3 Boxes".
fn plural(name: &str) -> String {
    match name {
        "Torus" => "Tori".into(),
        n if ["s", "x", "sh", "ch"].iter().any(|e| n.ends_with(e)) => format!("{n}es"),
        n => format!("{n}s"),
    }
}

fn upward_menu(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    let response = ui.button(title);
    Popup::menu(&response)
        .align(RectAlign::TOP_START)
        .show(add_contents);
}

/// The save-status dot's colour.
fn status_colour(status: SaveStatus) -> egui::Color32 {
    match status {
        SaveStatus::NeverSaved => egui::Color32::from_rgb(220, 50, 47),
        SaveStatus::Unsaved => egui::Color32::from_rgb(230, 180, 20),
        SaveStatus::Saved => egui::Color32::from_rgb(60, 170, 70),
    }
}

/// Width of the Operations Bar.
const TAB_BAR_WIDTH: f32 = 30.0;
/// How far a tab reaches past the strip onto the canvas.
const TAB_PROTRUDE: f32 = 4.0;
/// Gap above the first tab and between tabs.
const TAB_GAP: f32 = 6.0;
/// Space before and after a tab's label, along the tab.
const TAB_TEXT_PAD: f32 = 10.0;
/// Corner radius of a tab's canvas-side corners.
const TAB_RADIUS: u8 = 4;
/// A closed tab (Blender Light's inactive tab on white).
const TAB_FILL: egui::Color32 = egui::Color32::from_gray(153);
/// A closed tab under the pointer.
const TAB_HOVER: egui::Color32 = egui::Color32::from_gray(176);
/// A closed tab's label.
const TAB_TEXT: egui::Color32 = egui::Color32::from_gray(26);
/// How long the pointer rests on a tab before its panel opens.
const TAB_HOVER_SECS: f64 = 0.25;
/// A click this soon after the pointer opened a tab does not close it.
const TAB_CLICK_GRACE_SECS: f64 = 0.6;
/// How long a panel takes to slide out or in.
const PANEL_SLIDE_SECS: f32 = 0.15;
/// Width of an open panel and of Operation Inputs.
const PANEL_WIDTH: f32 = 250.0;

/// Points around a quarter circle at a tab's canvas-side corner: centre
/// `c`, radius `r`; `right` for a bar on the right (the tip faces left),
/// `top` for the upper corner. Runs from the tab's top or bottom edge round
/// to its tip.
fn tip_arc(c: egui::Pos2, r: f32, right: bool, top: bool) -> Vec<egui::Pos2> {
    let sx = if right { -1.0 } else { 1.0 };
    let sy = if top { -1.0 } else { 1.0 };
    let steps = 4;
    (0..=steps)
        .map(|i| {
            let a = std::f32::consts::FRAC_PI_2 * i as f32 / steps as f32;
            egui::pos2(c.x + sx * r * a.sin(), c.y + sy * r * a.cos())
        })
        .collect()
}

/// A layer's eye, drawn in code: dark when shown, grey when hidden. Returns
/// whether it was clicked. With `locked`, it cannot be clicked (the active
/// layer is always shown).
/// Width of a Layers row's grip.
const GRIP_WIDTH: f32 = 10.0;

/// A Layers row's grip: two columns of three dots, drawn in `colour`.
fn grip(ui: &mut Ui, colour: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(GRIP_WIDTH, 16.0), egui::Sense::hover());
    let dot = colour.gamma_multiply(0.6);
    for col in 0..2 {
        for row in 0..3 {
            let c = egui::pos2(
                rect.left() + 3.0 + 4.0 * col as f32,
                rect.center().y - 4.0 + 4.0 * row as f32,
            );
            ui.painter().circle_filled(c, 1.1, dot);
        }
    }
}

fn eye(ui: &mut Ui, shown: bool, locked: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 16.0), egui::Sense::click());
    let colour = if shown {
        egui::Color32::from_gray(35)
    } else {
        egui::Color32::from_gray(175)
    };
    let c = rect.center();
    let (w, h) = (8.0, 4.5);
    // The lids: two arcs meeting at the corners.
    let lid = |sign: f32| -> Vec<egui::Pos2> {
        (0..=12)
            .map(|i| {
                let t = -1.0 + 2.0 * i as f32 / 12.0;
                egui::pos2(c.x + t * w, c.y + sign * h * (1.0 - t * t))
            })
            .collect()
    };
    let stroke = egui::Stroke::new(1.6, colour);
    ui.painter().add(egui::Shape::line(lid(-1.0), stroke));
    ui.painter().add(egui::Shape::line(lid(1.0), stroke));
    ui.painter().circle_filled(c, 2.4, colour);
    let response = response.on_hover_text(if locked {
        "The active layer is always shown"
    } else if shown {
        "Shown · click to hide"
    } else {
        "Hidden · click to show"
    });
    !locked && response.clicked()
}

/// A layer's padlock: closed and dark when locked, open and grey when not.
/// Returns whether it was clicked; `fixed` (the active layer, which cannot
/// be locked) takes no click.
fn padlock(ui: &mut Ui, locked: bool, fixed: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
    let colour = if locked {
        egui::Color32::from_gray(35)
    } else {
        egui::Color32::from_gray(175)
    };
    let c = rect.center();
    // The body, and the shackle: shut on it when locked, lifted when open.
    let body = egui::Rect::from_center_size(egui::pos2(c.x, c.y + 2.5), egui::vec2(9.0, 6.5));
    ui.painter().rect_filled(body, 1.0, colour);
    let lift = if locked { 0.0 } else { 2.5 };
    let shackle: Vec<egui::Pos2> = (0..=12)
        .map(|i| {
            let a = std::f32::consts::PI * i as f32 / 12.0;
            egui::pos2(c.x - 3.0 * a.cos(), body.top() - lift - 3.5 * a.sin())
        })
        .collect();
    let stroke = egui::Stroke::new(1.5, colour);
    ui.painter().add(egui::Shape::line(shackle, stroke));
    let foot = |x: f32| [egui::pos2(x, body.top() - lift), egui::pos2(x, body.top())];
    ui.painter().line_segment(foot(c.x + 3.0), stroke);
    if locked {
        ui.painter().line_segment(foot(c.x - 3.0), stroke);
    }
    let response = response.on_hover_text(if fixed {
        "The active layer cannot be locked"
    } else if locked {
        "Locked · click to unlock"
    } else {
        "Unlocked · click to lock"
    });
    !fixed && response.clicked()
}

/// A disabled menu item for a command not built yet.
/// The drawing of what `scope` takes, from `canvas`'s view (always
/// parallel), with its dimensions and text in `units`; the root's active
/// section plane cuts while `cuts` is on. Also whether the view was in
/// perspective.
fn drawing_of(
    canvas: &Canvas,
    units: smith_infer::units::UnitSettings,
    cuts: bool,
    scope: Scope<'_>,
) -> (Drawing, bool) {
    let v = canvas.saved_view();
    let view = DrawingView {
        centre: v.target,
        direction: v.target - v.eye,
        up: v.up,
    };
    let doc = canvas.document();
    let section = cuts
        .then(|| doc.root().active_section().map(|(_, s)| *s))
        .flatten();
    // Figures as the canvas writes them, in the Preferences units.
    let labels = move |m: Measure| match m {
        Measure::Length(mm) => format_length(mm, &units),
        Measure::Area(mm2) => format_area(mm2, &units),
        Measure::Angle(deg) => crate::canvas::format_angle(deg),
    };
    let mut d = smith_io::export::draw(doc, scope, &view, section.as_ref());
    d.marks = smith_io::export::marks(doc, scope, &view, &labels);
    (d, v.perspective)
}

/// A scale figure for show: up to three decimals, trailing zeros dropped.
fn ratio(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A chord as Preferences shows it.
fn chord_label(c: Chord) -> String {
    c.label()
}

/// The key pressed this frame, for binding: `Some(Some(chord))` a key,
/// `Some(None)` Esc (cancel), `None` nothing yet. Ctrl+X, C and V arrive as
/// clipboard events and are read as those keys.
fn captured_chord(i: &egui::InputState) -> Option<Option<Chord>> {
    use egui::{Key, Modifiers};
    // Space or Tab held: the next key pressed makes a hold chord; released
    // with none, the key itself.
    let held = HoldKey::ALL.into_iter().find(|h| i.key_down(h.key()));
    for e in &i.events {
        if let egui::Event::Key {
            key,
            pressed: false,
            ..
        } = e
            && let Some(h) = HoldKey::of(*key)
        {
            return Some(Some(Chord::new(Modifiers::NONE, h.key())));
        }
        if let egui::Event::Key {
            key,
            pressed: true,
            repeat: false,
            ..
        } = e
        {
            if HoldKey::of(*key).is_some() {
                continue;
            }
            if let Some(h) = held {
                return Some(Some(Chord {
                    modifiers: Modifiers::NONE,
                    key: *key,
                    hold: Some(h),
                }));
            }
        }
        let (key, m) = match e {
            egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } => (*key, *modifiers),
            egui::Event::Cut => (Key::X, Modifiers::COMMAND),
            egui::Event::Copy => (Key::C, Modifiers::COMMAND),
            egui::Event::Paste(_) => (Key::V, Modifiers::COMMAND),
            _ => continue,
        };
        if key == Key::Escape && m.is_none() {
            return Some(None);
        }
        let mut modifiers = Modifiers::NONE;
        if m.ctrl || m.command {
            modifiers |= Modifiers::COMMAND;
        }
        if m.shift {
            modifiers |= Modifiers::SHIFT;
        }
        if m.alt {
            modifiers |= Modifiers::ALT;
        }
        return Some(Some(Chord::new(modifiers, key)));
    }
    None
}

/// The action behind a panel button, if that button is live.
fn panel_action(name: &str) -> Option<Action> {
    match name {
        "Select" => Some(Action::SelectTool),
        "Paint" => Some(Action::PaintTool),
        "Mark Edges" => Some(Action::MarkEdgesTool),
        "Line Style" => Some(Action::LineStyleTool),
        "Orbit" => Some(Action::OrbitTool),
        "Pan" => Some(Action::PanTool),
        "Zoom" => Some(Action::ZoomTool),
        "Zoom Extents" => Some(Action::ZoomExtents),
        "Axes" => Some(Action::AxesTool),
        "Line" => Some(Action::LineTool),
        "Rectangle" => Some(Action::RectangleTool),
        "Rotated Rectangle" => Some(Action::RotatedRectangleTool),
        "Circle" => Some(Action::CircleTool),
        "Arc" => Some(Action::ArcTool),
        "Polygon" => Some(Action::PolygonTool),
        "Bézier Spline" => Some(Action::BezierTool),
        "Curve Maker" => Some(Action::CurveMakerTool),
        "Extrude" => Some(Action::ExtrudeTool),
        "Eraser" => Some(Action::EraserTool),
        "Move" => Some(Action::MoveTool),
        "Rotate" => Some(Action::RotateTool),
        "Scale" => Some(Action::ScaleTool),
        "Mirror" => Some(Action::MirrorTool),
        "Offset (Inset)" => Some(Action::OffsetTool),
        "Divide" => Some(Action::DivideTool),
        "Loop Cut" => Some(Action::LoopCutTool),
        "Knife" => Some(Action::KnifeTool),
        "Slide" => Some(Action::SlideTool),
        "Make Group" => Some(Action::MakeGroup),
        "Make Component" => Some(Action::MakeComponent),
        "Dissolve" => Some(Action::DissolveTool),
        "Tape Measure" => Some(Action::TapeMeasureTool),
        "Protractor" => Some(Action::ProtractorTool),
        "Find Center" => Some(Action::FindCenterTool),
        "Dimensions" => Some(Action::DimensionTool),
        "Text" => Some(Action::TextTool),
        "Section Plane" => Some(Action::SectionTool),
        "Extrude along Path" => Some(Action::SweepTool),
        "Revolve" => Some(Action::RevolveTool),
        "Extrude Edges along Path" => Some(Action::ExtrudeEdgesTool),
        "Bevel" => Some(Action::BevelTool),
        "Fill" => Some(Action::Fill),
        "Fuse" => Some(Action::FuseTool),
        "Weld" => Some(Action::WeldTool),
        "Point at Percentage" => Some(Action::PercentTool),
        "Space" => Some(Action::SpaceTool),
        "Circle Loop" => Some(Action::CircleLoopTool),
        "Flatten" => Some(Action::FlattenTool),
        "Smooth" => Some(Action::SmoothTool),
        "Relax" => Some(Action::RelaxTool),
        "Intersect Faces" => Some(Action::IntersectTool),
        "Clean Up" => Some(Action::CleanUpTool),
        "Close Gaps" => Some(Action::CloseGapsTool),
        "Inspect Faces" => Some(Action::InspectFacesTool),
        "Inspect Manifold" => Some(Action::InspectManifoldTool),
        "Make Manifold" => Some(Action::MakeManifoldTool),
        "Trim / Extend" => Some(Action::TrimTool),
        "Bisector" => Some(Action::BisectorTool),
        "Extend to Face" => Some(Action::ExtendFaceTool),
        "Bridge" => Some(Action::BridgeTool),
        "Loft along Path" => Some(Action::LoftTool),
        "Skinning" => Some(Action::SkinTool),
        "Loft" => Some(Action::LoftSplineTool),
        "Copy Along Path" => Some(Action::CopyAlongPathTool),
        "Shapes" => Some(Action::ShapesTool),
        "Select Connected" => Some(Action::SelectConnected),
        "Select by Type" => Some(Action::SelectByType),
        "Select Vertices" => Some(Action::SelectVertices),
        "Select Edges" => Some(Action::SelectEdges),
        "Select Faces" => Some(Action::SelectFaces),
        "Select All" => Some(Action::SelectAll),
        "Select None" => Some(Action::SelectNone),
        "Cut" => Some(Action::Cut),
        "Copy" => Some(Action::Copy),
        "Paste" => Some(Action::Paste),
        "Undo" => Some(Action::Undo),
        "Redo" => Some(Action::Redo),
        _ => None,
    }
}

/// Index of a mode's panel layout in `Shell::layouts`.
fn mode_index(mode: Mode) -> usize {
    match mode {
        Mode::Edit => 0,
        Mode::Present => 1,
    }
}

/// Runs `f` with a mode's default groups in [`PanelLayout`] form.
fn with_defaults<R>(mode: Mode, f: impl FnOnce(&[DefaultGroup<'_>]) -> R) -> R {
    let groups = match mode {
        Mode::Edit => EDIT_GROUPS,
        Mode::Present => PRESENT_GROUPS,
    };
    let names: Vec<(&str, Vec<&str>)> = groups
        .iter()
        .map(|g| (g.title, g.tools.iter().map(|t| t.name).collect()))
        .collect();
    let defaults: Vec<DefaultGroup<'_>> = names.iter().map(|(t, n)| (*t, n.as_slice())).collect();
    f(&defaults)
}

/// The catalog entry for a tool name, from either mode.
fn find_slot(name: &str) -> Option<&'static ToolSlot> {
    EDIT_GROUPS
        .iter()
        .chain(PRESENT_GROUPS)
        .flat_map(|g| g.tools.iter())
        .find(|t| t.name == name)
}

/// The draw tool an action starts, if any.
fn draw_kind(action: Action) -> Option<DrawKind> {
    Some(match action {
        Action::LineTool => DrawKind::Line,
        Action::RectangleTool => DrawKind::Rectangle,
        Action::RotatedRectangleTool => DrawKind::RotatedRectangle,
        Action::CircleTool => DrawKind::Circle,
        Action::ArcTool => DrawKind::Arc,
        Action::PolygonTool => DrawKind::Polygon,
        Action::BezierTool => DrawKind::Bezier,
        Action::CurveMakerTool => DrawKind::CurveMaker,
        Action::ExtrudeTool => DrawKind::Extrude,
        Action::EraserTool => DrawKind::Eraser,
        Action::MoveTool => DrawKind::Move,
        Action::RotateTool => DrawKind::Rotate,
        Action::ScaleTool => DrawKind::Scale,
        Action::MirrorTool => DrawKind::Mirror,
        Action::OffsetTool => DrawKind::Offset,
        Action::DivideTool => DrawKind::Divide,
        Action::LoopCutTool => DrawKind::LoopCut,
        Action::KnifeTool => DrawKind::Knife,
        Action::SlideTool => DrawKind::Slide,
        Action::DissolveTool => DrawKind::Dissolve,
        Action::TapeMeasureTool => DrawKind::TapeMeasure,
        Action::ProtractorTool => DrawKind::Protractor,
        Action::FindCenterTool => DrawKind::FindCenter,
        Action::DimensionTool => DrawKind::Dimension,
        Action::TextTool => DrawKind::Text,
        Action::SectionTool => DrawKind::Section,
        Action::SweepTool => DrawKind::Sweep,
        Action::RevolveTool => DrawKind::Revolve,
        Action::ExtrudeEdgesTool => DrawKind::ExtrudeEdges,
        Action::BevelTool => DrawKind::Bevel,
        Action::WeldTool => DrawKind::Weld,
        Action::PercentTool => DrawKind::Percent,
        Action::SpaceTool => DrawKind::Space,
        Action::CircleLoopTool => DrawKind::CircleLoop,
        Action::FlattenTool => DrawKind::Flatten,
        Action::SmoothTool => DrawKind::Smooth,
        Action::RelaxTool => DrawKind::Relax,
        Action::IntersectTool => DrawKind::Intersect,
        Action::CleanUpTool => DrawKind::CleanUp,
        Action::CloseGapsTool => DrawKind::CloseGaps,
        Action::InspectFacesTool => DrawKind::InspectFaces,
        Action::InspectManifoldTool => DrawKind::InspectManifold,
        Action::MakeManifoldTool => DrawKind::MakeManifold,
        Action::FuseTool => DrawKind::Fuse,
        Action::TrimTool => DrawKind::TrimExtend,
        Action::BisectorTool => DrawKind::Bisector,
        Action::ExtendFaceTool => DrawKind::ExtendToFace,
        Action::BridgeTool => DrawKind::Bridge,
        Action::LoftTool => DrawKind::Loft,
        Action::SkinTool => DrawKind::Skin,
        Action::LoftSplineTool => DrawKind::LoftSpline,
        Action::CopyAlongPathTool => DrawKind::CopyAlongPath,
        Action::ShapesTool => DrawKind::Shapes,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_items_select_row_matches_the_select_group() {
        let select: Vec<&str> = EDIT_GROUPS[0].tools.iter().map(|t| t.name).collect();
        assert_eq!(SELECT_ROW, select.as_slice());
        assert!(SELECT_ROW.iter().all(|n| find_slot(n).is_some()));
    }

    #[test]
    fn several_of_a_kind_are_named_in_the_plural() {
        assert_eq!(plural("Box"), "Boxes");
        assert_eq!(plural("Torus"), "Tori");
        assert_eq!(plural("Rectangle"), "Rectangles");
        assert_eq!(plural("Loft"), "Lofts");
    }

    #[test]
    fn panel_groups_match_spec() {
        let titles: Vec<_> = EDIT_GROUPS.iter().map(|g| g.title).collect();
        assert_eq!(
            titles,
            [
                "Select", "Draw", "Modify", "Tweak", "Repair", "Generate", "Measure", "Camera"
            ]
        );
        let titles: Vec<_> = PRESENT_GROUPS.iter().map(|g| g.title).collect();
        assert_eq!(titles, ["Paint", "Marks", "Section", "Camera", "Render"]);
        assert_eq!(
            EDIT_GROUPS[1].tools.len(),
            9,
            "seven draw tools plus Curve Maker"
        );
    }

    #[test]
    fn default_panel_layouts_match_the_catalog() {
        for (mode, groups) in [(Mode::Edit, EDIT_GROUPS), (Mode::Present, PRESENT_GROUPS)] {
            let layout = with_defaults(mode, PanelLayout::from_defaults);
            assert_eq!(layout.groups.len(), groups.len());
            for (l, g) in layout.groups.iter().zip(groups) {
                assert_eq!(l.title, g.title);
                assert!(l.tools.iter().all(|t| find_slot(t).is_some()));
            }
        }
    }

    #[test]
    fn live_draw_buttons_start_their_tools() {
        for slot in EDIT_GROUPS[1].tools {
            if let Some(action) = panel_action(slot.name) {
                let kind = draw_kind(action).expect("draw buttons start draw tools");
                assert_eq!(kind.name(), slot.name);
            }
        }
    }

    #[test]
    fn repair_buttons_start_their_tools() {
        let group = &EDIT_GROUPS[4];
        assert_eq!(group.title, "Repair");
        for slot in group.tools {
            let action = panel_action(slot.name).expect("every repair icon works");
            let kind = draw_kind(action).expect("repair buttons start their tools");
            assert_eq!(kind.name(), slot.name);
        }
    }

    #[test]
    fn select_by_type_has_an_action() {
        assert_eq!(panel_action("Select by Type"), Some(Action::SelectByType));
    }

    #[test]
    fn the_select_modes_have_actions_and_numbers() {
        assert_eq!(
            panel_action("Select Vertices"),
            Some(Action::SelectVertices)
        );
        assert_eq!(panel_action("Select Edges"), Some(Action::SelectEdges));
        assert_eq!(panel_action("Select Faces"), Some(Action::SelectFaces));
        // Canvas work never needs the menus: these have buttons.
        for (name, action) in [
            ("Select All", Action::SelectAll),
            ("Select None", Action::SelectNone),
            ("Cut", Action::Cut),
            ("Copy", Action::Copy),
            ("Paste", Action::Paste),
        ] {
            assert_eq!(panel_action(name), Some(action));
            assert!(icons::NAMES.contains(&name));
            assert!(find_slot(name).is_some());
        }
        for (i, k) in SelectKind::ALL.iter().enumerate() {
            assert_eq!(usize::from(select_mode_number(*k)), i);
        }
    }

    #[test]
    fn generate_buttons_start_their_tools() {
        let names: Vec<&str> = EDIT_GROUPS[5].tools.iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            [
                "Extrude",
                "Extrude along Path",
                "Extrude Edges along Path",
                "Revolve",
                "Loft",
                "Loft along Path",
                "Skinning",
                "Bridge"
            ]
        );
        for slot in EDIT_GROUPS[4].tools {
            if let Some(action) = panel_action(slot.name) {
                let kind = draw_kind(action).expect("generate buttons start tools");
                assert_eq!(kind.name(), slot.name);
            }
        }
    }

    #[test]
    fn every_panel_tool_has_an_icon() {
        for g in EDIT_GROUPS.iter().chain(PRESENT_GROUPS) {
            for slot in g.tools {
                assert!(
                    icons::NAMES.contains(&slot.name),
                    "{} has no icon",
                    slot.name
                );
            }
        }
    }

    #[test]
    fn panel_labels_fit_buttons() {
        for g in EDIT_GROUPS.iter().chain(PRESENT_GROUPS) {
            for slot in g.tools {
                assert!(
                    slot.short.chars().count() <= 3,
                    "{} label too long",
                    slot.name
                );
            }
        }
    }
}
