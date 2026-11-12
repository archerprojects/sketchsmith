//! SketchSmith — Move tool | v1.0.0 | Thursday 12 November 2026
//!
//! Moves the selection; with nothing selected, the first click takes the edge
//! or face under the cursor. Click a start point, then an end point (the
//! usual snaps and axis locks apply), or type the distance or a vector.
//! Ctrl (a tap) switches Copy. After a copy, `5x` makes five copies one step
//! apart and `/4` spreads four copies evenly up to the moved position.
//! Operation Inputs shows the move along the drawing axes (X, Y, Z), Copy and
//! Copies; changing any rebuilds it in one undo step. A move that would bend
//! a face is refused; the geometry rules are in `smith_ops::modify`. With
//! proportional editing on, nearby connected points follow the move with a
//! falloff (not for copies).

use smith_geom::DVec3;
use smith_geom::tolerance::TOL_MERGE;
use smith_infer::units::TypedValue;
use smith_model::Pickable;
use smith_ops::draw::DrawOp;
use smith_ops::modify::Proportional;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count, axes_vector, item_lines};

/// A committed move.
#[derive(Debug, Clone, PartialEq)]
struct Done {
    items: Vec<Pickable>,
    delta: DVec3,
    copy: bool,
    copies: u32,
    divide: bool,
    proportional: Option<Proportional>,
}

impl Done {
    fn op(&self) -> DrawOp {
        DrawOp::Move {
            items: self.items.clone(),
            delta: self.delta,
            copy: self.copy,
            copies: self.copies,
            divide: self.divide,
            proportional: self.proportional,
        }
    }
}

/// The Move tool.
#[derive(Debug, Default)]
pub struct MoveTool {
    items: Vec<Pickable>,
    start: Option<DVec3>,
    hover: Option<DVec3>,
    copy: bool,
    proportional: Option<Proportional>,
    done: Option<Done>,
    /// Operation Inputs shows `done` (until accepted).
    shown: bool,
    /// The drawing axes last seen (red, green, blue); the world axes until
    /// the tool first sees them.
    frame: Option<[DVec3; 3]>,
}

impl MoveTool {
    fn frame(&self) -> [DVec3; 3] {
        self.frame.unwrap_or([DVec3::X, DVec3::Y, DVec3::Z])
    }

    fn see_axes(&mut self, ctx: &Ctx<'_>) {
        self.frame = Some([ctx.axes.x, ctx.axes.y, ctx.axes.z]);
    }

    fn shown_done(&self) -> Option<&Done> {
        self.done.as_ref().filter(|_| self.shown)
    }

    fn live(&self) -> Option<DVec3> {
        Some(self.hover? - self.start?)
    }

    fn commit(&mut self, delta: DVec3) -> Outcome {
        if delta.length() <= TOL_MERGE {
            return Outcome::Nothing;
        }
        self.start = None;
        let done = Done {
            items: self.items.clone(),
            delta,
            copy: self.copy,
            copies: 1,
            divide: false,
            proportional: self.proportional,
        };
        let op = done.op();
        self.done = Some(done);
        self.shown = true;
        Outcome::Commit(op)
    }

    fn redefine(&mut self, d: Done) -> Outcome {
        if d.delta.length() <= TOL_MERGE {
            return Outcome::Message("The distance is too small.".into());
        }
        let op = d.op();
        self.done = Some(d);
        self.shown = true;
        Outcome::Redefine(op)
    }

    /// A typed or edited vector, while moving or after.
    fn set_delta(&mut self, delta: DVec3) -> Outcome {
        if self.start.is_some() {
            return self.commit(delta);
        }
        match self.shown_done().cloned() {
            Some(d) => self.redefine(Done { delta, ..d }),
            None => Outcome::Message("Click a Start Point".into()),
        }
    }
}

impl Tool for MoveTool {
    fn name(&self) -> &'static str {
        "Move"
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        self.items = items.to_vec();
    }

    fn use_proportional(&mut self, p: Option<Proportional>) {
        self.proportional = p;
    }

    fn pauses(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        self.see_axes(ctx);
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Click(inf) => match self.start {
                None => {
                    if self.items.is_empty() || self.done.is_some() {
                        match inf.entity {
                            Some(p) => self.items = vec![p],
                            None if self.items.is_empty() => {
                                return Outcome::Message("Click an Edge or Face".into());
                            }
                            None => {}
                        }
                    }
                    self.done = None;
                    self.shown = false;
                    self.start = Some(inf.point);
                    self.hover = Some(inf.point);
                    Outcome::Nothing
                }
                Some(s) => self.commit(inf.point - s),
            },
            Input::Typed(TypedValue::Length(l)) => {
                let dir = match (self.live(), self.shown_done()) {
                    (Some(v), _) if self.start.is_some() => v,
                    (_, Some(d)) => d.delta,
                    _ => return Outcome::Message("Click a Start Point".into()),
                };
                match dir.try_normalize() {
                    Some(u) => self.set_delta(u * l),
                    None => Outcome::Message("Move the cursor to set the direction".into()),
                }
            }
            Input::Typed(TypedValue::Delta(v) | TypedValue::Relative(v)) => {
                self.set_delta(axes_vector(ctx.axes, v))
            }
            Input::Typed(TypedValue::Copies(n)) => match self.shown_done().cloned() {
                Some(d) if d.copy && n >= 1 => self.redefine(Done {
                    copies: n,
                    divide: false,
                    ..d
                }),
                _ => Outcome::Message("Copy first (Ctrl), then type the count".into()),
            },
            Input::Typed(TypedValue::Divide(n)) => match self.shown_done().cloned() {
                Some(d) if d.copy && n >= 1 => self.redefine(Done {
                    copies: n,
                    divide: true,
                    ..d
                }),
                _ => Outcome::Message("Copy first (Ctrl), then type the count".into()),
            },
            Input::Typed(_) => Outcome::Message("Type a distance".into()),
            Input::Toggle => {
                self.copy = !self.copy;
                match self.shown_done().cloned() {
                    Some(d) if self.start.is_none() => self.redefine(Done {
                        copy: self.copy,
                        ..d
                    }),
                    _ => Outcome::Nothing,
                }
            }
            Input::Finish => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        self.start.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        self.start
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        let (Some(s), Some(v)) = (self.start, self.live()) else {
            return Preview::default();
        };
        let lines = item_lines(ctx, &self.items)
            .into_iter()
            .map(|[a, b]| [a + v, b + v])
            .collect();
        Preview {
            lines,
            band: Some([s, s + v]),
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.start.is_some() {
            if self.copy {
                "Click the End Point, or Type the Distance · Ctrl: Move".into()
            } else {
                "Click the End Point, or Type the Distance · Ctrl: Copy".into()
            }
        } else if self.items.is_empty() {
            "Click an Edge or Face to Start".into()
        } else {
            "Click a Start Point".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        let along = |v: Option<DVec3>, axis: DVec3| v.map(|d| d.dot(axis));
        let live = self.start.is_some();
        let (v, copy, copies, editable) = match (live, self.shown_done()) {
            (true, _) => (self.live(), self.copy, 1, true),
            (false, Some(d)) => (Some(d.delta), d.copy, d.copies, true),
            (false, None) => (None, self.copy, 1, false),
        };
        // X, Y and Z are along the drawing axes.
        let [x, y, z] = self.frame();
        vec![
            Field::length("X", along(v, x), live, editable),
            Field::length("Y", along(v, y), live, editable),
            Field::length("Z", along(v, z), live, editable),
            Field::switch("Copy", copy, true),
            Field::count("Copies", copies, editable && copy),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        self.see_axes(ctx);
        let current = self
            .live()
            .filter(|_| self.start.is_some())
            .or_else(|| self.shown_done().map(|d| d.delta));
        match index {
            0..=2 => {
                let Some(v) = current else {
                    return Outcome::Nothing;
                };
                let axes = self.frame();
                let mut local = DVec3::new(v.dot(axes[0]), v.dot(axes[1]), v.dot(axes[2]));
                local[index] = value;
                self.set_delta(axes[0] * local.x + axes[1] * local.y + axes[2] * local.z)
            }
            3 => {
                if (value != 0.0) != self.copy {
                    self.handle(Input::Toggle, ctx)
                } else {
                    Outcome::Nothing
                }
            }
            4 => self.handle(Input::Typed(TypedValue::Copies(as_count(value))), ctx),
            _ => Outcome::Nothing,
        }
    }

    fn accept(&mut self) {
        self.shown = false;
    }

    fn reopen(&mut self) -> bool {
        if self.done.is_some() {
            self.shown = true;
        }
        self.shown
    }

    fn forget(&mut self) {
        self.done = None;
        self.shown = false;
    }
}

#[cfg(test)]
mod tests {
    use smith_infer::infer::{DrawingAxes, Inference, Kind};
    use smith_model::Entities;

    use super::*;

    fn inf(point: DVec3, entity: Option<Pickable>) -> Inference {
        Inference {
            point,
            kind: Kind::Free,
            from: None,
            from2: None,
            entity,
            locked: false,
            normal: None,
        }
    }

    #[test]
    fn move_then_copy_then_array() {
        let mut e = Entities::new();
        e.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(2.0, 0.0, 0.0),
                DVec3::new(2.0, 2.0, 0.0),
                DVec3::new(0.0, 2.0, 0.0),
            ],
            true,
        )
        .unwrap();
        let f = e.face_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let ctx = Ctx {
            entities: &e,
            axes: &axes,
            instances: &smith_ops::modify::InstanceBoxes::new(),
            instance_lines: &smith_ops::modify::InstanceLines::new(),
            pixel: 0.01,
            cursor: smith_geom::DVec2::ZERO,
        };
        let mut t = MoveTool::default();
        t.use_selection(&[Pickable::Face(f)]);
        t.handle(Input::Click(inf(DVec3::ZERO, None)), &ctx);
        t.handle(Input::Hover(inf(DVec3::new(3.0, 0.0, 0.0), None)), &ctx);
        let out = t.handle(Input::Typed(TypedValue::Length(5.0)), &ctx);
        let moved = DrawOp::Move {
            items: vec![Pickable::Face(f)],
            delta: DVec3::new(5.0, 0.0, 0.0),
            copy: false,
            copies: 1,
            divide: false,
            proportional: None,
        };
        assert_eq!(out, Outcome::Commit(moved));
        let out = t.handle(Input::Toggle, &ctx);
        assert!(matches!(
            out,
            Outcome::Redefine(DrawOp::Move { copy: true, .. })
        ));
        let out = t.handle(Input::Typed(TypedValue::Copies(5)), &ctx);
        assert!(matches!(
            out,
            Outcome::Redefine(DrawOp::Move {
                copy: true,
                copies: 5,
                divide: false,
                ..
            })
        ));
        let out = t.handle(Input::Typed(TypedValue::Divide(4)), &ctx);
        assert!(matches!(
            out,
            Outcome::Redefine(DrawOp::Move {
                copies: 4,
                divide: true,
                ..
            })
        ));
    }
}
