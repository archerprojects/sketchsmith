//! SketchSmith — sticky geometry acceptance tests | v1.0.0 | Thursday 12 November 2026
//!
//! Acceptance tests: auto-face,
//! face splitting and healing, edge splitting, holes, erasing a vertex, Fill,
//! sub-millimetre geometry,
//! merge tolerance, and non-manifold edges. Every test ends with
//! `Entities::validate`, so structural invariants are checked after each scenario.

use smith_geom::DVec3;
use smith_model::{Entities, ModelError};

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn rect(m: &mut Entities, x0: f64, y0: f64, x1: f64, y1: f64, z: f64) {
    m.add_polyline(
        &[v(x0, y0, z), v(x1, y0, z), v(x1, y1, z), v(x0, y1, z)],
        true,
    )
    .unwrap();
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
fn rectangle_auto_faces() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 20.0, 30.0, 0.0);
    assert_eq!(m.edge_count(), 4);
    assert_eq!(m.face_count(), 1);
    let f = m.face_ids().next().unwrap();
    assert_eq!(m.face_area(f).unwrap(), 600.0);
    assert_eq!(
        m.face(f).unwrap().plane().normal(),
        DVec3::Z,
        "ground faces point up"
    );
    check(&m);
}

#[test]
fn open_chain_makes_no_face() {
    let mut m = Entities::new();
    m.add_polyline(
        &[v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(10.0, 10.0, 0.0)],
        false,
    )
    .unwrap();
    assert_eq!(m.face_count(), 0);
    check(&m);
}

#[test]
fn line_across_face_splits_it_and_erase_heals() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 10.0, 10.0, 0.0);
    let created = m.add_line(v(4.0, 0.0, 0.0), v(4.0, 10.0, 0.0)).unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(m.face_count(), 2);
    let mut areas: Vec<f64> = m.face_ids().map(|f| m.face_area(f).unwrap()).collect();
    areas.sort_by(f64::total_cmp);
    assert_eq!(areas, vec![40.0, 60.0]);
    check(&m);

    m.erase_edge(created[0]).unwrap();
    assert_eq!(m.face_count(), 1, "coplanar faces heal");
    assert_eq!(total_area(&m), 100.0);
    // The two vertices the split left on the outline remain (SketchUp keeps them).
    assert_eq!(m.edge_count(), 6);
    check(&m);
}

#[test]
fn crossing_lines_split_both_edges() {
    let mut m = Entities::new();
    m.add_line(v(0.0, 0.0, 0.0), v(10.0, 10.0, 0.0)).unwrap();
    m.add_line(v(0.0, 10.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
    assert_eq!(m.edge_count(), 4);
    assert_eq!(m.vertex_count(), 5);
    assert!(m.find_vertex(v(5.0, 5.0, 0.0)).is_some());
    check(&m);
}

#[test]
fn line_along_existing_edge_is_not_duplicated() {
    let mut m = Entities::new();
    m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
    let created = m.add_line(v(2.0, 0.0, 0.0), v(15.0, 0.0, 0.0)).unwrap();
    // Existing edge split at 2; only 10→15 is new.
    assert_eq!(created.len(), 1);
    assert_eq!(m.edge_count(), 3);
    check(&m);
}

#[test]
fn circle_inside_face_becomes_hole_and_disc() {
    let mut m = Entities::new();
    rect(&mut m, -20.0, -20.0, 20.0, 20.0, 0.0);
    let circle: Vec<DVec3> = (0..24)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 24.0;
            v(5.0 * a.cos(), 5.0 * a.sin(), 0.0)
        })
        .collect();
    m.add_polyline(&circle, true).unwrap();
    assert_eq!(m.face_count(), 2);
    let plate = m
        .face_ids()
        .find(|f| !m.face(*f).unwrap().inner().is_empty())
        .expect("plate has a hole");
    assert_eq!(m.face(plate).unwrap().inner().len(), 1);
    // Plate area + disc area = original square.
    assert!((total_area(&m) - 1600.0).abs() < 1e-9);
    check(&m);
}

#[test]
fn erasing_a_hole_segment_dissolves_the_hole() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 30.0, 30.0, 0.0);
    rect(&mut m, 10.0, 10.0, 20.0, 20.0, 0.0);
    assert_eq!(m.face_count(), 2);
    let e = m
        .edge_ids()
        .find(|e| {
            let (a, b) = m.edge_points(*e).unwrap();
            a.y == 10.0 && b.y == 10.0
        })
        .unwrap();
    m.erase_edge(e).unwrap();
    assert_eq!(m.face_count(), 1);
    let f = m.face_ids().next().unwrap();
    assert!(m.face(f).unwrap().inner().is_empty());
    assert_eq!(m.face_area(f).unwrap(), 900.0);
    check(&m);
}

#[test]
fn drawing_around_an_existing_face_adopts_it_as_a_hole() {
    let mut m = Entities::new();
    rect(&mut m, 10.0, 10.0, 20.0, 20.0, 0.0);
    rect(&mut m, 0.0, 0.0, 30.0, 30.0, 0.0);
    assert_eq!(m.face_count(), 2);
    assert!((total_area(&m) - 900.0).abs() < 1e-9);
    check(&m);
}

#[test]
fn twelve_edges_of_a_cube_make_six_faces() {
    let mut m = Entities::new();
    let s = 10.0;
    rect(&mut m, 0.0, 0.0, s, s, 0.0);
    rect(&mut m, 0.0, 0.0, s, s, s);
    for (x, y) in [(0.0, 0.0), (s, 0.0), (s, s), (0.0, s)] {
        m.add_line(v(x, y, 0.0), v(x, y, s)).unwrap();
    }
    assert_eq!(m.edge_count(), 12);
    assert_eq!(m.face_count(), 6);
    for e in m.edge_ids() {
        assert_eq!(
            m.edge(e).unwrap().faces().len(),
            2,
            "closed box: two faces per edge"
        );
    }
    check(&m);
}

#[test]
fn fin_on_a_box_edge_gives_three_faces_on_one_edge() {
    let mut m = Entities::new();
    let s = 10.0;
    rect(&mut m, 0.0, 0.0, s, s, 0.0);
    rect(&mut m, 0.0, 0.0, s, s, s);
    for (x, y) in [(0.0, 0.0), (s, 0.0), (s, s), (0.0, s)] {
        m.add_line(v(x, y, 0.0), v(x, y, s)).unwrap();
    }
    // A fin sticking out diagonally from the top front edge (y = 0, z = s).
    m.add_polyline(
        &[
            v(0.0, 0.0, s),
            v(0.0, -5.0, s + 5.0),
            v(s, -5.0, s + 5.0),
            v(s, 0.0, s),
        ],
        false,
    )
    .unwrap();
    let shared = m
        .edge_ids()
        .find(|e| {
            let (a, b) = m.edge_points(*e).unwrap();
            a.y == 0.0 && b.y == 0.0 && a.z == s && b.z == s
        })
        .unwrap();
    assert_eq!(m.edge(shared).unwrap().faces().len(), 3);
    check(&m);
}

#[test]
fn sub_millimetre_rectangle_faces() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 0.001, 0.002, 0.0);
    assert_eq!(m.face_count(), 1);
    let area = total_area(&m);
    assert!((area - 2e-6).abs() < 1e-18);
    check(&m);
}

#[test]
fn sub_millimetre_split() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 0.01, 0.01, 0.0);
    m.add_line(v(0.005, 0.0, 0.0), v(0.005, 0.01, 0.0)).unwrap();
    assert_eq!(m.face_count(), 2);
    check(&m);
}

#[test]
fn merge_tolerance_boundary() {
    let mut m = Entities::new();
    m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
    // 0.00005 mm from the end: merges.
    m.add_line(v(10.0 + 5e-5, 0.0, 0.0), v(10.0, 10.0, 0.0))
        .unwrap();
    assert_eq!(m.vertex_count(), 3);
    // 0.0002 mm away from the far end, off the line: a new vertex.
    m.add_line(v(10.0, 10.0 + 2e-4, 0.0), v(0.0, 10.0, 5.0))
        .unwrap();
    assert_eq!(m.vertex_count(), 5);
    check(&m);
}

#[test]
fn zero_length_and_non_finite_lines_are_rejected() {
    let mut m = Entities::new();
    assert_eq!(
        m.add_line(v(1.0, 1.0, 1.0), v(1.0, 1.0, 1.0 + 1e-5)),
        Err(ModelError::ZeroLength)
    );
    assert_eq!(
        m.add_line(v(f64::NAN, 0.0, 0.0), v(1.0, 0.0, 0.0)),
        Err(ModelError::NonFinite)
    );
    assert_eq!(m.edge_count(), 0);
}

#[test]
fn erasing_a_lone_boundary_edge_removes_the_face() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 5.0, 5.0, 0.0);
    let e = m.edge_ids().next().unwrap();
    m.erase_edge(e).unwrap();
    assert_eq!(m.face_count(), 0);
    assert_eq!(m.edge_count(), 3);
    check(&m);
}

#[test]
fn tilted_face() {
    let mut m = Entities::new();
    // A rectangle on a 30° slope.
    let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let p = |x: f64, d: f64| v(x, d * c, d * s);
    m.add_polyline(
        &[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)],
        true,
    )
    .unwrap();
    assert_eq!(m.face_count(), 1);
    assert!((total_area(&m) - 100.0).abs() < 1e-9);
    check(&m);
}

#[test]
fn plus_sign_on_a_face_makes_four_quarters() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 10.0, 10.0, 0.0);
    m.add_line(v(5.0, 0.0, 0.0), v(5.0, 10.0, 0.0)).unwrap();
    m.add_line(v(0.0, 5.0, 0.0), v(10.0, 5.0, 0.0)).unwrap();
    assert_eq!(m.face_count(), 4);
    for f in m.face_ids() {
        assert_eq!(m.face_area(f).unwrap(), 25.0);
    }
    check(&m);
}

#[test]
fn diagonal_corner_to_corner_split() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 10.0, 10.0, 0.0);
    m.add_line(v(0.0, 0.0, 0.0), v(10.0, 10.0, 0.0)).unwrap();
    assert_eq!(m.face_count(), 2);
    for f in m.face_ids() {
        assert!((m.face_area(f).unwrap() - 50.0).abs() < 1e-12);
    }
    check(&m);
}

#[test]
fn split_keeps_hole_in_the_right_half() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 40.0, 20.0, 0.0);
    rect(&mut m, 5.0, 5.0, 10.0, 10.0, 0.0); // hole in the left half
    m.add_line(v(20.0, 0.0, 0.0), v(20.0, 20.0, 0.0)).unwrap();
    assert_eq!(m.face_count(), 3);
    let holed: Vec<_> = m
        .face_ids()
        .filter(|f| !m.face(*f).unwrap().inner().is_empty())
        .collect();
    assert_eq!(holed.len(), 1);
    assert_eq!(m.face_area(holed[0]).unwrap(), 400.0 - 25.0);
    check(&m);
}

#[test]
fn heal_keeps_holes_from_both_faces() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 40.0, 20.0, 0.0);
    let split = m.add_line(v(20.0, 0.0, 0.0), v(20.0, 20.0, 0.0)).unwrap();
    rect(&mut m, 5.0, 5.0, 10.0, 10.0, 0.0);
    rect(&mut m, 25.0, 5.0, 30.0, 10.0, 0.0);
    assert_eq!(m.face_count(), 4);
    m.erase_edge(split[0]).unwrap();
    assert_eq!(m.face_count(), 3);
    let big = m
        .face_ids()
        .find(|f| m.face(*f).unwrap().inner().len() == 2)
        .expect("healed face keeps both holes");
    assert_eq!(m.face_area(big).unwrap(), 800.0 - 50.0);
    check(&m);
}

#[test]
fn face_triangles_cover_the_face_and_face_front() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 20.0, 20.0, 0.0);
    rect(&mut m, 5.0, 5.0, 10.0, 10.0, 0.0);
    let plate = m
        .face_ids()
        .find(|f| !m.face(*f).unwrap().inner().is_empty())
        .unwrap();
    let tris = m.face_triangles(plate);
    let normal = m.face(plate).unwrap().plane().normal();
    let mut area = 0.0;
    for [a, b, c] in tris {
        let n = (b - a).cross(c - a);
        assert!(n.dot(normal) > 0.0, "triangle faces the face front");
        area += n.length() * 0.5;
    }
    assert!((area - 375.0).abs() < 1e-9);
    assert_eq!(m.bounds(), Some((v(0.0, 0.0, 0.0), v(20.0, 20.0, 0.0))));
}

#[test]
fn a_loop_drawn_round_a_hole_takes_the_hole() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 10.0, 10.0, 0.0);
    rect(&mut m, 4.0, 4.0, 6.0, 6.0, 0.0);
    let disc = m
        .face_ids()
        .find(|f| (m.face_area(*f).unwrap() - 4.0).abs() < 1e-9)
        .unwrap();
    m.erase_face(disc).unwrap();
    rect(&mut m, 2.0, 2.0, 8.0, 8.0, 0.0);
    check(&m);
    let mut areas: Vec<f64> = m.face_ids().map(|f| m.face_area(f).unwrap()).collect();
    areas.sort_by(f64::total_cmp);
    // The band 100 - 36 and the inner face 36 - 4, which now holds the hole.
    assert_eq!(areas.len(), 2);
    assert!(
        (areas[0] - 32.0).abs() < 1e-9 && (areas[1] - 64.0).abs() < 1e-9,
        "{areas:?}"
    );
}

#[test]
fn new_geometry_goes_on_the_active_layer_and_splits_keep_theirs() {
    use smith_model::Document;
    let mut doc = Document::new();
    rect(doc.root_mut(), 0.0, 0.0, 10.0, 10.0, 0.0);
    let red = doc.add_layer("Red", [200, 0, 0]);
    doc.set_active_layer(Some(red));
    // A line across the Base square splits it: both halves stay on Base,
    // and the new line is on Red.
    let new = doc
        .root_mut()
        .add_line(v(5.0, 0.0, 0.0), v(5.0, 10.0, 0.0))
        .unwrap();
    let m = doc.root();
    check(m);
    assert!(
        m.face_ids()
            .all(|f| m.face(f).unwrap().attributes.layer.is_none())
    );
    assert!(
        new.iter()
            .all(|e| m.edge(*e).unwrap().flags.layer == Some(red))
    );
    // A square drawn in empty space is wholly on Red.
    rect(doc.root_mut(), 20.0, 0.0, 25.0, 5.0, 0.0);
    let m = doc.root();
    let on_red = m
        .face_ids()
        .filter(|f| m.face(*f).unwrap().attributes.layer == Some(red))
        .count();
    assert_eq!(on_red, 1);
    // Undo keeps the active layer; removing the layer falls back to Base.
    doc.remove_layer(red).unwrap();
    assert_eq!(doc.active_layer(), None);
}

#[test]
fn layers_are_numbered_by_creation() {
    use smith_model::Document;
    let mut doc = Document::new();
    let names = |d: &Document| -> Vec<String> {
        d.layer_ids()
            .filter_map(|l| d.layer(l).map(|x| x.name.clone()))
            .collect()
    };
    let one = doc.add_numbered_layer([0, 0, 0]);
    let two = doc.add_numbered_layer([0, 0, 0]);
    let three = doc.add_numbered_layer([0, 0, 0]);
    assert_eq!(names(&doc), ["Base", "1", "2", "3"]);
    // The newest gives its number back.
    doc.remove_layer(three).unwrap();
    let three = doc.add_numbered_layer([0, 0, 0]);
    assert_eq!(doc.layer(three).unwrap().name, "3");
    // An older one does not.
    doc.remove_layer(two).unwrap();
    let four = doc.add_numbered_layer([0, 0, 0]);
    assert_eq!(doc.layer(four).unwrap().name, "4");
    // A renamed layer keeps its number; a typed name that matches the next
    // number is skipped.
    let mut l = doc.layer(four).unwrap().clone();
    l.name = "5".into();
    doc.set_layer(four, l).unwrap();
    let next = doc.add_numbered_layer([0, 0, 0]);
    assert_eq!(doc.layer(next).unwrap().name, "6");
    let _ = one;
}

#[test]
fn erasing_a_square_corner_leaves_an_open_l_and_no_face() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 10.0, 10.0, 0.0);
    let corner = m.find_vertex(v(0.0, 0.0, 0.0)).unwrap();
    m.erase_vertex(corner).unwrap();
    check(&m);
    assert_eq!(m.edge_count(), 2);
    assert_eq!(m.vertex_count(), 3);
    assert_eq!(m.face_count(), 0);
}

#[test]
fn erasing_the_centre_of_a_fan_takes_every_face_round_it() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 10.0, 10.0, 0.0);
    for p in [
        v(0.0, 0.0, 0.0),
        v(10.0, 0.0, 0.0),
        v(10.0, 10.0, 0.0),
        v(0.0, 10.0, 0.0),
    ] {
        m.add_line(v(5.0, 5.0, 0.0), p).unwrap();
    }
    assert_eq!(m.face_count(), 4);
    let centre = m.find_vertex(v(5.0, 5.0, 0.0)).unwrap();
    m.erase_vertex(centre).unwrap();
    check(&m);
    // Nothing heals: the outline stays, with no face.
    assert_eq!(m.face_count(), 0);
    assert_eq!(m.edge_count(), 4);
}

#[test]
fn fill_closes_an_open_l_through_its_three_vertices() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 10.0, 10.0, 0.0);
    let corner = m.find_vertex(v(0.0, 0.0, 0.0)).unwrap();
    m.erase_vertex(corner).unwrap();
    let picks: Vec<_> = [v(10.0, 0.0, 0.0), v(10.0, 10.0, 0.0), v(0.0, 10.0, 0.0)]
        .iter()
        .map(|p| m.find_vertex(*p).unwrap())
        .collect();
    m.fill(&picks, &[]).unwrap();
    check(&m);
    // The two edges there are reused, one is drawn: a triangle.
    assert_eq!(m.edge_count(), 3);
    assert_eq!(m.face_count(), 1);
    let f = m.face_ids().next().unwrap();
    assert_eq!(m.face_area(f).unwrap(), 50.0);
}

#[test]
fn fill_with_two_vertices_draws_the_edge_that_closes_the_loop() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 10.0, 10.0, 0.0);
    let corner = m.find_vertex(v(0.0, 0.0, 0.0)).unwrap();
    m.erase_vertex(corner).unwrap();
    let a = m.find_vertex(v(10.0, 0.0, 0.0)).unwrap();
    let c = m.find_vertex(v(0.0, 10.0, 0.0)).unwrap();
    m.fill(&[a, c], &[]).unwrap();
    check(&m);
    assert_eq!((m.edge_count(), m.face_count()), (3, 1));
}

#[test]
fn fill_gives_an_outline_of_selected_edges_its_face() {
    let mut m = Entities::new();
    rect(&mut m, 0.0, 0.0, 10.0, 10.0, 0.0);
    let f = m.face_ids().next().unwrap();
    m.erase_face(f).unwrap();
    let edges: Vec<_> = m.edge_ids().collect();
    m.fill(&[], &edges).unwrap();
    check(&m);
    assert_eq!(m.face_count(), 1);
    // The four corners alone do the same.
    let f = m.face_ids().next().unwrap();
    m.erase_face(f).unwrap();
    let corners: Vec<_> = m.vertex_ids().collect();
    let ordered: Vec<_> = [
        v(0.0, 0.0, 0.0),
        v(10.0, 0.0, 0.0),
        v(10.0, 10.0, 0.0),
        v(0.0, 10.0, 0.0),
    ]
    .iter()
    .map(|p| m.find_vertex(*p).unwrap())
    .collect();
    assert_eq!(corners.len(), 4);
    m.fill(&ordered, &[]).unwrap();
    check(&m);
    assert_eq!((m.edge_count(), m.face_count()), (4, 1));
}

#[test]
fn fill_on_edges_closing_no_loop_is_refused() {
    let mut m = Entities::new();
    // An open L.
    m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
    m.add_line(v(10.0, 0.0, 0.0), v(10.0, 10.0, 0.0)).unwrap();
    let edges: Vec<_> = m.edge_ids().collect();
    let e = m.fill(&[], &edges).unwrap_err();
    assert_eq!(e.to_string(), "These Edges Close No Flat Loop");
    assert_eq!(m.face_count(), 0);
}

#[test]
fn fill_off_one_plane_is_refused_unless_auto_tri() {
    let mut m = Entities::new();
    let pts = [
        v(0.0, 0.0, 0.0),
        v(10.0, 0.0, 0.0),
        v(10.0, 10.0, 3.0),
        v(0.0, 10.0, 0.0),
    ];
    for p in pts {
        m.add_line(p, p + v(0.0, 0.0, -20.0)).unwrap();
    }
    let ids: Vec<_> = pts.iter().map(|p| m.find_vertex(*p).unwrap()).collect();
    let before = m.clone();
    assert!(matches!(m.fill(&ids, &[]), Err(ModelError::NotFlat(_))));
    assert_eq!(m, before);
    smith_model::with_auto_tri(|| m.fill(&ids, &[])).unwrap();
    check(&m);
    // Two triangles, joined by one soft, smooth fold.
    let tops = m
        .face_ids()
        .filter(|f| m.face(*f).unwrap().outer().len() == 3)
        .count();
    assert_eq!(tops, 2);
    let soft = m
        .edge_ids()
        .filter(|e| m.edge(*e).unwrap().flags.soft && m.edge(*e).unwrap().flags.smooth)
        .count();
    assert_eq!(soft, 1);
}

#[test]
fn fill_refuses_vertices_in_a_line() {
    let mut m = Entities::new();
    m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
    m.add_line(v(20.0, 0.0, 0.0), v(20.0, 5.0, 0.0)).unwrap();
    let ids: Vec<_> = [v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(20.0, 0.0, 0.0)]
        .iter()
        .map(|p| m.find_vertex(*p).unwrap())
        .collect();
    assert!(matches!(m.fill(&ids, &[]), Err(ModelError::Refused(_))));
}
