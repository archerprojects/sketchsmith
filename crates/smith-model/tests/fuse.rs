//! SketchSmith — Fuse acceptance tests | v1.0.0 | Thursday 12 November 2026
//!
//! Two overlapping boxes: Union, Difference and Intersect each leave one
//! closed solid of the right volume, the target gone. Boxes sharing a face
//! join without an inner wall; boxes apart refuse Intersect. Paint stays
//! on the faces it was on. Refusals change nothing. Faces on one plane
//! fuse by area.

use smith_geom::{DMat4, DVec3};
use smith_model::{
    BooleanOp, DefinitionId, DefinitionKind, Document, Entities, InstanceId, Material, ModelError,
    Paint, Side, TextureFrame,
};

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// A closed box from the origin to `size`, faces outward.
fn solid_box(m: &mut Entities, size: DVec3) {
    let c = |x: bool, y: bool, z: bool| {
        v(
            if x { size.x } else { 0.0 },
            if y { size.y } else { 0.0 },
            if z { size.z } else { 0.0 },
        )
    };
    for q in [
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
        m.add_polyline(&q, true).unwrap();
    }
    let centre = size * 0.5;
    for f in m.face_ids().collect::<Vec<_>>() {
        let face = m.face(f).unwrap();
        let p = m.loop_points(face.outer())[0];
        if face.plane().normal().dot(p - centre) < 0.0 {
            m.reverse_face(f).unwrap();
        }
    }
}

/// A box group of `size` placed at `at`.
fn box_group(doc: &mut Document, size: DVec3, at: DVec3) -> (DefinitionId, InstanceId) {
    let def = doc.create_definition("Box", DefinitionKind::Group);
    solid_box(doc.definition_entities_mut(def).unwrap(), size);
    let inst = doc
        .add_instance(None, def, DMat4::from_translation(at), "")
        .unwrap();
    (def, inst)
}

/// Volume of a closed, outward solid.
fn volume(m: &Entities) -> f64 {
    m.face_ids()
        .flat_map(|f| m.face_triangles(f))
        .map(|[a, b, c]| a.dot(b.cross(c)) / 6.0)
        .sum()
}

fn closed(m: &Entities) -> bool {
    m.face_count() > 0 && m.edge_ids().all(|e| m.edge(e).unwrap().faces().len() == 2)
}

fn fused(op: BooleanOp) -> (Document, DefinitionId) {
    let mut doc = Document::new();
    let (def, base) = box_group(&mut doc, DVec3::splat(10.0), DVec3::ZERO);
    let (_, target) = box_group(&mut doc, DVec3::splat(10.0), DVec3::splat(5.0));
    doc.fuse_groups(None, base, target, op).unwrap();
    assert_eq!(doc.root().instance_count(), 1, "the target is gone");
    (doc, def)
}

#[test]
fn union_difference_and_intersect_of_two_boxes() {
    for (op, want) in [
        (BooleanOp::Union, 1000.0 + 1000.0 - 125.0),
        (BooleanOp::Difference, 1000.0 - 125.0),
        (BooleanOp::Intersect, 125.0),
    ] {
        let (doc, def) = fused(op);
        let m = doc.definition_entities(def).unwrap();
        m.validate().unwrap();
        assert!(closed(m), "{op:?} is closed");
        assert!((volume(m) - want).abs() < 1e-6, "{op:?}: {}", volume(m));
    }
}

#[test]
fn boxes_sharing_a_face_join_without_an_inner_wall() {
    let mut doc = Document::new();
    let (def, base) = box_group(&mut doc, DVec3::splat(10.0), DVec3::ZERO);
    let (_, target) = box_group(&mut doc, DVec3::splat(10.0), v(10.0, 0.0, 0.0));
    doc.fuse_groups(None, base, target, BooleanOp::Union)
        .unwrap();
    let m = doc.definition_entities(def).unwrap();
    assert!(closed(m));
    assert!((volume(m) - 2000.0).abs() < 1e-6);
    // A 20 × 10 × 10 block: six faces again.
    assert_eq!(m.face_count(), 6);
}

#[test]
fn boxes_apart_leave_nothing_to_intersect() {
    let mut doc = Document::new();
    let (_, base) = box_group(&mut doc, DVec3::splat(10.0), DVec3::ZERO);
    let (_, target) = box_group(&mut doc, DVec3::splat(10.0), v(50.0, 0.0, 0.0));
    let r = doc.fuse_groups(None, base, target, BooleanOp::Intersect);
    assert!(matches!(r, Err(ModelError::Refused(ref m)) if m == "Nothing Would Be Left"));
}

#[test]
fn paint_stays_on_its_faces() {
    let mut doc = Document::new();
    let red = doc.add_material(Material::default()).unwrap();
    let (def, base) = box_group(&mut doc, DVec3::splat(10.0), DVec3::ZERO);
    let (tdef, target) = box_group(&mut doc, DVec3::splat(10.0), DVec3::splat(5.0));
    // Paint the target's far top.
    let t = doc.definition_entities_mut(tdef).unwrap();
    let top = t
        .face_ids()
        .find(|f| t.face(*f).unwrap().plane().normal().z > 0.9)
        .unwrap();
    let paint = Paint {
        material: red,
        mapping: smith_model::Mapping::Face(TextureFrame::for_side(
            DVec3::Z,
            smith_geom::DVec2::ZERO,
            0.0,
        )),
    };
    t.set_face_paint(top, Side::Front, Some(paint)).unwrap();
    doc.fuse_groups(None, base, target, BooleanOp::Union)
        .unwrap();
    let m = doc.definition_entities(def).unwrap();
    let painted: Vec<_> = m
        .face_ids()
        .filter(|f| m.face_paint(*f, Side::Front).is_some())
        .collect();
    assert_eq!(painted.len(), 1);
    assert!(m.face(painted[0]).unwrap().plane().normal().z > 0.9);
}

#[test]
fn refusals_change_nothing() {
    // Not closed.
    let mut doc = Document::new();
    let (bdef, base) = box_group(&mut doc, DVec3::splat(10.0), DVec3::ZERO);
    let (_, target) = box_group(&mut doc, DVec3::splat(10.0), DVec3::splat(5.0));
    let b = doc.definition_entities_mut(bdef).unwrap();
    let f = b.face_ids().next().unwrap();
    b.erase_face(f).unwrap();
    let before = doc.clone();
    let r = doc.fuse_groups(None, base, target, BooleanOp::Union);
    assert!(
        matches!(r, Err(ModelError::Refused(ref m)) if m == "The Base Is Not Closed: Use Make Manifold")
    );
    assert_eq!(doc.root(), before.root());
    // Locked.
    let mut doc = Document::new();
    let (_, base) = box_group(&mut doc, DVec3::splat(10.0), DVec3::ZERO);
    let (_, target) = box_group(&mut doc, DVec3::splat(10.0), DVec3::splat(5.0));
    doc.root_mut().set_instance_locked(base, true).unwrap();
    let r = doc.fuse_groups(None, base, target, BooleanOp::Union);
    assert!(matches!(r, Err(ModelError::Refused(ref m)) if m == "Unlock the Groups First"));
    // Groups inside.
    let mut doc = Document::new();
    let (bdef, base) = box_group(&mut doc, DVec3::splat(10.0), DVec3::ZERO);
    let (_, target) = box_group(&mut doc, DVec3::splat(10.0), DVec3::splat(5.0));
    let (inner, _) = box_group(&mut doc, DVec3::splat(1.0), DVec3::ZERO);
    doc.add_instance(Some(bdef), inner, DMat4::IDENTITY, "")
        .unwrap();
    let r = doc.fuse_groups(None, base, target, BooleanOp::Union);
    assert!(matches!(r, Err(ModelError::Refused(ref m)) if m == "Explode the Groups Inside First"));
}

#[test]
fn faces_on_one_plane_fuse_by_area() {
    let square = |m: &mut Entities, x: f64, y: f64| {
        m.add_polyline(
            &[
                v(x, y, 9.0),
                v(x + 10.0, y, 9.0),
                v(x + 10.0, y + 10.0, 9.0),
                v(x, y + 10.0, 9.0),
            ],
            true,
        )
        .unwrap();
    };
    for (op, want) in [
        (BooleanOp::Union, 175.0),
        (BooleanOp::Difference, 75.0),
        (BooleanOp::Intersect, 25.0),
    ] {
        let mut m = Entities::new();
        square(&mut m, 0.0, 0.0);
        let base = m.face_ids().next().unwrap();
        // The target drawn 20 mm higher, then moved onto the base's plane,
        // so it overlaps without splitting it.
        m.add_polyline(
            &[
                v(5.0, 5.0, 29.0),
                v(15.0, 5.0, 29.0),
                v(15.0, 15.0, 29.0),
                v(5.0, 15.0, 29.0),
            ],
            true,
        )
        .unwrap();
        let up: Vec<_> = m
            .vertex_ids()
            .filter(|id| m.vertex(*id).unwrap().position().z > 20.0)
            .collect();
        m.move_vertices(&up, v(0.0, 0.0, -20.0)).unwrap();
        let target = m.face_ids().find(|f| *f != base).unwrap();
        m.fuse_faces(base, target, op).unwrap();
        m.validate().unwrap();
        let area: f64 = m.face_ids().map(|f| m.face_area(f).unwrap()).sum();
        assert!((area - want).abs() < 1e-9, "{op:?}: {area}");
    }
}
