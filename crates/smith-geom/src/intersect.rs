//! SketchSmith — intersections and closest points | v1.0.0 | Thursday 12 November 2026
//!
//! Two families of tests, used for different jobs:
//!
//! * **3D, tolerance-based** ([`segment_segment`], [`segment_plane`]). Two
//!   floating-point segments in space are almost never *exactly* coplanar, so
//!   "do these edges meet?" is answered by the closest-point distance against
//!   [`TOL_MERGE`]. This is what sticky edge splitting uses.
//! * **2D, exact** ([`classify_segments_2d`]). Once geometry is known to lie in
//!   a face plane and is mapped to plane coordinates, crossings are classified
//!   with exact orientation predicates — no epsilon — so face splitting never
//!   disagrees with itself.
//!
//! Face against face ([`plane_plane`], [`line_in_loops`], [`common_spans`])
//! works along the line where two face planes meet: each face covers some
//! stretches of it, and the cut lines are the stretches both cover.
//!
//! Closest points between segments follow C. Ericson, *Real-Time Collision
//! Detection* (2005), §5.1.9, including its handling of degenerate segments.

use glam::{DVec2, DVec3};

use crate::plane::Plane;
use crate::predicates::{Turn, orient2d};
use crate::tolerance::{TOL_MERGE, TOL_MERGE_SQ};

/// Relative threshold below which two directions are treated as parallel when
/// solving for closest points. Parallel and near-parallel segments are handled
/// by the overlap path instead.
const PARALLEL_EPS: f64 = 1e-12;

/// Two planes whose normals differ by less than this (|cos| above 1 − ε)
/// are parallel and have no line in common.
const PLANES_PARALLEL: f64 = 1e-9;

/// Closest point on segment `a–b` to `p`. Returns `(t, point)` with `t` in
/// `[0, 1]` along `a → b`. A zero-length segment returns `(0, a)`.
pub fn closest_point_on_segment(p: DVec3, a: DVec3, b: DVec3) -> (f64, DVec3) {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq == 0.0 {
        return (0.0, a);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    (t, a + t * ab)
}

/// Closest points between segments `p1–q1` and `p2–q2`.
///
/// Returns `(s, t, c1, c2)`: parameters along each segment in `[0, 1]` and the
/// closest points themselves. For parallel segments one valid pair is returned.
pub fn closest_points_segments(
    p1: DVec3,
    q1: DVec3,
    p2: DVec3,
    q2: DVec3,
) -> (f64, f64, DVec3, DVec3) {
    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.length_squared();
    let e = d2.length_squared();
    let f = d2.dot(r);

    let (s, t) = if a == 0.0 && e == 0.0 {
        (0.0, 0.0)
    } else if a == 0.0 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e == 0.0 {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let mut s = if denom > PARALLEL_EPS * a * e {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    (s, t, p1 + d1 * s, p2 + d2 * t)
}

/// Result of intersecting two 3D segments under [`TOL_MERGE`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SegmentIntersection {
    /// The segments do not come within `TOL_MERGE` of each other.
    None,
    /// The segments meet at a single point.
    Point {
        /// The meeting point (midpoint of the two closest points).
        point: DVec3,
        /// Parameter along the first segment, in `[0, 1]`.
        t_a: f64,
        /// Parameter along the second segment, in `[0, 1]`.
        t_b: f64,
    },
    /// The segments are collinear and share a stretch longer than `TOL_MERGE`.
    Overlap {
        /// Start and end parameters of the shared stretch along the first segment.
        t_a: (f64, f64),
        /// The same stretch's parameters along the second segment.
        t_b: (f64, f64),
    },
}

/// Intersects segments `a0–a1` and `b0–b1` using the merge tolerance.
///
/// Collinear overlaps are reported as [`SegmentIntersection::Overlap`] so the
/// model can merge the shared stretch instead of creating duplicate edges.
pub fn segment_segment(a0: DVec3, a1: DVec3, b0: DVec3, b1: DVec3) -> SegmentIntersection {
    let da = a1 - a0;
    let db = b1 - b0;
    let len_a_sq = da.length_squared();
    let len_b_sq = db.length_squared();

    // Collinear overlap: both of b's endpoints lie within TOL_MERGE of a's line,
    // and both segments have real length.
    if len_a_sq > TOL_MERGE_SQ && len_b_sq > TOL_MERGE_SQ {
        let near_line = |p: DVec3| {
            let t = (p - a0).dot(da) / len_a_sq;
            (a0 + t * da).distance_squared(p) <= TOL_MERGE_SQ
        };
        if near_line(b0) && near_line(b1) {
            return collinear_overlap(a0, da, len_a_sq, b0, b1);
        }
    }

    let (s, t, c1, c2) = closest_points_segments(a0, a1, b0, b1);
    if c1.distance_squared(c2) <= TOL_MERGE_SQ {
        SegmentIntersection::Point {
            point: (c1 + c2) * 0.5,
            t_a: s,
            t_b: t,
        }
    } else {
        SegmentIntersection::None
    }
}

/// Resolves two collinear segments into an overlap, a single touching point, or nothing.
fn collinear_overlap(
    a0: DVec3,
    da: DVec3,
    len_a_sq: f64,
    b0: DVec3,
    b1: DVec3,
) -> SegmentIntersection {
    let len_a = len_a_sq.sqrt();
    // Parameters of b's endpoints along a (unclamped).
    let u0 = (b0 - a0).dot(da) / len_a_sq;
    let u1 = (b1 - a0).dot(da) / len_a_sq;
    let (lo, hi) = (u0.min(u1).max(0.0), u0.max(u1).min(1.0));
    let tol = TOL_MERGE / len_a;
    if hi - lo > tol {
        // Map the shared stretch back onto b's own parameterisation.
        let to_b = |u: f64| (u - u0) / (u1 - u0);
        SegmentIntersection::Overlap {
            t_a: (lo, hi),
            t_b: (to_b(lo), to_b(hi)),
        }
    } else if hi - lo >= -tol {
        // Touching end to end.
        let u = ((lo + hi) * 0.5).clamp(0.0, 1.0);
        SegmentIntersection::Point {
            point: a0 + u * da,
            t_a: u,
            t_b: ((u - u0) / (u1 - u0)).clamp(0.0, 1.0),
        }
    } else {
        SegmentIntersection::None
    }
}

/// Result of intersecting a segment with a plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SegmentPlane {
    /// The segment stays on one side of the plane (beyond `TOL_MERGE`).
    None,
    /// The segment crosses or touches the plane at one point.
    Point {
        /// Parameter along the segment, in `[0, 1]`.
        t: f64,
        /// The crossing point, projected exactly onto the plane.
        point: DVec3,
    },
    /// Both endpoints lie within `TOL_MERGE` of the plane.
    InPlane,
}

/// Intersects segment `a–b` with `plane`, using [`TOL_MERGE`] as the on-plane band.
pub fn segment_plane(a: DVec3, b: DVec3, plane: &Plane) -> SegmentPlane {
    let da = plane.signed_distance(a);
    let db = plane.signed_distance(b);
    let a_on = da.abs() <= TOL_MERGE;
    let b_on = db.abs() <= TOL_MERGE;
    match (a_on, b_on) {
        (true, true) => SegmentPlane::InPlane,
        (true, false) => SegmentPlane::Point {
            t: 0.0,
            point: plane.project(a),
        },
        (false, true) => SegmentPlane::Point {
            t: 1.0,
            point: plane.project(b),
        },
        (false, false) if da.signum() == db.signum() => SegmentPlane::None,
        (false, false) => {
            let t = da / (da - db);
            SegmentPlane::Point {
                t,
                point: plane.project(a + t * (b - a)),
            }
        }
    }
}

/// The line where planes `a` and `b` meet, as a point on it and a unit
/// direction, or `None` when they are parallel (within `PLANES_PARALLEL`).
pub fn plane_plane(a: &Plane, b: &Plane) -> Option<(DVec3, DVec3)> {
    let (na, nb) = (a.normal(), b.normal());
    if na.dot(nb).abs() >= 1.0 - PLANES_PARALLEL {
        return None;
    }
    let u = na.cross(nb);
    // n · p = −d for each plane.
    let point = ((-a.d()) * nb.cross(u) + (-b.d()) * u.cross(na)) / u.length_squared();
    Some((point, u.normalize()))
}

/// The stretches of the line through `origin` along unit `dir`, which lies
/// in the loops' plane, that fall inside the region `loops` bound (outer
/// loop and holes, even-odd), as parameter pairs along `dir`. `cut` is a
/// second plane through the line: each loop edge is tested against it, with
/// points on it counted on its positive side, so an edge lying along the
/// line or a corner touching it is counted consistently.
pub fn line_in_loops(
    loops: &[Vec<DVec3>],
    cut: &Plane,
    origin: DVec3,
    dir: DVec3,
) -> Vec<(f64, f64)> {
    let mut ts = Vec::new();
    for l in loops {
        let n = l.len();
        for i in 0..n {
            let (p, q) = (l[i], l[(i + 1) % n]);
            let (dp, dq) = (cut.signed_distance(p), cut.signed_distance(q));
            if (dp < 0.0) != (dq < 0.0) {
                let x = p + (q - p) * (dp / (dp - dq));
                ts.push((x - origin).dot(dir));
            }
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.as_chunks::<2>()
        .0
        .iter()
        .map(|[a, b]| (*a, *b))
        .filter(|(a, b)| b - a > TOL_MERGE)
        .collect()
}

/// The stretches common to two sorted, separate lists of stretches, longer
/// than `TOL_MERGE`.
pub fn common_spans(a: &[(f64, f64)], b: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let lo = a[i].0.max(b[j].0);
        let hi = a[i].1.min(b[j].1);
        if hi - lo > TOL_MERGE {
            out.push((lo, hi));
        }
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// Exact classification of two 2D segments (for geometry already mapped into a
/// face plane with [`Plane::to_local`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crossing2 {
    /// The segments share no point.
    Disjoint,
    /// The interiors cross at a single point.
    Proper,
    /// They share exactly one point and at least one segment touches with an
    /// endpoint (T-junction or end-to-end).
    Touch,
    /// They are collinear and share more than one point.
    Overlap,
}

/// Exactly classifies segments `a0–a1` and `b0–b1` in the plane. No tolerance is
/// involved: the answer is exact for the given coordinates.
pub fn classify_segments_2d(a0: DVec2, a1: DVec2, b0: DVec2, b1: DVec2) -> Crossing2 {
    let o1 = orient2d(a0, a1, b0);
    let o2 = orient2d(a0, a1, b1);
    let o3 = orient2d(b0, b1, a0);
    let o4 = orient2d(b0, b1, a1);

    let opposite = |x: Turn, y: Turn| {
        matches!(
            (x, y),
            (Turn::Left, Turn::Right) | (Turn::Right, Turn::Left)
        )
    };

    if opposite(o1, o2) && opposite(o3, o4) {
        return Crossing2::Proper;
    }

    let all_collinear = [o1, o2, o3, o4].iter().all(|o| *o == Turn::Collinear);
    if all_collinear {
        // Project onto the dominant axis and compare intervals exactly.
        let d = a1 - a0;
        let key = |p: DVec2| if d.x.abs() >= d.y.abs() { p.x } else { p.y };
        let (alo, ahi) = (key(a0).min(key(a1)), key(a0).max(key(a1)));
        let (blo, bhi) = (key(b0).min(key(b1)), key(b0).max(key(b1)));
        let lo = alo.max(blo);
        let hi = ahi.min(bhi);
        return if lo < hi {
            Crossing2::Overlap
        } else if lo == hi {
            Crossing2::Touch
        } else {
            Crossing2::Disjoint
        };
    }

    // One endpoint lying exactly on the other segment is a touch.
    let within =
        |p: DVec2, q0: DVec2, q1: DVec2| p.cmpge(q0.min(q1)).all() && p.cmple(q0.max(q1)).all();
    let touches = (o1 == Turn::Collinear && within(b0, a0, a1))
        || (o2 == Turn::Collinear && within(b1, a0, a1))
        || (o3 == Turn::Collinear && within(a0, b0, b1))
        || (o4 == Turn::Collinear && within(a1, b0, b1));
    if touches {
        Crossing2::Touch
    } else {
        Crossing2::Disjoint
    }
}

/// The crossing point of two lines through `a0–a1` and `b0–b1` in 2D, or `None`
/// if they are parallel. Use after [`classify_segments_2d`] reports `Proper`;
/// the classification is exact, the point itself is a rounded `f64` result.
pub fn line_line_2d(a0: DVec2, a1: DVec2, b0: DVec2, b1: DVec2) -> Option<DVec2> {
    let r = a1 - a0;
    let s = b1 - b0;
    let denom = r.perp_dot(s);
    if denom == 0.0 {
        return None;
    }
    let t = (b0 - a0).perp_dot(s) / denom;
    Some(a0 + t * r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    #[test]
    fn two_planes_meet_in_a_line() {
        let ground = Plane::from_point_normal(v(0.0, 0.0, 0.0), DVec3::Z).unwrap();
        let wall = Plane::from_point_normal(v(3.0, 0.0, 0.0), DVec3::X).unwrap();
        let (p, d) = plane_plane(&ground, &wall).unwrap();
        assert!(ground.signed_distance(p).abs() < 1e-12);
        assert!(wall.signed_distance(p).abs() < 1e-12);
        assert!((d.dot(DVec3::Y).abs() - 1.0).abs() < 1e-12);
        let lifted = Plane::from_point_normal(v(0.0, 0.0, 5.0), DVec3::Z).unwrap();
        assert!(plane_plane(&ground, &lifted).is_none());
    }

    #[test]
    fn a_line_across_a_square_with_a_hole_gives_two_stretches() {
        let outer = vec![
            v(0.0, 0.0, 0.0),
            v(10.0, 0.0, 0.0),
            v(10.0, 10.0, 0.0),
            v(0.0, 10.0, 0.0),
        ];
        let hole = vec![
            v(4.0, 4.0, 0.0),
            v(4.0, 6.0, 0.0),
            v(6.0, 6.0, 0.0),
            v(6.0, 4.0, 0.0),
        ];
        let cut = Plane::from_point_normal(v(0.0, 5.0, 0.0), DVec3::Y).unwrap();
        let spans = line_in_loops(&[outer.clone(), hole], &cut, v(0.0, 5.0, 0.0), DVec3::X);
        assert_eq!(spans.len(), 2);
        assert!((spans[0].0 - 0.0).abs() < 1e-12 && (spans[0].1 - 4.0).abs() < 1e-12);
        assert!((spans[1].0 - 6.0).abs() < 1e-12 && (spans[1].1 - 10.0).abs() < 1e-12);
        // Along an edge from inside: counted once, the edge itself.
        let edge = Plane::from_point_normal(v(0.0, 0.0, 0.0), DVec3::NEG_Y).unwrap();
        let spans = line_in_loops(&[outer], &edge, v(0.0, 0.0, 0.0), DVec3::X);
        assert_eq!(spans.len(), 1);
        assert!((spans[0].1 - spans[0].0 - 10.0).abs() < 1e-12);
    }

    #[test]
    fn common_spans_keep_only_shared_stretches() {
        let a = [(0.0, 4.0), (6.0, 10.0)];
        let b = [(2.0, 8.0)];
        assert_eq!(common_spans(&a, &b), [(2.0, 4.0), (6.0, 8.0)]);
        assert!(common_spans(&a, &[(4.0, 6.0)]).is_empty());
    }

    #[test]
    fn closest_point_clamps_to_segment() {
        let (t, q) = closest_point_on_segment(v(5.0, 1.0, 0.0), v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0));
        assert_eq!(t, 1.0);
        assert_eq!(q, v(2.0, 0.0, 0.0));
        let (t, _) = closest_point_on_segment(v(1.0, 3.0, 0.0), v(0.0, 0.0, 0.0), v(4.0, 0.0, 0.0));
        assert_eq!(t, 0.25);
    }

    #[test]
    fn crossing_segments_meet_at_a_point() {
        let r = segment_segment(
            v(0.0, 0.0, 0.0),
            v(2.0, 2.0, 0.0),
            v(0.0, 2.0, 0.0),
            v(2.0, 0.0, 0.0),
        );
        let SegmentIntersection::Point { point, t_a, t_b } = r else {
            panic!("expected a point, got {r:?}");
        };
        assert!((point - v(1.0, 1.0, 0.0)).length() < 1e-12);
        assert!((t_a - 0.5).abs() < 1e-12 && (t_b - 0.5).abs() < 1e-12);
    }

    #[test]
    fn skew_segments_within_merge_tolerance_meet() {
        // Crossing in plan view, 0.00005 mm apart vertically: inside TOL_MERGE.
        let r = segment_segment(
            v(0.0, 0.0, 0.0),
            v(1.0, 0.0, 0.0),
            v(0.5, -1.0, 5e-5),
            v(0.5, 1.0, 5e-5),
        );
        assert!(matches!(r, SegmentIntersection::Point { .. }));
        // 0.0002 mm apart: outside TOL_MERGE.
        let r = segment_segment(
            v(0.0, 0.0, 0.0),
            v(1.0, 0.0, 0.0),
            v(0.5, -1.0, 2e-4),
            v(0.5, 1.0, 2e-4),
        );
        assert_eq!(r, SegmentIntersection::None);
    }

    #[test]
    fn collinear_overlap_maps_parameters() {
        let r = segment_segment(
            v(0.0, 0.0, 0.0),
            v(4.0, 0.0, 0.0),
            v(3.0, 0.0, 0.0),
            v(1.0, 0.0, 0.0),
        );
        let SegmentIntersection::Overlap { t_a, t_b } = r else {
            panic!("expected overlap, got {r:?}");
        };
        assert_eq!(t_a, (0.25, 0.75));
        // b runs backwards: a's 0.25 is b's 1.0, a's 0.75 is b's 0.0.
        assert_eq!(t_b, (1.0, 0.0));
    }

    #[test]
    fn collinear_end_to_end_is_a_point() {
        let r = segment_segment(
            v(0.0, 0.0, 0.0),
            v(1.0, 0.0, 0.0),
            v(1.0, 0.0, 0.0),
            v(3.0, 0.0, 0.0),
        );
        let SegmentIntersection::Point { t_a, t_b, .. } = r else {
            panic!("expected point, got {r:?}");
        };
        assert_eq!((t_a, t_b), (1.0, 0.0));
        let r = segment_segment(
            v(0.0, 0.0, 0.0),
            v(1.0, 0.0, 0.0),
            v(2.0, 0.0, 0.0),
            v(3.0, 0.0, 0.0),
        );
        assert_eq!(r, SegmentIntersection::None);
    }

    #[test]
    fn sub_millimetre_crossing() {
        // A 0.001 mm cross: well above TOL_MERGE, must still intersect correctly.
        let r = segment_segment(
            v(0.0, 0.0, 0.0),
            v(0.001, 0.0, 0.0),
            v(0.0005, -0.0005, 0.0),
            v(0.0005, 0.0005, 0.0),
        );
        let SegmentIntersection::Point { point, .. } = r else {
            panic!("expected point, got {r:?}");
        };
        assert!((point - v(0.0005, 0.0, 0.0)).length() < 1e-15);
    }

    #[test]
    fn segment_plane_cases() {
        let ground = Plane::from_point_normal(DVec3::ZERO, DVec3::Z).unwrap();
        let r = segment_plane(v(1.0, 1.0, -1.0), v(1.0, 1.0, 3.0), &ground);
        assert_eq!(
            r,
            SegmentPlane::Point {
                t: 0.25,
                point: v(1.0, 1.0, 0.0)
            }
        );
        assert_eq!(
            segment_plane(v(0.0, 0.0, 1.0), v(1.0, 0.0, 2.0), &ground),
            SegmentPlane::None
        );
        assert_eq!(
            segment_plane(v(0.0, 0.0, 0.0), v(5.0, 0.0, 5e-5), &ground),
            SegmentPlane::InPlane
        );
        assert!(matches!(
            segment_plane(v(0.0, 0.0, 0.0), v(0.0, 0.0, 2.0), &ground),
            SegmentPlane::Point { t, .. } if t == 0.0
        ));
    }

    #[test]
    fn classify_2d_exact() {
        let p = |x: f64, y: f64| DVec2::new(x, y);
        assert_eq!(
            classify_segments_2d(p(0.0, 0.0), p(2.0, 2.0), p(0.0, 2.0), p(2.0, 0.0)),
            Crossing2::Proper
        );
        assert_eq!(
            classify_segments_2d(p(0.0, 0.0), p(2.0, 0.0), p(1.0, 0.0), p(1.0, 3.0)),
            Crossing2::Touch
        );
        assert_eq!(
            classify_segments_2d(p(0.0, 0.0), p(2.0, 0.0), p(2.0, 0.0), p(5.0, 1.0)),
            Crossing2::Touch
        );
        assert_eq!(
            classify_segments_2d(p(0.0, 0.0), p(2.0, 0.0), p(1.0, 0.0), p(3.0, 0.0)),
            Crossing2::Overlap
        );
        assert_eq!(
            classify_segments_2d(p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.0), p(3.0, 0.0)),
            Crossing2::Disjoint
        );
        assert_eq!(
            classify_segments_2d(p(0.0, 0.0), p(1.0, 0.0), p(0.0, 1.0), p(1.0, 1.0)),
            Crossing2::Disjoint
        );
        // A hair above the line: exact predicates must say Disjoint, not Touch.
        assert_eq!(
            classify_segments_2d(p(0.0, 0.0), p(2.0, 0.0), p(1.0, 1e-300), p(1.0, 3.0)),
            Crossing2::Disjoint
        );
    }

    #[test]
    fn line_line_point() {
        let p = |x: f64, y: f64| DVec2::new(x, y);
        assert_eq!(
            line_line_2d(p(0.0, 0.0), p(2.0, 2.0), p(0.0, 2.0), p(2.0, 0.0)),
            Some(p(1.0, 1.0))
        );
        assert_eq!(
            line_line_2d(p(0.0, 0.0), p(1.0, 0.0), p(0.0, 1.0), p(1.0, 1.0)),
            None
        );
    }
}
