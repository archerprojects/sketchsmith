//! SketchSmith — debug scenes | v1.0.0 | Thursday 12 November 2026
//!
//! Built-in scenes for checking the canvas:
//!
//! * **Test scene** — a 20 mm cube, a plate with a hole, a circle curve, and a
//!   0.5 mm ring of small squares for checking zoom-to-cursor at jewelry scale.
//! * **Stress scene** — about 100 000 edges for frame-rate checks.
//!   It is generated straight into display buffers (render-only), because the
//!   model's edge search is a linear scan.

use smith_geom::DVec3;
use smith_geom::curve::{Circle, Curve, CurveFrame};
use smith_model::{Document, ModelError};
use smith_render::mesh::{EDGE_COLOR, FaceVertex, LineVertex, SceneMesh};

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// Builds the test scene as one undoable step.
///
/// # Errors
/// Only on an internal geometry error, which the tests rule out.
pub fn test_scene() -> Result<Document, ModelError> {
    let mut doc = Document::new();
    doc.transaction("Test scene", |d| {
        let m = d.root_mut();
        // 20 mm cube.
        let s = 20.0;
        let sq = |z: f64| [v(0.0, 0.0, z), v(s, 0.0, z), v(s, s, z), v(0.0, s, z)];
        m.add_polyline(&sq(0.0), true)?;
        m.add_polyline(&sq(s), true)?;
        for (x, y) in [(0.0, 0.0), (s, 0.0), (s, s), (0.0, s)] {
            m.add_line(v(x, y, 0.0), v(x, y, s))?;
        }
        // Plate with a hole.
        m.add_polyline(
            &[
                v(40.0, 0.0, 0.0),
                v(80.0, 0.0, 0.0),
                v(80.0, 30.0, 0.0),
                v(40.0, 30.0, 0.0),
            ],
            true,
        )?;
        m.add_polyline(
            &[
                v(50.0, 10.0, 0.0),
                v(70.0, 10.0, 0.0),
                v(70.0, 20.0, 0.0),
                v(50.0, 20.0, 0.0),
            ],
            true,
        )?;
        // A circle curve standing upright.
        let frame = CurveFrame::new(v(10.0, 50.0, 15.0), DVec3::X, DVec3::Y)?;
        m.add_curve(Curve::Circle(Circle::new(frame, 12.0)?), 48)?;
        // Jewelry scale: a ring of 0.5 mm squares.
        for i in 0..12 {
            let a = std::f64::consts::TAU * f64::from(i) / 12.0;
            let c = v(60.0 + 3.0 * a.cos(), 50.0 + 3.0 * a.sin(), 0.0);
            let h = 0.25;
            m.add_polyline(
                &[
                    c + v(-h, -h, 0.0),
                    c + v(h, -h, 0.0),
                    c + v(h, h, 0.0),
                    c + v(-h, h, 0.0),
                ],
                true,
            )?;
        }
        Ok(())
    })?;
    Ok(doc)
}

/// Generates a render-only grid of boxes with about `target_edges` edges.
pub fn stress_mesh(target_edges: usize) -> SceneMesh {
    let boxes = target_edges.div_ceil(12);
    let side = (boxes as f64).cbrt().ceil() as usize;
    let size = 4.0f32;
    let pitch = 6.0f32;
    let half = side as f32 * pitch * 0.5;
    let mut faces = Vec::with_capacity(boxes * 36);
    let mut edges = Vec::with_capacity(boxes * 24);
    let corners: [[f32; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];
    // Quads (CCW from outside) with their normals.
    let quads: [([usize; 4], [f32; 3]); 6] = [
        ([0, 3, 2, 1], [0.0, 0.0, -1.0]),
        ([4, 5, 6, 7], [0.0, 0.0, 1.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
        ([2, 3, 7, 6], [0.0, 1.0, 0.0]),
        ([1, 2, 6, 5], [1.0, 0.0, 0.0]),
        ([3, 0, 4, 7], [-1.0, 0.0, 0.0]),
    ];
    let edge_pairs = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    let mut n = 0;
    'outer: for i in 0..side {
        for j in 0..side {
            for k in 0..side {
                if n == boxes {
                    break 'outer;
                }
                n += 1;
                let base = [
                    i as f32 * pitch - half,
                    j as f32 * pitch - half,
                    k as f32 * pitch,
                ];
                let p = |c: usize| {
                    [
                        base[0] + corners[c][0] * size,
                        base[1] + corners[c][1] * size,
                        base[2] + corners[c][2] * size,
                    ]
                };
                for (q, normal) in quads {
                    for idx in [q[0], q[1], q[2], q[0], q[2], q[3]] {
                        faces.push(FaceVertex::plain(p(idx), normal, 0.0));
                    }
                }
                for (a, b) in edge_pairs {
                    for (c, along) in [(a, 0.0), (b, size)] {
                        edges.push(LineVertex {
                            position: p(c),
                            color: EDGE_COLOR,
                            along,
                            dashed: 0.0,
                        });
                    }
                }
            }
        }
    }
    SceneMesh {
        origin: DVec3::ZERO,
        faces,
        edges,
        ..SceneMesh::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scene_is_valid() {
        let doc = test_scene().unwrap();
        doc.root().validate().unwrap();
        assert!(doc.root().face_count() >= 6 + 2 + 1 + 12);
        assert_eq!(doc.root().curve_count(), 1);
    }

    #[test]
    fn stress_mesh_reaches_target() {
        let mesh = stress_mesh(100_000);
        assert!(mesh.edges.len() / 2 >= 100_000);
    }
}
