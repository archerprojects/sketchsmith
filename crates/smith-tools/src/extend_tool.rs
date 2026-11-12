//! SketchSmith — Trim / Extend, Bisector, Extend to Face and Bridge tools | v1.0.0 | Thursday 12 November 2026
//!
//! * **Trim / Extend:** click two edges; both meet where their lines cross.
//! * **Bisector:** click two edges that aren't parallel; a guide line splits
//!   the angle between them.
//! * **Extend to Face:** click an edge near the end to extend, then a face.
//! * **Bridge:** click an edge of each of two open loops; a band of faces
//!   joins them. Operation Inputs: Vertices, Segments; changing either
//!   right after does it again (one undo step).

use smith_geom::DVec3;
use smith_infer::units::TypedValue;
use smith_model::Pickable;
use smith_ops::draw::DrawOp;
use smith_ops::sweep::edge_chain;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count};

/// Two-edge tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairKind {
    /// Trim / Extend.
    Trim,
    /// Bisector.
    Bisector,
}

/// Trim / Extend or Bisector.
#[derive(Debug)]
pub struct PairTool {
    kind: PairKind,
    /// The first edge picked, by its ends.
    first: Option<(DVec3, DVec3)>,
}

impl PairTool {
    /// The tool for `kind`.
    pub fn new(kind: PairKind) -> Self {
        Self { kind, first: None }
    }
}

impl Tool for PairTool {
    fn name(&self) -> &'static str {
        match self.kind {
            PairKind::Trim => "Trim / Extend",
            PairKind::Bisector => "Bisector",
        }
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        let Input::Click(inf) = input else {
            return Outcome::Nothing;
        };
        let Some(Pickable::Edge(e)) = inf.entity else {
            return Outcome::Message("Click an Edge".into());
        };
        let Some(edge) = ctx.entities.edge_points(e) else {
            return Outcome::Nothing;
        };
        let Some(first) = self.first.take() else {
            self.first = Some(edge);
            return Outcome::Nothing;
        };
        match self.kind {
            PairKind::Trim => Outcome::Commit(DrawOp::TrimExtend { a: first, b: edge }),
            PairKind::Bisector => match smith_ops::extend::bisector(first, edge) {
                Ok(guide) => Outcome::Commit(DrawOp::Guide { guide }),
                Err(err) => Outcome::Message(err.to_string()),
            },
        }
    }

    fn cancel(&mut self) -> bool {
        self.first.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        Preview {
            lines: self.first.map(|(a, b)| vec![[a, b]]).unwrap_or_default(),
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.first.is_none() {
            "Click the First Edge".into()
        } else {
            "Click the Second Edge".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        Vec::new()
    }

    fn edit(&mut self, _index: usize, _value: f64, _ctx: &Ctx<'_>) -> Outcome {
        Outcome::Nothing
    }

    fn accept(&mut self) {}

    fn reopen(&mut self) -> bool {
        false
    }

    fn forget(&mut self) {}
}

/// Extend to Face.
#[derive(Debug, Default)]
pub struct ExtendFaceTool {
    /// The edge picked and the point clicked near the end to extend.
    edge: Option<((DVec3, DVec3), DVec3)>,
}

impl Tool for ExtendFaceTool {
    fn name(&self) -> &'static str {
        "Extend to Face"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        let Input::Click(inf) = input else {
            return Outcome::Nothing;
        };
        match (self.edge, inf.entity) {
            (None, Some(Pickable::Edge(e))) => {
                if let Some(points) = ctx.entities.edge_points(e) {
                    self.edge = Some((points, inf.point));
                }
                Outcome::Nothing
            }
            (None, _) => Outcome::Message("Click an Edge Near the End to Extend".into()),
            (Some((edge, end)), Some(Pickable::Face(f))) => {
                let Some(face) = ctx.entities.face(f) else {
                    return Outcome::Nothing;
                };
                let Some(point) = ctx.entities.loop_points(face.outer()).first().copied() else {
                    return Outcome::Nothing;
                };
                self.edge = None;
                Outcome::Commit(DrawOp::ExtendToFace {
                    edge,
                    end,
                    point,
                    normal: face.plane().normal(),
                })
            }
            (Some(_), _) => Outcome::Message("Click the Face to Extend To".into()),
        }
    }

    fn cancel(&mut self) -> bool {
        self.edge.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        Preview {
            lines: self
                .edge
                .map(|((a, b), _)| vec![[a, b]])
                .unwrap_or_default(),
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.edge.is_none() {
            "Click an Edge Near the End to Extend".into()
        } else {
            "Click the Face to Extend To".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        Vec::new()
    }

    fn edit(&mut self, _index: usize, _value: f64, _ctx: &Ctx<'_>) -> Outcome {
        Outcome::Nothing
    }

    fn accept(&mut self) {}

    fn reopen(&mut self) -> bool {
        false
    }

    fn forget(&mut self) {}
}

/// Bridge.
#[derive(Debug, Default)]
pub struct BridgeTool {
    /// The first loop picked.
    first: Option<Vec<DVec3>>,
    /// Points round each loop (raised to the most either has).
    vertices: u32,
    /// Rings across the band.
    segments: u32,
    /// The last bridge, while it can still be redone.
    made: Option<DrawOp>,
}

impl BridgeTool {
    /// The last bridge again with the current values.
    fn redo(&mut self) -> Outcome {
        let Some(DrawOp::Bridge { a, b, .. }) = self.made.clone() else {
            return Outcome::Nothing;
        };
        let op = DrawOp::Bridge {
            a,
            b,
            vertices: self.vertices,
            segments: self.segments,
        };
        self.made = Some(op.clone());
        Outcome::Redefine(op)
    }
}

impl Tool for BridgeTool {
    fn name(&self) -> &'static str {
        "Bridge"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Click(inf) => {
                let Some(Pickable::Edge(e)) = inf.entity else {
                    return Outcome::Message("Click an Edge of an Open Loop".into());
                };
                let Some((points, true)) = edge_chain(ctx.entities, e) else {
                    return Outcome::Message("Click an Edge of a Closed Loop".into());
                };
                let Some(a) = self.first.take() else {
                    self.first = Some(points);
                    self.made = None;
                    return Outcome::Nothing;
                };
                let op = DrawOp::Bridge {
                    a,
                    b: points,
                    vertices: self.vertices,
                    segments: self.segments,
                };
                self.made = Some(op.clone());
                Outcome::Commit(op)
            }
            Input::Typed(TypedValue::Segments(n)) => {
                self.segments = n;
                self.redo()
            }
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        self.first.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        let lines = self
            .first
            .as_ref()
            .map(|p| (0..p.len()).map(|i| [p[i], p[(i + 1) % p.len()]]).collect())
            .unwrap_or_default();
        Preview {
            lines,
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.first.is_none() {
            "Click an Edge of the First Loop".into()
        } else {
            "Click an Edge of the Second Loop".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::count("Vertices", self.vertices, true),
            Field::count("Segments", self.segments, true),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 => self.vertices = as_count(value),
            1 => self.segments = as_count(value).min(256),
            _ => return Outcome::Nothing,
        }
        self.redo()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{at, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    #[test]
    fn two_edges_make_a_trim_and_a_bisector_guide() {
        let mut m = Entities::new();
        let a = m.add_line(v(2.0, 0.0, 0.0), v(9.0, 0.0, 0.0)).unwrap()[0];
        let b = m.add_line(v(0.0, 2.0, 0.0), v(0.0, 9.0, 0.0)).unwrap()[0];
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        for (kind, guide) in [(PairKind::Trim, false), (PairKind::Bisector, true)] {
            let mut t = PairTool::new(kind);
            for e in [a, b] {
                let mut inf = at(v(0.0, 0.0, 0.0));
                inf.entity = Some(Pickable::Edge(e));
                let out = t.handle(Input::Click(inf), &c);
                if e == b {
                    assert_eq!(matches!(out, Outcome::Commit(DrawOp::Guide { .. })), guide);
                    assert_eq!(
                        matches!(out, Outcome::Commit(DrawOp::TrimExtend { .. })),
                        !guide
                    );
                }
            }
        }
    }
}
