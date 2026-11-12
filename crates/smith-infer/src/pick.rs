//! SketchSmith — ray picking | v1.0.0 | Thursday 12 November 2026
//!
//! Casts a ray from the camera into the model and returns the nearest face hit.
//! Used by zoom-to-cursor, selection and inference.
//!
//! Intersection uses the Möller–Trumbore ray/triangle test (T. Möller and
//! B. Trumbore, "Fast, Minimum Storage Ray-Triangle Intersection", 1997) on each
//! face's display triangles. It scans every face; the BVH in [`crate::bvh`]
//! gives the same results faster.

use smith_geom::DVec3;
use smith_model::{Entities, FaceId};

/// The nearest face hit along a ray.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    /// Distance along the (normalised) ray direction.
    pub t: f64,
    /// The hit point in world space.
    pub point: DVec3,
    /// The face that was hit.
    pub face: FaceId,
}

/// Casts a ray from `origin` along `dir` and returns the nearest face hit in
/// front of the origin. Both sides of a face count as hits.
pub fn ray_cast_faces(entities: &Entities, origin: DVec3, dir: DVec3) -> Option<RayHit> {
    let dir = dir.try_normalize()?;
    let mut best: Option<RayHit> = None;
    for face in entities.face_ids() {
        for tri in entities.face_triangles(face) {
            if let Some(t) = ray_triangle(origin, dir, tri)
                && best.is_none_or(|b| t < b.t)
            {
                best = Some(RayHit {
                    t,
                    point: origin + dir * t,
                    face,
                });
            }
        }
    }
    best
}

/// Möller–Trumbore. Returns the ray parameter of the hit, if it is in front of
/// the origin. Rays parallel to the triangle (to within `f64` resolution) miss.
pub fn ray_triangle(origin: DVec3, dir: DVec3, [a, b, c]: [DVec3; 3]) -> Option<f64> {
    let e1 = b - a;
    let e2 = c - a;
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() <= f64::EPSILON * e1.length() * e2.length() {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    #[test]
    fn hits_nearest_face_from_either_side() {
        let mut m = Entities::new();
        m.add_polyline(
            &[
                v(0.0, 0.0, 0.0),
                v(10.0, 0.0, 0.0),
                v(10.0, 10.0, 0.0),
                v(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        m.add_polyline(
            &[
                v(0.0, 0.0, 5.0),
                v(10.0, 0.0, 5.0),
                v(10.0, 10.0, 5.0),
                v(0.0, 10.0, 5.0),
            ],
            true,
        )
        .unwrap();
        let hit = ray_cast_faces(&m, v(5.0, 5.0, 20.0), -DVec3::Z).unwrap();
        assert!((hit.point - v(5.0, 5.0, 5.0)).length() < 1e-12);
        let below = ray_cast_faces(&m, v(5.0, 5.0, -3.0), DVec3::Z).unwrap();
        assert!((below.t - 3.0).abs() < 1e-12);
        assert!(ray_cast_faces(&m, v(50.0, 5.0, 20.0), -DVec3::Z).is_none());
        assert!(
            ray_cast_faces(&m, v(5.0, 5.0, 20.0), DVec3::Z).is_none(),
            "behind the origin"
        );
    }

    #[test]
    fn hole_is_not_hit() {
        let mut m = Entities::new();
        m.add_polyline(
            &[
                v(0.0, 0.0, 0.0),
                v(10.0, 0.0, 0.0),
                v(10.0, 10.0, 0.0),
                v(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        m.add_polyline(
            &[
                v(3.0, 3.0, 0.0),
                v(7.0, 3.0, 0.0),
                v(7.0, 7.0, 0.0),
                v(3.0, 7.0, 0.0),
            ],
            true,
        )
        .unwrap();
        // The disc inside the hole is a separate face, so the ray still hits — but
        // the plate itself does not.
        let hit = ray_cast_faces(&m, v(5.0, 5.0, 1.0), -DVec3::Z).unwrap();
        assert!(m.face(hit.face).unwrap().inner().is_empty());
    }
}
