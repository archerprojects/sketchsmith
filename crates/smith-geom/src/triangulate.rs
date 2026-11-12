//! SketchSmith — polygon triangulation | v1.0.0 | Thursday 12 November 2026
//!
//! Triangulates a planar polygon with holes, given in 2D plane coordinates, for
//! display, picking, and mesh export. Faces in the model are n-gons;
//! triangles exist only at these boundaries.
//!
//! Method: a constrained Delaunay triangulation (the `spade` crate) of every
//! loop vertex, with every loop edge added as a constraint, followed by keeping
//! only the triangles whose centroid lies inside the outer loop and outside all
//! holes. Constrained Delaunay avoids the long slivers ear-clipping produces on
//! curved outlines, which matters for smooth shading in Present mode.

use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};

use glam::DVec2;

/// Triangulates `outer` (any winding) with `holes`. Returns index triples into
/// the concatenation `outer ++ holes[0] ++ holes[1] ++ …`, each wound to match
/// the outer loop's winding. Degenerate input yields an empty list rather than
/// an error: a face that cannot be triangulated is simply not drawn.
pub fn triangulate_polygon(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Vec<[usize; 3]> {
    if outer.len() < 3 {
        return Vec::new();
    }
    let loops: Vec<&[DVec2]> = std::iter::once(outer)
        .chain(holes.iter().map(Vec::as_slice))
        .collect();
    let points: Vec<DVec2> = loops.iter().flat_map(|l| l.iter().copied()).collect();
    if points.iter().any(|p| !p.is_finite()) {
        return Vec::new();
    }

    let mut cdt: ConstrainedDelaunayTriangulation<Point2<f64>> =
        ConstrainedDelaunayTriangulation::new();
    // CDT vertex index → input point index. Duplicate points share one CDT vertex.
    let mut handles = Vec::with_capacity(points.len());
    let mut back = std::collections::HashMap::new();
    for (i, p) in points.iter().enumerate() {
        let Ok(h) = cdt.insert(Point2::new(p.x, p.y)) else {
            return Vec::new();
        };
        back.entry(h.index()).or_insert(i);
        handles.push(h);
    }
    let mut start = 0;
    for l in &loops {
        for k in 0..l.len() {
            let a = handles[start + k];
            let b = handles[start + (k + 1) % l.len()];
            if a != b && cdt.can_add_constraint(a, b) {
                cdt.add_constraint(a, b);
            }
        }
        start += l.len();
    }

    let outer_ccw = signed_area(outer) > 0.0;
    let mut out = Vec::new();
    for face in cdt.inner_faces() {
        let vs = face.vertices();
        let pos = vs.map(|v| DVec2::new(v.position().x, v.position().y));
        let centroid = (pos[0] + pos[1] + pos[2]) / 3.0;
        if !inside(centroid, outer) || holes.iter().any(|h| inside(centroid, h)) {
            continue;
        }
        let Some(idx) = vs
            .iter()
            .map(|v| back.get(&v.fix().index()).copied())
            .collect::<Option<Vec<usize>>>()
        else {
            continue;
        };
        let tri_ccw = (pos[1] - pos[0]).perp_dot(pos[2] - pos[0]) > 0.0;
        if tri_ccw == outer_ccw {
            out.push([idx[0], idx[1], idx[2]]);
        } else {
            out.push([idx[0], idx[2], idx[1]]);
        }
    }
    out
}

fn signed_area(pts: &[DVec2]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| pts[i].perp_dot(pts[(i + 1) % n]))
        .sum::<f64>()
        * 0.5
}

/// Even-odd point-in-polygon test.
fn inside(q: DVec2, poly: &[DVec2]) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(pts: &[DVec2], tris: &[[usize; 3]]) -> f64 {
        tris.iter()
            .map(|t| (pts[t[1]] - pts[t[0]]).perp_dot(pts[t[2]] - pts[t[0]]) * 0.5)
            .sum()
    }

    fn sq(x0: f64, y0: f64, s: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(x0, y0),
            DVec2::new(x0 + s, y0),
            DVec2::new(x0 + s, y0 + s),
            DVec2::new(x0, y0 + s),
        ]
    }

    #[test]
    fn square_makes_two_triangles() {
        let outer = sq(0.0, 0.0, 10.0);
        let tris = triangulate_polygon(&outer, &[]);
        assert_eq!(tris.len(), 2);
        assert!((area(&outer, &tris) - 100.0).abs() < 1e-12);
    }

    #[test]
    fn square_with_hole_covers_exact_area_and_keeps_winding() {
        let outer = sq(0.0, 0.0, 10.0);
        let mut hole = sq(3.0, 3.0, 4.0);
        hole.reverse();
        let tris = triangulate_polygon(&outer, std::slice::from_ref(&hole));
        let all: Vec<DVec2> = outer.iter().chain(hole.iter()).copied().collect();
        assert!(
            (area(&all, &tris) - 84.0).abs() < 1e-12,
            "every triangle wound like the outer loop"
        );
    }

    #[test]
    fn non_convex_l_shape() {
        let l = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(2.0, 1.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(1.0, 2.0),
            DVec2::new(0.0, 2.0),
        ];
        let tris = triangulate_polygon(&l, &[]);
        assert_eq!(tris.len(), 4);
        assert!((area(&l, &tris) - 3.0).abs() < 1e-12);
    }

    #[test]
    fn clockwise_outer_yields_clockwise_triangles() {
        let mut outer = sq(0.0, 0.0, 1.0);
        outer.reverse();
        let tris = triangulate_polygon(&outer, &[]);
        assert!((area(&outer, &tris) + 1.0).abs() < 1e-12);
    }

    #[test]
    fn sub_millimetre_and_degenerate() {
        let outer = sq(500.0, 500.0, 0.001);
        assert!((area(&outer, &triangulate_polygon(&outer, &[])) - 1e-6).abs() < 1e-15);
        assert!(triangulate_polygon(&outer[..2], &[]).is_empty());
    }
}
