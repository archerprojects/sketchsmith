//! SketchSmith — Polygon tool | v1.0.0 | Thursday 12 November 2026
//!
//! Click the centre, then a corner: the radius is measured to the
//! corners (inscribed in the circle). The polygon lies on the face the centre
//! snapped to, otherwise in the drawing red–green plane.
//!
//! Typed input: `Ns` sets the number of sides (default 6); a length or `Nr`
//! sets the radius. With no polygon in progress, either redefines the last one.
//!
//! Operation Inputs: Radius (live while drawing) and Sides. Before the first click,
//! typing goes to Sides.

use smith_geom::DVec3;
use smith_geom::tolerance::TOL_MERGE;
use smith_infer::units::TypedValue;
use smith_ops::draw::{DrawOp, Placement};

use crate::{
    Ctx, Field, Input, Outcome, Preview, Tool, as_count, closed_outline, face_normal, in_plane,
    plane_x,
};

/// Default number of sides.
pub const DEFAULT_SIDES: u32 = 6;

/// A placed polygon.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placed {
    at: Placement,
    radius: f64,
    sides: u32,
}

impl Placed {
    fn op(&self) -> DrawOp {
        DrawOp::Polygon {
            at: self.at,
            radius: self.radius,
            sides: self.sides,
        }
    }

    fn corners(&self) -> Vec<DVec3> {
        let y = self.at.normal.cross(self.at.x_axis);
        (0..self.sides)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / f64::from(self.sides);
                self.at.center + (self.at.x_axis * a.cos() + y * a.sin()) * self.radius
            })
            .collect()
    }
}

/// The Polygon tool.
#[derive(Debug)]
pub struct PolygonTool {
    centre: Option<(DVec3, DVec3)>,
    hover: Option<DVec3>,
    sides: u32,
    done: Option<Placed>,
    /// The Operation Inputs shows `done` (until accepted).
    shown: bool,
}

impl Default for PolygonTool {
    fn default() -> Self {
        Self {
            centre: None,
            hover: None,
            sides: DEFAULT_SIDES,
            done: None,
            shown: false,
        }
    }
}

impl PolygonTool {
    /// The last polygon, while the box still shows it.
    fn shown_done(&self) -> Option<Placed> {
        self.done.filter(|_| self.shown)
    }

    fn through(&self, p: DVec3, ctx: &Ctx<'_>) -> Option<Placed> {
        let (c, n) = self.centre?;
        let radial = in_plane(p - c, n);
        Some(Placed {
            at: Placement {
                center: c,
                x_axis: radial
                    .try_normalize()
                    .unwrap_or_else(|| plane_x(ctx.axes, n)),
                normal: n,
            },
            radius: radial.length(),
            sides: self.sides,
        })
    }

    fn commit(&mut self, p: Placed) -> Outcome {
        if p.radius < TOL_MERGE {
            return Outcome::Message("The radius is too small.".into());
        }
        self.centre = None;
        self.done = Some(p);
        self.shown = true;
        Outcome::Commit(p.op())
    }

    fn redefine(&mut self, p: Placed) -> Outcome {
        if p.radius < TOL_MERGE {
            return Outcome::Message("The radius is too small.".into());
        }
        self.done = Some(p);
        self.shown = true;
        Outcome::Redefine(p.op())
    }
}

impl Tool for PolygonTool {
    fn name(&self) -> &'static str {
        "Polygon"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Click(inf) => match self.centre {
                None => {
                    let n = face_normal(&inf, ctx.entities).unwrap_or(ctx.axes.z);
                    self.centre = Some((inf.point, n));
                    Outcome::Nothing
                }
                Some(_) => match self.through(inf.point, ctx) {
                    Some(p) => self.commit(p),
                    None => Outcome::Nothing,
                },
            },
            Input::Typed(TypedValue::Segments(n)) => {
                if n < 3 {
                    return Outcome::Message("A polygon needs at least 3 sides.".into());
                }
                self.sides = n;
                match self.shown_done() {
                    Some(d) if self.centre.is_none() => self.redefine(Placed { sides: n, ..d }),
                    _ => Outcome::Nothing,
                }
            }
            Input::Typed(TypedValue::Length(r) | TypedValue::Radius(r)) => match self.centre {
                Some((c, _)) => {
                    let shown = self.hover.and_then(|h| self.through(h, ctx));
                    match shown.or_else(|| self.through(c, ctx)) {
                        Some(p) => self.commit(Placed { radius: r, ..p }),
                        None => Outcome::Nothing,
                    }
                }
                None => match self.shown_done() {
                    Some(d) => self.redefine(Placed { radius: r, ..d }),
                    None => Outcome::Message("Click the centre first.".into()),
                },
            },
            Input::Typed(_) => {
                Outcome::Message("Polygon takes sides (e.g. 6s) or a radius.".into())
            }
            Input::Toggle | Input::Finish => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        self.centre.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        self.centre.map(|(c, _)| c)
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        let (Some((c, _)), Some(h)) = (self.centre, self.hover) else {
            return Preview::default();
        };
        let lines = self
            .through(h, ctx)
            .filter(|p| p.radius >= TOL_MERGE)
            .map(|p| closed_outline(&p.corners()))
            .unwrap_or_default();
        Preview {
            lines,
            band: Some([c, h]),
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.centre.is_some() {
            format!(
                "Polygon: click a corner, or type the radius · {} sides (type e.g. 8s)",
                self.sides
            )
        } else {
            format!("Polygon: click the centre · {} sides", self.sides)
        }
    }

    fn fields(&self) -> Vec<Field> {
        match (self.centre, self.shown_done()) {
            (Some((c, n)), _) => vec![
                Field::length(
                    "Radius",
                    self.hover.map(|h| in_plane(h - c, n).length()),
                    true,
                    true,
                ),
                Field::count("Sides", self.sides, true),
            ],
            (None, Some(d)) => vec![
                Field::length("Radius", Some(d.radius), false, true),
                Field::count("Sides", d.sides, true),
            ],
            (None, None) => vec![
                Field::length("Radius", None, false, false),
                Field::count("Sides", self.sides, true),
            ],
        }
    }

    fn primary_field(&self) -> usize {
        usize::from(self.centre.is_none() && self.shown_done().is_none())
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        let typed = match index {
            0 => TypedValue::Radius(value),
            _ => TypedValue::Segments(as_count(value)),
        };
        self.handle(Input::Typed(typed), ctx)
    }

    fn accept(&mut self) {
        self.shown = false;
    }

    fn reopen(&mut self) -> bool {
        self.shown = self.done.is_some();
        self.shown
    }

    fn forget(&mut self) {
        self.done = None;
        self.shown = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{click, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    #[test]
    fn typed_sides_make_a_hexagon_and_redefine() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = PolygonTool::default();
        t.handle(Input::Typed(TypedValue::Segments(6)), &c);
        click(&mut t, DVec3::ZERO, &c);
        let out = click(&mut t, v(4.0, 0.0, 0.0), &c);
        assert!(
            matches!(out, Outcome::Commit(DrawOp::Polygon { sides: 6, radius, .. }) if radius == 4.0)
        );
        let out = t.handle(Input::Typed(TypedValue::Segments(8)), &c);
        assert!(matches!(
            out,
            Outcome::Redefine(DrawOp::Polygon { sides: 8, .. })
        ));
    }
}
