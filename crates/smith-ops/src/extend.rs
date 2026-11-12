//! SketchSmith — Trim / Extend, Bisector, Extend to Face and Bridge | v1.0.0 | Thursday 12 November 2026
//!
//! * **Trim / Extend** ([`trim_extend`]): two edges meet where their lines
//!   cross; each edge's end nearer that point moves to it, lengthening an
//!   edge that falls short or trimming one that overshoots.
//! * **Bisector** ([`bisector`]): the guide line through two edges' crossing
//!   that splits the angle between them.
//! * **Extend to Face** ([`extend_to_face`]): an edge's end moves along the
//!   edge's line to where it meets a face's plane.
//! * **Bridge** ([`bridge`]): two open loops (holes' rims) are joined by a
//!   band of faces, part of the model. Vertices and Segments work as in
//!   Loft: points added along the loops, never moved; Segments the rings
//!   across the band. Bent strips need Auto-Tri.

use smith_geom::DVec3;
use smith_model::{Entities, Guide, ModelError, VertexId};

/// Where the lines through `a` and `b` (each a pair of points) meet: the
/// point, and how far apart they pass (0 when they truly cross).
fn crossing(a: (DVec3, DVec3), b: (DVec3, DVec3)) -> Option<(DVec3, f64)> {
    let (d1, d2) = (a.1 - a.0, b.1 - b.0);
    let r = a.0 - b.0;
    let (aa, bb, ab) = (d1.dot(d1), d2.dot(d2), d1.dot(d2));
    let denom = aa * bb - ab * ab;
    if denom.abs() < 1e-12 * aa * bb {
        return None;
    }
    let (c, f) = (d1.dot(r), d2.dot(r));
    let s = (ab * f - bb * c) / denom;
    let t = (aa * f - ab * c) / denom;
    let (p, q) = (a.0 + d1 * s, b.0 + d2 * t);
    Some(((p + q) * 0.5, p.distance(q)))
}

/// The crossing of two edges' lines, refused with a message when they are
/// in line or pass each other without meeting.
fn meeting(a: (DVec3, DVec3), b: (DVec3, DVec3)) -> Result<DVec3, ModelError> {
    match crossing(a, b) {
        None => Err(ModelError::Refused("The Edges Run in Line".into())),
        Some((_, gap)) if gap > 1e-6 => {
            Err(ModelError::Refused("The Edges' Lines Don't Meet".into()))
        }
        Some((x, _)) => Ok(x),
    }
}

/// The end of `edge` nearer `x`, as its vertex.
fn nearer_end(entities: &Entities, edge: (DVec3, DVec3), x: DVec3) -> Result<VertexId, ModelError> {
    let p = if edge.0.distance(x) <= edge.1.distance(x) {
        edge.0
    } else {
        edge.1
    };
    entities.find_vertex(p).ok_or(ModelError::NoSuchEntity)
}

/// Trims or extends both edges to where their lines meet.
///
/// # Errors
/// [`ModelError::Refused`] when the lines are in line or don't meet; as
/// [`Entities::reposition_vertices`].
pub fn trim_extend(
    entities: &mut Entities,
    a: (DVec3, DVec3),
    b: (DVec3, DVec3),
) -> Result<(), ModelError> {
    let x = meeting(a, b)?;
    let (va, vb) = (nearer_end(entities, a, x)?, nearer_end(entities, b, x)?);
    let at = |v: VertexId| entities.vertex(v).map(|p| p.position());
    // An end already there stays; the two ends meet in one point.
    let mut moves = Vec::new();
    if at(va).is_some_and(|p| p.distance(x) > 1e-9) {
        moves.push((va, x));
    }
    if va != vb && at(vb).is_some_and(|p| p.distance(x) > 1e-9) {
        if moves.is_empty() {
            moves.push((vb, x));
        } else {
            // Both move to one point: move the first, then join the second's
            // edge to it.
            entities.reposition_vertices(&moves)?;
            let far = if b.0.distance(x) <= b.1.distance(x) {
                b.1
            } else {
                b.0
            };
            let vf = entities.find_vertex(far).ok_or(ModelError::NoSuchEntity)?;
            if let Some(old) = entities.edge_between(vf, vb) {
                entities.erase_edge(old)?;
            }
            entities.add_line(far, x)?;
            return Ok(());
        }
    }
    if moves.is_empty() {
        return Ok(());
    }
    entities.reposition_vertices(&moves)
}

/// The guide line splitting the angle between two edges, through their
/// crossing, pointing between their far ends.
///
/// # Errors
/// [`ModelError::Refused`] when the edges are parallel or their lines
/// don't meet.
pub fn bisector(a: (DVec3, DVec3), b: (DVec3, DVec3)) -> Result<Guide, ModelError> {
    let x = meeting(a, b)?;
    let away = |e: (DVec3, DVec3)| {
        let far = if e.0.distance(x) >= e.1.distance(x) {
            e.0
        } else {
            e.1
        };
        (far - x).normalize_or_zero()
    };
    let direction = (away(a) + away(b))
        .try_normalize()
        .ok_or_else(|| ModelError::Refused("The Edges Point Opposite Ways".into()))?;
    Ok(Guide::Line {
        point: x,
        direction,
    })
}

/// Moves the end of `edge` nearer `end` along the edge's line to the plane
/// through `point` with normal `normal`.
///
/// # Errors
/// [`ModelError::Refused`] when the edge runs parallel to the plane; as
/// [`Entities::reposition_vertices`].
pub fn extend_to_face(
    entities: &mut Entities,
    edge: (DVec3, DVec3),
    end: DVec3,
    point: DVec3,
    normal: DVec3,
) -> Result<(), ModelError> {
    let (moving, fixed) = if edge.0.distance(end) <= edge.1.distance(end) {
        (edge.0, edge.1)
    } else {
        (edge.1, edge.0)
    };
    let d = moving - fixed;
    let denom = d.dot(normal);
    if denom.abs() < 1e-12 * d.length().max(1.0) {
        return Err(ModelError::Refused(
            "The Edge Runs Parallel to the Face".into(),
        ));
    }
    let t = (point - fixed).dot(normal) / denom;
    let hit = fixed + d * t;
    if hit.distance(moving) < 1e-9 {
        return Ok(());
    }
    let v = entities
        .find_vertex(moving)
        .ok_or(ModelError::NoSuchEntity)?;
    entities.reposition_vertices(&[(v, hit)])
}

/// Joins two open loops (each a closed chain of points) with a band of
/// faces: points added along each loop up to `vertices` (or the most
/// either has), paired from the nearest, `segments` rings across.
///
/// # Errors
/// [`ModelError::Refused`] for loops sharing a point or too short;
/// [`ModelError::NotFlat`] for strips that bend, without Auto-Tri.
pub fn bridge(
    entities: &mut Entities,
    a: &[DVec3],
    b: &[DVec3],
    vertices: u32,
    segments: u32,
) -> Result<(), ModelError> {
    if a.len() < 3 || b.len() < 3 {
        return Err(ModelError::Refused("Pick Two Closed Loops".into()));
    }
    if a.iter().any(|p| b.iter().any(|q| p.distance(*q) < 1e-9)) {
        return Err(ModelError::Refused("The Loops Share a Point".into()));
    }
    let k = (vertices as usize).max(a.len()).max(b.len());
    let ra = crate::weld::add_points(a, true, k);
    let mut rb = crate::weld::add_points(b, true, k);
    // Run the same way round, starting from the pair that sits nearest.
    let cost =
        |x: &[DVec3]| -> f64 { x.iter().zip(&ra).map(|(p, q)| p.distance_squared(*q)).sum() };
    let mut best = (f64::MAX, rb.clone());
    for flip in [false, true] {
        let mut base = rb.clone();
        if flip {
            base.reverse();
        }
        for turn in 0..k {
            let mut t = base.clone();
            t.rotate_left(turn);
            let c = cost(&t);
            if c < best.0 {
                best = (c, t);
            }
        }
    }
    rb = best.1;
    let rings: Vec<Vec<DVec3>> = (0..=segments + 1)
        .map(|j| {
            let f = f64::from(j) / f64::from(segments + 1);
            ra.iter().zip(&rb).map(|(p, q)| p.lerp(*q, f)).collect()
        })
        .collect();
    // The strips, checked before anything changes.
    let mut quads = Vec::new();
    for j in 0..rings.len() - 1 {
        for i in 0..k {
            quads.push([
                rings[j][i],
                rings[j][(i + 1) % k],
                rings[j + 1][(i + 1) % k],
                rings[j + 1][i],
            ]);
        }
    }
    let flat = |q: &[DVec3; 4]| {
        smith_geom::plane::Plane::fit_loop(q).is_ok_and(|p| p.max_deviation(q) <= 1e-6)
    };
    let bent: Vec<usize> = (0..quads.len()).filter(|i| !flat(&quads[*i])).collect();
    if !bent.is_empty() && !smith_model::auto_tri() {
        return Err(ModelError::NotFlat(Vec::new()));
    }
    // The loops' added points, the rings between, and the lines across.
    entities.add_polyline(&ra, true)?;
    entities.add_polyline(&rb, true)?;
    for r in &rings[1..rings.len() - 1] {
        entities.add_polyline(r, true)?;
    }
    for i in 0..k {
        let line: Vec<DVec3> = rings.iter().map(|r| r[i]).collect();
        entities.add_polyline(&line, false)?;
    }
    let mut diagonals = Vec::new();
    for i in &bent {
        let [p, _, r, _] = quads[*i];
        diagonals.extend(entities.add_line(p, r)?);
    }
    entities.soften_edges(&diagonals);
    // The rings between make their own flat discs; those are not the band.
    for r in &rings[1..rings.len() - 1] {
        if let Some(f) = crate::modify::face_through(entities, r) {
            entities.erase_face(f)?;
        }
    }
    // Face the band the way of what it joins.
    if let Some(f) = quads
        .iter()
        .find_map(|q| crate::modify::face_through(entities, q))
    {
        entities.orient_faces_from(f)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_short_edges_extend_to_meet_at_their_corner() {
        let mut e = Entities::new();
        e.add_line(DVec3::new(0.0, 0.0, 0.0), DVec3::new(8.0, 0.0, 0.0))
            .unwrap();
        e.add_line(DVec3::new(10.0, 2.0, 0.0), DVec3::new(10.0, 9.0, 0.0))
            .unwrap();
        trim_extend(
            &mut e,
            (DVec3::ZERO, DVec3::new(8.0, 0.0, 0.0)),
            (DVec3::new(10.0, 2.0, 0.0), DVec3::new(10.0, 9.0, 0.0)),
        )
        .unwrap();
        let corner = e.find_vertex(DVec3::new(10.0, 0.0, 0.0)).unwrap();
        assert_eq!(e.vertex(corner).unwrap().edges().len(), 2);
    }

    #[test]
    fn crossing_edges_trim_their_overshoot() {
        let mut e = Entities::new();
        e.add_line(DVec3::new(0.0, 0.0, 0.0), DVec3::new(12.0, 0.0, 0.0))
            .unwrap();
        e.add_line(DVec3::new(10.0, -3.0, 5.0), DVec3::new(10.0, 9.0, 5.0))
            .unwrap();
        // Skew lines don't meet.
        assert!(matches!(
            trim_extend(
                &mut e,
                (DVec3::ZERO, DVec3::new(12.0, 0.0, 0.0)),
                (DVec3::new(10.0, -3.0, 5.0), DVec3::new(10.0, 9.0, 5.0)),
            ),
            Err(ModelError::Refused(_))
        ));
    }

    #[test]
    fn a_bisector_splits_a_right_angle() {
        let g = bisector(
            (DVec3::new(2.0, 0.0, 0.0), DVec3::new(9.0, 0.0, 0.0)),
            (DVec3::new(0.0, 2.0, 0.0), DVec3::new(0.0, 9.0, 0.0)),
        )
        .unwrap();
        let Guide::Line { point, direction } = g else {
            panic!("not a line");
        };
        assert!(point.length() < 1e-9);
        assert!((direction - DVec3::new(1.0, 1.0, 0.0).normalize()).length() < 1e-9);
    }

    #[test]
    fn an_edge_extends_to_a_sloped_plane() {
        let mut e = Entities::new();
        e.add_line(DVec3::ZERO, DVec3::new(0.0, 0.0, 5.0)).unwrap();
        extend_to_face(
            &mut e,
            (DVec3::ZERO, DVec3::new(0.0, 0.0, 5.0)),
            DVec3::new(0.0, 0.0, 5.0),
            DVec3::new(0.0, 0.0, 10.0),
            DVec3::new(0.0, 1.0, 1.0).normalize(),
        )
        .unwrap();
        assert!(e.find_vertex(DVec3::new(0.0, 0.0, 10.0)).is_some());
    }

    #[test]
    fn two_square_holes_bridge_into_a_tube() {
        let sq = |z: f64, r: f64| {
            vec![
                DVec3::new(-r, -r, z),
                DVec3::new(r, -r, z),
                DVec3::new(r, r, z),
                DVec3::new(-r, r, z),
            ]
        };
        let mut e = Entities::new();
        e.add_polyline(&sq(0.0, 1.0), true).unwrap();
        e.add_polyline(&sq(10.0, 1.0), true).unwrap();
        // Drawn loops fill themselves; open them as holes would be.
        let discs: Vec<_> = e.face_ids().collect();
        for f in discs {
            e.erase_face(f).unwrap();
        }
        bridge(&mut e, &sq(0.0, 1.0), &sq(10.0, 1.0), 0, 1).unwrap();
        assert_eq!(e.face_count(), 4 * 2);
    }
}
