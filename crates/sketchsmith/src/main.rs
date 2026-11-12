//! # sketchsmith
//!
//! SketchSmith — application entry point | v1.0.0 | Thursday 12 November 2026
//!
//! Initialises logging and the XDG directories, then hands control to the winit
//! event loop driven by [`app::App`]. A `.glb` file named on the command line
//! (`sketchsmith ring.glb`) opens at start.
//!
//! Logging: `RUST_LOG` overrides the default filter, e.g.
//! `RUST_LOG=debug sketchsmith` or `RUST_LOG=debug flatpak run io.github.archerprojects.SketchSmith`.

mod app;
mod autosave;
mod canvas;
mod commands;
mod files;
mod gpu;
mod icons;
mod keymap;
mod paint_tool;
mod panel_layout;
mod paths;
mod recent;
mod scenes;
mod settings;
mod shell;
mod theme;

use anyhow::Context as _;
use winit::event_loop::{ControlFlow, EventLoop};

/// Reverse-DNS application ID. Used as the Flatpak ID, the Wayland `app_id`, the X11
/// `WM_CLASS`, and the desktop-file / icon name, so the compositor can match the
/// running window to its desktop entry.
pub const APP_ID: &str = "io.github.archerprojects.SketchSmith";

/// Human-readable application name shown in the window title and About dialog.
pub const APP_NAME: &str = "SketchSmith";

/// Default log filter: our crates at `info`, the noisy GPU stack at `warn`.
const DEFAULT_LOG_FILTER: &str = "info,wgpu_core=warn,wgpu_hal=warn,naga=warn";

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(DEFAULT_LOG_FILTER))
        .init();

    log::info!("{APP_NAME} {} starting", env!("CARGO_PKG_VERSION"));

    let paths = paths::AppPaths::resolve().context("resolving XDG directories")?;
    paths.log();

    let event_loop = EventLoop::new().context("creating the winit event loop")?;
    // Reactive rendering: sleep until input arrives or egui requests a repaint.
    event_loop.set_control_flow(ControlFlow::Wait);

    // The first argument that is not an option is a file to open.
    let open = std::env::args_os()
        .skip(1)
        .find(|a| !a.to_string_lossy().starts_with('-'))
        .map(std::path::PathBuf::from);

    let mut app = app::App::new(paths, event_loop.create_proxy(), open);
    event_loop
        .run_app(&mut app)
        .context("running the winit event loop")?;

    app.exit_result()
}
