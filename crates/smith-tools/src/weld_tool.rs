//! SketchSmith — Weld and Point at Percentage tools | v1.0.0 | Thursday 12 November 2026
//!
//! **Weld:** click an edge and its whole connected chain becomes one curve
//! (or select edges first and press Enter: the chain through the first).
//! Operation Inputs: Vertices, never fewer than the chain has; more are
//! added along its edges without moving any. Changing it right after does
//! it again (one undo step).
//!
//! **Point at Percentage:** click an edge and a guide point is placed that
//! far along it, measured from the end nearer the click. Operation Inputs:
//! Percent (type a number to set it); changing it right after moves the
//! point (one undo step).

use smith_geom::DVec3;
use smith_infer::units::TypedValue;
use smith_model::{EdgeId, Guide, Pickable};
use smith_ops::draw::DrawOp;
use smith_ops::sweep::edge_chain;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count};

/// Weld.
#[derive(Debug, Default)]
pub struct WeldTool {
    /// Edges selected before the tool started.
    selected: Vec<EdgeId>,
    /// Points requested (raised to the chain's own count).
    vertices: u32,
    /// The chain's own point count, once picked.
    least: u32,
    /// The last weld made, while it can still be redone.
    made: Option<DrawOp>,
}

impl WeldTool {
    /// Welds the chain through `edge`.
    fn weld(&mut self, edge: EdgeId, ctx: &Ctx<'_>) -> Outcome {
        let Some((points, closed)) = edge_chain(ctx.entities, edge) else {
            return Outcome::Nothing;
        };
        if points.len() < 2 || (closed && points.len() < 3) {
            return Outcome::Message("Click a Chain of Edges".into());
        }
        self.least = u32::try_from(points.len()).unwrap_or(u32::MAX);
        let op = DrawOp::Weld {
            points,
            closed,
            vertices: self.vertices.max(self.least),
        };
        self.made = Some(op.clone());
        Outcome::Commit(op)
    }
}

impl Tool for WeldTool {
    fn name(&self) -> &'static str {
        "Weld"
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        self.selected = items
            .iter()
            .filter_map(|p| match p {
                Pickable::Edge(e) => Some(*e),
                _ => None,
            })
            .collect();
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Click(inf) => match inf.entity {
                Some(Pickable::Edge(e)) => self.weld(e, ctx),
                _ => Outcome::Message("Click an Edge".into()),
            },
            Input::Finish => match self.selected.first().copied() {
                Some(e) => {
                    self.selected.clear();
                    self.weld(e, ctx)
                }
                None => Outcome::Nothing,
            },
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = !self.selected.is_empty();
        self.selected.clear();
        busy
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn wants_finish(&self) -> bool {
        !self.selected.is_empty()
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        Preview {
            lines: Vec::new(),
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.selected.is_empty() {
            "Click an Edge of the Chain".into()
        } else {
            "Enter Welds the Selected Chain".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::count(
            "Vertices",
            self.vertices.max(self.least),
            true,
        )]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        if index != 0 {
            return Outcome::Nothing;
        }
        self.vertices = as_count(value).max(self.least);
        let Some(DrawOp::Weld { points, closed, .. }) = self.made.clone() else {
            return Outcome::Nothing;
        };
        let op = DrawOp::Weld {
            points,
            closed,
            vertices: self.vertices,
        };
        self.made = Some(op.clone());
        Outcome::Redefine(op)
    }

    fn accept(&mut self) {
        self.made = None;
    }

    fn reopen(&mut self) -> bool {
        false
    }

    fn forget(&mut self) {
        self.made = None;
    }
}

/// Point at Percentage.
#[derive(Debug)]
pub struct PercentTool {
    /// How far along, 0 to 100.
    percent: f64,
    /// The edge picked: from, to (from is the end nearer the click).
    edge: Option<(DVec3, DVec3)>,
    /// A point was placed and can still be moved.
    made: bool,
}

impl Default for PercentTool {
    fn default() -> Self {
        Self {
            percent: 50.0,
            edge: None,
            made: false,
        }
    }
}

impl PercentTool {
    /// The guide point for the edge picked.
    fn op(&self) -> Option<DrawOp> {
        let (a, b) = self.edge?;
        Some(DrawOp::Guide {
            guide: Guide::Point {
                point: a.lerp(b, self.percent / 100.0),
            },
        })
    }
}

impl Tool for PercentTool {
    fn name(&self) -> &'static str {
        "Point at Percentage"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Click(inf) => {
                let Some(Pickable::Edge(e)) = inf.entity else {
                    return Outcome::Message("Click an Edge".into());
                };
                let Some((a, b)) = ctx.entities.edge_points(e) else {
                    return Outcome::Nothing;
                };
                self.edge = Some(if b.distance(inf.point) < a.distance(inf.point) {
                    (b, a)
                } else {
                    (a, b)
                });
                self.made = true;
                self.op().map_or(Outcome::Nothing, Outcome::Commit)
            }
            Input::Typed(TypedValue::Length(v)) if (0.0..=100.0).contains(&v) => {
                self.percent = v;
                self.redo()
            }
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = self.edge.is_some();
        self.edge = None;
        self.made = false;
        busy
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
        "Click an Edge · Type a Percentage".into()
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::number("Percent", Some(self.percent), true)]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        if index != 0 || !(0.0..=100.0).contains(&value) {
            return Outcome::Nothing;
        }
        self.percent = value;
        self.redo()
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

impl PercentTool {
    /// The last point moved to the current percentage.
    fn redo(&mut self) -> Outcome {
        if !self.made {
            return Outcome::Nothing;
        }
        self.op().map_or(Outcome::Nothing, Outcome::Redefine)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{at, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    #[test]
    fn weld_takes_the_whole_chain_and_vertices_never_drop() {
        let mut m = Entities::new();
        m.add_polyline(
            &[v(0.0, 0.0, 0.0), v(5.0, 0.0, 0.0), v(5.0, 5.0, 0.0)],
            false,
        )
        .unwrap();
        let e = m.edge_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = WeldTool::default();
        let mut inf = at(v(1.0, 0.0, 0.0));
        inf.entity = Some(Pickable::Edge(e));
        let out = t.handle(Input::Click(inf), &c);
        let Outcome::Commit(DrawOp::Weld {
            points, vertices, ..
        }) = out
        else {
            panic!("no weld: {out:?}");
        };
        assert_eq!(points.len(), 3);
        assert_eq!(vertices, 3);
        assert!(matches!(
            t.edit(0, 1.0, &c),
            Outcome::Redefine(DrawOp::Weld { vertices: 3, .. })
        ));
        assert!(matches!(
            t.edit(0, 7.0, &c),
            Outcome::Redefine(DrawOp::Weld { vertices: 7, .. })
        ));
    }

    #[test]
    fn a_percentage_places_a_guide_point_from_the_nearer_end() {
        let mut m = Entities::new();
        let e = m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap()[0];
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = PercentTool::default();
        t.edit(0, 25.0, &c);
        let mut inf = at(v(9.0, 0.0, 0.0));
        inf.entity = Some(Pickable::Edge(e));
        let out = t.handle(Input::Click(inf), &c);
        let Outcome::Commit(DrawOp::Guide {
            guide: Guide::Point { point },
        }) = out
        else {
            panic!("no point: {out:?}");
        };
        assert!(point.distance(v(7.5, 0.0, 0.0)) < 1e-9);
    }
}
