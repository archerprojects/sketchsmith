//! SketchSmith — exact geometric predicates | v1.0.0 | Thursday 12 November 2026
//!
//! Thin, intention-revealing wrappers over Shewchuk's adaptive-precision
//! predicates (J. R. Shewchuk, "Adaptive Precision Floating-Point Arithmetic and
//! Fast Robust Geometric Predicates", 1997), as implemented by the `robust` crate.
//!
//! The results are **exact** for the given `f64` inputs: the sign is always
//! correct, with no epsilon. Topology decisions (side of a line, side of a
//! plane, collinear, coplanar) must go through this module, never through
//! `==` on computed values or ad-hoc epsilons.

use glam::{DVec2, DVec3};
use robust::{Coord, Coord3D};

/// Which way three 2D points turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// `c` lies to the left of the directed line `a → b` (counter-clockwise).
    Left,
    /// `c` lies to the right of the directed line `a → b` (clockwise).
    Right,
    /// `a`, `b` and `c` are exactly collinear.
    Collinear,
}

/// Which side of an oriented plane a point lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// On the side the plane normal points to.
    Above,
    /// On the opposite side.
    Below,
    /// Exactly in the plane.
    On,
}

#[inline]
fn c2(p: DVec2) -> Coord<f64> {
    Coord { x: p.x, y: p.y }
}

#[inline]
fn c3(p: DVec3) -> Coord3D<f64> {
    Coord3D {
        x: p.x,
        y: p.y,
        z: p.z,
    }
}

/// Exact orientation of `c` relative to the directed line `a → b`.
pub fn orient2d(a: DVec2, b: DVec2, c: DVec2) -> Turn {
    let det = robust::orient2d(c2(a), c2(b), c2(c));
    if det > 0.0 {
        Turn::Left
    } else if det < 0.0 {
        Turn::Right
    } else {
        Turn::Collinear
    }
}

/// Exact side of `d` relative to the plane through `a`, `b`, `c`, oriented by
/// the right-hand rule: the normal is `(b − a) × (c − a)`.
///
/// `robust::orient3d` returns positive when `d` is *below* that plane, so the
/// sign is inverted here to give the conventional "above = normal side".
pub fn orient3d(a: DVec3, b: DVec3, c: DVec3, d: DVec3) -> Side {
    let det = robust::orient3d(c3(a), c3(b), c3(c), c3(d));
    if det < 0.0 {
        Side::Above
    } else if det > 0.0 {
        Side::Below
    } else {
        Side::On
    }
}

/// Exactly tests whether three 3D points are collinear.
///
/// The cross product `(b − a) × (c − a)` has components equal to the 2D
/// orientation determinants of the yz, zx and xy projections. All three are
/// zero exactly when the points are collinear, and each is evaluated exactly.
pub fn collinear(a: DVec3, b: DVec3, c: DVec3) -> bool {
    let yz = |p: DVec3| DVec2::new(p.y, p.z);
    let zx = |p: DVec3| DVec2::new(p.z, p.x);
    let xy = |p: DVec3| DVec2::new(p.x, p.y);
    orient2d(yz(a), yz(b), yz(c)) == Turn::Collinear
        && orient2d(zx(a), zx(b), zx(c)) == Turn::Collinear
        && orient2d(xy(a), xy(b), xy(c)) == Turn::Collinear
}

/// Exactly tests whether four 3D points are coplanar.
pub fn coplanar(a: DVec3, b: DVec3, c: DVec3, d: DVec3) -> bool {
    orient3d(a, b, c, d) == Side::On
}

/// Exactly tests whether `p` lies on the closed segment `a–b` (endpoints included).
///
/// The point must be exactly collinear with the segment and inside its
/// axis-aligned bounding box; both tests are exact comparisons.
pub fn on_segment(p: DVec3, a: DVec3, b: DVec3) -> bool {
    collinear(a, b, p) && p.cmpge(a.min(b)).all() && p.cmple(a.max(b)).all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orient2d_signs() {
        let a = DVec2::new(0.0, 0.0);
        let b = DVec2::new(1.0, 0.0);
        assert_eq!(orient2d(a, b, DVec2::new(0.5, 1.0)), Turn::Left);
        assert_eq!(orient2d(a, b, DVec2::new(0.5, -1.0)), Turn::Right);
        assert_eq!(orient2d(a, b, DVec2::new(7.0, 0.0)), Turn::Collinear);
    }

    #[test]
    fn orient2d_is_exact_where_naive_math_fails() {
        // Classic near-degenerate case: c is a hair off the line through a and b.
        // A naive determinant in f64 rounds this to zero; the exact predicate does not.
        let a = DVec2::new(0.5, 0.5);
        let b = DVec2::new(12.0, 12.0);
        let c = DVec2::new(24.0, 24.000_000_000_000_004);
        assert_eq!(orient2d(a, b, c), Turn::Left);
    }

    #[test]
    fn orient3d_above_is_normal_side() {
        // Counter-clockwise triangle in the XY plane seen from +Z: normal is +Z.
        let a = DVec3::new(0.0, 0.0, 0.0);
        let b = DVec3::new(1.0, 0.0, 0.0);
        let c = DVec3::new(0.0, 1.0, 0.0);
        assert_eq!(orient3d(a, b, c, DVec3::new(0.2, 0.2, 1.0)), Side::Above);
        assert_eq!(orient3d(a, b, c, DVec3::new(0.2, 0.2, -1.0)), Side::Below);
        assert_eq!(orient3d(a, b, c, DVec3::new(5.0, -3.0, 0.0)), Side::On);
    }

    #[test]
    fn collinear_3d_exact() {
        let a = DVec3::new(0.0, 0.0, 0.0);
        let b = DVec3::new(1.0, 2.0, 3.0);
        assert!(collinear(a, b, DVec3::new(2.0, 4.0, 6.0)));
        assert!(!collinear(a, b, DVec3::new(2.0, 4.0, 6.000_000_000_001)));
        // Coincident points are trivially collinear.
        assert!(collinear(a, a, b));
    }

    #[test]
    fn coplanar_exact() {
        let a = DVec3::new(0.0, 0.0, 0.0);
        let b = DVec3::new(1.0, 0.0, 0.0);
        let c = DVec3::new(0.0, 1.0, 0.0);
        assert!(coplanar(a, b, c, DVec3::new(9.0, 9.0, 0.0)));
        assert!(!coplanar(a, b, c, DVec3::new(9.0, 9.0, 1e-300)));
    }

    #[test]
    fn on_segment_includes_endpoints_only_on_the_line() {
        let a = DVec3::new(0.0, 0.0, 0.0);
        let b = DVec3::new(4.0, 0.0, 0.0);
        assert!(on_segment(a, a, b));
        assert!(on_segment(b, a, b));
        assert!(on_segment(DVec3::new(1.5, 0.0, 0.0), a, b));
        assert!(!on_segment(DVec3::new(5.0, 0.0, 0.0), a, b));
        assert!(!on_segment(DVec3::new(1.5, 1e-12, 0.0), a, b));
    }
}
