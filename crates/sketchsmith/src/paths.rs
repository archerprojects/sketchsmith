//! SketchSmith — XDG directory resolution | v1.0.0 | Thursday 12 November 2026
//!
//! Resolves and creates the per-user configuration, data and state directories.
//! Inside the Flatpak sandbox the XDG variables point into
//! `~/.var/app/io.github.archerprojects.SketchSmith/`, so the same code serves both
//! native and sandboxed runs.
//!
//! Planned use: `config` holds settings and user keymaps, `data` holds user
//! materials, `state` holds autosave and crash-recovery files.

use std::path::PathBuf;

use anyhow::Context as _;
use directories::ProjectDirs;

/// The application's resolved XDG directories. All three exist on disk once
/// [`AppPaths::resolve`] returns successfully.
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// Settings and keymap overrides (`$XDG_CONFIG_HOME/...`).
    pub config: PathBuf,
    /// User data such as custom materials (`$XDG_DATA_HOME/...`).
    pub data: PathBuf,
    /// Autosave and crash-recovery files (`$XDG_STATE_HOME/...`).
    pub state: PathBuf,
}

impl AppPaths {
    /// Resolves the XDG directories for SketchSmith and creates any that are missing.
    ///
    /// # Errors
    /// Fails if no home directory can be determined or a directory cannot be created.
    pub fn resolve() -> anyhow::Result<Self> {
        let dirs = ProjectDirs::from("io.github", "archerprojects", "SketchSmith")
            .context("no valid home directory found")?;

        // `state_dir` is defined on Linux; fall back to a subfolder of the local data
        // directory so the code never panics on an unusual environment.
        let state = dirs
            .state_dir()
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs.data_local_dir().join("state"));

        let paths = Self {
            config: dirs.config_dir().to_path_buf(),
            data: dirs.data_dir().to_path_buf(),
            state,
        };

        for dir in [&paths.config, &paths.data, &paths.state] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating directory {}", dir.display()))?;
        }
        Ok(paths)
    }

    /// Logs the resolved directories at `info` level.
    pub fn log(&self) {
        log::info!("config dir: {}", self.config.display());
        log::info!("data dir:   {}", self.data.display());
        log::info!("state dir:  {}", self.state.display());
    }
}
