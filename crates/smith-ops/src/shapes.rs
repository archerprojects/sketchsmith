//! SketchSmith — Shapes | v1.0.0 | Thursday 12 November 2026
//!
//! Ready-made solids: box, cylinder, cone, sphere and torus. Each is built
//! about its own origin, Z up, standing on the XY plane (the sphere and
//! torus resting on it), so the tool can place and turn it as a group.
//!
//! Every face is flat: boxes and cylinders have rectangles, a cone's sides
//! are triangles, a sphere's bands are isosceles trapezoids (triangles at
//! the poles), and a torus's quads lie in the plane of a tube edge and its
//! copy one step round. Each shape is a closed solid with its faces turned
//! outwards.

use smith_geom::DVec3;
pub use smith_model::ShapeSpec;
use smith_model::{Entities, FaceId, ModelError, VertexId};

use crate::modify::{face_through, orient};

/// The shape's faces as loops of points, and a point each face turns away
/// from (its centre of bulk).
fn shape_faces(spec: &ShapeSpec) -> Result<Vec<(Vec<DVec3>, DVec3)>, ModelError> {
    let positive = |x: f64| x > 0.0 && x.is_finite();
    let bad = || ModelError::InvalidData("shape size".into());
    let ring = |r: f64, z: f64, n: u32| -> Vec<DVec3> {
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / f64::from(n);
                DVec3::new(r * a.cos(), r * a.sin(), z)
            })
            .collect()
    };
    let mut out = Vec::new();
    match *spec {
        ShapeSpec::Box {
            width,
            depth,
            height,
        } => {
            if !(positive(width) && positive(depth) && positive(height)) {
                return Err(bad());
            }
            let (x, y) = (width * 0.5, depth * 0.5);
            let c = DVec3::new(0.0, 0.0, height * 0.5);
            let p = |i: f64, j: f64, k: f64| DVec3::new(i * x, j * y, k * height);
            let quads = [
                [
                    p(-1.0, -1.0, 0.0),
                    p(1.0, -1.0, 0.0),
                    p(1.0, 1.0, 0.0),
                    p(-1.0, 1.0, 0.0),
                ],
                [
                    p(-1.0, -1.0, 1.0),
                    p(1.0, -1.0, 1.0),
                    p(1.0, 1.0, 1.0),
                    p(-1.0, 1.0, 1.0),
                ],
                [
                    p(-1.0, -1.0, 0.0),
                    p(1.0, -1.0, 0.0),
                    p(1.0, -1.0, 1.0),
                    p(-1.0, -1.0, 1.0),
                ],
                [
                    p(1.0, -1.0, 0.0),
                    p(1.0, 1.0, 0.0),
                    p(1.0, 1.0, 1.0),
                    p(1.0, -1.0, 1.0),
                ],
                [
                    p(1.0, 1.0, 0.0),
                    p(-1.0, 1.0, 0.0),
                    p(-1.0, 1.0, 1.0),
                    p(1.0, 1.0, 1.0),
                ],
                [
                    p(-1.0, 1.0, 0.0),
                    p(-1.0, -1.0, 0.0),
                    p(-1.0, -1.0, 1.0),
                    p(-1.0, 1.0, 1.0),
                ],
            ];
            out.extend(quads.into_iter().map(|q| (q.to_vec(), c)));
        }
        ShapeSpec::Cylinder {
            radius,
            height,
            sides,
        } => {
            if !(positive(radius) && positive(height)) || sides < 3 {
                return Err(bad());
            }
            let c = DVec3::new(0.0, 0.0, height * 0.5);
            let (lo, hi) = (ring(radius, 0.0, sides), ring(radius, height, sides));
            out.push((lo.clone(), c));
            out.push((hi.clone(), c));
            let n = sides as usize;
            for i in 0..n {
                let j = (i + 1) % n;
                out.push((vec![lo[i], lo[j], hi[j], hi[i]], c));
            }
        }
        ShapeSpec::Cone {
            radius,
            height,
            sides,
        } => {
            if !(positive(radius) && positive(height)) || sides < 3 {
                return Err(bad());
            }
            let c = DVec3::new(0.0, 0.0, height * 0.25);
            let base = ring(radius, 0.0, sides);
            let apex = DVec3::new(0.0, 0.0, height);
            out.push((base.clone(), c));
            let n = sides as usize;
            for i in 0..n {
                out.push((vec![base[i], base[(i + 1) % n], apex], c));
            }
        }
        ShapeSpec::Sphere { radius, segments } => {
            if !positive(radius) || segments < 4 {
                return Err(bad());
            }
            let segments = segments + segments % 2;
            let bands = segments / 2;
            let c = DVec3::new(0.0, 0.0, radius);
            let lat = |b: u32| {
                let t = std::f64::consts::PI * f64::from(b) / f64::from(bands);
                (radius * t.sin(), radius - radius * t.cos())
            };
            let rings: Vec<Vec<DVec3>> = (1..bands)
                .map(|b| {
                    let (r, z) = lat(b);
                    ring(r, z, segments)
                })
                .collect();
            let (bottom, top) = (DVec3::ZERO, DVec3::new(0.0, 0.0, 2.0 * radius));
            let n = segments as usize;
            for i in 0..n {
                let j = (i + 1) % n;
                out.push((vec![bottom, rings[0][j], rings[0][i]], c));
                let last = &rings[rings.len() - 1];
                out.push((vec![top, last[i], last[j]], c));
            }
            for w in rings.windows(2) {
                for i in 0..n {
                    let j = (i + 1) % n;
                    out.push((vec![w[0][i], w[0][j], w[1][j], w[1][i]], c));
                }
            }
        }
        ShapeSpec::Torus {
            ring: big,
            tube,
            segments,
            sides,
        } => {
            if !(positive(big) && positive(tube)) || tube >= big || segments < 3 || sides < 3 {
                return Err(bad());
            }
            // Tube sections round the ring, resting on z = 0.
            let section = |k: u32| -> Vec<DVec3> {
                let a = std::f64::consts::TAU * f64::from(k) / f64::from(segments);
                let (dir, up) = (DVec3::new(a.cos(), a.sin(), 0.0), DVec3::Z);
                (0..sides)
                    .map(|s| {
                        let b = std::f64::consts::TAU * f64::from(s) / f64::from(sides);
                        dir * (big + tube * b.cos()) + up * (tube + tube * b.sin())
                    })
                    .collect()
            };
            let sections: Vec<Vec<DVec3>> = (0..segments).map(section).collect();
            let (m, k) = (segments as usize, sides as usize);
            for i in 0..m {
                let i2 = (i + 1) % m;
                let a = std::f64::consts::TAU * (i as f64 + 0.5) / segments as f64;
                // Each quad turns away from the tube's middle near it.
                let middle = DVec3::new(big * a.cos(), big * a.sin(), tube);
                for s in 0..k {
                    let s2 = (s + 1) % k;
                    let q = vec![
                        sections[i][s],
                        sections[i2][s],
                        sections[i2][s2],
                        sections[i][s2],
                    ];
                    out.push((q, middle));
                }
            }
        }
    }
    Ok(out)
}

/// Builds `spec` in `entities` about the origin (see the module
/// documentation): only its faces, turned outwards.
///
/// # Errors
/// [`ModelError::InvalidData`] for a bad size or too few sides; or as
/// [`Entities::add_line`].
pub fn build_shape(entities: &mut Entities, spec: ShapeSpec) -> Result<(), ModelError> {
    let faces = shape_faces(&spec)?;
    let faces_before: std::collections::HashSet<FaceId> = entities.face_ids().collect();
    let vertices_before: std::collections::HashSet<VertexId> = entities.vertex_ids().collect();
    for (loop_, _) in &faces {
        entities.add_polyline(loop_, true)?;
    }
    let keep: std::collections::HashSet<FaceId> = faces
        .iter()
        .filter_map(|(l, _)| face_through(entities, l))
        .collect();
    // The sticky model fills other flat loops the edges close (a cylinder's
    // or sphere's rings); only the shape's own faces stay.
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
    for (loop_, centre) in &faces {
        if let Some(f) = face_through(entities, loop_) {
            let mid = loop_.iter().copied().sum::<DVec3>() / loop_.len() as f64;
            orient(entities, f, mid - *centre)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Faces made, and edges not shared by exactly two faces.
    fn solid(spec: ShapeSpec) -> (usize, usize) {
        let mut e = Entities::new();
        build_shape(&mut e, spec).unwrap();
        let open = e
            .edge_ids()
            .filter(|x| e.edge(*x).is_some_and(|y| y.faces().len() != 2))
            .count();
        (e.face_count(), open)
    }

    #[test]
    fn every_shape_is_a_closed_solid_of_flat_faces() {
        assert_eq!(
            solid(ShapeSpec::Box {
                width: 10.0,
                depth: 6.0,
                height: 4.0
            }),
            (6, 0)
        );
        assert_eq!(
            solid(ShapeSpec::Cylinder {
                radius: 5.0,
                height: 8.0,
                sides: 24
            }),
            (26, 0)
        );
        assert_eq!(
            solid(ShapeSpec::Cone {
                radius: 5.0,
                height: 8.0,
                sides: 16
            }),
            (17, 0)
        );
        // 12 round: 6 bands — 2 × 12 pole triangles, 4 × 12 quads.
        assert_eq!(
            solid(ShapeSpec::Sphere {
                radius: 5.0,
                segments: 12
            }),
            (72, 0)
        );
        assert_eq!(
            solid(ShapeSpec::Torus {
                ring: 10.0,
                tube: 2.0,
                segments: 24,
                sides: 12
            }),
            (288, 0)
        );
    }

    #[test]
    fn faces_turn_outwards() {
        let mut e = Entities::new();
        build_shape(
            &mut e,
            ShapeSpec::Box {
                width: 2.0,
                depth: 2.0,
                height: 2.0,
            },
        )
        .unwrap();
        let centre = DVec3::new(0.0, 0.0, 1.0);
        for f in e.face_ids() {
            let face = e.face(f).unwrap();
            let p = face.plane();
            assert!(p.signed_distance(centre) < 0.0, "a face turns inwards");
        }
    }

    #[test]
    fn bad_sizes_are_refused() {
        let mut e = Entities::new();
        assert!(
            build_shape(
                &mut e,
                ShapeSpec::Torus {
                    ring: 2.0,
                    tube: 3.0,
                    segments: 12,
                    sides: 8
                }
            )
            .is_err()
        );
        assert_eq!(e.face_count(), 0);
    }
}
