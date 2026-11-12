//! SketchSmith — Skinning | v1.0.0 | Thursday 12 November 2026
//!
//! Skinning covers a frame of edges with surfaces, as Curviloft's skinning
//! does.
//!
//! * **The frame** is a set of line segments. Points closer than a
//!   thousandth of a millimetre are one. A **side** runs between corners:
//!   points where three or more lines meet, where a line ends, or where the
//!   frame turns by more than 60° (so an arc stays one side and a square has
//!   four).
//! * **Cells** are the loops of three or four sides; a four-sided loop with
//!   a side across it (two cells side by side) is not one.
//! * **The surface** over each cell is a Coons patch: the blend of its sides
//!   (a three-sided cell is a four-sided one with its fourth side shrunk to
//!   the corner). It is cut into Sampling × Sampling pieces; where two
//!   opposite sides have the same number of points (a helix and a line
//!   divided to match), their own points are used instead, so the skin
//!   meets them exactly.
//! * **Pieces** that are flat are single faces; bent ones are folded into
//!   two triangles across a soft, smooth diagonal, without asking (a skin is
//!   curved by nature). The lines inside a cell are soft and smooth, so the
//!   skin reads as one surface; its border stays drawn. With **Edges Only**
//!   every line is kept and drawn, and there are no faces.

use std::collections::{HashMap, HashSet};

use smith_geom::DVec3;
use smith_model::{Entities, FaceId, ModelError};

use crate::modify::{face_through, orient};

/// Points closer than this are one point of the frame (mm).
const MERGE: f64 = 1e-3;

/// A side ends where the frame turns further than this (the cosine of
/// 60°).
const CORNER_COS: f64 = 0.5;

/// What a skin covered: its cells, and the frame's segments in none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Skinned {
    /// Cells covered.
    pub cells: usize,
    /// Segments of the frame in no cell of three or four sides.
    pub unused: usize,
}

/// A side of the frame: its corners (node indices) and its points from the
/// first corner to the second.
#[derive(Debug, Clone)]
struct Side {
    a: usize,
    b: usize,
    points: Vec<DVec3>,
    /// How many frame segments it is made of.
    segments: usize,
}

/// The frame as sides between corners, and the segments in no side (closed
/// loops with no corner).
fn sides_of(segments: &[[DVec3; 2]]) -> (Vec<Side>, usize) {
    let key = |p: DVec3| {
        let q = (p / MERGE).round();
        (q.x as i64, q.y as i64, q.z as i64)
    };
    let mut index: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut nodes: Vec<DVec3> = Vec::new();
    let mut node = |p: DVec3| -> usize {
        *index.entry(key(p)).or_insert_with(|| {
            nodes.push(p);
            nodes.len() - 1
        })
    };
    let mut links: HashSet<(usize, usize)> = HashSet::new();
    for [a, b] in segments {
        let (x, y) = (node(*a), node(*b));
        if x != y {
            links.insert((x.min(y), x.max(y)));
        }
    }
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (x, y) in &links {
        next[*x].push(*y);
        next[*y].push(*x);
    }
    // Corners: not two lines, or two meeting at an angle.
    let corner: Vec<bool> = (0..nodes.len())
        .map(|n| {
            if next[n].len() != 2 {
                return true;
            }
            let into = (nodes[n] - nodes[next[n][0]]).normalize_or_zero();
            let out = (nodes[next[n][1]] - nodes[n]).normalize_or_zero();
            into.dot(out) < CORNER_COS
        })
        .collect();
    let mut walked: HashSet<(usize, usize)> = HashSet::new();
    let mut sides = Vec::new();
    for start in (0..nodes.len()).filter(|n| corner[*n]) {
        for first in next[start].clone() {
            if walked.contains(&(start.min(first), start.max(first))) {
                continue;
            }
            let mut path = vec![start];
            let (mut prev, mut at) = (start, first);
            walked.insert((prev.min(at), prev.max(at)));
            path.push(at);
            while !corner[at] {
                let Some(n) = next[at].iter().copied().find(|n| *n != prev) else {
                    break;
                };
                prev = at;
                at = n;
                walked.insert((prev.min(at), prev.max(at)));
                path.push(at);
            }
            sides.push(Side {
                a: start,
                b: at,
                segments: path.len() - 1,
                points: path.iter().map(|n| nodes[*n]).collect(),
            });
        }
    }
    let loose = links.len() - walked.len();
    (sides, loose)
}

/// The frame's cells: loops of three or four sides (as side indices, in
/// order round the loop, each with whether it runs backwards), a
/// four-sided loop with a side across it left out.
fn cells_of(sides: &[Side]) -> Vec<Vec<(usize, bool)>> {
    // The sides at each corner, with the corner at their other end.
    let mut at: HashMap<usize, Vec<(usize, usize, bool)>> = HashMap::new();
    for (i, s) in sides.iter().enumerate() {
        if s.a == s.b {
            continue;
        }
        at.entry(s.a).or_default().push((i, s.b, false));
        at.entry(s.b).or_default().push((i, s.a, true));
    }
    let joined = |x: usize, y: usize| {
        at.get(&x)
            .is_some_and(|v| v.iter().any(|(_, other, _)| *other == y))
    };
    let mut seen: HashSet<Vec<usize>> = HashSet::new();
    let mut out = Vec::new();
    let mut keep = |cell: Vec<(usize, bool)>, out: &mut Vec<Vec<(usize, bool)>>| {
        let mut ids: Vec<usize> = cell.iter().map(|(s, _)| *s).collect();
        ids.sort_unstable();
        if seen.insert(ids) {
            out.push(cell);
        }
    };
    let empty = Vec::new();
    for (&n0, firsts) in &at {
        for &(s1, n1, r1) in firsts {
            for &(s2, n2, r2) in at.get(&n1).unwrap_or(&empty) {
                if s2 == s1 || n2 == n0 {
                    continue;
                }
                for &(s3, n3, r3) in at.get(&n2).unwrap_or(&empty) {
                    if s3 == s1 || s3 == s2 {
                        continue;
                    }
                    if n3 == n0 {
                        keep(vec![(s1, r1), (s2, r2), (s3, r3)], &mut out);
                        continue;
                    }
                    if n3 == n1 {
                        continue;
                    }
                    for &(s4, n4, r4) in at.get(&n3).unwrap_or(&empty) {
                        if n4 != n0 || [s1, s2, s3].contains(&s4) {
                            continue;
                        }
                        // Two cells side by side are not one.
                        if joined(n0, n2) || joined(n1, n3) {
                            continue;
                        }
                        keep(vec![(s1, r1), (s2, r2), (s3, r3), (s4, r4)], &mut out);
                    }
                }
            }
        }
    }
    out
}

/// The point a fraction `t` (0–1) of the way along `pts` by length.
fn along(pts: &[DVec3], t: f64) -> DVec3 {
    let total: f64 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
    if total <= 0.0 || pts.len() < 2 {
        return pts[0];
    }
    let mut want = t.clamp(0.0, 1.0) * total;
    for w in pts.windows(2) {
        let len = w[0].distance(w[1]);
        if want <= len {
            return w[0].lerp(w[1], want / len.max(1e-12));
        }
        want -= len;
    }
    pts[pts.len() - 1]
}

/// The fractions along `pts` (by length) of each of its points.
fn fractions(pts: &[DVec3]) -> Vec<f64> {
    let total: f64 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
    let mut out = vec![0.0];
    let mut run = 0.0;
    for w in pts.windows(2) {
        run += w[0].distance(w[1]);
        out.push(if total > 0.0 { run / total } else { 0.0 });
    }
    out
}

/// One direction of a patch between two opposite sides: for each step, the
/// point on each and the fraction across. Their own points when both have
/// the same number (and `own`), else `n` even steps by length.
fn steps(one: &[DVec3], other: &[DVec3], n: usize, own: bool) -> Vec<(DVec3, DVec3, f64)> {
    if own && one.len() == other.len() && one.len() > 2 {
        let (f1, f2) = (fractions(one), fractions(other));
        return (0..one.len())
            .map(|i| (one[i], other[i], (f1[i] + f2[i]) / 2.0))
            .collect();
    }
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            (along(one, t), along(other, t), t)
        })
        .collect()
}

/// The Coons patch over a cell, as a grid of points, rows from the first
/// side to the opposite one.
fn patch(sides: &[Vec<DVec3>], n: usize) -> Vec<Vec<DVec3>> {
    let rev = |p: &Vec<DVec3>| p.iter().rev().copied().collect::<Vec<_>>();
    // Bottom c0 and top c1 run the same way; left d0 and right d1 too.
    let (c0, d1, c1, d0) = match sides {
        [s0, s1, s2, s3] => (s0.clone(), s1.clone(), rev(s2), rev(s3)),
        [s0, s1, s2] => {
            let apex = s1[s1.len() - 1];
            (s0.clone(), s1.clone(), vec![apex, apex], rev(s2))
        }
        _ => return Vec::new(),
    };
    let four = sides.len() == 4;
    let us = steps(&c0, &c1, n, four);
    let vs = steps(&d0, &d1, n, true);
    let (p00, p10) = (c0[0], c0[c0.len() - 1]);
    let (p01, p11) = (c1[0], c1[c1.len() - 1]);
    vs.iter()
        .map(|(l, r, v)| {
            us.iter()
                .map(|(b, t, u)| {
                    (1.0 - v) * *b + *v * *t + (1.0 - u) * *l + *u * *r
                        - ((1.0 - u) * (1.0 - v) * p00
                            + u * (1.0 - v) * p10
                            + (1.0 - u) * v * p01
                            + u * v * p11)
                })
                .collect()
        })
        .collect()
}

/// How many segments of the frame are in no cell, and how many cells it
/// has, without building anything.
pub fn frame_cells(segments: &[[DVec3; 2]]) -> Skinned {
    let (sides, loose) = sides_of(segments);
    let cells = cells_of(&sides);
    let used: HashSet<usize> = cells.iter().flatten().map(|(s, _)| *s).collect();
    let unused = loose
        + sides
            .iter()
            .enumerate()
            .filter(|(i, _)| !used.contains(i))
            .map(|(_, s)| s.segments)
            .sum::<usize>();
    Skinned {
        cells: cells.len(),
        unused,
    }
}

/// Skins the frame `segments` into `entities` (see the module notes):
/// `sampling` pieces each way across a cell (1–64), or only the lines with
/// `edges_only`.
///
/// # Errors
/// [`ModelError::DoesNotFit`] when the frame has no cell of three or four
/// sides; [`ModelError::InvalidData`] for points that are not finite.
pub fn skin(
    entities: &mut Entities,
    segments: &[[DVec3; 2]],
    sampling: u32,
    edges_only: bool,
) -> Result<Skinned, ModelError> {
    if segments.iter().flatten().any(|p| !p.is_finite()) {
        return Err(ModelError::InvalidData("frame point".into()));
    }
    let (sides, _) = sides_of(segments);
    let cells = cells_of(&sides);
    if cells.is_empty() {
        return Err(ModelError::DoesNotFit("no cell of three or four sides"));
    }
    let n = sampling.clamp(1, 64) as usize;
    let grids: Vec<Vec<Vec<DVec3>>> = cells
        .iter()
        .map(|cell| {
            let pts: Vec<Vec<DVec3>> = cell
                .iter()
                .map(|(s, back)| {
                    let mut p = sides[*s].points.clone();
                    if *back {
                        p.reverse();
                    }
                    p
                })
                .collect();
            patch(&pts, n)
        })
        .collect();

    let same = |a: DVec3, b: DVec3| a.distance(b) < 1e-9;
    let line = |entities: &mut Entities, a: DVec3, b: DVec3| -> Result<Vec<_>, ModelError> {
        if same(a, b) {
            return Ok(Vec::new());
        }
        match entities.add_line(a, b) {
            Ok(e) => Ok(e),
            Err(ModelError::ZeroLength) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    };
    // The lines: borders drawn, the rest soft and smooth (with faces).
    let mut inner = Vec::new();
    for g in &grids {
        let (rows, cols) = (g.len(), g[0].len());
        for j in 0..rows {
            for i in 0..cols {
                let border_row = j == 0 || j == rows - 1;
                let border_col = i == 0 || i == cols - 1;
                if i + 1 < cols {
                    let made = line(entities, g[j][i], g[j][i + 1])?;
                    if !border_row {
                        inner.extend(made);
                    }
                }
                if j + 1 < rows {
                    let made = line(entities, g[j][i], g[j + 1][i])?;
                    if !border_col {
                        inner.extend(made);
                    }
                }
            }
        }
    }
    // Each piece: its distinct corners, and whether it is flat.
    let flat = |pts: &[DVec3]| {
        smith_geom::plane::Plane::fit_loop(pts).is_ok_and(|p| p.max_deviation(pts) <= 1e-6)
    };
    let mut pieces: Vec<Vec<DVec3>> = Vec::new();
    for g in &grids {
        for j in 0..g.len() - 1 {
            for i in 0..g[0].len() - 1 {
                // A three-sided cell's last row meets at its corner.
                let mut q: Vec<DVec3> = Vec::new();
                for p in [g[j][i], g[j][i + 1], g[j + 1][i + 1], g[j + 1][i]] {
                    if q.last().is_none_or(|l| !same(*l, p)) {
                        q.push(p);
                    }
                }
                if q.len() > 1 && same(q[0], q[q.len() - 1]) {
                    q.pop();
                }
                if q.len() >= 3 {
                    pieces.push(q);
                }
            }
        }
    }
    if edges_only {
        let made: Vec<FaceId> = entities.face_ids().collect();
        for f in made {
            entities.erase_face(f)?;
        }
        return Ok(frame_cells(segments));
    }
    // Bent pieces folded across a soft diagonal.
    let mut folds = Vec::new();
    let mut parts: Vec<Vec<DVec3>> = Vec::new();
    for q in pieces {
        if q.len() == 4 && !flat(&q) {
            folds.extend(line(entities, q[0], q[2])?);
            parts.push(vec![q[0], q[1], q[2]]);
            parts.push(vec![q[0], q[2], q[3]]);
        } else {
            parts.push(q);
        }
    }
    entities.soften_edges(&inner);
    entities.soften_edges(&folds);
    // Keep only the pieces: the sticky model fills other flat loops too.
    let mut keep: HashSet<FaceId> = HashSet::new();
    for p in &parts {
        if let Some(f) = face_through(entities, p) {
            keep.insert(f);
            let n = (p[1] - p[0]).cross(p[2] - p[0]);
            orient(entities, f, n)?;
        }
    }
    let extra: Vec<FaceId> = entities.face_ids().filter(|f| !keep.contains(f)).collect();
    for f in extra {
        entities.erase_face(f)?;
    }
    Ok(frame_cells(segments))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs(pts: &[DVec3], closed: bool) -> Vec<[DVec3; 2]> {
        let mut out: Vec<[DVec3; 2]> = pts.windows(2).map(|w| [w[0], w[1]]).collect();
        if closed {
            out.push([pts[pts.len() - 1], pts[0]]);
        }
        out
    }

    fn closed_surface_edges_ok(e: &Entities) -> bool {
        e.edge_ids().all(|x| {
            e.edge(x)
                .is_some_and(|y| (1..=2).contains(&y.faces().len()))
        })
    }

    #[test]
    fn a_square_of_lines_is_one_flat_cell() {
        let sq = [
            DVec3::ZERO,
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 10.0, 0.0),
            DVec3::new(0.0, 10.0, 0.0),
        ];
        let mut e = Entities::new();
        let out = skin(&mut e, &segs(&sq, true), 4, false).unwrap();
        assert_eq!(
            out,
            Skinned {
                cells: 1,
                unused: 0
            }
        );
        // Flat: sixteen square pieces.
        assert_eq!(e.face_count(), 16);
        assert!(
            e.face_ids()
                .all(|f| e.face(f).is_some_and(|x| x.plane().normal().z.abs() > 0.99))
        );
    }

    #[test]
    fn a_helix_and_a_line_divided_to_match_skin_point_to_point() {
        // A turn of helix up 10 mm in 12 pieces, a centre line in 12, and
        // a line across at each end.
        let helix: Vec<DVec3> = (0..=12)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 12.0;
                DVec3::new(5.0 * a.cos(), 5.0 * a.sin(), 10.0 * f64::from(i) / 12.0)
            })
            .collect();
        let centre: Vec<DVec3> = (0..=12)
            .map(|i| DVec3::new(0.0, 0.0, 10.0 * f64::from(i) / 12.0))
            .collect();
        let mut frame = segs(&helix, false);
        frame.extend(segs(&centre, false));
        frame.push([helix[0], centre[0]]);
        frame.push([helix[12], centre[12]]);
        let mut e = Entities::new();
        let out = skin(&mut e, &frame, 8, false).unwrap();
        assert_eq!(out.cells, 1);
        // Every helix and centre point is a corner of the skin.
        for p in helix.iter().chain(&centre) {
            assert!(e.find_vertex(*p).is_some(), "{p}");
        }
        assert!(e.face_count() > 0);
        assert!(closed_surface_edges_ok(&e));
    }

    #[test]
    fn a_dome_of_arcs_over_an_octagon_has_eight_three_sided_cells() {
        let base: Vec<DVec3> = (0..8)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 8.0;
                DVec3::new(10.0 * a.cos(), 10.0 * a.sin(), 0.0)
            })
            .collect();
        let apex = DVec3::new(0.0, 0.0, 10.0);
        let mut frame = segs(&base, true);
        for p in &base {
            // A quarter arc from the corner up to the apex, in 6 pieces.
            let arc: Vec<DVec3> = (0..=6)
                .map(|k| {
                    let t = std::f64::consts::FRAC_PI_2 * f64::from(k) / 6.0;
                    *p * t.cos() + apex * t.sin()
                })
                .collect();
            frame.extend(segs(&arc, false));
        }
        let mut e = Entities::new();
        let out = skin(&mut e, &frame, 6, false).unwrap();
        assert_eq!(
            out,
            Skinned {
                cells: 8,
                unused: 0
            }
        );
        assert!(e.face_count() > 8);
        assert!(closed_surface_edges_ok(&e));
    }

    #[test]
    fn a_pentagon_has_no_cell_and_edges_only_makes_no_faces() {
        let penta: Vec<DVec3> = (0..5)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 5.0;
                DVec3::new(10.0 * a.cos(), 10.0 * a.sin(), 0.0)
            })
            .collect();
        let mut e = Entities::new();
        assert!(matches!(
            skin(&mut e, &segs(&penta, true), 4, false),
            Err(ModelError::DoesNotFit(_))
        ));
        assert_eq!(frame_cells(&segs(&penta, true)).unused, 5);
        // A square beside a loose line: the line is counted.
        let sq = [
            DVec3::ZERO,
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 10.0, 5.0),
            DVec3::new(0.0, 10.0, 0.0),
        ];
        let mut frame = segs(&sq, true);
        frame.push([DVec3::new(50.0, 0.0, 0.0), DVec3::new(60.0, 0.0, 0.0)]);
        let mut w = Entities::new();
        let out = skin(&mut w, &frame, 4, true).unwrap();
        assert_eq!(
            out,
            Skinned {
                cells: 1,
                unused: 1
            }
        );
        assert_eq!(w.face_count(), 0);
        assert!(w.edge_count() >= 4 * 5);
        // Faced, the bent square folds without asking.
        let mut f = Entities::new();
        skin(&mut f, &frame, 4, false).unwrap();
        assert!(f.face_count() > 16);
    }
}
