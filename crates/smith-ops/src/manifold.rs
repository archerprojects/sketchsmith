//! SketchSmith — Make Manifold | v1.0.0 | Thursday 12 November 2026
//!
//! [`make_manifold`] makes a shell a closed, consistent solid where it can,
//! in passes, each relying on those before it:
//!
//! 1. **Merge by Distance** and 2. **Remove Zero-Size** — Clean Up's own
//!    passes, so near-miss corners meet and slivers go. Points merge but
//!    are never deleted.
//! 3. **Delete Interior** (only when asked) — faces whose edges are all
//!    shared by three or more faces: walls inside the solid.
//! 4. **Fill Holes** — each closed loop of edges with a face on one side
//!    only. A flat one gets a face ([`Entities::fill`]); a bent one is left
//!    and listed, unless Auto-Tri is on for the run, when it is filled with
//!    triangles.
//! 5. **Orient Faces** — every face of a shell turned to agree with the
//!    rest, and a closed shell outward ([`crate::solid::reversed_faces`]).
//!
//! Hidden geometry stays hidden and counts as part of the solid. The scope
//! works as the other repair tools': the selection's points, or everything
//! in the context.

use std::collections::{BTreeMap, BTreeSet};

use smith_geom::DVec3;
use smith_geom::plane::Plane;
use smith_geom::tolerance::TOL_MERGE;
use smith_model::{EdgeId, Entities, FaceId, ModelError, Pickable, VertexId};

use crate::cleanup::{CleanUp, Passes, ReportLine};

/// One Make Manifold run.
#[derive(Debug, Clone, PartialEq)]
pub struct MakeManifold {
    /// The points it works on, by position; `None`: everything in the
    /// context.
    pub scope: Option<Vec<DVec3>>,
    /// Merge Distance, mm (Merge by Distance and Remove Zero-Size).
    pub distance: f64,
    /// Delete Interior.
    pub delete_interior: bool,
}

impl Default for MakeManifold {
    fn default() -> Self {
        Self {
            scope: None,
            distance: 0.01,
            delete_interior: false,
        }
    }
}

/// Runs Make Manifold. Returns its report lines, and whether bent holes
/// were left that Auto-Tri could fill.
///
/// # Errors
/// As Clean Up's passes and [`Entities::fill`].
pub fn make_manifold(
    entities: &mut Entities,
    op: &MakeManifold,
) -> Result<(Vec<ReportLine>, bool), ModelError> {
    let clean = CleanUp {
        scope: op.scope.clone(),
        distance: op.distance,
        passes: Passes {
            merge_by_distance: true,
            zero_size: true,
            duplicate_faces: false,
            fuse_overlaps: false,
            merge_coplanar: false,
            stray_edges: false,
            join_split_edges: false,
            reset_to_base: false,
            purge_unused: false,
        },
        ..CleanUp::default()
    };
    let mut report = crate::cleanup::clean_up(entities, &clean)?;

    if op.delete_interior {
        let interior: Vec<FaceId> = faces_here(entities, op)
            .into_iter()
            .filter(|f| {
                entities.face(*f).is_some_and(|face| {
                    face.loops()
                        .flatten()
                        .all(|u| entities.edge(u.edge).is_some_and(|e| e.faces().len() >= 3))
                })
            })
            .collect();
        for f in &interior {
            entities.erase_face(*f)?;
        }
        report.push(ReportLine::count(
            "Interior Faces Removed",
            interior.len(),
            Vec::new(),
        ));
    }

    // Holes: closed loops of edges with one face.
    let before: BTreeSet<FaceId> = entities.face_ids().collect();
    let mut bent: Vec<EdgeId> = Vec::new();
    let mut bent_count = 0;
    for hole in holes(entities, op) {
        let pts: Vec<DVec3> = hole
            .iter()
            .filter_map(|v| entities.vertex(*v).map(smith_model::Vertex::position))
            .collect();
        let flat = Plane::fit_loop(&pts).is_ok_and(|p| p.contains_all(&pts));
        if flat || smith_model::auto_tri() {
            entities.fill(&hole, &[])?;
        } else {
            bent_count += 1;
            for w in 0..hole.len() {
                if let Some(e) = entities.edge_between(hole[w], hole[(w + 1) % hole.len()]) {
                    bent.push(e);
                }
            }
        }
    }
    let filled: Vec<Pickable> = entities
        .face_ids()
        .filter(|f| !before.contains(f))
        .map(Pickable::Face)
        .collect();
    report.push(ReportLine::count("Holes Filled", filled.len(), filled));
    report.push(ReportLine::count(
        "Bent Holes",
        bent_count,
        bent.into_iter().map(Pickable::Edge).collect(),
    ));

    let faces = faces_here(entities, op);
    let turned = crate::solid::reversed_faces(entities, &faces);
    for f in &turned {
        entities.reverse_face(*f)?;
    }
    report.push(ReportLine::count(
        "Faces Turned",
        turned.len(),
        turned.iter().copied().map(Pickable::Face).collect(),
    ));

    let left: Vec<Pickable> = edges_here(entities, op)
        .into_iter()
        .filter(|e| {
            entities
                .edge(*e)
                .is_some_and(|x| !x.faces().is_empty() && x.faces().len() != 2)
        })
        .map(Pickable::Edge)
        .collect();
    report.push(ReportLine::count(
        "Non-Manifold Edges Left",
        left.len(),
        left,
    ));
    Ok((report, bent_count > 0 && !smith_model::auto_tri()))
}

/// Whether point `p` is in scope.
fn in_scope(op: &MakeManifold, p: DVec3) -> bool {
    op.scope
        .as_ref()
        .is_none_or(|s| s.iter().any(|q| q.distance(p) <= TOL_MERGE))
}

/// The faces in scope.
fn faces_here(entities: &Entities, op: &MakeManifold) -> Vec<FaceId> {
    crate::solid::faces_in(entities, op.scope.as_deref())
}

/// The edges in scope: both ends in it.
fn edges_here(entities: &Entities, op: &MakeManifold) -> Vec<EdgeId> {
    entities
        .edge_ids()
        .filter(|e| {
            entities.edge(*e).is_some_and(|x| {
                x.vertices().iter().all(|v| {
                    entities
                        .vertex(*v)
                        .is_some_and(|y| in_scope(op, y.position()))
                })
            })
        })
        .collect()
}

/// The holes in scope: closed loops of edges with exactly one face, each
/// as its vertices in walking order.
fn holes(entities: &Entities, op: &MakeManifold) -> Vec<Vec<VertexId>> {
    let border: Vec<EdgeId> = edges_here(entities, op)
        .into_iter()
        .filter(|e| entities.edge(*e).is_some_and(|x| x.faces().len() == 1))
        .collect();
    let mut at: BTreeMap<VertexId, Vec<EdgeId>> = BTreeMap::new();
    for e in &border {
        if let Some(x) = entities.edge(*e) {
            for v in x.vertices() {
                at.entry(v).or_default().push(*e);
            }
        }
    }
    let mut used: BTreeSet<EdgeId> = BTreeSet::new();
    let mut out = Vec::new();
    for start in &border {
        if used.contains(start) {
            continue;
        }
        let Some([first, mut here]) = entities.edge(*start).map(smith_model::Edge::vertices) else {
            continue;
        };
        used.insert(*start);
        let mut walk = vec![first];
        let mut closed = false;
        for _ in 0..border.len() {
            if here == first {
                closed = true;
                break;
            }
            walk.push(here);
            let next = at
                .get(&here)
                .and_then(|es| es.iter().find(|e| !used.contains(*e)).copied());
            let Some(e) = next else {
                break;
            };
            used.insert(e);
            here = entities.edge(e).map_or(here, |x| x.other(here));
        }
        if closed && walk.len() >= 3 {
            out.push(walk);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    /// A box, its faces turned outward, with the faces `skip` names erased
    /// (0 bottom, 1 top, 2 front, 3 back, 4 left, 5 right); their edges
    /// stay, so each leaves a hole.
    fn open_box(m: &mut Entities, size: f64, skip: &[usize]) {
        let c = |x: bool, y: bool, z: bool| {
            v(
                if x { size } else { 0.0 },
                if y { size } else { 0.0 },
                if z { size } else { 0.0 },
            )
        };
        let quads = [
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
        ];
        for q in &quads {
            m.add_polyline(q, true).unwrap();
        }
        for i in skip {
            let mid = quads[*i].iter().copied().sum::<DVec3>() / 4.0;
            let f = m
                .face_ids()
                .find(|f| {
                    let face = m.face(*f).unwrap();
                    face.plane().signed_distance(mid).abs() < 1e-9
                        && m.loop_points(face.outer())
                            .iter()
                            .all(|p| face.plane().signed_distance(*p).abs() < 1e-9)
                        && quads[*i].iter().all(|q| {
                            m.loop_points(face.outer())
                                .iter()
                                .any(|p| p.distance(*q) < 1e-9)
                        })
                })
                .unwrap();
            m.erase_face(f).unwrap();
        }
        let centre = v(size / 2.0, size / 2.0, size / 2.0);
        for f in m.face_ids().collect::<Vec<_>>() {
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
    fn a_box_missing_its_top_is_closed_and_turned_outward() {
        let mut m = Entities::new();
        open_box(&mut m, 10.0, &[1]);
        assert_eq!(m.face_count(), 5);
        let (r, offer) = make_manifold(&mut m, &MakeManifold::default()).unwrap();
        m.validate().unwrap();
        assert!(!offer);
        assert_eq!(line(&r, "Holes Filled"), "Holes Filled: 1");
        assert_eq!(
            line(&r, "Non-Manifold Edges Left"),
            "Non-Manifold Edges Left: 0"
        );
        assert_eq!(m.face_count(), 6);
        let faces: Vec<FaceId> = m.face_ids().collect();
        assert!(crate::solid::reversed_faces(&m, &faces).is_empty());
        let top = m
            .face_ids()
            .find(|f| {
                m.loop_points(m.face(*f).unwrap().outer())
                    .iter()
                    .all(|p| (p.z - 10.0).abs() < 1e-9)
            })
            .unwrap();
        assert!(m.face(top).unwrap().plane().normal().z > 0.9);
    }

    #[test]
    fn a_face_turned_in_is_turned_back() {
        let mut m = Entities::new();
        open_box(&mut m, 10.0, &[]);
        let f = m.face_ids().next().unwrap();
        m.reverse_face(f).unwrap();
        let (r, _) = make_manifold(&mut m, &MakeManifold::default()).unwrap();
        assert_eq!(line(&r, "Faces Turned"), "Faces Turned: 1");
        let faces: Vec<FaceId> = m.face_ids().collect();
        assert!(crate::solid::reversed_faces(&m, &faces).is_empty());
    }

    #[test]
    fn an_inner_wall_goes_only_with_delete_interior() {
        let wall = [
            v(5.0, 0.0, 0.0),
            v(5.0, 10.0, 0.0),
            v(5.0, 10.0, 10.0),
            v(5.0, 0.0, 10.0),
        ];
        let mut m = Entities::new();
        open_box(&mut m, 10.0, &[]);
        m.add_polyline(&wall, true).unwrap();
        let faces = m.face_count();
        let (r, _) = make_manifold(&mut m, &MakeManifold::default()).unwrap();
        assert!(
            !r.iter()
                .any(|l| l.text.starts_with("Interior Faces Removed"))
        );
        assert_eq!(m.face_count(), faces);
        let op = MakeManifold {
            delete_interior: true,
            ..MakeManifold::default()
        };
        let (r, _) = make_manifold(&mut m, &op).unwrap();
        assert_eq!(
            line(&r, "Interior Faces Removed"),
            "Interior Faces Removed: 1"
        );
        assert_eq!(m.face_count(), faces - 1);
    }

    #[test]
    fn a_bent_hole_is_listed_and_filled_with_auto_tri() {
        let mut m = Entities::new();
        open_box(&mut m, 10.0, &[1]);
        // Lift one top corner: the hole's outline bends.
        let corner = m.find_vertex(v(10.0, 10.0, 10.0)).unwrap();
        smith_model::with_auto_tri(|| m.move_vertices(&[corner], v(0.0, 0.0, 2.0))).unwrap();
        let before = m.clone();
        let (r, offer) = make_manifold(&mut m, &MakeManifold::default()).unwrap();
        assert!(offer);
        assert_eq!(line(&r, "Bent Holes"), "Bent Holes: 1");
        assert_eq!(line(&r, "Holes Filled"), "Holes Filled: 0");
        let mut tri = before;
        let (r, offer) =
            smith_model::with_auto_tri(|| make_manifold(&mut tri, &MakeManifold::default()))
                .unwrap();
        assert!(!offer);
        assert_eq!(line(&r, "Bent Holes"), "Bent Holes: 0");
        assert_eq!(
            line(&r, "Non-Manifold Edges Left"),
            "Non-Manifold Edges Left: 0"
        );
    }

    #[test]
    fn the_merge_passes_run_and_report() {
        let mut m = Entities::new();
        open_box(&mut m, 10.0, &[]);
        let (r, _) = make_manifold(&mut m, &MakeManifold::default()).unwrap();
        let texts: Vec<&str> = r.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Points Merged: 0",
                "Short Edges Collapsed: 0",
                "Zero Faces Removed: 0",
                "Holes Filled: 0",
                "Bent Holes: 0",
                "Faces Turned: 0",
                "Non-Manifold Edges Left: 0"
            ]
        );
    }
}
