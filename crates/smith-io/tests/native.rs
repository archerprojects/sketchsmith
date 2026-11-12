//! SketchSmith — native format tests | v1.0.0 | Thursday 12 November 2026
//!
//! Round trips of the native `.glb` format:
//! save → open reproduces the model exactly, including curves of every kind,
//! edge flags, holes, fins, groups, components, the view and the drawing axes,
//! materials with their images, paint, wraps, edge marks and line styles and
//! the saved mode; and that smoothing (marks, the angle) never changes the
//! geometry saved;
//! the viewer layer is in metres; a newer schema is flagged; a `.glb`
//! from another application is refused; saving is atomic.
//!
//! The files in `tests/fixtures/` are checked against freshly built models,
//! so a change to the format that would break existing files fails here. The
//! first five were written with schema 1 and are kept as they are, to prove
//! older files still open; `layers_hidden_locked.glb` is schema 2 and
//! `guides.glb` schema 3, `annotations.glb` schema 4, `scenes.glb`
//! schema 5, `sections.glb` schema 6, `shape_recipe.glb` schema 7 and
//! `sweep_recipe.glb` schema 8, `loft_recipe.glb` schema 9 and
//! `edges_recipe.glb` schema 10, `paint_look.glb` schema 11. To write a
//! fixture that is missing, run
//! `SKETCHSMITH_WRITE_FIXTURES=1 cargo test -p smith-io --test native`
//! (existing fixtures are never overwritten).

use std::path::PathBuf;

use glam::{DMat4, DVec3};
use serde_json::Value;
use smith_geom::curve::{Arc, Bezier, Circle, Curve, CurveFrame, Helix, Polygon, Polyline};
use smith_io::{IoError, NativeFile, SavedAxes, SavedView, read_glb, write_glb};
use smith_model::{
    Anchor, Annotation, Axis, DefinitionKind, Document, EdgeMark, FaceDisplay, Guide, ImageFormat,
    Layer, LeaderText, LinePattern, LineStyle, LoftOptions, Mapping, Material, ModelError, Paint,
    PenWeight, Pickable, Projection, Scene, SceneStyle, SceneView, SectionPlane, ShapeSpec, Side,
    SweepRecipe, SweepSection, Texture, TextureFrame, TextureImage, Wrap,
};

fn p(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

fn draw(doc: &mut Document, f: impl FnOnce(&mut Document) -> Result<(), ModelError>) {
    doc.transaction("build", f).expect("build fixture model");
}

/// A 10 mm cube made of 12 lines (auto-face gives 6 faces).
fn cube() -> NativeFile {
    let mut doc = Document::new();
    draw(&mut doc, |d| {
        let r = d.root_mut();
        let c = [
            p(0.0, 0.0, 0.0),
            p(10.0, 0.0, 0.0),
            p(10.0, 10.0, 0.0),
            p(0.0, 10.0, 0.0),
        ];
        for i in 0..4 {
            let (a, b) = (c[i], c[(i + 1) % 4]);
            let up = p(0.0, 0.0, 10.0);
            r.add_line(a, b)?;
            r.add_line(a + up, b + up)?;
            r.add_line(a, a + up)?;
        }
        Ok(())
    });
    NativeFile {
        document: doc,
        ..Default::default()
    }
}

/// A square with a circular hole, crossed by a line, plus a dangling line.
fn square_with_hole() -> NativeFile {
    let mut doc = Document::new();
    draw(&mut doc, |d| {
        let r = d.root_mut();
        let s = [
            p(0.0, 0.0, 0.0),
            p(50.0, 0.0, 0.0),
            p(50.0, 50.0, 0.0),
            p(0.0, 50.0, 0.0),
        ];
        for i in 0..4 {
            r.add_line(s[i], s[(i + 1) % 4])?;
        }
        let frame = CurveFrame::new(p(25.0, 25.0, 0.0), DVec3::X, DVec3::Z)?;
        r.add_curve(Curve::Circle(Circle::new(frame, 10.0)?), 24)?;
        r.add_line(p(0.0, 25.0, 0.0), p(50.0, 25.0, 0.0))?;
        r.add_line(p(50.0, 50.0, 0.0), p(70.0, 65.0, 5.0))?;
        Ok(())
    });
    NativeFile {
        document: doc,
        ..Default::default()
    }
}

/// One curve of every kind, in different planes.
fn all_curves() -> NativeFile {
    let mut doc = Document::new();
    draw(&mut doc, |d| {
        let r = d.root_mut();
        let tilted = CurveFrame::new(p(0.0, 0.0, 0.0), p(1.0, 0.2, 0.0), p(0.1, -0.3, 1.0))?;
        r.add_curve(Curve::Arc(Arc::new(tilted, 12.5, 0.3, 2.9)?), 12)?;
        let f = CurveFrame::new(p(40.0, 0.0, 0.0), DVec3::X, DVec3::Z)?;
        r.add_curve(Curve::Circle(Circle::new(f, 7.25)?), 36)?;
        let f = CurveFrame::new(p(80.0, 0.0, 0.0), DVec3::Y, DVec3::X)?;
        r.add_curve(Curve::Polygon(Polygon::new(f, 9.0, 6)?), 6)?;
        r.add_curve(
            Curve::Bezier(Bezier::new(vec![
                p(0.0, 40.0, 0.0),
                p(10.0, 60.0, 0.0),
                p(30.0, 20.0, 0.0),
                p(40.0, 40.0, 0.0),
                p(50.0, 60.0, 3.0),
                p(60.0, 45.0, 0.0),
                p(70.0, 40.0, 0.0),
            ])?),
            8,
        )?;
        r.add_curve(
            Curve::Polyline(Polyline::new(
                vec![p(0.0, 80.0, 0.0), p(15.0, 90.0, 0.0), p(30.0, 80.0, 0.0)],
                false,
            )?),
            2,
        )?;
        let f = CurveFrame::new(p(120.0, 0.0, 0.0), DVec3::X, DVec3::Z)?;
        r.add_curve(Curve::Helix(Helix::new(f, 10.0, 4.0, 3.5, 4.25, true)?), 96)?;
        Ok(())
    });
    NativeFile {
        document: doc,
        ..Default::default()
    }
}

/// A box corner with a fin (an edge used by three faces) and every edge flag.
fn flags_and_fin() -> NativeFile {
    let mut doc = Document::new();
    draw(&mut doc, |d| {
        let r = d.root_mut();
        let loops = [
            [
                p(0.0, 0.0, 0.0),
                p(20.0, 0.0, 0.0),
                p(20.0, 20.0, 0.0),
                p(0.0, 20.0, 0.0),
            ],
            [
                p(0.0, 0.0, 0.0),
                p(20.0, 0.0, 0.0),
                p(20.0, 0.0, 20.0),
                p(0.0, 0.0, 20.0),
            ],
            [
                p(0.0, 0.0, 0.0),
                p(20.0, 0.0, 0.0),
                p(20.0, -15.0, -10.0),
                p(0.0, -15.0, -10.0),
            ],
        ];
        for l in loops {
            for i in 0..4 {
                r.add_line(l[i], l[(i + 1) % 4])?;
            }
        }
        Ok(())
    });
    // Edge flags have no editing tool yet; set them through the data form.
    let mut data = doc.to_data();
    for (i, e) in data.root.edges.iter_mut().enumerate() {
        e.flags.soft = i % 3 == 0;
        e.flags.smooth = i % 3 == 0 || i % 4 == 1;
        e.flags.hidden = i % 5 == 2;
    }
    NativeFile {
        document: Document::from_data(data).expect("flagged"),
        ..Default::default()
    }
}

/// A component placed twice, a group nested inside it, the view and axes.
fn groups_view_axes() -> NativeFile {
    let mut doc = Document::new();
    draw(&mut doc, |d| {
        let comp = d.create_definition("Bracket", DefinitionKind::Component);
        let group = d.create_definition("Pin", DefinitionKind::Group);
        let g = d
            .definition_entities_mut(group)
            .ok_or(ModelError::NoSuchEntity)?;
        g.add_line(p(0.0, 0.0, 0.0), p(0.0, 0.0, 8.0))?;
        let c = d
            .definition_entities_mut(comp)
            .ok_or(ModelError::NoSuchEntity)?;
        let s = [
            p(0.0, 0.0, 0.0),
            p(30.0, 0.0, 0.0),
            p(30.0, 12.0, 0.0),
            p(0.0, 12.0, 0.0),
        ];
        for i in 0..4 {
            c.add_line(s[i], s[(i + 1) % 4])?;
        }
        d.add_instance(
            Some(comp),
            group,
            DMat4::from_translation(p(5.0, 6.0, 0.0)),
            "",
        )?;
        d.add_instance(None, comp, DMat4::IDENTITY, "Left")?;
        d.add_instance(
            None,
            comp,
            DMat4::from_rotation_z(0.7) * DMat4::from_translation(p(50.0, 0.0, 0.0)),
            "Right",
        )?;
        d.root_mut()
            .add_line(p(-10.0, -10.0, 0.0), p(90.0, -10.0, 0.0))?;
        Ok(())
    });
    NativeFile {
        document: doc,
        view: Some(SavedView {
            target: p(20.0, 5.0, 0.0),
            distance: 240.0,
            yaw: -1.1,
            pitch: 0.6,
            perspective: false,
            fov_y: 35f64.to_radians(),
            eye: p(20.0, 5.0, 0.0)
                + p(
                    0.6f64.cos() * (-1.1f64).cos(),
                    0.6f64.cos() * (-1.1f64).sin(),
                    0.6f64.sin(),
                ) * 240.0,
            up: DVec3::Z,
            half_height: 75.66,
            aspect: 1.6,
        }),
        axes: Some(SavedAxes {
            origin: p(5.0, 5.0, 0.0),
            x: p(0.6, 0.8, 0.0),
            y: p(-0.8, 0.6, 0.0),
            z: DVec3::Z,
        }),
        present: false,
    }
}

type Builder = fn() -> NativeFile;

/// Schema 2: a layer with a face and its edges on it, a hidden face, and a
/// locked group on a hidden layer.
fn layers_hidden_locked() -> NativeFile {
    let mut doc = Document::new();
    draw(&mut doc, |d| {
        let red = d.add_layer("Red", [200, 30, 30]);
        let off = d.add_layer("Off", [30, 30, 200]);
        d.set_layer(
            off,
            Layer {
                name: "Off".into(),
                number: 2,
                color: [30, 30, 200],
                visible: false,
                style: None,
                locked: false,
                order: 2,
            },
        )?;
        let r = d.root_mut();
        r.add_polyline(
            &[
                p(0.0, 0.0, 0.0),
                p(5.0, 0.0, 0.0),
                p(5.0, 5.0, 0.0),
                p(0.0, 5.0, 0.0),
            ],
            true,
        )?;
        r.add_polyline(
            &[
                p(10.0, 0.0, 0.0),
                p(15.0, 0.0, 0.0),
                p(15.0, 5.0, 0.0),
                p(10.0, 5.0, 0.0),
            ],
            true,
        )?;
        let faces: Vec<_> = r.face_ids().collect();
        let first = faces[0];
        let mut items = vec![Pickable::Face(first)];
        let outer = r.face(first).expect("face").outer().to_vec();
        items.extend(outer.iter().map(|u| Pickable::Edge(u.edge)));
        r.set_layer(&items, Some(red))?;
        r.set_face_hidden(faces[1], true)?;
        let def = d.create_definition("Lid", DefinitionKind::Group);
        d.context_entities_mut(Some(def))
            .expect("def")
            .add_line(p(0.0, 0.0, 0.0), p(1.0, 1.0, 1.0))?;
        let i = d.add_instance(None, def, DMat4::from_translation(p(0.0, 0.0, 9.0)), "")?;
        let r = d.root_mut();
        r.set_layer(&[Pickable::Instance(i)], Some(off))?;
        r.set_instance_locked(i, true)?;
        Ok(())
    });
    NativeFile {
        document: doc,
        ..Default::default()
    }
}

/// Guides (schema 3): a guide line 2.5 mm off a cube edge, a slanted guide
/// line, a guide point, and a guide inside a group.
fn guides() -> NativeFile {
    let mut file = cube();
    draw(&mut file.document, |d| {
        let r = d.root_mut();
        r.add_guide(Guide::line(p(0.0, -2.5, 0.0), p(1.0, 0.0, 0.0)).expect("line"))?;
        r.add_guide(Guide::line(p(0.0, 0.0, 0.0), p(1.0, 1.0, 0.3)).expect("slant"))?;
        r.add_guide(Guide::point(p(5.0, 5.0, 10.0)).expect("point"))?;
        let def = d.create_definition("Jig", DefinitionKind::Group);
        let g = d.context_entities_mut(Some(def)).expect("def");
        g.add_line(p(0.0, 0.0, 0.0), p(3.0, 0.0, 0.0))?;
        g.add_guide(Guide::point(p(1.5, 0.0, 0.0)).expect("point"))?;
        d.add_instance(None, def, DMat4::from_translation(p(20.0, 0.0, 0.0)), "")?;
        Ok(())
    });
    file
}

/// Dimensions and text (schema 4) on the square with a hole: a linear
/// dimension across the square, the hole's radius and diameter, an angle
/// at a corner, leader text of every kind, and screen text.
fn annotations() -> NativeFile {
    let mut file = square_with_hole();
    draw(&mut file.document, |d| {
        let r = d.root_mut();
        let a = Anchor::at(r, p(0.0, 0.0, 0.0));
        let b = Anchor::at(r, p(50.0, 0.0, 0.0));
        let c = Anchor::at(r, p(0.0, 50.0, 0.0));
        let curve = r.curve_ids().next();
        let edge = r.edge_ids().next();
        let face = r.face_ids().next();
        let notes = [
            Annotation::Linear {
                a,
                b,
                offset: p(0.0, -8.0, 0.0),
            },
            Annotation::Radial {
                curve,
                diameter: false,
                centre: p(25.0, 25.0, 0.0),
                radius: 10.0,
                label: p(14.0, 14.0, 0.0),
            },
            Annotation::Radial {
                curve,
                diameter: true,
                centre: p(25.0, 25.0, 0.0),
                radius: 10.0,
                label: p(-14.0, 14.0, 0.0),
            },
            Annotation::Angular {
                centre: a,
                a: b,
                b: c,
                radius: 12.0,
            },
            Annotation::Leader {
                anchor: b,
                label: p(10.0, -10.0, 0.0),
                text: LeaderText::Length { edge, value: 50.0 },
            },
            Annotation::Leader {
                anchor: Anchor::Point(p(10.0, 40.0, 0.0)),
                label: p(-12.0, 6.0, 0.0),
                text: LeaderText::Area {
                    face,
                    value: 2185.84,
                },
            },
            Annotation::Leader {
                anchor: c,
                label: p(-6.0, 8.0, 0.0),
                text: LeaderText::Coordinates,
            },
            Annotation::Leader {
                anchor: Anchor::Point(p(60.0, 60.0, 0.0)),
                label: p(5.0, 5.0, 3.0),
                text: LeaderText::Custom("Drill Ø 3 · “through”".into()),
            },
            Annotation::Screen {
                at: [0.05, 0.08],
                text: "Front plate".into(),
            },
        ];
        for n in notes {
            r.add_annotation(n)?;
        }
        Ok(())
    });
    file
}

/// Scenes (schema 5): the cube with a second layer, a perspective scene
/// that hides it and a parallel Present-mode scene that shows everything.
fn scenes() -> NativeFile {
    let mut file = cube();
    draw(&mut file.document, |d| {
        let red = d.add_layer("Red", [200, 30, 30]);
        d.add_scene(Scene {
            name: "Front".into(),
            view: SceneView {
                target: p(5.0, 5.0, 5.0),
                distance: 80.0,
                yaw: -std::f64::consts::FRAC_PI_2,
                pitch: 0.1,
                perspective: true,
                fov_y: std::f64::consts::FRAC_PI_4,
            },
            present: false,
            style: SceneStyle::default(),
            hidden_layers: vec![red],
            section: None,
        })?;
        d.add_scene(Scene {
            name: "Plan · “final”".into(),
            view: SceneView {
                target: p(5.0, 5.0, 0.0),
                distance: 120.0,
                yaw: 0.0,
                pitch: std::f64::consts::FRAC_PI_2,
                perspective: false,
                fov_y: std::f64::consts::FRAC_PI_4,
            },
            present: true,
            style: SceneStyle::default(),
            hidden_layers: Vec::new(),
            section: None,
        })?;
        Ok(())
    });
    file
}

/// Section planes (schema 6): the cube with a level plane (active) and an
/// upright one, and a scene that turns the upright one on.
fn sections() -> NativeFile {
    let mut file = cube();
    draw(&mut file.document, |d| {
        let r = d.root_mut();
        let _level =
            r.add_section(SectionPlane::new(p(5.0, 5.0, 4.5), DVec3::Z).expect("level"))?;
        let upright = r.add_section(
            SectionPlane::new(p(2.5, 0.0, 0.0), p(-1.0, 0.0, 0.0)).expect("upright"),
        )?;
        r.set_active_section(Some(_level))?;
        d.add_scene(Scene {
            name: "Cut".into(),
            view: SceneView {
                target: p(5.0, 5.0, 5.0),
                distance: 60.0,
                yaw: 0.5,
                pitch: 0.4,
                perspective: true,
                fov_y: std::f64::consts::FRAC_PI_4,
            },
            present: false,
            style: SceneStyle {
                faces: FaceDisplay::Wireframe,
                back_edges: false,
            },
            hidden_layers: Vec::new(),
            section: Some(upright),
        })?;
        Ok(())
    });
    file
}

/// A shape recipe (schema 7): a group made as a box that keeps its sizes.
fn shape_recipe() -> NativeFile {
    let mut file = cube();
    draw(&mut file.document, |d| {
        let def = d.create_definition("Box", DefinitionKind::Group);
        let g = d.context_entities_mut(Some(def)).expect("def");
        g.add_line(p(0.0, 0.0, 0.0), p(4.0, 0.0, 0.0))?;
        d.set_definition_shape(
            def,
            Some(ShapeSpec::Torus {
                ring: 12.5,
                tube: 2.25,
                segments: 36,
                sides: 10,
            }),
        )?;
        d.add_instance(None, def, DMat4::from_translation(p(30.0, 0.0, 0.0)), "")?;
        Ok(())
    });
    file
}

/// Sweep recipes (schema 8): a sweep with an outline profile and a
/// revolve, each a group that keeps what it was made from.
fn sweep_recipe() -> NativeFile {
    let mut file = cube();
    draw(&mut file.document, |d| {
        let sweep = SweepRecipe::Sweep {
            path: vec![p(0.0, 0.0, 0.0), p(20.0, 0.0, 0.0), p(20.0, 15.5, 0.0)],
            closed: false,
            path2: None,
            section: SweepSection::Outline(vec![
                p(0.0, -1.0, -1.0),
                p(0.0, 1.0, -1.0),
                p(0.0, 1.0, 1.25),
                p(0.0, -1.0, 1.25),
            ]),
            up: p(0.0, 0.0, 1.0),
            taper: 0.75,
            twist: 0.5,
            upright: true,
            align: false,
        };
        let revolve = SweepRecipe::Revolve {
            outline: vec![p(5.0, 0.0, 0.0), p(7.0, 0.0, 0.0), p(7.0, 0.0, 2.0)],
            axis_point: p(0.0, 0.0, 0.0),
            axis_dir: p(0.0, 0.0, 1.0),
            angle: 1.5,
            steps: 9,
        };
        for (name, recipe, x) in [("Sweep", sweep, 40.0), ("Revolve", revolve, 80.0)] {
            let def = d.create_definition(name, DefinitionKind::Group);
            let g = d.context_entities_mut(Some(def)).expect("def");
            g.add_line(p(0.0, 0.0, 0.0), p(4.0, 0.0, 0.0))?;
            d.set_definition_sweep(def, Some(recipe))?;
            d.add_instance(None, def, DMat4::from_translation(p(x, 0.0, 0.0)), "")?;
        }
        Ok(())
    });
    file
}

/// Loft recipes (schema 9): a square lofted into a triangle along a bent
/// path, and three profiles lofted by spline, each a group that keeps them
/// and their options.
fn loft_recipe() -> NativeFile {
    let mut file = cube();
    draw(&mut file.document, |d| {
        let along = SweepRecipe::Loft {
            path: Some(vec![
                p(0.0, 0.0, 0.0),
                p(0.0, 0.0, 20.0),
                p(10.0, 0.0, 30.0),
            ]),
            profiles: vec![
                vec![
                    p(-1.0, -1.0, 0.0),
                    p(1.0, -1.0, 0.0),
                    p(1.0, 1.0, 0.0),
                    p(-1.0, 1.0, 0.0),
                ],
                vec![p(10.0, -1.5, 31.0), p(10.0, 1.5, 31.0), p(10.0, 0.0, 28.5)],
            ],
            up: p(0.0, 1.0, 0.0),
            options: LoftOptions {
                vertices: 12,
                start_shift: 1,
                segments: 3,
                twist: 0.25,
                edges_only: false,
            },
            path2: None,
            edges: None,
            closed: false,
            twists: Vec::new(),
        };
        let spline = SweepRecipe::Loft {
            path: None,
            profiles: vec![
                vec![p(0.0, 0.0, 0.0), p(2.0, 0.0, 0.0), p(1.0, 2.0, 0.0)],
                vec![p(0.0, 0.0, 10.0), p(3.0, 0.0, 10.0), p(1.5, 3.0, 10.0)],
                vec![p(0.0, 0.0, 20.0), p(1.0, 0.0, 20.0), p(0.5, 1.0, 20.0)],
            ],
            up: p(0.0, 0.0, 1.0),
            options: LoftOptions {
                segments: 8,
                ..LoftOptions::default()
            },
            path2: None,
            edges: None,
            closed: false,
            twists: Vec::new(),
        };
        for (recipe, x) in [(along, 120.0), (spline, 160.0)] {
            let def = d.create_definition("Loft", DefinitionKind::Group);
            let g = d.context_entities_mut(Some(def)).expect("def");
            g.add_line(p(0.0, 0.0, 0.0), p(4.0, 0.0, 0.0))?;
            d.set_definition_sweep(def, Some(recipe))?;
            d.add_instance(None, def, DMat4::from_translation(p(x, 0.0, 0.0)), "")?;
        }
        Ok(())
    });
    file
}

/// Second paths and bare edges (schema 10): an outline swept between two
/// paths, and an open edge chain lofted into another along a path.
fn edges_recipe() -> NativeFile {
    let mut file = cube();
    draw(&mut file.document, |d| {
        let two_paths = SweepRecipe::Sweep {
            path: vec![p(0.0, 0.0, 0.0), p(0.0, 0.0, 10.0)],
            closed: false,
            path2: Some(vec![p(2.0, 0.0, 0.0), p(4.0, 0.0, 10.0)]),
            section: SweepSection::Outline(vec![
                p(-1.0, -1.0, 0.0),
                p(1.0, -1.0, 0.0),
                p(1.0, 1.0, 0.0),
                p(-1.0, 1.0, 0.0),
            ]),
            up: p(0.0, 1.0, 0.0),
            taper: 1.0,
            twist: 0.0,
            upright: false,
            align: false,
        };
        let edges = SweepRecipe::Loft {
            path: Some(vec![p(0.0, 0.0, 0.0), p(0.0, 0.0, 10.0)]),
            profiles: vec![
                vec![p(0.0, 0.0, 0.0), p(0.0, 4.0, 0.0)],
                vec![p(0.0, 0.0, 10.0), p(0.0, 2.0, 10.0), p(0.0, 4.0, 10.0)],
            ],
            up: p(0.0, 1.0, 0.0),
            options: LoftOptions::default(),
            path2: None,
            edges: Some(false),
            closed: false,
            twists: Vec::new(),
        };
        for (recipe, x) in [(two_paths, 200.0), (edges, 240.0)] {
            let def = d.create_definition("Extrude", DefinitionKind::Group);
            let g = d.context_entities_mut(Some(def)).expect("def");
            g.add_line(p(0.0, 0.0, 0.0), p(4.0, 0.0, 0.0))?;
            d.set_definition_sweep(def, Some(recipe))?;
            d.add_instance(None, def, DMat4::from_translation(p(x, 0.0, 0.0)), "")?;
        }
        Ok(())
    });
    file
}

const FIXTURES: [(&str, Builder); 15] = [
    ("cube_10mm.glb", cube),
    ("square_with_hole.glb", square_with_hole),
    ("all_curves.glb", all_curves),
    ("flags_and_fin.glb", flags_and_fin),
    ("groups_view_axes.glb", groups_view_axes),
    ("layers_hidden_locked.glb", layers_hidden_locked),
    ("guides.glb", guides),
    ("annotations.glb", annotations),
    ("scenes.glb", scenes),
    ("sections.glb", sections),
    ("shape_recipe.glb", shape_recipe),
    ("sweep_recipe.glb", sweep_recipe),
    ("loft_recipe.glb", loft_recipe),
    ("edges_recipe.glb", edges_recipe),
    ("paint_look.glb", paint_look),
];

/// A 2 × 1 pixel PNG (red, blue).
const TINY_PNG: [u8; 70] = [
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 123, 64, 232, 221, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 0, 4, 255, 1, 7,
    0, 1, 255, 226, 35, 158, 89, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// Schema 11: a textured material and a metal (one image, shared), a
/// face painted on both sides (its own frame and a wrap), an unused
/// material, marked and styled edges, a styled layer, a 20° smoothing
/// angle, saved in Present mode.
fn paint_look() -> NativeFile {
    let mut doc = Document::new();
    draw(&mut doc, |d| {
        let image = std::sync::Arc::new(TextureImage {
            name: "check.png".into(),
            format: ImageFormat::Png,
            bytes: TINY_PNG.to_vec(),
            pixels: [2, 1],
        });
        let wood = d.add_material(Material {
            name: "Wood".into(),
            color: [1.0, 1.0, 1.0],
            texture: Some(Texture {
                image: image.clone(),
                width_mm: 50.0,
            }),
            ..Material::default()
        })?;
        let gold = d.add_material(Material {
            name: "Gold".into(),
            color: [1.0, 0.773, 0.307],
            metallic: true,
            opacity: 0.5,
            texture: Some(Texture {
                image,
                width_mm: 20.0,
            }),
        })?;
        d.add_material(Material {
            name: "Unused".into(),
            ..Material::default()
        })?;
        let dashed = d.add_layer("Hidden lines", [10, 10, 10]);
        let mut layer = d.layer(dashed).cloned().ok_or(ModelError::NoSuchEntity)?;
        layer.style = Some(LineStyle {
            weight: PenWeight::W050,
            pattern: LinePattern::Dashed,
        });
        d.set_layer(dashed, layer)?;
        d.set_smoothing_angle(20.0)?;
        let r = d.root_mut();
        r.add_polyline(
            &[
                p(0.0, 0.0, 0.0),
                p(50.0, 0.0, 0.0),
                p(50.0, 30.0, 0.0),
                p(0.0, 30.0, 0.0),
            ],
            true,
        )?;
        let f = r.face_ids().next().ok_or(ModelError::NoSuchEntity)?;
        let wrap = r.add_wrap(Wrap::new(
            Projection::Cylinder,
            Axis::Y,
            [DVec3::X, DVec3::Y, DVec3::Z],
            p(25.0, 15.0, 0.0),
            8.0,
        ))?;
        r.set_face_paint(
            f,
            Side::Front,
            Some(Paint {
                material: wood,
                mapping: Mapping::Face(TextureFrame::for_side(
                    DVec3::Z,
                    glam::DVec2::new(3.0, 4.0),
                    0.3,
                )),
            }),
        )?;
        r.set_face_paint(
            f,
            Side::Back,
            Some(Paint {
                material: gold,
                mapping: Mapping::Wrap(wrap),
            }),
        )?;
        let edges: Vec<_> = r.edge_ids().collect();
        r.set_edge_mark(edges[0], EdgeMark::Sharp)?;
        r.set_edge_mark(edges[1], EdgeMark::Smooth)?;
        r.set_edge_style(
            edges[2],
            Some(LineStyle {
                weight: PenWeight::W013,
                pattern: LinePattern::DashDoubleDot,
            }),
        )?;
        r.set_layer(&[Pickable::Edge(edges[3])], Some(dashed))?;
        Ok(())
    });
    NativeFile {
        document: doc,
        present: true,
        ..Default::default()
    }
}

#[test]
fn materials_share_one_image_and_unused_ones_are_not_saved() {
    let bytes = write_glb(&paint_look()).expect("save");
    let json = json_of(&bytes);
    assert_eq!(json["images"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["images"][0]["mimeType"], "image/png");
    let ext = &json["extensions"]["SKETCHSMITH_model"];
    assert_eq!(ext["schema"], 17);
    assert_eq!(ext["present"], true);
    assert_eq!(
        ext["document"]["materials"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(ext["document"]["smoothing_angle"], 20.0);
    let opened = read_glb(&bytes).expect("open").file;
    let doc = &opened.document;
    let ids: Vec<_> = doc.material_ids().collect();
    let a = doc
        .material(ids[0])
        .and_then(|m| m.texture.clone())
        .expect("tex");
    let b = doc
        .material(ids[1])
        .and_then(|m| m.texture.clone())
        .expect("tex");
    assert!(std::sync::Arc::ptr_eq(&a.image, &b.image));
    assert_eq!(a.image.bytes, TINY_PNG);
}

#[test]
fn labels_on_an_edge_and_a_face_come_back_tied_to_them() {
    let mut file = square_with_hole();
    draw(&mut file.document, |d| {
        let r = d.root_mut();
        let edge = r
            .edge_ids()
            .find(|e| {
                r.edge_points(*e)
                    .is_some_and(|(a, b)| a.y == 0.0 && b.y == 0.0)
            })
            .unwrap();
        let face = r.face_ids().next().unwrap();
        let on_edge = Anchor::on_edge(r, edge, p(20.0, 0.0, 0.0));
        let on_face = Anchor::on_face(r, face, p(5.0, 40.0, 0.0));
        for anchor in [on_edge, on_face] {
            r.add_annotation(Annotation::Leader {
                anchor,
                label: p(0.0, 0.0, 10.0),
                text: LeaderText::Custom("Part".into()),
            })?;
        }
        Ok(())
    });
    let bytes = write_glb(&file).expect("save");
    let ext = &json_of(&bytes)["extensions"]["SKETCHSMITH_model"];
    assert_eq!(ext["schema"], 17);
    let opened = read_glb(&bytes).expect("open").file;
    let anchors: Vec<Anchor> = opened
        .document
        .root()
        .annotations()
        .filter_map(|(_, a)| match a {
            Annotation::Leader { anchor, .. } => Some(*anchor),
            _ => None,
        })
        .collect();
    assert!(
        matches!(anchors[0], Anchor::Edge(_, t, _) if (t - 0.4).abs() < 1e-12 || (t - 0.6).abs() < 1e-12)
    );
    assert!(matches!(anchors[1], Anchor::Face(..)));
    let root = opened.document.root();
    assert!((anchors[0].position(root) - p(20.0, 0.0, 0.0)).length() < 1e-9);
    assert!((anchors[1].position(root) - p(5.0, 40.0, 0.0)).length() < 1e-9);
}

#[test]
fn an_edges_only_loft_keeps_its_switch_and_others_write_none() {
    let mut file = cube();
    draw(&mut file.document, |d| {
        let def = d.create_definition("Loft", DefinitionKind::Group);
        let g = d.context_entities_mut(Some(def)).expect("def");
        g.add_line(p(0.0, 0.0, 0.0), p(4.0, 0.0, 0.0))?;
        d.set_definition_sweep(
            def,
            Some(SweepRecipe::Loft {
                path: None,
                profiles: vec![
                    vec![p(0.0, 0.0, 0.0), p(2.0, 0.0, 0.0), p(1.0, 2.0, 0.0)],
                    vec![p(0.0, 0.0, 10.0), p(2.0, 0.0, 10.0), p(1.0, 2.0, 10.0)],
                ],
                up: p(0.0, 0.0, 1.0),
                options: LoftOptions {
                    edges_only: true,
                    ..LoftOptions::default()
                },
                path2: None,
                edges: None,
                closed: false,
                twists: Vec::new(),
            }),
        )?;
        d.add_instance(None, def, DMat4::IDENTITY, "")?;
        Ok(())
    });
    let bytes = write_glb(&file).expect("save");
    let opened = read_glb(&bytes).expect("open").file;
    let recipe = opened
        .document
        .definition_ids()
        .find_map(|d| opened.document.definition(d)?.sweep.clone());
    assert!(matches!(
        recipe,
        Some(SweepRecipe::Loft { options, .. }) if options.edges_only
    ));
    // A faced loft writes no switch.
    let plain = write_glb(&loft_recipe()).expect("save");
    assert!(!String::from_utf8_lossy(&plain).contains("edges_only"));
}

#[test]
fn a_loft_round_a_closed_path_comes_back_closed_and_open_ones_write_none() {
    let mut file = cube();
    draw(&mut file.document, |d| {
        let def = d.create_definition("Loft", DefinitionKind::Group);
        let g = d.context_entities_mut(Some(def)).expect("def");
        g.add_line(p(0.0, 0.0, 0.0), p(4.0, 0.0, 0.0))?;
        d.set_definition_sweep(
            def,
            Some(SweepRecipe::Loft {
                path: Some(vec![
                    p(0.0, 0.0, 0.0),
                    p(100.0, 0.0, 0.0),
                    p(100.0, 100.0, 0.0),
                    p(0.0, 100.0, 0.0),
                ]),
                profiles: vec![
                    vec![p(50.0, -5.0, -5.0), p(50.0, 5.0, -5.0), p(50.0, 5.0, 5.0)],
                    vec![
                        p(50.0, 95.0, -5.0),
                        p(50.0, 105.0, -5.0),
                        p(50.0, 105.0, 5.0),
                    ],
                ],
                up: p(0.0, 0.0, 1.0),
                options: LoftOptions::default(),
                path2: None,
                edges: None,
                closed: true,
                twists: Vec::new(),
            }),
        )?;
        d.add_instance(None, def, DMat4::IDENTITY, "")?;
        Ok(())
    });
    let bytes = write_glb(&file).expect("save");
    let opened = read_glb(&bytes).expect("open").file;
    let recipe = opened
        .document
        .definition_ids()
        .find_map(|d| opened.document.definition(d)?.sweep.clone());
    assert!(matches!(
        recipe,
        Some(SweepRecipe::Loft { closed: true, .. })
    ));
    assert!(!String::from_utf8_lossy(&bytes).contains("\"twists\""));
    let plain = write_glb(&loft_recipe()).expect("save");
    assert!(!String::from_utf8_lossy(&plain).contains("\"closed\":true"));
}

#[test]
fn profile_twists_come_back_in_profile_order() {
    let mut file = cube();
    draw(&mut file.document, |d| {
        let def = d.create_definition("Loft", DefinitionKind::Group);
        let g = d.context_entities_mut(Some(def)).expect("def");
        g.add_line(p(0.0, 0.0, 0.0), p(4.0, 0.0, 0.0))?;
        d.set_definition_sweep(
            def,
            Some(SweepRecipe::Loft {
                path: None,
                profiles: vec![
                    vec![p(0.0, 0.0, 0.0), p(2.0, 0.0, 0.0), p(1.0, 2.0, 0.0)],
                    vec![p(0.0, 0.0, 10.0), p(2.0, 0.0, 10.0), p(1.0, 2.0, 10.0)],
                    vec![p(0.0, 0.0, 20.0), p(2.0, 0.0, 20.0), p(1.0, 2.0, 20.0)],
                ],
                up: p(0.0, 0.0, 1.0),
                options: LoftOptions::default(),
                path2: None,
                edges: None,
                closed: false,
                twists: vec![0.0, 0.5, -0.25],
            }),
        )?;
        d.add_instance(None, def, DMat4::IDENTITY, "")?;
        Ok(())
    });
    let bytes = write_glb(&file).expect("save");
    let opened = read_glb(&bytes).expect("open").file;
    let recipe = opened
        .document
        .definition_ids()
        .find_map(|d| opened.document.definition(d)?.sweep.clone());
    let Some(SweepRecipe::Loft { twists, .. }) = recipe else {
        panic!("no loft: {recipe:?}");
    };
    assert_eq!(twists, vec![0.0, 0.5, -0.25]);
}

/// A Skin group: a bent square frame skinned.
fn skin_file() -> NativeFile {
    let mut file = cube();
    draw(&mut file.document, |d| {
        let def = d.create_definition("Skin", DefinitionKind::Group);
        let g = d.context_entities_mut(Some(def)).expect("def");
        g.add_line(p(0.0, 0.0, 0.0), p(4.0, 0.0, 0.0))?;
        d.set_definition_sweep(
            def,
            Some(SweepRecipe::Skin {
                segments: vec![
                    [p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0)],
                    [p(10.0, 0.0, 0.0), p(10.0, 10.0, 4.0)],
                    [p(10.0, 10.0, 4.0), p(0.0, 10.0, 0.0)],
                    [p(0.0, 10.0, 0.0), p(0.0, 0.0, 0.0)],
                ],
                sampling: 5,
                edges_only: true,
            }),
        )?;
        d.add_instance(None, def, DMat4::IDENTITY, "")?;
        Ok(())
    });
    file
}

#[test]
fn a_skin_keeps_its_frame_and_values() {
    let bytes = write_glb(&skin_file()).expect("save");
    let opened = read_glb(&bytes).expect("open").file;
    let recipe = opened
        .document
        .definition_ids()
        .find_map(|d| opened.document.definition(d)?.sweep.clone());
    let Some(SweepRecipe::Skin {
        segments,
        sampling,
        edges_only,
    }) = recipe
    else {
        panic!("no skin: {recipe:?}");
    };
    assert_eq!(segments.len(), 4);
    assert_eq!(segments[1], [p(10.0, 0.0, 0.0), p(10.0, 10.0, 4.0)]);
    assert_eq!(sampling, 5);
    assert!(edges_only);
}

#[test]
fn a_recipe_of_a_kind_not_known_is_dropped_and_its_group_kept() {
    let mut bytes = write_glb(&skin_file()).expect("save");
    // As a newer build might write: a kind this one does not know (the
    // same length, so the file's chunk sizes still hold).
    let from = b"\"kind\":\"skin\"";
    let at = bytes
        .windows(from.len())
        .position(|w| w == from)
        .expect("the skin kind");
    bytes[at..at + from.len()].copy_from_slice(b"\"kind\":\"weft\"");
    let opened = read_glb(&bytes).expect("still opens").file;
    let doc = &opened.document;
    let skin = doc
        .definition_ids()
        .find(|d| doc.definition(*d).is_some_and(|x| x.name == "Skin"))
        .expect("the group");
    assert!(doc.definition(skin).unwrap().sweep.is_none());
    assert_eq!(doc.definition_entities(skin).unwrap().edge_count(), 1);
}

#[test]
fn a_file_without_paint_writes_no_paint_fields() {
    let bytes = write_glb(&cube()).expect("save");
    let json = json_of(&bytes);
    let ext = &json["extensions"]["SKETCHSMITH_model"];
    for key in ["present", "document.materials", "document.smoothing_angle"] {
        let mut v = ext;
        for k in key.split('.') {
            v = &v[k];
        }
        assert!(v.is_null(), "{key} written");
    }
    let root = &ext["document"]["root"];
    for key in ["face_paint", "paints", "wraps", "edge_styles"] {
        assert!(root[key].is_null(), "{key} written");
    }
    assert!(json["images"].is_null());
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn assert_same(a: &NativeFile, b: &NativeFile, what: &str) {
    assert_eq!(a.document.to_data(), b.document.to_data(), "{what}: model");
    assert_eq!(a.view, b.view, "{what}: view");
    assert_eq!(a.axes, b.axes, "{what}: axes");
    assert_eq!(a.present, b.present, "{what}: mode");
}

fn json_of(bytes: &[u8]) -> Value {
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().expect("len")) as usize;
    serde_json::from_slice(&bytes[20..20 + json_len]).expect("json")
}

#[test]
fn every_fixture_round_trips_exactly() {
    for (name, build) in FIXTURES {
        let original = build();
        original.document.root().validate().expect(name);
        let bytes = write_glb(&original).expect(name);
        let opened = read_glb(&bytes).expect(name);
        assert!(!opened.newer, "{name}");
        assert!(opened.warnings.is_empty(), "{name}: {:?}", opened.warnings);
        assert_same(&original, &opened.file, name);
        opened.file.document.root().validate().expect(name);
        // A second save of what was read is byte-identical.
        assert_eq!(
            write_glb(&opened.file).expect(name),
            bytes,
            "{name}: stable"
        );
    }
}

#[test]
fn committed_fixtures_still_read_the_same() {
    let write = std::env::var_os("SKETCHSMITH_WRITE_FIXTURES").is_some();
    for (name, build) in FIXTURES {
        let path = fixture_path(name);
        let fresh = build();
        if write && !path.exists() {
            std::fs::create_dir_all(path.parent().expect("dir")).expect("mkdir");
            smith_io::save(&path, &fresh).expect("write fixture");
        }
        let opened = smith_io::open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_same(&fresh, &opened.file, name);
    }
}

#[test]
fn the_curve_kinds_and_holes_survive() {
    let opened = read_glb(&write_glb(&all_curves()).expect("w")).expect("r");
    let root = opened.file.document.root();
    assert_eq!(root.curve_count(), 6);
    let kinds: Vec<&str> = root
        .curve_ids()
        .filter_map(|id| root.curve(id))
        .map(|c| match c.curve() {
            Curve::Arc(_) => "arc",
            Curve::Circle(_) => "circle",
            Curve::Polygon(_) => "polygon",
            Curve::Bezier(_) => "bezier",
            Curve::Polyline(_) => "polyline",
            Curve::Helix(_) => "helix",
        })
        .collect();
    assert_eq!(
        kinds,
        ["arc", "circle", "polygon", "bezier", "polyline", "helix"]
    );

    let opened = read_glb(&write_glb(&square_with_hole()).expect("w")).expect("r");
    let root = opened.file.document.root();
    let holes: usize = root
        .face_ids()
        .filter_map(|f| root.face(f))
        .map(|f| f.inner().len())
        .sum();
    assert!(holes >= 1, "the circle stays a hole");
}

#[test]
fn a_10mm_cube_is_a_centimetre_in_the_viewer_layer() {
    let json = json_of(&write_glb(&cube()).expect("w"));
    let top = &json["nodes"][json["scenes"][0]["nodes"][0].as_u64().expect("top") as usize];
    let m: Vec<f64> = top["matrix"]
        .as_array()
        .expect("matrix")
        .iter()
        .map(|v| v.as_f64().expect("f"))
        .collect();
    let m = DMat4::from_cols_slice(&m);
    let mesh = &json["meshes"][0];
    let tri = mesh["primitives"]
        .as_array()
        .expect("prims")
        .iter()
        .find(|p| p["mode"].as_u64().unwrap_or(4) == 4)
        .expect("triangles");
    let acc = &json["accessors"][tri["attributes"]["POSITION"].as_u64().expect("pos") as usize];
    let v3 = |v: &Value| {
        let a: Vec<f64> = v
            .as_array()
            .expect("a")
            .iter()
            .map(|x| x.as_f64().expect("f"))
            .collect();
        DVec3::new(a[0], a[1], a[2])
    };
    let lo = m.transform_point3(v3(&acc["min"]));
    let hi = m.transform_point3(v3(&acc["max"]));
    let size = (hi - lo).abs();
    for k in 0..3 {
        assert!((size[k] - 0.01).abs() < 1e-7, "axis {k}: {}", size[k]);
    }
    // Z-up becomes Y-up: the cube's height (model Z) is the glTF Y extent, and
    // it sits on the ground (glTF y from 0 up).
    assert!(lo.y.abs() < 1e-7 && (hi.y - 0.01).abs() < 1e-7);
    assert_eq!(json["extensionsUsed"][0], "SKETCHSMITH_model");
    assert!(json.get("extensionsRequired").is_none());
}

#[test]
fn layers_hidden_faces_and_locks_round_trip() {
    let file = layers_hidden_locked();
    let opened = read_glb(&write_glb(&file).expect("write")).expect("read");
    assert_same(&file, &opened.file, "layers");
    let doc = &opened.file.document;
    let names: Vec<String> = doc
        .layer_ids()
        .filter_map(|l| doc.layer(l).map(|x| x.name.clone()))
        .collect();
    assert_eq!(names, ["Base", "Red", "Off"]);
    let hidden = doc.root().hidden_faces();
    assert_eq!(hidden.len(), 1);
    let i = doc.root().instance_ids().next().expect("instance");
    let inst = doc.root().instance(i).expect("instance");
    assert!(inst.locked);
    assert!(!doc.layer_visible(inst.layer));
}

#[test]
fn a_locked_layer_round_trips_and_only_a_lock_is_written() {
    let mut file = layers_hidden_locked();
    let doc = &mut file.document;
    let red = doc.layer_ids().nth(1).expect("Red");
    let mut layer = doc.layer(red).expect("Red").clone();
    layer.locked = true;
    doc.transaction("Lock", |d| d.set_layer(red, layer))
        .expect("lock");
    let bytes = write_glb(&file).expect("write");
    let json = json_of(&bytes);
    let layers = &json["extensions"]["SKETCHSMITH_model"]["document"]["layers"];
    assert_eq!(layers[1]["locked"], true);
    // Unlocked layers carry no field.
    assert!(layers[0].get("locked").is_none());
    assert!(layers[2].get("locked").is_none());
    let opened = read_glb(&bytes).expect("read");
    let doc = &opened.file.document;
    let locks: Vec<bool> = doc
        .layer_ids()
        .filter_map(|l| doc.layer(l).map(|x| x.locked))
        .collect();
    assert_eq!(locks, [false, true, false]);
}

#[test]
fn the_viewer_layer_leaves_out_what_is_hidden() {
    let json = json_of(&write_glb(&layers_hidden_locked()).expect("w"));
    // The root mesh: one square shown (two triangles), the hidden one left
    // out; the group on the switched-off layer has no node.
    let mesh = &json["meshes"][0];
    let tris = mesh["primitives"]
        .as_array()
        .expect("primitives")
        .iter()
        .find(|p| p["mode"].as_u64().unwrap_or(4) == 4)
        .expect("triangles");
    let idx = tris["indices"].as_u64().expect("indices") as usize;
    assert_eq!(json["accessors"][idx]["count"], 6);
    let instance_nodes = json["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .filter(|n| n.get("matrix").is_some() && n.get("mesh").is_some() && n["name"] != "Model")
        .count();
    assert_eq!(instance_nodes, 0);
}

#[test]
fn a_document_on_base_only_writes_no_layer_fields() {
    let bytes = write_glb(&cube()).expect("w");
    let json = json_of(&bytes);
    let model = &json["extensions"]["SKETCHSMITH_model"];
    assert_eq!(model["schema"], smith_io::native::SCHEMA);
    assert!(model["document"].get("layers").is_none());
    assert!(model["document"]["root"].get("edge_layers").is_none());
    assert!(model["document"]["root"].get("face_flags").is_none());
}

#[test]
fn a_newer_schema_is_flagged() {
    let bytes = write_glb(&cube()).expect("w");
    let mut json = json_of(&bytes);
    json["extensions"]["SKETCHSMITH_model"]["schema"] = (smith_io::native::SCHEMA + 1).into();
    json["extensions"]["SKETCHSMITH_model"]["future_field"] = "ignored".into();
    let bin_at = 20 + u32::from_le_bytes(bytes[12..16].try_into().expect("len")) as usize;
    let bin_len = u32::from_le_bytes(bytes[bin_at..bin_at + 4].try_into().expect("len")) as usize;
    let bin = &bytes[bin_at + 8..bin_at + 8 + bin_len];
    let rebuilt = glb_pack(&serde_json::to_vec(&json).expect("json"), bin);
    let opened = read_glb(&rebuilt).expect("still opens");
    assert!(opened.newer);
    assert!(opened.warnings.is_empty());
    assert_same(&cube(), &opened.file, "newer schema");
}

#[test]
fn foreign_and_broken_files_are_refused() {
    let plain = glb_pack(br#"{"asset":{"version":"2.0"}}"#, &[]);
    assert!(matches!(read_glb(&plain), Err(IoError::Foreign)));
    assert!(matches!(read_glb(b"solid cube\n"), Err(IoError::NotGlb)));
    let mut bytes = write_glb(&cube()).expect("w");
    let n = bytes.len();
    bytes.truncate(n - 40);
    assert!(read_glb(&bytes).is_err());
}

#[test]
fn saving_is_atomic_and_replaces_the_old_file() {
    let dir = std::env::temp_dir().join(format!("smith-io-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("model.glb");
    smith_io::save(&path, &cube()).expect("first save");
    smith_io::save(&path, &all_curves()).expect("second save");
    let opened = smith_io::open(&path).expect("open");
    assert_same(&all_curves(), &opened.file, "replaced");
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .expect("ls")
        .filter_map(Result::ok)
        .filter(|e| e.file_name() != "model.glb")
        .collect();
    assert!(leftovers.is_empty(), "temporary file left behind");
    // A folder that does not exist fails cleanly.
    assert!(smith_io::save(&dir.join("missing/model.glb"), &cube()).is_err());
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Packs a GLB by hand (test helper, independent of the crate's writer).
fn glb_pack(json: &[u8], bin: &[u8]) -> Vec<u8> {
    let pad = |n: usize| n.div_ceil(4) * 4;
    let total = 12
        + 8
        + pad(json.len())
        + if bin.is_empty() {
            0
        } else {
            8 + pad(bin.len())
        };
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(pad(json.len()) as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(json);
    out.resize(20 + pad(json.len()), b' ');
    if !bin.is_empty() {
        out.extend_from_slice(&(pad(bin.len()) as u32).to_le_bytes());
        out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        out.extend_from_slice(bin);
        out.resize(total, 0);
    }
    out
}

#[test]
fn guides_round_trip_and_a_bad_guide_refuses_the_file() {
    let file = guides();
    let bytes = write_glb(&file).expect("w");
    let opened = read_glb(&bytes).expect("r");
    assert_same(&file, &opened.file, "guides");
    assert_eq!(opened.file.document.root().guide_count(), 3);

    // A guide line whose direction is not a unit vector is damage.
    let mut json = json_of(&bytes);
    json["extensions"]["SKETCHSMITH_model"]["document"]["root"]["guides"][0]["direction"] =
        serde_json::json!([2.0, 0.0, 0.0]);
    let bin_at = 20 + u32::from_le_bytes(bytes[12..16].try_into().expect("len")) as usize;
    let bin_len = u32::from_le_bytes(bytes[bin_at..bin_at + 4].try_into().expect("len")) as usize;
    let bin = &bytes[bin_at + 8..bin_at + 8 + bin_len];
    let broken = glb_pack(&serde_json::to_vec(&json).expect("json"), bin);
    assert!(matches!(read_glb(&broken), Err(IoError::Damaged(_))));

    // A guide of an unknown kind is skipped with a warning.
    let mut json = json_of(&bytes);
    json["extensions"]["SKETCHSMITH_model"]["document"]["root"]["guides"][1] =
        serde_json::json!({"kind": "plane", "point": [0.0, 0.0, 0.0]});
    let skipped = glb_pack(&serde_json::to_vec(&json).expect("json"), bin);
    let opened = read_glb(&skipped).expect("unknown kind");
    assert_eq!(opened.file.document.root().guide_count(), 2);
    assert_eq!(opened.warnings.len(), 1);
}

#[test]
fn annotations_round_trip_and_bad_references_refuse_the_file() {
    let file = annotations();
    let bytes = write_glb(&file).expect("w");
    let opened = read_glb(&bytes).expect("r");
    assert_same(&file, &opened.file, "annotations");
    assert_eq!(opened.file.document.root().annotation_count(), 9);

    let bin_at = 20 + u32::from_le_bytes(bytes[12..16].try_into().expect("len")) as usize;
    let bin_len = u32::from_le_bytes(bytes[bin_at..bin_at + 4].try_into().expect("len")) as usize;
    let bin = &bytes[bin_at + 8..bin_at + 8 + bin_len];

    // A vertex index past the end is damage.
    let mut json = json_of(&bytes);
    json["extensions"]["SKETCHSMITH_model"]["document"]["root"]["annotations"][0]["a"]["vertex"] =
        9999.into();
    let broken = glb_pack(&serde_json::to_vec(&json).expect("json"), bin);
    assert!(matches!(read_glb(&broken), Err(IoError::Damaged(_))));

    // An unknown kind is skipped with a warning.
    let mut json = json_of(&bytes);
    json["extensions"]["SKETCHSMITH_model"]["document"]["root"]["annotations"][8] =
        serde_json::json!({"kind": "balloon", "at": [0.0, 0.0]});
    let skipped = glb_pack(&serde_json::to_vec(&json).expect("json"), bin);
    let opened = read_glb(&skipped).expect("unknown kind");
    assert_eq!(opened.file.document.root().annotation_count(), 8);
    assert_eq!(opened.warnings.len(), 1);
}

#[test]
fn scenes_round_trip_and_a_bad_layer_index_refuses_the_file() {
    let file = scenes();
    let bytes = write_glb(&file).expect("w");
    let opened = read_glb(&bytes).expect("r");
    assert_same(&file, &opened.file, "scenes");
    assert_eq!(opened.file.document.scenes().count(), 2);

    let bin_at = 20 + u32::from_le_bytes(bytes[12..16].try_into().expect("len")) as usize;
    let bin_len = u32::from_le_bytes(bytes[bin_at..bin_at + 4].try_into().expect("len")) as usize;
    let bin = &bytes[bin_at + 8..bin_at + 8 + bin_len];
    let mut json = json_of(&bytes);
    json["extensions"]["SKETCHSMITH_model"]["document"]["scenes"][0]["hidden_layers"] =
        serde_json::json!([42]);
    let broken = glb_pack(&serde_json::to_vec(&json).expect("json"), bin);
    assert!(matches!(read_glb(&broken), Err(IoError::Damaged(_))));
}

#[test]
fn sections_round_trip_and_two_active_planes_refuse_the_file() {
    let file = sections();
    let bytes = write_glb(&file).expect("w");
    let opened = read_glb(&bytes).expect("r");
    assert_same(&file, &opened.file, "sections");

    let bin_at = 20 + u32::from_le_bytes(bytes[12..16].try_into().expect("len")) as usize;
    let bin_len = u32::from_le_bytes(bytes[bin_at..bin_at + 4].try_into().expect("len")) as usize;
    let bin = &bytes[bin_at + 8..bin_at + 8 + bin_len];
    let mut json = json_of(&bytes);
    json["extensions"]["SKETCHSMITH_model"]["document"]["root"]["sections"][1]["active"] =
        true.into();
    let broken = glb_pack(&serde_json::to_vec(&json).expect("json"), bin);
    assert!(matches!(read_glb(&broken), Err(IoError::Damaged(_))));
}

/// Reads the `TEXCOORD_0` values of the first primitive using glTF material
/// `material` in the root mesh.
fn uvs_of(bytes: &[u8], material: u64) -> Vec<[f32; 2]> {
    let json = json_of(bytes);
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().expect("len")) as usize;
    let bin = &bytes[20 + json_len + 8..];
    let prim = json["meshes"][0]["primitives"]
        .as_array()
        .expect("primitives")
        .iter()
        .find(|p| p["material"] == material)
        .expect("primitive")
        .clone();
    let acc = &json["accessors"][prim["attributes"]["TEXCOORD_0"].as_u64().expect("uv") as usize];
    let view = &json["bufferViews"][acc["bufferView"].as_u64().expect("view") as usize];
    let off = view["byteOffset"].as_u64().unwrap_or(0) as usize;
    let n = acc["count"].as_u64().expect("count") as usize;
    bin[off..off + n * 8]
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| {
            [
                f32::from_le_bytes(c[0..4].try_into().expect("u")),
                f32::from_le_bytes(c[4..8].try_into().expect("v")),
            ]
        })
        .collect()
}

#[test]
fn a_50mm_texture_repeats_once_on_a_50mm_face_in_the_viewer_layer() {
    let mut file = paint_look();
    // Paint the front with the plain frame (no offset or turn).
    let doc = &mut file.document;
    let wood = doc.material_ids().next().expect("wood");
    let r = doc.root_mut();
    let f = r.face_ids().next().expect("face");
    r.set_face_paint(
        f,
        Side::Front,
        Some(Paint {
            material: wood,
            mapping: Mapping::Face(TextureFrame::for_side(DVec3::Z, glam::DVec2::ZERO, 0.0)),
        }),
    )
    .expect("paint");
    let bytes = write_glb(&file).expect("save");
    let json = json_of(&bytes);
    // Plain face, edge, then the painted sides as they are first met.
    let wood_index = json["materials"]
        .as_array()
        .expect("materials")
        .iter()
        .position(|m| m["name"] == "Wood")
        .expect("wood material") as u64;
    let m = &json["materials"][wood_index as usize];
    assert_eq!(m["doubleSided"], false);
    assert_eq!(m["pbrMetallicRoughness"]["baseColorTexture"]["index"], 0);
    let us: Vec<f32> = uvs_of(&bytes, wood_index).iter().map(|t| t[0]).collect();
    let span =
        us.iter().copied().fold(f32::MIN, f32::max) - us.iter().copied().fold(f32::MAX, f32::min);
    assert!((span - 1.0).abs() < 1e-6, "{span}");
    let gold = &json["materials"]
        .as_array()
        .expect("materials")
        .iter()
        .find(|m| m["name"] == "Gold")
        .expect("gold")
        .clone();
    assert_eq!(gold["alphaMode"], "BLEND");
    assert_eq!(gold["pbrMetallicRoughness"]["metallicFactor"], 1.0);
    assert_eq!(json["samplers"][0]["wrapS"], 10497);
}

#[test]
fn smoothing_never_changes_the_geometry_saved() {
    // The geometry of a file as saved: points, edges, faces and their loops.
    let geometry = |file: &NativeFile| {
        let data = read_glb(&write_glb(file).expect("save"))
            .expect("open")
            .file
            .document
            .to_data()
            .root;
        let edges: Vec<_> = data
            .edges
            .iter()
            .map(|e| (e.vertices, e.faces.clone()))
            .collect();
        let faces: Vec<_> = data
            .faces
            .iter()
            .map(|f| (f.outer.clone(), f.inner.clone(), f.normal))
            .collect();
        (data.positions, data.vertex_edges, edges, faces)
    };
    let mut file = cube();
    let before = geometry(&file);
    let r = file.document.root_mut();
    let edges: Vec<_> = r.edge_ids().collect();
    for (i, e) in edges.iter().enumerate() {
        let mark = [EdgeMark::Sharp, EdgeMark::Smooth, EdgeMark::Auto][i % 3];
        r.set_edge_mark(*e, mark).expect("mark");
    }
    file.document.set_smoothing_angle(89.0).expect("angle");
    assert_eq!(geometry(&file), before);
}
