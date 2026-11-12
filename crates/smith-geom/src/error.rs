//! SketchSmith — geometry errors | v1.0.0 | Thursday 12 November 2026
//!
//! Every fallible constructor in `smith-geom` returns [`GeomError`]. Geometry code
//! never panics on user data.

use thiserror::Error;

/// Errors raised when geometric input cannot define the requested object.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum GeomError {
    /// An input coordinate or scalar was NaN or infinite.
    #[error("non-finite value in geometric input")]
    NonFinite,
    /// A normal vector had zero (or numerically negligible) length.
    #[error("normal vector is zero or degenerate")]
    DegenerateNormal,
    /// An axis was zero-length or parallel to the normal, so no frame exists.
    #[error("axis is zero-length or parallel to the normal")]
    DegenerateAxis,
    /// The points are collinear or coincident, so they define no plane.
    #[error("points are collinear or coincident and define no plane")]
    DegeneratePlane,
    /// Too few points were supplied for the requested operation.
    #[error("at least {required} points required, {given} given")]
    TooFewPoints {
        /// Minimum number of points the operation needs.
        required: usize,
        /// Number of points actually supplied.
        given: usize,
    },
    /// A radius was zero or negative.
    #[error("radius must be positive, got {0} mm")]
    NonPositiveRadius(f64),
    /// An arc's sweep was zero, negative, or greater than a full turn.
    #[error("arc sweep must be in (0, 2π], got {0} rad")]
    InvalidSweep(f64),
    /// A polygon had fewer than three sides.
    #[error("a polygon needs at least 3 sides, got {0}")]
    TooFewSides(u32),
    /// A segment count was below the curve kind's minimum.
    #[error("segment count must be at least {minimum}, got {given}")]
    TooFewSegments {
        /// Minimum segment count for this curve kind.
        minimum: u32,
        /// Segment count actually requested.
        given: u32,
    },
    /// A Bézier control point list was not of the form `3n + 1` with `n >= 1`.
    #[error("a cubic Bézier chain needs 3n + 1 control points (n >= 1), got {0}")]
    BezierControlPoints(usize),
}
