//! SketchSmith — Dimensions and Text tools | v1.0.0 | Thursday 12 November 2026
//!
//! * **Dimensions** — click an edge for its length, or click two points;
//!   then move the cursor out and click where the dimension line goes.
//!   Click a circle or arc for its radius (Ctrl: diameter), then click where
//!   the label goes. With **Angle** on (Operation Inputs), click the vertex
//!   of the angle, a point on each arm, then where the arc goes. Points on
//!   vertices tie the dimension to them, so it follows when they move.
//! * **Text** — click a point, an edge or a face, then where the text goes:
//!   leader text showing the edge's length, the face's area or the point's
//!   coordinates. Type words and press Enter, before or after placing it, to
//!   show them instead. Click empty canvas, type and press Enter for text
//!   fixed to that place on the canvas.

use smith_geom::DVec3;
use smith_infer::infer::{Inference, Kind};
use smith_model::{Anchor, Annotation, LeaderText, Pickable};
use smith_ops::draw::DrawOp;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, in_plane};

// ----- Dimensions ---------------------------------------------------------------

/// The radius and centre of the circle or arc edge `p` belongs to.
fn circle_of(
    p: Option<Pickable>,
    ctx: &Ctx<'_>,
) -> Option<(smith_model::CurveId, DVec3, f64, DVec3)> {
    use smith_geom::curve::Curve;
    let Some(Pickable::Edge(e)) = p else {
        return None;
    };
    let id = ctx.entities.edge(e)?.curve()?;
    match ctx.entities.curve(id)?.curve() {
        Curve::Circle(c) => Some((id, c.frame().center(), c.radius(), c.frame().normal())),
        Curve::Arc(a) => Some((id, a.frame().center(), a.radius(), a.frame().normal())),
        _ => None,
    }
}

/// What the Dimensions tool is placing.
#[derive(Debug, Clone, PartialEq)]
enum Placing {
    /// The first point is down.
    First(DVec3),
    /// Both points are down; the dimension line follows the cursor.
    Linear(DVec3, DVec3),
    /// A circle or arc: the label follows the cursor.
    Radial {
        curve: smith_model::CurveId,
        centre: DVec3,
        radius: f64,
        normal: DVec3,
    },
    /// The angle's vertex is down.
    AngleCentre(DVec3),
    /// The vertex and the first arm are down.
    AngleArm(DVec3, DVec3),
    /// All three points are down; the arc follows the cursor.
    Angle(DVec3, DVec3, DVec3),
}

/// The Dimensions tool.
#[derive(Debug, Default)]
pub struct DimensionTool {
    placing: Option<Placing>,
    hover: Option<DVec3>,
    /// Angular dimensions.
    angle: bool,
    /// Radial dimensions show the diameter.
    diameter: bool,
}

impl DimensionTool {
    /// The offset of the dimension line from `a`–`b` toward `p`: square to
    /// the measured line.
    fn offset(a: DVec3, b: DVec3, p: DVec3) -> DVec3 {
        match (b - a).try_normalize() {
            Some(d) => in_plane(p - a, d),
            None => p - a,
        }
    }

    fn finish(&mut self, annotation: Annotation) -> Outcome {
        self.placing = None;
        if annotation.is_valid() {
            Outcome::Commit(DrawOp::Annotate { annotation })
        } else {
            Outcome::Message("Pick Two Different Points".into())
        }
    }
}

impl Tool for DimensionTool {
    fn name(&self) -> &'static str {
        "Dimensions"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        let e = ctx.entities;
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Click(inf) => match self.placing.clone() {
                None if self.angle => {
                    self.placing = Some(Placing::AngleCentre(inf.point));
                    Outcome::Nothing
                }
                None => {
                    if let Some((curve, centre, radius, normal)) = circle_of(inf.entity, ctx) {
                        self.placing = Some(Placing::Radial {
                            curve,
                            centre,
                            radius,
                            normal,
                        });
                    } else if let (Kind::OnEdge, Some(Pickable::Edge(id))) = (inf.kind, inf.entity)
                        && let Some((a, b)) = e.edge_points(id)
                    {
                        self.placing = Some(Placing::Linear(a, b));
                    } else {
                        self.placing = Some(Placing::First(inf.point));
                    }
                    Outcome::Nothing
                }
                Some(Placing::First(a)) => {
                    if a.distance(inf.point) > 1e-9 {
                        self.placing = Some(Placing::Linear(a, inf.point));
                    }
                    Outcome::Nothing
                }
                Some(Placing::Linear(a, b)) => self.finish(Annotation::Linear {
                    a: Anchor::at(e, a),
                    b: Anchor::at(e, b),
                    offset: Self::offset(a, b, inf.point),
                }),
                Some(Placing::Radial {
                    curve,
                    centre,
                    radius,
                    ..
                }) => self.finish(Annotation::Radial {
                    curve: Some(curve),
                    diameter: self.diameter,
                    centre,
                    radius,
                    label: inf.point - centre,
                }),
                Some(Placing::AngleCentre(c)) => {
                    if c.distance(inf.point) > 1e-9 {
                        self.placing = Some(Placing::AngleArm(c, inf.point));
                    }
                    Outcome::Nothing
                }
                Some(Placing::AngleArm(c, a)) => {
                    if c.distance(inf.point) > 1e-9 {
                        self.placing = Some(Placing::Angle(c, a, inf.point));
                    }
                    Outcome::Nothing
                }
                Some(Placing::Angle(c, a, b)) => self.finish(Annotation::Angular {
                    centre: Anchor::at(e, c),
                    a: Anchor::at(e, a),
                    b: Anchor::at(e, b),
                    radius: c.distance(inf.point),
                }),
            },
            Input::Toggle => {
                self.diameter = !self.diameter;
                Outcome::Nothing
            }
            Input::Typed(_) | Input::Finish => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        self.placing.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        match self.placing {
            Some(Placing::First(a) | Placing::AngleCentre(a) | Placing::AngleArm(a, _)) => Some(a),
            _ => None,
        }
    }

    fn pauses(&self) -> bool {
        false
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        let Some(h) = self.hover else {
            return Preview::default();
        };
        let lines = match self.placing {
            None => Vec::new(),
            Some(Placing::First(a)) => vec![[a, h]],
            Some(Placing::Linear(a, b)) => {
                let o = Self::offset(a, b, h);
                vec![[a, a + o], [b, b + o], [a + o, b + o]]
            }
            Some(Placing::Radial {
                centre,
                radius,
                normal,
                ..
            }) => {
                let d = in_plane(h - centre, normal).normalize_or_zero();
                if self.diameter {
                    vec![
                        [centre - d * radius, centre + d * radius],
                        [centre + d * radius, h],
                    ]
                } else {
                    vec![[centre, centre + d * radius], [centre + d * radius, h]]
                }
            }
            Some(Placing::AngleCentre(c)) => vec![[c, h]],
            Some(Placing::AngleArm(c, a)) => vec![[c, a], [c, h]],
            Some(Placing::Angle(c, a, b)) => vec![[c, a], [c, b], [c, h]],
        };
        Preview {
            lines,
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        match self.placing {
            None if self.angle => "Click the Vertex of the Angle".into(),
            None => "Click an Edge, a Circle or Arc, or a First Point".into(),
            Some(Placing::First(_)) => "Click the Second Point".into(),
            Some(Placing::Linear(..)) => "Click Where the Dimension Line Goes".into(),
            Some(Placing::Radial { .. }) => {
                if self.diameter {
                    "Click Where the Label Goes · Ctrl: Radius".into()
                } else {
                    "Click Where the Label Goes · Ctrl: Diameter".into()
                }
            }
            Some(Placing::AngleCentre(_)) => "Click a Point on the First Arm".into(),
            Some(Placing::AngleArm(..)) => "Click a Point on the Second Arm".into(),
            Some(Placing::Angle(..)) => "Click Where the Arc Goes".into(),
        }
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::switch("Angle", self.angle, self.placing.is_none()),
            Field::switch("Diameter", self.diameter, true),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 if self.placing.is_none() => self.angle = value != 0.0,
            1 => self.diameter = value != 0.0,
            _ => {}
        }
        Outcome::Nothing
    }

    fn accept(&mut self) {}

    fn reopen(&mut self) -> bool {
        false
    }

    fn forget(&mut self) {}
}

// ----- Text ---------------------------------------------------------------------

/// What the Text tool has pinned down.
#[derive(Debug, Clone, PartialEq)]
enum Pinned {
    /// Leader text: what it points at (the vertex, edge or face clicked,
    /// else the point), and what it shows.
    Leader { anchor: Anchor, text: LeaderText },
    /// Text fixed to the canvas at this place.
    Screen([f64; 2]),
}

/// The last text made, kept so typed words can replace its text.
#[derive(Debug, Clone, PartialEq)]
struct Placed {
    annotation: Annotation,
    shown: bool,
}

/// The Text tool.
#[derive(Debug, Default)]
pub struct TextTool {
    pinned: Option<Pinned>,
    /// Words typed before the text was placed.
    typed: Option<String>,
    hover: Option<DVec3>,
    placed: Option<Placed>,
}

impl TextTool {
    /// What leader text at `inf` shows by itself.
    fn auto_text(inf: &Inference, ctx: &Ctx<'_>) -> LeaderText {
        match inf.entity {
            Some(Pickable::Edge(e)) if inf.kind == Kind::OnEdge => {
                let value = ctx
                    .entities
                    .edge_points(e)
                    .map_or(0.0, |(a, b)| a.distance(b));
                LeaderText::Length {
                    edge: Some(e),
                    value,
                }
            }
            Some(Pickable::Face(f)) if inf.kind == Kind::OnFace => LeaderText::Area {
                face: Some(f),
                value: ctx.entities.face_area(f).unwrap_or(0.0),
            },
            _ => LeaderText::Coordinates,
        }
    }

    /// What leader text at `inf` points at: the vertex there, else the edge
    /// or face clicked (so it follows them), else the point.
    fn anchor(inf: &Inference, ctx: &Ctx<'_>) -> Anchor {
        match inf.entity {
            Some(Pickable::Edge(e)) => Anchor::on_edge(ctx.entities, e, inf.point),
            Some(Pickable::Face(f)) => Anchor::on_face(ctx.entities, f, inf.point),
            _ => Anchor::at(ctx.entities, inf.point),
        }
    }

    fn commit(&mut self, annotation: Annotation) -> Outcome {
        self.pinned = None;
        self.typed = None;
        if !annotation.is_valid() {
            return Outcome::Nothing;
        }
        self.placed = Some(Placed {
            annotation: annotation.clone(),
            shown: true,
        });
        Outcome::Commit(DrawOp::Annotate { annotation })
    }
}

impl Tool for TextTool {
    fn name(&self) -> &'static str {
        "Text"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Click(inf) => match self.pinned.clone() {
                None => {
                    if let Some(p) = &mut self.placed {
                        p.shown = false;
                    }
                    self.pinned = Some(if inf.entity.is_none() && inf.kind == Kind::Free {
                        Pinned::Screen([ctx.cursor.x, ctx.cursor.y])
                    } else {
                        Pinned::Leader {
                            anchor: Self::anchor(&inf, ctx),
                            text: Self::auto_text(&inf, ctx),
                        }
                    });
                    Outcome::Nothing
                }
                Some(Pinned::Leader { anchor, text }) => {
                    let text = self.typed.clone().map_or(text, LeaderText::Custom);
                    self.commit(Annotation::Leader {
                        anchor,
                        label: inf.point - anchor.fallback(),
                        text,
                    })
                }
                Some(Pinned::Screen(_)) => Outcome::Message("Type the Text, then Enter".into()),
            },
            Input::Typed(_) | Input::Toggle | Input::Finish => Outcome::Nothing,
        }
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text(&mut self, text: &str, _ctx: &Ctx<'_>) -> Outcome {
        let words = text.trim().to_string();
        if words.is_empty() {
            return Outcome::Nothing;
        }
        match self.pinned.clone() {
            Some(Pinned::Screen(at)) => self.commit(Annotation::Screen { at, text: words }),
            Some(Pinned::Leader { .. }) => {
                self.typed = Some(words);
                Outcome::Nothing
            }
            None => match self.placed.clone() {
                Some(Placed {
                    annotation,
                    shown: true,
                }) => {
                    let annotation = match annotation {
                        Annotation::Leader { anchor, label, .. } => Annotation::Leader {
                            anchor,
                            label,
                            text: LeaderText::Custom(words),
                        },
                        Annotation::Screen { at, .. } => Annotation::Screen { at, text: words },
                        other => other,
                    };
                    self.placed = Some(Placed {
                        annotation: annotation.clone(),
                        shown: true,
                    });
                    Outcome::Redefine(DrawOp::Annotate { annotation })
                }
                _ => Outcome::Message("Click Where the Text Points".into()),
            },
        }
    }

    fn cancel(&mut self) -> bool {
        self.typed = None;
        self.pinned.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn pauses(&self) -> bool {
        false
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        match (&self.pinned, self.hover) {
            (Some(Pinned::Leader { anchor, .. }), Some(h)) => Preview {
                lines: vec![[anchor.fallback(), h]],
                band: None,
                note: None,
            },
            _ => Preview::default(),
        }
    }

    fn status(&self) -> String {
        match self.pinned {
            None => "Click a Point, Edge or Face · or Empty Canvas for Screen Text".into(),
            Some(Pinned::Leader { .. }) => {
                "Click Where the Text Goes · Type Words and Enter to Replace It".into()
            }
            Some(Pinned::Screen(_)) => "Type the Text, then Enter".into(),
        }
    }

    fn fields(&self) -> Vec<Field> {
        Vec::new()
    }

    fn edit(&mut self, _index: usize, _value: f64, _ctx: &Ctx<'_>) -> Outcome {
        Outcome::Nothing
    }

    fn accept(&mut self) {
        if let Some(p) = &mut self.placed {
            p.shown = false;
        }
    }

    fn reopen(&mut self) -> bool {
        match &mut self.placed {
            Some(p) => {
                p.shown = true;
                true
            }
            None => false,
        }
    }

    fn forget(&mut self) {
        self.placed = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{at, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    fn note(out: &Outcome) -> Annotation {
        match out {
            Outcome::Commit(DrawOp::Annotate { annotation })
            | Outcome::Redefine(DrawOp::Annotate { annotation }) => annotation.clone(),
            other => panic!("expected an annotation, got {other:?}"),
        }
    }

    #[test]
    fn an_edge_dimension_ties_to_its_vertices_and_offsets_square() {
        let mut m = Entities::new();
        let e = m.add_line(v(0.0, 0.0, 0.0), v(30.0, 0.0, 0.0)).unwrap()[0];
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = DimensionTool::default();
        let mut inf = at(v(12.0, 0.0, 0.0));
        inf.kind = Kind::OnEdge;
        inf.entity = Some(Pickable::Edge(e));
        t.handle(Input::Click(inf), &c);
        let out = t.handle(Input::Click(at(v(20.0, -6.0, 0.0))), &c);
        let Annotation::Linear { a, b, offset } = note(&out) else {
            panic!("linear");
        };
        assert!(matches!(a, Anchor::Vertex(..)) && matches!(b, Anchor::Vertex(..)));
        assert_eq!(offset, v(0.0, -6.0, 0.0));
    }

    #[test]
    fn a_circle_gives_a_radius_and_ctrl_a_diameter() {
        use smith_geom::curve::{Circle, Curve, CurveFrame};
        let mut m = Entities::new();
        let frame = CurveFrame::new(v(0.0, 0.0, 0.0), DVec3::X, DVec3::Z).unwrap();
        m.add_curve(Curve::Circle(Circle::new(frame, 4.0).unwrap()), 24)
            .unwrap();
        let e = m.edge_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = DimensionTool::default();
        let mut inf = at(v(4.0, 0.0, 0.0));
        inf.entity = Some(Pickable::Edge(e));
        t.handle(Input::Click(inf), &c);
        t.handle(Input::Toggle, &c);
        let out = t.handle(Input::Click(at(v(8.0, 8.0, 0.0))), &c);
        let Annotation::Radial {
            diameter, radius, ..
        } = note(&out)
        else {
            panic!("radial");
        };
        assert!(diameter);
        assert_eq!(radius, 4.0);
    }

    #[test]
    fn three_clicks_and_a_fourth_make_an_angle() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = DimensionTool {
            angle: true,
            ..DimensionTool::default()
        };
        for p in [v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(0.0, 10.0, 0.0)] {
            assert_eq!(t.handle(Input::Click(at(p)), &c), Outcome::Nothing);
        }
        let out = t.handle(Input::Click(at(v(5.0, 5.0, 0.0))), &c);
        let Annotation::Angular { radius, .. } = note(&out) else {
            panic!("angular");
        };
        assert!((radius - 50.0f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn leader_text_on_an_edge_shows_its_length_and_typing_replaces_it() {
        let mut m = Entities::new();
        let e = m.add_line(v(0.0, 0.0, 0.0), v(25.0, 0.0, 0.0)).unwrap()[0];
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = TextTool::default();
        let mut inf = at(v(5.0, 0.0, 0.0));
        inf.kind = Kind::OnEdge;
        inf.entity = Some(Pickable::Edge(e));
        t.handle(Input::Click(inf), &c);
        let out = t.handle(Input::Click(at(v(10.0, 10.0, 0.0))), &c);
        let Annotation::Leader {
            text,
            label,
            anchor,
        } = note(&out)
        else {
            panic!("leader");
        };
        assert_eq!(label, v(5.0, 10.0, 0.0));
        // Tied to the edge, a fifth of the way along.
        assert!(matches!(anchor, Anchor::Edge(k, t, _) if k == e && (t - 0.2).abs() < 1e-12));
        assert_eq!(
            text,
            LeaderText::Length {
                edge: Some(e),
                value: 25.0
            }
        );
        let out = t.text("Rail", &c);
        assert!(matches!(out, Outcome::Redefine(_)));
        let Annotation::Leader { text, .. } = note(&out) else {
            panic!("leader");
        };
        assert_eq!(text, LeaderText::Custom("Rail".into()));
    }

    #[test]
    fn leader_text_on_a_face_is_tied_to_the_face() {
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
        let f = m.face_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = TextTool::default();
        let mut inf = at(v(4.0, 6.0, 0.0));
        inf.kind = Kind::OnFace;
        inf.entity = Some(Pickable::Face(f));
        t.handle(Input::Click(inf), &c);
        let out = t.handle(Input::Click(at(v(4.0, 6.0, 8.0))), &c);
        let Annotation::Leader { anchor, label, .. } = note(&out) else {
            panic!("leader");
        };
        assert!(matches!(anchor, Anchor::Face(k, ..) if k == f));
        assert!((anchor.position(&m) - v(4.0, 6.0, 0.0)).length() < 1e-9);
        assert_eq!(label, v(0.0, 0.0, 8.0));
    }

    #[test]
    fn screen_text_takes_the_cursor_place_and_typed_words() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let mut c = ctx(&m, &axes);
        c.cursor = smith_geom::DVec2::new(0.25, 0.75);
        let mut t = TextTool::default();
        let mut inf = at(v(0.0, 0.0, 0.0));
        inf.kind = Kind::Free;
        inf.entity = None;
        t.handle(Input::Click(inf), &c);
        let out = t.text("Back", &c);
        assert_eq!(
            note(&out),
            Annotation::Screen {
                at: [0.25, 0.75],
                text: "Back".into()
            }
        );
    }
}
