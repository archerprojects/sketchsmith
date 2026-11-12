//! SketchSmith — Arc tool | v1.0.0 | Thursday 12 November 2026
//!
//! 2-point arc with bulge. Click the start, click the end, then move
//! out to set the bulge and click.
//!
//! **Plane.** The arc lies on the face the start point snapped to, or the
//! drawing red–green plane, while the chord lies in that plane and the cursor
//! stays within a few pixels of it; otherwise it lies in the plane through the
//! chord and the cursor.
//!
//! **Tangent arcs.** When the start point ends exactly one edge or arc, the
//! bulge snaps to the arc that leaves tangent to it ("Tangent to Edge"), so
//! arcs and lines chain smoothly.
//!
//! Typed input: a length sets the chord, then the bulge; `Nr` sets the radius
//! (the smaller arc); `Ns` sets the segment count (default 12). With no arc in
//! progress, each redefines the last arc.
//!
//! Operation Inputs: Chord, Bulge, Radius and Segments — live while drawing,
//! exact and editable after. Before the first click, typing goes to Segments.

use smith_geom::DVec3;
use smith_geom::curve::DEFAULT_ARC_SEGMENTS;
use smith_geom::tolerance::{TOL_MERGE, coincident};
use smith_infer::infer::continuation;
use smith_infer::units::TypedValue;
use smith_ops::draw::{DrawOp, arc_from_bulge, tangent_bulge};

use crate::{
    Ctx, Field, Input, Outcome, Preview, TOOL_SNAP_PX, Tool, arc_points, as_count, face_normal,
    open_outline,
};

/// A fully specified arc: chord ends, plane normal, signed bulge, segments.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bulged {
    a: DVec3,
    b: DVec3,
    normal: DVec3,
    sagitta: f64,
    segments: u32,
}

impl Bulged {
    fn op(&self) -> Option<DrawOp> {
        let (at, radius, sweep) = arc_from_bulge(self.a, self.b, self.sagitta, self.normal)?;
        Some(DrawOp::Arc {
            at,
            radius,
            sweep,
            segments: self.segments,
        })
    }
}

/// The start of an arc: point, incoming tangent, and the plane hint.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Start {
    a: DVec3,
    incoming: Option<DVec3>,
    hint: DVec3,
}

/// The Arc tool.
#[derive(Debug)]
pub struct ArcTool {
    start: Option<Start>,
    b: Option<DVec3>,
    hover: Option<DVec3>,
    /// The arc under the cursor, once both ends are placed.
    shown: Option<Bulged>,
    /// Whether `shown` snapped to the tangent arc.
    tangent: bool,
    segments: u32,
    done: Option<Bulged>,
    /// The Operation Inputs shows `done` (until accepted).
    shown_done: bool,
}

impl Default for ArcTool {
    fn default() -> Self {
        Self {
            start: None,
            b: None,
            hover: None,
            shown: None,
            tangent: false,
            segments: DEFAULT_ARC_SEGMENTS,
            done: None,
            shown_done: false,
        }
    }
}

impl ArcTool {
    fn busy(&self) -> bool {
        self.start.is_some()
    }

    /// The last arc, while the box still shows it.
    fn last(&self) -> Option<Bulged> {
        self.done.filter(|_| self.shown_done)
    }

    /// The arc's plane normal for a cursor at `h`.
    fn normal_for(s: &Start, b: DVec3, h: DVec3, ctx: &Ctx<'_>) -> Option<DVec3> {
        let chord = b - s.a;
        let chord_in_hint = chord.normalize_or_zero().dot(s.hint).abs() < 1e-9;
        let near_hint = (h - s.a).dot(s.hint).abs() <= TOOL_SNAP_PX * ctx.pixel;
        if chord_in_hint && near_hint {
            Some(s.hint)
        } else {
            chord.cross(h - s.a).try_normalize()
        }
    }

    /// Updates `shown` for the cursor at `h`.
    fn update(&mut self, h: DVec3, ctx: &Ctx<'_>) {
        self.shown = None;
        self.tangent = false;
        let (Some(s), Some(b)) = (self.start, self.b) else {
            return;
        };
        let Some(normal) = Self::normal_for(&s, b, h, ctx) else {
            return;
        };
        let e = (b - s.a).normalize_or_zero();
        let u = normal.cross(e);
        let mut sagitta = (h - (s.a + b) * 0.5).dot(u);
        if let Some(t) = s.incoming.and_then(|t| tangent_bulge(s.a, b, t, normal))
            && (sagitta - t).abs() <= TOOL_SNAP_PX * ctx.pixel
        {
            sagitta = t;
            self.tangent = true;
        }
        self.shown = Some(Bulged {
            a: s.a,
            b,
            normal,
            sagitta,
            segments: self.segments,
        });
    }

    fn commit(&mut self, arc: Bulged) -> Outcome {
        match arc.op() {
            Some(op) => {
                self.start = None;
                self.b = None;
                self.shown = None;
                self.done = Some(arc);
                self.shown_done = true;
                Outcome::Commit(op)
            }
            None => Outcome::Message("Move away from the chord to give the arc a bulge.".into()),
        }
    }

    fn redefine(&mut self, arc: Bulged) -> Outcome {
        match arc.op() {
            Some(op) => {
                self.done = Some(arc);
                self.shown_done = true;
                Outcome::Redefine(op)
            }
            None => Outcome::Message("That value gives no arc.".into()),
        }
    }

    /// The sagitta for radius `r` on chord length `c` (the smaller arc).
    fn sagitta_for_radius(r: f64, c: f64) -> Option<f64> {
        let half = c / 2.0;
        (r + TOL_MERGE >= half).then(|| r - (r * r - half * half).max(0.0).sqrt())
    }
}

/// `1` or `-1` by the sign of `x` (positive for zero).
fn sign(x: f64) -> f64 {
    if x < 0.0 { -1.0 } else { 1.0 }
}

impl Tool for ArcTool {
    fn name(&self) -> &'static str {
        "Arc"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                self.update(inf.point, ctx);
                Outcome::Nothing
            }
            Input::Click(inf) => match (self.start, self.b) {
                (None, _) => {
                    self.start = Some(Start {
                        a: inf.point,
                        incoming: continuation(ctx.entities, inf.point),
                        hint: face_normal(&inf, ctx.entities).unwrap_or(ctx.axes.z),
                    });
                    Outcome::Nothing
                }
                (Some(s), None) => {
                    if !coincident(s.a, inf.point) {
                        self.b = Some(inf.point);
                    }
                    Outcome::Nothing
                }
                (Some(_), Some(_)) => {
                    self.update(inf.point, ctx);
                    match self.shown {
                        Some(arc) => self.commit(arc),
                        None => Outcome::Nothing,
                    }
                }
            },
            Input::Typed(TypedValue::Length(len)) => match (self.start, self.b) {
                (Some(s), None) => match self.hover.and_then(|h| (h - s.a).try_normalize()) {
                    Some(dir) if len > TOL_MERGE => {
                        self.b = Some(s.a + dir * len);
                        Outcome::Nothing
                    }
                    _ => Outcome::Message("Move the cursor to show the chord's direction.".into()),
                },
                (Some(_), Some(_)) => match self.shown {
                    Some(arc) => self.commit(Bulged {
                        sagitta: len * sign(arc.sagitta),
                        ..arc
                    }),
                    None => Outcome::Message("Move the cursor to show the bulge side.".into()),
                },
                (None, _) => match self.last() {
                    Some(d) => self.redefine(Bulged {
                        sagitta: len * sign(d.sagitta),
                        ..d
                    }),
                    None => Outcome::Message("Click the start point first.".into()),
                },
            },
            Input::Typed(TypedValue::Radius(r)) => {
                let base = if self.busy() { self.shown } else { self.last() };
                let Some(arc) = base else {
                    return Outcome::Message("Place both ends of the arc first.".into());
                };
                match Self::sagitta_for_radius(r, arc.a.distance(arc.b)) {
                    Some(s) => {
                        let next = Bulged {
                            sagitta: s * sign(arc.sagitta),
                            ..arc
                        };
                        if self.busy() {
                            self.commit(next)
                        } else {
                            self.redefine(next)
                        }
                    }
                    None => Outcome::Message("The radius must be at least half the chord.".into()),
                }
            }
            Input::Typed(TypedValue::Segments(n)) => {
                if n < 1 {
                    return Outcome::Message("An arc needs at least 1 segment.".into());
                }
                self.segments = n;
                if let Some(arc) = self.shown.as_mut() {
                    arc.segments = n;
                }
                match self.last() {
                    Some(d) if !self.busy() => self.redefine(Bulged { segments: n, ..d }),
                    _ => Outcome::Nothing,
                }
            }
            Input::Typed(_) => Outcome::Message(
                "Arc takes a length, a radius (e.g. 5r) or segments (e.g. 12s).".into(),
            ),
            Input::Toggle | Input::Finish => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = self.busy();
        self.start = None;
        self.b = None;
        self.shown = None;
        busy
    }

    fn last_point(&self) -> Option<DVec3> {
        self.b.or(self.start.map(|s| s.a))
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        match (self.start, self.b, self.hover) {
            (Some(s), None, Some(h)) => Preview {
                band: Some([s.a, h]),
                ..Preview::default()
            },
            (Some(s), Some(b), _) => {
                let mut p = Preview {
                    lines: vec![[s.a, b]],
                    ..Preview::default()
                };
                if let Some(DrawOp::Arc {
                    at,
                    radius,
                    sweep,
                    segments,
                }) = self.shown.and_then(|a| a.op())
                {
                    p.lines = open_outline(&arc_points(&at, radius, sweep, segments));
                }
                if self.tangent {
                    p.note = Some("Tangent to Edge");
                }
                p
            }
            _ => Preview::default(),
        }
    }

    fn status(&self) -> String {
        match (self.start, self.b) {
            (None, _) => format!("Arc: click the start point · {} segments", self.segments),
            (Some(_), None) => "Arc: click the end point, or type the chord length".into(),
            _ => {
                "Arc: move out and click to set the bulge, or type it (or a radius, e.g. 5r)".into()
            }
        }
    }

    fn fields(&self) -> Vec<Field> {
        let radius = |a: &Bulged| {
            let (c, h) = (a.a.distance(a.b), a.sagitta.abs());
            (c * c / 4.0 + h * h) / (2.0 * h)
        };
        let segments = |n: u32| Field::count("Segments", n, true);
        match (self.start, self.b) {
            (Some(s), None) => vec![
                Field::length("Chord", self.hover.map(|h| s.a.distance(h)), true, true),
                Field::length("Bulge", None, true, false),
                Field::length("Radius", None, true, false),
                segments(self.segments),
            ],
            (Some(s), Some(b)) => vec![
                Field::length("Chord", Some(s.a.distance(b)), false, false),
                Field::length("Bulge", self.shown.map(|a| a.sagitta.abs()), true, true),
                Field::length("Radius", self.shown.map(|a| radius(&a)), true, true),
                segments(self.segments),
            ],
            (None, _) => match self.last() {
                Some(d) => vec![
                    Field::length("Chord", Some(d.a.distance(d.b)), false, true),
                    Field::length("Bulge", Some(d.sagitta.abs()), false, true),
                    Field::length("Radius", Some(radius(&d)), false, true),
                    segments(d.segments),
                ],
                None => vec![
                    Field::length("Chord", None, false, false),
                    Field::length("Bulge", None, false, false),
                    Field::length("Radius", None, false, false),
                    segments(self.segments),
                ],
            },
        }
    }

    fn primary_field(&self) -> usize {
        match (self.start, self.b, self.last()) {
            (Some(_), None, _) => 0,
            (Some(_), Some(_), _) | (None, _, Some(_)) => 1,
            _ => 3,
        }
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        match index {
            // A finished arc's chord: keep its start, direction, bulge side and
            // height.
            0 if !self.busy() => match self.last() {
                Some(d) => match (d.b - d.a).try_normalize() {
                    Some(dir) if value > TOL_MERGE => self.redefine(Bulged {
                        b: d.a + dir * value,
                        ..d
                    }),
                    _ => Outcome::Message("The chord needs a length.".into()),
                },
                None => Outcome::Nothing,
            },
            0 | 1 => self.handle(Input::Typed(TypedValue::Length(value)), ctx),
            2 => self.handle(Input::Typed(TypedValue::Radius(value)), ctx),
            _ => self.handle(Input::Typed(TypedValue::Segments(as_count(value))), ctx),
        }
    }

    fn accept(&mut self) {
        self.shown_done = false;
    }

    fn reopen(&mut self) -> bool {
        self.shown_done = self.done.is_some();
        self.shown_done
    }

    fn forget(&mut self) {
        self.done = None;
        self.shown_done = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{at, click, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    #[test]
    fn bulge_click_commits_an_arc() {
        let (m, axes) = (Entities::new(), DrawingAxes::default());
        let c = ctx(&m, &axes);
        let mut t = ArcTool::default();
        click(&mut t, v(0.0, 0.0, 0.0), &c);
        click(&mut t, v(10.0, 0.0, 0.0), &c);
        let out = click(&mut t, v(5.0, 2.0, 0.0), &c);
        let Outcome::Commit(DrawOp::Arc {
            radius,
            segments,
            at,
            ..
        }) = out
        else {
            panic!("expected an arc");
        };
        assert!((radius - 7.25).abs() < 1e-12, "(25 + 4) / 4");
        assert_eq!(segments, 12);
        assert!(
            (at.normal.z.abs() - 1.0).abs() < 1e-12,
            "on the ground plane"
        );
        // Typed radius redefines it.
        let out = t.handle(Input::Typed(TypedValue::Radius(5.0)), &c);
        assert!(
            matches!(out, Outcome::Redefine(DrawOp::Arc { radius, .. }) if (radius - 5.0).abs() < 1e-12)
        );
    }

    #[test]
    fn arc_snaps_tangent_to_the_line_it_starts_from() {
        let mut m = Entities::new();
        m.add_line(v(-10.0, 0.0, 0.0), v(0.0, 0.0, 0.0)).unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = ArcTool::default();
        click(&mut t, DVec3::ZERO, &c);
        click(&mut t, v(5.0, 5.0, 0.0), &c);
        // The tangent quarter arc bulges r(1 − cos 45°)·… toward +x/−y; hover
        // within the snap distance of it.
        let s = tangent_bulge(DVec3::ZERO, v(5.0, 5.0, 0.0), DVec3::X, DVec3::Z).unwrap();
        let u = DVec3::Z.cross(v(1.0, 1.0, 0.0).normalize());
        let h = v(2.5, 2.5, 0.0) + u * (s + 0.03);
        t.handle(Input::Hover(at(h)), &c);
        assert_eq!(t.preview(&c).note, Some("Tangent to Edge"));
        let Outcome::Commit(DrawOp::Arc { radius, .. }) = click(&mut t, h, &c) else {
            panic!("expected an arc");
        };
        assert!((radius - 5.0).abs() < 1e-9, "the tangent quarter arc");
    }
}
