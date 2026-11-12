//! SketchSmith — Curve Maker tool | v1.0.0 | Thursday 12 November 2026
//!
//! Makes a helix, conical helix or flat spiral as a true curve
//! (`smith_geom::curve::Helix`), ready as a Sweep path for coils, springs and
//! twisted wire. Click the axis start (on a face the axis follows its normal,
//! otherwise the blue axis), then a point at the start radius. The curve is made
//! at once from the values in the Operation Inputs, and every value stays editable:
//!
//! | Field | Meaning |
//! |---|---|
//! | Start radius | radius at the start (set by the second click) |
//! | End radius | radius at the end; empty = same as the start (a plain helix) |
//! | Pitch | rise per turn; 0 = flat spiral; empty = 40 % of the start radius |
//! | Turns | number of turns (default 3; fractions allowed) |
//! | Segments per turn | default 24 |
//! | Left-handed | turns clockwise about the axis |
//!
//! Values set before the first click apply to the next curve.

use smith_geom::DVec3;
use smith_geom::curve::DEFAULT_HELIX_SEGMENTS_PER_TURN;
use smith_geom::tolerance::TOL_MERGE;
use smith_infer::units::TypedValue;
use smith_ops::draw::{DrawOp, Placement};

use crate::{
    Ctx, Field, Input, Outcome, Preview, Tool, as_count, face_normal, in_plane, open_outline,
    plane_x,
};

/// Default number of turns.
const DEFAULT_TURNS: f64 = 3.0;
/// Default pitch as a fraction of the start radius.
const DEFAULT_PITCH_RATIO: f64 = 0.4;

/// Settings for the next curve.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Params {
    end_radius: Option<f64>,
    pitch: Option<f64>,
    turns: f64,
    per_turn: u32,
    left_handed: bool,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            end_radius: None,
            pitch: None,
            turns: DEFAULT_TURNS,
            per_turn: DEFAULT_HELIX_SEGMENTS_PER_TURN,
            left_handed: false,
        }
    }
}

/// A made curve, fully specified.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Made {
    at: Placement,
    start_radius: f64,
    end_radius: f64,
    pitch: f64,
    turns: f64,
    per_turn: u32,
    left_handed: bool,
}

impl Made {
    fn from_params(at: Placement, start_radius: f64, p: &Params) -> Self {
        Self {
            at,
            start_radius,
            end_radius: p.end_radius.unwrap_or(start_radius),
            pitch: p.pitch.unwrap_or(start_radius * DEFAULT_PITCH_RATIO),
            turns: p.turns,
            per_turn: p.per_turn,
            left_handed: p.left_handed,
        }
    }

    fn segments(&self) -> u32 {
        ((f64::from(self.per_turn) * self.turns).ceil() as u32).max(1)
    }

    fn op(&self) -> DrawOp {
        DrawOp::Helix {
            at: self.at,
            start_radius: self.start_radius,
            end_radius: self.end_radius,
            pitch: self.pitch,
            turns: self.turns,
            left_handed: self.left_handed,
            segments: self.segments(),
        }
    }

    /// Points along the curve, for the preview.
    fn points(&self) -> Vec<DVec3> {
        let y = self.at.normal.cross(self.at.x_axis);
        let hand = if self.left_handed { -1.0 } else { 1.0 };
        let n = self.segments();
        (0..=n)
            .map(|i| {
                let t = f64::from(i) / f64::from(n);
                let r = self.start_radius + (self.end_radius - self.start_radius) * t;
                let a = std::f64::consts::TAU * self.turns * t * hand;
                self.at.center
                    + (self.at.x_axis * a.cos() + y * a.sin()) * r
                    + self.at.normal * (self.pitch * self.turns * t)
            })
            .collect()
    }

    /// The same curve with field `index` set to `value`.
    fn with(mut self, index: usize, value: f64) -> Self {
        match index {
            0 => self.start_radius = value,
            1 => self.end_radius = value,
            2 => self.pitch = value,
            3 => self.turns = value,
            4 => self.per_turn = as_count(value).max(1),
            _ => self.left_handed = value != 0.0,
        }
        self
    }
}

/// The Curve Maker tool.
#[derive(Debug, Default)]
pub struct CurveMakerTool {
    /// Axis start and axis direction, once placed.
    centre: Option<(DVec3, DVec3)>,
    hover: Option<DVec3>,
    params: Params,
    done: Option<Made>,
    /// The Operation Inputs shows `done` (until accepted).
    shown: bool,
}

impl CurveMakerTool {
    fn shown_done(&self) -> Option<Made> {
        self.done.filter(|_| self.shown)
    }

    /// The curve the cursor would make now.
    fn live(&self, ctx: Option<&Ctx<'_>>) -> Option<Made> {
        let (c, n) = self.centre?;
        let radial = in_plane(self.hover? - c, n);
        let x_axis = radial
            .try_normalize()
            .or_else(|| ctx.map(|ctx| plane_x(ctx.axes, n)))?;
        let at = Placement {
            center: c,
            x_axis,
            normal: n,
        };
        Some(Made::from_params(at, radial.length(), &self.params))
    }

    fn check(m: &Made) -> Option<&'static str> {
        if m.start_radius.max(m.end_radius) < TOL_MERGE
            || m.start_radius < 0.0
            || m.end_radius < 0.0
        {
            Some("The radius is too small.")
        } else if m.turns <= 0.0 {
            Some("Turns must be more than 0.")
        } else {
            None
        }
    }

    fn commit(&mut self, m: Made) -> Outcome {
        if let Some(why) = Self::check(&m) {
            return Outcome::Message(why.into());
        }
        self.centre = None;
        self.done = Some(m);
        self.shown = true;
        Outcome::Commit(m.op())
    }

    fn redefine(&mut self, m: Made) -> Outcome {
        if let Some(why) = Self::check(&m) {
            return Outcome::Message(why.into());
        }
        self.done = Some(m);
        Outcome::Redefine(m.op())
    }
}

impl Tool for CurveMakerTool {
    fn name(&self) -> &'static str {
        "Curve Maker"
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
                Some(_) => {
                    self.hover = Some(inf.point);
                    match self.live(Some(ctx)) {
                        Some(m) => self.commit(m),
                        None => Outcome::Nothing,
                    }
                }
            },
            Input::Typed(TypedValue::Length(r) | TypedValue::Radius(r)) => self.edit(0, r, ctx),
            Input::Typed(TypedValue::Segments(n)) => self.edit(4, f64::from(n), ctx),
            Input::Typed(_) => {
                Outcome::Message("Curve Maker takes a radius, or set values in the box.".into())
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

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        let Some(m) = self.live(None) else {
            return Preview::default();
        };
        Preview {
            lines: if Self::check(&m).is_none() {
                open_outline(&m.points())
            } else {
                Vec::new()
            },
            band: self.centre.zip(self.hover).map(|((c, _), h)| [c, h]),
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.centre.is_some() {
            "Curve Maker: click the start radius, or type it · set pitch, turns and more in the box"
                .into()
        } else {
            "Curve Maker: click the start of the axis (on a face the axis follows its normal)"
                .into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        let p = &self.params;
        let (shown, live) = match (self.centre, self.shown_done()) {
            (Some(_), _) => (self.live(None), true),
            (None, Some(d)) => (Some(d), false),
            (None, None) => (None, false),
        };
        match shown {
            Some(m) => vec![
                Field::length("Start radius", Some(m.start_radius), live, true),
                Field::length(
                    "End radius",
                    Some(m.end_radius),
                    live && p.end_radius.is_none(),
                    true,
                ),
                Field::length("Pitch", Some(m.pitch), live && p.pitch.is_none(), true),
                Field::number("Turns", Some(m.turns), true),
                Field::count("Segments per turn", m.per_turn, true),
                Field::switch("Left-handed", m.left_handed, true),
            ],
            None => vec![
                Field::length("Start radius", None, false, false),
                Field::length("End radius", p.end_radius, false, true),
                Field::length("Pitch", p.pitch, false, true),
                Field::number("Turns", Some(p.turns), true),
                Field::count("Segments per turn", p.per_turn, true),
                Field::switch("Left-handed", p.left_handed, true),
            ],
        }
    }

    fn primary_field(&self) -> usize {
        if self.centre.is_some() || self.shown_done().is_some() {
            0
        } else {
            3
        }
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        // A finished curve: change it.
        if self.centre.is_none()
            && let Some(d) = self.shown_done()
        {
            return self.redefine(d.with(index, value));
        }
        // While drawing, the start radius finishes the curve.
        if index == 0 {
            return match self.live(Some(ctx)) {
                Some(m) => self.commit(m.with(0, value)),
                None => Outcome::Message("Click the start of the axis first.".into()),
            };
        }
        // Otherwise the value is a setting for this and the next curves.
        let p = &mut self.params;
        match index {
            1 => p.end_radius = Some(value),
            2 => p.pitch = Some(value),
            3 if value > 0.0 => p.turns = value,
            3 => return Outcome::Message("Turns must be more than 0.".into()),
            4 => p.per_turn = as_count(value).max(1),
            _ => p.left_handed = value != 0.0,
        }
        Outcome::Nothing
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
    fn helix_from_two_clicks_then_edited() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = CurveMakerTool::default();
        assert_eq!(
            t.edit(2, 2.0, &c),
            Outcome::Nothing,
            "pitch set before drawing"
        );
        click(&mut t, DVec3::ZERO, &c);
        let Outcome::Commit(DrawOp::Helix {
            start_radius,
            end_radius,
            pitch,
            turns,
            segments,
            ..
        }) = click(&mut t, v(5.0, 0.0, 0.0), &c)
        else {
            panic!("expected a helix");
        };
        assert_eq!(
            (start_radius, end_radius, pitch, turns, segments),
            (5.0, 5.0, 2.0, 3.0, 72)
        );
        // A flat spiral: pitch 0 and a larger end radius.
        assert!(
            matches!(t.edit(2, 0.0, &c), Outcome::Redefine(DrawOp::Helix { pitch, .. }) if pitch == 0.0)
        );
        assert!(
            matches!(t.edit(1, 9.0, &c), Outcome::Redefine(DrawOp::Helix { end_radius, .. }) if end_radius == 9.0)
        );
        assert!(matches!(
            t.edit(5, 1.0, &c),
            Outcome::Redefine(DrawOp::Helix {
                left_handed: true,
                ..
            })
        ));
        assert!(matches!(t.edit(3, 0.0, &c), Outcome::Message(_)));
    }
}
