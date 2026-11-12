//! SketchSmith — curve and undo acceptance tests | v1.0.0 | Thursday 12 November 2026
//!
//! Acceptance tests: curve
//! segment regeneration, splitting, explode, the extrusion lock, undo/redo
//! restoring identical models, and the definition/instance skeleton.

use smith_geom::curve::{Circle, Curve, CurveFrame};
use smith_geom::{DMat4, DVec3};
use smith_model::{DefinitionKind, Document, Entities, ModelError};

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn rect(m: &mut Entities, x0: f64, y0: f64, x1: f64, y1: f64) -> Result<(), ModelError> {
    m.add_polyline(
        &[
            v(x0, y0, 0.0),
            v(x1, y0, 0.0),
            v(x1, y1, 0.0),
            v(x0, y1, 0.0),
        ],
        true,
    )
    .map(|_| ())
}

fn circle(r: f64) -> Curve {
    Curve::Circle(
        Circle::new(CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap(), r).unwrap(),
    )
}

fn check(m: &Entities) {
    if let Err(e) = m.validate() {
        panic!("invalid model: {e}");
    }
}

fn total_area(m: &Entities) -> f64 {
    m.face_ids().map(|f| m.face_area(f).unwrap()).sum()
}

#[test]
fn circle_records_its_edges_and_faces() {
    let mut m = Entities::new();
    let c = m.add_curve(circle(5.0), 24).unwrap();
    let rec = m.curve(c).unwrap();
    assert_eq!(rec.edges().len(), 24);
    assert_eq!(m.face_count(), 1);
    for e in rec.edges() {
        assert_eq!(m.edge(*e).unwrap().curve(), Some(c));
    }
    check(&m);
}

#[test]
fn changing_segments_regenerates_and_keeps_faces_valid() {
    // Acceptance: 24 → 48 segments regenerates edges and keeps the face valid.
    let mut m = Entities::new();
    rect(&mut m, -20.0, -20.0, 20.0, 20.0).unwrap();
    let c = m.add_curve(circle(5.0), 24).unwrap();
    assert_eq!(m.face_count(), 2);
    m.set_curve_segments(c, 48).unwrap();
    assert_eq!(m.curve(c).unwrap().edges().len(), 48);
    assert_eq!(m.curve(c).unwrap().segments(), 48);
    assert_eq!(m.edge_count(), 4 + 48);
    assert_eq!(m.face_count(), 2, "plate with hole + disc");
    assert!((total_area(&m) - 1600.0).abs() < 1e-9);
    check(&m);
    assert!(matches!(
        m.set_curve_segments(c, 2),
        Err(ModelError::Geometry(_))
    ));
}

#[test]
fn crossing_line_keeps_curve_editable() {
    let mut m = Entities::new();
    let c = m.add_curve(circle(5.0), 24).unwrap();
    m.add_line(v(-10.0, 0.3, 0.0), v(10.0, 0.3, 0.0)).unwrap();
    assert_eq!(
        m.curve(c).unwrap().edges().len(),
        26,
        "two split pieces stay in the curve"
    );
    assert_eq!(m.face_count(), 2, "disc split by the line");
    m.set_curve_segments(c, 12).unwrap();
    assert_eq!(m.curve(c).unwrap().edges().len(), 14);
    assert_eq!(m.face_count(), 2, "regeneration re-splits at the crossing");
    check(&m);
}

#[test]
fn explode_and_erase_turn_curves_into_plain_edges() {
    let mut m = Entities::new();
    let c = m.add_curve(circle(5.0), 24).unwrap();
    m.explode_curve(c).unwrap();
    assert_eq!(m.curve_count(), 0);
    assert!(m.edge_ids().all(|e| m.edge(e).unwrap().curve().is_none()));
    check(&m);

    // One segment of a circle erased: the rest stays a curve, as an arc whose
    // points are exactly the remaining vertices.
    let c = m.add_curve(circle(9.0), 12).unwrap();
    let first = m.curve(c).unwrap().edges()[0];
    m.erase_edge(first).unwrap();
    let record = m.curve(c).expect("the circle becomes an arc");
    assert!(matches!(record.curve(), smith_geom::curve::Curve::Arc(_)));
    assert_eq!(record.segments(), 11);
    assert_eq!(record.edges().len(), 11);
    let points = record.curve().tessellate(11).unwrap().points;
    assert_eq!(points.len(), 12);
    for p in points {
        assert!(m.find_vertex(p).is_some(), "arc point {p} is a vertex");
    }
    check(&m);

    // Erasing part of the arc explodes it.
    let next = m.curve(c).unwrap().edges()[0];
    m.erase_edge(next).unwrap();
    assert!(
        m.curve(c).is_none(),
        "erasing an arc edge explodes the curve"
    );
    check(&m);
}

#[test]
fn extruded_curve_is_locked() {
    let mut m = Entities::new();
    let c = m.add_curve(circle(5.0), 24).unwrap();
    // Stand a wall on one circle segment: an out-of-plane face on a curve edge.
    let e = m.curve(c).unwrap().edges()[0];
    let (a, b) = m.edge_points(e).unwrap();
    m.add_polyline(&[a, a + DVec3::Z * 5.0, b + DVec3::Z * 5.0, b], false)
        .unwrap();
    assert!(m.curve_is_locked(c));
    assert_eq!(m.set_curve_segments(c, 48), Err(ModelError::CurveLocked));
    check(&m);
}

#[test]
fn sub_millimetre_circle() {
    let mut m = Entities::new();
    let c = m.add_curve(circle(0.05), 24).unwrap();
    assert_eq!(m.face_count(), 1);
    m.set_curve_segments(c, 48).unwrap();
    assert_eq!(m.face_count(), 1);
    check(&m);
}

#[test]
fn undo_redo_restore_identical_models() {
    let mut doc = Document::new();
    let empty = doc.clone();

    doc.transaction("Rectangle", |d| rect(d.root_mut(), 0.0, 0.0, 10.0, 10.0))
        .unwrap();
    let after_rect = doc.clone();
    doc.transaction("Line", |d| {
        d.root_mut()
            .add_line(v(5.0, 0.0, 0.0), v(5.0, 10.0, 0.0))
            .map(|_| ())
    })
    .unwrap();
    let after_line = doc.clone();
    assert_eq!(doc.root().face_count(), 2);

    assert_eq!(doc.undo().as_deref(), Some("Line"));
    assert!(doc.same_content(&after_rect));
    assert_eq!(doc.root().face_count(), 1);
    check(doc.root());

    assert_eq!(doc.undo().as_deref(), Some("Rectangle"));
    assert!(doc.same_content(&empty));
    assert!(doc.undo().is_none());

    doc.redo();
    doc.redo();
    assert!(doc.same_content(&after_line));
    check(doc.root());
}

#[test]
fn undo_erase_heal_and_curve_regeneration() {
    let mut doc = Document::new();
    let c = doc
        .transaction("Plate and circle", |d| {
            rect(d.root_mut(), -20.0, -20.0, 20.0, 20.0)?;
            d.root_mut().add_curve(circle(5.0), 24)
        })
        .unwrap();
    let before = doc.clone();
    doc.transaction("Segments", |d| d.root_mut().set_curve_segments(c, 48))
        .unwrap();
    let after = doc.clone();
    doc.undo();
    assert!(doc.same_content(&before));
    check(doc.root());
    doc.redo();
    assert!(doc.same_content(&after));
    check(doc.root());

    // Erase a plate edge (removes the plate face), then undo.
    let plate_edge = doc
        .root()
        .edge_ids()
        .find(|e| doc.root().edge(*e).unwrap().curve().is_none())
        .unwrap();
    let before_erase = doc.clone();
    doc.transaction("Erase", |d| d.root_mut().erase_edge(plate_edge))
        .unwrap();
    doc.undo();
    assert!(doc.same_content(&before_erase));
    check(doc.root());
}

#[test]
fn failed_operation_rolls_back_completely() {
    let mut doc = Document::new();
    doc.transaction("Rect", |d| rect(d.root_mut(), 0.0, 0.0, 4.0, 4.0))
        .unwrap();
    let before = doc.clone();
    let r = doc.transaction("Half done", |d| {
        d.root_mut().add_line(v(2.0, 0.0, 0.0), v(2.0, 4.0, 0.0))?;
        d.root_mut()
            .add_line(v(f64::NAN, 0.0, 0.0), v(1.0, 1.0, 0.0))
    });
    assert_eq!(r, Err(ModelError::NonFinite));
    assert!(doc.same_content(&before));
    assert_eq!(doc.undo_name(), Some("Rect"));
    check(doc.root());
}

#[test]
fn definitions_and_instances_undo() {
    let mut doc = Document::new();
    let empty = doc.clone();
    let def = doc
        .transaction("Make Component", |d| {
            let def = d.create_definition("Bead", DefinitionKind::Component);
            d.definition_entities_mut(def)
                .ok_or(ModelError::NoSuchEntity)?
                .add_curve(circle(1.0), 24)?;
            d.add_instance(None, def, DMat4::IDENTITY, "")?;
            d.add_instance(
                None,
                def,
                DMat4::from_translation(v(5.0, 0.0, 0.0)),
                "second",
            )?;
            Ok(def)
        })
        .unwrap();
    let made = doc.clone();
    assert_eq!(doc.root().instance_count(), 2);
    assert_eq!(doc.definition_entities(def).unwrap().face_count(), 1);

    // Edit the existing definition in a second step, then undo just that.
    doc.transaction("Edit", |d| {
        d.definition_entities_mut(def)
            .ok_or(ModelError::NoSuchEntity)?
            .add_line(v(-2.0, 0.0, 0.0), v(2.0, 0.0, 0.0))
            .map(|_| ())
    })
    .unwrap();
    assert_eq!(doc.definition_entities(def).unwrap().face_count(), 2);
    doc.undo();
    assert!(doc.same_content(&made));

    doc.undo();
    assert!(doc.same_content(&empty));
    doc.redo();
    assert!(doc.same_content(&made));

    // A definition cannot contain itself.
    let r = doc.transaction("Bad", |d| {
        d.add_instance(Some(def), def, DMat4::IDENTITY, "")
    });
    assert_eq!(r, Err(ModelError::RecursiveInstance));
}

#[test]
fn removing_an_edited_definition_undoes_to_its_original_state() {
    let mut doc = Document::new();
    let def = doc
        .transaction("Make Group", |d| {
            let def = d.create_definition("Group", DefinitionKind::Group);
            rect(
                d.definition_entities_mut(def)
                    .ok_or(ModelError::NoSuchEntity)?,
                0.0,
                0.0,
                2.0,
                2.0,
            )?;
            Ok(def)
        })
        .unwrap();
    let before = doc.clone();
    doc.transaction("Edit then delete", |d| {
        d.definition_entities_mut(def)
            .ok_or(ModelError::NoSuchEntity)?
            .add_line(v(1.0, 0.0, 0.0), v(1.0, 2.0, 0.0))?;
        d.remove_definition(def)
    })
    .unwrap();
    assert!(doc.definition(def).is_none());
    doc.undo();
    assert!(doc.same_content(&before));
}
