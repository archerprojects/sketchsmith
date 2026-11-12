//! SketchSmith — offset, divide, slide, dissolve and face direction | v1.0.0 | Thursday 12 November 2026
//!
//! **Offset (Inset)** ([`offset_face`]) draws a copy of a face's outline and
//! of each of its holes a set distance into the face (or out of it, for a
//! negative distance), so the face splits into bands and an inner face. Each
//! corner is the meeting point of the two offset edges (a miter). A circle or
//! polygon loop gives a circle or polygon curve, so the offset stays
//! editable. `count` repeats the offset that many times at the same spacing,
//! for concentric rings. An offset that would cross itself or another loop's
//! offset is refused.
//!
//! **Offset of an edge chain** ([`offset_chain`]): the chain through an edge
//! (followed through every point where exactly two edges meet) is offset to
//! one side within its plane, open ends straight out, corners mitred. A chain
//! that is one arc or circle gives an arc or circle of the new radius, so it
//! stays editable.
//!
//! **Divide** ([`divide_edge`]) inserts points along an edge: into equal
//! parts, every set length (with the remainder at the far end, or split
//! between both ends), or one point at a distance. Measured from either end.
//!
//! **Reverse Faces** and **Orient Faces** turn faces front to back.
//!
//! **Slide** ([`slide`]) moves a point along one of its edges, or an edge
//! along the edges at its two ends, by a set distance; attached faces must
//! stay flat. **Dissolve** removes a point between two edges in a straight
//! line, joining them.

use smith_geom::DVec3;
use smith_geom::curve::{Circle, Curve, Polygon};
use smith_geom::tolerance::TOL_MERGE;
use smith_model::{EdgeId, Entities, FaceId, ModelError, VertexId};

/// Draws `count` offsets of `face`'s outline and holes, each `distance` mm
/// further into the face (negative: out of it, so the outline grows and the
/// holes shrink).
///
/// # Errors
/// [`ModelError::NoSuchEntity`]; [`ModelError::ZeroLength`];
/// [`ModelError::DoesNotFit`] when an offset would cross itself or another
/// loop's offset, or a hole's offset would leave the outline's; drawing errors.
pub fn offset_face(
    entities: &mut Entities,
    face: FaceId,
    distance: f64,
    count: u32,
) -> Result<(), ModelError> {
    if !distance.is_finite() {
        return Err(ModelError::NonFinite);
    }
    if distance.abs() <= TOL_MERGE {
        return Err(ModelError::ZeroLength);
    }
    let n = entities
        .face(face)
        .ok_or(ModelError::NoSuchEntity)?
        .plane()
        .normal();
    let loops = face_loops(entities, face);
    let mut planned = Vec::new();
    for k in 1..=count.max(1) {
        let d = distance * f64::from(k);
        let shapes = loops
            .iter()
            .map(|l| l.offset(n, d))
            .collect::<Result<Vec<_>, _>>()?;
        check_apart(&shapes, n)?;
        planned.extend(shapes);
    }
    for shape in planned {
        match shape.curve {
            Some((c, segments)) => {
                entities.add_curve(c, segments)?;
            }
            None => {
                entities.add_polyline(&shape.points, true)?;
            }
        }
    }
    Ok(())
}

/// One loop of a face, for offsetting.
struct FaceLoop {
    /// The loop's points, turned so the face lies to their left about the
    /// normal (the outline counter-clockwise, holes clockwise).
    points: Vec<DVec3>,
    /// The circle or polygon curve the whole loop is, with its segments.
    curve: Option<(Curve, u32)>,
    hole: bool,
}

/// An offset loop: points for checking (and drawing, without a curve).
struct Offset {
    points: Vec<DVec3>,
    curve: Option<(Curve, u32)>,
}

impl FaceLoop {
    /// This loop moved `d` into the face.
    fn offset(&self, n: DVec3, d: f64) -> Result<Offset, ModelError> {
        // A hole grows away from the face's material as the outline shrinks.
        let grow = if self.hole { d } else { -d };
        let scaled = |center: DVec3, r: f64, r2: f64| -> Vec<DVec3> {
            self.points
                .iter()
                .map(|p| center + (*p - center) * (r2 / r))
                .collect()
        };
        match self.curve {
            Some((Curve::Circle(c), segments)) => {
                let r = c.radius() + grow;
                if r <= TOL_MERGE {
                    return Err(ModelError::DoesNotFit("offset"));
                }
                Ok(Offset {
                    points: scaled(c.frame().center(), c.radius(), r),
                    curve: Some((Curve::Circle(Circle::new(*c.frame(), r)?), segments)),
                })
            }
            Some((Curve::Polygon(pg), segments)) => {
                let half = std::f64::consts::PI / f64::from(pg.sides());
                let r = pg.radius() + grow / half.cos();
                if r <= TOL_MERGE {
                    return Err(ModelError::DoesNotFit("offset"));
                }
                Ok(Offset {
                    points: scaled(pg.frame().center(), pg.radius(), r),
                    curve: Some((
                        Curve::Polygon(Polygon::new(*pg.frame(), r, pg.sides())?),
                        segments,
                    )),
                })
            }
            _ => Ok(Offset {
                points: miter_offset(&self.points, n, d)?,
                curve: None,
            }),
        }
    }
}

/// The outline and holes of `face`, outline first.
fn face_loops(entities: &Entities, face: FaceId) -> Vec<FaceLoop> {
    let Some(f) = entities.face(face) else {
        return Vec::new();
    };
    let n = f.plane().normal();
    f.loops()
        .enumerate()
        .map(|(i, l)| {
            let hole = i > 0;
            let mut points = entities.loop_points(l);
            if (signed_area(&points, n) > 0.0) == hole {
                points.reverse();
            }
            let curves: Vec<_> = l
                .iter()
                .map(|u| entities.edge(u.edge).and_then(|e| e.curve()))
                .collect();
            let curve = match curves.first() {
                Some(Some(c)) if curves.iter().all(|x| *x == Some(*c)) => entities
                    .curve(*c)
                    .map(|r| (r.curve().clone(), r.segments()))
                    .filter(|(c, _)| matches!(c, Curve::Circle(_) | Curve::Polygon(_))),
                _ => None,
            };
            FaceLoop {
                points,
                curve,
                hole,
            }
        })
        .collect()
}

/// Twice the area enclosed by `pts`, positive when they run counter-clockwise
/// about `n`.
fn signed_area(pts: &[DVec3], n: DVec3) -> f64 {
    let k = pts.len();
    (0..k).map(|i| pts[i].cross(pts[(i + 1) % k]).dot(n)).sum()
}

/// Refuses offset loops that cross each other, or a hole's offset (after the
/// first) that is not inside the outline's.
fn check_apart(shapes: &[Offset], n: DVec3) -> Result<(), ModelError> {
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    let flat: Vec<Vec<(f64, f64)>> = shapes
        .iter()
        .map(|s| s.points.iter().map(|p| (p.dot(u), p.dot(v))).collect())
        .collect();
    let refuse = Err(ModelError::DoesNotFit("offset"));
    for i in 0..flat.len() {
        for j in i + 1..flat.len() {
            if loops_cross(&flat[i], &flat[j]) {
                return refuse;
            }
            let inside_outline = i > 0 || inside(flat[j][0], &flat[0]);
            let nested = i > 0 && (inside(flat[j][0], &flat[i]) || inside(flat[i][0], &flat[j]));
            if !inside_outline || nested {
                return refuse;
            }
        }
    }
    Ok(())
}

fn loops_cross(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
    let orient = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| {
        (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
    };
    let (ka, kb) = (a.len(), b.len());
    (0..ka).any(|i| {
        let (p, q) = (a[i], a[(i + 1) % ka]);
        (0..kb).any(|j| {
            let (r, s) = (b[j], b[(j + 1) % kb]);
            orient(p, q, r) * orient(p, q, s) <= 0.0 && orient(r, s, p) * orient(r, s, q) <= 0.0
        })
    })
}

/// Whether `p` lies inside the polygon `poly` (even-odd rule).
fn inside(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
    let k = poly.len();
    let mut on = false;
    for i in 0..k {
        let (a, b) = (poly[i], poly[(i + 1) % k]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) * (b.0 - a.0) / (b.1 - a.1) {
            on = !on;
        }
    }
    on
}

/// The outline `pts` (counter-clockwise about `n`) moved `d` inward, each
/// corner where the two offset edges meet.
fn miter_offset(pts: &[DVec3], n: DVec3, d: f64) -> Result<Vec<DVec3>, ModelError> {
    let k = pts.len();
    if k < 3 {
        return Err(ModelError::DoesNotFit("offset"));
    }
    let inward = |i: usize| {
        let a = pts[i];
        let b = pts[(i + 1) % k];
        n.cross(b - a).normalize()
    };
    let mut out = Vec::with_capacity(k);
    for (j, p) in pts.iter().enumerate() {
        let a = inward((j + k - 1) % k);
        let b = inward(j);
        let denom = 1.0 + a.dot(b);
        if denom < 1e-6 {
            return Err(ModelError::DoesNotFit("offset"));
        }
        out.push(*p + (a + b) * (d / denom));
    }
    // Each offset edge must keep its direction; a reversed edge means the
    // offset has crossed itself.
    for i in 0..k {
        let before = pts[(i + 1) % k] - pts[i];
        let after = out[(i + 1) % k] - out[i];
        if before.dot(after) <= 0.0 || after.length() <= TOL_MERGE {
            return Err(ModelError::DoesNotFit("offset"));
        }
    }
    Ok(out)
}

/// The chain of edges through one edge: its points in order, whether it
/// closes, and the arc or circle it is when it is exactly one.
#[derive(Debug, Clone)]
pub struct Chain {
    /// The points, in order (not repeating the first for a closed chain).
    pub points: Vec<DVec3>,
    /// The chain closes on itself.
    pub closed: bool,
    /// The arc or circle the whole chain is, with its segments.
    pub curve: Option<(Curve, u32)>,
}

/// The chain through `edge`: from each end, it carries on through every
/// point where exactly two edges meet.
pub fn edge_chain(entities: &Entities, edge: EdgeId) -> Option<Chain> {
    let first = entities.edge(edge)?;
    let [a, b] = first.vertices();
    let mut edges = vec![edge];
    // Walks on from `v`, arriving along `from`, collecting points.
    let walk = |mut v: smith_model::VertexId, mut from: EdgeId, edges: &mut Vec<EdgeId>| {
        let mut pts = Vec::new();
        while let Some(vx) = entities.vertex(v) {
            let ends = vx.edges();
            if ends.len() != 2 {
                break;
            }
            let next = if ends[0] == from { ends[1] } else { ends[0] };
            if edges.contains(&next) {
                break;
            }
            edges.push(next);
            let Some(e) = entities.edge(next) else { break };
            v = e.other(v);
            from = next;
            pts.push(v);
        }
        pts
    };
    let forward = walk(b, edge, &mut edges);
    let closed = forward.last() == Some(&a);
    let backward = if closed {
        Vec::new()
    } else {
        walk(a, edge, &mut edges)
    };
    let mut order: Vec<smith_model::VertexId> = backward.into_iter().rev().collect();
    order.push(a);
    order.push(b);
    order.extend(forward);
    if closed {
        order.pop();
    }
    let points: Vec<DVec3> = order
        .iter()
        .filter_map(|v| entities.vertex(*v).map(|x| x.position()))
        .collect();
    let curves: Vec<_> = edges
        .iter()
        .map(|e| entities.edge(*e).and_then(|x| x.curve()))
        .collect();
    let curve = match curves.first() {
        Some(Some(c)) if curves.iter().all(|x| *x == Some(*c)) => entities
            .curve(*c)
            .filter(|r| r.edges().len() == edges.len())
            .map(|r| (r.curve().clone(), r.segments()))
            .filter(|(c, _)| matches!(c, Curve::Arc(_) | Curve::Circle(_))),
        _ => None,
    };
    Some(Chain {
        points,
        closed,
        curve,
    })
}

/// The plane normal of `pts` (Newell's method); `hint` when they lie on a line.
pub fn chain_normal(pts: &[DVec3], hint: DVec3) -> DVec3 {
    let k = pts.len();
    let n: DVec3 = (0..k).map(|i| pts[i].cross(pts[(i + 1) % k])).sum();
    if n.length() > 1e-9 * (1.0 + pts.iter().map(|p| p.length_squared()).sum::<f64>()) {
        n.normalize()
    } else {
        hint.normalize_or(DVec3::Z)
    }
}

/// `chain` offset `d` mm to its left about `n` (negative: to its right).
fn chain_offset_points(chain: &Chain, n: DVec3, d: f64) -> Result<Vec<DVec3>, ModelError> {
    let pts = &chain.points;
    if chain.closed {
        return miter_offset(pts, n, d);
    }
    let k = pts.len();
    if k < 2 {
        return Err(ModelError::DoesNotFit("offset"));
    }
    let left = |i: usize| n.cross(pts[i + 1] - pts[i]).normalize_or_zero();
    let mut out = Vec::with_capacity(k);
    out.push(pts[0] + left(0) * d);
    for (j, p) in pts.iter().enumerate().take(k - 1).skip(1) {
        let (a, b) = (left(j - 1), left(j));
        let denom = 1.0 + a.dot(b);
        if denom < 1e-6 {
            return Err(ModelError::DoesNotFit("offset"));
        }
        out.push(*p + (a + b) * (d / denom));
    }
    out.push(pts[k - 1] + left(k - 2) * d);
    for i in 0..k - 1 {
        let before = pts[i + 1] - pts[i];
        let after = out[i + 1] - out[i];
        if before.dot(after) <= 0.0 || after.length() <= TOL_MERGE {
            return Err(ModelError::DoesNotFit("offset"));
        }
    }
    Ok(out)
}

/// What offsetting `chain` by `d` draws: the arc or circle of the new radius
/// when the chain is one, else the offset points.
fn chain_offset(chain: &Chain, n: DVec3, d: f64) -> Result<Offset, ModelError> {
    let points = chain_offset_points(chain, n, d)?;
    // The new radius is exactly the old one plus or minus the distance; the
    // offset of the first point tells which.
    let radius = |centre: DVec3, r: f64| {
        if centre.distance(points[0]) > r {
            r + d.abs()
        } else {
            r - d.abs()
        }
    };
    let curve = match &chain.curve {
        Some((Curve::Arc(a), segments)) => {
            let r = radius(a.frame().center(), a.radius());
            Some((
                Curve::Arc(smith_geom::curve::Arc::new(
                    *a.frame(),
                    r,
                    a.start_angle(),
                    a.end_angle(),
                )?),
                *segments,
            ))
        }
        Some((Curve::Circle(c), segments)) => {
            let r = radius(c.frame().center(), c.radius());
            Some((Curve::Circle(Circle::new(*c.frame(), r)?), *segments))
        }
        _ => None,
    };
    Ok(Offset { points, curve })
}

/// Draws `count` offsets of the chain through `edge`, each `distance` mm
/// further to its left about `normal` (negative: right). `normal` is used
/// only when the chain is a straight line; otherwise its own plane is.
///
/// # Errors
/// [`ModelError::NoSuchEntity`]; [`ModelError::ZeroLength`];
/// [`ModelError::DoesNotFit`] when an offset would cross itself; drawing
/// errors.
pub fn offset_chain(
    entities: &mut Entities,
    edge: EdgeId,
    distance: f64,
    count: u32,
    normal: DVec3,
) -> Result<(), ModelError> {
    if !distance.is_finite() || !normal.is_finite() {
        return Err(ModelError::NonFinite);
    }
    if distance.abs() <= TOL_MERGE {
        return Err(ModelError::ZeroLength);
    }
    let chain = edge_chain(entities, edge).ok_or(ModelError::NoSuchEntity)?;
    let n = chain_normal(&chain.points, normal);
    let shapes = (1..=count.max(1))
        .map(|k| chain_offset(&chain, n, distance * f64::from(k)))
        .collect::<Result<Vec<_>, _>>()?;
    for shape in shapes {
        match shape.curve {
            Some((c, segments)) => {
                entities.add_curve(c, segments)?;
            }
            None => {
                entities.add_polyline(&shape.points, chain.closed)?;
            }
        }
    }
    Ok(())
}

/// The chain through `edge` offset `d` mm (see [`offset_chain`]), for
/// showing while the Offset tool works; empty when it does not fit.
pub fn offset_chain_preview(
    entities: &Entities,
    edge: EdgeId,
    d: f64,
    normal: DVec3,
) -> Vec<[DVec3; 2]> {
    let Some(chain) = edge_chain(entities, edge) else {
        return Vec::new();
    };
    let n = chain_normal(&chain.points, normal);
    let Ok(pts) = chain_offset_points(&chain, n, d) else {
        return Vec::new();
    };
    let mut lines: Vec<[DVec3; 2]> = pts.windows(2).map(|w| [w[0], w[1]]).collect();
    if chain.closed
        && let (Some(a), Some(b)) = (pts.last(), pts.first())
    {
        lines.push([*a, *b]);
    }
    lines
}

/// Which side of the chain through `edge` the point `p` is on, and how far:
/// positive to the chain's left about `normal` (as [`offset_chain`] measures).
pub fn chain_side(entities: &Entities, edge: EdgeId, p: DVec3, normal: DVec3) -> Option<f64> {
    let chain = edge_chain(entities, edge)?;
    let n = chain_normal(&chain.points, normal);
    let pts = &chain.points;
    let k = pts.len();
    let segs = if chain.closed { k } else { k.saturating_sub(1) };
    (0..segs)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % k]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
            let q = a + ab * t;
            let flat = (p - q) - n * n.dot(p - q);
            let side = n.cross(ab).dot(flat).signum();
            (flat.length(), side)
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(dist, side)| dist * side)
}

/// The loops of `face` offset `d` mm into it, for showing while the Offset
/// tool works. Empty when the offset does not fit.
pub fn offset_preview(entities: &Entities, face: FaceId, d: f64) -> Vec<Vec<DVec3>> {
    let Some(f) = entities.face(face) else {
        return Vec::new();
    };
    let n = f.plane().normal();
    let shapes: Result<Vec<Offset>, ModelError> = face_loops(entities, face)
        .iter()
        .map(|l| l.offset(n, d))
        .collect();
    match shapes {
        Ok(s) if check_apart(&s, n).is_ok() => s.into_iter().map(|o| o.points).collect(),
        _ => Vec::new(),
    }
}

/// How [`divide_edge`] places points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Division {
    /// Into this many equal parts.
    Parts(u32),
    /// A point every `length` mm; with `even_ends`, the remainder is split
    /// between both ends instead of left at the far end.
    Every {
        /// Spacing, mm.
        length: f64,
        /// Split the remainder between both ends.
        even_ends: bool,
    },
    /// One point this far along.
    At(f64),
}

/// The distances along an edge of length `len` where `division` puts points.
///
/// # Errors
/// [`ModelError::DoesNotFit`] when no point lands inside the edge.
pub fn division_points(len: f64, division: Division) -> Result<Vec<f64>, ModelError> {
    let ts: Vec<f64> = match division {
        Division::Parts(n) => (1..n).map(|k| len * f64::from(k) / f64::from(n)).collect(),
        Division::Every { length, even_ends } => {
            if length <= TOL_MERGE {
                return Err(ModelError::ZeroLength);
            }
            let whole = (len / length - 1e-9).floor();
            let first = if even_ends {
                (len - whole * length) / 2.0
            } else {
                length
            };
            let mut ts = Vec::new();
            let mut t = first;
            while t < len - TOL_MERGE {
                ts.push(t);
                t += length;
            }
            ts
        }
        Division::At(d) => vec![d],
    };
    let ts: Vec<f64> = ts
        .into_iter()
        .filter(|t| *t > TOL_MERGE && *t < len - TOL_MERGE)
        .collect();
    if ts.is_empty() {
        return Err(ModelError::DoesNotFit("division"));
    }
    Ok(ts)
}

/// Divides `edge`, measuring from its start (`from_start`) or its end.
///
/// # Errors
/// [`ModelError::NoSuchEntity`]; as [`division_points`].
pub fn divide_edge(
    entities: &mut Entities,
    edge: EdgeId,
    from_start: bool,
    division: Division,
) -> Result<(), ModelError> {
    let (a, b) = entities.edge_points(edge).ok_or(ModelError::NoSuchEntity)?;
    let (from, to) = if from_start { (a, b) } else { (b, a) };
    let len = from.distance(to);
    let dir = (to - from) / len;
    let points: Vec<DVec3> = division_points(len, division)?
        .into_iter()
        .map(|t| from + dir * t)
        .collect();
    entities.split_edge_at(edge, &points)?;
    Ok(())
}

/// What Slide moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SlideTarget {
    /// A point, along one of its edges (towards that edge's far end).
    Point {
        /// The point.
        vertex: VertexId,
        /// The edge it slides along.
        along: EdgeId,
    },
    /// An edge, along the edges at its two ends.
    Edge(EdgeId),
}

/// For an edge slide: at each end, the other edge it slides along, and the
/// unit direction away from the end (both pointing the same general way).
pub fn edge_slide_rails(entities: &Entities, e: EdgeId) -> Option<[(VertexId, DVec3); 2]> {
    let edge = entities.edge(e)?;
    let [a, b] = edge.vertices();
    let pos = |v: VertexId| entities.vertex(v).map(|x| x.position());
    let dirs = |v: VertexId| -> Vec<DVec3> {
        entities
            .vertex(v)
            .map(|x| {
                x.edges()
                    .iter()
                    .filter(|s| **s != e)
                    .filter_map(|s| {
                        let w = entities.edge(*s)?.other(v);
                        Some((pos(w)? - pos(v)?).normalize())
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let (da, db) = (dirs(a), dirs(b));
    // The pair of side edges that run most nearly the same way.
    let mut best: Option<(f64, DVec3, DVec3)> = None;
    for x in &da {
        for y in &db {
            let score = x.dot(*y);
            if best.is_none_or(|(s, _, _)| score > s) {
                best = Some((score, *x, *y));
            }
        }
    }
    let (score, x, y) = best?;
    (score > 0.5).then_some([(a, x), (b, y)])
}

/// Slides `target` by `distance` mm.
///
/// # Errors
/// [`ModelError::NoSuchEntity`]; [`ModelError::DoesNotFit`] when there is
/// nothing to slide along or the distance runs off the edge;
/// [`ModelError::NotFlat`] when a face would bend.
pub fn slide(
    entities: &mut Entities,
    target: SlideTarget,
    distance: f64,
) -> Result<(), ModelError> {
    let targets = slide_targets(entities, target, distance)?;
    entities.reposition_vertices(&targets)
}

/// Where a slide of `target` by `distance` mm puts each point it moves,
/// without moving them (the Slide tool's preview uses it).
///
/// # Errors
/// As [`slide`], apart from faces bending, which only the move itself finds.
pub fn slide_targets(
    entities: &Entities,
    target: SlideTarget,
    distance: f64,
) -> Result<Vec<(VertexId, DVec3)>, ModelError> {
    if !distance.is_finite() {
        return Err(ModelError::NonFinite);
    }
    let pos = |e: &Entities, v: VertexId| {
        e.vertex(v)
            .map(|x| x.position())
            .ok_or(ModelError::NoSuchEntity)
    };
    let targets = match target {
        SlideTarget::Point { vertex, along } => {
            let edge = entities.edge(along).ok_or(ModelError::NoSuchEntity)?;
            let far = edge.other(vertex);
            let (p, q) = (pos(entities, vertex)?, pos(entities, far)?);
            if distance <= 0.0 || distance >= p.distance(q) {
                return Err(ModelError::DoesNotFit("slide"));
            }
            vec![(vertex, p + (q - p).normalize() * distance)]
        }
        SlideTarget::Edge(e) => {
            let rails = edge_slide_rails(entities, e).ok_or(ModelError::DoesNotFit("slide"))?;
            let mut out = Vec::new();
            for (v, dir) in rails {
                out.push((v, pos(entities, v)? + dir * distance));
            }
            out
        }
    };
    Ok(targets)
}

/// Reverses each of `faces`.
///
/// # Errors
/// [`ModelError::NoSuchEntity`] if a face is gone.
pub fn reverse_faces(entities: &mut Entities, faces: &[FaceId]) -> Result<(), ModelError> {
    for f in faces {
        entities.reverse_face(*f)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use smith_geom::curve::CurveFrame;

    use super::*;

    fn square(e: &mut Entities, s: f64) -> FaceId {
        e.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(s, 0.0, 0.0),
                DVec3::new(s, s, 0.0),
                DVec3::new(0.0, s, 0.0),
            ],
            true,
        )
        .unwrap();
        e.face_ids().next().unwrap()
    }

    #[test]
    fn inset_of_a_1mm_square_by_0_2_is_0_6() {
        let mut e = Entities::new();
        let f = square(&mut e, 1.0);
        offset_face(&mut e, f, 0.2, 1).unwrap();
        assert_eq!(e.face_count(), 2);
        let inner = e
            .face_ids()
            .find(|f| (e.face_area(*f).unwrap() - 0.36).abs() < 1e-12)
            .expect("a 0.6 mm square");
        assert!(e.face(inner).is_some());
        e.validate().unwrap();
    }

    #[test]
    fn three_offsets_make_three_rings() {
        let mut e = Entities::new();
        let f = square(&mut e, 10.0);
        offset_face(&mut e, f, 0.5, 3).unwrap();
        assert_eq!(e.face_count(), 4);
        for x in [0.5, 1.0, 1.5] {
            assert!(
                e.find_vertex(DVec3::new(x, x, 0.0)).is_some(),
                "ring at {x}"
            );
        }
    }

    #[test]
    fn a_circle_offsets_as_a_circle() {
        let mut e = Entities::new();
        let frame = CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap();
        e.add_curve(Curve::Circle(Circle::new(frame, 5.0).unwrap()), 24)
            .unwrap();
        let f = e.face_ids().next().unwrap();
        offset_face(&mut e, f, 1.0, 1).unwrap();
        assert_eq!(e.curve_count(), 2);
        assert!(e.curve_ids().any(|c| matches!(
            e.curve(c).unwrap().curve(),
            Curve::Circle(c) if (c.radius() - 4.0).abs() < 1e-12
        )));
    }

    #[test]
    fn an_offset_too_large_is_refused() {
        let mut e = Entities::new();
        let f = square(&mut e, 1.0);
        assert_eq!(
            offset_face(&mut e, f, 0.6, 1),
            Err(ModelError::DoesNotFit("offset"))
        );
    }

    /// A 10 mm square face with a 2 mm square hole at its centre.
    fn plate(e: &mut Entities) -> FaceId {
        square(e, 10.0);
        e.add_polyline(
            &[
                DVec3::new(4.0, 4.0, 0.0),
                DVec3::new(6.0, 4.0, 0.0),
                DVec3::new(6.0, 6.0, 0.0),
                DVec3::new(4.0, 6.0, 0.0),
            ],
            true,
        )
        .unwrap();
        let disc = e
            .face_ids()
            .find(|f| (e.face_area(*f).unwrap() - 4.0).abs() < 1e-9)
            .unwrap();
        e.erase_face(disc).unwrap();
        e.face_ids().next().unwrap()
    }

    #[test]
    fn a_hole_offsets_into_the_face() {
        let mut e = Entities::new();
        let f = plate(&mut e);
        offset_face(&mut e, f, 1.0, 1).unwrap();
        e.validate().unwrap();
        // The outline moved in to 1..9, the hole out to 3..7.
        for p in [(1.0, 1.0), (9.0, 9.0), (3.0, 3.0), (7.0, 7.0)] {
            assert!(e.find_vertex(DVec3::new(p.0, p.1, 0.0)).is_some(), "{p:?}");
        }
        // Bands 100 - 64 and 16 - 4, and the face between 64 - 16.
        let mut areas: Vec<f64> = e.face_ids().map(|f| e.face_area(f).unwrap()).collect();
        areas.sort_by(f64::total_cmp);
        let want = [12.0, 36.0, 48.0];
        assert_eq!(areas.len(), 3);
        for (a, w) in areas.iter().zip(want) {
            assert!((a - w).abs() < 1e-9, "{areas:?}");
        }
    }

    #[test]
    fn hole_and_outline_offsets_that_meet_are_refused() {
        let mut e = Entities::new();
        let f = plate(&mut e);
        assert_eq!(
            offset_face(&mut e, f, 2.0, 1),
            Err(ModelError::DoesNotFit("offset"))
        );
        assert_eq!(offset_preview(&e, f, 2.0), Vec::<Vec<DVec3>>::new());
        assert_eq!(offset_preview(&e, f, 1.0).len(), 2);
    }

    #[test]
    fn an_open_chain_offsets_to_one_side() {
        let mut e = Entities::new();
        // An L of two edges in the ground plane.
        e.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(10.0, 0.0, 0.0),
                DVec3::new(10.0, 10.0, 0.0),
            ],
            false,
        )
        .unwrap();
        let first = e.edge_ids().next().unwrap();
        let chain = edge_chain(&e, first).unwrap();
        assert_eq!(chain.points.len(), 3);
        assert!(!chain.closed);
        // Left of travel about +Z is inside the L: a point there is positive.
        let side = chain_side(&e, first, DVec3::new(8.0, 2.0, 0.0), DVec3::Z).unwrap();
        assert!(side > 0.0);
        offset_chain(&mut e, first, 1.0, 1, DVec3::Z).unwrap();
        e.validate().unwrap();
        for p in [(0.0, 1.0), (9.0, 1.0), (9.0, 10.0)] {
            assert!(e.find_vertex(DVec3::new(p.0, p.1, 0.0)).is_some(), "{p:?}");
        }
    }

    #[test]
    fn an_arc_offsets_as_an_arc() {
        let mut e = Entities::new();
        let frame = CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap();
        let arc = smith_geom::curve::Arc::new(frame, 5.0, 0.0, std::f64::consts::PI).unwrap();
        e.add_curve(Curve::Arc(arc), 12).unwrap();
        let first = e.edge_ids().next().unwrap();
        // Outward is to the right of a counter-clockwise arc about +Z.
        offset_chain(&mut e, first, -1.0, 2, DVec3::Z).unwrap();
        let radii: Vec<f64> = e
            .curve_ids()
            .filter_map(|c| match e.curve(c).unwrap().curve() {
                Curve::Arc(a) => Some(a.radius()),
                _ => None,
            })
            .collect();
        let mut radii = radii;
        radii.sort_by(f64::total_cmp);
        assert_eq!(radii.len(), 3);
        assert!(
            (radii[1] - 6.0).abs() < 1e-9 && (radii[2] - 7.0).abs() < 1e-9,
            "{radii:?}"
        );
    }

    #[test]
    fn divide_rules() {
        assert_eq!(
            division_points(10.0, Division::Parts(4)).unwrap(),
            vec![2.5, 5.0, 7.5]
        );
        let even = division_points(
            10.0,
            Division::Every {
                length: 3.0,
                even_ends: true,
            },
        )
        .unwrap();
        for (got, want) in even.iter().zip([0.5, 3.5, 6.5, 9.5]) {
            assert!((got - want).abs() < 1e-12);
        }
        let end = division_points(
            10.0,
            Division::Every {
                length: 3.0,
                even_ends: false,
            },
        )
        .unwrap();
        assert_eq!(end, vec![3.0, 6.0, 9.0]);
        assert_eq!(division_points(10.0, Division::At(4.2)).unwrap(), vec![4.2]);
        assert!(division_points(10.0, Division::At(12.0)).is_err());
    }

    #[test]
    fn sliding_an_edge_keeps_faces_flat() {
        let mut e = Entities::new();
        square(&mut e, 10.0);
        e.add_line(DVec3::new(4.0, 0.0, 0.0), DVec3::new(4.0, 10.0, 0.0))
            .unwrap();
        let mid = e
            .edge_between(
                e.find_vertex(DVec3::new(4.0, 0.0, 0.0)).unwrap(),
                e.find_vertex(DVec3::new(4.0, 10.0, 0.0)).unwrap(),
            )
            .unwrap();
        slide(&mut e, SlideTarget::Edge(mid), 2.0).unwrap();
        let moved = e.find_vertex(DVec3::new(6.0, 0.0, 0.0)).is_some()
            || e.find_vertex(DVec3::new(2.0, 0.0, 0.0)).is_some();
        assert!(moved, "the edge slid 2 mm along the sides");
        e.validate().unwrap();
    }

    #[test]
    fn dividing_a_10mm_edge_in_four() {
        let mut e = Entities::new();
        square(&mut e, 10.0);
        let edge = e
            .edge_between(
                e.find_vertex(DVec3::ZERO).unwrap(),
                e.find_vertex(DVec3::new(10.0, 0.0, 0.0)).unwrap(),
            )
            .unwrap();
        divide_edge(&mut e, edge, true, Division::Parts(4)).unwrap();
        assert_eq!(e.edge_count(), 7);
        e.validate().unwrap();
    }
}
