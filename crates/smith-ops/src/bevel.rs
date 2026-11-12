//! SketchSmith — Bevel | v1.0.0 | Thursday 12 November 2026
//!
//! Bevels straight edges between two flat faces. Width is how far the
//! bevel cuts back along each face from the edge; Segments 1 makes a flat
//! chamfer, more a round fillet (an arc tangent to both faces, the radius
//! following from the width and the faces' angle). Each end of a bevelled
//! edge must be a corner of exactly three edges (the edge itself and one
//! along each face), and the end faces there get the bevel's outline cut
//! into them. Two bevelled edges meeting at a corner are refused, naming
//! the corner: blending such corners comes later. A curved edge is refused
//! for now. Sides of the bevel that cannot stay flat (the two ends'
//! outlines not lining up) are refused unless Auto-Tri folds them.
//!
//! **Corners** ([`round_corners`]): a corner point of a flat outline (two
//! edges meeting) is rounded or cut by Radius instead: the cut-back along
//! each edge follows from the corner's angle. A face the outline bounds
//! keeps the new corner.

use smith_geom::DVec3;
use smith_model::{EdgeId, Entities, FaceId, ModelError, VertexId};

use crate::modify::orient;

/// One end of an edge being bevelled: the corner, and its edges along the
/// first and the second face.
struct End {
    at: DVec3,
    along1: DVec3,
    along2: DVec3,
    len1: f64,
    len2: f64,
}

/// A corner, named for messages: its position, mm.
fn named(p: DVec3) -> String {
    format!("{:.2}, {:.2}, {:.2}", p.x, p.y, p.z)
}

/// The edges at vertex `v` other than `e`, sorted to faces `f1` and `f2`.
fn end_of(
    entities: &Entities,
    e: EdgeId,
    v: VertexId,
    f1: FaceId,
    f2: FaceId,
) -> Result<End, ModelError> {
    let at = entities
        .vertex(v)
        .ok_or(ModelError::NoSuchEntity)?
        .position();
    let others: Vec<EdgeId> = entities
        .vertex(v)
        .ok_or(ModelError::NoSuchEntity)?
        .edges()
        .iter()
        .copied()
        .filter(|x| *x != e)
        .collect();
    let corner_error = || ModelError::Refused(format!("Corner at {} Needs Three Edges", named(at)));
    if others.len() != 2 {
        return Err(corner_error());
    }
    let on = |f: FaceId| {
        others
            .iter()
            .copied()
            .find(|x| entities.edge(*x).is_some_and(|y| y.faces().contains(&f)))
    };
    let (Some(e1), Some(e2)) = (on(f1), on(f2)) else {
        return Err(corner_error());
    };
    let far = |x: EdgeId| -> Result<DVec3, ModelError> {
        let other = entities.edge(x).ok_or(ModelError::NoSuchEntity)?.other(v);
        Ok(entities
            .vertex(other)
            .ok_or(ModelError::NoSuchEntity)?
            .position())
    };
    let (p1, p2) = (far(e1)?, far(e2)?);
    Ok(End {
        at,
        along1: (p1 - at).normalize_or_zero(),
        along2: (p2 - at).normalize_or_zero(),
        len1: p1.distance(at),
        len2: p2.distance(at),
    })
}

/// The bevel's outline at one end, from the first face's side to the
/// second's: a straight cut, or an arc tangent to both edges.
fn outline(end: &End, width: f64, segments: u32) -> Vec<DVec3> {
    let p1 = end.at + end.along1 * width;
    let p2 = end.at + end.along2 * width;
    if segments <= 1 {
        return vec![p1, p2];
    }
    // The arc's centre lies on the corner's bisector.
    let theta = end.along1.angle_between(end.along2);
    let bisector = (end.along1 + end.along2).normalize_or_zero();
    let centre = end.at + bisector * (width / (theta * 0.5).cos());
    let (v1, v2) = (p1 - centre, p2 - centre);
    let axis = v1.cross(v2).normalize_or_zero();
    let sweep = v1.angle_between(v2);
    (0..=segments)
        .map(|i| {
            let a = sweep * f64::from(i) / f64::from(segments);
            centre + smith_geom::DMat4::from_axis_angle(axis, a).transform_vector3(v1)
        })
        .collect()
}

/// Bevels the straight edges running between the point pairs `edges`
/// (each between two flat faces) by `width`, mm, in `segments` facets.
///
/// # Errors
/// [`ModelError::Refused`] naming a corner two bevelled edges share, a
/// corner without exactly three edges, or a width that reaches past the
/// next corner; for a curved edge or an edge not between two faces.
/// [`ModelError::NotFlat`] for bevel sides that bend, without Auto-Tri.
/// [`ModelError::InvalidData`] for a width that is not above zero.
pub fn bevel(
    entities: &mut Entities,
    edges: &[(DVec3, DVec3)],
    width: f64,
    segments: u32,
) -> Result<(), ModelError> {
    if !(width > 0.0 && width.is_finite()) {
        return Err(ModelError::InvalidData("bevel width".into()));
    }
    let segments = segments.clamp(1, 64);
    // Shared corners first, before anything changes.
    for (i, a) in edges.iter().enumerate() {
        for b in &edges[i + 1..] {
            for p in [a.0, a.1] {
                if p.distance(b.0) < 1e-9 || p.distance(b.1) < 1e-9 {
                    return Err(ModelError::Refused(format!(
                        "Two Bevelled Edges Meet at {}",
                        named(p)
                    )));
                }
            }
        }
    }
    for (a, b) in edges {
        bevel_one(entities, *a, *b, width, segments)?;
    }
    Ok(())
}

/// Bevels the one edge from `a` to `b`.
fn bevel_one(
    entities: &mut Entities,
    a: DVec3,
    b: DVec3,
    width: f64,
    segments: u32,
) -> Result<(), ModelError> {
    let (va, vb) = (
        entities.find_vertex(a).ok_or(ModelError::NoSuchEntity)?,
        entities.find_vertex(b).ok_or(ModelError::NoSuchEntity)?,
    );
    let e = entities
        .edge_between(va, vb)
        .ok_or(ModelError::NoSuchEntity)?;
    let edge = entities.edge(e).ok_or(ModelError::NoSuchEntity)?;
    if edge.curve().is_some() {
        return Err(ModelError::Refused(
            "Curved Edges Can't Be Bevelled Yet".into(),
        ));
    }
    let &[f1, f2] = edge.faces() else {
        return Err(ModelError::Refused(
            "Bevel an Edge Between Two Faces".into(),
        ));
    };
    let (n1, n2) = (
        entities
            .face(f1)
            .ok_or(ModelError::NoSuchEntity)?
            .plane()
            .normal(),
        entities
            .face(f2)
            .ok_or(ModelError::NoSuchEntity)?
            .plane()
            .normal(),
    );
    if n1.cross(n2).length() < 1e-9 {
        return Err(ModelError::Refused(
            "The Faces Here Are Flat Across the Edge".into(),
        ));
    }
    let (start, end) = (
        end_of(entities, e, va, f1, f2)?,
        end_of(entities, e, vb, f1, f2)?,
    );
    for x in [&start, &end] {
        if width >= x.len1 - 1e-9 || width >= x.len2 - 1e-9 {
            return Err(ModelError::Refused(format!(
                "Width Reaches Past the Corner at {}",
                named(x.at)
            )));
        }
    }
    let (ring_a, ring_b) = (
        outline(&start, width, segments),
        outline(&end, width, segments),
    );
    // The bevel's sides, from the first face round to the second.
    let quads: Vec<[DVec3; 4]> = (0..ring_a.len() - 1)
        .map(|i| [ring_a[i], ring_a[i + 1], ring_b[i + 1], ring_b[i]])
        .collect();
    let flat = |q: &[DVec3; 4]| {
        smith_geom::plane::Plane::fit_loop(q).is_ok_and(|p| p.max_deviation(q) <= 1e-6)
    };
    let bent: Vec<usize> = (0..quads.len()).filter(|i| !flat(&quads[*i])).collect();
    if !bent.is_empty() && !smith_model::auto_tri() {
        return Err(ModelError::NotFlat(Vec::new()));
    }
    // Cut lines along both faces and the outlines across the end faces,
    // then erase the edge and the corners' short pieces: what was cut off
    // goes with them.
    let last = segments as usize;
    entities.add_line(ring_a[0], ring_b[0])?;
    entities.add_line(ring_a[last], ring_b[last])?;
    entities.add_polyline(&ring_a, false)?;
    entities.add_polyline(&ring_b, false)?;
    for (corner, p1, p2) in [(a, ring_a[0], ring_a[last]), (b, ring_b[0], ring_b[last])] {
        for p in [p1, p2] {
            if let (Some(x), Some(y)) = (entities.find_vertex(corner), entities.find_vertex(p))
                && let Some(piece) = entities.edge_between(x, y)
            {
                entities.erase_edge(piece)?;
            }
        }
    }
    if let (Some(x), Some(y)) = (entities.find_vertex(a), entities.find_vertex(b))
        && let Some(edge) = entities.edge_between(x, y)
    {
        entities.erase_edge(edge)?;
    }
    // The bevel's own faces: the lines between the outlines, and the
    // folds' soft diagonals.
    for i in 1..last {
        entities.add_line(ring_a[i], ring_b[i])?;
    }
    let mut diagonals = Vec::new();
    for i in &bent {
        let [p, _, r, _] = quads[*i];
        diagonals.extend(entities.add_line(p, r)?);
    }
    entities.soften_edges(&diagonals);
    let out = (n1 + n2).normalize_or_zero();
    for (i, q) in quads.iter().enumerate() {
        let parts: Vec<Vec<DVec3>> = if bent.contains(&i) {
            vec![vec![q[0], q[1], q[2]], vec![q[0], q[2], q[3]]]
        } else {
            vec![q.to_vec()]
        };
        for part in parts {
            if let Some(f) = crate::modify::face_through(entities, &part) {
                orient(entities, f, out)?;
            }
        }
    }
    Ok(())
}

/// Rounds (or with `segments` 1, cuts) the corners at `corners`: points
/// where exactly two edges of a flat outline meet, by `radius`, mm.
///
/// # Errors
/// [`ModelError::Refused`] naming a corner without exactly two edges, two
/// edges in line, or a radius whose cut-back reaches past an edge's far
/// end; [`ModelError::InvalidData`] for a radius not above zero.
pub fn round_corners(
    entities: &mut Entities,
    corners: &[DVec3],
    radius: f64,
    segments: u32,
) -> Result<(), ModelError> {
    if !(radius > 0.0 && radius.is_finite()) {
        return Err(ModelError::InvalidData("corner radius".into()));
    }
    let segments = segments.clamp(1, 64);
    for at in corners {
        let v = entities.find_vertex(*at).ok_or(ModelError::NoSuchEntity)?;
        let edges: Vec<EdgeId> = entities
            .vertex(v)
            .ok_or(ModelError::NoSuchEntity)?
            .edges()
            .to_vec();
        let [e1, e2] = edges.as_slice() else {
            return Err(ModelError::Refused(format!(
                "Corner at {} Needs Two Edges",
                named(*at)
            )));
        };
        let far = |x: EdgeId| -> Result<DVec3, ModelError> {
            let other = entities.edge(x).ok_or(ModelError::NoSuchEntity)?.other(v);
            Ok(entities
                .vertex(other)
                .ok_or(ModelError::NoSuchEntity)?
                .position())
        };
        let (p1, p2) = (far(*e1)?, far(*e2)?);
        let end = End {
            at: *at,
            along1: (p1 - *at).normalize_or_zero(),
            along2: (p2 - *at).normalize_or_zero(),
            len1: p1.distance(*at),
            len2: p2.distance(*at),
        };
        let theta = end.along1.angle_between(end.along2);
        if !(1e-6..std::f64::consts::PI - 1e-6).contains(&theta) {
            return Err(ModelError::Refused(format!(
                "The Edges at {} Run in Line",
                named(*at)
            )));
        }
        // The cut-back along each edge for a round of this radius.
        let width = radius / (theta * 0.5).tan();
        if width >= end.len1 - 1e-9 || width >= end.len2 - 1e-9 {
            return Err(ModelError::Refused(format!(
                "Radius Reaches Past the Next Corner at {}",
                named(*at)
            )));
        }
        let ring = outline(&end, width, segments);
        entities.add_polyline(&ring, false)?;
        // The corner's two short pieces go, and the corner with them.
        for p in [ring[0], ring[ring.len() - 1]] {
            if let (Some(x), Some(y)) = (entities.find_vertex(*at), entities.find_vertex(p))
                && let Some(piece) = entities.edge_between(x, y)
            {
                entities.erase_edge(piece)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_model::ShapeSpec;

    /// A 10 mm cube from the origin.
    fn cube() -> Entities {
        let mut e = Entities::new();
        crate::shapes::build_shape(
            &mut e,
            ShapeSpec::Box {
                width: 10.0,
                depth: 10.0,
                height: 10.0,
            },
        )
        .unwrap();
        e
    }

    /// An edge of the cube along Z (its two ends).
    fn vertical_edge(e: &Entities) -> (DVec3, DVec3) {
        let (mut lo, mut hi) = (DVec3::ZERO, DVec3::ZERO);
        let mut best = f64::MAX;
        for id in e.edge_ids() {
            let (p, q) = e.edge_points(id).unwrap();
            if (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9 {
                let key = p.x + p.y;
                if key < best {
                    best = key;
                    (lo, hi) = (p, q);
                }
            }
        }
        (lo, hi)
    }

    fn closed_solid(e: &Entities) -> bool {
        e.edge_ids()
            .all(|x| e.edge(x).is_some_and(|y| y.faces().len() == 2))
    }

    #[test]
    fn a_chamfer_cuts_one_edge_flat() {
        let mut e = cube();
        let edge = vertical_edge(&e);
        bevel(&mut e, &[edge], 2.0, 1).unwrap();
        assert_eq!(e.face_count(), 7);
        assert!(closed_solid(&e));
        assert!(e.find_vertex(edge.0).is_none());
    }

    #[test]
    fn a_fillet_rounds_one_edge_in_segments() {
        let mut e = cube();
        let edge = vertical_edge(&e);
        bevel(&mut e, &[edge], 2.0, 4).unwrap();
        assert_eq!(e.face_count(), 6 + 4);
        assert!(closed_solid(&e));
    }

    #[test]
    fn edges_sharing_a_corner_are_refused_and_named() {
        let mut e = cube();
        let a = vertical_edge(&e);
        // Another edge from the same bottom corner.
        let other = e
            .edge_ids()
            .filter_map(|x| e.edge_points(x))
            .find(|(p, q)| {
                (p.distance(a.0) < 1e-9 || q.distance(a.0) < 1e-9)
                    && !(p.distance(a.1) < 1e-9 || q.distance(a.1) < 1e-9)
            })
            .unwrap();
        let out = bevel(&mut e, &[a, other], 1.0, 1);
        let Err(ModelError::Refused(msg)) = out else {
            panic!("not refused: {out:?}");
        };
        assert!(msg.contains("Meet at"), "{msg}");
        // Nothing changed.
        assert_eq!(e.face_count(), 6);
    }

    #[test]
    fn a_width_past_the_next_corner_is_refused() {
        let mut e = cube();
        let edge = vertical_edge(&e);
        assert!(matches!(
            bevel(&mut e, &[edge], 12.0, 1),
            Err(ModelError::Refused(_))
        ));
    }

    #[test]
    fn a_square_corner_rounds_by_radius_and_keeps_its_face() {
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
        round_corners(&mut e, &[DVec3::ZERO], 2.0, 4).unwrap();
        assert_eq!(e.face_count(), 1);
        assert!(e.find_vertex(DVec3::ZERO).is_none());
        // On a square corner the cut-back equals the radius.
        assert!(e.find_vertex(DVec3::new(2.0, 0.0, 0.0)).is_some());
        assert!(e.find_vertex(DVec3::new(0.0, 2.0, 0.0)).is_some());
        assert_eq!(e.edge_count(), 4 + 4);
        // Too big a radius is refused.
        assert!(matches!(
            round_corners(&mut e, &[DVec3::new(10.0, 0.0, 0.0)], 20.0, 1),
            Err(ModelError::Refused(_))
        ));
    }
}
