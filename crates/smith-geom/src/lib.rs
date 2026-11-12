//! # smith-geom
//!
//! SketchSmith — geometric foundations | v1.0.0 | Thursday 12 November 2026
//!
//! The maths layer every other SketchSmith crate builds on. It owns:
//!
//! * [`tolerance`] — the named tolerances (`TOL_MERGE`, `TOL_PLANAR`) and the
//!   helpers that apply them. No other crate may invent its own epsilon.
//! * [`predicates`] — exact orientation tests (Shewchuk's adaptive predicates via
//!   the `robust` crate). These decide topology: which side, collinear, coplanar.
//! * [`plane`] — planes, Newell plane fitting, and 2D/3D mapping onto a plane.
//! * [`intersect`] — closest points and segment/segment, segment/plane
//!   intersection, plus an exact 2D crossing classifier for in-plane work.
//! * [`triangulate`] — constrained-Delaunay triangulation of faces with holes.
//! * [`curve`] — the five curve kinds (arc, circle, polygon, Bézier, polyline)
//!   and their tessellation into points.
//!
//! All coordinates are `f64` millimetres in a right-handed, Z-up world
//! (red = X, green = Y, blue = Z). `f32` never appears in this crate.
//!
//! Rotation-minimizing frames for Sweep will be added here.
//!
//! Dependency rule: this crate depends on no other SketchSmith crate.

pub mod curve;
pub mod error;
pub mod intersect;
pub mod plane;
pub mod predicates;
pub mod tolerance;
pub mod triangulate;

pub use error::GeomError;
/// Re-exported so downstream crates use the same `glam` types and version.
pub use glam::{DMat3, DMat4, DVec2, DVec3};
