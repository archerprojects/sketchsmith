//! SketchSmith — Rectangle and Rotated Rectangle tools | v1.0.0 | Thursday 12 November 2026
//!
//! **Rectangle.** Click a corner, then the opposite corner. The rectangle lies
//! on the face the first corner snapped to (sides along the red axis flattened
//! onto it), otherwise on the drawing-axis plane the two corners share (red–
//! green, red–blue or green–blue), defaulting to red–green. Typed `w,h` sets
//! the sides exactly, in the direction the cursor shows; typed again after
//! drawing, it redefines the rectangle.
//!
//! **Rotated Rectangle.** Click two points for the first side, then a third to
//! set the width and the plane. Typed lengths set the first side, then the
//! width; after drawing, a length redefines the width.
//!
//! Operation Inputs: Rectangle shows Width and Height; Rotated Rectangle shows
//! Length and Width. Each is live while drawing and editable after.

use smith_geom::DVec3;
use smith_geom::tolerance::{TOL_MERGE, coincident};
use smith_infer::infer::DrawingAxes;
use smith_infer::units::TypedValue;
use smith_ops::draw::{DrawOp, rectangle_corners};

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, closed_outline, face_normal, plane_x};

/// A placed rectangle: corner, in-plane axes and signed sides.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    p0: DVec3,
    u: DVec3,
    v: DVec3,
    du: f64,
    dv: f64,
}

impl Rect {
    fn op(&self) -> DrawOp {
        DrawOp::Rectangle {
            corners: rectangle_corners(self.p0, self.u, self.v, self.du, self.dv),
        }
    }
}

/// The Rectangle tool.
#[derive(Debug, Default)]
pub struct RectangleTool {
    p0: Option<DVec3>,
    /// Normal of the face the first corner is on, if any.
    face: Option<DVec3>,
    hover: Option<DVec3>,
    done: Option<Rect>,
    /// The Operation Inputs shows `done` (until accepted).
    shown: bool,
    /// Width and height under the cursor while drawing.
    live: Option<(f64, f64)>,
}

/// The in-plane axes for a rectangle from `p0` toward `p1`.
fn basis(p0: DVec3, p1: DVec3, face: Option<DVec3>, axes: &DrawingAxes) -> (DVec3, DVec3) {
    if let Some(n) = face {
        let u = plane_x(axes, n);
        return (u, n.cross(u));
    }
    let d = p1 - p0;
    let (cx, cy, cz) = (d.dot(axes.x), d.dot(axes.y), d.dot(axes.z));
    if cz.abs() > TOL_MERGE && cy.abs() <= TOL_MERGE && cx.abs() > TOL_MERGE {
        (axes.x, axes.z)
    } else if cz.abs() > TOL_MERGE && cx.abs() <= TOL_MERGE && cy.abs() > TOL_MERGE {
        (axes.y, axes.z)
    } else {
        (axes.x, axes.y)
    }
}

impl RectangleTool {
    fn rect_to(&self, p1: DVec3, axes: &DrawingAxes) -> Option<Rect> {
        let p0 = self.p0?;
        let (u, v) = basis(p0, p1, self.face, axes);
        let d = p1 - p0;
        Some(Rect {
            p0,
            u,
            v,
            du: d.dot(u),
            dv: d.dot(v),
        })
    }

    fn commit(&mut self, r: Rect) -> Outcome {
        if r.du.abs() < TOL_MERGE || r.dv.abs() < TOL_MERGE {
            return Outcome::Message("A rectangle needs a width and a height.".into());
        }
        self.p0 = None;
        self.face = None;
        self.done = Some(r);
        self.shown = true;
        Outcome::Commit(r.op())
    }
}

/// `1` or `-1` by the sign of `x` (positive for zero).
fn sign(x: f64) -> f64 {
    if x < 0.0 { -1.0 } else { 1.0 }
}

impl Tool for RectangleTool {
    fn name(&self) -> &'static str {
        "Rectangle"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                self.live = self
                    .rect_to(inf.point, ctx.axes)
                    .map(|r| (r.du.abs(), r.dv.abs()));
                Outcome::Nothing
            }
            Input::Click(inf) => {
                if self.p0.is_none() {
                    self.p0 = Some(inf.point);
                    self.face = face_normal(&inf, ctx.entities);
                    return Outcome::Nothing;
                }
                match self.rect_to(inf.point, ctx.axes) {
                    Some(r) => self.commit(r),
                    None => Outcome::Nothing,
                }
            }
            Input::Typed(TypedValue::Pair(a, b)) => {
                if let Some(p0) = self.p0 {
                    let shown = self
                        .hover
                        .and_then(|h| self.rect_to(h, ctx.axes))
                        .unwrap_or_else(|| {
                            let (u, v) = basis(p0, p0, self.face, ctx.axes);
                            Rect {
                                p0,
                                u,
                                v,
                                du: 1.0,
                                dv: 1.0,
                            }
                        });
                    let r = Rect {
                        du: a * sign(shown.du),
                        dv: b * sign(shown.dv),
                        ..shown
                    };
                    self.commit(r)
                } else if let Some(done) = self.done.filter(|_| self.shown) {
                    let r = Rect {
                        du: a * sign(done.du),
                        dv: b * sign(done.dv),
                        ..done
                    };
                    if r.du.abs() < TOL_MERGE || r.dv.abs() < TOL_MERGE {
                        return Outcome::Message("A rectangle needs a width and a height.".into());
                    }
                    self.done = Some(r);
                    Outcome::Redefine(r.op())
                } else {
                    Outcome::Message("Click the first corner, then type width,height.".into())
                }
            }
            Input::Typed(_) => {
                Outcome::Message("Rectangle takes width,height (e.g. 20,30).".into())
            }
            Input::Toggle | Input::Finish => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        self.face = None;
        self.p0.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        self.p0
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        let lines = self
            .hover
            .and_then(|h| self.rect_to(h, ctx.axes))
            .map(|r| closed_outline(&rectangle_corners(r.p0, r.u, r.v, r.du, r.dv)))
            .unwrap_or_default();
        Preview {
            lines,
            ..Preview::default()
        }
    }

    fn status(&self) -> String {
        if self.p0.is_some() {
            "Rectangle: click the opposite corner, or type width,height".into()
        } else {
            "Rectangle: click the first corner".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        let (r, live) = match self.p0 {
            Some(_) => (None, true),
            None => (self.done.filter(|_| self.shown), false),
        };
        let sides = match self.p0 {
            Some(_) => self.live,
            None => r.map(|r| (r.du.abs(), r.dv.abs())),
        };
        let editable = self.p0.is_some() || r.is_some();
        vec![
            Field::length("Width", sides.map(|s| s.0), live, editable),
            Field::length("Height", sides.map(|s| s.1), live, editable),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        let current = self.fields();
        let (w, h) = (current[0].value, current[1].value);
        let pair = if index == 0 {
            (value, h.unwrap_or(value))
        } else {
            (w.unwrap_or(value), value)
        };
        self.handle(Input::Typed(TypedValue::Pair(pair.0, pair.1)), ctx)
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

/// A placed rotated rectangle: first side and width vector.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rotated {
    p0: DVec3,
    p1: DVec3,
    w: DVec3,
}

impl Rotated {
    fn corners(&self) -> [DVec3; 4] {
        [self.p0, self.p1, self.p1 + self.w, self.p0 + self.w]
    }
}

/// The Rotated Rectangle tool.
#[derive(Debug, Default)]
pub struct RotatedRectangleTool {
    p0: Option<DVec3>,
    p1: Option<DVec3>,
    hover: Option<DVec3>,
    done: Option<Rotated>,
    /// The Operation Inputs shows `done` (until accepted).
    shown: bool,
}

impl RotatedRectangleTool {
    /// The last rectangle, while the box still shows it.
    fn shown_done(&self) -> Option<Rotated> {
        self.done.filter(|_| self.shown)
    }

    /// The width vector toward `p`: its part perpendicular to the first side.
    fn width_to(&self, p: DVec3) -> Option<Rotated> {
        let (p0, p1) = (self.p0?, self.p1?);
        let side = (p1 - p0).try_normalize()?;
        let d = p - p1;
        Some(Rotated {
            p0,
            p1,
            w: d - side * d.dot(side),
        })
    }

    fn commit(&mut self, r: Rotated) -> Outcome {
        if r.w.length() < TOL_MERGE {
            return Outcome::Message(
                "Move off the first side to give the rectangle a width.".into(),
            );
        }
        self.p0 = None;
        self.p1 = None;
        self.done = Some(r);
        self.shown = true;
        Outcome::Commit(DrawOp::Rectangle {
            corners: r.corners(),
        })
    }
}

impl Tool for RotatedRectangleTool {
    fn name(&self) -> &'static str {
        "Rotated Rectangle"
    }

    fn handle(&mut self, input: Input, _ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Click(inf) => match (self.p0, self.p1) {
                (None, _) => {
                    self.p0 = Some(inf.point);
                    Outcome::Nothing
                }
                (Some(p0), None) => {
                    if !coincident(p0, inf.point) {
                        self.p1 = Some(inf.point);
                    }
                    Outcome::Nothing
                }
                (Some(_), Some(_)) => match self.width_to(inf.point) {
                    Some(r) => self.commit(r),
                    None => Outcome::Nothing,
                },
            },
            Input::Typed(TypedValue::Length(len)) => match (self.p0, self.p1) {
                (Some(p0), None) => match self.hover.and_then(|h| (h - p0).try_normalize()) {
                    Some(dir) if len > TOL_MERGE => {
                        self.p1 = Some(p0 + dir * len);
                        Outcome::Nothing
                    }
                    _ => Outcome::Message("Move the cursor to show the side's direction.".into()),
                },
                (Some(_), Some(_)) => match self.hover.and_then(|h| self.width_to(h)) {
                    Some(r) => match r.w.try_normalize() {
                        Some(dir) => self.commit(Rotated { w: dir * len, ..r }),
                        None => Outcome::Message(
                            "Move off the first side to show the width direction.".into(),
                        ),
                    },
                    None => Outcome::Nothing,
                },
                (None, _) => match self
                    .shown_done()
                    .and_then(|d| d.w.try_normalize().map(|dir| (d, dir)))
                {
                    Some((d, dir)) if len > TOL_MERGE => {
                        let r = Rotated { w: dir * len, ..d };
                        self.done = Some(r);
                        Outcome::Redefine(DrawOp::Rectangle {
                            corners: r.corners(),
                        })
                    }
                    _ => Outcome::Message("Click the first corner.".into()),
                },
            },
            Input::Typed(_) => Outcome::Message("Rotated Rectangle takes a length.".into()),
            Input::Toggle | Input::Finish => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = self.p0.is_some();
        self.p0 = None;
        self.p1 = None;
        busy
    }

    fn last_point(&self) -> Option<DVec3> {
        self.p1.or(self.p0)
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        match (self.p0, self.p1, self.hover) {
            (Some(p0), None, Some(h)) => Preview {
                band: Some([p0, h]),
                ..Preview::default()
            },
            (Some(_), Some(_), Some(h)) => Preview {
                lines: self
                    .width_to(h)
                    .map(|r| closed_outline(&r.corners()))
                    .unwrap_or_default(),
                ..Preview::default()
            },
            _ => Preview::default(),
        }
    }

    fn status(&self) -> String {
        match (self.p0, self.p1) {
            (None, _) => "Rotated Rectangle: click the first corner".into(),
            (Some(_), None) => {
                "Rotated Rectangle: click the end of the first side, or type its length".into()
            }
            _ => "Rotated Rectangle: click to set the width and plane, or type the width".into(),
        }
    }

    fn fields(&self) -> Vec<Field> {
        let width = |r: Rotated| r.w.length();
        match (self.p0, self.p1) {
            (Some(p0), None) => vec![
                Field::length("Length", self.hover.map(|h| p0.distance(h)), true, true),
                Field::length("Width", None, true, false),
            ],
            (Some(p0), Some(p1)) => vec![
                Field::length("Length", Some(p0.distance(p1)), false, false),
                Field::length(
                    "Width",
                    self.hover.and_then(|h| self.width_to(h)).map(width),
                    true,
                    true,
                ),
            ],
            _ => {
                let d = self.shown_done();
                vec![
                    Field::length("Length", d.map(|d| d.p0.distance(d.p1)), false, d.is_some()),
                    Field::length("Width", d.map(width), false, d.is_some()),
                ]
            }
        }
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        // Changing the first side's length of a finished rectangle keeps its
        // start, direction and width.
        if index == 0
            && self.p0.is_none()
            && let Some(d) = self.shown_done()
        {
            let Some(dir) = (d.p1 - d.p0).try_normalize() else {
                return Outcome::Nothing;
            };
            if value < TOL_MERGE {
                return Outcome::Message("The side needs a length.".into());
            }
            let r = Rotated {
                p1: d.p0 + dir * value,
                ..d
            };
            self.done = Some(r);
            return Outcome::Redefine(DrawOp::Rectangle {
                corners: r.corners(),
            });
        }
        self.handle(Input::Typed(TypedValue::Length(value)), ctx)
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
    use crate::testing::{at, click, ctx, v};
    use smith_model::Entities;

    #[test]
    fn typed_pair_follows_the_cursor_direction() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = RectangleTool::default();
        click(&mut t, DVec3::ZERO, &c);
        t.handle(Input::Hover(at(v(-3.0, 2.0, 0.0))), &c);
        let out = t.handle(Input::Typed(TypedValue::Pair(20.0, 30.0)), &c);
        assert_eq!(
            out,
            Outcome::Commit(DrawOp::Rectangle {
                corners: [
                    DVec3::ZERO,
                    v(-20.0, 0.0, 0.0),
                    v(-20.0, 30.0, 0.0),
                    v(0.0, 30.0, 0.0)
                ]
            })
        );
        // Typing again redefines, keeping the direction.
        let out = t.handle(Input::Typed(TypedValue::Pair(5.0, 6.0)), &c);
        assert!(
            matches!(out, Outcome::Redefine(DrawOp::Rectangle { corners })
            if corners[2] == v(-5.0, 6.0, 0.0))
        );
    }

    #[test]
    fn vertical_rectangle_uses_the_red_blue_plane() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = RectangleTool::default();
        click(&mut t, DVec3::ZERO, &c);
        let out = click(&mut t, v(4.0, 0.0, 3.0), &c);
        assert_eq!(
            out,
            Outcome::Commit(DrawOp::Rectangle {
                corners: [
                    DVec3::ZERO,
                    v(4.0, 0.0, 0.0),
                    v(4.0, 0.0, 3.0),
                    v(0.0, 0.0, 3.0)
                ]
            })
        );
    }

    #[test]
    fn rotated_rectangle_three_points() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = RotatedRectangleTool::default();
        click(&mut t, DVec3::ZERO, &c);
        click(&mut t, v(3.0, 3.0, 0.0), &c);
        let out = click(&mut t, v(1.0, 5.0, 0.0), &c);
        let Outcome::Commit(DrawOp::Rectangle { corners }) = out else {
            panic!("expected a rectangle");
        };
        assert!((corners[2] - v(1.0, 5.0, 0.0)).length() < 1e-12);
        assert!((corners[3] - v(-2.0, 2.0, 0.0)).length() < 1e-12);
    }

    #[test]
    fn editing_one_side_keeps_the_other() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = RectangleTool::default();
        click(&mut t, DVec3::ZERO, &c);
        click(&mut t, v(4.0, 3.0, 0.0), &c);
        let out = t.edit(0, 10.0, &c);
        assert!(
            matches!(out, Outcome::Redefine(DrawOp::Rectangle { corners })
            if corners[2] == v(10.0, 3.0, 0.0))
        );
        let f = t.fields();
        assert_eq!((f[0].value, f[1].value), (Some(10.0), Some(3.0)));
    }
}
