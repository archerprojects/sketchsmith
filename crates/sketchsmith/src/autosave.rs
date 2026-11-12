//! SketchSmith — autosave and crash recovery | v1.0.0 | Thursday 12 November 2026
//!
//! Unsaved work is copied to the XDG state folder at a set interval, so a crash loses at most that interval's work.
//!
//! * Each running SketchSmith is a **session** with its own id. Its copy is
//!   `autosave/<id>.glb` (a normal native file, written atomically) beside
//!   `autosave/<id>.toml`, which records the file it belongs to.
//! * The session holds an exclusive lock on `autosave/<id>.lock` while it runs.
//!   The operating system releases the lock when the process ends, however it
//!   ends, so a copy whose lock can be taken belongs to a session that crashed
//!   or was killed; a copy whose lock is held belongs to another SketchSmith
//!   that is still open, and is left alone. This works across Flatpak
//!   sandboxes, which share the state folder but not process ids.
//! * The copy is removed when the work is saved, discarded, replaced or the
//!   model returns to its saved state, and on a normal quit, so only a crash
//!   leaves one behind.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use smith_io::{SavedAxes, SavedView};
use smith_model::Document;

/// Folder inside the state folder.
const DIR: &str = "autosave";

/// What a copy belongs to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    /// The file the work belongs to; `None` for a model never saved.
    #[serde(default)]
    pub source: Option<PathBuf>,
    /// The SketchSmith version that wrote the copy.
    #[serde(default)]
    pub version: String,
}

/// A copy left behind by a session that ended without saving.
#[derive(Debug, Clone)]
pub struct Recovery {
    /// Session id.
    id: String,
    /// The copied model.
    pub model: PathBuf,
    /// What it belongs to.
    pub meta: Meta,
    /// When the copy was written.
    pub when: SystemTime,
}

impl Recovery {
    /// The name to show: the file name it belongs to, or "Untitled".
    pub fn display_name(&self) -> String {
        self.meta
            .source
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(|| "Untitled".into(), |n| n.to_string_lossy().into_owned())
    }

    /// How long ago the copy was written, in words.
    pub fn age(&self) -> String {
        let secs = SystemTime::now()
            .duration_since(self.when)
            .unwrap_or_default()
            .as_secs();
        let (n, unit) = match secs {
            0..60 => return "less than a minute ago".into(),
            60..3600 => (secs / 60, "minute"),
            3600..86_400 => (secs / 3600, "hour"),
            _ => (secs / 86_400, "day"),
        };
        format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
    }
}

/// This session's autosave state.
pub struct Autosave {
    dir: PathBuf,
    id: String,
    /// Held for the life of the session (see the module notes).
    lock: Option<File>,
    /// Whether this session's copy exists on disk.
    written: bool,
}

impl Autosave {
    /// Starts the session: creates the folder and takes the session lock.
    /// Failures are logged; autosave then simply does nothing.
    pub fn start(state_dir: &Path) -> Self {
        let dir = state_dir.join(DIR);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let id = format!("{stamp}-{}", std::process::id());
        let lock = fs::create_dir_all(&dir)
            .and_then(|()| File::create(dir.join(format!("{id}.lock"))))
            .and_then(|f| f.try_lock().map(|()| f).map_err(std::io::Error::other));
        let lock = match lock {
            Ok(f) => Some(f),
            Err(e) => {
                log::warn!("autosave disabled: {e}");
                None
            }
        };
        Self {
            dir,
            id,
            lock,
            written: false,
        }
    }

    fn file(&self, id: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("{id}.{ext}"))
    }

    /// Copies the model, with the mode it is shown in (`present`). Returns
    /// whether it worked (failures are logged).
    pub fn write(
        &mut self,
        document: &Document,
        view: Option<&SavedView>,
        axes: Option<&SavedAxes>,
        present: bool,
        source: Option<&Path>,
    ) -> bool {
        if self.lock.is_none() {
            return false;
        }
        let meta = Meta {
            source: source.map(Path::to_path_buf),
            version: env!("CARGO_PKG_VERSION").into(),
        };
        let model = self.file(&self.id, "glb");
        let result = smith_io::save_parts(&model, document, view, axes, present)
            .map_err(|e| e.to_string())
            .and_then(|()| toml::to_string(&meta).map_err(|e| e.to_string()))
            .and_then(|text| {
                fs::write(self.file(&self.id, "toml"), text).map_err(|e| e.to_string())
            });
        match result {
            Ok(()) => {
                self.written = true;
                log::info!("autosaved to {}", model.display());
                true
            }
            Err(e) => {
                log::warn!("autosave failed: {e}");
                false
            }
        }
    }

    /// Removes this session's copy (the work was saved, discarded or replaced).
    pub fn clear(&mut self) {
        if self.written {
            let _ = fs::remove_file(self.file(&self.id, "glb"));
            let _ = fs::remove_file(self.file(&self.id, "toml"));
            self.written = false;
        }
    }

    /// Copies left by sessions that ended without saving, newest first.
    /// Leftover locks with no copy are tidied away.
    pub fn crashed(&self) -> Vec<Recovery> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("lock") {
                continue;
            }
            let Some(id) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            if id == self.id {
                continue;
            }
            // A lock we can take belongs to a session that is no longer running.
            let free = File::open(&path).is_ok_and(|f| f.try_lock().is_ok());
            if !free {
                continue;
            }
            let model = self.file(&id, "glb");
            match fs::metadata(&model).and_then(|m| m.modified()) {
                Ok(when) => {
                    let meta = fs::read_to_string(self.file(&id, "toml"))
                        .ok()
                        .and_then(|t| toml::from_str(&t).ok())
                        .unwrap_or_default();
                    found.push(Recovery {
                        id,
                        model,
                        meta,
                        when,
                    });
                }
                Err(_) => {
                    let _ = fs::remove_file(&path);
                    let _ = fs::remove_file(self.file(&id, "toml"));
                }
            }
        }
        found.sort_by_key(|r| std::cmp::Reverse(r.when));
        found
    }

    /// Deletes a recovered or refused copy and its lock.
    pub fn forget(&self, r: &Recovery) {
        for ext in ["glb", "toml", "lock"] {
            let _ = fs::remove_file(self.file(&r.id, ext));
        }
    }

    /// Ends the session normally: removes this session's copy and lock.
    pub fn finish(&mut self) {
        self.clear();
        if self.lock.take().is_some() {
            let _ = fs::remove_file(self.file(&self.id, "lock"));
        }
    }
}

/// The autosave interval: `minutes` from the settings (0 = off). Debug builds
/// accept `SKETCHSMITH_AUTOSAVE_SECS` instead, for testing.
pub fn interval(minutes: u32) -> Option<Duration> {
    #[cfg(debug_assertions)]
    if let Some(secs) = std::env::var("SKETCHSMITH_AUTOSAVE_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
    {
        return (secs > 0).then(|| Duration::from_secs(secs));
    }
    (minutes > 0).then(|| Duration::from_secs(u64::from(minutes) * 60))
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_geom::DVec3;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sketchsmith-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn model() -> Document {
        let mut doc = Document::new();
        doc.transaction("line", |d| {
            d.root_mut()
                .add_line(DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0))
                .map(|_| ())
        })
        .expect("line");
        doc
    }

    #[test]
    fn a_running_session_is_not_offered_but_a_dead_one_is() {
        let dir = temp("autosave");
        let mut a = Autosave::start(&dir);
        assert!(a.write(&model(), None, None, false, Some(Path::new("/x/ring.glb"))));
        let b = Autosave::start(&dir);
        // `a` still holds its lock: nothing to recover.
        assert!(b.crashed().is_empty());
        // Simulate a crash: the lock is released without the copy being cleared.
        drop(a.lock.take());
        let found = b.crashed();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].display_name(), "ring.glb");
        let opened = smith_io::open(&found[0].model).expect("copy opens");
        assert_eq!(opened.file.document.to_data(), model().to_data());
        b.forget(&found[0]);
        assert!(b.crashed().is_empty());
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_normal_finish_leaves_nothing_behind() {
        let dir = temp("autosave-finish");
        let mut a = Autosave::start(&dir);
        assert!(a.write(&model(), None, None, false, None));
        a.finish();
        let b = Autosave::start(&dir);
        assert!(b.crashed().is_empty());
        let left: Vec<_> = fs::read_dir(dir.join(DIR))
            .expect("ls")
            .filter_map(Result::ok)
            .filter(|e| !e.file_name().to_string_lossy().starts_with(&b.id))
            .collect();
        assert!(left.is_empty());
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn interval_off_and_on() {
        if std::env::var_os("SKETCHSMITH_AUTOSAVE_SECS").is_none() {
            assert_eq!(interval(0), None);
            assert_eq!(interval(5), Some(Duration::from_secs(300)));
        }
    }
}
