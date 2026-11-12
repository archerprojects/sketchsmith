//! SketchSmith — recent files | v1.0.0 | Thursday 12 November 2026
//!
//! The File → Open Recent list: the last [`MAX`] files opened or saved,
//! newest first, kept in `recent.toml` in the state folder (it is history, not
//! a setting). Inside the Flatpak the paths are the document portal's, which
//! stay valid between runs. A file that has gone missing is dropped from the
//! list when it fails to open.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File name inside the state folder.
const FILE: &str = "recent.toml";

/// How many files the list keeps.
pub const MAX: usize = 10;

/// The recent-files list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    /// Newest first.
    #[serde(default)]
    pub files: Vec<PathBuf>,
}

impl Recent {
    /// Loads the list from `state_dir`, or an empty list.
    pub fn load(state_dir: &Path) -> Self {
        std::fs::read_to_string(state_dir.join(FILE))
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Saves the list to `state_dir`. Failures are logged, not fatal.
    pub fn save(&self, state_dir: &Path) {
        let result = toml::to_string(self)
            .map_err(|e| e.to_string())
            .and_then(|t| std::fs::write(state_dir.join(FILE), t).map_err(|e| e.to_string()));
        if let Err(e) = result {
            log::warn!("could not save the recent-files list: {e}");
        }
    }

    /// Puts `path` at the top, removing any older entry for it.
    pub fn touch(&mut self, path: &Path) {
        self.files.retain(|p| p != path);
        self.files.insert(0, path.to_path_buf());
        self.files.truncate(MAX);
    }

    /// Removes `path` (it could not be opened).
    pub fn remove(&mut self, path: &Path) {
        self.files.retain(|p| p != path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_no_duplicates_capped_and_saved() {
        let mut r = Recent::default();
        for i in 0..12 {
            r.touch(Path::new(&format!("/m/{i}.glb")));
        }
        r.touch(Path::new("/m/5.glb"));
        assert_eq!(r.files.len(), MAX);
        assert_eq!(r.files[0], PathBuf::from("/m/5.glb"));
        assert_eq!(r.files[1], PathBuf::from("/m/11.glb"));
        assert_eq!(r.files.iter().filter(|p| p.ends_with("5.glb")).count(), 1);
        r.remove(Path::new("/m/11.glb"));
        assert_eq!(r.files[1], PathBuf::from("/m/10.glb"));

        let dir = std::env::temp_dir().join(format!("sketchsmith-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        r.save(&dir);
        assert_eq!(Recent::load(&dir), r);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
