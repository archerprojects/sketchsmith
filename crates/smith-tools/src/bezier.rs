//! SketchSmith — Bézier Spline tool | v1.0.0 | Thursday 12 November 2026
//!
//! Click the points the curve passes through; Enter finishes (two
//! points or more), Esc abandons. The result is a true Bézier curve entity
//! (`smith_ops::draw::smooth_through` places the control points), so its
//! segments per span stay editable afterwards — in the Operation Inputs, or later
//! with Edit Curve.
//!
//! Typed input: a length places the next point along the rubber band; `Ns`
//! sets segments per span (default 12), for this curve and the next. With no
//! curve in progress, a segment count redefines the last curve.

use smith_geom::DVec3;
use smith_geom::curve::{Bezier, Curve, DEFAULT_BEZIER_SEGMENTS};
use smith_geom::tolerance::coincident;
use smith_infer::units::TypedValue;
use smith_ops::draw::{DrawOp, smooth_through};

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count, open_outline};

/// The Bézier Spline tool.
#[derive(Debug)]
pub struct BezierTool {
    /// Points placed so far.
    points: Vec<DVec3>,
    hover: Option<DVec3>,
    /// Segments per span.
    segments: u32,
    /// The last curve: control points and segments.
    done: Option<(Vec<DVec3>, u32)>,
    /// The Operation Inputs shows `done` (until accepted).
    shown: bool,
}

impl Default for BezierTool {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            hover: None,
            segments: DEFAULT_BEZIER_SEGMENTS,
            done: None,
            shown: false,
        }
    }
}

impl BezierTool {
    fn shown_done(&self) -> Option<&(Vec<DVec3>, u32)> {
        self.done.as_ref().filter(|_| self.shown)
    }

    fn place(&mut self, p: DVec3) -> Outcome {
        if self.points.last().is_some_and(|q| coincident(*q, p)) {
            return Outcome::Nothing;
        }
        self.points.push(p);
        Outcome::Nothing
    }

    fn finish(&mut self) -> Outcome {
        if self.points.len() < 2 {
            return Outcome::Message("Place at least two points, then press Enter.".into());
        }
        let control_points = smooth_through(&std::mem::take(&mut self.points));
        self.done = Some((control_points.clone(), self.segments));
        self.shown = true;
        Outcome::Commit(DrawOp::Bezier {
            control_points,
            segments: self.segments,
        })
    }

    /// Outline points of the curve through `points` (plus the cursor).
    fn outline(&self) -> Vec<DVec3> {
        let mut pts = self.points.clone();
        if let Some(h) = self.hover
            && !pts.last().is_some_and(|q| coincident(*q, h))
        {
            pts.push(h);
        }
        if pts.len() < 2 {
            return Vec::new();
        }
        Bezier::new(smooth_through(&pts))
            .ok()
            .and_then(|b| Curve::Bezier(b).tessellate(self.segments).ok())
            .map(|t| t.points)
            .unwrap_or_default()
    }
}

impl Tool for BezierTool {
    fn name(&self) -> &'static str {
        "Bézier Spline"
    }

    fn handle(&mut self, input: Input, _ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Click(inf) => self.place(inf.point),
            Input::Finish => self.finish(),
            Input::Typed(TypedValue::Segments(n)) => {
                if n < 1 {
                    return Outcome::Message("A span needs at least 1 segment.".into());
                }
                self.segments = n;
                match self.shown_done() {
                    Some((cps, _)) if self.points.is_empty() => {
                        let control_points = cps.clone();
                        self.done = Some((control_points.clone(), n));
                        Outcome::Redefine(DrawOp::Bezier {
                            control_points,
                            segments: n,
                        })
                    }
                    _ => Outcome::Nothing,
                }
            }
            Input::Typed(TypedValue::Length(len)) => match (self.points.last(), self.hover) {
                (Some(&last), Some(h)) => match (h - last).try_normalize() {
                    Some(dir) => self.place(last + dir * len),
                    None => Outcome::Message("Move the cursor to show the direction.".into()),
                },
                _ => Outcome::Message("Click the first point first.".into()),
            },
            Input::Typed(_) => {
                Outcome::Message("Bézier Spline takes a length or segments (e.g. 12s).".into())
            }
            Input::Toggle => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = !self.points.is_empty();
        self.points.clear();
        busy
    }

    fn last_point(&self) -> Option<DVec3> {
        self.points.last().copied()
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        Preview {
            lines: open_outline(&self.outline()),
            band: self
                .points
                .last()
                .copied()
                .zip(self.hover)
                .map(|(a, b)| [a, b]),
            note: None,
        }
    }

    fn status(&self) -> String {
        match self.points.len() {
            0 => "Bézier Spline: click the first point the curve passes through".into(),
            1 => "Bézier Spline: click the next point".into(),
            _ => "Bézier Spline: click more points · Enter finishes · Esc cancels".into(),
        }
    }

    fn fields(&self) -> Vec<Field> {
        match self.shown_done() {
            Some((cps, segs)) if self.points.is_empty() => vec![
                Field::count("Points", ((cps.len() - 1) / 3 + 1) as u32, false),
                Field::count("Segments per span", *segs, true),
            ],
            _ => vec![
                Field::count("Points", self.points.len() as u32, false),
                Field::count("Segments per span", self.segments, true),
            ],
        }
    }

    fn primary_field(&self) -> usize {
        1
    }

    fn edit(&mut self, _index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        self.handle(Input::Typed(TypedValue::Segments(as_count(value))), ctx)
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
    fn points_then_enter_make_a_curve_with_editable_segments() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = BezierTool::default();
        assert!(matches!(t.handle(Input::Finish, &c), Outcome::Message(_)));
        click(&mut t, v(0.0, 0.0, 0.0), &c);
        click(&mut t, v(5.0, 5.0, 0.0), &c);
        click(&mut t, v(10.0, 0.0, 0.0), &c);
        let Outcome::Commit(DrawOp::Bezier {
            control_points,
            segments,
        }) = t.handle(Input::Finish, &c)
        else {
            panic!("expected a Bézier");
        };
        assert_eq!((control_points.len(), segments), (7, 12));
        assert_eq!(t.fields()[0].value, Some(3.0), "three points");
        let out = t.edit(1, 4.0, &c);
        assert!(matches!(
            out,
            Outcome::Redefine(DrawOp::Bezier { segments: 4, .. })
        ));
    }
}
