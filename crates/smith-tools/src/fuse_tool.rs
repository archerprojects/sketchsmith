//! SketchSmith — Fuse tool | v1.0.0 | Thursday 12 November 2026
//!
//! Click the base, then the target: two closed groups, or two faces on one
//! plane. Fuse runs at once with the Operation in Operation Inputs (Union,
//! Difference or Intersect); changing it right after does it again (one
//! undo step). With one item selected when the tool starts, that is the
//! base.

use smith_geom::DVec3;
use smith_model::{BooleanOp, Pickable};
use smith_ops::draw::DrawOp;
use smith_ops::fuse::Fuse;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool};

/// The Fuse tool.
#[derive(Debug, Default)]
pub struct FuseTool {
    /// The operation.
    op: BooleanOp,
    /// The base, once picked.
    base: Option<Pickable>,
    /// The last base and target fused (for a redo).
    last: Option<(Pickable, Pickable)>,
}

impl FuseTool {
    /// The tool, with nothing picked.
    pub fn new() -> Self {
        Self::default()
    }

    fn op(&self, base: Pickable, target: Pickable) -> DrawOp {
        DrawOp::Fuse(Fuse {
            base,
            target,
            op: self.op,
        })
    }
}

/// What Fuse can pick.
fn pickable(p: Option<Pickable>) -> Option<Pickable> {
    p.filter(|p| matches!(p, Pickable::Instance(_) | Pickable::Face(_)))
}

impl Tool for FuseTool {
    fn name(&self) -> &'static str {
        "Fuse"
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        let picks: Vec<Pickable> = items
            .iter()
            .copied()
            .filter(|p| matches!(p, Pickable::Instance(_) | Pickable::Face(_)))
            .collect();
        self.base = match picks[..] {
            [one] => Some(one),
            _ => None,
        };
    }

    fn handle(&mut self, input: Input, _ctx: &Ctx<'_>) -> Outcome {
        let Input::Click(inf) = input else {
            return Outcome::Nothing;
        };
        let Some(picked) = pickable(inf.entity) else {
            return Outcome::Message(self.status());
        };
        match self.base {
            None => {
                self.base = Some(picked);
                self.last = None;
                Outcome::Message(self.status())
            }
            Some(base) if base == picked => Outcome::Message(self.status()),
            Some(base) => {
                self.base = None;
                self.last = Some((base, picked));
                Outcome::Commit(self.op(base, picked))
            }
        }
    }

    fn cancel(&mut self) -> bool {
        self.base.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        Preview {
            lines: Vec::new(),
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.base.is_none() {
            "Click the Base".into()
        } else {
            "Click the Target".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        let index = BooleanOp::ALL
            .iter()
            .position(|o| *o == self.op)
            .unwrap_or(0);
        vec![Field::choice("Operation", &BooleanOp::NAMES, index, true)]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        if index != 0 || !value.is_finite() {
            return Outcome::Nothing;
        }
        let Some(op) = BooleanOp::ALL.get(value.max(0.0) as usize) else {
            return Outcome::Nothing;
        };
        self.op = *op;
        match self.last {
            Some((base, target)) => Outcome::Redefine(self.op(base, target)),
            None => Outcome::Nothing,
        }
    }

    fn accept(&mut self) {
        self.last = None;
    }

    fn reopen(&mut self) -> bool {
        false
    }

    fn forget(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{at, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    #[test]
    fn base_then_target_then_the_operation_redoes_it() {
        let mut m = Entities::new();
        for x in [0.0, 20.0] {
            m.add_polyline(
                &[v(x, 0.0, 0.0), v(x + 5.0, 0.0, 0.0), v(x + 5.0, 5.0, 0.0)],
                true,
            )
            .unwrap();
        }
        let faces: Vec<_> = m.face_ids().collect();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = FuseTool::new();
        assert_eq!(t.status(), "Click the Base");
        let (a, b) = (Pickable::Face(faces[0]), Pickable::Face(faces[1]));
        let click = |p: Pickable| {
            let mut i = at(DVec3::ZERO);
            i.entity = Some(p);
            Input::Click(i)
        };
        assert!(matches!(t.handle(click(a), &c), Outcome::Message(_)));
        assert_eq!(t.status(), "Click the Target");
        let Outcome::Commit(DrawOp::Fuse(run)) = t.handle(click(b), &c) else {
            panic!("no fuse");
        };
        assert_eq!((run.base, run.target, run.op), (a, b, BooleanOp::Union));
        let Outcome::Redefine(DrawOp::Fuse(run)) = t.edit(0, 1.0, &c) else {
            panic!("no redo");
        };
        assert_eq!(run.op, BooleanOp::Difference);
        // One item selected beforehand is the base.
        let mut t = FuseTool::new();
        t.use_selection(&[a]);
        assert_eq!(t.status(), "Click the Target");
    }
}
