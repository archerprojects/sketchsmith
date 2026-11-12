//! SketchSmith — Loop Cut, Knife and Align | v1.0.0 | Thursday 12 November 2026
//!
//! **Loop Cut** ([`loop_cut`]) cuts a ring of four-sided faces — the sides of
//! a cylinder or a box — all the way round, like Blender's Loop Cut. The ring
//! is found from an edge: the cut crosses that edge and the edge opposite it
//! in each four-sided face, face to face, until it meets a face that is not
//! four-sided, an edge not shared by exactly two faces, or comes back to the
//! start. `cuts` evenly spaced cuts are made. Each cut joins two points on
//! the boundary of a flat face, so every face stays flat.
//!
//! **Knife** ([`knife`]) cuts along a line drawn in the view: each drawn
//! segment and the eye define a flat cutting plane (in a parallel view, the
//! plane holds the view direction), and every face it crosses within the
//! segment's reach is cut along it, around corners. Each piece is a straight
//! line on a flat face, so every face stays flat. Only faces turned towards
//! the view are cut, unless Cut Through is on (Blender's Bisect).
//!
//! **Align** ([`align_items`]) sets the end points of the selected edges and
//! faces to one shared X, Y or Z value (Blender's scale-to-zero), subject to
//! the flat-face rule.

use smith_geom::{DMat4, DVec3};
use smith_model::{EdgeId, Entities, FaceId, ModelError, Pickable, VertexId};

/// One rail of the ring: an edge, and which of its ends the cuts measure from.
#[derive(Debug, Clone, Copy)]
struct Rail {
    edge: EdgeId,
    start: VertexId,
}

/// The four edges of a face's outer loop, if it is four-sided with no holes.
fn quad(entities: &Entities, f: FaceId) -> Option<[EdgeId; 4]> {
    let face = entities.face(f)?;
    if !face.inner().is_empty() || face.outer().len() != 4 {
        return None;
    }
    let o = face.outer();
    Some([o[0].edge, o[1].edge, o[2].edge, o[3].edge])
}

/// The edge opposite `e` in quad `f`, with the end of it that a side edge
/// joins to `start`.
fn across(entities: &Entities, f: FaceId, rail: Rail) -> Option<Rail> {
    let edges = quad(entities, f)?;
    let i = edges.iter().position(|x| *x == rail.edge)?;
    let opposite = edges[(i + 2) % 4];
    let [o0, o1] = entities.edge(opposite)?.vertices();
    // The side edge from `start` reaches one end of the opposite edge.
    let joined = |v: VertexId| {
        entities
            .edge_between(rail.start, v)
            .is_some_and(|s| edges.contains(&s))
    };
    let start = if joined(o0) {
        o0
    } else if joined(o1) {
        o1
    } else {
        return None;
    };
    Some(Rail {
        edge: opposite,
        start,
    })
}

/// The ring of rails through `edge`, and whether it closes on itself.
fn ring(entities: &Entities, edge: EdgeId) -> Option<(Vec<Rail>, bool)> {
    let e = entities.edge(edge)?;
    let first = Rail {
        edge,
        start: e.vertices()[0],
    };
    let faces: Vec<FaceId> = e
        .faces()
        .iter()
        .copied()
        .filter(|f| quad(entities, *f).is_some())
        .collect();
    let mut forward = vec![first];
    let mut closed = false;
    // Walk one way from the first quad face.
    if let Some(&f0) = faces.first() {
        let mut face = f0;
        let mut rail = first;
        while let Some(next) = across(entities, face, rail) {
            if next.edge == edge {
                closed = true;
                break;
            }
            if forward.iter().any(|r| r.edge == next.edge) {
                break;
            }
            forward.push(next);
            let shared = entities.edge(next.edge)?.faces();
            if shared.len() != 2 {
                break;
            }
            let other = if shared[0] == face {
                shared[1]
            } else {
                shared[0]
            };
            if quad(entities, other).is_none() {
                break;
            }
            face = other;
            rail = next;
        }
    }
    // An open ring also runs the other way from the start edge.
    let mut backward = Vec::new();
    if !closed && faces.len() == 2 {
        let mut face = faces[1];
        let mut rail = first;
        while let Some(next) = across(entities, face, rail) {
            if forward
                .iter()
                .chain(&backward)
                .any(|r: &Rail| r.edge == next.edge)
            {
                break;
            }
            backward.push(next);
            let shared = entities.edge(next.edge)?.faces();
            if shared.len() != 2 {
                break;
            }
            let other = if shared[0] == face {
                shared[1]
            } else {
                shared[0]
            };
            if quad(entities, other).is_none() {
                break;
            }
            face = other;
            rail = next;
        }
    }
    backward.reverse();
    backward.extend(forward);
    (backward.len() >= 2).then_some((backward, closed))
}

/// Where Loop Cut places its cuts along each side edge of the ring.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Spacing {
    /// 0: `cuts` even cuts. Otherwise the first cut this many mm along each
    /// side edge, and each further cut this much further on.
    pub every: f64,
    /// Measure from the other end of the side edges.
    pub from_end: bool,
}

/// The cut lines Loop Cut would draw through `edge`. Empty when there is no
/// ring, or when a measured cut would not land inside every side edge.
pub fn loop_cut_lines(
    entities: &Entities,
    edge: EdgeId,
    cuts: u32,
    spacing: Spacing,
) -> Vec<[DVec3; 2]> {
    let Some((rails, closed)) = ring(entities, edge) else {
        return Vec::new();
    };
    let point = |r: &Rail, t: f64| {
        let e = entities.edge(r.edge)?;
        let a = entities.vertex(r.start)?.position();
        let b = entities.vertex(e.other(r.start))?.position();
        Some(a + (b - a) * t)
    };
    let rail_len = |r: &Rail| -> Option<f64> {
        let e = entities.edge(r.edge)?;
        let a = entities.vertex(r.start)?.position();
        Some(a.distance(entities.vertex(e.other(r.start))?.position()))
    };
    let mut lines = Vec::new();
    for k in 1..=cuts.max(1) {
        let even = f64::from(k) / f64::from(cuts.max(1) + 1);
        let along = |r: &Rail| -> Option<f64> {
            if spacing.every <= 0.0 {
                return Some(even);
            }
            let len = rail_len(r)?;
            let t = spacing.every * f64::from(k) / len;
            // A cut on or past the far end is no cut.
            if t >= 1.0 - 1e-9 {
                return None;
            }
            Some(if spacing.from_end { 1.0 - t } else { t })
        };
        let pts: Vec<DVec3> = rails
            .iter()
            .filter_map(|r| along(r).and_then(|t| point(r, t)))
            .collect();
        if pts.len() != rails.len() {
            return Vec::new();
        }
        lines.extend(pts.windows(2).map(|w| [w[0], w[1]]));
        if closed && let (Some(a), Some(b)) = (pts.last(), pts.first()) {
            lines.push([*a, *b]);
        }
    }
    lines
}

/// Cuts the ring of four-sided faces through `edge` with `cuts` cuts,
/// placed by `spacing`.
///
/// # Errors
/// [`ModelError::DoesNotFit`] when `edge` has no ring of four-sided faces, or
/// a measured cut would not land inside every side edge; drawing errors.
pub fn loop_cut(
    entities: &mut Entities,
    edge: EdgeId,
    cuts: u32,
    spacing: Spacing,
) -> Result<(), ModelError> {
    let lines = loop_cut_lines(entities, edge, cuts, spacing);
    if lines.is_empty() {
        return Err(ModelError::DoesNotFit("loop cut"));
    }
    let closed = ring(entities, edge).is_some_and(|(_, c)| c);
    let ring_len = ring(entities, edge).map_or(0, |(r, _)| r.len());
    for [a, b] in &lines {
        entities.add_line(*a, *b)?;
    }
    // A closed ring of cuts is itself a flat loop and fills in as a face
    // across the inside of the solid; that face is not part of the cut.
    if closed && ring_len > 0 {
        for ring_lines in lines.chunks(ring_len) {
            let ids: std::collections::BTreeSet<VertexId> = ring_lines
                .iter()
                .filter_map(|[a, _]| entities.find_vertex(*a))
                .collect();
            let inside = entities.face_ids().find(|f| {
                entities.face(*f).is_some_and(|face| {
                    face.inner().is_empty() && {
                        let have: std::collections::BTreeSet<VertexId> =
                            entities.loop_vertices(face.outer()).into_iter().collect();
                        have == ids
                    }
                })
            });
            if let Some(f) = inside {
                entities.erase_face(f)?;
            }
        }
    }
    Ok(())
}

/// How the view looks at the model, for the Knife.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Eye position (perspective).
    pub eye: DVec3,
    /// Unit view direction.
    pub forward: DVec3,
    /// Parallel projection.
    pub parallel: bool,
}

impl View {
    /// The direction from the view towards `p`.
    fn ray(&self, p: DVec3) -> DVec3 {
        if self.parallel {
            self.forward
        } else {
            p - self.eye
        }
    }
}

/// The cut lines the Knife makes along `points` (in order), seen from `view`.
pub fn knife_lines(
    entities: &Entities,
    points: &[DVec3],
    view: &View,
    through: bool,
) -> Vec<[DVec3; 2]> {
    let mut out = Vec::new();
    for seg in points.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        let u = b - a;
        let n = u.cross(view.ray(a));
        if u.length() < 1e-9 || n.length() < 1e-12 {
            continue;
        }
        let n = n.normalize();
        // Inside the segment's reach: on b's side of the edge through a, and
        // on a's side of the edge through b (both edges along the view).
        let side = |q: DVec3, at: DVec3, other: DVec3| {
            let d = view.ray(at);
            let s = d.cross(q - at).dot(n);
            let o = d.cross(other - at).dot(n);
            s * o >= -1e-12
        };
        for f in entities.face_ids() {
            let Some(face) = entities.face(f) else {
                continue;
            };
            let plane = face.plane();
            if !through && plane.normal().dot(view.ray(a)) >= 0.0 {
                continue;
            }
            // Where the face's edges cross the cutting plane.
            let dir = n.cross(plane.normal());
            if dir.length() < 1e-9 {
                continue;
            }
            let mut hits: Vec<DVec3> = Vec::new();
            for l in face.loops() {
                let pts = entities.loop_points(l);
                let k = pts.len();
                for i in 0..k {
                    let (p, q) = (pts[i], pts[(i + 1) % k]);
                    let (dp, dq) = ((p - a).dot(n), (q - a).dot(n));
                    if (dp > 0.0) != (dq > 0.0) {
                        hits.push(p + (q - p) * (dp / (dp - dq)));
                    }
                }
            }
            hits.sort_by(|x, y| x.dot(dir).total_cmp(&y.dot(dir)));
            for pair in hits.chunks(2) {
                let [p, q] = pair else {
                    continue;
                };
                if let Some(piece) = clip(*p, *q, |x| side(x, a, b) && side(x, b, a)) {
                    out.push(piece);
                }
            }
        }
    }
    out
}

/// The part of segment `p`–`q` where `inside` holds, assuming that part is
/// one piece (it is: the inside is an intersection of half-planes).
fn clip(p: DVec3, q: DVec3, inside: impl Fn(DVec3) -> bool) -> Option<[DVec3; 2]> {
    let at = |t: f64| p + (q - p) * t;
    let (ip, iq) = (inside(p), inside(q));
    // Find the boundary between an inside and an outside parameter.
    let edge = |mut lo: f64, mut hi: f64| {
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if inside(at(mid)) == inside(at(lo)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    };
    let (s, e) = match (ip, iq) {
        (true, true) => (0.0, 1.0),
        (true, false) => (0.0, edge(0.0, 1.0)),
        (false, true) => (edge(0.0, 1.0), 1.0),
        (false, false) => {
            // Possibly inside in the middle only: look for an inside sample.
            let t = (1..64)
                .map(|i| f64::from(i) / 64.0)
                .find(|t| inside(at(*t)))?;
            (edge(0.0, t), edge(t, 1.0))
        }
    };
    let (x, y) = (at(s), at(e));
    (x.distance(y) > smith_geom::tolerance::TOL_MERGE).then_some([x, y])
}

/// Cuts the model along `points` as seen from `view`.
///
/// # Errors
/// [`ModelError::DoesNotFit`] when the line crosses no face; drawing errors.
pub fn knife(
    entities: &mut Entities,
    points: &[DVec3],
    view: &View,
    through: bool,
) -> Result<(), ModelError> {
    let lines = knife_lines(entities, points, view, through);
    if lines.is_empty() {
        return Err(ModelError::DoesNotFit("knife line"));
    }
    let mut made = std::collections::BTreeSet::new();
    for [a, b] in lines {
        made.extend(entities.add_line(a, b)?);
    }
    // A cut all the way round is itself a flat loop and fills in as a face
    // across the inside of the solid; that face is not part of the cut.
    let inside: Vec<FaceId> = entities
        .face_ids()
        .filter(|f| {
            entities
                .face(*f)
                .is_some_and(|face| face.loops().flatten().all(|u| made.contains(&u.edge)))
        })
        .collect();
    for f in inside {
        entities.erase_face(f)?;
    }
    Ok(())
}

/// Sets the end points of `items` to `value` along unit `axis` (a shared X,
/// Y or Z).
///
/// # Errors
/// [`ModelError::NotFlat`] or [`ModelError::Collides`]; nothing changes.
pub fn align_items(
    entities: &mut Entities,
    items: &[Pickable],
    axis: DVec3,
    value: f64,
) -> Result<(), ModelError> {
    if !axis.is_finite() || !value.is_finite() {
        return Err(ModelError::NonFinite);
    }
    let vertices: Vec<VertexId> = crate::modify::item_vertices(entities, items);
    if vertices.is_empty() {
        return Err(ModelError::NoSuchEntity);
    }
    // p' = p − axis (axis·p − value): a projection onto the plane axis·p = value.
    let a = axis.normalize();
    let outer = glam_outer(a);
    let m =
        DMat4::from_translation(a * value) * DMat4::from_mat3(smith_geom::DMat3::IDENTITY - outer);
    entities.transform_vertices(&vertices, &m)
}

/// The outer product a·aᵀ.
fn glam_outer(a: DVec3) -> smith_geom::DMat3 {
    smith_geom::DMat3::from_cols(a * a.x, a * a.y, a * a.z)
}

#[cfg(test)]
mod tests {
    use smith_geom::curve::{Circle, Curve, CurveFrame};

    use super::*;
    use crate::modify::extrude;

    fn cylinder(e: &mut Entities, segments: u32, height: f64) {
        let frame = CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap();
        e.add_curve(Curve::Circle(Circle::new(frame, 5.0).unwrap()), segments)
            .unwrap();
        let f = e.face_ids().next().unwrap();
        extrude(e, f, height, false).unwrap();
    }

    fn side_edge(e: &Entities) -> EdgeId {
        e.edge_ids()
            .find(|id| {
                let (a, b) = e.edge_points(*id).unwrap();
                (a.z - b.z).abs() > 1.0
            })
            .unwrap()
    }

    #[test]
    fn cuts_at_a_measured_spacing_from_either_end() {
        let heights = |from_end: bool| -> Vec<i64> {
            let mut e = Entities::new();
            cylinder(&mut e, 12, 12.0);
            let s = side_edge(&e);
            let spacing = Spacing {
                every: 2.0,
                from_end,
            };
            loop_cut(&mut e, s, 2, spacing).unwrap();
            e.validate().unwrap();
            let mut z: Vec<i64> = e
                .vertex_ids()
                .map(|v| (e.vertex(v).unwrap().position().z * 1000.0).round() as i64)
                .filter(|z| *z != 0 && *z != 12000)
                .collect();
            z.sort_unstable();
            z.dedup();
            z
        };
        let a = heights(false);
        let b = heights(true);
        // Two rings 2 mm apart, starting 2 mm from one end; From End the
        // other end.
        assert!(a == [2000, 4000] || a == [8000, 10000], "{a:?}");
        assert!(b == [2000, 4000] || b == [8000, 10000], "{b:?}");
        assert_ne!(a, b);
        // A spacing that runs off the end is refused.
        let mut e = Entities::new();
        cylinder(&mut e, 12, 12.0);
        let s = side_edge(&e);
        let far = Spacing {
            every: 7.0,
            from_end: false,
        };
        assert_eq!(
            loop_cut(&mut e, s, 2, far),
            Err(ModelError::DoesNotFit("loop cut"))
        );
    }

    #[test]
    fn three_cuts_on_a_cylinder_make_four_bands() {
        let mut e = Entities::new();
        cylinder(&mut e, 24, 12.0);
        let before = e.face_count();
        let s = side_edge(&e);
        loop_cut(&mut e, s, 3, Spacing::default()).unwrap();
        // 24 side faces become 96; caps unchanged.
        assert_eq!(e.face_count(), before + 24 * 3);
        for z in [3.0, 6.0, 9.0] {
            assert!(
                e.find_vertex(DVec3::new(5.0, 0.0, z)).is_some(),
                "cut at {z}"
            );
        }
        e.validate().unwrap();
    }

    #[test]
    fn a_box_side_ring_has_four_faces() {
        let mut e = Entities::new();
        e.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(10.0, 0.0, 0.0),
                DVec3::new(10.0, 10.0, 0.0),
                DVec3::new(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        let f = e.face_ids().next().unwrap();
        extrude(&mut e, f, 10.0, false).unwrap();
        let s = side_edge(&e);
        loop_cut(&mut e, s, 1, Spacing::default()).unwrap();
        assert_eq!(e.face_count(), 10);
        e.validate().unwrap();
    }

    fn cube(e: &mut Entities) {
        e.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(10.0, 0.0, 0.0),
                DVec3::new(10.0, 10.0, 0.0),
                DVec3::new(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        let f = e.face_ids().next().unwrap();
        extrude(e, f, 10.0, false).unwrap();
    }

    #[test]
    fn knife_across_two_faces_of_a_box() {
        let mut e = Entities::new();
        cube(&mut e);
        // Looking down at the box from above and in front, cut from the
        // top's back edge, over the front edge, down the front face.
        let view = View {
            eye: DVec3::new(5.0, -40.0, 40.0),
            forward: DVec3::new(0.0, 1.0, -1.0).normalize(),
            parallel: false,
        };
        let pts = [DVec3::new(5.0, 10.0, 10.0), DVec3::new(5.0, 0.0, 0.0)];
        knife(&mut e, &pts, &view, false).unwrap();
        // Top and front each split in two; the hidden faces are untouched.
        assert_eq!(e.face_count(), 8);
        e.validate().unwrap();
    }

    #[test]
    fn cut_through_also_cuts_the_far_side() {
        let mut e = Entities::new();
        cube(&mut e);
        let view = View {
            eye: DVec3::new(5.0, 5.0, 100.0),
            forward: DVec3::NEG_Z,
            parallel: true,
        };
        let pts = [DVec3::new(5.0, -1.0, 10.0), DVec3::new(5.0, 11.0, 10.0)];
        knife(&mut e, &pts, &view, true).unwrap();
        // Top, bottom, front and back all split.
        assert_eq!(e.face_count(), 10);
        e.validate().unwrap();
    }

    #[test]
    fn align_flattens_to_one_height() {
        let mut e = Entities::new();
        e.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(10.0, 0.0, 2.0),
                DVec3::new(20.0, 0.0, 5.0),
            ],
            false,
        )
        .unwrap();
        let items: Vec<Pickable> = e.edge_ids().map(Pickable::Edge).collect();
        align_items(&mut e, &items, DVec3::Z, 1.0).unwrap();
        assert!(
            e.vertex_ids()
                .all(|v| (e.vertex(v).unwrap().position().z - 1.0).abs() < 1e-12)
        );
    }
}
