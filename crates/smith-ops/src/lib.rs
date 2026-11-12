//! # smith-ops
//!
//! SketchSmith — modelling operations | v1.0.0 | Thursday 12 November 2026
//!
//! Pure operations over the model, run inside a transaction by the caller.
//!
//! * [`draw`] — the geometry behind the draw tools: lines, rectangles,
//!   circles (centre–radius and through three points), arcs from a chord and
//!   bulge, and polygons. Tools work out the parameters interactively; these
//!   functions build the geometry, so the same code serves the tools, typed
//!   redefinition, and Operation Inputs.
//! * [`modify`] — Extrude, Erase, Move, Rotate, Scale, Mirror.
//! * [`shape`] — Offset (Inset), Divide, Reverse and Orient Faces.
//! * [`cut`] — Loop Cut and Align.
//! * [`organize`] — Make Group, Make Component and Explode.
//! * [`sweep`] — Extrude along Path, Extrude Edges along Path, Loft, Loft
//!   along Path and Revolve.
//! * [`bevel`] — Bevel (chamfer and fillet).
//! * [`weld`] — Weld (a chain of edges to one curve).
//! * [`tweak`] — Space, Circle, Flatten, Smooth and Relax.
//! * [`extend`] — Trim / Extend, Bisector, Extend to Face and Bridge.
//! * [`shapes`] — Box, Cylinder, Cone, Sphere and Torus.
//! * [`cleanup`] — Clean Up and the report lines the repair tools show.
//! * [`intersect`] — Intersect Faces.
//! * [`gaps`] — Close Gaps.
//! * [`inspect`] — Inspect Faces.
//! * [`solid`] — Inspect Manifold.
//! * [`manifold`] — Make Manifold.
//! * [`fuse`] — Fuse (Union, Difference, Intersect).
//! * [`skin`] — Skinning.
//!
//! Dependency rule: depends only on `smith-geom` and `smith-model`.

pub mod bevel;
pub mod cleanup;
pub mod cut;
pub mod draw;
pub mod extend;
pub mod fuse;
pub mod gaps;
pub mod inspect;
pub mod intersect;
pub mod manifold;
pub mod modify;
pub mod organize;
pub mod shape;
pub mod shapes;
pub mod skin;
pub mod solid;
pub mod sweep;
pub mod tweak;
pub mod weld;
