//! SketchSmith — Clean Up | v1.0.0 | Thursday 12 November 2026
//!
//! [`clean_up`] tidies the geometry of one context, pass by pass, in this
//! order (each pass relies on the ones before it):
//!
//! 1. **Merge by Distance** — points closer than the Merge Distance join
//!    ([`Entities::merge_by_distance`]).
//! 2. **Remove Zero-Size** — edges no longer than the Merge Distance
//!    collapse to a point; faces with no area go.
//! 3. **Duplicate Faces** — faces on exactly the same points (either way
//!    round) are made one.
//! 4. **Fuse Overlapped Faces** (off unless asked) — faces on one plane
//!    that overlap become one face covering them all, with the first one's
//!    paint ([`Entities::fuse_overlaps`]).
//! 5. **Merge Coplanar** — flat neighbours on one plane join across the
//!    edge between them ([`Entities::merge_faces_across`]), whatever their
//!    paint; each join across different paint is counted. Edges of drawn
//!    curves are kept.
//! 6. **Stray Edges** — edges with no face go, except those of drawn
//!    curves.
//! 7. **Join Split Edges** — a point where just two edges meet in a line,
//!    or bend by no more than the Angle, goes, joining them into one edge
//!    ([`Entities::join_at_vertex`]). Points of drawn curves are kept.
//! 8. **Reset to Base** — loose edges and faces go onto the Base layer.
//!
//! Purge Unused works on the whole document, so the canvas runs it after
//! these: unused groups always go
//! ([`smith_model::Document::purge_unused_groups`]); unused materials are
//! held back and go only on a run with [`CleanUp::remove_materials`] set
//! ([`smith_model::Document::remove_unused_materials`]).
//!
//! With a selection, Clean Up works on its points: an edge counts when both
//! its ends do, a face when all its corners do. The selection is kept as
//! point positions, so the same run can be redone with new values after an
//! undo. Each pass reports one line, with the items it left to select.

use std::collections::{BTreeMap, HashSet};

use smith_geom::DVec3;
use smith_geom::tolerance::TOL_MERGE;
use smith_model::{EdgeId, Entities, FaceId, ModelError, Pickable, VertexId};

/// Which passes run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Passes {
    /// Merge by Distance.
    pub merge_by_distance: bool,
    /// Remove Zero-Size.
    pub zero_size: bool,
    /// Duplicate Faces.
    pub duplicate_faces: bool,
    /// Fuse Overlapped Faces.
    pub fuse_overlaps: bool,
    /// Merge Coplanar.
    pub merge_coplanar: bool,
    /// Stray Edges.
    pub stray_edges: bool,
    /// Join Split Edges.
    pub join_split_edges: bool,
    /// Reset to Base.
    pub reset_to_base: bool,
    /// Purge Unused (run by the canvas on the document).
    pub purge_unused: bool,
}

impl Default for Passes {
    fn default() -> Self {
        Self {
            merge_by_distance: true,
            zero_size: true,
            duplicate_faces: true,
            fuse_overlaps: false,
            merge_coplanar: true,
            stray_edges: true,
            join_split_edges: true,
            reset_to_base: false,
            purge_unused: true,
        }
    }
}

/// One Clean Up run.
#[derive(Debug, Clone, PartialEq)]
pub struct CleanUp {
    /// The points it works on, by position; `None`: everything in the
    /// context.
    pub scope: Option<Vec<DVec3>>,
    /// Merge Distance, mm (Merge by Distance and Remove Zero-Size).
    pub distance: f64,
    /// Join Split Edges' Angle, radians: 0 joins only straight runs.
    pub angle: f64,
    /// The passes.
    pub passes: Passes,
    /// Purge Unused also removes unused materials (only once asked).
    pub remove_materials: bool,
}

impl Default for CleanUp {
    fn default() -> Self {
        Self {
            scope: None,
            distance: 0.01,
            angle: 0.0,
            passes: Passes::default(),
            remove_materials: false,
        }
    }
}

/// One line of a report: what was found or done, and the items to select
/// for it (empty when nothing is left to select, as for removed edges).
#[derive(Debug, Clone, PartialEq)]
pub struct ReportLine {
    /// The text, e.g. "Faces Merged: 6".
    pub text: String,
    /// What a click on the line selects.
    pub items: Vec<Pickable>,
}

impl ReportLine {
    /// A line reading "`label`: `count`".
    pub fn count(label: &str, count: usize, items: Vec<Pickable>) -> Self {
        Self {
            text: format!("{label}: {count}"),
            items,
        }
    }
}

/// Runs Clean Up's geometry passes on `entities` (see the module
/// documentation). Returns one report line per pass run.
///
/// # Errors
/// [`ModelError::NotFlat`] if merging points would bend faces (nothing
/// changes), unless Auto-Tri is on; otherwise as the passes' own
/// operations.
pub fn clean_up(entities: &mut Entities, c: &CleanUp) -> Result<Vec<ReportLine>, ModelError> {
    let p = c.passes;
    let mut report = Vec::new();
    if p.merge_by_distance {
        let scope = scope_points(entities, c);
        let ids: Option<Vec<VertexId>> = scope.map(|s| s.into_iter().collect());
        let n = entities.merge_by_distance(ids.as_deref(), c.distance)?;
        report.push(ReportLine::count("Points Merged", n, Vec::new()));
    }
    if p.zero_size {
        let (edges, faces) = remove_zero_size(entities, c)?;
        report.push(ReportLine::count(
            "Short Edges Collapsed",
            edges,
            Vec::new(),
        ));
        report.push(ReportLine::count("Zero Faces Removed", faces, Vec::new()));
    }
    if p.duplicate_faces {
        let n = remove_duplicate_faces(entities, c)?;
        report.push(ReportLine::count("Duplicate Faces Removed", n, Vec::new()));
    }
    if p.fuse_overlaps {
        let scope = scope_points(entities, c);
        let faces: Vec<FaceId> = entities
            .face_ids()
            .filter(|f| face_in(entities, scope.as_ref(), *f))
            .collect();
        let (n, made) = entities.fuse_overlaps(&faces);
        let items = made.into_iter().map(Pickable::Face).collect();
        report.push(ReportLine::count("Overlapped Faces Fused", n, items));
    }
    if p.merge_coplanar {
        let (merged, painted, faces) = merge_coplanar(entities, c)?;
        let items: Vec<Pickable> = faces.into_iter().map(Pickable::Face).collect();
        report.push(ReportLine::count("Faces Merged", merged, items));
        report.push(ReportLine::count(
            "Paint Borders Merged",
            painted,
            Vec::new(),
        ));
    }
    if p.stray_edges {
        let n = remove_stray_edges(entities, c)?;
        report.push(ReportLine::count("Stray Edges Removed", n, Vec::new()));
    }
    if p.join_split_edges {
        let joined = join_split_edges(entities, c);
        let items: Vec<Pickable> = joined.iter().copied().map(Pickable::Edge).collect();
        report.push(ReportLine::count("Split Edges Joined", joined.len(), items));
    }
    if p.reset_to_base {
        let items = reset_to_base(entities, c)?;
        report.push(ReportLine::count("Moved to Base", items.len(), items));
    }
    Ok(report)
}

/// The points in scope now: those at the scope's positions, or `None` for
/// everything.
fn scope_points(entities: &Entities, c: &CleanUp) -> Option<HashSet<VertexId>> {
    c.scope
        .as_ref()
        .map(|ps| ps.iter().filter_map(|p| entities.find_vertex(*p)).collect())
}

/// Whether both ends of `e` are in scope.
fn edge_in(entities: &Entities, scope: Option<&HashSet<VertexId>>, e: EdgeId) -> bool {
    match (scope, entities.edge(e)) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(s), Some(edge)) => edge.vertices().iter().all(|v| s.contains(v)),
    }
}

/// Whether every corner of `f` is in scope.
fn face_in(entities: &Entities, scope: Option<&HashSet<VertexId>>, f: FaceId) -> bool {
    match (scope, entities.face(f)) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(s), Some(face)) => entities
            .loop_vertices(face.outer())
            .iter()
            .all(|v| s.contains(v)),
    }
}

/// Collapses edges no longer than the Merge Distance and removes faces
/// with no area. Returns how many of each.
fn remove_zero_size(entities: &mut Entities, c: &CleanUp) -> Result<(usize, usize), ModelError> {
    let scope = scope_points(entities, c);
    let pairs: Vec<(VertexId, VertexId)> = entities
        .edge_ids()
        .filter(|e| edge_in(entities, scope.as_ref(), *e))
        .filter(|e| entities.edge_length(*e).is_some_and(|l| l <= c.distance))
        .filter_map(|e| entities.edge(e).map(|x| (x.vertices()[0], x.vertices()[1])))
        .collect();
    let edges = pairs.len();
    if edges > 0 {
        entities.merge_vertices(&pairs)?;
    }
    // A face with no area: narrower than the merge tolerance (twice its
    // area over its outline's length is its width for a sliver).
    let scope = scope_points(entities, c);
    let flat: Vec<FaceId> = entities
        .face_ids()
        .filter(|f| face_in(entities, scope.as_ref(), *f))
        .filter(|f| {
            let Some(face) = entities.face(*f) else {
                return false;
            };
            let pts = entities.loop_points(face.outer());
            let perimeter: f64 = (0..pts.len())
                .map(|i| pts[i].distance(pts[(i + 1) % pts.len()]))
                .sum();
            let area = entities.face_area(*f).unwrap_or(0.0);
            perimeter <= 0.0 || 2.0 * area / perimeter <= TOL_MERGE
        })
        .collect();
    for f in &flat {
        entities.erase_face(*f)?;
    }
    Ok((edges, flat.len()))
}

/// Removes every face but one of each set on exactly the same points.
fn remove_duplicate_faces(entities: &mut Entities, c: &CleanUp) -> Result<usize, ModelError> {
    let scope = scope_points(entities, c);
    let mut seen: BTreeMap<(Vec<VertexId>, Vec<Vec<VertexId>>), FaceId> = BTreeMap::new();
    let mut doubles = Vec::new();
    for f in entities.face_ids().collect::<Vec<_>>() {
        if !face_in(entities, scope.as_ref(), f) {
            continue;
        }
        let Some(face) = entities.face(f) else {
            continue;
        };
        let mut outer = entities.loop_vertices(face.outer());
        outer.sort();
        let mut holes: Vec<Vec<VertexId>> = face
            .inner()
            .iter()
            .map(|h| {
                let mut h = entities.loop_vertices(h);
                h.sort();
                h
            })
            .collect();
        holes.sort();
        if seen.insert((outer, holes), f).is_some() {
            doubles.push(f);
        }
    }
    for f in &doubles {
        entities.erase_face(*f)?;
    }
    Ok(doubles.len())
}

/// Joins coplanar neighbours until none are left. Returns the joins, how
/// many joined different paint, and the faces they left.
fn merge_coplanar(
    entities: &mut Entities,
    c: &CleanUp,
) -> Result<(usize, usize, Vec<FaceId>), ModelError> {
    let (mut merged, mut painted) = (0, 0);
    let mut touched: HashSet<VertexId> = HashSet::new();
    loop {
        let scope = scope_points(entities, c);
        let candidates: Vec<EdgeId> = entities
            .edge_ids()
            .filter(|e| edge_in(entities, scope.as_ref(), *e))
            .filter(|e| {
                entities.edge(*e).is_some_and(|x| {
                    x.faces().len() == 2
                        && x.curve().is_none()
                        && x.faces()
                            .iter()
                            .all(|f| face_in(entities, scope.as_ref(), *f))
                })
            })
            .collect();
        let mut progress = false;
        for e in candidates {
            let Some(edge) = entities.edge(e) else {
                continue;
            };
            let [f1, f2] = edge.faces()[..] else {
                continue;
            };
            let ends = edge.vertices();
            let differ = match (entities.face(f1), entities.face(f2)) {
                (Some(a), Some(b)) => {
                    a.attributes.front != b.attributes.front
                        || a.attributes.back != b.attributes.back
                }
                _ => false,
            };
            if entities.merge_faces_across(e)? {
                merged += 1;
                painted += usize::from(differ);
                touched.extend(ends);
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    let still: Vec<VertexId> = touched
        .into_iter()
        .filter(|v| entities.vertex(*v).is_some())
        .collect();
    Ok((merged, painted, entities.faces_at(&still)))
}

/// Removes edges with no face that aren't part of a drawn curve.
fn remove_stray_edges(entities: &mut Entities, c: &CleanUp) -> Result<usize, ModelError> {
    let scope = scope_points(entities, c);
    let stray: Vec<EdgeId> = entities
        .edge_ids()
        .filter(|e| edge_in(entities, scope.as_ref(), *e))
        .filter(|e| {
            entities
                .edge(*e)
                .is_some_and(|x| x.faces().is_empty() && x.curve().is_none())
        })
        .collect();
    for e in &stray {
        entities.erase_edge(*e)?;
    }
    Ok(stray.len())
}

/// Joins edges at points where just two meet in a line (or within the
/// Angle). Returns the joined edges.
pub(crate) fn join_split_edges(entities: &mut Entities, c: &CleanUp) -> Vec<EdgeId> {
    let scope = scope_points(entities, c);
    let points: Vec<VertexId> = entities
        .vertex_ids()
        .filter(|v| scope.as_ref().is_none_or(|s| s.contains(v)))
        .collect();
    let mut ends: Vec<(VertexId, VertexId)> = Vec::new();
    for v in points {
        let Some(vertex) = entities.vertex(v) else {
            continue;
        };
        let [e1, e2] = vertex.edges()[..] else {
            continue;
        };
        let (Some(a), Some(b)) = (entities.edge(e1), entities.edge(e2)) else {
            continue;
        };
        if a.curve().is_some() || b.curve().is_some() {
            continue;
        }
        let (p, q) = (a.other(v), b.other(v));
        if entities.join_at_vertex(v, c.angle).is_ok() {
            // Earlier joins along the same run end at its far ends.
            ends.retain(|(x, y)| ![*x, *y].contains(&v));
            ends.push((p, q));
        }
    }
    ends.into_iter()
        .filter_map(|(p, q)| entities.edge_between(p, q))
        .collect()
}

/// Puts loose edges and faces in scope onto Base. Returns them.
fn reset_to_base(entities: &mut Entities, c: &CleanUp) -> Result<Vec<Pickable>, ModelError> {
    let scope = scope_points(entities, c);
    let mut items: Vec<Pickable> = entities
        .edge_ids()
        .filter(|e| edge_in(entities, scope.as_ref(), *e))
        .filter(|e| entities.edge(*e).is_some_and(|x| x.flags.layer.is_some()))
        .map(Pickable::Edge)
        .collect();
    items.extend(
        entities
            .face_ids()
            .filter(|f| face_in(entities, scope.as_ref(), *f))
            .filter(|f| {
                entities
                    .face(*f)
                    .is_some_and(|x| x.attributes.layer.is_some())
            })
            .map(Pickable::Face),
    );
    entities.set_layer(&items, None)?;
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    fn only(passes: impl FnOnce(&mut Passes)) -> CleanUp {
        let mut p = Passes {
            merge_by_distance: false,
            zero_size: false,
            duplicate_faces: false,
            fuse_overlaps: false,
            merge_coplanar: false,
            stray_edges: false,
            join_split_edges: false,
            reset_to_base: false,
            purge_unused: false,
        };
        passes(&mut p);
        CleanUp {
            passes: p,
            ..CleanUp::default()
        }
    }

    /// A 10 mm box whose six faces are each split into two triangles.
    fn triangulated_box() -> Entities {
        let mut m = Entities::new();
        let c = |x: f64, y: f64, z: f64| v(x * 10.0, y * 10.0, z * 10.0);
        let square = |z: f64| [c(0., 0., z), c(1., 0., z), c(1., 1., z), c(0., 1., z)];
        m.add_polyline(&square(0.0), true).unwrap();
        m.add_polyline(&square(1.0), true).unwrap();
        for (a, b) in square(0.0).into_iter().zip(square(1.0)) {
            m.add_line(a, b).unwrap();
        }
        assert_eq!(m.face_count(), 6);
        // Opposite faces' diagonals cross over, so no four of these edges
        // lie in one plane through the box (which would make a face there).
        for (a, b) in [
            (c(0., 0., 0.), c(1., 1., 0.)),
            (c(1., 0., 1.), c(0., 1., 1.)),
            (c(0., 0., 0.), c(1., 0., 1.)),
            (c(1., 1., 0.), c(0., 1., 1.)),
            (c(0., 1., 0.), c(0., 0., 1.)),
            (c(1., 0., 0.), c(1., 1., 1.)),
        ] {
            m.add_line(a, b).unwrap();
        }
        assert_eq!(m.face_count(), 12);
        m
    }

    #[test]
    fn twelve_triangles_become_six_quads() {
        let mut m = triangulated_box();
        assert_eq!(m.face_count(), 12);
        let report = clean_up(&mut m, &CleanUp::default()).unwrap();
        assert_eq!(m.face_count(), 6);
        assert_eq!(m.edge_count(), 12);
        assert_eq!(m.vertex_count(), 8);
        assert!(m.face_ids().all(|f| m.face(f).unwrap().outer().len() == 4));
        assert!(report.iter().any(|l| l.text == "Faces Merged: 6"));
        m.validate().unwrap();
    }

    #[test]
    fn merging_across_paint_is_counted() {
        let mut m = Entities::new();
        m.add_polyline(
            &[
                v(0.0, 0.0, 0.0),
                v(10.0, 0.0, 0.0),
                v(10.0, 10.0, 0.0),
                v(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        m.add_line(v(5.0, 0.0, 0.0), v(5.0, 10.0, 0.0)).unwrap();
        assert_eq!(m.face_count(), 2);
        let mut doc = smith_model::Document::new();
        let red = doc
            .add_material(smith_model::Material {
                name: "Red".into(),
                ..smith_model::Material::default()
            })
            .unwrap();
        let f = m.face_ids().next().unwrap();
        let paint = smith_model::Paint {
            material: red,
            mapping: smith_model::Mapping::Face(smith_model::TextureFrame::for_side(
                DVec3::Z,
                smith_geom::DVec2::ZERO,
                0.0,
            )),
        };
        m.set_face_paint(f, smith_model::Side::Front, Some(paint))
            .unwrap();
        let report = clean_up(&mut m, &only(|p| p.merge_coplanar = true)).unwrap();
        assert_eq!(m.face_count(), 1);
        assert!(report.iter().any(|l| l.text == "Paint Borders Merged: 1"));
    }

    #[test]
    fn stray_edges_go_but_drawn_curves_stay() {
        let mut m = Entities::new();
        m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
        m.add_curve(
            smith_geom::curve::Curve::Arc(
                smith_geom::curve::Arc::new(
                    smith_geom::curve::CurveFrame::new(v(50.0, 0.0, 0.0), DVec3::X, DVec3::Z)
                        .unwrap(),
                    5.0,
                    0.0,
                    1.0,
                )
                .unwrap(),
            ),
            6,
        )
        .unwrap();
        let before = m.edge_count();
        clean_up(&mut m, &only(|p| p.stray_edges = true)).unwrap();
        assert_eq!(m.edge_count(), before - 1);
    }

    #[test]
    fn split_edges_join_into_one() {
        let mut m = Entities::new();
        m.add_polyline(
            &[
                v(0.0, 0.0, 0.0),
                v(3.0, 0.0, 0.0),
                v(7.0, 0.0, 0.0),
                v(10.0, 0.0, 0.0),
            ],
            false,
        )
        .unwrap();
        let report = clean_up(&mut m, &only(|p| p.join_split_edges = true)).unwrap();
        assert_eq!(m.edge_count(), 1);
        assert_eq!(report[0].text, "Split Edges Joined: 1");
        assert_eq!(report[0].items.len(), 1);
    }

    #[test]
    fn a_bent_run_joins_only_within_the_angle() {
        let mut m = Entities::new();
        m.add_polyline(
            &[v(0.0, 0.0, 0.0), v(5.0, 0.2, 0.0), v(10.0, 0.0, 0.0)],
            false,
        )
        .unwrap();
        clean_up(&mut m, &only(|p| p.join_split_edges = true)).unwrap();
        assert_eq!(m.edge_count(), 2);
        let mut c = only(|p| p.join_split_edges = true);
        c.angle = 10f64.to_radians();
        clean_up(&mut m, &c).unwrap();
        assert_eq!(m.edge_count(), 1);
    }

    #[test]
    fn faces_merged_onto_the_same_points_are_made_one() {
        let mut m = Entities::new();
        let sq = |z: f64| {
            [
                v(0.0, 0.0, z),
                v(10.0, 0.0, z),
                v(10.0, 10.0, z),
                v(0.0, 10.0, z),
            ]
        };
        m.add_polyline(&sq(0.0), true).unwrap();
        m.add_polyline(&sq(0.005), true).unwrap();
        assert_eq!(m.face_count(), 2);
        let report = clean_up(
            &mut m,
            &only(|p| {
                p.merge_by_distance = true;
                p.duplicate_faces = true;
            }),
        )
        .unwrap();
        assert_eq!(report[0].text, "Points Merged: 4");
        assert_eq!(report[1].text, "Duplicate Faces Removed: 1");
        assert_eq!(m.face_count(), 1);
        assert_eq!(m.edge_count(), 4);
        m.validate().unwrap();
    }

    #[test]
    fn a_selection_limits_the_passes() {
        let mut m = triangulated_box();
        // Only the bottom face's four corners: its two triangles merge,
        // nothing else does.
        let scope: Vec<DVec3> = [
            v(0.0, 0.0, 0.0),
            v(10.0, 0.0, 0.0),
            v(10.0, 10.0, 0.0),
            v(0.0, 10.0, 0.0),
        ]
        .to_vec();
        let mut c = only(|p| p.merge_coplanar = true);
        c.scope = Some(scope);
        clean_up(&mut m, &c).unwrap();
        assert_eq!(m.face_count(), 11);
    }

    #[test]
    fn close_points_merge_and_short_edges_collapse() {
        let mut m = Entities::new();
        m.add_polyline(
            &[
                v(0.0, 0.0, 0.0),
                v(10.0, 0.0, 0.0),
                v(10.0, 5.0, 0.0),
                v(10.0, 5.005, 0.0),
                v(10.0, 10.0, 0.0),
                v(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        let report = clean_up(&mut m, &only(|p| p.zero_size = true)).unwrap();
        assert_eq!(report[0].text, "Short Edges Collapsed: 1");
        assert_eq!(m.vertex_count(), 5);
        assert_eq!(m.face_count(), 1);
        m.validate().unwrap();
    }

    #[test]
    fn fuse_overlapped_faces_makes_two_overlapping_squares_one() {
        let mut m = Entities::new();
        m.add_polyline(
            &[
                v(0.0, 0.0, 0.0),
                v(10.0, 0.0, 0.0),
                v(10.0, 10.0, 0.0),
                v(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        // The second square drawn above and moved down onto the first, so
        // it lies over it without splitting it.
        m.add_polyline(
            &[
                v(5.0, 5.0, 3.0),
                v(15.0, 5.0, 3.0),
                v(15.0, 15.0, 3.0),
                v(5.0, 15.0, 3.0),
            ],
            true,
        )
        .unwrap();
        let up: Vec<VertexId> = m
            .vertex_ids()
            .filter(|id| m.vertex(*id).unwrap().position().z > 1.0)
            .collect();
        m.move_vertices(&up, v(0.0, 0.0, -3.0)).unwrap();
        // Off by default: nothing happens.
        let before = m.clone();
        clean_up(&mut m, &only(|_| {})).unwrap();
        assert_eq!(m, before);
        let report = clean_up(&mut m, &only(|p| p.fuse_overlaps = true)).unwrap();
        assert_eq!(report[0].text, "Overlapped Faces Fused: 2");
        assert_eq!(report[0].items.len(), 1);
        assert_eq!(m.face_count(), 1);
        let f = m.face_ids().next().unwrap();
        assert!((m.face_area(f).unwrap() - 175.0).abs() < 1e-9);
        m.validate().unwrap();
    }
}
