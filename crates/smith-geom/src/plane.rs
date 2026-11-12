//! SketchSmith — planes | v1.0.0 | Thursday 12 November 2026
//!
//! An oriented plane `n · p + d = 0` with a unit normal `n`, plus the operations
//! the face code needs: Newell fitting of a vertex loop, signed distance,
//! projection, a planarity check, and mapping between 3D points and 2D
//! coordinates in the plane (for exact in-plane predicates and triangulation).
//!
//! [`Plane::from_stored`] restores a saved plane bit for bit (native file
//! format): it checks the values instead of renormalising them.
//!
//! Newell's method (M. E. Newell, as described in Sutherland, Sproull and
//! Schumacker, 1974) computes a polygon's normal from all of its edges. It is
//! robust for non-convex and nearly degenerate loops where a three-point cross
//! product can pick a bad triple. For a loop wound counter-clockwise when viewed
//! from the normal side, the normal points toward the viewer.

use glam::{DVec2, DVec3};

use crate::error::GeomError;
use crate::tolerance::{TOL_MERGE, extent, planar_tolerance};

/// How far a stored unit vector's length (or a stored pair's dot product) may
/// stray from exact before a file is treated as damaged. Values written by this
/// program are unit to about 1e-16; the margin only absorbs other writers.
pub const STORED_UNIT_TOLERANCE: f64 = 1e-9;

/// An oriented plane `normal · p + d = 0`, with `normal` of unit length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    normal: DVec3,
    d: f64,
}

impl Plane {
    /// Creates a plane through `point` with the given `normal` (normalised here).
    ///
    /// # Errors
    /// [`GeomError::NonFinite`] for NaN or infinite input;
    /// [`GeomError::DegenerateNormal`] if `normal` has zero length.
    pub fn from_point_normal(point: DVec3, normal: DVec3) -> Result<Self, GeomError> {
        if !point.is_finite() || !normal.is_finite() {
            return Err(GeomError::NonFinite);
        }
        let normal = normal.try_normalize().ok_or(GeomError::DegenerateNormal)?;
        Ok(Self {
            normal,
            d: -normal.dot(point),
        })
    }

    /// Restores a plane exactly as stored (native file reading). Unlike
    /// [`Plane::from_point_normal`] the normal is *not* renormalised, so a saved
    /// plane reads back bit-identical; it must already be of unit length.
    ///
    /// # Errors
    /// [`GeomError::NonFinite`]; [`GeomError::DegenerateNormal`] if the normal's
    /// length differs from 1 by more than [`STORED_UNIT_TOLERANCE`].
    pub fn from_stored(normal: DVec3, d: f64) -> Result<Self, GeomError> {
        if !normal.is_finite() || !d.is_finite() {
            return Err(GeomError::NonFinite);
        }
        if (normal.length() - 1.0).abs() > STORED_UNIT_TOLERANCE {
            return Err(GeomError::DegenerateNormal);
        }
        Ok(Self { normal, d })
    }

    /// Creates the plane through three points, with normal `(b − a) × (c − a)`.
    ///
    /// # Errors
    /// [`GeomError::DegeneratePlane`] if the points are collinear or coincident.
    pub fn from_three_points(a: DVec3, b: DVec3, c: DVec3) -> Result<Self, GeomError> {
        if !a.is_finite() || !b.is_finite() || !c.is_finite() {
            return Err(GeomError::NonFinite);
        }
        let n = (b - a).cross(c - a);
        let normal = n.try_normalize().ok_or(GeomError::DegeneratePlane)?;
        Ok(Self {
            normal,
            d: -normal.dot(a),
        })
    }

    /// Fits a plane to a closed vertex loop using Newell's method. The plane
    /// passes through the loop's centroid; the normal follows the loop winding
    /// (right-hand rule).
    ///
    /// # Errors
    /// [`GeomError::TooFewPoints`] for fewer than three points;
    /// [`GeomError::NonFinite`] for NaN or infinite input;
    /// [`GeomError::DegeneratePlane`] if the loop encloses no area (all points
    /// collinear or coincident within `TOL_MERGE`).
    pub fn fit_loop(points: &[DVec3]) -> Result<Self, GeomError> {
        if points.len() < 3 {
            return Err(GeomError::TooFewPoints {
                required: 3,
                given: points.len(),
            });
        }
        if points.iter().any(|p| !p.is_finite()) {
            return Err(GeomError::NonFinite);
        }

        // Work relative to the first point: identical normal, much better
        // conditioning for small loops far from the origin.
        let origin = points[0];
        let mut n = DVec3::ZERO;
        let mut centroid = DVec3::ZERO;
        for (i, p) in points.iter().enumerate() {
            let cur = *p - origin;
            let next = points[(i + 1) % points.len()] - origin;
            n.x += (cur.y - next.y) * (cur.z + next.z);
            n.y += (cur.z - next.z) * (cur.x + next.x);
            n.z += (cur.x - next.x) * (cur.y + next.y);
            centroid += cur;
        }
        centroid = origin + centroid / points.len() as f64;

        // |n| is twice the projected area. Reject loops whose area is below what a
        // TOL_MERGE-wide sliver of the loop's extent would have.
        let area2 = n.length();
        if area2 <= TOL_MERGE * extent(points) {
            return Err(GeomError::DegeneratePlane);
        }
        let normal = n / area2;
        Ok(Self {
            normal,
            d: -normal.dot(centroid),
        })
    }

    /// The unit normal.
    #[inline]
    pub fn normal(&self) -> DVec3 {
        self.normal
    }

    /// The plane constant `d` in `normal · p + d = 0`.
    #[inline]
    pub fn d(&self) -> f64 {
        self.d
    }

    /// The point on the plane closest to the world origin.
    #[inline]
    pub fn origin(&self) -> DVec3 {
        -self.d * self.normal
    }

    /// Signed distance from `p` to the plane: positive on the normal side.
    #[inline]
    pub fn signed_distance(&self, p: DVec3) -> f64 {
        self.normal.dot(p) + self.d
    }

    /// Orthogonal projection of `p` onto the plane.
    #[inline]
    pub fn project(&self, p: DVec3) -> DVec3 {
        p - self.signed_distance(p) * self.normal
    }

    /// The same plane with the normal reversed.
    #[inline]
    pub fn flipped(&self) -> Self {
        Self {
            normal: -self.normal,
            d: -self.d,
        }
    }

    /// Largest absolute distance of any point in `points` from the plane.
    pub fn max_deviation(&self, points: &[DVec3]) -> f64 {
        points
            .iter()
            .map(|p| self.signed_distance(*p).abs())
            .fold(0.0, f64::max)
    }

    /// Returns `true` if every point lies within the planar tolerance for the
    /// points' extent (`TOL_PLANAR × extent`).
    pub fn contains_all(&self, points: &[DVec3]) -> bool {
        self.max_deviation(points) <= planar_tolerance(extent(points))
    }

    /// An orthonormal in-plane basis `(u, v)` with `u × v = normal`.
    ///
    /// `u` is built from the world axis least aligned with the normal, so the
    /// basis is stable (never derived from a near-parallel axis).
    pub fn basis(&self) -> (DVec3, DVec3) {
        let n = self.normal;
        let a = n.abs();
        let helper = if a.x <= a.y && a.x <= a.z {
            DVec3::X
        } else if a.y <= a.z {
            DVec3::Y
        } else {
            DVec3::Z
        };
        // helper is not parallel to n, so the cross product is non-zero.
        let u = helper.cross(n).normalize();
        let v = n.cross(u);
        (u, v)
    }

    /// Maps a 3D point to 2D coordinates in the plane, using [`Plane::basis`]
    /// and [`Plane::origin`]. The out-of-plane component is discarded.
    pub fn to_local(&self, p: DVec3) -> DVec2 {
        let (u, v) = self.basis();
        let rel = p - self.origin();
        DVec2::new(rel.dot(u), rel.dot(v))
    }

    /// Maps 2D in-plane coordinates back to the 3D point on the plane.
    pub fn from_local(&self, q: DVec2) -> DVec3 {
        let (u, v) = self.basis();
        self.origin() + q.x * u + q.y * v
    }
}

/// Fits a plane to a loop and checks it is flat enough to become a face.
///
/// # Errors
/// As [`Plane::fit_loop`]; additionally returns `Ok(None)` (not an error) when the
/// loop fits a plane but some vertex lies outside the planar tolerance.
pub fn planar_loop(points: &[DVec3]) -> Result<Option<Plane>, GeomError> {
    let plane = Plane::fit_loop(points)?;
    Ok(plane.contains_all(points).then_some(plane))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(size: f64, z: f64) -> Vec<DVec3> {
        vec![
            DVec3::new(0.0, 0.0, z),
            DVec3::new(size, 0.0, z),
            DVec3::new(size, size, z),
            DVec3::new(0.0, size, z),
        ]
    }

    #[test]
    fn newell_normal_follows_winding() {
        let ccw = square(10.0, 2.0);
        let plane = Plane::fit_loop(&ccw).unwrap();
        assert!((plane.normal() - DVec3::Z).length() < 1e-15);
        assert!((plane.signed_distance(DVec3::new(3.0, 3.0, 2.0))).abs() < 1e-15);

        let mut cw = ccw.clone();
        cw.reverse();
        assert!((Plane::fit_loop(&cw).unwrap().normal() + DVec3::Z).length() < 1e-15);
    }

    #[test]
    fn fits_non_convex_loop() {
        // L-shape: a three-point cross product from a reflex corner would flip.
        let l = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(2.0, 1.0, 0.0),
            DVec3::new(1.0, 1.0, 0.0),
            DVec3::new(1.0, 2.0, 0.0),
            DVec3::new(0.0, 2.0, 0.0),
        ];
        let plane = Plane::fit_loop(&l).unwrap();
        assert!((plane.normal() - DVec3::Z).length() < 1e-15);
    }

    #[test]
    fn sub_millimetre_loop_far_from_origin() {
        // A 0.001 mm square placed 1 m from the origin .
        let offset = DVec3::new(1000.0, -1000.0, 500.0);
        let pts: Vec<DVec3> = square(0.001, 0.0).into_iter().map(|p| p + offset).collect();
        let plane = Plane::fit_loop(&pts).unwrap();
        assert!((plane.normal() - DVec3::Z).length() < 1e-9);
        assert!(plane.contains_all(&pts));
    }

    #[test]
    fn degenerate_loops_are_rejected() {
        let collinear = [DVec3::ZERO, DVec3::X, DVec3::X * 2.0];
        assert_eq!(Plane::fit_loop(&collinear), Err(GeomError::DegeneratePlane));
        assert!(matches!(
            Plane::fit_loop(&[DVec3::ZERO, DVec3::X]),
            Err(GeomError::TooFewPoints { .. })
        ));
        let nan = [DVec3::ZERO, DVec3::X, DVec3::new(f64::NAN, 0.0, 0.0)];
        assert_eq!(Plane::fit_loop(&nan), Err(GeomError::NonFinite));
    }

    #[test]
    fn planarity_check_uses_relative_tolerance() {
        let mut pts = square(100.0, 0.0);
        // Extent ≈ 141.4 mm → tolerance ≈ 1.414e-4 mm.
        pts[2].z = 1e-4;
        assert!(planar_loop(&pts).unwrap().is_some());
        pts[2].z = 1e-3;
        assert!(planar_loop(&pts).unwrap().is_none());
    }

    #[test]
    fn basis_is_orthonormal_and_right_handed() {
        for n in [DVec3::X, DVec3::Y, DVec3::Z, DVec3::new(1.0, 2.0, -3.0)] {
            let plane = Plane::from_point_normal(DVec3::new(1.0, 2.0, 3.0), n).unwrap();
            let (u, v) = plane.basis();
            assert!((u.length() - 1.0).abs() < 1e-15);
            assert!((v.length() - 1.0).abs() < 1e-15);
            assert!(u.dot(v).abs() < 1e-15);
            assert!((u.cross(v) - plane.normal()).length() < 1e-15);
        }
    }

    #[test]
    fn local_round_trip() {
        let plane =
            Plane::from_point_normal(DVec3::new(5.0, -2.0, 7.0), DVec3::new(0.3, -0.4, 0.8))
                .unwrap();
        let p = plane.project(DVec3::new(12.5, 3.25, -8.0));
        let back = plane.from_local(plane.to_local(p));
        assert!((back - p).length() < 1e-12);
    }

    #[test]
    fn three_point_plane_and_flip() {
        let plane = Plane::from_three_points(DVec3::ZERO, DVec3::X, DVec3::Y).unwrap();
        assert_eq!(plane.normal(), DVec3::Z);
        assert_eq!(plane.flipped().normal(), -DVec3::Z);
        assert_eq!(
            Plane::from_three_points(DVec3::ZERO, DVec3::X, DVec3::X * 3.0),
            Err(GeomError::DegeneratePlane)
        );
    }
}

#[cfg(test)]
mod stored_tests {
    use super::*;

    #[test]
    fn stored_plane_round_trips_bit_for_bit_and_rejects_damage() {
        let p = Plane::from_point_normal(DVec3::new(3.0, -2.0, 7.5), DVec3::new(1.0, 2.0, 3.0))
            .expect("plane");
        let q = Plane::from_stored(p.normal(), p.d()).expect("stored");
        assert_eq!(p, q);
        assert!(Plane::from_stored(DVec3::new(0.0, 0.0, 2.0), 0.0).is_err());
        assert!(Plane::from_stored(DVec3::Z, f64::NAN).is_err());
    }
}
