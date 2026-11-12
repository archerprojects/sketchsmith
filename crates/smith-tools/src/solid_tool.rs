//! SketchSmith — Inspect Manifold tool | v1.0.0 | Thursday 12 November 2026
//!
//! Inspect Manifold looks as soon as it starts: at the selection's points, or
//! at everything in the open context when nothing is selected (closed
//! groups are left alone). It changes nothing. What it found appears in the
//! report under Statistics; a click on a line selects it, for the tool that
//! fixes it. Changing Thickness, Sharp Angle or Overhang looks again.

use smith_geom::DVec3;
use smith_model::Pickable;
use smith_ops::draw::DrawOp;
use smith_ops::solid::InspectSolid;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool};

/// The Inspect Manifold tool.
#[derive(Debug, Default)]
pub struct SolidTool {
    /// The values, and the scope once taken.
    inspect: InspectSolid,
    /// What was selected when the tool started.
    items: Vec<Pickable>,
    /// It has looked once (so a value change looks again).
    made: bool,
}

impl SolidTool {
    /// The tool with its default values.
    pub fn new() -> Self {
        Self::default()
    }

    fn op(&self) -> DrawOp {
        DrawOp::InspectSolid(self.inspect.clone())
    }
}

impl Tool for SolidTool {
    fn name(&self) -> &'static str {
        "Inspect Manifold"
    }

    fn use_selection(&mut self, items: &[Pickable]) {
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

    fn start(&mut self, ctx: &Ctx<'_>) -> Outcome {
        self.inspect.scope = (!self.items.is_empty()).then(|| {
            smith_ops::modify::item_vertices(ctx.entities, &self.items)
                .into_iter()
                .filter_map(|v| ctx.entities.vertex(v).map(smith_model::Vertex::position))
                .collect()
        });
        self.made = true;
        Outcome::Commit(self.op())
    }

    fn handle(&mut self, input: Input, _ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Click(_) => Outcome::Message(self.status()),
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        false
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
        "Click a Line to Select What It Found".into()
    }

    fn fields(&self) -> Vec<Field> {
        let i = &self.inspect;
        vec![
            Field::length("Thickness", Some(i.thickness), false, true),
            Field::number("Sharp Angle", Some(i.sharp.to_degrees()), true),
            Field::number("Overhang", Some(i.overhang.to_degrees()), true),
        ]
    }

    fn primary_field(&self) -> usize {
        0
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        if !value.is_finite() || value < 0.0 {
            return Outcome::Nothing;
        }
        match index {
            0 if value > 0.0 => self.inspect.thickness = value,
            1 => self.inspect.sharp = value.clamp(0.0, 180.0).to_radians(),
            2 => self.inspect.overhang = value.clamp(0.0, 90.0).to_radians(),
            _ => return Outcome::Nothing,
        }
        if self.made {
            Outcome::Redefine(self.op())
        } else {
            Outcome::Nothing
        }
    }

    fn accept(&mut self) {}

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
    fn it_looks_on_start_and_a_value_looks_again() {
        let mut m = Entities::new();
        m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = SolidTool::new();
        assert_eq!(t.status(), "Click a Line to Select What It Found");
        let labels: Vec<_> = t.fields().iter().map(|f| f.label).collect();
        assert_eq!(labels, ["Thickness", "Sharp Angle", "Overhang"]);
        let Outcome::Commit(DrawOp::InspectSolid(run)) = t.start(&c) else {
            panic!("no inspect");
        };
        assert!(run.scope.is_none());
        let Outcome::Redefine(DrawOp::InspectSolid(run)) = t.edit(2, 5.0, &c) else {
            panic!("no look again");
        };
        assert!((run.overhang.to_degrees() - 5.0).abs() < 1e-9);
        let e = m.edge_ids().next().unwrap();
        let mut t = SolidTool::new();
        t.use_selection(&[Pickable::Edge(e)]);
        let Outcome::Commit(DrawOp::InspectSolid(run)) = t.start(&c) else {
            panic!("no inspect");
        };
        assert_eq!(run.scope.map(|s| s.len()), Some(2));
    }
}
