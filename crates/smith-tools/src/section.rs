//! SketchSmith — Section Plane tool | v1.0.0 | Thursday 12 November 2026
//!
//! Hover a face and the plane lies on it; elsewhere it lies flat on the
//! drawing axes, or on the plane the arrow keys choose. Click to place it:
//! it becomes the active section plane and cuts away the side facing the
//! viewer's side of the face (the side its normal points to). Type a
//! distance afterwards to move it that far along its normal from where it
//! was placed (one undo step), for an exact position. Clicking an existing
//! plane's outline with this tool turns it on or off (handled by the
//! canvas, which knows where the outlines are on screen).

use smith_geom::DVec3;
use smith_infer::units::TypedValue;
use smith_model::SectionPlane;
use smith_ops::draw::DrawOp;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, face_normal};

/// Half the side of the plane preview, in screen pixels.
const PREVIEW_PX: f64 = 60.0;

/// The Section Plane tool.
#[derive(Debug, Default)]
pub struct SectionTool {
    /// Where the cursor is, and the normal the plane would take there.
    hover: Option<(DVec3, DVec3)>,
    /// The plane chosen with the arrow keys, as its normal.
    axis_lock: Option<DVec3>,
    /// The last plane placed and how far it has been moved, for typing.
    placed: Option<(SectionPlane, f64, bool)>,
}

impl SectionTool {
    /// The square outline of a plane through `p` with normal `n`, and a
    /// short stroke along the normal.
    fn outline(p: DVec3, n: DVec3, ctx: &Ctx<'_>) -> Vec<[DVec3; 2]> {
        let h = PREVIEW_PX * ctx.pixel.max(1e-9);
        let u = n.any_orthonormal_vector() * h;
        let v = n.cross(u.normalize()) * h;
        let c = [p - u - v, p + u - v, p + u + v, p - u + v];
        let mut lines: Vec<[DVec3; 2]> = (0..4).map(|i| [c[i], c[(i + 1) % 4]]).collect();
        lines.push([p, p + n * h * 0.5]);
        lines
    }
}

impl Tool for SectionTool {
    fn name(&self) -> &'static str {
        "Section Plane"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                let n = self
                    .axis_lock
                    .or_else(|| face_normal(&inf, ctx.entities))
                    .unwrap_or(ctx.axes.z);
                self.hover = Some((inf.point, n));
                Outcome::Nothing
            }
            Input::Click(inf) => {
                let n = self
                    .axis_lock
                    .or_else(|| face_normal(&inf, ctx.entities))
                    .unwrap_or(ctx.axes.z);
                match SectionPlane::new(inf.point, n) {
                    Some(plane) => {
                        self.placed = Some((plane, 0.0, true));
                        Outcome::Commit(DrawOp::Section { plane })
                    }
                    None => Outcome::Nothing,
                }
            }
            Input::Typed(TypedValue::Length(l)) => self.edit(0, l, ctx),
            Input::Typed(_) => Outcome::Message("Type a Distance".into()),
            Input::Toggle | Input::Finish => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        self.axis_lock.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn use_axis_lock(&mut self, axis: Option<DVec3>) -> bool {
        self.axis_lock = axis;
        true
    }

    fn pauses(&self) -> bool {
        false
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        Preview {
            lines: self
                .hover
                .map(|(p, n)| Self::outline(p, n, ctx))
                .unwrap_or_default(),
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        match self.placed {
            Some((_, _, true)) => "Type a Distance to Move It · Click to Place Another".into(),
            _ => "Click a Face or a Point · Arrow Keys: Plane".into(),
        }
    }

    fn fields(&self) -> Vec<Field> {
        match self.placed {
            Some((_, moved, true)) => vec![Field::length("Distance", Some(moved), false, true)],
            _ => vec![Field::length("Distance", None, false, false)],
        }
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        match (index, self.placed) {
            (0, Some((plane, _, true))) => {
                let moved = SectionPlane {
                    point: plane.point + plane.normal * value,
                    ..plane
                };
                self.placed = Some((plane, value, true));
                Outcome::Redefine(DrawOp::Section { plane: moved })
            }
            _ => Outcome::Message("Click a Face or a Point".into()),
        }
    }

    fn accept(&mut self) {
        if let Some(p) = &mut self.placed {
            p.2 = false;
        }
    }

    fn reopen(&mut self) -> bool {
        match &mut self.placed {
            Some(p) => {
                p.2 = true;
                true
            }
            None => false,
        }
    }

    fn forget(&mut self) {
        self.placed = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{at, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    #[test]
    fn a_plane_placed_in_space_lies_flat_and_typing_moves_it_along_its_normal() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = SectionTool::default();
        let out = t.handle(Input::Click(at(v(1.0, 2.0, 3.0))), &c);
        let Outcome::Commit(DrawOp::Section { plane }) = out else {
            panic!("a plane");
        };
        assert_eq!(plane.normal, DVec3::Z);
        let out = t.handle(Input::Typed(TypedValue::Length(2.5)), &c);
        let Outcome::Redefine(DrawOp::Section { plane }) = out else {
            panic!("moved");
        };
        assert_eq!(plane.point, v(1.0, 2.0, 5.5));
    }

    #[test]
    fn the_arrow_keys_choose_the_plane() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = SectionTool::default();
        t.use_axis_lock(Some(DVec3::X));
        let out = t.handle(Input::Click(at(v(0.0, 0.0, 0.0))), &c);
        let Outcome::Commit(DrawOp::Section { plane }) = out else {
            panic!("a plane");
        };
        assert_eq!(plane.normal, DVec3::X);
    }
}
