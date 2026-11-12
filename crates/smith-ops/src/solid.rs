//! SketchSmith — Inspect Manifold | v1.0.0 | Thursday 12 November 2026
//!
//! Inspect Manifold: [`inspect_solid`] only finds; it changes nothing. Faces are grouped into
//! shells: faces joined through edges that exactly two faces share. Each
//! report line names what it found and selects it when clicked:
//!
//! * **Non-Manifold Edges** — edges with one face (a hole's border) or
//!   three or more (an inner wall or a fin). Edges with no face are Inspect
//!   Faces' Open Ends.
//! * **Non-Manifold Points** — vertices whose faces do not form one fan
//!   (two solids touching only at a corner).
//! * **Reversed Faces** ([`reversed_faces`]) — in each shell, the faces
//!   turned the other way from the rest: on a closed shell, those facing
//!   inward (judged by the shell's volume); on an open one, the fewer, and
//!   none when the two sides are even.
//! * **Thin Walls** — faces with another face of their shell within the
//!   Thickness straight behind them, looked for from the middle of each of
//!   the face's triangles. Every face is tried against every other face of
//!   its shell, so very large shells are slow.
//! * **Sharp Edges** — edges whose two faces point more than the Sharp
//!   Angle apart: a knife-edge.
//! * **Overhangs** — faces pointing down (world −Z) more steeply than the
//!   Overhang angle from upright, except faces along the bottom of their
//!   shell, which would sit on the print bed.
//!
//! The scope works as the other repair tools': the selection's points, or
//! everything in the context.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use smith_geom::DVec3;
use smith_geom::tolerance::TOL_MERGE;
use smith_model::{EdgeId, Entities, FaceId, ModelError, Pickable, VertexId};

use crate::cleanup::ReportLine;

/// One Inspect Manifold run.
#[derive(Debug, Clone, PartialEq)]
pub struct InspectSolid {
    /// The points it looks at, by position; `None`: everything in the
    /// context.
    pub scope: Option<Vec<DVec3>>,
    /// Thin Walls: the thinnest wall not counted, mm.
    pub thickness: f64,
    /// Sharp Edges: faces pointing further apart than this, radians.
    pub sharp: f64,
    /// Overhangs: the steepest downward slope allowed, from upright,
    /// radians.
    pub overhang: f64,
}

impl Default for InspectSolid {
    fn default() -> Self {
        Self {
            scope: None,
            thickness: 1.0,
            sharp: 160f64.to_radians(),
            overhang: 45f64.to_radians(),
        }
    }
}

/// The faces in scope: every corner within reach of a scope point.
pub fn faces_in(entities: &Entities, scope: Option<&[DVec3]>) -> Vec<FaceId> {
    let in_scope = |v: VertexId| {
        entities.vertex(v).is_some_and(|x| {
            scope.is_none_or(|s| s.iter().any(|q| q.distance(x.position()) <= TOL_MERGE))
        })
    };
    entities
        .face_ids()
        .filter(|f| {
            entities.face(*f).is_some_and(|x| {
                entities
                    .loop_vertices(x.outer())
                    .iter()
                    .all(|v| in_scope(*v))
            })
        })
        .collect()
}

/// Whether face `f` walks edge `e` end to start.
fn walks_back(entities: &Entities, f: FaceId, e: EdgeId) -> Option<bool> {
    entities
        .face(f)?
        .loops()
        .flatten()
        .find(|u| u.edge == e)
        .map(|u| u.reversed)
}

/// The faces of `faces` grouped into shells (joined through edges exactly
/// two of them share), each with every face's turn relative to the shell's
/// first face (`true`: turned the other way).
fn shells(entities: &Entities, faces: &[FaceId]) -> Vec<Vec<(FaceId, bool)>> {
    let set: BTreeSet<FaceId> = faces.iter().copied().collect();
    let mut done: BTreeMap<FaceId, bool> = BTreeMap::new();
    let mut out = Vec::new();
    for start in faces {
        if done.contains_key(start) {
            continue;
        }
        let mut shell = vec![(*start, false)];
        done.insert(*start, false);
        let mut queue = VecDeque::from([*start]);
        while let Some(f) = queue.pop_front() {
            let flip = done[&f];
            let Some(face) = entities.face(f) else {
                continue;
            };
            for u in face.loops().flatten() {
                let Some(edge) = entities.edge(u.edge) else {
                    continue;
                };
                let [a, b] = edge.faces()[..] else {
                    continue;
                };
                let g = if a == f { b } else { a };
                if g == f || !set.contains(&g) || done.contains_key(&g) {
                    continue;
                }
                // Neighbours agree when they walk the shared edge opposite
                // ways.
                let agree = walks_back(entities, g, u.edge) != Some(u.reversed);
                let g_flip = flip ^ !agree;
                done.insert(g, g_flip);
                shell.push((g, g_flip));
                queue.push_back(g);
            }
        }
        out.push(shell);
    }
    out
}

/// Whether every edge of the shell's faces is shared by exactly two of
/// them.
fn closed(entities: &Entities, shell: &[(FaceId, bool)]) -> bool {
    let set: BTreeSet<FaceId> = shell.iter().map(|(f, _)| *f).collect();
    shell.iter().all(|(f, _)| {
        entities.face(*f).is_some_and(|face| {
            face.loops().flatten().all(|u| {
                entities.edge(u.edge).is_some_and(|e| {
                    e.faces().len() == 2 && e.faces().iter().all(|g| set.contains(g))
                })
            })
        })
    })
}

/// Of `faces`, those turned the wrong way within their shell (see the
/// module documentation).
pub fn reversed_faces(entities: &Entities, faces: &[FaceId]) -> Vec<FaceId> {
    let mut out = Vec::new();
    for shell in shells(entities, faces) {
        let wrong_way = if closed(entities, &shell) {
            // Volume with every face turned the first face's way.
            let volume: f64 = shell
                .iter()
                .map(|(f, flip)| {
                    let v: f64 = entities
                        .face_triangles(*f)
                        .iter()
                        .map(|[a, b, c]| a.dot(b.cross(*c)) / 6.0)
                        .sum();
                    if *flip { -v } else { v }
                })
                .sum();
            // Positive: the first face's way is outward, so the faces
            // turned from it are the wrong ones.
            Some(volume > 0.0)
        } else {
            let turned = shell.iter().filter(|(_, flip)| *flip).count();
            let rest = shell.len() - turned;
            match turned.cmp(&rest) {
                std::cmp::Ordering::Less => Some(true),
                std::cmp::Ordering::Greater => Some(false),
                std::cmp::Ordering::Equal => None,
            }
        };
        // `wrong_way`: whether the faces turned from the first are the wrong
        // ones (else the first face's side is).
        if let Some(turned_wrong) = wrong_way {
            out.extend(
                shell
                    .iter()
                    .filter(|(_, flip)| *flip == turned_wrong)
                    .map(|(f, _)| *f),
            );
        }
    }
    out
}

/// Whether a ray from `origin` along unit `dir` meets triangle `abc`
/// between `near` and `far` (Möller–Trumbore).
fn ray_hits(origin: DVec3, dir: DVec3, [a, b, c]: [DVec3; 3], near: f64, far: f64) -> bool {
    let (e1, e2) = (b - a, c - a);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return false;
    }
    let inv = 1.0 / det;
    let s = origin - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    let t = e2.dot(q) * inv;
    t > near && t <= far
}

/// Runs Inspect Manifold. Returns its six report lines; nothing changes.
///
/// # Errors
/// None at present; the signature matches the other repair tools.
pub fn inspect_solid(
    entities: &Entities,
    op: &InspectSolid,
) -> Result<Vec<ReportLine>, ModelError> {
    let faces = faces_in(entities, op.scope.as_deref());
    let face_set: BTreeSet<FaceId> = faces.iter().copied().collect();
    let edges: BTreeSet<EdgeId> = faces
        .iter()
        .filter_map(|f| entities.face(*f))
        .flat_map(|f| f.loops().flatten().map(|u| u.edge).collect::<Vec<_>>())
        .collect();

    let non_manifold: Vec<EdgeId> = edges
        .iter()
        .copied()
        .filter(|e| entities.edge(*e).is_some_and(|x| x.faces().len() != 2))
        .collect();

    // Points: the faces round each vertex, joined through edges at it that
    // exactly two faces share, must make one fan.
    let vertices: BTreeSet<VertexId> = edges
        .iter()
        .filter_map(|e| entities.edge(*e))
        .flat_map(|e| e.vertices())
        .collect();
    let pinched: Vec<VertexId> = vertices
        .iter()
        .copied()
        .filter(|v| {
            let Some(vertex) = entities.vertex(*v) else {
                return false;
            };
            let round: BTreeSet<FaceId> = vertex
                .edges()
                .iter()
                .filter_map(|e| entities.edge(*e))
                .flat_map(|e| e.faces().iter().copied())
                .filter(|f| face_set.contains(f))
                .collect();
            let Some(first) = round.first().copied() else {
                return false;
            };
            let mut seen = BTreeSet::from([first]);
            let mut queue = vec![first];
            while let Some(f) = queue.pop() {
                for e in vertex.edges() {
                    let Some(edge) = entities.edge(*e) else {
                        continue;
                    };
                    if let [a, b] = edge.faces()[..]
                        && (a == f || b == f)
                    {
                        let g = if a == f { b } else { a };
                        if round.contains(&g) && seen.insert(g) {
                            queue.push(g);
                        }
                    }
                }
            }
            seen.len() < round.len()
        })
        .collect();

    let reversed = reversed_faces(entities, &faces);

    let mut thin: Vec<FaceId> = Vec::new();
    for shell in shells(entities, &faces) {
        let tris: Vec<(FaceId, Vec<[DVec3; 3]>)> = shell
            .iter()
            .map(|(f, _)| (*f, entities.face_triangles(*f)))
            .collect();
        for (f, own) in &tris {
            let Some(n) = entities.face(*f).map(|x| x.plane().normal()) else {
                continue;
            };
            let behind = own.iter().any(|[a, b, c]| {
                let from = (*a + *b + *c) / 3.0;
                tris.iter().filter(|(g, _)| g != f).any(|(_, other)| {
                    other
                        .iter()
                        .any(|t| ray_hits(from, -n, *t, TOL_MERGE, op.thickness))
                })
            });
            if behind {
                thin.push(*f);
            }
        }
    }

    let sharp: Vec<EdgeId> = edges
        .iter()
        .copied()
        .filter(|e| {
            let Some(edge) = entities.edge(*e) else {
                return false;
            };
            let [a, b] = edge.faces()[..] else {
                return false;
            };
            match (entities.face(a), entities.face(b)) {
                (Some(x), Some(y)) => {
                    let cos = x.plane().normal().dot(y.plane().normal()).clamp(-1.0, 1.0);
                    cos.acos() > op.sharp
                }
                _ => false,
            }
        })
        .collect();

    let mut overhangs: Vec<FaceId> = Vec::new();
    for shell in shells(entities, &faces) {
        let bottom = shell
            .iter()
            .filter_map(|(f, _)| entities.face(*f))
            .flat_map(|f| entities.loop_points(f.outer()))
            .map(|p| p.z)
            .fold(f64::INFINITY, f64::min);
        for (f, _) in &shell {
            let Some(face) = entities.face(*f) else {
                continue;
            };
            let down = -face.plane().normal().z;
            // Steeper than the Overhang from upright: its normal is within
            // (90° − Overhang) of straight down.
            if down <= op.overhang.sin() {
                continue;
            }
            let on_bed = entities
                .loop_points(face.outer())
                .iter()
                .all(|p| (p.z - bottom).abs() <= TOL_MERGE);
            if !on_bed {
                overhangs.push(*f);
            }
        }
    }

    let face_items = |f: &[FaceId]| f.iter().copied().map(Pickable::Face).collect();
    let edge_items = |e: &[EdgeId]| e.iter().copied().map(Pickable::Edge).collect();
    Ok(vec![
        ReportLine::count(
            "Non-Manifold Edges",
            non_manifold.len(),
            edge_items(&non_manifold),
        ),
        ReportLine::count(
            "Non-Manifold Points",
            pinched.len(),
            pinched.iter().copied().map(Pickable::Vertex).collect(),
        ),
        ReportLine::count("Reversed Faces", reversed.len(), face_items(&reversed)),
        ReportLine::count("Thin Walls", thin.len(), face_items(&thin)),
        ReportLine::count("Sharp Edges", sharp.len(), edge_items(&sharp)),
        ReportLine::count("Overhangs", overhangs.len(), face_items(&overhangs)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    /// A closed box from `lo` to `hi`, its faces turned outward.
    fn solid_box(m: &mut Entities, lo: DVec3, hi: DVec3) {
        let c = |x: bool, y: bool, z: bool| {
            v(
                if x { hi.x } else { lo.x },
                if y { hi.y } else { lo.y },
                if z { hi.z } else { lo.z },
            )
        };
        for quad in [
            [
                c(false, false, false),
                c(true, false, false),
                c(true, true, false),
                c(false, true, false),
            ],
            [
                c(false, false, true),
                c(true, false, true),
                c(true, true, true),
                c(false, true, true),
            ],
            [
                c(false, false, false),
                c(true, false, false),
                c(true, false, true),
                c(false, false, true),
            ],
            [
                c(false, true, false),
                c(true, true, false),
                c(true, true, true),
                c(false, true, true),
            ],
            [
                c(false, false, false),
                c(false, true, false),
                c(false, true, true),
                c(false, false, true),
            ],
            [
                c(true, false, false),
                c(true, true, false),
                c(true, true, true),
                c(true, false, true),
            ],
        ] {
            m.add_polyline(&quad, true).unwrap();
        }
        // Turn them all outward from one face.
        let centre = (lo + hi) * 0.5;
        let faces: Vec<FaceId> = m.face_ids().collect();
        for f in faces {
            let face = m.face(f).unwrap();
            let p = m.loop_points(face.outer())[0];
            if face.plane().normal().dot(p - centre) < 0.0 {
                m.reverse_face(f).unwrap();
            }
        }
    }

    fn line(r: &[ReportLine], start: &str) -> String {
        r.iter()
            .find(|l| l.text.starts_with(start))
            .map(|l| l.text.clone())
            .unwrap_or_default()
    }

    #[test]
    fn a_closed_box_is_clean() {
        let mut m = Entities::new();
        solid_box(&mut m, v(0.0, 0.0, 0.0), v(10.0, 10.0, 10.0));
        let before = m.clone();
        let r = inspect_solid(&m, &InspectSolid::default()).unwrap();
        assert_eq!(r.len(), 6);
        assert!(r.iter().all(|l| l.text.ends_with(": 0")), "{r:?}");
        assert_eq!(m, before);
    }

    #[test]
    fn one_reversed_face_is_found() {
        let mut m = Entities::new();
        solid_box(&mut m, v(0.0, 0.0, 0.0), v(10.0, 10.0, 10.0));
        let f = m.face_ids().next().unwrap();
        m.reverse_face(f).unwrap();
        let faces: Vec<FaceId> = m.face_ids().collect();
        assert_eq!(reversed_faces(&m, &faces), [f]);
        let r = inspect_solid(&m, &InspectSolid::default()).unwrap();
        assert_eq!(line(&r, "Reversed Faces"), "Reversed Faces: 1");
        // Every face reversed: the whole box faces inward.
        let mut inside = Entities::new();
        solid_box(&mut inside, v(0.0, 0.0, 0.0), v(10.0, 10.0, 10.0));
        for f in inside.face_ids().collect::<Vec<_>>() {
            inside.reverse_face(f).unwrap();
        }
        let all: Vec<FaceId> = inside.face_ids().collect();
        assert_eq!(reversed_faces(&inside, &all).len(), 6);
        // A lone face is never reversed.
        let mut one = Entities::new();
        one.add_polyline(
            &[v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0)],
            true,
        )
        .unwrap();
        let f: Vec<FaceId> = one.face_ids().collect();
        assert!(reversed_faces(&one, &f).is_empty());
    }

    #[test]
    fn a_missing_face_and_an_inner_wall_are_non_manifold() {
        let mut m = Entities::new();
        solid_box(&mut m, v(0.0, 0.0, 0.0), v(10.0, 10.0, 10.0));
        let top = m
            .face_ids()
            .find(|f| m.face(*f).unwrap().plane().normal().z > 0.9)
            .unwrap();
        m.erase_face(top).unwrap();
        let r = inspect_solid(&m, &InspectSolid::default()).unwrap();
        assert_eq!(line(&r, "Non-Manifold Edges"), "Non-Manifold Edges: 4");
        let mut wall = Entities::new();
        solid_box(&mut wall, v(0.0, 0.0, 0.0), v(10.0, 10.0, 10.0));
        wall.add_polyline(
            &[
                v(5.0, 0.0, 0.0),
                v(5.0, 10.0, 0.0),
                v(5.0, 10.0, 10.0),
                v(5.0, 0.0, 10.0),
            ],
            true,
        )
        .unwrap();
        let r = inspect_solid(&wall, &InspectSolid::default()).unwrap();
        assert_ne!(line(&r, "Non-Manifold Edges"), "Non-Manifold Edges: 0");
    }

    #[test]
    fn boxes_touching_at_a_corner_pinch_it() {
        let mut m = Entities::new();
        solid_box(&mut m, v(0.0, 0.0, 0.0), v(10.0, 10.0, 10.0));
        // A second box, sharing only the corner at (10, 10, 10).
        solid_box(&mut m, v(10.0, 10.0, 10.0), v(20.0, 20.0, 20.0));
        let r = inspect_solid(&m, &InspectSolid::default()).unwrap();
        assert_eq!(line(&r, "Non-Manifold Points"), "Non-Manifold Points: 1");
    }

    #[test]
    fn thin_plates_and_sharp_wedges() {
        let mut thin = Entities::new();
        solid_box(&mut thin, v(0.0, 0.0, 0.0), v(20.0, 20.0, 0.5));
        let r = inspect_solid(&thin, &InspectSolid::default()).unwrap();
        assert_eq!(line(&r, "Thin Walls"), "Thin Walls: 2");
        let mut thick = Entities::new();
        solid_box(&mut thick, v(0.0, 0.0, 0.0), v(20.0, 20.0, 5.0));
        let r = inspect_solid(&thick, &InspectSolid::default()).unwrap();
        assert_eq!(line(&r, "Thin Walls"), "Thin Walls: 0");
        // A wedge 2 mm high and 20 mm long: a knife-edge along its tip.
        let mut wedge = Entities::new();
        let (a, b, c) = (v(0.0, 0.0, 0.0), v(20.0, 0.0, 0.0), v(0.0, 0.0, 2.0));
        let up = v(0.0, 10.0, 0.0);
        for quad in [
            [a, b, b + up, a + up],
            [b, c, c + up, b + up],
            [c, a, a + up, c + up],
        ] {
            wedge.add_polyline(&quad, true).unwrap();
        }
        wedge.add_polyline(&[a, b, c], true).unwrap();
        wedge.add_polyline(&[a + up, b + up, c + up], true).unwrap();
        let centre = (a + b + c) / 3.0 + up * 0.5;
        for f in wedge.face_ids().collect::<Vec<_>>() {
            let face = wedge.face(f).unwrap();
            let p = wedge.loop_points(face.outer())[0];
            if face.plane().normal().dot(p - centre) < 0.0 {
                wedge.reverse_face(f).unwrap();
            }
        }
        let r = inspect_solid(&wedge, &InspectSolid::default()).unwrap();
        assert_eq!(line(&r, "Sharp Edges"), "Sharp Edges: 1");
    }

    #[test]
    fn the_bottom_is_not_an_overhang_but_a_ledge_is() {
        let mut m = Entities::new();
        solid_box(&mut m, v(0.0, 0.0, 0.0), v(10.0, 10.0, 10.0));
        let r = inspect_solid(&m, &InspectSolid::default()).unwrap();
        assert_eq!(line(&r, "Overhangs"), "Overhangs: 0");
        // The same box raised off the bed by another box under one half: its
        // other half's underside is a ledge.
        let mut t = Entities::new();
        let pts = [
            v(0.0, 0.0, 0.0),
            v(5.0, 0.0, 0.0),
            v(5.0, 0.0, 5.0),
            v(10.0, 0.0, 5.0),
            v(10.0, 0.0, 10.0),
            v(0.0, 0.0, 10.0),
        ];
        let depth = v(0.0, 10.0, 0.0);
        t.add_polyline(&pts, true).unwrap();
        t.add_polyline(&pts.map(|p| p + depth), true).unwrap();
        for i in 0..pts.len() {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            t.add_polyline(&[p, q, q + depth, p + depth], true).unwrap();
        }
        let centre = v(4.0, 5.0, 6.0);
        for f in t.face_ids().collect::<Vec<_>>() {
            let face = t.face(f).unwrap();
            let n = face.plane().normal();
            let p = t.loop_points(face.outer())[0];
            // The L's faces all point away from its inside; the centre test
            // works for every face but the ledge, checked by hand.
            let outward = if (p.z - 5.0).abs() < 1e-9 && n.z.abs() > 0.9 {
                n.z < 0.0
            } else {
                n.dot(p - centre) > 0.0
            };
            if !outward {
                t.reverse_face(f).unwrap();
            }
        }
        let r = inspect_solid(&t, &InspectSolid::default()).unwrap();
        assert_eq!(line(&r, "Overhangs"), "Overhangs: 1");
        assert_eq!(line(&r, "Reversed Faces"), "Reversed Faces: 0");
    }
}
