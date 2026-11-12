//! SketchSmith — Extrude tool | v1.0.0 | Thursday 12 November 2026
//!
//! Click a face, move along its normal and click again; or type the distance.
//! The cursor is held to the line along the face normal through the clicked
//! point, so any snap along the way (an endpoint, another face) sets the
//! distance. A negative distance pushes back through the face. Ctrl (a tap)
//! switches New Start Face, which keeps the original face. After a push or
//! pull, Operation Inputs shows the distance and New Start Face; changing
//! either rebuilds it in one undo step. Double-clicking a face pushes or
//! pulls it by the last distance again. The geometry rules are in
//! `smith_ops::modify`.

use smith_geom::DVec3;
use smith_geom::tolerance::TOL_MERGE;
use smith_infer::infer::{Kind, Lock};
use smith_infer::units::TypedValue;
use smith_model::{FaceId, Pickable};
use smith_ops::draw::DrawOp;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool};

/// A push or pull in progress: the face, where it was clicked, and its normal.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pulling {
    face: FaceId,
    origin: DVec3,
    normal: DVec3,
}

/// A committed push or pull.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Done {
    face: FaceId,
    distance: f64,
    new_start: bool,
}

impl Done {
    fn op(&self) -> DrawOp {
        DrawOp::Extrude {
            face: self.face,
            distance: self.distance,
            new_start: self.new_start,
        }
    }
}

/// The Extrude tool.
#[derive(Debug, Default)]
pub struct ExtrudeTool {
    pulling: Option<Pulling>,
    hover: Option<DVec3>,
    new_start: bool,
    /// The last distance used, for double-click.
    last: Option<f64>,
    done: Option<Done>,
    /// Operation Inputs shows `done` (until accepted).
    shown: bool,
}

impl ExtrudeTool {
    fn shown_done(&self) -> Option<Done> {
        self.done.filter(|_| self.shown)
    }

    /// The distance the cursor sets, while pulling.
    fn live(&self) -> Option<f64> {
        let p = self.pulling?;
        Some((self.hover? - p.origin).dot(p.normal))
    }

    fn commit(&mut self, p: Pulling, distance: f64) -> Outcome {
        if distance.abs() <= TOL_MERGE {
            return Outcome::Nothing;
        }
        self.pulling = None;
        let done = Done {
            face: p.face,
            distance,
            new_start: self.new_start,
        };
        self.done = Some(done);
        self.shown = true;
        self.last = Some(distance);
        Outcome::Commit(done.op())
    }

    fn redefine(&mut self, d: Done) -> Outcome {
        if d.distance.abs() <= TOL_MERGE {
            return Outcome::Message("The distance is too small.".into());
        }
        self.done = Some(d);
        self.shown = true;
        self.last = Some(d.distance);
        Outcome::Redefine(d.op())
    }
}

impl Tool for ExtrudeTool {
    fn name(&self) -> &'static str {
        "Extrude"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Click(inf) => match self.pulling {
                None => match inf.entity {
                    Some(Pickable::Face(f)) => match ctx.entities.face(f) {
                        Some(face) => {
                            self.pulling = Some(Pulling {
                                face: f,
                                origin: inf.point,
                                normal: face.plane().normal(),
                            });
                            self.hover = Some(inf.point);
                            Outcome::Nothing
                        }
                        None => Outcome::Nothing,
                    },
                    _ => Outcome::Message("Click a Face".into()),
                },
                Some(p) => {
                    let d = (inf.point - p.origin).dot(p.normal);
                    self.commit(p, d)
                }
            },
            Input::Typed(TypedValue::Length(d)) => match (self.pulling, self.shown_done()) {
                (Some(p), _) => {
                    // A typed length follows the side the cursor is on.
                    let sign = if self.live().unwrap_or(1.0) < 0.0 {
                        -1.0
                    } else {
                        1.0
                    };
                    self.commit(p, d * sign)
                }
                (None, Some(done)) => self.redefine(Done {
                    distance: d,
                    ..done
                }),
                (None, None) => Outcome::Message("Click a Face".into()),
            },
            Input::Typed(_) => Outcome::Message("Type a distance".into()),
            Input::Toggle => {
                self.new_start = !self.new_start;
                match (self.pulling, self.shown_done()) {
                    (None, Some(done)) => self.redefine(Done {
                        new_start: self.new_start,
                        ..done
                    }),
                    _ => Outcome::Nothing,
                }
            }
            Input::Finish => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        self.pulling.take().is_some()
    }

    fn repeat(&mut self, _ctx: &Ctx<'_>) -> Outcome {
        // The double-click's first click started a pull on the face.
        match (self.pulling, self.last) {
            (Some(p), Some(d)) => self.commit(p, d),
            _ => Outcome::Nothing,
        }
    }

    fn last_point(&self) -> Option<DVec3> {
        self.pulling.map(|p| p.origin)
    }

    fn pauses(&self) -> bool {
        false
    }

    fn lock(&self) -> Option<Lock> {
        self.pulling.map(|p| Lock::Line {
            origin: p.origin,
            dir: p.normal,
            kind: Kind::Free,
        })
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        let (Some(p), Some(d)) = (self.pulling, self.live()) else {
            return Preview::default();
        };
        let Some(face) = ctx.entities.face(p.face) else {
            return Preview::default();
        };
        let offset = p.normal * d;
        let mut lines = Vec::new();
        for l in face.loops() {
            let pts = ctx.entities.loop_points(l);
            let k = pts.len();
            for i in 0..k {
                let (a, b) = (pts[i], pts[(i + 1) % k]);
                lines.push([a + offset, b + offset]);
                lines.push([a, a + offset]);
            }
        }
        Preview {
            lines,
            band: Some([p.origin, p.origin + offset]),
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.pulling.is_some() {
            "Move and Click, or Type the Distance · Ctrl: New Start Face".into()
        } else {
            "Click a Face".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        match (self.pulling, self.shown_done()) {
            (Some(_), _) => vec![
                Field::length("Distance", self.live(), true, true),
                Field::switch("New Start Face", self.new_start, true),
            ],
            (None, Some(d)) => vec![
                Field::length("Distance", Some(d.distance), false, true),
                Field::switch("New Start Face", d.new_start, true),
            ],
            (None, None) => vec![
                Field::length("Distance", None, false, false),
                Field::switch("New Start Face", self.new_start, true),
            ],
        }
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 => self.handle(Input::Typed(TypedValue::Length(value)), ctx),
            1 => {
                let on = value != 0.0;
                if on != self.new_start {
                    self.handle(Input::Toggle, ctx)
                } else {
                    Outcome::Nothing
                }
            }
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
    use smith_infer::infer::{DrawingAxes, Inference};
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
    fn click_face_then_click_pulls_and_typing_redefines() {
        let mut e = Entities::new();
        e.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(10.0, 0.0, 0.0),
                DVec3::new(10.0, 10.0, 0.0),
                DVec3::new(0.0, 10.0, 0.0),
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
        let mut t = ExtrudeTool::default();
        assert_eq!(
            t.handle(Input::Click(inf(DVec3::new(5.0, 5.0, 0.0), None)), &ctx),
            Outcome::Message("Click a Face".into())
        );
        let start = DVec3::new(5.0, 5.0, 0.0);
        t.handle(Input::Click(inf(start, Some(Pickable::Face(f)))), &ctx);
        assert!(t.lock().is_some());
        let out = t.handle(Input::Click(inf(start + DVec3::Z * 10.0, None)), &ctx);
        assert_eq!(
            out,
            Outcome::Commit(DrawOp::Extrude {
                face: f,
                distance: 10.0,
                new_start: false
            })
        );
        let out = t.handle(Input::Typed(TypedValue::Length(12.7)), &ctx);
        assert_eq!(
            out,
            Outcome::Redefine(DrawOp::Extrude {
                face: f,
                distance: 12.7,
                new_start: false
            })
        );
        let out = t.handle(Input::Toggle, &ctx);
        assert_eq!(
            out,
            Outcome::Redefine(DrawOp::Extrude {
                face: f,
                distance: 12.7,
                new_start: true
            })
        );
    }
}
