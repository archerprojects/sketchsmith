//! SketchSmith — Intersect Faces acceptance tests | v1.0.0 | Thursday 12 November 2026
//!
//! Faces on different planes are cut where both cover the line their
//! planes share; two overlapping boxes give a closed loop of cut lines.
//! Overlapping faces on one plane are split at the edge of the overlap and
//! left stacked there, keeping their own paint; fusing makes them one face.
//! Against the context, unselected loose faces cut and are cut; closed
//! groups are sealed, and a face lying flat on one is only counted; with
//! the model, loose faces outside an open group cut into it. What cannot
//! be seen is left out.

use std::collections::HashMap;

use smith_geom::{DMat4, DVec3};
use smith_model::{
    Against, DefinitionId, DefinitionKind, Document, Entities, FaceId, InstanceId, VertexId,
};

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn check(m: &Entities) {
    if let Err(e) = m.validate() {
        panic!("invalid model: {e}");
    }
}

fn all_faces(m: &Entities) -> Vec<FaceId> {
    m.face_ids().collect()
}

fn total_area(m: &Entities) -> f64 {
    m.face_ids().map(|f| m.face_area(f).unwrap()).sum()
}

/// A square on the ground, `x0..x1` by `y0..y1`, drawn at height `z` and then
/// moved down to the ground, so it lies over what is there without
/// splitting it (as a moved face does).
fn square_over(m: &mut Entities, x0: f64, y0: f64, x1: f64, y1: f64) {
    let lift = 7.0;
    m.add_polyline(
        &[
            v(x0, y0, lift),
            v(x1, y0, lift),
            v(x1, y1, lift),
            v(x0, y1, lift),
        ],
        true,
    )
    .unwrap();
    let up: Vec<VertexId> = m
        .vertex_ids()
        .filter(|id| m.vertex(*id).unwrap().position().z == lift)
        .collect();
    m.move_vertices(&up, v(0.0, 0.0, -lift)).unwrap();
}

/// The six faces of a box from `lo` to `hi`.
fn add_box(m: &mut Entities, lo: DVec3, hi: DVec3) {
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
}

#[test]
fn two_crossing_faces_get_one_cut_line_and_both_split() {
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
    // A wall through the middle of the square, reaching below it, drawn
    // high and moved down so it isn't split on the way.
    m.add_polyline(
        &[
            v(5.0, -2.0, 20.0),
            v(5.0, 12.0, 20.0),
            v(5.0, 12.0, 25.0),
            v(5.0, -2.0, 25.0),
        ],
        true,
    )
    .unwrap();
    let up: Vec<VertexId> = m
        .vertex_ids()
        .filter(|id| m.vertex(*id).unwrap().position().z >= 20.0)
        .collect();
    m.move_vertices(&up, v(0.0, 0.0, -22.0)).unwrap();
    assert_eq!(m.face_count(), 2);
    let cuts = m
        .intersect_faces(&all_faces(&m), &Against::default())
        .unwrap();
    check(&m);
    assert_eq!(cuts.edges.len(), 1);
    let (a, b) = m.edge_points(cuts.edges[0]).unwrap();
    let (lo, hi) = if a.y < b.y { (a, b) } else { (b, a) };
    assert!(lo.distance(v(5.0, 0.0, 0.0)) < 1e-9);
    assert!(hi.distance(v(5.0, 10.0, 0.0)) < 1e-9);
    // The square is cut in two; the wall, crossed only partway, keeps one
    // face with the cut line running into it.
    assert_eq!(m.face_count(), 3);
    assert_eq!(cuts.splits, 1);
    assert_eq!(cuts.overlaps, 0);
}

#[test]
fn two_overlapping_boxes_give_a_closed_loop_of_cut_lines() {
    let mut m = Entities::new();
    add_box(&mut m, v(0.0, 0.0, 0.0), v(10.0, 10.0, 10.0));
    // Lay the second box's faces in without sticky splitting: draw it
    // far away and move it into place.
    add_box(&mut m, v(105.0, 5.0, 5.0), v(115.0, 15.0, 15.0));
    let far: Vec<VertexId> = m
        .vertex_ids()
        .filter(|id| m.vertex(*id).unwrap().position().x >= 100.0)
        .collect();
    m.move_vertices(&far, v(-100.0, 0.0, 0.0)).unwrap();
    assert_eq!(m.face_count(), 12);
    let cuts = m
        .intersect_faces(&all_faces(&m), &Against::default())
        .unwrap();
    check(&m);
    assert!(cuts.edges.len() >= 6, "cut lines: {}", cuts.edges.len());
    // Every point on the cut lines joins exactly two of them: one closed
    // loop round the overlap.
    let mut ends: HashMap<VertexId, usize> = HashMap::new();
    for e in &cuts.edges {
        for p in m.edge(*e).unwrap().vertices() {
            *ends.entry(p).or_default() += 1;
        }
    }
    assert!(ends.values().all(|n| *n == 2), "{ends:?}");
    assert_eq!(cuts.overlaps, 0);
    assert!(cuts.splits >= 6);
}

#[test]
fn overlapping_squares_on_one_plane_are_split_and_stacked() {
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
    square_over(&mut m, 5.0, 0.0, 15.0, 10.0);
    assert_eq!(m.face_count(), 2);
    let cuts = m
        .intersect_faces(&all_faces(&m), &Against::default())
        .unwrap();
    check(&m);
    assert_eq!(cuts.overlaps, 1);
    assert_eq!(cuts.stacked.len(), 2);
    // Each square is now its own part and the shared part: four faces, the
    // shared part twice over.
    assert_eq!(m.face_count(), 4);
    assert!((total_area(&m) - 200.0).abs() < 1e-9);
    for f in &cuts.stacked {
        assert!((m.face_area(*f).unwrap() - 50.0).abs() < 1e-9);
    }
}

#[test]
fn a_face_inside_another_is_cut_round() {
    let mut m = Entities::new();
    m.add_polyline(
        &[
            v(0.0, 0.0, 0.0),
            v(20.0, 0.0, 0.0),
            v(20.0, 20.0, 0.0),
            v(0.0, 20.0, 0.0),
        ],
        true,
    )
    .unwrap();
    square_over(&mut m, 5.0, 5.0, 10.0, 10.0);
    let cuts = m
        .intersect_faces(&all_faces(&m), &Against::default())
        .unwrap();
    check(&m);
    assert_eq!(cuts.overlaps, 1);
    // The big square with a hole, and the small square twice.
    assert_eq!(m.face_count(), 3);
    let holed = m
        .face_ids()
        .find(|f| !m.face(*f).unwrap().inner().is_empty())
        .unwrap();
    assert!((m.face_area(holed).unwrap() - 375.0).abs() < 1e-9);
}

#[test]
fn faces_that_only_touch_are_left_alone() {
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
    m.add_polyline(
        &[
            v(10.0, 0.0, 0.0),
            v(20.0, 0.0, 0.0),
            v(20.0, 10.0, 0.0),
            v(10.0, 10.0, 0.0),
        ],
        true,
    )
    .unwrap();
    let (faces, edges) = (m.face_count(), m.edge_count());
    let cuts = m
        .intersect_faces(&all_faces(&m), &Against::default())
        .unwrap();
    assert_eq!((m.face_count(), m.edge_count()), (faces, edges));
    assert!(cuts.edges.is_empty() && cuts.overlaps == 0 && cuts.splits == 0);
}

#[test]
fn fusing_two_overlapping_squares_makes_one_face() {
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
    square_over(&mut m, 5.0, 5.0, 15.0, 15.0);
    let (fused, made) = m.fuse_overlaps(&all_faces(&m));
    check(&m);
    assert_eq!(fused, 2);
    assert_eq!(made.len(), 1);
    assert_eq!(m.face_count(), 1);
    assert!((total_area(&m) - 175.0).abs() < 1e-9);
}

fn square(m: &mut Entities, x0: f64, y0: f64, x1: f64, y1: f64) {
    m.add_polyline(
        &[
            v(x0, y0, 0.0),
            v(x1, y0, 0.0),
            v(x1, y1, 0.0),
            v(x0, y1, 0.0),
        ],
        true,
    )
    .unwrap();
}

/// An upright wall across x = 5, from y = −2 to 12 and z = −2 to 3.
fn wall(m: &mut Entities) {
    m.add_polyline(
        &[
            v(5.0, -2.0, -2.0),
            v(5.0, 12.0, -2.0),
            v(5.0, 12.0, 3.0),
            v(5.0, -2.0, 3.0),
        ],
        true,
    )
    .unwrap();
}

/// A group in the top level holding what `fill` draws.
fn group(doc: &mut Document, fill: impl FnOnce(&mut Entities)) -> (DefinitionId, InstanceId) {
    let def = doc.create_definition("Group", DefinitionKind::Group);
    fill(doc.definition_entities_mut(def).unwrap());
    let inst = doc.add_instance(None, def, DMat4::IDENTITY, "").unwrap();
    (def, inst)
}

#[test]
fn against_the_context_an_unselected_loose_face_cuts_and_is_cut() {
    let mut doc = Document::new();
    square(doc.root_mut(), 0.0, 0.0, 10.0, 10.0);
    let floor = all_faces(doc.root())[0];
    // The wall is drawn high and moved down, so it lies across the floor
    // without having split it.
    let m = doc.root_mut();
    m.add_polyline(
        &[
            v(5.0, -2.0, 18.0),
            v(5.0, 12.0, 18.0),
            v(5.0, 12.0, 23.0),
            v(5.0, -2.0, 23.0),
        ],
        true,
    )
    .unwrap();
    let up: Vec<VertexId> = m
        .vertex_ids()
        .filter(|id| m.vertex(*id).unwrap().position().z > 10.0)
        .collect();
    m.move_vertices(&up, v(0.0, 0.0, -20.0)).unwrap();
    let against = doc.intersect_against(&[], false).unwrap();
    assert_eq!(against.here.len(), 2);
    let cuts = doc.root_mut().intersect_faces(&[floor], &against).unwrap();
    check(doc.root());
    assert_eq!(cuts.edges.len(), 1);
    // The floor in two; the wall keeps one face with the cut running in.
    assert_eq!(doc.root().face_count(), 3);
    // With the selection alone, the wall is not cut against.
    let mut alone = Document::new();
    square(alone.root_mut(), 0.0, 0.0, 10.0, 10.0);
    let floor = all_faces(alone.root())[0];
    let cuts = alone
        .root_mut()
        .intersect_faces(&[floor], &Against::default())
        .unwrap();
    assert!(cuts.edges.is_empty());
}

#[test]
fn a_closed_group_is_sealed() {
    let mut doc = Document::new();
    square(doc.root_mut(), 0.0, 0.0, 10.0, 10.0);
    let floor = all_faces(doc.root())[0];
    let (def, _) = group(&mut doc, wall);
    let inside = doc.definition_entities(def).unwrap().clone();
    let against = doc.intersect_against(&[], true).unwrap();
    assert_eq!(against.sealed.len(), 1);
    let cuts = doc.root_mut().intersect_faces(&[floor], &against).unwrap();
    assert!(cuts.edges.is_empty() && cuts.flat_on.is_empty());
    assert_eq!(doc.root().face_count(), 1);
    assert_eq!(doc.definition_entities(def).unwrap(), &inside);
}

#[test]
fn a_face_lying_on_a_group_is_counted_not_cut() {
    let mut doc = Document::new();
    square(doc.root_mut(), 0.0, 0.0, 10.0, 10.0);
    let floor = all_faces(doc.root())[0];
    group(&mut doc, |m| square(m, 5.0, 5.0, 15.0, 15.0));
    let against = doc.intersect_against(&[], false).unwrap();
    let edges = doc.root().edge_count();
    let cuts = doc.root_mut().intersect_faces(&[floor], &against).unwrap();
    assert_eq!(cuts.flat_on, [floor]);
    assert!(cuts.edges.is_empty());
    assert_eq!(doc.root().edge_count(), edges);
}

#[test]
fn with_the_model_loose_faces_outside_cut_into_the_open_group() {
    let mut doc = Document::new();
    wall(doc.root_mut());
    let def = doc.create_definition("Group", DefinitionKind::Group);
    square(
        doc.definition_entities_mut(def).unwrap(),
        0.0,
        0.0,
        10.0,
        10.0,
    );
    // The group sits 1 mm along X, so the wall crosses it at x = 4 inside.
    let inst = doc
        .add_instance(None, def, DMat4::from_translation(v(1.0, 0.0, 0.0)), "")
        .unwrap();
    let floor = all_faces(doc.definition_entities(def).unwrap())[0];
    let context = doc.intersect_against(&[inst], false).unwrap();
    assert!(context.outside.is_empty());
    let model = doc.intersect_against(&[inst], true).unwrap();
    assert_eq!(model.outside.len(), 1);
    let top = doc.root().clone();
    let inside = doc.definition_entities_mut(def).unwrap();
    let cuts = inside.intersect_faces(&[floor], &model).unwrap();
    check(inside);
    assert_eq!(cuts.edges.len(), 1);
    let (a, b) = inside.edge_points(cuts.edges[0]).unwrap();
    assert!((a.x - 4.0).abs() < 1e-9 && (b.x - 4.0).abs() < 1e-9);
    assert_eq!(inside.face_count(), 2);
    // The top level is never changed.
    assert_eq!(doc.root(), &top);
}

#[test]
fn what_cannot_be_seen_is_left_out() {
    let mut doc = Document::new();
    wall(doc.root_mut());
    let w = all_faces(doc.root())[0];
    doc.root_mut().set_face_hidden(w, true).unwrap();
    let off = doc.add_layer("Off", [0, 0, 0]);
    let mut layer = doc.layer(off).unwrap().clone();
    layer.visible = false;
    doc.set_layer(off, layer).unwrap();
    let (_, inst) = group(&mut doc, |m| square(m, 5.0, 5.0, 15.0, 15.0));
    doc.root_mut()
        .set_layer(&[smith_model::Pickable::Instance(inst)], Some(off))
        .unwrap();
    let against = doc.intersect_against(&[], true).unwrap();
    assert!(against.here.is_empty());
    assert!(against.sealed.is_empty());
}
