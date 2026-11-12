//! SketchSmith — Weld | v1.0.0 | Thursday 12 November 2026
//!
//! Welds a chain of connected edges into one curve (a polyline curve), so
//! Edit Curve and the path tools treat it as one. Vertices sets how many
//! points the curve has: never fewer than the chain has, and any more are
//! added along its edges, shared out by length and never moved, so the
//! shape stays exactly as it was. Curves the chain ran through (an arc, a
//! circle) become part of the polyline; faces the chain bounds survive.

use smith_geom::DVec3;
use smith_geom::curve::{Curve, Polyline};
use smith_model::{Entities, ModelError};

/// The chain `points` (closed or not) with points added along its edges,
/// none moved, up to `k` in all, shared out by edge length.
pub fn add_points(points: &[DVec3], closed: bool, k: usize) -> Vec<DVec3> {
    let m = points.len();
    if k <= m || m < 2 {
        return points.to_vec();
    }
    let spans = if closed { m } else { m - 1 };
    let lens: Vec<f64> = (0..spans)
        .map(|i| points[i].distance(points[(i + 1) % m]))
        .collect();
    let total: f64 = lens.iter().sum::<f64>().max(1e-12);
    let extra = k - m;
    let exact: Vec<f64> = lens.iter().map(|l| extra as f64 * l / total).collect();
    let mut adds: Vec<usize> = exact.iter().map(|x| x.floor() as usize).collect();
    let mut left = extra - adds.iter().sum::<usize>();
    let mut order: Vec<usize> = (0..spans).collect();
    order.sort_by(|a, b| {
        (exact[*b] - exact[*b].floor())
            .total_cmp(&(exact[*a] - exact[*a].floor()))
            .then(a.cmp(b))
    });
    for i in order {
        if left == 0 {
            break;
        }
        adds[i] += 1;
        left -= 1;
    }
    let mut out = Vec::with_capacity(k);
    for i in 0..spans {
        let (a, b) = (points[i], points[(i + 1) % m]);
        out.push(a);
        for j in 1..=adds[i] {
            out.push(a.lerp(b, j as f64 / (adds[i] + 1) as f64));
        }
    }
    if !closed {
        out.push(points[m - 1]);
    }
    out
}

/// Welds the chain through `points` (closed or not) into one polyline
/// curve with at least `vertices` points.
///
/// # Errors
/// [`ModelError::Geometry`] for a chain too short to be a curve; as
/// [`Entities::add_curve`].
pub fn weld(
    entities: &mut Entities,
    points: &[DVec3],
    closed: bool,
    vertices: u32,
) -> Result<(), ModelError> {
    // Curves the chain runs through become plain edges first.
    let m = points.len();
    let spans = if closed { m } else { m.saturating_sub(1) };
    for i in 0..spans {
        let (a, b) = (points[i], points[(i + 1) % m]);
        let (Some(va), Some(vb)) = (entities.find_vertex(a), entities.find_vertex(b)) else {
            continue;
        };
        if let Some(c) = entities
            .edge_between(va, vb)
            .and_then(|e| entities.edge(e))
            .and_then(|e| e.curve())
        {
            entities.explode_curve(c)?;
        }
    }
    let pts = add_points(points, closed, vertices as usize);
    let n = u32::try_from(pts.len()).unwrap_or(u32::MAX);
    let segments = if closed { n } else { n.saturating_sub(1) };
    let line = Polyline::new(pts, closed)?;
    entities.add_curve(Curve::Polyline(line), segments)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_points_keep_the_chain_and_its_ends() {
        let pts = [
            DVec3::ZERO,
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 2.0, 0.0),
        ];
        let out = add_points(&pts, false, 6);
        assert_eq!(out.len(), 6);
        assert_eq!(out[0], pts[0]);
        assert_eq!(out[5], pts[2]);
        assert!(out.contains(&pts[1]));
    }

    #[test]
    fn a_chain_becomes_one_curve_and_its_face_survives() {
        let mut e = Entities::new();
        let sq = [
            DVec3::ZERO,
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 4.0, 0.0),
            DVec3::new(0.0, 4.0, 0.0),
        ];
        e.add_polyline(&sq, true).unwrap();
        assert_eq!(e.face_count(), 1);
        weld(&mut e, &sq, true, 8).unwrap();
        assert_eq!(e.face_count(), 1);
        assert_eq!(e.curve_count(), 1);
        let c = e.curve_ids().next().unwrap();
        assert_eq!(e.curve(c).unwrap().edges().len(), 8);
        assert_eq!(e.edge_count(), 8);
    }
}
