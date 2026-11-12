//! SketchSmith — Eraser tool | v1.0.0 | Thursday 12 November 2026
//!
//! Click an edge to erase it; coplanar faces either side heal into one, and a
//! face left without its boundary goes too. With Hide switched on (Operation
//! Inputs), clicked edges are hidden instead. A click on a guide erases it
//! (guides cannot be hidden). Each click is one undo step, and
//! the tool stays ready for the next click.

use smith_geom::DVec3;
use smith_model::Pickable;
use smith_ops::draw::DrawOp;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool};

/// The Eraser tool.
#[derive(Debug, Default)]
pub struct EraserTool {
    hide: bool,
}

impl Tool for EraserTool {
    fn name(&self) -> &'static str {
        "Eraser"
    }

    fn handle(&mut self, input: Input, _ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Click(inf) => match inf.entity {
                Some(Pickable::Edge(e)) => Outcome::Commit(DrawOp::Erase {
                    edges: vec![e],
                    hide: self.hide,
                }),
                // Guides cannot be hidden: a click erases one either way.
                Some(Pickable::Guide(g)) => Outcome::Commit(DrawOp::EraseGuide { guide: g }),
                _ => Outcome::Message("Click an Edge".into()),
            },
            Input::Toggle => {
                self.hide = !self.hide;
                Outcome::Nothing
            }
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        false
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn pauses(&self) -> bool {
        false
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        Preview::default()
    }

    fn status(&self) -> String {
        if self.hide {
            "Click an Edge to Hide · Ctrl: Erase".into()
        } else {
            "Click an Edge to Erase · Ctrl: Hide".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::switch("Hide", self.hide, true)]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        if index == 0 {
            self.hide = value != 0.0;
        }
        Outcome::Nothing
    }

    fn accept(&mut self) {}

    fn reopen(&mut self) -> bool {
        false
    }

    fn forget(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{at, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::{Entities, Guide};

    #[test]
    fn a_click_on_a_guide_erases_it_even_with_hide_on() {
        let mut m = Entities::new();
        let g = m
            .add_guide(Guide::point(v(1.0, 0.0, 0.0)).unwrap())
            .unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = EraserTool { hide: true };
        let mut inf = at(v(1.0, 0.0, 0.0));
        inf.entity = Some(Pickable::Guide(g));
        assert_eq!(
            t.handle(Input::Click(inf), &c),
            Outcome::Commit(DrawOp::EraseGuide { guide: g })
        );
    }
}
