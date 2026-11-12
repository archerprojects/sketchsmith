//! SketchSmith — Sweep and Revolve | v1.0.0 | Thursday 12 November 2026
//!
//! Sweep carries a profile along a path, replacing Follow Me.
//!
//! * **Path:** an unbranched chain of edges (a curve's edges included),
//!   open or closed ([`edge_chain`]).
//! * **Profile:** a face square to the path where it starts (it becomes
//!   the start cap), or anywhere with **Align to Path** (it is placed square
//!   at the start, by its centre); or a typed tube: round (`5r`, with `24s`
//!   for its sides) or rectangular (`w,h`), centred on the path.
//!
//! **Construction: mitred joints on rotation-minimising frames.** Each
//! path segment is a straight prism of the profile. The profile's frame is
//! carried from one segment to the next by the smallest turn that takes one
//! segment's direction to the next (parallel transport: no twist), and at
//! each bend the profile sits in the mitre plane halfway between the
//! segments. So every side face of a plain sweep is flat. Only the sides
//! and an open sweep's two caps are faces.
//!
//! **Options** ([`SweepOptions`]): **Taper** scales the profile from 1 at
//! the start to the given factor at the end, along the path's length;
//! **Twist** turns it by the given angle over the length (the path is cut
//! into pieces turning at most 10° each); **Upright** keeps
//! its first axis as near the drawing axes' Z as the path allows, instead of
//! carrying it along. A closed path that is not flat brings the frame back
//! turned; the turn is shared out over the joints (the closing twist).
//!
//! **Flat faces and Auto-Tri.** Taper on a path with bends, Twist, the
//! closing twist and Upright on a path that climbs while it turns make side
//! faces that cannot be flat. Such a sweep is refused
//! ([`ModelError::NotFlat`]) unless it runs with Auto-Tri, which builds each
//! of those sides as two triangles across a soft, smooth diagonal; sides
//! that are flat stay single faces.
//!
//! **Revolve** ([`revolve`]) turns a face about an axis, fully or partly;
//! its faces are always flat (each lies in the plane of a profile edge
//! turned by one step).
//!
//! **Loft along Path** ([`loft`]) carries two or more profiles along a path,
//! blending the shape between them by length along it; it is Sweep's
//! engine with the profile changing on the way. On a closed path it runs
//! through every profile and back into the first, with no caps; Twist does
//! not apply there (it would leave the seam turned). Each profile may also
//! be turned by its own angle: the loft turns from one profile's angle to
//! the next as it blends (round a loop, back to the first's). A loft with **Edges Only**
//! keeps its rings and the lines along them and makes no faces (so nothing
//! needs to be flat).
//!
//! **Groups and recipes.** The canvas makes every sweep, loft and revolve a group
//! that keeps what it was made from ([`SweepRecipe`]): [`sweep_recipe`] and
//! [`revolve_recipe`] take a picked face's outline as the group's own copy,
//! and [`build_sweep`] builds the group's geometry from the recipe, again
//! whenever Edit Sweep changes it. The face picked stays where it was.

use smith_geom::{DVec2, DVec3};
use smith_model::{
    EdgeId, Entities, FaceId, LoftOptions, ModelError, SweepRecipe, SweepSection, VertexId,
};

use crate::modify::{face_through, orient};

/// The profile swept along the path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SweepProfile {
    /// A face square to the path at its start.
    Face(FaceId),
    /// A round tube: radius (mm) and number of sides.
    Round {
        /// Radius, mm.
        radius: f64,
        /// Sides (at least 3).
        sides: u32,
    },
    /// A rectangular tube, `width` across the frame's first axis and
    /// `height` across its second, mm.
    Rect {
        /// Width, mm.
        width: f64,
        /// Height, mm.
        height: f64,
    },
}

/// Sweep options (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepOptions {
    /// Profile scale at the end of the path (1: no taper).
    pub taper: f64,
    /// Turn of the profile over the path's length, radians.
    pub twist: f64,
    /// Keep the profile's first axis near the drawing axes' Z.
    pub upright: bool,
    /// Place a face profile square at the path's start, by its centre.
    pub align: bool,
}

impl Default for SweepOptions {
    fn default() -> Self {
        Self {
            taper: 1.0,
            twist: 0.0,
            upright: false,
            align: false,
        }
    }
}

/// The ordered points of the unbranched chain of edges through `edge`, and
/// whether it closes. The chain runs on through every point where exactly
/// two edges meet.
pub fn edge_chain(entities: &Entities, edge: EdgeId) -> Option<(Vec<DVec3>, bool)> {
    let [a, b] = entities.edge(edge)?.vertices();
    let pos = |v: VertexId| entities.vertex(v).map(|x| x.position());
    // Walk from `from` away along `via` until a branch, an end or the start.
    let walk = |from: VertexId, via: EdgeId| -> (Vec<VertexId>, bool) {
        let mut out = Vec::new();
        let (mut v, mut e) = (from, via);
        loop {
            let Some(next) = entities.edge(e).map(|x| x.other(v)) else {
                return (out, false);
            };
            if next == a || next == b {
                return (out, next == a || next == b);
            }
            out.push(next);
            let edges = entities
                .vertex(next)
                .map(|x| x.edges().to_vec())
                .unwrap_or_default();
            if edges.len() != 2 {
                return (out, false);
            }
            let Some(&n) = edges.iter().find(|x| **x != e) else {
                return (out, false);
            };
            v = next;
            e = n;
        }
    };
    let degree = |v: VertexId| entities.vertex(v).map_or(0, |x| x.edges().len());
    // Forward from b (away from a), backward from a (away from b).
    let mut fwd = Vec::new();
    let mut closed = false;
    if degree(b) == 2 {
        let next = entities
            .vertex(b)?
            .edges()
            .iter()
            .copied()
            .find(|x| *x != edge)?;
        let (rest, back_to_start) = walk(b, next);
        fwd = rest;
        closed = back_to_start;
    }
    let mut back = Vec::new();
    if !closed && degree(a) == 2 {
        let next = entities
            .vertex(a)?
            .edges()
            .iter()
            .copied()
            .find(|x| *x != edge)?;
        back = walk(a, next).0;
    }
    if closed {
        // The walk came back round to `a`.
        let mut ids = vec![a, b];
        ids.extend(fwd.into_iter().filter(|v| *v != a));
        let pts = ids.into_iter().map(pos).collect::<Option<Vec<_>>>()?;
        return Some((pts, true));
    }
    let mut ids: Vec<VertexId> = back.into_iter().rev().collect();
    ids.push(a);
    ids.push(b);
    ids.extend(fwd);
    Some((ids.into_iter().map(pos).collect::<Option<Vec<_>>>()?, false))
}

/// Turns `x` by the smallest rotation that takes unit `from` to unit `to`.
fn transport(x: DVec3, from: DVec3, to: DVec3) -> DVec3 {
    let axis = from.cross(to);
    let s = axis.length();
    let c = from.dot(to);
    if s < 1e-12 {
        return x;
    }
    let k = axis / s;
    // Rodrigues' rotation.
    x * c + k.cross(x) * s + k * k.dot(x) * (1.0 - c)
}

/// Where a sweep's profile comes from: a face or typed tube, or an outline
/// kept by a Sweep group's recipe.
#[derive(Debug, Clone, Copy)]
enum Source<'a> {
    Profile(SweepProfile),
    Outline(&'a [DVec3]),
    /// Bare edges: a chain's points, and whether it closes.
    Edges(&'a [DVec3], bool),
}

/// What changes a sweep beyond its profile and path: a second path the
/// profile keeps spanning, and bare-edge profiles (a surface, no caps;
/// `Some(closed)`).
#[derive(Debug, Clone, Copy, Default)]
struct Shaping<'a> {
    path2: Option<&'a [DVec3]>,
    edges: Option<bool>,
    /// A loft's profiles and their matching (see [`Morph`]).
    morph: Option<&'a Morph>,
    /// Edges only: no faces.
    wire: bool,
}

/// A face's outer loop points and its normal, refusing a face with holes.
fn face_outline(entities: &Entities, f: FaceId) -> Result<(Vec<DVec3>, DVec3), ModelError> {
    let face = entities.face(f).ok_or(ModelError::NoSuchEntity)?;
    if face.inner().iter().any(|l| !l.is_empty()) {
        return Err(ModelError::DoesNotFit("a profile with holes"));
    }
    let pts = entities
        .loop_vertices(face.outer())
        .into_iter()
        .filter_map(|v| entities.vertex(v).map(|x| x.position()))
        .collect();
    Ok((pts, face.plane().normal()))
}

/// The outer loop points of face `face`, in order: what a Sweep or Revolve
/// group keeps of a picked profile face.
///
/// # Errors
/// [`ModelError::NoSuchEntity`]; [`ModelError::DoesNotFit`] for a face with
/// holes.
pub fn profile_outline(entities: &Entities, face: FaceId) -> Result<Vec<DVec3>, ModelError> {
    face_outline(entities, face).map(|(pts, _)| pts)
}

/// The profile's points in its frame (counter-clockwise), and where its
/// start ring sits: for a face or outline, its own plane and first frame
/// axis.
fn profile_points(
    entities: &Entities,
    source: Source<'_>,
    start: DVec3,
    d0: DVec3,
    up: DVec3,
    align: bool,
) -> Result<(Vec<DVec2>, DVec3), ModelError> {
    // The frame's first axis: `up` made square to the path, else any.
    let u0 = (up - d0 * up.dot(d0))
        .try_normalize()
        .unwrap_or_else(|| d0.any_orthonormal_vector());
    let v0 = d0.cross(u0);
    // Bare edges: their points as they lie, seen along the path.
    if let Source::Edges(pts, closed) = source {
        if pts.len() < 2 || (closed && pts.len() < 3) {
            return Err(ModelError::InvalidData("profile edges".into()));
        }
        let mut out: Vec<DVec2> = pts
            .iter()
            .map(|p| DVec2::new((*p - start).dot(u0), (*p - start).dot(v0)))
            .collect();
        let area: f64 = (0..out.len())
            .map(|i| out[i].perp_dot(out[(i + 1) % out.len()]))
            .sum();
        if closed && area < 0.0 {
            out.reverse();
        }
        return Ok((out, u0));
    }
    // A face or outline: its points and normal.
    let outline = match source {
        Source::Profile(SweepProfile::Face(f)) => Some(face_outline(entities, f)?),
        Source::Outline(pts) => {
            let n = smith_geom::plane::Plane::fit_loop(pts)
                .map_err(|_| ModelError::InvalidData("profile outline".into()))?
                .normal();
            Some((pts.to_vec(), n))
        }
        Source::Profile(_) | Source::Edges(..) => None,
    };
    let pts = match (source, outline) {
        (Source::Profile(SweepProfile::Round { radius, sides }), _) => {
            if !(radius > 0.0 && radius.is_finite()) || sides < 3 {
                return Err(ModelError::InvalidData("tube radius or sides".into()));
            }
            (0..sides)
                .map(|i| {
                    let a = std::f64::consts::TAU * f64::from(i) / f64::from(sides);
                    DVec2::new(radius * a.cos(), radius * a.sin())
                })
                .collect()
        }
        (Source::Profile(SweepProfile::Rect { width, height }), _) => {
            if !(width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite()) {
                return Err(ModelError::InvalidData("tube width or height".into()));
            }
            let (w, h) = (width * 0.5, height * 0.5);
            vec![
                DVec2::new(-w, -h),
                DVec2::new(w, -h),
                DVec2::new(w, h),
                DVec2::new(-w, h),
            ]
        }
        (_, Some((pts, n))) if align => {
            // Placed square at the start by its centre, in its own plane's
            // axes.
            let c = pts.iter().copied().sum::<DVec3>() / pts.len().max(1) as f64;
            let a = (up - n * up.dot(n))
                .try_normalize()
                .unwrap_or_else(|| n.any_orthonormal_vector());
            let b = n.cross(a);
            pts.iter()
                .map(|p| DVec2::new((*p - c).dot(a), (*p - c).dot(b)))
                .collect()
        }
        (_, Some((pts, n))) => {
            if n.cross(d0).length() > 1e-6 {
                return Err(ModelError::DoesNotFit(
                    "the profile is not square to the path where it starts (try Align to Path)",
                ));
            }
            if n.dot(start - pts[0]).abs() > 1e-6 {
                return Err(ModelError::DoesNotFit(
                    "the profile does not lie at the start of the path (try Align to Path)",
                ));
            }
            pts.iter()
                .map(|p| DVec2::new((*p - start).dot(u0), (*p - start).dot(v0)))
                .collect()
        }
        (_, None) => return Err(ModelError::NoSuchEntity),
    };
    let mut pts: Vec<DVec2> = pts;
    if pts.len() < 3 {
        return Err(ModelError::InvalidData("profile outline".into()));
    }
    // Counter-clockwise, so the right of each edge is outside.
    let area: f64 = (0..pts.len())
        .map(|i| pts[i].perp_dot(pts[(i + 1) % pts.len()]))
        .sum();
    if area < 0.0 {
        pts.reverse();
    }
    Ok((pts, u0))
}

/// Sweeps `profile` along the path through `points` (closed if `closed`)
/// with `options`. `up` is the drawing axes' Z: it sets a typed tube's
/// first axis, and Upright's.
///
/// # Errors
/// [`ModelError::DoesNotFit`] for a path that is too short or turns back on
/// itself, or a face profile not square to the path at its start (without
/// Align to Path); [`ModelError::NotFlat`] when side faces would bend and
/// Auto-Tri is off; [`ModelError::InvalidData`] for a bad typed tube; or as
/// [`Entities::add_line`].
pub fn sweep(
    entities: &mut Entities,
    points: &[DVec3],
    closed: bool,
    profile: SweepProfile,
    up: DVec3,
    options: SweepOptions,
) -> Result<(), ModelError> {
    sweep_from(
        entities,
        points,
        closed,
        Source::Profile(profile),
        up,
        options,
        Shaping::default(),
    )
}

/// As [`sweep`], with the profile given as an outline of points (a Sweep
/// group's recipe): the sweep makes its own start cap, so nothing need be
/// drawn first.
///
/// # Errors
/// As [`sweep`]; [`ModelError::InvalidData`] for an outline that is not a
/// flat loop.
pub fn sweep_outline(
    entities: &mut Entities,
    points: &[DVec3],
    closed: bool,
    outline: &[DVec3],
    up: DVec3,
    options: SweepOptions,
) -> Result<(), ModelError> {
    sweep_from(
        entities,
        points,
        closed,
        Source::Outline(outline),
        up,
        options,
        Shaping::default(),
    )
}

/// The sweep itself, from either kind of profile source; or, with
/// `morph`, a loft: profile outlines at path joints (by index, first at the
/// start and last at the end), the shape blended between them by length
/// along the path.
fn sweep_from(
    entities: &mut Entities,
    points: &[DVec3],
    closed: bool,
    source: Source<'_>,
    up: DVec3,
    options: SweepOptions,
    shaping: Shaping<'_>,
) -> Result<(), ModelError> {
    let morph = shaping.morph;
    if shaping.path2.is_some() && closed {
        return Err(ModelError::DoesNotFit("two paths must both be open"));
    }
    if points.len() < 2 || (closed && points.len() < 3) {
        return Err(ModelError::DoesNotFit("the path is too short"));
    }
    if !(options.taper > 0.0 && options.taper.is_finite() && options.twist.is_finite()) {
        return Err(ModelError::InvalidData("taper or twist".into()));
    }
    // Twist turns at most TWIST_STEP per piece of path, so long straight
    // runs are cut into pieces.
    // (A loft has cut its own path into pieces.)
    let points = if !closed && options.twist != 0.0 && morph.is_none() {
        twist_steps(points, options.twist)
    } else {
        points.to_vec()
    };
    // A second path's corners become joints too (a loft has added them).
    let points = match shaping.path2 {
        Some(p2) if morph.is_none() => with_cuts(&points, &path2_cuts(&points, p2)),
        _ => points,
    };
    let n = points.len();
    let segs = if closed { n } else { n - 1 };
    let dirs: Vec<DVec3> = (0..segs)
        .map(|i| (points[(i + 1) % n] - points[i]).try_normalize())
        .collect::<Option<_>>()
        .ok_or(ModelError::DoesNotFit("the path has a zero-length segment"))?;
    for i in 0..segs {
        let next = if closed { (i + 1) % segs } else { i + 1 };
        if next < segs && dirs[i].dot(dirs[next]) < -1.0 + 1e-9 {
            return Err(ModelError::DoesNotFit("the path turns back on itself"));
        }
    }
    let (profile2, u0) = if morph.is_some() {
        let d0 = dirs[0];
        let u0 = (up - d0 * up.dot(d0))
            .try_normalize()
            .unwrap_or_else(|| d0.any_orthonormal_vector());
        (Vec::new(), u0)
    } else {
        profile_points(entities, source, points[0], dirs[0], up, options.align)?
    };
    let mut k = profile2.len();

    // Frames per segment: carried along (parallel transport), or Upright.
    let square = |u: DVec3, d: DVec3| (u - d * u.dot(d)).try_normalize();
    let mut us = vec![u0];
    for i in 1..segs {
        let carried = square(transport(us[i - 1], dirs[i - 1], dirs[i]), dirs[i])
            .unwrap_or_else(|| dirs[i].any_orthonormal_vector());
        let u = if options.upright {
            square(up, dirs[i]).unwrap_or(carried)
        } else {
            carried
        };
        us.push(u);
    }
    // A closed path's leftover turn, shared out over the joints.
    let closing = if closed {
        let back = transport(us[segs - 1], dirs[segs - 1], dirs[0]);
        back.cross(us[0]).dot(dirs[0]).atan2(back.dot(us[0]))
    } else {
        0.0
    };
    // Along-path length at each joint, for Taper and Twist.
    let mut along = vec![0.0];
    for i in 0..segs {
        along.push(along[i] + points[(i + 1) % n].distance(points[i]));
    }
    let total = along[segs].max(1e-12);
    let joints = n;
    // A loft's profiles in their joints' frames, matched point for point.
    // Round a closed path the first comes again at the end, so the last
    // stretch blends back into it.
    let shapes: Option<Vec<(f64, Vec<DVec2>)>> = match morph {
        None => None,
        Some(morph) => {
            let mut flat: Vec<(f64, Vec<DVec2>)> = morph
                .stations
                .iter()
                .map(|(j, outline)| {
                    // The segment its ring is framed on: the one before
                    // the joint, the first for the start.
                    let seg = j.saturating_sub(1).min(segs - 1);
                    let (d, u) = (dirs[seg], us[seg]);
                    let v = d.cross(u);
                    let at = points[*j];
                    let pts = outline
                        .iter()
                        .map(|p| DVec2::new((*p - at).dot(u), (*p - at).dot(v)))
                        .collect();
                    (along[*j], pts)
                })
                .collect();
            if closed && let Some((_, p)) = flat.first() {
                let again = p.clone();
                flat.push((total, again));
            }
            let outlines: Vec<Vec<DVec2>> = flat.iter().map(|(_, p)| p.clone()).collect();
            let matched = if shaping.edges == Some(false) {
                match_open(outlines, morph.vertices as usize)?
            } else {
                match_outlines(
                    outlines,
                    morph.vertices as usize,
                    morph.start_shift as usize,
                )?
            };
            k = matched[0].len();
            Some(flat.iter().map(|(a, _)| *a).zip(matched).collect())
        }
    };
    // Each station's own turn (round a loop the first's again at the end).
    let angles: Vec<f64> = match morph {
        Some(m) => {
            let mut a = m.turns.clone();
            a.resize(m.stations.len(), 0.0);
            if closed && let Some(first) = a.first().copied() {
                a.push(first);
            }
            a
        }
        None => Vec::new(),
    };
    // Round a closed loft, where the first profile comes back: matched
    // again after the last, its points can come round shifted; the closing
    // stretch joins the first ring that many points on.
    let seam = match (&shapes, closed) {
        (Some(s), true) if s.len() > 2 => {
            let (start, again) = (&s[0].1, &s[s.len() - 1].1);
            (0..k)
                .min_by(|a, b| {
                    again[0]
                        .distance(start[*a])
                        .total_cmp(&again[0].distance(start[*b]))
                })
                .unwrap_or(0)
        }
        _ => 0,
    };
    // A loft's shape at length `a` along the path: blended between the
    // profiles either side.
    let blend = |a: f64| -> Vec<DVec2> {
        let Some(shapes) = &shapes else {
            return Vec::new();
        };
        let i = shapes
            .iter()
            .rposition(|(at, _)| *at <= a + 1e-9)
            .unwrap_or(0)
            .min(shapes.len() - 2);
        let ((a0, p0), (a1, p1)) = (&shapes[i], &shapes[i + 1]);
        let t = ((a - a0) / (a1 - a0).max(1e-12)).clamp(0.0, 1.0);
        // Curved: easing out of one profile and into the next.
        let t = if morph.is_some_and(|m| m.eased) {
            t * t * (3.0 - 2.0 * t)
        } else {
            t
        };
        // Turned by the profiles' own angles, blended the same way.
        let (a0, a1) = (
            angles.get(i).copied().unwrap_or(0.0),
            angles.get(i + 1).copied().unwrap_or(0.0),
        );
        let turn = a0 + (a1 - a0) * t;
        let (c, s) = (turn.cos(), turn.sin());
        p0.iter()
            .zip(p1)
            .map(|(x, y)| {
                let q = *x + (*y - *x) * t;
                DVec2::new(q.x * c - q.y * s, q.x * s + q.y * c)
            })
            .collect()
    };
    // The profile at joint j: its frame (turned by Twist and the closing
    // twist) and scale, in the plane square to `d`, pushed along `d` into
    // the plane through the joint with normal `m`.
    // `j` counts along the path: a closed path's start joint is reached
    // again as joint `segs`. Taper and Twist are for open paths; a closed
    // path turns only by its closing twist.
    // A second path: where it starts, seen from the first path's start, so
    // each ring can be turned and scaled to keep spanning the two.
    let rail = match shaping.path2 {
        Some(p2) => {
            if p2.len() < 2 {
                return Err(ModelError::DoesNotFit("the second path is too short"));
            }
            let (u0r, v0r) = (us[0], dirs[0].cross(us[0]));
            let w0 = p2[0] - points[0];
            let w02 = DVec2::new(w0.dot(u0r), w0.dot(v0r));
            if w02.length() < 1e-9 {
                return Err(ModelError::DoesNotFit(
                    "the second path starts on the first",
                ));
            }
            Some((p2, path_length(p2, false), w02))
        }
        None => None,
    };
    let ring = |j: usize, seg: usize, m: DVec3| -> Vec<DVec3> {
        let f = along[j] / total;
        let d = dirs[seg];
        let (turn, scale) = if closed {
            (closing * (j as f64 / segs as f64), 1.0)
        } else {
            (options.twist * f, 1.0 + (options.taper - 1.0) * f)
        };
        let u = us[seg];
        let v = d.cross(u);
        let (c, s_) = (turn.cos(), turn.sin());
        let (u, v) = (u * c + v * s_, v * c - u * s_);
        let at = points[j % n];
        // A closed loft's start ring is the first profile as it stands.
        let shape = if shapes.is_some() {
            blend(if closed && j == segs { 0.0 } else { along[j] })
        } else {
            profile2.clone()
        };
        // Turned and scaled to span the two paths here as at the start.
        let shape: Vec<DVec2> = match &rail {
            Some((p2, len2, w02)) => {
                let w = point_at(p2, f * len2) - at;
                let w2 = DVec2::new(w.dot(u), w.dot(v));
                let k2 = w2.length() / w02.length();
                let turn = w2.y.atan2(w2.x) - w02.y.atan2(w02.x);
                let (c, s_) = (turn.cos(), turn.sin());
                shape
                    .iter()
                    .map(|q| DVec2::new(q.x * c - q.y * s_, q.x * s_ + q.y * c) * k2)
                    .collect()
            }
            None => shape,
        };
        shape
            .iter()
            .map(|q| {
                let x = at + (u * q.x + v * q.y) * scale;
                x + d * ((at - x).dot(m) / d.dot(m))
            })
            .collect()
    };
    let mut rings: Vec<Vec<DVec3>> = Vec::with_capacity(joints);
    for j in 0..joints {
        let r = if !closed && j == 0 {
            ring(0, 0, dirs[0])
        } else if !closed && j == n - 1 {
            ring(j, segs - 1, dirs[segs - 1])
        } else {
            let (before, after) = ((j + segs - 1) % segs, j % segs);
            let mitre = (dirs[before] + dirs[after]).normalize();
            // A closed path's first joint is reached from its last segment,
            // with the whole closing turn.
            ring(if closed && j == 0 { segs } else { j }, before, mitre)
        };
        rings.push(r);
    }

    // Which sides are flat; the rest need Auto-Tri.
    let quad = |s: usize, q: usize| -> [DVec3; 4] {
        let (a, b) = (s, (s + 1) % joints);
        let q2 = (q + 1) % k;
        // The closing stretch joins the start ring past the seam.
        let off = if closed && b == 0 { seam } else { 0 };
        [
            rings[a][q],
            rings[a][q2],
            rings[b][(q2 + off) % k],
            rings[b][(q + off) % k],
        ]
    };
    let flat = |pts: &[DVec3; 4]| {
        smith_geom::plane::Plane::fit_loop(pts).is_ok_and(|p| p.max_deviation(pts) <= 1e-6)
    };
    // An open edge profile has one side fewer than points.
    let open_profile = shaping.edges == Some(false);
    let sides = if open_profile { k - 1 } else { k };
    // Edges only: no faces, so none needs to be flat.
    let bent: Vec<(usize, usize)> = (0..segs)
        .flat_map(|s| (0..sides).map(move |q| (s, q)))
        .filter(|(s, q)| !shaping.wire && !flat(&quad(*s, *q)))
        .collect();
    let fold = smith_model::auto_tri();
    if !bent.is_empty() && !fold {
        return Err(ModelError::NotFlat(Vec::new()));
    }

    let faces_before: std::collections::HashSet<FaceId> = entities.face_ids().collect();
    // A face profile's own points count as the sweep's.
    let own: Vec<VertexId> = match source {
        Source::Profile(SweepProfile::Face(f)) => entities
            .face(f)
            .map(|x| entities.loop_vertices(x.outer()))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let vertices_before: std::collections::HashSet<VertexId> =
        entities.vertex_ids().filter(|v| !own.contains(v)).collect();
    // Edges: rings, the lines along each segment, and the folds' diagonals.
    for r in &rings {
        entities.add_polyline(r, !open_profile)?;
    }
    for s in 0..segs {
        let (a, b) = (s, (s + 1) % joints);
        let off = if closed && b == 0 { seam } else { 0 };
        for (q, p) in rings[a].iter().enumerate() {
            entities.add_line(*p, rings[b][(q + off) % rings[b].len()])?;
        }
    }
    let mut diagonals = Vec::new();
    for (s, q) in &bent {
        let [a, _, c, _] = quad(*s, *q);
        diagonals.extend(entities.add_line(a, c)?);
    }
    entities.soften_edges(&diagonals);

    // Keep only the sides (or their triangles) and an open sweep's caps:
    // the sticky model also fills other flat loops the new edges close.
    let mut keep: std::collections::HashSet<FaceId> = std::collections::HashSet::new();
    for s in (0..segs).filter(|_| !shaping.wire) {
        for q in 0..sides {
            let [a, b, c, d] = quad(s, q);
            if bent.contains(&(s, q)) {
                keep.extend(face_through(entities, &[a, b, c]));
                keep.extend(face_through(entities, &[a, c, d]));
            } else {
                keep.extend(face_through(entities, &[a, b, c, d]));
            }
        }
    }
    // Caps for a face profile only: bare edges make a surface.
    let caps = !closed && shaping.edges.is_none() && !shaping.wire;
    if caps {
        keep.extend(face_through(entities, &rings[0]));
        keep.extend(face_through(entities, &rings[joints - 1]));
    }
    // Only faces wholly of the sweep's own new points: a face of the model
    // that the new edges split keeps its pieces.
    let extra: Vec<FaceId> = entities
        .face_ids()
        .filter(|f| !faces_before.contains(f) && !keep.contains(f))
        .filter(|f| {
            entities.face(*f).is_some_and(|face| {
                face.loops()
                    .flat_map(|l| entities.loop_vertices(l))
                    .all(|v| !vertices_before.contains(&v))
            })
        })
        .collect();
    for f in extra {
        entities.erase_face(f)?;
    }
    // Sides face out of the profile; caps face out along the path. (The
    // rings are counter-clockwise seen along the path, so the right of each
    // ring edge is outside.)
    for (s, dir) in dirs.iter().enumerate().take(segs) {
        for q in 0..sides {
            let [a, b, c, d] = quad(s, q);
            let out = (b - a).cross(*dir);
            let parts: Vec<Vec<DVec3>> = if bent.contains(&(s, q)) {
                vec![vec![a, b, c], vec![a, c, d]]
            } else {
                vec![vec![a, b, c, d]]
            };
            for part in parts {
                if let Some(f) = face_through(entities, &part) {
                    orient(entities, f, out)?;
                }
            }
        }
    }
    if caps {
        if let Some(f) = face_through(entities, &rings[0]) {
            orient(entities, f, -dirs[0])?;
        }
        if let Some(f) = face_through(entities, &rings[joints - 1]) {
            orient(entities, f, dirs[segs - 1])?;
        }
    }
    Ok(())
}

/// The most a Twist turns over one piece of path.
const TWIST_STEP: f64 = 10.0 * std::f64::consts::PI / 180.0;

/// `points` with each segment cut into equal pieces so that `twist`,
/// shared out by length, turns at most [`TWIST_STEP`] per piece.
fn twist_steps(points: &[DVec3], twist: f64) -> Vec<DVec3> {
    let total: f64 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
    if total <= 0.0 {
        return points.to_vec();
    }
    let per_mm = twist.abs() / total;
    let mut out = vec![points[0]];
    for w in points.windows(2) {
        let len = w[0].distance(w[1]);
        let pieces = ((per_mm * len / TWIST_STEP).ceil() as usize).max(1);
        for i in 1..=pieces {
            out.push(w[0].lerp(w[1], i as f64 / pieces as f64));
        }
    }
    out
}

/// Revolves face `face` about the axis through `axis_point` along unit
/// `axis_dir` by `angle` radians (a full turn closes) in `steps` steps.
/// Every face is flat: each lies in the plane of a profile edge and its
/// copy one step round.
///
/// # Errors
/// [`ModelError::NoSuchEntity`]; [`ModelError::DoesNotFit`] for a profile
/// crossing the axis or with holes, or fewer than 3 steps for a full turn;
/// or as [`Entities::add_line`].
pub fn revolve(
    entities: &mut Entities,
    face: FaceId,
    axis_point: DVec3,
    axis_dir: DVec3,
    angle: f64,
    steps: u32,
) -> Result<(), ModelError> {
    let axis = axis_dir
        .try_normalize()
        .ok_or(ModelError::DoesNotFit("the axis has no direction"))?;
    let f = entities.face(face).ok_or(ModelError::NoSuchEntity)?;
    if f.inner().iter().any(|l| !l.is_empty()) {
        return Err(ModelError::DoesNotFit("a profile with holes"));
    }
    let f_outer = f.outer().to_vec();
    let profile: Vec<DVec3> = entities
        .loop_vertices(f.outer())
        .into_iter()
        .filter_map(|v| entities.vertex(v).map(|x| x.position()))
        .collect();
    // All on one side of the axis, and none on it except touching.
    let radial = |p: DVec3| {
        let r = p - axis_point;
        r - axis * r.dot(axis)
    };
    let side = profile
        .iter()
        .map(|p| radial(*p))
        .find(|r| r.length() > 1e-9)
        .ok_or(ModelError::DoesNotFit("the profile lies on the axis"))?;
    if profile
        .iter()
        .any(|p| radial(*p).length() > 1e-9 && radial(*p).dot(side) < 0.0)
    {
        return Err(ModelError::DoesNotFit("the profile crosses the axis"));
    }
    let full = (angle.abs() - std::f64::consts::TAU).abs() < 1e-9;
    if !angle.is_finite() || angle.abs() < 1e-9 || angle.abs() > std::f64::consts::TAU + 1e-9 {
        return Err(ModelError::DoesNotFit(
            "the angle must be between 0° and 360°",
        ));
    }
    if steps < if full { 3 } else { 1 } {
        return Err(ModelError::DoesNotFit("too few steps"));
    }
    let turned = |p: DVec3, a: f64| -> DVec3 {
        let r = p - axis_point;
        axis_point + smith_geom::DMat3::from_axis_angle(axis, a) * r
    };
    let copies = if full { steps } else { steps + 1 };
    let rings: Vec<Vec<DVec3>> = (0..copies)
        .map(|i| {
            let a = angle * f64::from(i) / f64::from(steps);
            profile.iter().map(|p| turned(*p, a)).collect()
        })
        .collect();
    let faces_before: std::collections::HashSet<FaceId> = entities.face_ids().collect();
    // The profile's own points count as the revolve's.
    let own: Vec<VertexId> = entities.loop_vertices(&f_outer);
    let vertices_before: std::collections::HashSet<VertexId> =
        entities.vertex_ids().filter(|v| !own.contains(v)).collect();
    for r in rings.iter().skip(1) {
        entities.add_polyline(r, true)?;
    }
    let spans = if full { copies } else { copies - 1 };
    for s in 0..spans {
        let (a, b) = (s as usize, ((s + 1) % copies) as usize);
        for (p, q) in rings[a].iter().zip(&rings[b]) {
            if p.distance(*q) > 1e-9 {
                entities.add_line(*p, *q)?;
            }
        }
    }
    // Rings in between are not faces; a part turn keeps its end cap.
    let mut keep: std::collections::HashSet<FaceId> = std::collections::HashSet::new();
    if !full {
        keep.extend(face_through(entities, &rings[copies as usize - 1]));
    }
    // Everything else the new edges filled — rings in between, a flat
    // profile's loops round the axis — goes, unless it is a side or holds
    // points of the model from before.
    let extra: Vec<FaceId> = entities
        .face_ids()
        .filter(|f| !faces_before.contains(f) && !keep.contains(f))
        .filter(|f| !is_side(entities, *f, &rings))
        .filter(|f| {
            entities.face(*f).is_some_and(|face| {
                face.loops()
                    .flat_map(|l| entities.loop_vertices(l))
                    .all(|v| !vertices_before.contains(&v))
            })
        })
        .collect();
    for f in extra {
        entities.erase_face(f)?;
    }
    if full && let Some(f) = face_through(entities, &rings[0]) {
        // A full turn swallows the profile face too.
        entities.erase_face(f)?;
    }
    Ok(())
}

/// A loft for [`sweep_from`]: profile outlines at path joints (by index,
/// first at the start and last at the end) and how they are matched and
/// blended.
#[derive(Debug)]
struct Morph {
    stations: Vec<(usize, Vec<DVec3>)>,
    /// Each station's own turn, radians.
    turns: Vec<f64>,
    /// Points round every profile (raised to the most any has).
    vertices: u32,
    /// Turns each profile's pairing with the one before by this many points.
    start_shift: u32,
    /// The blend eases into each profile (Segments above 0).
    eased: bool,
}

/// Profiles made to match point for point: each counter-clockwise, with
/// points added along its edges (none moved) up to `vertices` or the most
/// any profile has, and turned to pair with the profile before (nearest
/// first, then `shift` points further round).
fn match_outlines(
    outlines: Vec<Vec<DVec2>>,
    vertices: usize,
    shift: usize,
) -> Result<Vec<Vec<DVec2>>, ModelError> {
    if outlines.iter().any(|o| o.len() < 3) {
        return Err(ModelError::InvalidData("profile outline".into()));
    }
    let most = outlines.iter().map(Vec::len).max().unwrap_or(0);
    let k = vertices.max(most);
    let mut out: Vec<Vec<DVec2>> = Vec::with_capacity(outlines.len());
    for mut o in outlines {
        let area: f64 = (0..o.len())
            .map(|i| o[i].perp_dot(o[(i + 1) % o.len()]))
            .sum();
        if area < 0.0 {
            o.reverse();
        }
        let mut r = add_points(&o, k);
        if let Some(prev) = out.last() {
            // The turn of the loop that sits closest to the previous one.
            let cost = |turn: usize| -> f64 {
                (0..k)
                    .map(|i| r[(i + turn) % k].distance_squared(prev[i]))
                    .sum()
            };
            let best = (0..k)
                .min_by(|a, b| cost(*a).total_cmp(&cost(*b)))
                .unwrap_or(0);
            r.rotate_left((best + shift) % k);
        }
        out.push(r);
    }
    Ok(out)
}

/// The closed loop `o` with points added along its edges, none moved, up to
/// `k` points in all: shared out by edge length (the longest edges first
/// for what is left over), each edge's new points evenly spaced along it.
fn add_points(o: &[DVec2], k: usize) -> Vec<DVec2> {
    let m = o.len();
    if k <= m {
        return o.to_vec();
    }
    let extra = k - m;
    let lens: Vec<f64> = (0..m).map(|i| o[i].distance(o[(i + 1) % m])).collect();
    let total: f64 = lens.iter().sum::<f64>().max(1e-12);
    // Whole shares first, then the largest remainders.
    let exact: Vec<f64> = lens.iter().map(|l| extra as f64 * l / total).collect();
    let mut adds: Vec<usize> = exact.iter().map(|x| x.floor() as usize).collect();
    let mut left = extra - adds.iter().sum::<usize>();
    let mut order: Vec<usize> = (0..m).collect();
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
    for i in 0..m {
        let (a, b) = (o[i], o[(i + 1) % m]);
        out.push(a);
        for j in 1..=adds[i] {
            out.push(a + (b - a) * (j as f64 / (adds[i] + 1) as f64));
        }
    }
    out
}

/// Open edge profiles made to match point for point: points added along
/// their edges (none moved) up to `vertices` or the most any has, each
/// running the same way as the one before.
fn match_open(outlines: Vec<Vec<DVec2>>, vertices: usize) -> Result<Vec<Vec<DVec2>>, ModelError> {
    if outlines.iter().any(|o| o.len() < 2) {
        return Err(ModelError::InvalidData("profile edges".into()));
    }
    let most = outlines.iter().map(Vec::len).max().unwrap_or(0);
    let k = vertices.max(most);
    let mut out: Vec<Vec<DVec2>> = Vec::with_capacity(outlines.len());
    for o in outlines {
        let mut r = add_points_open(&o, k);
        if let Some(prev) = out.last() {
            let cost = |x: &[DVec2]| -> f64 {
                x.iter()
                    .zip(prev)
                    .map(|(a, b)| a.distance_squared(*b))
                    .sum()
            };
            let mut back = r.clone();
            back.reverse();
            if cost(&back) < cost(&r) {
                r = back;
            }
        }
        out.push(r);
    }
    Ok(out)
}

/// The open chain `o` with points added along its edges, none moved, up to
/// `k` points in all, shared out by edge length.
fn add_points_open(o: &[DVec2], k: usize) -> Vec<DVec2> {
    let m = o.len();
    if k <= m {
        return o.to_vec();
    }
    // As a loop whose closing edge takes nothing: shares by length.
    let extra = k - m;
    let lens: Vec<f64> = (0..m - 1).map(|i| o[i].distance(o[i + 1])).collect();
    let total: f64 = lens.iter().sum::<f64>().max(1e-12);
    let exact: Vec<f64> = lens.iter().map(|l| extra as f64 * l / total).collect();
    let mut adds: Vec<usize> = exact.iter().map(|x| x.floor() as usize).collect();
    let mut left = extra - adds.iter().sum::<usize>();
    let mut order: Vec<usize> = (0..m - 1).collect();
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
    for i in 0..m - 1 {
        let (a, b) = (o[i], o[i + 1]);
        out.push(a);
        for j in 1..=adds[i] {
            out.push(a + (b - a) * (j as f64 / (adds[i] + 1) as f64));
        }
    }
    out.push(o[m - 1]);
    out
}

/// Lengths along the open path through `points` matching the corners of
/// `path2` (by share of each path's length), where joints are added so the
/// rings follow the second path's turns.
fn path2_cuts(points: &[DVec3], path2: &[DVec3]) -> Vec<f64> {
    let total = path_length(points, false);
    let total2 = path_length(path2, false).max(1e-12);
    let mut along = 0.0;
    let mut cuts = Vec::new();
    for w in path2.windows(2).take(path2.len().saturating_sub(2)) {
        along += w[0].distance(w[1]);
        cuts.push(total * along / total2);
    }
    cuts
}

/// The open path through `points` with joints added at lengths `cuts`
/// along it (none where there is one already).
fn with_cuts(points: &[DVec3], cuts: &[f64]) -> Vec<DVec3> {
    let mut out = vec![points[0]];
    let mut along = 0.0;
    for w in points.windows(2) {
        let len = w[0].distance(w[1]);
        for c in cuts {
            if *c > along + 1e-9 && *c < along + len - 1e-9 {
                out.push(w[0].lerp(w[1], (c - along) / len));
            }
        }
        out.push(w[1]);
        along += len;
    }
    out
}

/// `points` (open) with extra joints: `segments` evenly spaced between
/// each pair of neighbouring stations (lengths along it, which must be
/// joints already), and more so that `twist`, shared out by length, turns
/// at most [`TWIST_STEP`] per piece. Returns the points and each station's
/// joint index.
fn loft_joints(
    points: &[DVec3],
    stations: &[usize],
    segments: u32,
    twist: f64,
) -> (Vec<DVec3>, Vec<usize>) {
    let total: f64 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
    let per_mm = if total > 0.0 {
        twist.abs() / total
    } else {
        0.0
    };
    let mut out = vec![points[0]];
    let mut index = vec![0usize; stations.len()];
    for (si, pair) in stations.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        // The span's length and where its added rings go.
        let span: f64 = points[a..=b].windows(2).map(|w| w[0].distance(w[1])).sum();
        let rings: Vec<f64> = (1..=segments)
            .map(|i| span * f64::from(i) / f64::from(segments + 1))
            .collect();
        let mut along = 0.0;
        for w in points[a..=b].windows(2) {
            let len = w[0].distance(w[1]);
            // Cut points within this segment: added rings, then twist
            // pieces, in order.
            let mut cuts: Vec<f64> = rings
                .iter()
                .filter(|r| **r > along + 1e-9 && **r < along + len - 1e-9)
                .map(|r| (r - along) / len)
                .collect();
            let pieces = ((per_mm * len / TWIST_STEP).ceil() as usize).max(1);
            cuts.extend((1..pieces).map(|i| i as f64 / pieces as f64));
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
            for t in cuts {
                out.push(w[0].lerp(w[1], t));
            }
            out.push(w[1]);
            along += len;
        }
        index[si + 1] = out.len() - 1;
    }
    (out, index)
}

/// Lofts along the path through `points` (open) between profile outlines
/// lying across it (at least two): each profile sits where the path is
/// nearest its centre, the loft runs from the first to the last, and the
/// shape is blended between them by length along the path (see
/// [`LoftOptions`]). Profiles are matched point for point by adding points
/// along their edges, so a square can loft into a circle. Sides that the
/// blend bends need Auto-Tri, as Sweep's do.
///
/// # Errors
/// [`ModelError::DoesNotFit`] for a too short path, fewer than two
/// profiles, or profiles at the same place; [`ModelError::InvalidData`] for
/// an outline that is not a loop; [`ModelError::NotFlat`] for bent sides
/// without Auto-Tri.
pub fn loft(
    entities: &mut Entities,
    points: &[DVec3],
    outlines: &[Vec<DVec3>],
    up: DVec3,
    options: LoftOptions,
) -> Result<(), ModelError> {
    loft_shaped(
        entities,
        points,
        outlines,
        &[],
        up,
        options,
        None,
        None,
        false,
    )
}

/// As [`loft`], with an optional second path the profiles keep spanning,
/// bare-edge profiles (`Some(closed)`: a surface with no caps), and a path
/// that closes on itself (`closed`: `points` without the first again; the
/// loft runs from the first profile round through the others and back into
/// it, with no caps). `turns` turns each profile by its own angle
/// (radians; missing ones are 0).
///
/// # Errors
/// As [`loft`]; [`ModelError::DoesNotFit`] for a second path with a closed
/// one; [`ModelError::InvalidData`] for a turn that is not finite.
// The nine inputs are the recipe's own, each used once.
#[allow(clippy::too_many_arguments)]
pub fn loft_shaped(
    entities: &mut Entities,
    points: &[DVec3],
    outlines: &[Vec<DVec3>],
    turns: &[f64],
    up: DVec3,
    options: LoftOptions,
    path2: Option<&[DVec3]>,
    edges: Option<bool>,
    closed: bool,
) -> Result<(), ModelError> {
    if points.len() < 2 || (closed && points.len() < 3) {
        return Err(ModelError::DoesNotFit("the path is too short"));
    }
    if outlines.len() < 2 {
        return Err(ModelError::DoesNotFit("a loft needs two profiles"));
    }
    if turns.iter().any(|t| !t.is_finite()) {
        return Err(ModelError::InvalidData("profile twist".into()));
    }
    if closed {
        if path2.is_some() {
            return Err(ModelError::DoesNotFit("two paths must both be open"));
        }
        return loft_closed(entities, points, outlines, turns, up, options, edges);
    }
    // Where each profile meets the path: by length along it.
    let mut stations: Vec<(f64, &Vec<DVec3>, f64)> = Vec::new();
    let least = if edges == Some(false) { 2 } else { 3 };
    for (i, o) in outlines.iter().enumerate() {
        if o.len() < least {
            return Err(ModelError::InvalidData("profile outline".into()));
        }
        let turn = turns.get(i).copied().unwrap_or(0.0);
        stations.push((
            length_at(points, nearest_on_path(points, centre(o))),
            o,
            turn,
        ));
    }
    stations.sort_by(|a, b| a.0.total_cmp(&b.0));
    if stations.windows(2).any(|w| w[1].0 - w[0].0 < 1e-6) {
        return Err(ModelError::DoesNotFit(
            "two profiles sit at the same place on the path",
        ));
    }
    // The path from the first profile to the last, with a joint at each.
    let first = stations[0].0;
    let last = stations[stations.len() - 1].0;
    let mut path: Vec<DVec3> = vec![point_at(points, first)];
    let mut joint_of: Vec<usize> = vec![0];
    let mut along = 0.0;
    let mut next = 1;
    for w in points.windows(2) {
        let len = w[0].distance(w[1]);
        while next < stations.len() && stations[next].0 <= along + len + 1e-9 {
            let at = stations[next].0;
            let p = w[0] + (w[1] - w[0]) * ((at - along) / len.max(1e-12));
            if path.last().is_none_or(|q| q.distance(p) > 1e-9) {
                path.push(p);
            }
            joint_of.push(path.len() - 1);
            next += 1;
        }
        along += len;
        if along > first + 1e-9
            && along < last - 1e-9
            && path.last().is_none_or(|q| q.distance(w[1]) > 1e-9)
        {
            path.push(w[1]);
        }
    }
    let profiles: Vec<(Vec<DVec3>, f64)> = stations
        .iter()
        .map(|(_, o, t)| ((*o).clone(), *t))
        .collect();
    loft_on(
        entities,
        &path,
        &joint_of,
        profiles,
        up,
        options,
        options.segments,
        Shaping {
            path2,
            edges,
            morph: None,
            wire: false,
        },
        false,
    )
}

/// A loft round the closed path through `points` (see [`loft_shaped`]):
/// the loop walked from the first profile's place round to it again, with a
/// joint at each profile and the closing joint last.
fn loft_closed(
    entities: &mut Entities,
    points: &[DVec3],
    outlines: &[Vec<DVec3>],
    turns: &[f64],
    up: DVec3,
    options: LoftOptions,
    edges: Option<bool>,
) -> Result<(), ModelError> {
    let mut lp = points.to_vec();
    lp.push(points[0]);
    let total = path_length(&lp, false);
    let least = if edges == Some(false) { 2 } else { 3 };
    let mut stations: Vec<(f64, &Vec<DVec3>, f64)> = Vec::new();
    for (i, o) in outlines.iter().enumerate() {
        if o.len() < least {
            return Err(ModelError::InvalidData("profile outline".into()));
        }
        let turn = turns.get(i).copied().unwrap_or(0.0);
        stations.push((length_at(&lp, nearest_on_path(&lp, centre(o))), o, turn));
    }
    stations.sort_by(|a, b| a.0.total_cmp(&b.0));
    let first = stations[0].0;
    // Each profile's place measured on from the first, round the loop.
    let places: Vec<f64> = stations.iter().map(|(s, ..)| s - first).collect();
    let mut gaps: Vec<f64> = places.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.push(total - places[places.len() - 1]);
    if gaps.iter().any(|g| *g < 1e-6) {
        return Err(ModelError::DoesNotFit(
            "two profiles sit at the same place on the path",
        ));
    }
    // The loop from the first profile round to it again: its points in
    // order, a joint at each profile.
    let mut path: Vec<DVec3> = vec![point_at(&lp, first)];
    let mut joint_of: Vec<usize> = vec![0];
    let mut next = 1;
    let corners: Vec<(f64, DVec3)> = {
        // The loop's corners by length on from the first profile.
        let mut at = 0.0;
        let mut out = Vec::new();
        for w in lp.windows(2) {
            at += w[0].distance(w[1]);
            out.push(((at - first).rem_euclid(total), w[1]));
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    };
    for (c, p) in corners.iter().filter(|(c, _)| *c > 1e-9) {
        while next < places.len() && places[next] <= *c + 1e-9 {
            let at = point_at_loop(&lp, first + places[next], total);
            if path.last().is_none_or(|q| q.distance(at) > 1e-9) {
                path.push(at);
            }
            joint_of.push(path.len() - 1);
            next += 1;
        }
        if path.last().is_none_or(|q| q.distance(*p) > 1e-9) {
            path.push(*p);
        }
    }
    while next < places.len() {
        path.push(point_at_loop(&lp, first + places[next], total));
        joint_of.push(path.len() - 1);
        next += 1;
    }
    // Back at the first profile's place: the closing joint.
    let start = path[0];
    if path.last().is_none_or(|q| q.distance(start) > 1e-9) {
        path.push(start);
    }
    joint_of.push(path.len() - 1);
    let profiles: Vec<(Vec<DVec3>, f64)> = stations
        .iter()
        .map(|(_, o, t)| ((*o).clone(), *t))
        .collect();
    loft_on(
        entities,
        &path,
        &joint_of,
        profiles,
        up,
        options,
        options.segments,
        Shaping {
            path2: None,
            edges,
            morph: None,
            wire: false,
        },
        true,
    )
}

/// The point at length `a` round the closed loop `lp` (its first point
/// again last) of length `total`.
fn point_at_loop(lp: &[DVec3], a: f64, total: f64) -> DVec3 {
    point_at(lp, a.rem_euclid(total))
}

/// Lofts between profile outlines (at least two) in the order given, along
/// a smooth curve through their centres that leaves each square to its
/// face, toward the next (Loft). Segments sets the rings between
/// each pair: 0 lofts straight from centre to centre. Otherwise as
/// [`loft`].
///
/// # Errors
/// As [`loft`]; [`ModelError::DoesNotFit`] for profiles whose centres
/// coincide.
pub fn loft_spline(
    entities: &mut Entities,
    outlines: &[Vec<DVec3>],
    turns: &[f64],
    up: DVec3,
    options: LoftOptions,
) -> Result<(), ModelError> {
    if turns.iter().any(|t| !t.is_finite()) {
        return Err(ModelError::InvalidData("profile twist".into()));
    }
    if outlines.len() < 2 {
        return Err(ModelError::DoesNotFit("a loft needs two profiles"));
    }
    let mut centres = Vec::with_capacity(outlines.len());
    let mut normals = Vec::with_capacity(outlines.len());
    for o in outlines {
        let plane = smith_geom::plane::Plane::fit_loop(o)
            .map_err(|_| ModelError::InvalidData("profile outline".into()))?;
        centres.push(centre(o));
        normals.push(plane.normal());
    }
    if centres.windows(2).any(|w| w[0].distance(w[1]) < 1e-6) {
        return Err(ModelError::DoesNotFit("two profiles share a centre"));
    }
    // Each profile's normal turned to point on along the loft: the first
    // toward the next profile (`up` when it is square to that), each later
    // one so the curve arrives into it from the previous one's pull.
    let n = outlines.len();
    for i in 0..n {
        let toward = if i == 0 {
            centres[1] - centres[0]
        } else {
            let pull = centres[i - 1].distance(centres[i]) / 3.0;
            centres[i] - (centres[i - 1] + normals[i - 1] * pull)
        };
        let d = normals[i].dot(toward);
        let flip = if d.abs() <= 1e-9 * toward.length().max(1.0) {
            normals[i].dot(up) < 0.0
        } else {
            d < 0.0
        };
        if flip {
            normals[i] = -normals[i];
        }
    }
    // A cubic between each pair, leaving and arriving along the normals,
    // its pull a third of the gap; Segments points within it.
    let mut path = vec![centres[0]];
    let mut joint_of = vec![0usize];
    for i in 0..n - 1 {
        let (a, b) = (centres[i], centres[i + 1]);
        let pull = a.distance(b) / 3.0;
        let (c1, c2) = (a + normals[i] * pull, b - normals[i + 1] * pull);
        let pieces = options.segments + 1;
        for j in 1..=pieces {
            let t = f64::from(j) / f64::from(pieces);
            let u = 1.0 - t;
            path.push(
                a * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + b * (t * t * t),
            );
        }
        joint_of.push(path.len() - 1);
    }
    // Its rings are already its Segments: none more are added along it.
    loft_on(
        entities,
        &path,
        &joint_of,
        outlines
            .iter()
            .enumerate()
            .map(|(i, o)| (o.clone(), turns.get(i).copied().unwrap_or(0.0)))
            .collect(),
        up,
        options,
        0,
        Shaping::default(),
        false,
    )
}

// The loft's nine inputs are distinct and each used once; a struct for
// one private call would only rename them.
#[allow(clippy::too_many_arguments)]
/// The loft itself: `profiles` at joints `joint_of` of `path`, with
/// `rings` more joints added between each pair (the blend eases into each
/// profile when Segments is above 0). On a `closed` path the last joint is
/// the start again, closing the loop.
fn loft_on(
    entities: &mut Entities,
    path: &[DVec3],
    joint_of: &[usize],
    profiles: Vec<(Vec<DVec3>, f64)>,
    up: DVec3,
    options: LoftOptions,
    rings: u32,
    shaping: Shaping<'_>,
    closed: bool,
) -> Result<(), ModelError> {
    if !options.twist.is_finite() {
        return Err(ModelError::InvalidData("twist".into()));
    }
    // A closed loft does not twist: the seam would be left turned.
    let twist = if closed { 0.0 } else { options.twist };
    // A second path's corners as joints, the profiles' joints found again.
    let (path, joint_of) = match shaping.path2 {
        Some(p2) => {
            let cut = with_cuts(path, &path2_cuts(path, p2));
            let mut from = 0;
            let mut index = Vec::with_capacity(joint_of.len());
            for j in joint_of {
                let at = path[*j];
                let i = (from..cut.len())
                    .find(|i| cut[*i].distance(at) < 1e-9)
                    .unwrap_or(from);
                index.push(i);
                from = i;
            }
            (cut, index)
        }
        None => (path.to_vec(), joint_of.to_vec()),
    };
    let (mut path, mut index) = loft_joints(&path, &joint_of, rings, twist);
    if closed {
        // The closing joint is the start again: the sweep closes there.
        path.pop();
        index.pop();
    }
    let morph = Morph {
        turns: profiles.iter().map(|(_, t)| *t).collect(),
        stations: index
            .into_iter()
            .zip(profiles.into_iter().map(|(o, _)| o))
            .collect(),
        vertices: options.vertices,
        start_shift: options.start_shift,
        eased: options.segments > 0,
    };
    sweep_from(
        entities,
        &path,
        closed,
        Source::Outline(&[]),
        up,
        SweepOptions {
            twist,
            ..SweepOptions::default()
        },
        Shaping {
            morph: Some(&morph),
            wire: options.edges_only,
            ..shaping
        },
    )
}

/// The centre of an outline.
fn centre(o: &[DVec3]) -> DVec3 {
    o.iter().copied().sum::<DVec3>() / o.len().max(1) as f64
}

/// Length along the path through `points` at its point nearest `p`.
fn nearest_on_path(points: &[DVec3], p: DVec3) -> (usize, f64) {
    let mut best = (0usize, 0.0f64, f64::INFINITY);
    for (i, w) in points.windows(2).enumerate() {
        let d = w[1] - w[0];
        let t = ((p - w[0]).dot(d) / d.length_squared().max(1e-24)).clamp(0.0, 1.0);
        let dist = (w[0] + d * t).distance(p);
        if dist < best.2 {
            best = (i, t, dist);
        }
    }
    (best.0, best.1)
}

/// Length along the path at segment `seg`, fraction `t`.
fn length_at(points: &[DVec3], (seg, t): (usize, f64)) -> f64 {
    let before: f64 = points
        .windows(2)
        .take(seg)
        .map(|w| w[0].distance(w[1]))
        .sum();
    before + points[seg].distance(points[seg + 1]) * t
}

/// The point at length `a` along the path.
fn point_at(points: &[DVec3], a: f64) -> DVec3 {
    let mut along = 0.0;
    for w in points.windows(2) {
        let len = w[0].distance(w[1]);
        if a <= along + len {
            return w[0] + (w[1] - w[0]) * ((a - along) / len.max(1e-12));
        }
        along += len;
    }
    points[points.len() - 1]
}

/// Where Copy Along Path puts its copies: for each length along the path
/// through `points` in `stations`, the move that carries the path's start
/// frame to the frame there (frames carried along the path as Sweep's are,
/// the first axis from `up`). A copy is its original moved by this.
///
/// # Errors
/// [`ModelError::DoesNotFit`] for a path that is too short or has a
/// zero-length segment.
pub fn path_placements(
    points: &[DVec3],
    closed: bool,
    up: DVec3,
    stations: &[f64],
) -> Result<Vec<smith_geom::DMat4>, ModelError> {
    let mut pts = points.to_vec();
    if closed {
        pts.push(points[0]);
    }
    if pts.len() < 2 {
        return Err(ModelError::DoesNotFit("the path is too short"));
    }
    let dirs: Vec<DVec3> = pts
        .windows(2)
        .map(|w| (w[1] - w[0]).try_normalize())
        .collect::<Option<_>>()
        .ok_or(ModelError::DoesNotFit("the path has a zero-length segment"))?;
    let square = |u: DVec3, d: DVec3| (u - d * u.dot(d)).try_normalize();
    let mut us = vec![square(up, dirs[0]).unwrap_or_else(|| dirs[0].any_orthonormal_vector())];
    for i in 1..dirs.len() {
        let u = square(transport(us[i - 1], dirs[i - 1], dirs[i]), dirs[i])
            .unwrap_or_else(|| dirs[i].any_orthonormal_vector());
        us.push(u);
    }
    let frame = |i: usize, at: DVec3| {
        let (d, u) = (dirs[i], us[i]);
        smith_geom::DMat4::from_cols(
            d.extend(0.0),
            u.extend(0.0),
            d.cross(u).extend(0.0),
            at.extend(1.0),
        )
    };
    let start_inv = frame(0, pts[0]).inverse();
    let lens: Vec<f64> = pts.windows(2).map(|w| w[0].distance(w[1])).collect();
    Ok(stations
        .iter()
        .map(|s| {
            // The segment the length falls in (the last one past the end).
            let mut along = 0.0;
            let mut i = 0;
            while i + 1 < lens.len() && along + lens[i] < *s - 1e-9 {
                along += lens[i];
                i += 1;
            }
            let t = ((*s - along) / lens[i].max(1e-12)).clamp(0.0, 1.0);
            frame(i, pts[i] + (pts[i + 1] - pts[i]) * t) * start_inv
        })
        .collect())
}

/// The lengths along a path of length `total` where Copy Along Path puts
/// its copies: `count` evenly spread to the end (short of the start again
/// on a closed path), or every `spacing` when it is above zero.
pub fn copy_stations(total: f64, closed: bool, count: u32, spacing: f64) -> Vec<f64> {
    if spacing > 1e-9 {
        let mut out = Vec::new();
        let mut s = spacing;
        let end = if closed { total - 1e-6 } else { total + 1e-6 };
        while s <= end && out.len() < 10_000 {
            out.push(s.min(total));
            s += spacing;
        }
        return out;
    }
    let n = count.max(1);
    let parts = if closed { n + 1 } else { n };
    (1..=n)
        .map(|k| total * f64::from(k) / f64::from(parts))
        .collect()
}

/// A path's length (closing back to its start when `closed`).
pub fn path_length(points: &[DVec3], closed: bool) -> f64 {
    let open: f64 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
    match (closed, points.first(), points.last()) {
        (true, Some(a), Some(b)) => open + a.distance(*b),
        _ => open,
    }
}

/// Builds an Extrude along Path (or Extrude Edges along Path) from its
/// recipe parts: `section` along the path through `points`, with an
/// optional second path the profile keeps spanning.
///
/// # Errors
/// As [`sweep`].
pub fn sweep_shaped(
    entities: &mut Entities,
    points: &[DVec3],
    closed: bool,
    section: &SweepSection,
    path2: Option<&[DVec3]>,
    up: DVec3,
    options: SweepOptions,
) -> Result<(), ModelError> {
    let (source, edges) = match section {
        SweepSection::Outline(pts) => (Source::Outline(pts), None),
        SweepSection::Edges { points: e, closed } => (Source::Edges(e, *closed), Some(*closed)),
        SweepSection::Round { radius, sides } => (
            Source::Profile(SweepProfile::Round {
                radius: *radius,
                sides: *sides,
            }),
            None,
        ),
        SweepSection::Rect { width, height } => (
            Source::Profile(SweepProfile::Rect {
                width: *width,
                height: *height,
            }),
            None,
        ),
    };
    sweep_from(
        entities,
        points,
        closed,
        source,
        up,
        options,
        Shaping {
            path2,
            edges,
            morph: None,
            wire: false,
        },
    )
}

/// The recipe a Sweep group keeps: the path, and the profile as its own
/// outline (a face's points) or the typed tube.
///
/// # Errors
/// As [`profile_outline`] for a face profile.
pub fn sweep_recipe(
    entities: &Entities,
    path: &[DVec3],
    closed: bool,
    path2: Option<&[DVec3]>,
    profile: SweepProfile,
    up: DVec3,
    options: SweepOptions,
) -> Result<SweepRecipe, ModelError> {
    let section = match profile {
        SweepProfile::Face(f) => SweepSection::Outline(profile_outline(entities, f)?),
        SweepProfile::Round { radius, sides } => SweepSection::Round { radius, sides },
        SweepProfile::Rect { width, height } => SweepSection::Rect { width, height },
    };
    Ok(SweepRecipe::Sweep {
        path: path.to_vec(),
        closed,
        path2: path2.map(<[DVec3]>::to_vec),
        section,
        up,
        taper: options.taper,
        twist: options.twist,
        upright: options.upright,
        align: options.align,
    })
}

/// The recipe a Revolve group keeps: the face's outline and the axis.
///
/// # Errors
/// As [`profile_outline`].
pub fn revolve_recipe(
    entities: &Entities,
    face: FaceId,
    axis_point: DVec3,
    axis_dir: DVec3,
    angle: f64,
    steps: u32,
) -> Result<SweepRecipe, ModelError> {
    Ok(SweepRecipe::Revolve {
        outline: profile_outline(entities, face)?,
        axis_point,
        axis_dir,
        angle,
        steps,
    })
}

/// Builds a Sweep or Revolve group's geometry from its recipe into
/// `entities` (the group's own, empty): the profile is the group's own, so
/// a sweep gets its own start cap and a part revolve its own first end.
///
/// # Errors
/// As [`sweep`] and [`revolve`]; [`ModelError::InvalidData`] for an
/// outline that is not a flat loop.
pub fn build_sweep(entities: &mut Entities, recipe: &SweepRecipe) -> Result<(), ModelError> {
    match recipe {
        SweepRecipe::Skin {
            segments,
            sampling,
            edges_only,
        } => crate::skin::skin(entities, segments, *sampling, *edges_only).map(|_| ()),
        SweepRecipe::Sweep {
            path,
            closed,
            path2,
            section,
            up,
            taper,
            twist,
            upright,
            align,
        } => {
            let options = SweepOptions {
                taper: *taper,
                twist: *twist,
                upright: *upright,
                align: *align,
            };
            sweep_shaped(
                entities,
                path,
                *closed,
                section,
                path2.as_deref(),
                *up,
                options,
            )
        }
        SweepRecipe::Loft {
            path: Some(path),
            profiles,
            up,
            options,
            path2,
            edges,
            closed,
            twists,
        } => loft_shaped(
            entities,
            path,
            profiles,
            twists,
            *up,
            *options,
            path2.as_deref(),
            *edges,
            *closed,
        ),
        SweepRecipe::Loft {
            path: None,
            profiles,
            up,
            options,
            twists,
            ..
        } => loft_spline(entities, profiles, twists, *up, *options),
        SweepRecipe::Revolve {
            outline,
            axis_point,
            axis_dir,
            angle,
            steps,
        } => {
            if outline.len() < 3 {
                return Err(ModelError::InvalidData("profile outline".into()));
            }
            entities.add_polyline(outline, true)?;
            let face = face_through(entities, outline)
                .ok_or_else(|| ModelError::InvalidData("profile outline".into()))?;
            revolve(entities, face, *axis_point, *axis_dir, *angle, *steps)
        }
    }
}

/// Whether face `f` is one of a revolve's sides: its points all lie on two
/// neighbouring rings.
fn is_side(entities: &Entities, f: FaceId, rings: &[Vec<DVec3>]) -> bool {
    let Some(face) = entities.face(f) else {
        return false;
    };
    let pts: Vec<DVec3> = entities
        .loop_vertices(face.outer())
        .into_iter()
        .filter_map(|v| entities.vertex(v).map(|x| x.position()))
        .collect();
    let on = |r: &Vec<DVec3>, p: &DVec3| r.iter().any(|x| x.distance(*p) < 1e-9);
    (0..rings.len()).any(|i| {
        let j = (i + 1) % rings.len();
        pts.iter().all(|p| on(&rings[i], p) || on(&rings[j], p))
            && pts.iter().any(|p| on(&rings[i], p))
            && pts.iter().any(|p| on(&rings[j], p))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_geom::curve::{Circle, Curve, CurveFrame};

    #[test]
    fn a_1mm_tube_on_a_10mm_circle_is_a_closed_torus() {
        let mut e = Entities::new();
        let frame = CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap();
        e.add_curve(Curve::Circle(Circle::new(frame, 10.0).unwrap()), 24)
            .unwrap();
        // Just the circle's edges: its disc would be cut by the tube.
        let disc = e.face_ids().next().unwrap();
        e.erase_face(disc).unwrap();
        let first = e.edge_ids().next().unwrap();
        let (path, closed) = edge_chain(&e, first).unwrap();
        assert!(closed);
        assert_eq!(path.len(), 24);
        let before = (e.edge_count(), e.face_count());
        sweep(
            &mut e,
            &path,
            true,
            SweepProfile::Round {
                radius: 1.0,
                sides: 12,
            },
            DVec3::Z,
            SweepOptions::default(),
        )
        .unwrap();
        // 24 × 12 quads, no caps; every face made, so every face is flat.
        assert_eq!(e.face_count() - before.1, 24 * 12);
        assert_eq!(e.edge_count() - before.0, 24 * 12 * 2);
        // Closed: every edge of the tube has two faces.
        let open = e
            .edge_ids()
            .filter(|x| e.edge(*x).is_some_and(|y| y.faces().len() == 1))
            .count();
        assert_eq!(open, 0);
    }

    #[test]
    fn a_square_tube_along_a_helix_is_flat_faced_and_capped() {
        let mut e = Entities::new();
        let pts: Vec<DVec3> = (0..=36)
            .map(|i| {
                let a = f64::from(i) * 10f64.to_radians();
                DVec3::new(20.0 * a.cos(), 20.0 * a.sin(), f64::from(i) * 0.8)
            })
            .collect();
        sweep(
            &mut e,
            &pts,
            false,
            SweepProfile::Rect {
                width: 2.0,
                height: 1.0,
            },
            DVec3::Z,
            SweepOptions::default(),
        )
        .unwrap();
        // 36 segments × 4 sides, plus two caps — all flat, or they would not
        // have been made.
        assert_eq!(e.face_count(), 36 * 4 + 2);
    }

    #[test]
    fn a_square_profile_on_a_closed_path_that_is_not_flat_needs_auto_tri() {
        let mut e = Entities::new();
        // An uneven loop that rises and falls (a symmetric saddle closes
        // with no turn left over, so it would not show the problem).
        let pts = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 1.0),
            DVec3::new(12.0, 9.0, 4.0),
            DVec3::new(1.0, 11.0, -2.0),
        ];
        let out = sweep(
            &mut e,
            &pts,
            true,
            SweepProfile::Rect {
                width: 1.0,
                height: 1.0,
            },
            DVec3::Z,
            SweepOptions::default(),
        );
        assert!(matches!(out, Err(ModelError::NotFlat(_))));
        assert_eq!(e.face_count(), 0);
        // With Auto-Tri the closing twist is shared out, and the sides that
        // cannot be flat are folded.
        smith_model::with_auto_tri(|| {
            sweep(
                &mut e,
                &pts,
                true,
                SweepProfile::Rect {
                    width: 1.0,
                    height: 1.0,
                },
                DVec3::Z,
                SweepOptions::default(),
            )
        })
        .unwrap();
        let open = e
            .edge_ids()
            .filter(|x| e.edge(*x).is_some_and(|y| y.faces().len() != 2))
            .count();
        assert_eq!(open, 0, "the twisted loop still closes");
    }

    #[test]
    fn twist_needs_auto_tri_and_taper_on_a_straight_path_stays_flat() {
        let path = [DVec3::ZERO, DVec3::new(0.0, 0.0, 20.0)];
        let rect = SweepProfile::Rect {
            width: 4.0,
            height: 2.0,
        };
        let mut e = Entities::new();
        let twist = SweepOptions {
            twist: 90f64.to_radians(),
            ..SweepOptions::default()
        };
        assert!(matches!(
            sweep(&mut e, &path, false, rect, DVec3::Y, twist),
            Err(ModelError::NotFlat(_))
        ));
        smith_model::with_auto_tri(|| sweep(&mut e, &path, false, rect, DVec3::Y, twist)).unwrap();
        // 90° in 10° pieces: 9 pieces × 4 twisted sides as triangles, plus
        // two caps; and it closes.
        assert_eq!(e.face_count(), 9 * 4 * 2 + 2);
        let open = e
            .edge_ids()
            .filter(|x| e.edge(*x).is_some_and(|y| y.faces().len() != 2))
            .count();
        assert_eq!(open, 0);

        let mut e = Entities::new();
        let taper = SweepOptions {
            taper: 0.5,
            ..SweepOptions::default()
        };
        sweep(&mut e, &path, false, rect, DVec3::Y, taper).unwrap();
        assert_eq!(e.face_count(), 4 + 2);
    }

    #[test]
    fn revolve_a_full_turn_closes_and_a_part_turn_keeps_its_ends() {
        let square = [
            DVec3::new(5.0, 0.0, 0.0),
            DVec3::new(7.0, 0.0, 0.0),
            DVec3::new(7.0, 0.0, 2.0),
            DVec3::new(5.0, 0.0, 2.0),
        ];
        let mut e = Entities::new();
        e.add_polyline(&square, true).unwrap();
        let f = e.face_ids().next().unwrap();
        revolve(&mut e, f, DVec3::ZERO, DVec3::Z, std::f64::consts::TAU, 12).unwrap();
        assert_eq!(e.face_count(), 12 * 4);
        let open = e
            .edge_ids()
            .filter(|x| e.edge(*x).is_some_and(|y| y.faces().len() != 2))
            .count();
        assert_eq!(open, 0);

        let mut e = Entities::new();
        e.add_polyline(&square, true).unwrap();
        let f = e.face_ids().next().unwrap();
        revolve(&mut e, f, DVec3::ZERO, DVec3::Z, 90f64.to_radians(), 6).unwrap();
        assert_eq!(e.face_count(), 6 * 4 + 2);
    }

    #[test]
    fn a_face_square_to_the_path_start_becomes_the_start_cap() {
        let mut e = Entities::new();
        // A 2 × 2 square in the YZ plane at the origin, and a bent path.
        e.add_polyline(
            &[
                DVec3::new(0.0, -1.0, -1.0),
                DVec3::new(0.0, 1.0, -1.0),
                DVec3::new(0.0, 1.0, 1.0),
                DVec3::new(0.0, -1.0, 1.0),
            ],
            true,
        )
        .unwrap();
        let face = e.face_ids().next().unwrap();
        let path = [
            DVec3::ZERO,
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 10.0, 0.0),
        ];
        sweep(
            &mut e,
            &path,
            false,
            SweepProfile::Face(face),
            DVec3::Z,
            SweepOptions::default(),
        )
        .unwrap();
        assert_eq!(e.face_count(), 2 * 4 + 2);
    }

    /// Every edge has exactly two faces: a closed solid.
    fn closed_solid(e: &Entities) -> bool {
        e.edge_ids()
            .all(|x| e.edge(x).is_some_and(|y| y.faces().len() == 2))
    }

    fn square_yz(x: f64) -> Vec<DVec3> {
        vec![
            DVec3::new(x, -1.0, -1.0),
            DVec3::new(x, 1.0, -1.0),
            DVec3::new(x, 1.0, 1.0),
            DVec3::new(x, -1.0, 1.0),
        ]
    }

    #[test]
    fn a_picked_face_is_kept_as_an_outline_and_the_group_gets_its_own_start_cap() {
        let mut model = Entities::new();
        model.add_polyline(&square_yz(0.0), true).unwrap();
        let face = model.face_ids().next().unwrap();
        let path = vec![
            DVec3::ZERO,
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 10.0, 0.0),
        ];
        let recipe = sweep_recipe(
            &model,
            &path,
            false,
            None,
            SweepProfile::Face(face),
            DVec3::Z,
            SweepOptions::default(),
        )
        .unwrap();
        let SweepRecipe::Sweep {
            section: SweepSection::Outline(pts),
            ..
        } = &recipe
        else {
            panic!("not an outline sweep: {recipe:?}");
        };
        assert_eq!(pts.len(), 4);
        let mut group = Entities::new();
        build_sweep(&mut group, &recipe).unwrap();
        // Same faces as the sweep from the face itself, caps included.
        assert_eq!(group.face_count(), 2 * 4 + 2);
        assert!(closed_solid(&group));
        // The picked face is untouched.
        assert_eq!(model.face_count(), 1);
    }

    #[test]
    fn an_outline_round_a_closed_path_leaves_no_disc_or_loose_edges() {
        let mut group = Entities::new();
        let path: Vec<DVec3> = (0..16)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 16.0;
                DVec3::new(10.0 * a.cos(), 10.0 * a.sin(), 0.0)
            })
            .collect();
        // A 1 × 1 square square to the path at its start (10, 0, 0).
        let outline = vec![
            DVec3::new(9.5, 0.0, -0.5),
            DVec3::new(10.5, 0.0, -0.5),
            DVec3::new(10.5, 0.0, 0.5),
            DVec3::new(9.5, 0.0, 0.5),
        ];
        let recipe = SweepRecipe::Sweep {
            path,
            closed: true,
            path2: None,
            section: SweepSection::Outline(outline),
            up: DVec3::Z,
            taper: 1.0,
            twist: 0.0,
            upright: false,
            align: true,
        };
        build_sweep(&mut group, &recipe).unwrap();
        assert_eq!(group.face_count(), 16 * 4);
        assert!(closed_solid(&group));
    }

    #[test]
    fn an_aligned_outline_away_from_the_path_leaves_nothing_where_it_was() {
        let mut group = Entities::new();
        let recipe = SweepRecipe::Sweep {
            path: vec![DVec3::ZERO, DVec3::new(0.0, 0.0, 10.0)],
            closed: false,
            path2: None,
            section: SweepSection::Outline(square_yz(50.0)),
            up: DVec3::Z,
            taper: 1.0,
            twist: 0.0,
            upright: false,
            align: true,
        };
        build_sweep(&mut group, &recipe).unwrap();
        assert_eq!(group.face_count(), 4 + 2);
        assert!(closed_solid(&group));
        assert!(group.find_vertex(DVec3::new(50.0, 1.0, 1.0)).is_none());
    }

    #[test]
    fn a_revolve_recipe_builds_whole_and_part_turns() {
        let outline = vec![
            DVec3::new(5.0, 0.0, 0.0),
            DVec3::new(7.0, 0.0, 0.0),
            DVec3::new(7.0, 0.0, 2.0),
            DVec3::new(5.0, 0.0, 2.0),
        ];
        let recipe = |angle: f64| SweepRecipe::Revolve {
            outline: outline.clone(),
            axis_point: DVec3::ZERO,
            axis_dir: DVec3::Z,
            angle,
            steps: 12,
        };
        let mut group = Entities::new();
        build_sweep(&mut group, &recipe(std::f64::consts::TAU)).unwrap();
        assert_eq!(group.face_count(), 12 * 4);
        assert!(closed_solid(&group));
        let mut group = Entities::new();
        build_sweep(&mut group, &recipe(90f64.to_radians())).unwrap();
        assert_eq!(group.face_count(), 12 * 4 + 2);
        assert!(closed_solid(&group));
    }

    #[test]
    fn a_twisted_recipe_is_refused_without_auto_tri_and_built_with_it() {
        let recipe = SweepRecipe::Sweep {
            path: vec![DVec3::ZERO, DVec3::new(0.0, 0.0, 10.0)],
            closed: false,
            path2: None,
            section: SweepSection::Rect {
                width: 2.0,
                height: 1.0,
            },
            up: DVec3::X,
            taper: 1.0,
            twist: 90f64.to_radians(),
            upright: false,
            align: false,
        };
        let mut group = Entities::new();
        assert!(matches!(
            build_sweep(&mut group, &recipe),
            Err(ModelError::NotFlat(_))
        ));
        let mut group = Entities::new();
        smith_model::with_auto_tri(|| build_sweep(&mut group, &recipe)).unwrap();
        assert!(closed_solid(&group));
    }

    #[test]
    fn copies_spread_evenly_or_by_spacing() {
        assert_eq!(copy_stations(10.0, false, 2, 0.0), vec![5.0, 10.0]);
        assert_eq!(copy_stations(12.0, true, 2, 0.0), vec![4.0, 8.0]);
        assert_eq!(copy_stations(10.0, false, 0, 4.0), vec![4.0, 8.0]);
        assert_eq!(copy_stations(12.0, true, 0, 4.0), vec![4.0, 8.0]);
    }

    #[test]
    fn a_copy_is_carried_and_turned_by_the_path_frames() {
        // Along X for 10, then along Y for 10.
        let path = [
            DVec3::ZERO,
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 10.0, 0.0),
        ];
        let m = path_placements(&path, false, DVec3::Z, &[5.0, 20.0]).unwrap();
        // Halfway along the first leg: moved, not turned.
        let p = m[0].transform_point3(DVec3::new(0.0, 1.0, 0.0));
        assert!(p.distance(DVec3::new(5.0, 1.0, 0.0)) < 1e-9);
        // At the end: turned a quarter about Z with the path.
        let p = m[1].transform_point3(DVec3::new(0.0, 1.0, 0.0));
        assert!(p.distance(DVec3::new(9.0, 10.0, 0.0)) < 1e-9, "{p}");
        let d = m[1].transform_vector3(DVec3::X);
        assert!(d.distance(DVec3::Y) < 1e-9);
    }

    #[test]
    fn a_square_lofts_into_a_round_along_a_bent_path() {
        let path = vec![
            DVec3::ZERO,
            DVec3::new(0.0, 0.0, 20.0),
            DVec3::new(10.0, 0.0, 30.0),
        ];
        let square = vec![
            DVec3::new(-1.0, -1.0, 0.0),
            DVec3::new(1.0, -1.0, 0.0),
            DVec3::new(1.0, 1.0, 0.0),
            DVec3::new(-1.0, 1.0, 0.0),
        ];
        // An 8-sided round at the end, square to the last segment.
        let d = DVec3::new(1.0, 0.0, 1.0).normalize();
        let (u, v) = (DVec3::Y, d.cross(DVec3::Y));
        let round: Vec<DVec3> = (0..8)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 8.0;
                DVec3::new(10.0, 0.0, 30.0) + (u * a.cos() + v * a.sin()) * 1.5
            })
            .collect();
        let mut e = Entities::new();
        // Blending a square into a round bends the sides.
        assert!(matches!(
            loft(
                &mut e,
                &path,
                &[square.clone(), round.clone()],
                DVec3::Y,
                LoftOptions::default()
            ),
            Err(ModelError::NotFlat(_))
        ));
        let mut e = Entities::new();
        smith_model::with_auto_tri(|| {
            loft(
                &mut e,
                &path,
                &[round.clone(), square],
                DVec3::Y,
                LoftOptions::default(),
            )
        })
        .unwrap();
        assert!(closed_solid(&e));
        // The same round at both ends lofts flat, like a sweep.
        let start: Vec<DVec3> = (0..8)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 8.0;
                DVec3::new(1.5 * a.cos(), 1.5 * a.sin(), 0.0)
            })
            .collect();
        let mut e = Entities::new();
        loft(
            &mut e,
            &path,
            &[start, round],
            DVec3::Y,
            LoftOptions::default(),
        )
        .unwrap();
        assert!(closed_solid(&e));
        assert_eq!(e.face_count(), 8 * 2 + 2);
    }

    #[test]
    fn a_loft_runs_only_between_its_first_and_last_profiles() {
        let path = vec![DVec3::ZERO, DVec3::new(0.0, 0.0, 30.0)];
        let sq = |z: f64, r: f64| {
            vec![
                DVec3::new(-r, -r, z),
                DVec3::new(r, -r, z),
                DVec3::new(r, r, z),
                DVec3::new(-r, r, z),
            ]
        };
        let mut e = Entities::new();
        // Three squares, the middle one larger: two tapered pieces.
        loft(
            &mut e,
            &path,
            &[sq(20.0, 1.0), sq(10.0, 1.0), sq(15.0, 2.0)],
            DVec3::Y,
            LoftOptions::default(),
        )
        .unwrap();
        assert!(closed_solid(&e));
        assert_eq!(e.face_count(), 4 * 2 + 2);
        assert!(e.find_vertex(DVec3::new(-1.0, -1.0, 0.0)).is_none());
        assert!(e.find_vertex(DVec3::new(-2.0, -2.0, 15.0)).is_some());
    }

    fn square_at(z: f64, r: f64) -> Vec<DVec3> {
        vec![
            DVec3::new(-r, -r, z),
            DVec3::new(r, -r, z),
            DVec3::new(r, r, z),
            DVec3::new(-r, r, z),
        ]
    }

    /// A 100 mm square loop on the ground.
    fn ring_path() -> Vec<DVec3> {
        vec![
            DVec3::ZERO,
            DVec3::new(100.0, 0.0, 0.0),
            DVec3::new(100.0, 100.0, 0.0),
            DVec3::new(0.0, 100.0, 0.0),
        ]
    }

    /// A square across the loop's first side, and an octagon across its
    /// third.
    fn ring_profiles() -> Vec<Vec<DVec3>> {
        let square = vec![
            DVec3::new(50.0, -5.0, -5.0),
            DVec3::new(50.0, 5.0, -5.0),
            DVec3::new(50.0, 5.0, 5.0),
            DVec3::new(50.0, -5.0, 5.0),
        ];
        let octagon = (0..8)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 8.0;
                DVec3::new(50.0, 100.0 + 8.0 * a.cos(), 8.0 * a.sin())
            })
            .collect();
        vec![square, octagon]
    }

    fn ring(e: &mut Entities, options: LoftOptions) -> Result<(), ModelError> {
        loft_shaped(
            e,
            &ring_path(),
            &ring_profiles(),
            &[],
            DVec3::Z,
            options,
            None,
            None,
            true,
        )
    }

    #[test]
    fn a_closed_path_lofts_round_through_every_profile_into_a_ring() {
        let mut e = Entities::new();
        smith_model::with_auto_tri(|| ring(&mut e, LoftOptions::default())).unwrap();
        // One closed shell, with no caps.
        assert!(closed_solid(&e));
        assert!(e.face_count() > 0);
        let faces = e.face_count();
        // Twist does not apply round a loop.
        let mut t = Entities::new();
        smith_model::with_auto_tri(|| {
            ring(
                &mut t,
                LoftOptions {
                    twist: 1.0,
                    ..LoftOptions::default()
                },
            )
        })
        .unwrap();
        assert_eq!(t.face_count(), faces);
        // Shifted pairings come round to a seam, still closed.
        let mut s = Entities::new();
        smith_model::with_auto_tri(|| {
            ring(
                &mut s,
                LoftOptions {
                    start_shift: 1,
                    segments: 4,
                    ..LoftOptions::default()
                },
            )
        })
        .unwrap();
        assert!(closed_solid(&s));
        // Into the first profile (across x = 50 on the straight first side)
        // every line (not the folds' soft diagonals) arrives at the point of
        // it nearest where it comes from: a seam joined wrongly would land
        // them points further round.
        let first_ring: Vec<DVec3> = s
            .vertex_ids()
            .filter_map(|v| s.vertex(v).map(|x| x.position()))
            .filter(|p| (p.x - 50.0).abs() < 1e-9 && p.y.abs() <= 5.0 + 1e-9)
            .collect();
        assert_eq!(first_ring.len(), 8);
        let arriving: Vec<(DVec3, DVec3)> = s
            .edge_ids()
            .filter(|x| s.edge(*x).is_some_and(|y| !y.flags.soft))
            .filter_map(|x| s.edge_points(x))
            .filter_map(|(a, b)| {
                let on_ring = |p: DVec3| (p.x - 50.0).abs() < 1e-9 && p.y.abs() <= 5.0 + 1e-9;
                let before = |p: DVec3| p.y.abs() < 10.0 && p.x < 50.0 - 1.0;
                match (on_ring(a), on_ring(b)) {
                    (true, false) if before(b) => Some((a, b)),
                    (false, true) if before(a) => Some((b, a)),
                    _ => None,
                }
            })
            .collect();
        assert_eq!(arriving.len(), 8);
        let across = |a: DVec3, b: DVec3| DVec2::new(a.y - b.y, a.z - b.z).length();
        for (r, p) in &arriving {
            let nearest = first_ring
                .iter()
                .copied()
                .min_by(|a, b| across(*a, *p).total_cmp(&across(*b, *p)))
                .unwrap();
            assert!(
                (*r - nearest).length() < 1e-9,
                "{r} from {p}: nearest {nearest}"
            );
        }
        // Edges Only: the cage, no faces.
        let mut w = Entities::new();
        ring(
            &mut w,
            LoftOptions {
                edges_only: true,
                ..LoftOptions::default()
            },
        )
        .unwrap();
        assert_eq!(w.face_count(), 0);
        assert!(w.edge_count() > 0);
        // A second path wants open paths.
        let mut x = Entities::new();
        assert!(matches!(
            loft_shaped(
                &mut x,
                &ring_path(),
                &ring_profiles(),
                &[],
                DVec3::Z,
                LoftOptions::default(),
                Some(&[DVec3::ZERO, DVec3::X]),
                None,
                true,
            ),
            Err(ModelError::DoesNotFit(_))
        ));
    }

    #[test]
    fn each_profile_turns_by_its_own_angle_and_round_a_loop_too() {
        let path = vec![DVec3::ZERO, DVec3::new(0.0, 0.0, 10.0)];
        let profiles = [square_at(0.0, 1.0), square_at(10.0, 1.0)];
        let quarter = std::f64::consts::FRAC_PI_4;
        // Turning B bends the sides: Auto-Tri, as Twist.
        let mut e = Entities::new();
        assert!(matches!(
            loft_shaped(
                &mut e,
                &path,
                &profiles,
                &[0.0, quarter],
                DVec3::Y,
                LoftOptions::default(),
                None,
                None,
                false,
            ),
            Err(ModelError::NotFlat(_))
        ));
        let mut e = Entities::new();
        smith_model::with_auto_tri(|| {
            loft_shaped(
                &mut e,
                &path,
                &profiles,
                &[0.0, quarter],
                DVec3::Y,
                LoftOptions::default(),
                None,
                None,
                false,
            )
        })
        .unwrap();
        assert!(closed_solid(&e));
        // The far end is the square turned an eighth: its corners on the axes.
        let r2 = std::f64::consts::SQRT_2;
        assert!(e.find_vertex(DVec3::new(r2, 0.0, 10.0)).is_some());
        assert!(e.find_vertex(DVec3::new(0.0, r2, 10.0)).is_some());
        // The near end is not turned.
        assert!(e.find_vertex(DVec3::new(1.0, 1.0, 0.0)).is_some());
        // Not finite: refused.
        let mut x = Entities::new();
        assert!(matches!(
            loft_shaped(
                &mut x,
                &path,
                &profiles,
                &[0.0, f64::NAN],
                DVec3::Y,
                LoftOptions::default(),
                None,
                None,
                false,
            ),
            Err(ModelError::InvalidData(_))
        ));
        // Round a loop: the octagon turned, back to the square at the seam.
        let mut ring = Entities::new();
        smith_model::with_auto_tri(|| {
            loft_shaped(
                &mut ring,
                &ring_path(),
                &ring_profiles(),
                &[0.0, 0.5],
                DVec3::Z,
                LoftOptions::default(),
                None,
                None,
                true,
            )
        })
        .unwrap();
        assert!(closed_solid(&ring));
    }

    #[test]
    fn added_points_keep_every_corner_and_the_shape() {
        let sq = [
            DVec2::new(0.0, 0.0),
            DVec2::new(4.0, 0.0),
            DVec2::new(4.0, 4.0),
            DVec2::new(0.0, 4.0),
        ];
        let out = add_points(&sq, 24);
        assert_eq!(out.len(), 24);
        // Every corner kept, five points on each side between them.
        for (i, c) in sq.iter().enumerate() {
            assert_eq!(out[i * 6], *c);
        }
        // Every point lies on the square's outline.
        assert!(out.iter().all(|p| p.x.abs() < 1e-12
            || (p.x - 4.0).abs() < 1e-12
            || p.y.abs() < 1e-12
            || (p.y - 4.0).abs() < 1e-12));
        // Uneven sides share out by length.
        let rect = [
            DVec2::new(0.0, 0.0),
            DVec2::new(3.0, 0.0),
            DVec2::new(3.0, 1.0),
            DVec2::new(0.0, 1.0),
        ];
        assert_eq!(add_points(&rect, 12).len(), 12);
    }

    #[test]
    fn vertices_raise_every_profile_and_stay_flat_between_like_shapes() {
        let path = vec![DVec3::ZERO, DVec3::new(0.0, 0.0, 10.0)];
        let mut e = Entities::new();
        let options = LoftOptions {
            vertices: 8,
            ..LoftOptions::default()
        };
        loft(
            &mut e,
            &path,
            &[square_at(0.0, 1.0), square_at(10.0, 2.0)],
            DVec3::Y,
            options,
        )
        .unwrap();
        assert!(closed_solid(&e));
        // Eight sides, each split at its added point: still flat.
        assert_eq!(e.face_count(), 8 + 2);
    }

    #[test]
    fn start_shift_turns_the_pairing_and_bends_the_sides() {
        let path = vec![DVec3::ZERO, DVec3::new(0.0, 0.0, 10.0)];
        let options = LoftOptions {
            start_shift: 1,
            ..LoftOptions::default()
        };
        let mut e = Entities::new();
        assert!(matches!(
            loft(
                &mut e,
                &path,
                &[square_at(0.0, 1.0), square_at(10.0, 1.0)],
                DVec3::Y,
                options,
            ),
            Err(ModelError::NotFlat(_))
        ));
    }

    #[test]
    fn edges_only_lofts_a_cage_with_no_faces_even_twisted() {
        let path = vec![DVec3::ZERO, DVec3::new(0.0, 0.0, 10.0)];
        let options = LoftOptions {
            segments: 3,
            twist: 45f64.to_radians(),
            edges_only: true,
            ..LoftOptions::default()
        };
        let mut e = Entities::new();
        // Twisted sides would need Auto-Tri; with no faces they do not.
        loft(
            &mut e,
            &path,
            &[square_at(0.0, 1.0), square_at(10.0, 2.0)],
            DVec3::Y,
            options,
        )
        .unwrap();
        assert_eq!(e.face_count(), 0);
        // Rings of four, and four lines along each stretch between them.
        let rings = e.edge_count();
        assert!(rings >= 4 * 5 + 4 * 4, "{rings}");
        // By spline too.
        let mut e = Entities::new();
        loft_spline(
            &mut e,
            &[square_at(0.0, 1.0), square_at(10.0, 1.0)],
            &[],
            DVec3::Y,
            LoftOptions {
                edges_only: true,
                ..LoftOptions::default()
            },
        )
        .unwrap();
        assert_eq!(e.face_count(), 0);
        assert!(e.edge_count() >= 4 + 4 + 4);
    }

    #[test]
    fn segments_add_rings_and_twist_needs_auto_tri() {
        let path = vec![DVec3::ZERO, DVec3::new(0.0, 0.0, 10.0)];
        let mut e = Entities::new();
        let options = LoftOptions {
            segments: 3,
            ..LoftOptions::default()
        };
        loft(
            &mut e,
            &path,
            &[square_at(0.0, 1.0), square_at(10.0, 1.0)],
            DVec3::Y,
            options,
        )
        .unwrap();
        assert_eq!(e.face_count(), 4 * 4 + 2);
        assert!(closed_solid(&e));
        let twisted = LoftOptions {
            twist: 45f64.to_radians(),
            ..LoftOptions::default()
        };
        let mut e = Entities::new();
        assert!(matches!(
            loft(
                &mut e,
                &path,
                &[square_at(0.0, 1.0), square_at(10.0, 1.0)],
                DVec3::Y,
                twisted,
            ),
            Err(ModelError::NotFlat(_))
        ));
        let mut e = Entities::new();
        smith_model::with_auto_tri(|| {
            loft(
                &mut e,
                &path,
                &[square_at(0.0, 1.0), square_at(10.0, 1.0)],
                DVec3::Y,
                twisted,
            )
        })
        .unwrap();
        assert!(closed_solid(&e));
    }

    #[test]
    fn loft_by_spline_runs_straight_between_facing_profiles_and_curves_otherwise() {
        // Two squares facing each other along Z: a straight loft.
        let mut e = Entities::new();
        let options = LoftOptions {
            segments: 4,
            ..LoftOptions::default()
        };
        loft_spline(
            &mut e,
            &[square_at(0.0, 1.0), square_at(10.0, 1.0)],
            &[],
            DVec3::Y,
            options,
        )
        .unwrap();
        assert!(closed_solid(&e));
        assert_eq!(e.face_count(), 4 * 5 + 2);
        // The second square moved aside and stood upright: a curve.
        let side = vec![
            DVec3::new(10.0, -1.0, 9.0),
            DVec3::new(10.0, 1.0, 9.0),
            DVec3::new(10.0, 1.0, 11.0),
            DVec3::new(10.0, -1.0, 11.0),
        ];
        let mut e = Entities::new();
        smith_model::with_auto_tri(|| {
            loft_spline(&mut e, &[square_at(0.0, 1.0), side], &[], DVec3::Y, options)
        })
        .unwrap();
        assert!(closed_solid(&e));
        // Two squares side by side, both facing up: an arch, leaving the
        // first upward and coming down into the second.
        let beside: Vec<DVec3> = square_at(0.0, 1.0)
            .iter()
            .map(|p| *p + DVec3::new(10.0, 0.0, 0.0))
            .collect();
        let mut e = Entities::new();
        smith_model::with_auto_tri(|| {
            loft_spline(
                &mut e,
                &[square_at(0.0, 1.0), beside],
                &[],
                DVec3::Z,
                options,
            )
        })
        .unwrap();
        assert!(closed_solid(&e));
        let top = e
            .vertex_ids()
            .filter_map(|v| e.vertex(v).map(|x| x.position().z))
            .fold(f64::MIN, f64::max);
        assert!(top > 2.0, "no arch: top {top}");
        // Two profiles with one centre cannot loft.
        let mut e = Entities::new();
        assert!(matches!(
            loft_spline(
                &mut e,
                &[square_at(0.0, 1.0), square_at(0.0, 2.0)],
                &[],
                DVec3::Y,
                options
            ),
            Err(ModelError::DoesNotFit(_))
        ));
    }

    #[test]
    fn a_second_path_turns_and_scales_the_profile_to_span_both() {
        // Path 1 up Z; Path 2 starts 2 mm away along X and ends 4 mm away:
        // the square doubles in size on the way.
        let path = vec![DVec3::ZERO, DVec3::new(0.0, 0.0, 10.0)];
        let path2 = vec![DVec3::new(2.0, 0.0, 0.0), DVec3::new(4.0, 0.0, 10.0)];
        let mut e = Entities::new();
        sweep_shaped(
            &mut e,
            &path,
            false,
            &SweepSection::Outline(square_at(0.0, 1.0)),
            Some(&path2),
            DVec3::Y,
            SweepOptions::default(),
        )
        .unwrap();
        assert!(closed_solid(&e));
        assert!(e.find_vertex(DVec3::new(2.0, 2.0, 10.0)).is_some());
        // Starting on the first path is refused.
        let mut e = Entities::new();
        assert!(matches!(
            sweep_shaped(
                &mut e,
                &path,
                false,
                &SweepSection::Outline(square_at(0.0, 1.0)),
                Some(&[DVec3::ZERO, DVec3::new(1.0, 0.0, 10.0)]),
                DVec3::Y,
                SweepOptions::default(),
            ),
            Err(ModelError::DoesNotFit(_))
        ));
    }

    #[test]
    fn bare_edges_make_a_surface_with_no_caps() {
        let path = vec![
            DVec3::ZERO,
            DVec3::new(0.0, 0.0, 10.0),
            DVec3::new(10.0, 0.0, 20.0),
        ];
        // An open line across the path's start: a ribbon of two quads.
        let mut e = Entities::new();
        sweep_shaped(
            &mut e,
            &path,
            false,
            &SweepSection::Edges {
                points: vec![DVec3::ZERO, DVec3::new(0.0, 3.0, 0.0)],
                closed: false,
            },
            None,
            DVec3::Y,
            SweepOptions::default(),
        )
        .unwrap();
        assert_eq!(e.face_count(), 2);
        // A closed chain: a tube with open ends.
        let mut e = Entities::new();
        sweep_shaped(
            &mut e,
            &path,
            false,
            &SweepSection::Edges {
                points: square_at(0.0, 1.0),
                closed: true,
            },
            None,
            DVec3::Y,
            SweepOptions::default(),
        )
        .unwrap();
        assert_eq!(e.face_count(), 2 * 4);
        // An open start lofted into an open end with more points.
        let mut e = Entities::new();
        loft_shaped(
            &mut e,
            &path[..2],
            &[
                vec![DVec3::ZERO, DVec3::new(0.0, 4.0, 0.0)],
                vec![
                    DVec3::new(0.0, 0.0, 10.0),
                    DVec3::new(0.0, 2.0, 10.0),
                    DVec3::new(0.0, 4.0, 10.0),
                ],
            ],
            &[],
            DVec3::Y,
            LoftOptions::default(),
            None,
            Some(false),
            false,
        )
        .unwrap();
        assert_eq!(e.face_count(), 2);
    }
}
