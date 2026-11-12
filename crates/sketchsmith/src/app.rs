//! SketchSmith — application lifecycle | v1.0.0 | Thursday 12 November 2026
//!
//! Implements the winit [`ApplicationHandler`]: creates the main window on
//! `resumed`, routes window events to egui, and renders frames on demand.
//!
//! Rendering model: reactive. A frame is drawn only when input arrives or egui asks
//! for a repaint; timed repaints (animations, tooltips) are scheduled with
//! `ControlFlow::WaitUntil`. The 3D canvas renders inside the egui frame
//! (see `canvas.rs`), using the painter's GPU context.
//!
//! Closing the window asks the shell first (unsaved-changes prompt). A file
//! dialog's worker thread wakes the loop through an [`EventLoopProxy`] user
//! event, which requests a redraw so the shell collects the answer. The window
//! title follows the shell (file name, `•` for unsaved changes).

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context as _;
use egui::ViewportId;
use std::path::PathBuf;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, StartCause, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::keymap::{TapDetector, TapKey};
use crate::paths::AppPaths;
use crate::shell::Shell;
use crate::{APP_ID, APP_NAME, gpu, theme};

/// Window clear colour (linear RGBA), matching the canvas background. The 3D
/// canvas covers the central area with its own rendered image.
const CANVAS_CLEAR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// Initial window size in logical pixels.
const INITIAL_SIZE: LogicalSize<f64> = LogicalSize::new(1280.0, 800.0);

/// Smallest usable window: room for the bottom bar, tool column and a canvas.
const MIN_SIZE: LogicalSize<f64> = LogicalSize::new(800.0, 500.0);

/// Top-level application state handed to the winit event loop.
pub struct App {
    paths: AppPaths,
    proxy: EventLoopProxy<()>,
    /// A file named on the command line, opened once the window exists.
    open: Option<PathBuf>,
    running: Option<Running>,
    fatal: Option<anyhow::Error>,
}

/// Everything that exists only while the main window is open.
struct Running {
    window: Arc<Window>,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    painter: egui_wgpu::winit::Painter,
    shell: Shell,
    /// When egui has asked for a delayed repaint, the instant it is due.
    next_repaint: Option<Instant>,
    /// The title last given to the window.
    title: String,
    /// Lone taps of right Alt and right Ctrl.
    taps: TapDetector,
}

impl App {
    /// Creates the application. The window and GPU are created later, on `resumed`.
    pub fn new(paths: AppPaths, proxy: EventLoopProxy<()>, open: Option<PathBuf>) -> Self {
        Self {
            paths,
            proxy,
            open,
            running: None,
            fatal: None,
        }
    }

    /// Consumes the app after the event loop ends, returning any fatal error that
    /// stopped it so `main` exits with a non-zero status.
    pub fn exit_result(self) -> anyhow::Result<()> {
        self.fatal.map_or(Ok(()), Err)
    }

    /// Records a fatal error and stops the event loop.
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        log::error!("{error:#}");
        self.fatal = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        match Running::start(
            event_loop,
            &self.paths,
            self.proxy.clone(),
            self.open.take(),
        ) {
            Ok(running) => {
                running.window.request_redraw();
                self.running = Some(running);
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause
            && let Some(running) = self.running.as_mut()
        {
            running.next_repaint = None;
            running.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(running) = self.running.as_mut() else {
            return;
        };

        let response = running.egui_state.on_window_event(&running.window, &event);
        let mut quit = false;

        match event {
            WindowEvent::CloseRequested => {
                running.shell.request_close();
                running.window.request_redraw();
            }
            WindowEvent::Resized(size) => {
                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    running
                        .painter
                        .on_window_resized(ViewportId::ROOT, width, height);
                }
                running.window.request_redraw();
            }
            WindowEvent::RedrawRequested => quit = running.redraw(),
            // The toolkit does not tell left from right modifiers: taps are
            // read from the window's own key events.
            WindowEvent::KeyboardInput { event, .. } => {
                let key = match event.physical_key {
                    PhysicalKey::Code(KeyCode::AltRight) => Some(TapKey::RightAlt),
                    PhysicalKey::Code(KeyCode::ControlRight) => Some(TapKey::RightCtrl),
                    PhysicalKey::Code(KeyCode::ControlLeft) => Some(TapKey::LeftCtrl),
                    _ => None,
                };
                let pressed = event.state == ElementState::Pressed;
                if let Some(tap) = running.taps.key(key, pressed, event.repeat)
                    && !running.egui_ctx.text_edit_focused()
                {
                    running.shell.key_tap(tap);
                    running.window.request_redraw();
                }
            }
            WindowEvent::Focused(_)
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. } => {
                running.taps.interrupt();
            }
            _ => {}
        }

        if response.repaint {
            running.window.request_redraw();
        }
        if quit {
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, (): ()) {
        // A file dialog closed: redraw so the shell picks up the answer.
        if let Some(running) = self.running.as_ref() {
            running.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let flow = match self.running.as_ref().and_then(|r| r.next_repaint) {
            Some(due) => ControlFlow::WaitUntil(due),
            None => ControlFlow::Wait,
        };
        event_loop.set_control_flow(flow);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(mut running) = self.running.take() {
            running.painter.destroy();
        }
        log::info!("{APP_NAME} exiting");
    }
}

impl Running {
    /// Creates the main window, the GPU painter and the egui integration.
    fn start(
        event_loop: &ActiveEventLoop,
        paths: &AppPaths,
        proxy: EventLoopProxy<()>,
        open: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let window = Arc::new(
            event_loop
                .create_window(window_attributes())
                .context("creating the main window")?,
        );

        let egui_ctx = egui::Context::default();
        theme::apply(&egui_ctx);

        let mut painter = pollster::block_on(egui_wgpu::winit::Painter::new(
            egui_ctx.clone(),
            gpu::painter_configuration(event_loop.owned_display_handle()),
            false,
            egui_wgpu::RendererOptions::default(),
        ));
        pollster::block_on(painter.set_window(ViewportId::ROOT, Some(Arc::clone(&window))))
            .context("initialising the GPU (a Vulkan or OpenGL ES driver is required)")?;

        let render_state = painter.render_state();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            window.theme(),
            painter.max_texture_side(),
        );

        Ok(Self {
            egui_ctx,
            egui_state,
            painter,
            shell: Shell::new(
                paths.clone(),
                render_state,
                Some(Arc::clone(&window)),
                Arc::new(move || {
                    let _ = proxy.send_event(());
                }),
                open,
            ),
            window,
            next_repaint: None,
            title: String::new(),
            taps: TapDetector::default(),
        })
    }

    /// Runs one egui frame and paints it. Returns `true` if the user asked to quit.
    fn redraw(&mut self) -> bool {
        let mut raw_input = self.egui_state.take_egui_input(&self.window);
        // App shortcuts the UI toolkit would otherwise use for keyboard focus
        // (Tab) are taken out before egui sees them (shell.rs).
        let typing = self.egui_ctx.egui_wants_keyboard_input();
        self.shell.intercept_keys(&mut raw_input, typing);
        let shell = &mut self.shell;
        let full_output = self.egui_ctx.run_ui(raw_input, |ui| shell.ui(ui));

        self.egui_state
            .handle_platform_output(&self.window, full_output.platform_output);

        let clipped = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let mut textures_delta = full_output.textures_delta;
        self.painter.paint_and_update_textures(
            ViewportId::ROOT,
            full_output.pixels_per_point,
            CANVAS_CLEAR,
            &clipped,
            &mut textures_delta,
            Vec::new(),
            &self.window,
        );

        if let Some(output) = full_output.viewport_output.get(&ViewportId::ROOT) {
            if output.repaint_delay.is_zero() {
                self.next_repaint = None;
                self.window.request_redraw();
            } else {
                // `Duration::MAX` ("no repaint needed") overflows and yields `None`.
                self.next_repaint = Instant::now().checked_add(output.repaint_delay);
            }
        }

        let title = self.shell.window_title();
        if title != self.title {
            self.window.set_title(&title);
            self.title = title;
        }
        self.shell.take_quit_request()
    }
}

/// Main-window attributes. The Wayland `app_id` and X11 `WM_CLASS` are set to the
/// application ID so desktops match the window to its `.desktop` file and icon.
fn window_attributes() -> WindowAttributes {
    use winit::platform::wayland::WindowAttributesExtWayland;
    use winit::platform::x11::WindowAttributesExtX11;

    let attributes = Window::default_attributes()
        .with_title(APP_NAME)
        .with_inner_size(INITIAL_SIZE)
        .with_min_inner_size(MIN_SIZE);
    let attributes = WindowAttributesExtWayland::with_name(attributes, APP_ID, "sketchsmith");
    WindowAttributesExtX11::with_name(attributes, APP_ID, "sketchsmith")
}
