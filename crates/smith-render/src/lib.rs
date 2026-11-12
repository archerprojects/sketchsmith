//! # smith-render
//!
//! SketchSmith — canvas renderer | v1.0.0 | Thursday 12 November 2026
//!
//! The wgpu renderer for the 3D canvas. It reads the model
//! and never mutates it.
//!
//! * [`camera`] — orbit camera, standard views, zoom to cursor, projections.
//! * [`mesh`] — CPU-side display buffers built from the model.
//! * [`renderer`] — GPU pipelines; renders into an offscreen texture that the
//!   application shows inside the UI.
//!
//! Faces, edges and axes, with MSAA and reversed-Z depth; display styles,
//! selection highlight and the Edit/Present modes.

pub mod camera;
pub mod mesh;
pub mod renderer;
