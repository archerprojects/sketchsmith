//! SketchSmith — Line tool | v1.0.0 | Thursday 12 November 2026
//!
//! Click to start, click each next point; every segment is its own
//! undo step and triggers the sticky rules (splitting, auto-face). The chain
//! ends when it closes on its start point, or with Esc.
//!
//! Typed input: a length along the rubber band; `x,y,z` or `<x,y,z>` as an
//! offset from the last point; `[x,y,z]` as an absolute point (all in drawing
//! axes). With no chain in progress, a length redefines the last segment.
//!
//! Operation Inputs: Length — live while drawing, the last segment's exact length
//! after it.

use smith_geom::DVec3;
use smith_geom::tolerance::coincident;
use smith_infer::infer::arc_continuation;
use smith_infer::units::TypedValue;
use smith_ops::draw::DrawOp;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, axes_point, axes_vector};

/// The Line tool.
#[derive(Debug, Default)]
pub struct LineTool {
    /// First point of the chain, to detect closure.
    start: Option<DVec3>,
    /// Last placed point.
    last: Option<DVec3>,
    hover: Option<DVec3>,
    /// The last committed segment, for redefinition.
    done: Option<(DVec3, DVec3)>,
    /// The Operation Inputs shows `done` (until accepted).
    shown: bool,
}

impl LineTool {
    /// Places a point: starts the chain, or commits a segment to it.
    fn place(&mut self, p: DVec3) -> Outcome {
        let Some(last) = self.last else {
            self.start = Some(p);
            self.last = Some(p);
            return Outcome::Nothing;
        };
        if coincident(p, last) {
            return Outcome::Nothing;
        }
        self.done = Some((last, p));
        self.shown = true;
        if self.start.is_some_and(|s| coincident(s, p)) {
            self.start = None;
            self.last = None;
        } else {
            self.last = Some(p);
        }
        Outcome::Commit(DrawOp::Line { a: last, b: p })
    }
}

impl LineTool {
    /// The last segment, while the box still shows it.
    fn shown_done(&self) -> Option<(DVec3, DVec3)> {
        self.done.filter(|_| self.shown)
    }
}

impl Tool for LineTool {
    fn name(&self) -> &'static str {
        "Line"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Click(inf) => self.place(inf.point),
            Input::Typed(TypedValue::Length(len)) => {
                match (self.last, self.hover, self.shown_done()) {
                    (Some(last), Some(h), _) => match (h - last).try_normalize() {
                        Some(dir) => self.place(last + dir * len),
                        None => Outcome::Message("Move the cursor to show the direction.".into()),
                    },
                    (None, _, Some((a, b))) => match (b - a).try_normalize() {
                        Some(dir) => {
                            let b2 = a + dir * len;
                            self.done = Some((a, b2));
                            Outcome::Redefine(DrawOp::Line { a, b: b2 })
                        }
                        None => Outcome::Nothing,
                    },
                    _ => Outcome::Message("Click the start point first.".into()),
                }
            }
            Input::Typed(TypedValue::Delta(d) | TypedValue::Relative(d)) => match self.last {
                Some(last) => self.place(last + axes_vector(ctx.axes, d)),
                None => Outcome::Message("Click the start point first.".into()),
            },
            Input::Typed(TypedValue::Absolute(p)) => self.place(axes_point(ctx.axes, p)),
            Input::Typed(_) => {
                Outcome::Message("Line takes a length, x,y,z, <x,y,z> or [x,y,z].".into())
            }
            Input::Toggle => Outcome::Nothing,
            // Enter ends the chain, keeping what is drawn.
            Input::Finish => {
                self.cancel();
                Outcome::Nothing
            }
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = self.last.is_some();
        self.start = None;
        self.last = None;
        busy
    }

    fn last_point(&self) -> Option<DVec3> {
        self.last
    }

    fn tangent(&self, ctx: &Ctx<'_>) -> Option<DVec3> {
        self.last.and_then(|l| arc_continuation(ctx.entities, l))
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        Preview {
            band: self.last.zip(self.hover).map(|(a, b)| [a, b]),
            ..Preview::default()
        }
    }

    fn status(&self) -> String {
        if self.last.is_some() {
            "Line: click the next point or type a length · Shift locks · arrows lock axes · Esc ends the line".into()
        } else {
            "Line: click the start point".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        let f = match (self.last, self.hover, self.shown_done()) {
            (Some(a), h, _) => Field::length("Length", h.map(|b| a.distance(b)), true, true),
            (None, _, Some((a, b))) => Field::length("Length", Some(a.distance(b)), false, true),
            _ => Field::length("Length", None, false, false),
        };
        vec![f]
    }

    fn edit(&mut self, _index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
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
    use crate::testing::{click, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    #[test]
    fn chain_commits_each_segment_and_ends_on_closure() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = LineTool::default();
        assert_eq!(click(&mut t, v(0.0, 0.0, 0.0), &c), Outcome::Nothing);
        assert!(matches!(
            click(&mut t, v(5.0, 0.0, 0.0), &c),
            Outcome::Commit(_)
        ));
        assert!(matches!(
            click(&mut t, v(5.0, 5.0, 0.0), &c),
            Outcome::Commit(_)
        ));
        let closing = click(&mut t, v(0.0, 0.0, 0.0), &c);
        assert_eq!(
            closing,
            Outcome::Commit(DrawOp::Line {
                a: v(5.0, 5.0, 0.0),
                b: v(0.0, 0.0, 0.0)
            })
        );
        assert!(t.last_point().is_none(), "closing the loop ends the chain");
    }

    /// The end point of a line outcome, for approximate comparisons.
    fn end(out: &Outcome) -> Option<DVec3> {
        match out {
            Outcome::Commit(DrawOp::Line { b, .. }) | Outcome::Redefine(DrawOp::Line { b, .. }) => {
                Some(*b)
            }
            _ => None,
        }
    }

    #[test]
    fn typed_length_then_redefine() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = LineTool::default();
        click(&mut t, v(0.0, 0.0, 0.0), &c);
        t.handle(Input::Hover(crate::testing::at(v(3.0, 4.0, 0.0))), &c);
        let out = t.handle(Input::Typed(TypedValue::Length(10.0)), &c);
        assert!(matches!(out, Outcome::Commit(_)));
        assert!((end(&out).unwrap() - v(6.0, 8.0, 0.0)).length() < 1e-12);
        t.cancel();
        let out = t.handle(Input::Typed(TypedValue::Length(5.0)), &c);
        assert!(matches!(out, Outcome::Redefine(_)));
        assert!((end(&out).unwrap() - v(3.0, 4.0, 0.0)).length() < 1e-12);
        t.forget();
        assert!(matches!(
            t.handle(Input::Typed(TypedValue::Length(5.0)), &c),
            Outcome::Message(_)
        ));
    }
}
