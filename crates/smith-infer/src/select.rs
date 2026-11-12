//! SketchSmith — screen-space selection picking | v1.0.0 | Thursday 12 November 2026
//!
//! Turns a click or a drag rectangle into the entities it selects.
//!
//! * **Click.** Edges win over faces, as in SketchUp: an edge within the pick
//!   radius of the cursor (6 px by default) and not hidden behind a
//!   face is chosen; otherwise the face under the cursor. With a select
//!   mode ([`pick_kind`]), only that kind is picked: a vertex within its
//!   pick radius (8 px), an edge, or the face under the cursor.
//! * **Window (drag left → right).** Only entities entirely inside the box.
//! * **Crossing (drag right → left).** Anything the box touches.
//!
//! Screen positions are in logical pixels with y down; the caller supplies a
//! projection function (world → screen, `None` if behind the camera).

use smith_geom::intersect::closest_points_segments;
use smith_geom::{DVec2, DVec3};
use smith_model::{EdgeId, Entities, Pickable, SelectKind, VertexId};

use crate::bvh::{FaceBvh, SegmentBvh};

/// The edge and vertex indexes picking looks through, built once per
/// change of the geometry they index.
#[derive(Debug, Clone, Default)]
pub struct PickIndex {
    /// The edges.
    pub edges: SegmentBvh<EdgeId>,
    /// The vertices.
    pub vertices: SegmentBvh<VertexId>,
}

impl PickIndex {
    /// Both indexes for `entities`.
    pub fn build(entities: &Entities) -> Self {
        Self {
            edges: SegmentBvh::edges(entities),
            vertices: SegmentBvh::vertices(entities),
        }
    }
}

/// Default edge pick radius in logical pixels.
pub const EDGE_PICK_PX: f64 = 6.0;

/// Vertex pick radius in logical pixels.
pub const VERTEX_PICK_PX: f64 = 8.0;

/// Picks the item of `kind` under the cursor: the nearest vertex within
/// [`VERTEX_PICK_PX`] not hidden behind a face, an edge as
/// [`pick_click`] chooses one, or the face under the cursor.
pub fn pick_kind(
    entities: &Entities,
    bvh: &FaceBvh,
    origin: DVec3,
    dir: DVec3,
    px_at: impl Fn(f64) -> f64,
    kind: SelectKind,
) -> Option<Pickable> {
    pick_kind_in(&PickIndex::build(entities), bvh, origin, dir, px_at, kind)
}

/// [`pick_kind`] through a [`PickIndex`] built for the same geometry.
pub fn pick_kind_in(
    index: &PickIndex,
    bvh: &FaceBvh,
    origin: DVec3,
    dir: DVec3,
    px_at: impl Fn(f64) -> f64,
    kind: SelectKind,
) -> Option<Pickable> {
    match kind {
        SelectKind::Vertices => {
            pick_vertex_in(&index.vertices, bvh, origin, dir, px_at).map(Pickable::Vertex)
        }
        SelectKind::Edges => match pick_click_in(&index.edges, bvh, origin, dir, px_at) {
            Some(Pickable::Edge(e)) => Some(Pickable::Edge(e)),
            _ => None,
        },
        SelectKind::Faces => bvh.ray_cast(origin, dir).map(|h| Pickable::Face(h.face)),
    }
}

/// The vertex nearest the cursor ray within [`VERTEX_PICK_PX`], not hidden
/// behind a face.
pub fn pick_vertex(
    entities: &Entities,
    bvh: &FaceBvh,
    origin: DVec3,
    dir: DVec3,
    px_at: impl Fn(f64) -> f64,
) -> Option<VertexId> {
    pick_vertex_in(&SegmentBvh::vertices(entities), bvh, origin, dir, px_at)
}

/// [`pick_vertex`] through a vertex index built for the same geometry.
pub fn pick_vertex_in(
    vertices: &SegmentBvh<VertexId>,
    bvh: &FaceBvh,
    origin: DVec3,
    dir: DVec3,
    px_at: impl Fn(f64) -> f64,
) -> Option<VertexId> {
    let far = bvh.ray_cast(origin, dir).map_or(1e12, |h| h.t);
    let mut best: Option<(f64, VertexId)> = None;
    let radius = |t: f64| VERTEX_PICK_PX * px_at(t);
    vertices.near_ray(origin, dir, far, radius, |p, _, v| {
        let t = (p - origin).dot(dir);
        if t <= 0.0 {
            return;
        }
        let off = (origin + dir * t).distance(p);
        let tolerance = VERTEX_PICK_PX * px_at(t);
        if off <= tolerance && t <= far + tolerance && best.is_none_or(|(bt, _)| t < bt) {
            best = Some((t, v));
        }
    });
    best.map(|(_, v)| v)
}

/// The vertices inside a drag rectangle with corners `a` and `b` (screen
/// px).
pub fn pick_rect_vertices(
    entities: &Entities,
    a: DVec2,
    b: DVec2,
    project: impl Fn(DVec3) -> Option<DVec2>,
) -> Vec<Pickable> {
    let (min, max) = (a.min(b), a.max(b));
    entities
        .vertex_ids()
        .filter(|v| {
            entities
                .vertex(*v)
                .and_then(|x| project(x.position()))
                .is_some_and(|p| p.cmpge(min).all() && p.cmple(max).all())
        })
        .map(Pickable::Vertex)
        .collect()
}

/// Picks the entity under the cursor.
///
/// `origin`/`dir` is the cursor ray; `px_at(t)` is the world size of one
/// pixel at distance `t` along it.
pub fn pick_click(
    entities: &Entities,
    bvh: &FaceBvh,
    origin: DVec3,
    dir: DVec3,
    px_at: impl Fn(f64) -> f64,
) -> Option<Pickable> {
    pick_click_in(&SegmentBvh::edges(entities), bvh, origin, dir, px_at)
}

/// [`pick_click`] through an edge index built for the same geometry.
pub fn pick_click_in(
    edges: &SegmentBvh<EdgeId>,
    bvh: &FaceBvh,
    origin: DVec3,
    dir: DVec3,
    px_at: impl Fn(f64) -> f64,
) -> Option<Pickable> {
    let face_hit = bvh.ray_cast(origin, dir);
    let far = face_hit.map_or(1e12, |h| h.t);
    let ray_end = origin + dir * 1e12;
    let mut best: Option<(f64, Pickable)> = None;
    let radius = |t: f64| EDGE_PICK_PX * px_at(t);
    edges.near_ray(origin, dir, far, radius, |a, b, e| {
        let (s, _, on_ray, on_edge) = closest_points_segments(origin, ray_end, a, b);
        let t = s * 1e12;
        let tolerance = EDGE_PICK_PX * px_at(t);
        // Allow an edge lying on the hit face (same depth) but not behind it.
        if on_ray.distance(on_edge) <= tolerance
            && t <= far + tolerance
            && best.is_none_or(|(bt, _)| t < bt)
        {
            best = Some((t, Pickable::Edge(e)));
        }
    });
    best.map(|(_, p)| p)
        .or(face_hit.map(|h| Pickable::Face(h.face)))
}

/// Entities selected by a drag rectangle with corners `a` and `b` (screen px).
/// `crossing` selects anything touched; otherwise only entities fully inside.
pub fn pick_rect(
    entities: &Entities,
    a: DVec2,
    b: DVec2,
    crossing: bool,
    project: impl Fn(DVec3) -> Option<DVec2>,
) -> Vec<Pickable> {
    let (min, max) = (a.min(b), a.max(b));
    let inside = |p: Option<DVec2>| p.is_some_and(|p| p.cmpge(min).all() && p.cmple(max).all());
    let mut out = Vec::new();
    for e in entities.edge_ids() {
        let Some((p, q)) = entities.edge_points(e) else {
            continue;
        };
        let (sp, sq) = (project(p), project(q));
        let hit = if crossing {
            inside(sp)
                || inside(sq)
                || matches!((sp, sq), (Some(x), Some(y)) if segment_hits_rect(x, y, min, max))
        } else {
            inside(sp) && inside(sq)
        };
        if hit {
            out.push(Pickable::Edge(e));
        }
    }
    for f in entities.face_ids() {
        let Some(face) = entities.face(f) else {
            continue;
        };
        let pts: Vec<Option<DVec2>> = entities
            .loop_points(face.outer())
            .into_iter()
            .map(&project)
            .collect();
        let hit = if crossing {
            let n = pts.len();
            pts.iter().any(|p| inside(*p))
                || (0..n).any(|i| matches!((pts[i], pts[(i + 1) % n]), (Some(x), Some(y)) if segment_hits_rect(x, y, min, max)))
                || {
                    let poly: Option<Vec<DVec2>> = pts.iter().copied().collect();
                    poly.is_some_and(|poly| point_in_polygon((min + max) * 0.5, &poly))
                }
        } else {
            pts.iter().all(|p| inside(*p))
        };
        if hit {
            out.push(Pickable::Face(f));
        }
    }
    out
}

/// Whether segment `p–q` crosses any side of the axis-aligned rectangle.
fn segment_hits_rect(p: DVec2, q: DVec2, min: DVec2, max: DVec2) -> bool {
    let corners = [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)];
    (0..4).any(|i| segments_cross(p, q, corners[i], corners[(i + 1) % 4]))
}

fn segments_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let o = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
    let (d1, d2, d3, d4) = (o(a, b, c), o(a, b, d), o(c, d, a), o(c, d, b));
    (d1 * d2 <= 0.0) && (d3 * d4 <= 0.0) && !(d1 == 0.0 && d2 == 0.0)
}

fn point_in_polygon(q: DVec2, poly: &[DVec2]) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    /// Top-down orthographic projection: 10 px per mm, y flipped.
    fn top(p: DVec3) -> Option<DVec2> {
        Some(DVec2::new(p.x * 10.0, -p.y * 10.0))
    }

    fn square() -> Entities {
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
        m
    }

    #[test]
    fn click_prefers_edge_near_cursor_then_face() {
        let m = square();
        let bvh = FaceBvh::build(&m);
        let px = |_t: f64| 0.1; // 0.1 mm per pixel
        // 0.3 mm (3 px) from the bottom edge: the edge.
        let near_edge = pick_click(&m, &bvh, v(5.0, 0.3, 50.0), -DVec3::Z, px);
        assert!(matches!(near_edge, Some(Pickable::Edge(_))));
        // Middle of the face: the face.
        let mid = pick_click(&m, &bvh, v(5.0, 5.0, 50.0), -DVec3::Z, px);
        assert!(matches!(mid, Some(Pickable::Face(_))));
        // Outside everything.
        assert_eq!(
            pick_click(&m, &bvh, v(50.0, 50.0, 50.0), -DVec3::Z, px),
            None
        );
    }

    #[test]
    fn each_mode_picks_only_its_kind() {
        let m = square();
        let bvh = FaceBvh::build(&m);
        let px = |_t: f64| 0.1;
        let corner = v(0.5, 0.3, 50.0);
        let pick = |at, kind| pick_kind(&m, &bvh, at, -DVec3::Z, px, kind);
        // Near a corner: a vertex, an edge, or the face beneath.
        assert!(matches!(
            pick(corner, SelectKind::Vertices),
            Some(Pickable::Vertex(_))
        ));
        assert!(matches!(
            pick(corner, SelectKind::Edges),
            Some(Pickable::Edge(_))
        ));
        assert!(matches!(
            pick(corner, SelectKind::Faces),
            Some(Pickable::Face(_))
        ));
        // The middle of the face: no vertex or edge there.
        let mid = v(5.0, 5.0, 50.0);
        assert_eq!(pick(mid, SelectKind::Vertices), None);
        assert_eq!(pick(mid, SelectKind::Edges), None);
        let inside = pick_rect_vertices(&m, DVec2::new(-10.0, 10.0), DVec2::new(50.0, -110.0), top);
        assert_eq!(inside.len(), 2, "the two left corners");
    }

    #[test]
    fn edge_behind_a_face_is_not_picked() {
        let mut m = square();
        // A line 5 mm below the square, under the cursor.
        m.add_line(v(-5.0, 5.0, -5.0), v(15.0, 5.0, -5.0)).unwrap();
        let bvh = FaceBvh::build(&m);
        let hit = pick_click(&m, &bvh, v(5.0, 5.0, 50.0), -DVec3::Z, |_| 0.1);
        assert!(matches!(hit, Some(Pickable::Face(_))));
    }

    #[test]
    fn window_versus_crossing() {
        let m = square();
        // Box covering the left half only (screen coords).
        let a = DVec2::new(-10.0, 10.0);
        let b = DVec2::new(50.0, -110.0);
        let window = pick_rect(&m, a, b, false, top);
        assert_eq!(window.len(), 1, "only the left edge is fully inside");
        let crossing = pick_rect(&m, a, b, true, top);
        assert_eq!(
            crossing.len(),
            4,
            "left, top and bottom edges plus the face; the right edge is untouched"
        );
    }

    #[test]
    fn crossing_box_inside_a_face_selects_the_face() {
        let m = square();
        let crossing = pick_rect(
            &m,
            DVec2::new(40.0, -40.0),
            DVec2::new(60.0, -60.0),
            true,
            top,
        );
        assert_eq!(crossing.len(), 1);
        assert!(matches!(crossing[0], Pickable::Face(_)));
    }
}
