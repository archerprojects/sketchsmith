//! SketchSmith — paint acceptance tests | v1.0.0 | Thursday 12 November 2026
//!
//! Paint follows the face it is on: splitting, healing and rebuilding a
//! curve keep it; a face moved whole carries it, a stretched face leaves it
//! where it lies; Reverse Faces keeps it on its physical side. Painting is not
//! a hand edit. Materials and the smoothing angle undo like any other step,
//! and the plain-data form round-trips paint exactly, leaving out materials
//! and wraps nothing is painted with. Purging unused groups leaves
//! materials alone; removing unused materials is its own step.

use smith_geom::curve::{Circle, Curve, CurveFrame};
use smith_geom::{DVec2, DVec3};
use smith_model::{
    Axis, DefinitionKind, Document, EdgeMark, Entities, FaceId, Mapping, Material, MaterialId,
    ModelError, Paint, Projection, ShapeSpec, Side, TextureFrame, Wrap,
};

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn rect(m: &mut Entities, x0: f64, y0: f64, x1: f64, y1: f64) {
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

fn check(m: &Entities) {
    if let Err(e) = m.validate() {
        panic!("invalid model: {e}");
    }
}

fn material(doc: &mut Document, name: &str) -> MaterialId {
    doc.add_material(Material {
        name: name.into(),
        ..Material::default()
    })
    .unwrap()
}

fn face_paint(id: MaterialId) -> Paint {
    Paint {
        material: id,
        mapping: Mapping::Face(TextureFrame::for_side(DVec3::Z, DVec2::ZERO, 0.0)),
    }
}

fn only_face(m: &Entities) -> FaceId {
    assert_eq!(m.face_count(), 1);
    m.face_ids().next().unwrap()
}

#[test]
fn both_pieces_of_a_split_face_keep_its_paint() {
    let mut doc = Document::new();
    let red = material(&mut doc, "Red");
    let m = doc.root_mut();
    rect(m, 0.0, 0.0, 10.0, 10.0);
    let f = only_face(m);
    m.set_face_paint(f, Side::Front, Some(face_paint(red)))
        .unwrap();
    m.add_line(v(5.0, 0.0, 0.0), v(5.0, 10.0, 0.0)).unwrap();
    assert_eq!(m.face_count(), 2);
    for f in m.face_ids().collect::<Vec<_>>() {
        assert_eq!(m.face_paint(f, Side::Front), Some(&face_paint(red)));
        assert_eq!(m.face_paint(f, Side::Back), None);
    }
    check(m);
}

#[test]
fn a_face_moved_whole_carries_its_texture_a_stretched_one_leaves_it() {
    let mut doc = Document::new();
    let red = material(&mut doc, "Red");
    let m = doc.root_mut();
    rect(m, 0.0, 0.0, 10.0, 10.0);
    let f = only_face(m);
    m.set_face_paint(f, Side::Front, Some(face_paint(red)))
        .unwrap();
    let corner = m.find_vertex(v(10.0, 10.0, 0.0)).unwrap();
    let all: Vec<_> = m.vertex_ids().collect();

    // Moved whole: the point that showed texture (10, 10) still does.
    m.move_vertices(&all, v(3.0, 0.0, 0.0)).unwrap();
    let p = *m.face_paint(f, Side::Front).unwrap();
    let uv = m.paint_uvs(&p, DVec3::Z, &[v(13.0, 10.0, 0.0)]).unwrap();
    assert!(uv[0].distance(DVec2::new(10.0, 10.0)) < 1e-9);

    // Stretched: the frame stays where it was.
    m.move_vertices(&[corner], v(0.0, 5.0, 0.0)).unwrap();
    assert_eq!(m.face_paint(f, Side::Front), Some(&p));
    check(m);
}

#[test]
fn a_wrap_moved_in_part_is_split_so_each_place_has_its_own() {
    let mut doc = Document::new();
    let red = material(&mut doc, "Red");
    let m = doc.root_mut();
    rect(m, 0.0, 0.0, 10.0, 10.0);
    rect(m, 20.0, 0.0, 30.0, 10.0);
    let wrap = m
        .add_wrap(Wrap::new(
            Projection::Planar,
            Axis::Z,
            [DVec3::X, DVec3::Y, DVec3::Z],
            v(15.0, 5.0, 0.0),
            1.0,
        ))
        .unwrap();
    let faces: Vec<_> = m.face_ids().collect();
    for f in &faces {
        m.set_face_paint(
            *f,
            Side::Front,
            Some(Paint {
                material: red,
                mapping: Mapping::Wrap(wrap),
            }),
        )
        .unwrap();
    }
    assert_eq!(m.wrap_users(wrap).len(), 2);
    // Move the second square only.
    let right: Vec<_> = m
        .vertex_ids()
        .filter(|id| m.vertex(*id).unwrap().position().x > 15.0)
        .collect();
    m.move_vertices(&right, v(0.0, 50.0, 0.0)).unwrap();
    assert_eq!(m.wrap_users(wrap).len(), 1);
    assert_eq!(m.wrap_ids().count(), 2);
    check(m);
}

#[test]
fn reversing_a_face_keeps_paint_on_its_physical_side() {
    let mut doc = Document::new();
    let red = material(&mut doc, "Red");
    let m = doc.root_mut();
    rect(m, 0.0, 0.0, 10.0, 10.0);
    let f = only_face(m);
    m.set_face_paint(f, Side::Front, Some(face_paint(red)))
        .unwrap();
    m.reverse_face(f).unwrap();
    assert_eq!(m.face_paint(f, Side::Front), None);
    assert_eq!(m.face_paint(f, Side::Back), Some(&face_paint(red)));
}

#[test]
fn painting_a_shape_group_keeps_its_recipe_and_undo_unpaints() {
    let mut doc = Document::new();
    let red = material(&mut doc, "Red");
    let spec = ShapeSpec::Box {
        width: 1.0,
        depth: 2.0,
        height: 3.0,
    };
    let def = doc
        .transaction("Box", |d| {
            let def = d.create_definition("Box", DefinitionKind::Group);
            rect(
                d.context_entities_mut(Some(def))
                    .ok_or(ModelError::NoSuchEntity)?,
                0.0,
                0.0,
                1.0,
                2.0,
            );
            d.set_definition_shape(def, Some(spec))?;
            Ok(def)
        })
        .unwrap();
    doc.transaction("Paint", |d| {
        let g = d
            .context_entities_mut(Some(def))
            .ok_or(ModelError::NoSuchEntity)?;
        let f = g.face_ids().next().ok_or(ModelError::NoSuchEntity)?;
        g.set_face_paint(f, Side::Front, Some(face_paint(red)))
    })
    .unwrap();
    assert_eq!(doc.definition(def).unwrap().shape, Some(spec));
    // A mark on an edge is not a hand edit either.
    doc.transaction("Mark", |d| {
        let g = d
            .context_entities_mut(Some(def))
            .ok_or(ModelError::NoSuchEntity)?;
        let e = g.edge_ids().next().ok_or(ModelError::NoSuchEntity)?;
        g.set_edge_mark(e, EdgeMark::Sharp)
    })
    .unwrap();
    assert_eq!(doc.definition(def).unwrap().shape, Some(spec));
    doc.undo();
    doc.undo();
    let g = doc.definition_entities(def).unwrap();
    let f = g.face_ids().next().unwrap();
    assert_eq!(g.face_paint(f, Side::Front), None);
}

#[test]
fn materials_and_the_smoothing_angle_undo_like_any_step() {
    let mut doc = Document::new();
    let a = doc
        .transaction("Add", |d| {
            d.add_material(Material {
                name: "Red".into(),
                ..Material::default()
            })
        })
        .unwrap();
    // Names stay unique.
    let b = doc
        .transaction("Add", |d| {
            d.add_material(Material {
                name: "Red".into(),
                ..Material::default()
            })
        })
        .unwrap();
    assert_eq!(doc.material(b).unwrap().name, "Red 2");
    let m = doc.root_mut();
    rect(m, 0.0, 0.0, 10.0, 10.0);
    let f = only_face(m);
    m.set_face_paint(f, Side::Front, Some(face_paint(a)))
        .unwrap();
    doc.transaction("Replace", |d| d.replace_material(a, Some(b)))
        .unwrap();
    assert_eq!(doc.root().face_paint(f, Side::Front).unwrap().material, b);
    doc.transaction("Angle", |d| d.set_smoothing_angle(45.0))
        .unwrap();
    assert_eq!(doc.look().smoothing_angle_deg, 45.0);
    doc.undo();
    assert_eq!(doc.look().smoothing_angle_deg, 30.0);
    doc.undo();
    assert_eq!(doc.root().face_paint(f, Side::Front).unwrap().material, a);
    doc.transaction("Remove", |d| d.remove_material(a)).unwrap();
    assert_eq!(doc.root().face_paint(f, Side::Front), None);
    doc.undo();
    assert_eq!(doc.root().face_paint(f, Side::Front).unwrap().material, a);
    assert!(doc.set_smoothing_angle(200.0).is_err());
}

#[test]
fn rebuilding_a_curve_keeps_the_paint_of_the_faces_beside_it() {
    let mut doc = Document::new();
    let red = material(&mut doc, "Red");
    let blue = material(&mut doc, "Blue");
    let m = doc.root_mut();
    rect(m, -20.0, -20.0, 20.0, 20.0);
    let circle = Curve::Circle(
        Circle::new(
            CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap(),
            5.0,
        )
        .unwrap(),
    );
    let c = m.add_curve(circle, 24).unwrap();
    assert_eq!(m.face_count(), 2);
    // The disc red, the ring blue.
    for f in m.face_ids().collect::<Vec<_>>() {
        let disc = m.face(f).unwrap().inner().is_empty();
        let id = if disc { red } else { blue };
        m.set_face_paint(f, Side::Front, Some(face_paint(id)))
            .unwrap();
    }
    m.set_curve_segments(c, 48).unwrap();
    assert_eq!(m.face_count(), 2);
    for f in m.face_ids().collect::<Vec<_>>() {
        let disc = m.face(f).unwrap().inner().is_empty();
        let id = if disc { red } else { blue };
        assert_eq!(m.face_paint(f, Side::Front).unwrap().material, id);
    }
    check(m);
}

#[test]
fn paint_marks_and_styles_round_trip_and_unused_wraps_are_left_out() {
    let mut doc = Document::new();
    let red = material(&mut doc, "Red");
    let m = doc.root_mut();
    rect(m, 0.0, 0.0, 10.0, 10.0);
    let f = only_face(m);
    let axes = [DVec3::X, DVec3::Y, DVec3::Z];
    // A wrap nothing is painted with.
    m.add_wrap(Wrap::new(Projection::Box, Axis::Z, axes, DVec3::ZERO, 1.0))
        .unwrap();
    let used = m
        .add_wrap(Wrap::new(
            Projection::Cylinder,
            Axis::X,
            axes,
            DVec3::ZERO,
            4.0,
        ))
        .unwrap();
    m.set_face_paint(f, Side::Front, Some(face_paint(red)))
        .unwrap();
    m.set_face_paint(
        f,
        Side::Back,
        Some(Paint {
            material: red,
            mapping: Mapping::Wrap(used),
        }),
    )
    .unwrap();
    let e = m.edge_ids().next().unwrap();
    m.set_edge_mark(e, EdgeMark::Smooth).unwrap();
    doc.set_smoothing_angle(20.0).unwrap();

    // A material nothing is painted with.
    material(&mut doc, "Unused");
    let data = doc.to_data();
    assert_eq!(data.root.wraps.len(), 1);
    assert_eq!(data.materials.len(), 1);
    let back = Document::from_data(data.clone()).unwrap();
    assert_eq!(back.to_data(), data);
    assert_eq!(back.look().smoothing_angle_deg, 20.0);
    let g = back.root();
    let f = g.face_ids().next().unwrap();
    let Some(Paint {
        mapping: Mapping::Wrap(w),
        ..
    }) = g.face_paint(f, Side::Back)
    else {
        panic!("back paint lost");
    };
    assert_eq!(g.wrap(*w).unwrap().projection, Projection::Cylinder);
}

#[test]
fn a_copy_keeps_the_originals_layer_flags_and_paint() {
    let mut doc = Document::new();
    let red = material(&mut doc, "Red");
    let layer = doc.add_layer("Two", [0, 0, 0]);
    let m = doc.root_mut();
    rect(m, 0.0, 0.0, 10.0, 10.0);
    let f = only_face(m);
    m.set_face_paint(f, Side::Front, Some(face_paint(red)))
        .unwrap();
    let edges: Vec<_> = m.edge_ids().collect();
    m.set_layer(&[smith_model::Pickable::Face(f)], Some(layer))
        .unwrap();
    m.set_edge_mark(edges[0], EdgeMark::Sharp).unwrap();
    let look = m.capture_look(&edges);
    // Copy the square 20 mm along X, as Move with Copy draws it.
    let shift = smith_geom::DMat4::from_translation(v(20.0, 0.0, 0.0));
    m.add_polyline(
        &[
            v(20.0, 0.0, 0.0),
            v(30.0, 0.0, 0.0),
            v(30.0, 10.0, 0.0),
            v(20.0, 10.0, 0.0),
        ],
        true,
    )
    .unwrap();
    m.apply_look(&look, &shift);
    let copy = m.face_ids().find(|g| *g != f).unwrap();
    let face = m.face(copy).unwrap();
    assert_eq!(face.attributes.layer, Some(layer));
    let p = *m.face_paint(copy, Side::Front).unwrap();
    let uv = m.paint_uvs(&p, DVec3::Z, &[v(25.0, 5.0, 0.0)]).unwrap();
    assert!(uv[0].distance(DVec2::new(5.0, 5.0)) < 1e-9);
    let marked = m
        .edge_ids()
        .filter(|e| m.edge(*e).unwrap().flags.mark == EdgeMark::Sharp)
        .count();
    assert_eq!(marked, 2);
    check(m);
}

#[test]
fn purging_groups_keeps_materials_until_they_are_removed_on_their_own() {
    let mut doc = Document::new();
    let red = material(&mut doc, "Red");
    let blue = material(&mut doc, "Blue");
    let green = material(&mut doc, "Green");
    let m = doc.root_mut();
    rect(m, 0.0, 0.0, 10.0, 10.0);
    let f = only_face(m);
    m.set_face_paint(f, Side::Front, Some(face_paint(red)))
        .unwrap();
    assert_eq!(doc.unused_material_count(), 2);
    assert_eq!(doc.purge_unused_groups().unwrap(), 0);
    assert_eq!(doc.material_ids().count(), 3);
    assert_eq!(doc.remove_unused_materials(), 2);
    let left: Vec<MaterialId> = doc.material_ids().collect();
    assert_eq!(left, [red]);
    assert!(doc.material(blue).is_none() && doc.material(green).is_none());
    assert_eq!(doc.unused_material_count(), 0);
    assert_eq!(doc.remove_unused_materials(), 0);
}
