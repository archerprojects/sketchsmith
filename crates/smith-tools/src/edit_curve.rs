//! SketchSmith — Edit Curve | v1.0.0 | Thursday 12 November 2026
//!
//! Right-click → Edit Curve loads a curve's values into the
//! Operation Inputs, however long ago it was drawn. Changing a value rebuilds the
//! curve in place (`DrawOp::EditCurve`); all changes made in one Edit Curve
//! session are a single undo step.
//!
//! | Curve | Fields |
//! |---|---|
//! | Circle, Arc | Radius (about the same centre), Segments |
//! | Polygon | Radius, Sides |
//! | Bézier | Segments per span |
//! | Curve Maker helix | Start radius, End radius, Pitch, Turns, Segments, Left-handed |
//!
//! A curve whose edges bound faces outside its plane (after Extrude, for
//! example) is locked: the fields are shown disabled, with a note. Welded
//! polylines have fixed edges and nothing to edit.

use smith_geom::DVec3;
use smith_geom::curve::{Arc, Circle, Curve, Helix, Polygon};
use smith_infer::units::TypedValue;
use smith_model::{CurveId, Entities};
use smith_ops::draw::DrawOp;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count};

/// Note shown on a locked curve's fields.
const LOCKED: &str = "Locked: the curve bounds faces outside its plane.";

/// The Edit Curve tool for one curve.
#[derive(Debug)]
pub struct CurveEditTool {
    id: CurveId,
    curve: Curve,
    segments: u32,
    locked: bool,
    /// A change has been committed in this session (later ones redefine it).
    edited: bool,
}

impl CurveEditTool {
    /// Starts editing curve `id`. `None` if it does not exist or has nothing
    /// to edit (a welded polyline).
    pub fn new(id: CurveId, entities: &Entities) -> Option<Self> {
        let rec = entities.curve(id)?;
        if matches!(rec.curve(), Curve::Polyline(_)) {
            return None;
        }
        Some(Self {
            id,
            curve: rec.curve().clone(),
            segments: rec.segments(),
            locked: entities.curve_is_locked(id),
            edited: false,
        })
    }

    /// The curve with field `index` set to `value`, and its segment count.
    fn changed(&self, index: usize, value: f64) -> Result<(Curve, u32), String> {
        let n = as_count(value);
        let geom = |e: smith_geom::GeomError| e.to_string();
        Ok(match &self.curve {
            Curve::Circle(c) => match index {
                0 => (
                    Curve::Circle(Circle::new(*c.frame(), value).map_err(geom)?),
                    self.segments,
                ),
                _ => (self.curve.clone(), n),
            },
            Curve::Arc(a) => match index {
                0 => {
                    let arc = Arc::new(*a.frame(), value, a.start_angle(), a.end_angle());
                    (Curve::Arc(arc.map_err(geom)?), self.segments)
                }
                _ => (self.curve.clone(), n),
            },
            Curve::Polygon(p) => {
                let (r, sides) = if index == 0 {
                    (value, p.sides())
                } else {
                    (p.radius(), n)
                };
                let poly = Polygon::new(*p.frame(), r, sides).map_err(geom)?;
                (Curve::Polygon(poly), sides)
            }
            Curve::Bezier(_) => (self.curve.clone(), n),
            Curve::Helix(h) => {
                let mut v = [h.start_radius(), h.end_radius(), h.pitch(), h.turns()];
                let mut left = h.left_handed();
                let mut segments = self.segments;
                match index {
                    0..=3 => v[index] = value,
                    4 => segments = n,
                    _ => left = value != 0.0,
                }
                let helix = Helix::new(*h.frame(), v[0], v[1], v[2], v[3], left).map_err(geom)?;
                (Curve::Helix(helix), segments)
            }
            Curve::Polyline(_) => return Err("A welded polyline has nothing to edit.".into()),
        })
    }
}

impl Tool for CurveEditTool {
    fn name(&self) -> &'static str {
        "Edit Curve"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        let segment_field = match self.curve {
            Curve::Bezier(_) => 0,
            Curve::Helix(_) => 4,
            _ => 1,
        };
        match input {
            Input::Typed(TypedValue::Length(v) | TypedValue::Radius(v)) => self.edit(0, v, ctx),
            Input::Typed(TypedValue::Segments(n)) => self.edit(segment_field, f64::from(n), ctx),
            Input::Typed(_) => Outcome::Message("Adjust Values in Operation Inputs".into()),
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        false
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        Preview::default()
    }

    fn status(&self) -> String {
        if self.locked {
            LOCKED.into()
        } else {
            "Adjust Values in Operation Inputs · Enter accepts".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        let e = !self.locked;
        let s = self.segments;
        let mut fields = match &self.curve {
            Curve::Circle(c) => {
                vec![
                    Field::length("Radius", Some(c.radius()), false, e),
                    Field::count("Segments", s, e),
                ]
            }
            Curve::Arc(a) => {
                vec![
                    Field::length("Radius", Some(a.radius()), false, e),
                    Field::count("Segments", s, e),
                ]
            }
            Curve::Polygon(p) => vec![
                Field::length("Radius", Some(p.radius()), false, e),
                Field::count("Sides", p.sides(), e),
            ],
            Curve::Bezier(_) => vec![Field::count("Segments per span", s, e)],
            Curve::Helix(h) => vec![
                Field::length("Start radius", Some(h.start_radius()), false, e),
                Field::length("End radius", Some(h.end_radius()), false, e),
                Field::length("Pitch", Some(h.pitch()), false, e),
                Field::number("Turns", Some(h.turns()), e),
                Field::count("Segments", s, e),
                Field::switch("Left-handed", h.left_handed(), e),
            ],
            Curve::Polyline(_) => Vec::new(),
        };
        if self.locked
            && let Some(last) = fields.pop()
        {
            fields.push(last.with_note(LOCKED));
        }
        fields
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        if self.locked {
            return Outcome::Message(LOCKED.into());
        }
        match self.changed(index, value) {
            Ok((curve, segments)) => {
                self.curve = curve.clone();
                self.segments = segments;
                let op = DrawOp::EditCurve {
                    id: self.id,
                    curve,
                    segments,
                };
                if std::mem::replace(&mut self.edited, true) {
                    Outcome::Redefine(op)
                } else {
                    Outcome::Commit(op)
                }
            }
            Err(why) => Outcome::Message(why),
        }
    }

    fn accept(&mut self) {}

    fn reopen(&mut self) -> bool {
        true
    }

    fn forget(&mut self) {
        self.edited = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ctx;
    use smith_infer::infer::DrawingAxes;
    use smith_ops::draw::{Placement, draw};

    #[test]
    fn first_change_commits_then_later_changes_redefine() {
        let mut m = Entities::new();
        let op = DrawOp::Circle {
            at: Placement {
                center: DVec3::ZERO,
                x_axis: DVec3::X,
                normal: DVec3::Z,
            },
            radius: 5.0,
            segments: 24,
        };
        let id = draw(&mut m, &op).unwrap().curve.unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = CurveEditTool::new(id, &m).unwrap();
        assert_eq!(t.fields()[0].value, Some(5.0));
        assert!(matches!(
            t.edit(0, 12.3456, &c),
            Outcome::Commit(DrawOp::EditCurve { segments: 24, .. })
        ));
        assert!(matches!(
            t.edit(1, 48.0, &c),
            Outcome::Redefine(DrawOp::EditCurve { segments: 48, .. })
        ));
        assert_eq!(t.fields()[0].value, Some(12.3456), "full precision kept");
        assert!(matches!(t.edit(0, 0.0, &c), Outcome::Message(_)));
    }
}
