//! SketchSmith — Close Gaps tool | v1.0.0 | Thursday 12 November 2026
//!
//! Set Gap, Join Split Edges and Angle in Operation Inputs, then press
//! Enter: Close Gaps runs on the selection's points, or on everything in
//! the open context when nothing is selected (closed groups are left
//! alone). Changing a value right after does it again (one undo step).
//! What it did appears in the report under Statistics while the tool is
//! active.

use smith_geom::DVec3;
use smith_model::Pickable;
use smith_ops::draw::DrawOp;
use smith_ops::gaps::CloseGaps;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool};

/// The Close Gaps tool.
#[derive(Debug, Default)]
pub struct GapsTool {
    /// The values, and the scope once taken.
    gaps: CloseGaps,
    /// What was selected when the tool started.
    items: Vec<Pickable>,
    /// A run was made and can still be redone.
    made: bool,
}

impl GapsTool {
    /// The tool with its default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the selection's points (or everything) and runs.
    fn run(&mut self, ctx: &Ctx<'_>) -> Outcome {
        self.gaps.scope = (!self.items.is_empty()).then(|| {
            smith_ops::modify::item_vertices(ctx.entities, &self.items)
                .into_iter()
                .filter_map(|v| ctx.entities.vertex(v).map(smith_model::Vertex::position))
                .collect()
        });
        self.made = true;
        Outcome::Commit(DrawOp::CloseGaps(self.gaps.clone()))
    }
}

impl Tool for GapsTool {
    fn name(&self) -> &'static str {
        "Close Gaps"
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        // Groups and guides are not closed from outside.
        self.items = items
            .iter()
            .filter(|p| {
                matches!(
                    p,
                    Pickable::Vertex(_) | Pickable::Edge(_) | Pickable::Face(_)
                )
            })
            .copied()
            .collect();
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Finish => self.run(ctx),
            Input::Click(_) => Outcome::Message(self.status()),
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = !self.items.is_empty();
        self.items.clear();
        busy
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn wants_finish(&self) -> bool {
        true
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        Preview {
            lines: Vec::new(),
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.items.is_empty() {
            "Enter Applies to All Geometry Open".into()
        } else {
            "Enter Applies to the Selection".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        let g = &self.gaps;
        vec![
            Field::length("Gap", Some(g.gap), false, true),
            Field::switch("Join Split Edges", g.join, true),
            Field::number("Angle", Some(g.angle.to_degrees()), true),
        ]
    }

    fn primary_field(&self) -> usize {
        0
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        if !value.is_finite() {
            return Outcome::Nothing;
        }
        match index {
            0 if value > 0.0 => self.gaps.gap = value,
            1 => self.gaps.join = value != 0.0,
            2 => self.gaps.angle = value.clamp(0.0, 90.0).to_radians(),
            _ => return Outcome::Nothing,
        }
        if self.made {
            Outcome::Redefine(DrawOp::CloseGaps(self.gaps.clone()))
        } else {
            Outcome::Nothing
        }
    }

    fn accept(&mut self) {
        self.made = false;
    }

    fn reopen(&mut self) -> bool {
        false
    }

    fn forget(&mut self) {
        self.made = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    #[test]
    fn enter_runs_on_everything_and_a_value_redoes_it() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = GapsTool::new();
        assert_eq!(t.status(), "Enter Applies to All Geometry Open");
        let labels: Vec<_> = t.fields().iter().map(|f| f.label).collect();
        assert_eq!(labels, ["Gap", "Join Split Edges", "Angle"]);
        let Outcome::Commit(DrawOp::CloseGaps(run)) = t.handle(Input::Finish, &c) else {
            panic!("no close gaps");
        };
        assert!(run.scope.is_none());
        assert!((run.gap - 1.0).abs() < 1e-12 && !run.join);
        let Outcome::Redefine(DrawOp::CloseGaps(run)) = t.edit(0, 2.5, &c) else {
            panic!("no redo");
        };
        assert!((run.gap - 2.5).abs() < 1e-12);
        let Outcome::Redefine(DrawOp::CloseGaps(run)) = t.edit(1, 1.0, &c) else {
            panic!("no redo");
        };
        assert!(run.join);
    }

    #[test]
    fn a_selection_becomes_its_points() {
        let mut m = Entities::new();
        m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
        let e = m.edge_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = GapsTool::new();
        t.use_selection(&[Pickable::Edge(e)]);
        assert_eq!(t.status(), "Enter Applies to the Selection");
        let Outcome::Commit(DrawOp::CloseGaps(run)) = t.handle(Input::Finish, &c) else {
            panic!("no close gaps");
        };
        assert_eq!(run.scope.map(|s| s.len()), Some(2));
    }
}
