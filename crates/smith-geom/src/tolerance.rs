//! SketchSmith — named tolerances | v1.0.0 | Thursday 12 November 2026
//!
//! The only tolerances in the code base. Two
//! different questions get two different answers:
//!
//! * **Identity** — "are these the same point?" — uses the absolute
//!   [`TOL_MERGE`] of 0.0001 mm. Vertices closer than this merge.
//! * **Planarity** — "is this loop flat enough to fill?" — uses
//!   [`TOL_PLANAR`] scaled by the loop's size, so a 1 mm bezel and a 2 m
//!   table are judged by the same relative flatness.
//!
//! Neither replaces exact predicates. Exact predicates
//! ([`crate::predicates`]) decide topology on given coordinates; tolerances
//! decide when two *different* floating-point coordinates should be treated as one.
//!
//! There is deliberately no minimum face size: anything larger than
//! `TOL_MERGE` is valid geometry. SketchUp's "scale up 1000×" workaround must
//! never be needed.

use glam::DVec3;

/// Merge tolerance in millimetres. Points closer than this are the same point.
pub const TOL_MERGE: f64 = 1e-4;

/// `TOL_MERGE` squared, for comparisons against squared distances.
pub const TOL_MERGE_SQ: f64 = TOL_MERGE * TOL_MERGE;

/// Relative planarity factor. A loop is planar when every vertex lies within
/// `TOL_PLANAR × extent` of its fitted plane, where `extent` is the loop's
/// bounding-box diagonal (see [`planar_tolerance`]).
pub const TOL_PLANAR: f64 = 1e-6;

/// Returns `true` when `a` and `b` are within [`TOL_MERGE`] of each other.
#[inline]
pub fn coincident(a: DVec3, b: DVec3) -> bool {
    a.distance_squared(b) <= TOL_MERGE_SQ
}

/// Bounding-box diagonal length of `points`, in millimetres. Zero for fewer than
/// two distinct points.
pub fn extent(points: &[DVec3]) -> f64 {
    let Some(first) = points.first() else {
        return 0.0;
    };
    let (min, max) = points
        .iter()
        .fold((*first, *first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    min.distance(max)
}

/// The absolute planarity tolerance for a loop of the given extent:
/// `TOL_PLANAR × extent`. Always positive, so a vanishingly small loop still has
/// a usable (tiny) tolerance rather than zero.
#[inline]
pub fn planar_tolerance(extent: f64) -> f64 {
    (TOL_PLANAR * extent).max(f64::MIN_POSITIVE)
}

/// Returns `true` if every component of `v` is finite.
#[inline]
pub fn is_finite(v: DVec3) -> bool {
    v.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_tolerance_boundary() {
        let a = DVec3::ZERO;
        // Just inside and just outside 0.0001 mm.
        assert!(coincident(a, DVec3::new(0.99e-4, 0.0, 0.0)));
        assert!(!coincident(a, DVec3::new(1.01e-4, 0.0, 0.0)));
        // Diagonal distance is what counts, not per-axis.
        let d = 0.8e-4;
        assert!(!coincident(a, DVec3::new(d, d, 0.0)));
    }

    #[test]
    fn extent_is_bbox_diagonal() {
        let pts = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(3.0, 0.0, 0.0),
            DVec3::new(3.0, 4.0, 0.0),
        ];
        assert_eq!(extent(&pts), 5.0);
        assert_eq!(extent(&[]), 0.0);
        assert_eq!(extent(&[DVec3::ONE]), 0.0);
    }

    #[test]
    fn planar_tolerance_scales_and_stays_positive() {
        assert_eq!(planar_tolerance(1000.0), 1e-3);
        assert!(planar_tolerance(0.0) > 0.0);
    }
}
