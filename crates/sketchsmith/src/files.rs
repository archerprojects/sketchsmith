//! SketchSmith — file state and dialogs | v1.0.0 | Thursday 12 November 2026
//!
//! What the shell needs to open and save native files:
//!
//! * [`Files`] remembers the open file's path, whether a newer SketchSmith
//!   wrote it, and the model state at the last save (document revision and drawing-axes
//!   count), from which the window title and the unsaved-changes prompt follow.
//! * File dialogs are the desktop's own, through the XDG desktop portal
//!   (`rfd`), so they work the same inside the Flatpak. Each runs on a worker
//!   thread: waiting on the main thread would stop the window answering the
//!   compositor, and GNOME would offer to force-quit it. The worker wakes the
//!   event loop when the user has chosen; [`Files::poll`] collects the choice.
//! * [`next_version`] finds Save Version's next file name, as Blender's Save
//!   Incremental does.
//! * Debug builds only: when `SKETCHSMITH_DIALOG_PATH` is set, dialogs return
//!   that path at once instead of opening (for automated checks on a virtual
//!   display, which has no portal).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use smith_io::export::{ExportOptions, Format};
use smith_io::import::{ImportFormat, ImportOptions};
use winit::window::Window;

use crate::APP_NAME;

/// Something that would discard the current model, so it asks first when
/// there are unsaved changes.
#[derive(Debug, Clone)]
pub enum Discard {
    /// File → New.
    New,
    /// File → Open (shows the open dialog next).
    Open,
    /// File → Open Recent: open this file.
    OpenPath(PathBuf),
    /// File → Quit, or closing the window.
    Quit,
    /// Help → Load Test Scene.
    TestScene,
    /// Help → Load Stress Scene.
    StressScene,
}

impl Discard {
    /// How the prompt names the action ("before …").
    pub fn verb(&self) -> &'static str {
        match self {
            Discard::New => "starting a new model",
            Discard::Open | Discard::OpenPath(_) => "opening another file",
            Discard::Quit => "quitting",
            Discard::TestScene | Discard::StressScene => "loading the scene",
        }
    }
}

/// What a dialog's answer is for.
#[derive(Debug)]
pub enum Purpose {
    /// Open the chosen file.
    Open,
    /// Save to the chosen file, then carry on with the action that asked.
    SaveAs(Option<Discard>),
    /// Load the chosen PNG or JPEG as a new material.
    LoadImage,
    /// Export to the chosen file, in this format with these choices.
    Export(Format, ExportOptions),
    /// Export saved Render `.0`, named `.3`, in format `.1` with choices `.2`.
    ExportRender(smith_model::SceneId, Format, ExportOptions, String),
    /// Import the chosen file, in this format with these choices.
    Import(ImportFormat, ImportOptions),
}

/// A file dialog running on its worker thread.
struct Pending {
    purpose: Purpose,
    answer: Receiver<Option<PathBuf>>,
}

/// Lets a worker thread wake the event loop.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// A message window shown over the canvas until dismissed.
pub struct Notice {
    /// Window title.
    pub title: String,
    /// Body text.
    pub text: String,
}

/// File state for the open model.
pub struct Files {
    /// The open file, or `None` for an unsaved new model.
    pub path: Option<PathBuf>,
    /// An unsaved model's name, when it was opened from another program's
    /// file (an SKP): the title shows it and Save As offers it.
    pub name: Option<String>,
    /// Written by a newer SketchSmith: saving over it asks first.
    pub newer: bool,
    /// A save over a newer file waiting on its prompt: the file, and the
    /// action to carry on with once it is saved.
    pub newer_prompt: Option<(PathBuf, Option<Discard>)>,
    /// (document revision, axes count) when last saved or opened.
    saved: (u64, u64),
    /// The action waiting on the unsaved-changes prompt.
    pub confirm: Option<Discard>,
    pending: Option<Pending>,
    /// Message to show (errors, warnings).
    pub notice: Option<Notice>,
    window: Option<Arc<Window>>,
    wake: Waker,
}

impl Files {
    /// File state for an empty new model. `window` parents the dialogs and
    /// `wake` is called from the dialog thread when it has an answer.
    pub fn new(window: Option<Arc<Window>>, wake: Waker) -> Self {
        Self {
            path: None,
            name: None,
            newer: false,
            newer_prompt: None,
            saved: (0, 0),
            confirm: None,
            pending: None,
            notice: None,
            window,
            wake,
        }
    }

    /// Records the model state now on disk (after a save or an open).
    pub fn mark_saved(&mut self, revision: u64, axes: u64) {
        self.saved = (revision, axes);
    }

    /// Whether the model differs from the file.
    pub fn dirty(&self, revision: u64, axes: u64) -> bool {
        self.saved != (revision, axes)
    }

    /// The file name shown in the title and top bar.
    pub fn display_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .or_else(|| self.name.clone())
            .unwrap_or_else(|| "Untitled".into())
    }

    /// Window title: `• name — SketchSmith`, the dot marking unsaved changes.
    pub fn title(&self, dirty: bool) -> String {
        let dot = if dirty { "• " } else { "" };
        format!("{dot}{} — {APP_NAME}", self.display_name())
    }

    /// The save status shown by the dot at the right end of the top bar.
    pub fn status(&self, dirty: bool) -> SaveStatus {
        match (&self.path, dirty) {
            (None, _) => SaveStatus::NeverSaved,
            (Some(_), true) => SaveStatus::Unsaved,
            (Some(_), false) => SaveStatus::Saved,
        }
    }

    /// Whether a dialog is open.
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Shows a message window.
    pub fn notify(&mut self, title: impl Into<String>, text: impl Into<String>) {
        self.notice = Some(Notice {
            title: title.into(),
            text: text.into(),
        });
    }

    /// Opens the open or save dialog on a worker thread.
    pub fn start_dialog(&mut self, purpose: Purpose) {
        if self.pending.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let wake = Arc::clone(&self.wake);
        let saving = matches!(
            purpose,
            Purpose::SaveAs(_) | Purpose::Export(..) | Purpose::ExportRender(..)
        );
        let export = match &purpose {
            Purpose::Export(f, _) | Purpose::ExportRender(_, f, ..) => Some(*f),
            _ => None,
        };
        // A Render's file is named after it.
        let render_name = match &purpose {
            Purpose::ExportRender(.., name) => Some(name.clone()),
            _ => None,
        };

        #[cfg(debug_assertions)]
        if let Some(fixed) = std::env::var_os("SKETCHSMITH_DIALOG_PATH") {
            let _ = tx.send(Some(PathBuf::from(fixed)));
            self.pending = Some(Pending {
                purpose,
                answer: rx,
            });
            wake();
            return;
        }

        let image = matches!(purpose, Purpose::LoadImage);
        let import = match &purpose {
            Purpose::Import(f, _) => Some(*f),
            _ => None,
        };
        let mut dialog = if image {
            rfd::FileDialog::new()
                .add_filter("PNG or JPEG image", &["png", "jpg", "jpeg"])
                .set_title("Load Image")
        } else if let Some(f) = import {
            rfd::FileDialog::new()
                .add_filter(f.filter(), f.extensions())
                .set_title(format!("Import {}", f.name()))
        } else if let Some(f) = export {
            rfd::FileDialog::new()
                .add_filter(f.filter(), &[f.extension()])
                .set_title(format!("Export {}", f.name()))
        } else if saving {
            rfd::FileDialog::new()
                .add_filter("SketchSmith model (.glb)", &["glb"])
                .set_title("Save As")
        } else {
            // Open takes an SKP too, as a new model.
            let skp = ImportFormat::Skp;
            rfd::FileDialog::new()
                .add_filter("SketchSmith model (.glb)", &["glb"])
                .add_filter(skp.filter(), skp.extensions())
                .set_title("Open")
        };
        if let Some(dir) = self.path.as_deref().and_then(Path::parent)
            && !image
        {
            dialog = dialog.set_directory(dir);
        }
        if saving {
            // An unsaved model opened from an SKP offers its name.
            let named = self
                .name
                .as_ref()
                .map(|n| PathBuf::from(format!("{n}.{}", smith_io::NATIVE_EXTENSION)));
            let open = self.path.as_deref().or(named.as_deref());
            let name = suggested_name(open, export, render_name.as_deref());
            dialog = dialog.set_file_name(name);
        }
        if let Some(w) = &self.window {
            dialog = dialog.set_parent(&**w);
        }
        std::thread::spawn(move || {
            let answer = if saving {
                dialog.save_file()
            } else {
                dialog.pick_file()
            };
            let _ = tx.send(answer);
            wake();
        });
        self.pending = Some(Pending {
            purpose,
            answer: rx,
        });
    }

    /// The finished dialog's purpose and choice (`None` = cancelled), if one
    /// has finished. A saved file always gets the `.glb` extension; an export
    /// gets its own in [`smith_io::export::target`].
    pub fn poll(&mut self) -> Option<(Purpose, Option<PathBuf>)> {
        let pending = self.pending.as_ref()?;
        let answer = match pending.answer.try_recv() {
            Ok(a) => a,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => None,
        };
        let pending = self.pending.take()?;
        let answer = match pending.purpose {
            Purpose::SaveAs(_) => answer.map(with_glb_extension),
            Purpose::Open
            | Purpose::LoadImage
            | Purpose::Export(..)
            | Purpose::ExportRender(..)
            | Purpose::Import(..) => answer,
        };
        Some((pending.purpose, answer))
    }
}

/// The model's save status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveStatus {
    /// No file yet (red).
    NeverSaved,
    /// A file, with changes since it was saved (yellow).
    Unsaved,
    /// A file, matching the model (green).
    Saved,
}

impl SaveStatus {
    /// The dot's hover text.
    pub fn label(self) -> &'static str {
        match self {
            SaveStatus::NeverSaved => "Never saved",
            SaveStatus::Unsaved => "Unsaved changes",
            SaveStatus::Saved => "Saved",
        }
    }
}

/// Save Version's next file name for `path`, following Blender's Save
/// Incremental: the number at the end of the name goes up by one, keeping its
/// zero padding (`bracket_002.glb` → `bracket_003.glb`); a name with no number
/// gets `1` (`bracket.glb` → `bracket1.glb`). Names already taken are
/// skipped, so no file is ever overwritten. `None` if 1000 tries all exist.
pub fn next_version(path: &Path) -> Option<PathBuf> {
    next_version_where(path, |p| p.exists())
}

/// [`next_version`] with the existence check supplied (for tests).
fn next_version_where(path: &Path, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let tail = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let digits = stem.chars().rev().take_while(char::is_ascii_digit).count();
    let (head, number) = stem.split_at(stem.len() - digits);
    // A number too large to count up is dropped and counting starts at 1, as
    // Blender does.
    let (mut num, width) = match number.parse::<u64>() {
        Ok(n) => (n, digits),
        Err(_) => (0, 0),
    };
    let dir = path.parent().unwrap_or(Path::new(""));
    for _ in 0..1000 {
        num = num.checked_add(1)?;
        let candidate = dir.join(format!("{head}{num:0width$}{tail}"));
        if !exists(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// The name a save dialog suggests: a Render's own name for Export Render,
/// else the open file's name (`Untitled` for a new model), with the export
/// format's extension or `.glb`.
fn suggested_name(open: Option<&Path>, export: Option<Format>, render: Option<&str>) -> String {
    // Save As keeps the open file's name as it is.
    if export.is_none()
        && let Some(name) = open.and_then(Path::file_name)
    {
        return name.to_string_lossy().into_owned();
    }
    let stem = render.map(str::to_owned).unwrap_or_else(|| {
        open.and_then(Path::file_stem)
            .map_or_else(|| "Untitled".into(), |s| s.to_string_lossy().into_owned())
    });
    let ext = export.map_or(smith_io::NATIVE_EXTENSION, Format::extension);
    format!("{stem}.{ext}")
}

/// Adds `.glb` unless the name already ends with it (any case).
fn with_glb_extension(path: PathBuf) -> PathBuf {
    let has = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(smith_io::NATIVE_EXTENSION));
    if has {
        path
    } else {
        let mut name = path.into_os_string();
        name.push(".");
        name.push(smith_io::NATIVE_EXTENSION);
        PathBuf::from(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Files {
        Files::new(None, Arc::new(|| {}))
    }

    #[test]
    fn title_shows_name_and_unsaved_dot() {
        let mut f = files();
        assert_eq!(f.title(false), "Untitled — SketchSmith");
        f.name = Some("house".into());
        assert_eq!(f.title(false), "house — SketchSmith");
        f.path = Some(PathBuf::from("/home/rjv/ring.glb"));
        assert_eq!(f.title(true), "• ring.glb — SketchSmith");
    }

    #[test]
    fn status_follows_file_and_changes() {
        let mut f = files();
        assert_eq!(f.status(false), SaveStatus::NeverSaved);
        assert_eq!(f.status(true), SaveStatus::NeverSaved);
        f.path = Some(PathBuf::from("/home/rjv/ring.glb"));
        assert_eq!(f.status(true), SaveStatus::Unsaved);
        assert_eq!(f.status(false), SaveStatus::Saved);
    }

    #[test]
    fn versions_count_up_like_blender() {
        let none = |_: &Path| false;
        let next = |p: &str| next_version_where(Path::new(p), none).unwrap();
        assert_eq!(
            next("/w/bracket_002.glb"),
            PathBuf::from("/w/bracket_003.glb")
        );
        assert_eq!(next("/w/bracket.glb"), PathBuf::from("/w/bracket1.glb"));
        assert_eq!(next("/w/bracket9.glb"), PathBuf::from("/w/bracket10.glb"));
        assert_eq!(
            next("/w/bracket_099.glb"),
            PathBuf::from("/w/bracket_100.glb")
        );
        assert_eq!(
            next("/w/bracket_999.glb"),
            PathBuf::from("/w/bracket_1000.glb")
        );
        assert_eq!(next("/w/plate_000.glb"), PathBuf::from("/w/plate_001.glb"));
    }

    #[test]
    fn versions_skip_names_in_use() {
        let taken = |p: &Path| {
            let n = p.file_name().unwrap().to_string_lossy();
            n == "bracket_003.glb" || n == "bracket_004.glb"
        };
        assert_eq!(
            next_version_where(Path::new("/w/bracket_002.glb"), taken),
            Some(PathBuf::from("/w/bracket_005.glb"))
        );
        assert_eq!(next_version_where(Path::new("/w/b.glb"), |_| true), None);
    }

    #[test]
    fn dirty_follows_revision_and_axes() {
        let mut f = files();
        assert!(!f.dirty(0, 0));
        assert!(f.dirty(3, 0));
        f.mark_saved(3, 1);
        assert!(!f.dirty(3, 1));
        assert!(f.dirty(3, 2));
    }

    #[test]
    fn dialogs_suggest_a_name() {
        let open = Path::new("/w/bracket.glb");
        assert_eq!(suggested_name(Some(open), None, None), "bracket.glb");
        assert_eq!(suggested_name(None, None, None), "Untitled.glb");
        assert_eq!(
            suggested_name(Some(Path::new("/w/ring.GLB")), None, None),
            "ring.GLB"
        );
        assert_eq!(
            suggested_name(Some(open), Some(Format::Obj), None),
            "bracket.obj"
        );
        assert_eq!(
            suggested_name(Some(open), Some(Format::Pdf), Some("Front Elevation")),
            "Front Elevation.pdf"
        );
    }

    #[test]
    fn saved_names_get_the_extension_once() {
        assert_eq!(
            with_glb_extension("a/ring".into()),
            PathBuf::from("a/ring.glb")
        );
        assert_eq!(
            with_glb_extension("a/ring.GLB".into()),
            PathBuf::from("a/ring.GLB")
        );
        assert_eq!(
            with_glb_extension("a/ring.v2".into()),
            PathBuf::from("a/ring.v2.glb")
        );
    }
}
