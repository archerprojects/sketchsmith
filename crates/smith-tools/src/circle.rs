//! SketchSmith — Circle tool | v1.0.0 | Thursday 12 November 2026
//!
//! **Centre mode** (default): click the centre, then a point on the circle. The
//! circle lies on the face the centre snapped to, otherwise in the drawing
//! red–green plane; its first segment starts at the clicked radius point.
//! **3-point mode** (Ctrl toggles): click three points on the circle.
//!
//! Typed input: a length or `Nr` sets the radius; `Ns` sets the segment count
//! (default 24), for the circle being drawn and the ones after it. With no
//! circle in progress, either redefines the last circle.
//!
//! Operation Inputs: Radius (live while drawing) and Segments. Before the first
//! click, typing goes to Segments, so a bare `48` sets 48 segments.

use smith_geom::DVec3;
use smith_geom::curve::DEFAULT_CIRCLE_SEGMENTS;
use smith_geom::tolerance::TOL_MERGE;
use smith_infer::units::TypedValue;
use smith_ops::draw::{DrawOp, Placement, circumcircle};

use crate::{
    Ctx, Field, Input, Outcome, Preview, Tool, arc_points, as_count, face_normal, in_plane,
    open_outline, plane_x,
};

/// A placed circle.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placed {
    at: Placement,
    radius: f64,
    segments: u32,
}

impl Placed {
    fn op(&self) -> DrawOp {
        DrawOp::Circle {
            at: self.at,
            radius: self.radius,
            segments: self.segments,
        }
    }
}

/// The Circle tool.
#[derive(Debug)]
pub struct CircleTool {
    three_points: bool,
    /// Centre and plane normal, once the centre is placed.
    centre: Option<(DVec3, DVec3)>,
    /// Points picked in 3-point mode.
    picked: Vec<DVec3>,
    hover: Option<DVec3>,
    segments: u32,
    done: Option<Placed>,
    /// The Operation Inputs shows `done` (until accepted).
    shown: bool,
}

impl Default for CircleTool {
    fn default() -> Self {
        Self {
            three_points: false,
            centre: None,
            picked: Vec::new(),
            hover: None,
            segments: DEFAULT_CIRCLE_SEGMENTS,
            done: None,
            shown: false,
        }
    }
}

impl CircleTool {
    fn busy(&self) -> bool {
        self.centre.is_some() || !self.picked.is_empty()
    }

    /// The last circle, while the box still shows it.
    fn shown_done(&self) -> Option<Placed> {
        self.done.filter(|_| self.shown)
    }

    /// The radius under the cursor while drawing.
    fn live_radius(&self) -> Option<f64> {
        let h = self.hover?;
        match self.centre {
            Some((c, n)) => Some(in_plane(h - c, n).length()),
            None => self.three(h).map(|c| c.radius),
        }
    }

    /// The circle from the placed centre through `p` (centre mode).
    fn through(&self, p: DVec3, ctx: &Ctx<'_>) -> Option<Placed> {
        let (c, n) = self.centre?;
        let radial = in_plane(p - c, n);
        let radius = radial.length();
        let x_axis = radial
            .try_normalize()
            .unwrap_or_else(|| plane_x(ctx.axes, n));
        Some(Placed {
            at: Placement {
                center: c,
                x_axis,
                normal: n,
            },
            radius,
            segments: self.segments,
        })
    }

    /// The circle through the two picked points and `p` (3-point mode).
    fn three(&self, p: DVec3) -> Option<Placed> {
        let [a, b] = self.picked[..] else {
            return None;
        };
        let (center, normal, radius) = circumcircle(a, b, p)?;
        Some(Placed {
            at: Placement {
                center,
                x_axis: (a - center) / radius,
                normal,
            },
            radius,
            segments: self.segments,
        })
    }

    fn commit(&mut self, c: Placed) -> Outcome {
        if c.radius < TOL_MERGE {
            return Outcome::Message("The radius is too small.".into());
        }
        self.centre = None;
        self.picked.clear();
        self.done = Some(c);
        self.shown = true;
        Outcome::Commit(c.op())
    }

    fn redefine(&mut self, c: Placed) -> Outcome {
        if c.radius < TOL_MERGE {
            return Outcome::Message("The radius is too small.".into());
        }
        self.done = Some(c);
        self.shown = true;
        Outcome::Redefine(c.op())
    }
}

impl Tool for CircleTool {
    fn name(&self) -> &'static str {
        "Circle"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Finish => Outcome::Nothing,
            Input::Toggle => {
                self.three_points = !self.three_points;
                self.centre = None;
                self.picked.clear();
                Outcome::Nothing
            }
            Input::Click(inf) if self.three_points => {
                let p = inf.point;
                if self.picked.iter().any(|q| q.distance(p) < TOL_MERGE) {
                    return Outcome::Nothing;
                }
                if self.picked.len() < 2 {
                    self.picked.push(p);
                    return Outcome::Nothing;
                }
                match self.three(p) {
                    Some(c) => self.commit(c),
                    None => {
                        self.picked.clear();
                        Outcome::Message("Those points are in a line; start again.".into())
                    }
                }
            }
            Input::Click(inf) => match self.centre {
                None => {
                    let n = face_normal(&inf, ctx.entities).unwrap_or(ctx.axes.z);
                    self.centre = Some((inf.point, n));
                    Outcome::Nothing
                }
                Some(_) => match self.through(inf.point, ctx) {
                    Some(c) => self.commit(c),
                    None => Outcome::Nothing,
                },
            },
            Input::Typed(TypedValue::Length(r) | TypedValue::Radius(r)) => {
                if self.centre.is_some() {
                    let shown = self.hover.and_then(|h| self.through(h, ctx));
                    match shown.or_else(|| self.through(self.centre?.0, ctx)) {
                        Some(c) => self.commit(Placed { radius: r, ..c }),
                        None => Outcome::Nothing,
                    }
                } else if !self.busy()
                    && let Some(d) = self.shown_done()
                {
                    self.redefine(Placed { radius: r, ..d })
                } else {
                    Outcome::Message("Click the centre first.".into())
                }
            }
            Input::Typed(TypedValue::Segments(n)) => {
                if n < 3 {
                    return Outcome::Message("A circle needs at least 3 segments.".into());
                }
                self.segments = n;
                match self.shown_done() {
                    Some(d) if !self.busy() => self.redefine(Placed { segments: n, ..d }),
                    _ => Outcome::Nothing,
                }
            }
            Input::Typed(_) => {
                Outcome::Message("Circle takes a radius or a segment count (e.g. 24s).".into())
            }
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = self.busy();
        self.centre = None;
        self.picked.clear();
        busy
    }

    fn last_point(&self) -> Option<DVec3> {
        self.centre.map(|(c, _)| c).or(self.picked.last().copied())
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        let Some(h) = self.hover else {
            return Preview::default();
        };
        let shown = if self.three_points {
            self.three(h)
        } else {
            self.through(h, ctx)
        };
        let mut p = Preview::default();
        if let Some(c) = shown.filter(|c| c.radius >= TOL_MERGE) {
            let pts = arc_points(&c.at, c.radius, std::f64::consts::TAU, c.segments);
            p.lines = open_outline(&pts);
        }
        if let Some((centre, _)) = self.centre {
            p.band = Some([centre, h]);
        } else if let [a] = self.picked[..] {
            p.band = Some([a, h]);
        }
        p
    }

    fn status(&self) -> String {
        let s = self.segments;
        if self.three_points {
            match self.picked.len() {
                0 => format!(
                    "Circle (3 points): click the first point · {s} segments · Ctrl: centre mode"
                ),
                1 => "Circle (3 points): click the second point".into(),
                _ => "Circle (3 points): click the third point".into(),
            }
        } else if self.centre.is_some() {
            format!("Circle: click to set the radius, or type it · {s} segments (type e.g. 48s)")
        } else {
            format!("Circle: click the centre · {s} segments · Ctrl: through 3 points")
        }
    }

    fn fields(&self) -> Vec<Field> {
        if self.busy() {
            vec![
                Field::length("Radius", self.live_radius(), true, self.centre.is_some()),
                Field::count("Segments", self.segments, true),
            ]
        } else if let Some(d) = self.shown_done() {
            vec![
                Field::length("Radius", Some(d.radius), false, true),
                Field::count("Segments", d.segments, true),
            ]
        } else {
            vec![
                Field::length("Radius", None, false, false),
                Field::count("Segments", self.segments, true),
            ]
        }
    }

    fn primary_field(&self) -> usize {
        usize::from(!self.busy() && self.shown_done().is_none())
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
    use crate::testing::{at, click, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    #[test]
    fn typed_segments_then_radius_and_redefine() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = CircleTool::default();
        assert_eq!(
            t.handle(Input::Typed(TypedValue::Segments(48)), &c),
            Outcome::Nothing
        );
        click(&mut t, v(1.0, 1.0, 0.0), &c);
        t.handle(Input::Hover(at(v(1.0, 3.0, 0.0))), &c);
        let out = t.handle(Input::Typed(TypedValue::Radius(0.5)), &c);
        let Outcome::Commit(DrawOp::Circle {
            radius,
            segments,
            at,
        }) = out
        else {
            panic!("expected a circle");
        };
        assert_eq!((radius, segments), (0.5, 48));
        assert!(
            (at.x_axis - DVec3::Y).length() < 1e-12,
            "starts toward the cursor"
        );
        // Redefine segments, then radius: each replaces the last circle.
        let out = t.handle(Input::Typed(TypedValue::Segments(24)), &c);
        assert!(matches!(
            out,
            Outcome::Redefine(DrawOp::Circle { segments: 24, .. })
        ));
        let out = t.handle(Input::Typed(TypedValue::Length(2.0)), &c);
        assert!(
            matches!(out, Outcome::Redefine(DrawOp::Circle { radius, segments: 24, .. }) if radius == 2.0)
        );
    }

    #[test]
    fn three_point_mode() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = CircleTool::default();
        t.handle(Input::Toggle, &c);
        click(&mut t, v(5.0, 0.0, 0.0), &c);
        click(&mut t, v(0.0, 5.0, 0.0), &c);
        let out = click(&mut t, v(-5.0, 0.0, 0.0), &c);
        let Outcome::Commit(DrawOp::Circle { at, radius, .. }) = out else {
            panic!("expected a circle");
        };
        assert!(at.center.length() < 1e-12);
        assert!((radius - 5.0).abs() < 1e-12);
        // Collinear points are refused.
        click(&mut t, v(0.0, 0.0, 0.0), &c);
        click(&mut t, v(1.0, 0.0, 0.0), &c);
        assert!(matches!(
            click(&mut t, v(2.0, 0.0, 0.0), &c),
            Outcome::Message(_)
        ));
    }

    #[test]
    fn operation_inputs_accept_and_reopen() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = CircleTool::default();
        assert_eq!(
            t.primary_field(),
            1,
            "typing goes to Segments before a click"
        );
        assert_eq!(t.fields()[1].value, Some(24.0));
        click(&mut t, DVec3::ZERO, &c);
        t.handle(Input::Hover(at(v(3.0, 4.0, 0.0))), &c);
        let f = t.fields();
        assert!(f[0].live && f[0].value == Some(5.0), "live radius");
        assert!(matches!(
            click(&mut t, v(3.0, 4.0, 0.0), &c),
            Outcome::Commit(_)
        ));
        let f = t.fields();
        assert!(!f[0].live && f[0].value == Some(5.0), "stored radius");
        assert_eq!(t.primary_field(), 0);
        // Enter accepts: the box resets and a count sets the next circle only.
        t.accept();
        assert_eq!(t.fields()[0].value, None);
        assert_eq!(t.edit(1, 48.0, &c), Outcome::Nothing);
        // F9 brings the last circle back; editing it redefines it.
        assert!(t.reopen());
        assert!(matches!(
            t.edit(1, 96.0, &c),
            Outcome::Redefine(DrawOp::Circle { segments: 96, .. })
        ));
        t.forget();
        assert!(!t.reopen());
    }
}
