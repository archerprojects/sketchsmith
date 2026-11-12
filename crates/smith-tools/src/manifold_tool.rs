//! SketchSmith — Make Manifold tool | v1.0.0 | Thursday 12 November 2026
//!
//! Set Merge Distance and Delete Interior in Operation Inputs, then press
//! Enter: Make Manifold runs on the selection's points, or on everything
//! in the open context when nothing is selected (closed groups are left
//! alone). Changing a value right after does it again (one undo step).
//! What it did appears in the report under Statistics while the tool is
//! active.

use smith_geom::DVec3;
use smith_model::Pickable;
use smith_ops::draw::DrawOp;
use smith_ops::manifold::MakeManifold;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool};

/// The Make Manifold tool.
#[derive(Debug, Default)]
pub struct ManifoldTool {
    /// The values, and the scope once taken.
    manifold: MakeManifold,
    /// What was selected when the tool started.
    items: Vec<Pickable>,
    /// A run was made and can still be redone.
    made: bool,
}

impl ManifoldTool {
    /// The tool with its default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the selection's points (or everything) and runs.
    fn run(&mut self, ctx: &Ctx<'_>) -> Outcome {
        self.manifold.scope = (!self.items.is_empty()).then(|| {
            smith_ops::modify::item_vertices(ctx.entities, &self.items)
                .into_iter()
                .filter_map(|v| ctx.entities.vertex(v).map(smith_model::Vertex::position))
                .collect()
        });
        self.made = true;
        Outcome::Commit(DrawOp::MakeManifold(self.manifold.clone()))
    }
}

impl Tool for ManifoldTool {
    fn name(&self) -> &'static str {
        "Make Manifold"
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        // Groups and guides are not changed from outside.
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
        let m = &self.manifold;
        vec![
            Field::length("Merge Distance", Some(m.distance), false, true),
            Field::switch("Delete Interior", m.delete_interior, true),
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
            0 if value > 0.0 => self.manifold.distance = value,
            1 => self.manifold.delete_interior = value != 0.0,
            _ => return Outcome::Nothing,
        }
        if self.made {
            Outcome::Redefine(DrawOp::MakeManifold(self.manifold.clone()))
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
        let mut t = ManifoldTool::new();
        assert_eq!(t.status(), "Enter Applies to All Geometry Open");
        let labels: Vec<_> = t.fields().iter().map(|f| f.label).collect();
        assert_eq!(labels, ["Merge Distance", "Delete Interior"]);
        let Outcome::Commit(DrawOp::MakeManifold(run)) = t.handle(Input::Finish, &c) else {
            panic!("no make manifold");
        };
        assert!(run.scope.is_none());
        assert!((run.distance - 0.01).abs() < 1e-12 && !run.delete_interior);
        let Outcome::Redefine(DrawOp::MakeManifold(run)) = t.edit(0, 0.5, &c) else {
            panic!("no redo");
        };
        assert!((run.distance - 0.5).abs() < 1e-12);
        let Outcome::Redefine(DrawOp::MakeManifold(run)) = t.edit(1, 1.0, &c) else {
            panic!("no redo");
        };
        assert!(run.delete_interior);
    }

    #[test]
    fn a_selection_becomes_its_points() {
        let mut m = Entities::new();
        m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
        let e = m.edge_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = ManifoldTool::new();
        t.use_selection(&[Pickable::Edge(e)]);
        assert_eq!(t.status(), "Enter Applies to the Selection");
        let Outcome::Commit(DrawOp::MakeManifold(run)) = t.handle(Input::Finish, &c) else {
            panic!("no make manifold");
        };
        assert_eq!(run.scope.map(|s| s.len()), Some(2));
    }
}
