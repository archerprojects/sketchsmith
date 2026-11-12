//! SketchSmith — Tweak tools | v1.0.0 | Thursday 12 November 2026
//!
//! Tools that move the points of a chain, a loop or a selection without
//! adding or removing any (Blender's LoopTools, for hard modeling):
//!
//! * **Space** evens out the spacing along a chain; points slide along its
//!   own edges, so the faces beside it stay flat. Points where the chain
//!   turns sharper than Corner Angle stay put, and so do an open chain's
//!   ends.
//! * **Circle** moves a closed loop's points onto the best-fit circle in the
//!   loop's plane, evenly spaced, the first staying nearest its old place;
//!   Radius starts at the fitted one.
//! * **Flatten** moves every point of a selection onto one plane through its
//!   centre: the best fit, or square to X, Y or Z.
//! * **Smooth** pulls each point toward the middle of its neighbours;
//!   **Relax** also evens their spacing as it smooths. Both keep the
//!   chain's ends and its sharp corners.
//!
//! Influence (0 to 1) sets how far each point goes toward its new place.
//! Faces beside the moved points that would bend go through the Auto-Tri
//! refusal and offer.

use std::collections::HashMap;

use smith_geom::DVec3;
use smith_model::{Entities, ModelError, VertexId};

/// Which tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TweakKind {
    /// Even spacing along the chain.
    Space,
    /// Onto a circle.
    Circle,
    /// Onto one plane.
    Flatten,
    /// Toward the neighbours' middle.
    Smooth,
    /// Smooth and even out.
    Relax,
}

/// Flatten's plane: the best fit, or square to an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlatPlane {
    /// The plane that fits the points best.
    BestFit,
    /// Square to X.
    X,
    /// Square to Y.
    Y,
    /// Square to Z.
    Z,
}

/// One tweak: what it works on and its values.
#[derive(Debug, Clone, PartialEq)]
pub struct Tweak {
    /// Which tool.
    pub kind: TweakKind,
    /// The chain's points in order (Space, Circle, Smooth, Relax), or the
    /// selection's points (Flatten).
    pub points: Vec<DVec3>,
    /// The chain closes on itself.
    pub closed: bool,
    /// Flatten's best fit when faces were picked: their area-weighted
    /// normal (else the points' own fit).
    pub normal: Option<DVec3>,
    /// Space, Smooth, Relax: turns sharper than this stay put (radians).
    pub corner: f64,
    /// Circle: the radius (`None`: the fitted one).
    pub radius: Option<f64>,
    /// Flatten: the plane.
    pub plane: FlatPlane,
    /// Smooth, Relax: passes.
    pub iterations: u32,
    /// How far each point moves toward its new place, 0 to 1.
    pub influence: f64,
}

/// The chain's turn at each point (0 at an open chain's ends).
fn turns(p: &[DVec3], closed: bool) -> Vec<f64> {
    let n = p.len();
    (0..n)
        .map(|i| {
            if !closed && (i == 0 || i + 1 == n) {
                return 0.0;
            }
            let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
            (b - a).angle_between(c - b)
        })
        .collect()
}

/// The points that stay: an open chain's ends and turns sharper than
/// `corner`.
fn fixed(p: &[DVec3], closed: bool, corner: f64) -> Vec<bool> {
    let t = turns(p, closed);
    (0..p.len())
        .map(|i| (!closed && (i == 0 || i + 1 == p.len())) || t[i] > corner)
        .collect()
}

/// The point at length `a` along the open polyline `p`.
fn along(p: &[DVec3], a: f64) -> DVec3 {
    let mut done = 0.0;
    for w in p.windows(2) {
        let len = w[0].distance(w[1]);
        if a <= done + len {
            return w[0].lerp(w[1], ((a - done) / len.max(1e-12)).clamp(0.0, 1.0));
        }
        done += len;
    }
    p[p.len() - 1]
}

/// Even spacing between the fixed points (all of a closed chain without
/// any is one run from its first point round to itself).
fn spaced(p: &[DVec3], closed: bool, keep: &[bool]) -> Vec<DVec3> {
    let n = p.len();
    let mut out = p.to_vec();
    let mut stops: Vec<usize> = (0..n).filter(|i| keep[*i]).collect();
    if stops.is_empty() {
        stops.push(0);
    }
    let runs: Vec<(usize, usize)> = if closed {
        (0..stops.len())
            .map(|k| (stops[k], stops[(k + 1) % stops.len()]))
            .collect()
    } else {
        stops.windows(2).map(|w| (w[0], w[1])).collect()
    };
    for (a, b) in runs {
        // The run's points, a to b, round the loop if need be.
        let mut idx = vec![a];
        let mut i = a;
        loop {
            i = (i + 1) % n;
            idx.push(i);
            if i == b {
                break;
            }
        }
        let run: Vec<DVec3> = idx.iter().map(|i| p[*i]).collect();
        let total: f64 = run.windows(2).map(|w| w[0].distance(w[1])).sum();
        let m = idx.len() - 1;
        for (k, i) in idx.iter().enumerate().take(m).skip(1) {
            out[*i] = along(&run, total * k as f64 / m as f64);
        }
    }
    out
}

/// One smoothing pass: each free point to its neighbours' middle.
fn smoothed(p: &[DVec3], keep: &[bool]) -> Vec<DVec3> {
    let n = p.len();
    (0..n)
        .map(|i| {
            if keep[i] {
                return p[i];
            }
            (p[(i + n - 1) % n] + p[(i + 1) % n]) * 0.5
        })
        .collect()
}

/// Where each point goes.
fn targets(t: &Tweak) -> Result<Vec<DVec3>, ModelError> {
    let p = &t.points;
    let n = p.len();
    match t.kind {
        TweakKind::Space => {
            if n < 3 {
                return Err(ModelError::Refused(
                    "Pick a Chain of Three Points or More".into(),
                ));
            }
            Ok(spaced(p, t.closed, &fixed(p, t.closed, t.corner)))
        }
        TweakKind::Smooth | TweakKind::Relax => {
            if n < 3 {
                return Err(ModelError::Refused(
                    "Pick a Chain of Three Points or More".into(),
                ));
            }
            let keep = fixed(p, t.closed, t.corner);
            let mut q = p.clone();
            for _ in 0..t.iterations.clamp(1, 100) {
                q = smoothed(&q, &keep);
                if t.kind == TweakKind::Relax {
                    q = spaced(&q, t.closed, &keep);
                }
            }
            Ok(q)
        }
        TweakKind::Circle => {
            if !t.closed || n < 3 {
                return Err(ModelError::Refused("Pick a Closed Loop".into()));
            }
            let plane = smith_geom::plane::Plane::fit_loop(p)
                .map_err(|_| ModelError::Refused("The Loop Has No Plane".into()))?;
            let nrm = plane.normal();
            let c = p.iter().copied().sum::<DVec3>() / n as f64;
            let flat: Vec<DVec3> = p.iter().map(|q| *q - nrm * (*q - c).dot(nrm)).collect();
            let fitted = flat.iter().map(|q| q.distance(c)).sum::<f64>() / n as f64;
            let r = t.radius.filter(|r| *r > 0.0).unwrap_or(fitted);
            let u = (flat[0] - c)
                .try_normalize()
                .unwrap_or_else(|| nrm.any_orthonormal_vector());
            let v = nrm.cross(u);
            // Round the way the loop runs.
            let area: f64 = (0..n)
                .map(|i| (flat[i] - c).cross(flat[(i + 1) % n] - c).dot(nrm))
                .sum();
            let dir = if area < 0.0 { -1.0 } else { 1.0 };
            Ok((0..n)
                .map(|i| {
                    let a = dir * std::f64::consts::TAU * i as f64 / n as f64;
                    c + (u * a.cos() + v * a.sin()) * r
                })
                .collect())
        }
        TweakKind::Flatten => {
            if n < 3 {
                return Err(ModelError::Refused(
                    "Pick Faces or a Loop to Flatten".into(),
                ));
            }
            let c = p.iter().copied().sum::<DVec3>() / n as f64;
            let nrm = match t.plane {
                FlatPlane::X => DVec3::X,
                FlatPlane::Y => DVec3::Y,
                FlatPlane::Z => DVec3::Z,
                FlatPlane::BestFit => match t.normal.and_then(DVec3::try_normalize) {
                    Some(nrm) => nrm,
                    None => smith_geom::plane::Plane::fit_loop(p)
                        .map_err(|_| ModelError::Refused("The Points Have No Plane".into()))?
                        .normal(),
                },
            };
            Ok(p.iter().map(|q| *q - nrm * (*q - c).dot(nrm)).collect())
        }
    }
}

/// Applies the tweak: each point moves toward its target by the influence.
///
/// # Errors
/// [`ModelError::Refused`] for a pick the tool cannot use (a chain too
/// short, an open loop for Circle); [`ModelError::NoSuchEntity`] for a point
/// no longer there; as [`Entities::reposition_vertices`] ([`ModelError::NotFlat`]
/// for faces that would bend, without Auto-Tri).
pub fn tweak(entities: &mut Entities, t: &Tweak) -> Result<(), ModelError> {
    let to = targets(t)?;
    let k = t.influence.clamp(0.0, 1.0);
    let mut moves: HashMap<VertexId, DVec3> = HashMap::new();
    for (from, goal) in t.points.iter().zip(&to) {
        let v = entities
            .find_vertex(*from)
            .ok_or(ModelError::NoSuchEntity)?;
        let p = from.lerp(*goal, k);
        if p.distance(*from) > 1e-12 {
            moves.insert(v, p);
        }
    }
    if moves.is_empty() {
        return Ok(());
    }
    let list: Vec<(VertexId, DVec3)> = moves.into_iter().collect();
    entities.reposition_vertices(&list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(kind: TweakKind, points: Vec<DVec3>, closed: bool) -> Tweak {
        Tweak {
            kind,
            points,
            closed,
            normal: None,
            corner: 30f64.to_radians(),
            radius: None,
            plane: FlatPlane::BestFit,
            iterations: 1,
            influence: 1.0,
        }
    }

    #[test]
    fn space_evens_a_straight_run_and_keeps_its_ends() {
        let p = vec![
            DVec3::ZERO,
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(9.0, 0.0, 0.0),
            DVec3::new(12.0, 0.0, 0.0),
        ];
        let mut e = Entities::new();
        e.add_polyline(&p, false).unwrap();
        tweak(&mut e, &base(TweakKind::Space, p, false)).unwrap();
        assert!(e.find_vertex(DVec3::new(4.0, 0.0, 0.0)).is_some());
        assert!(e.find_vertex(DVec3::new(8.0, 0.0, 0.0)).is_some());
        assert!(e.find_vertex(DVec3::new(12.0, 0.0, 0.0)).is_some());
    }

    #[test]
    fn space_keeps_a_rectangles_corners_and_its_face_flat() {
        // A 10 × 4 rectangle with uneven extra points along its long sides.
        let p = vec![
            DVec3::ZERO,
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 4.0, 0.0),
            DVec3::new(9.0, 4.0, 0.0),
            DVec3::new(0.0, 4.0, 0.0),
        ];
        let mut e = Entities::new();
        e.add_polyline(&p, true).unwrap();
        tweak(&mut e, &base(TweakKind::Space, p, true)).unwrap();
        assert_eq!(e.face_count(), 1);
        assert!(e.find_vertex(DVec3::new(5.0, 0.0, 0.0)).is_some());
        assert!(e.find_vertex(DVec3::new(5.0, 4.0, 0.0)).is_some());
    }

    #[test]
    fn circle_rounds_a_square_loop_to_its_fitted_radius_or_a_typed_one() {
        let p = vec![
            DVec3::new(-1.0, -1.0, 0.0),
            DVec3::new(1.0, -1.0, 0.0),
            DVec3::new(1.0, 1.0, 0.0),
            DVec3::new(-1.0, 1.0, 0.0),
        ];
        let mut e = Entities::new();
        e.add_polyline(&p, true).unwrap();
        let mut t = base(TweakKind::Circle, p, true);
        t.radius = Some(3.0);
        tweak(&mut e, &t).unwrap();
        for v in e.vertex_ids() {
            let q = e.vertex(v).unwrap().position();
            assert!((q.length() - 3.0).abs() < 1e-9);
        }
        assert_eq!(e.face_count(), 1);
    }

    #[test]
    fn flatten_puts_points_on_one_plane_best_fit_or_square_to_an_axis() {
        let p = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.2),
            DVec3::new(4.0, 4.0, 0.0),
            DVec3::new(0.0, 4.0, 0.2),
        ];
        let to = targets(&base(TweakKind::Flatten, p.clone(), true)).unwrap();
        let plane = smith_geom::plane::Plane::fit_loop(&to).unwrap();
        assert!(to.iter().all(|q| plane.signed_distance(*q).abs() < 1e-9));
        let mut t = base(TweakKind::Flatten, p, true);
        t.plane = FlatPlane::Z;
        let to = targets(&t).unwrap();
        assert!(to.iter().all(|q| (q.z - 0.1).abs() < 1e-9));
    }

    #[test]
    fn smooth_and_relax_keep_the_ends_and_pull_a_bump_in() {
        let p = vec![
            DVec3::ZERO,
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(2.0, 0.4, 0.0),
            DVec3::new(3.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
        ];
        let mut t = base(TweakKind::Smooth, p.clone(), false);
        t.corner = 179f64.to_radians();
        let to = targets(&t).unwrap();
        assert_eq!(to[0], p[0]);
        assert_eq!(to[4], p[4]);
        assert!(to[2].y < 0.4);
        t.kind = TweakKind::Relax;
        let to = targets(&t).unwrap();
        assert_eq!(to[0], p[0]);
        assert!(to[2].y < 0.4);
    }

    #[test]
    fn circle_needs_a_closed_loop() {
        let p = vec![DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0)];
        assert!(matches!(
            targets(&base(TweakKind::Circle, p, false)),
            Err(ModelError::Refused(_))
        ));
    }
}
