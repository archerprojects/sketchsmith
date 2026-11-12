//! SketchSmith — Skinning tool | v1.0.0 | Thursday 12 November 2026
//!
//! Click the frame's edges (a curve's whole run at once; click again to
//! leave it out), or select them before starting the tool, then press
//! Enter: each cell of three or four sides the frame closes is covered with
//! a surface (see `smith_ops::skin`). Operation Inputs: Sampling (pieces
//! each way across a cell, 1–64) and Edges Only (the lines, no faces).
//! Changing either right after does it again (one undo step). The result is
//! a group named "Skin" that keeps its frame; the edges picked stay where
//! they were.
//!
//! **Edit Skin** on a Skin group reopens it here: Operation Inputs shows its
//! values and each change rebuilds it (one undo step each). Esc ends it.

use smith_geom::{DMat4, DVec3};
use smith_model::{DefinitionId, EdgeId, Pickable, SweepRecipe};
use smith_ops::draw::DrawOp;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count};

/// Skinning.
#[derive(Debug)]
pub struct SkinTool {
    /// The frame's edges picked.
    edges: Vec<EdgeId>,
    /// Pieces each way across a cell.
    sampling: u32,
    /// The lines only, no faces.
    edges_only: bool,
    /// The last skin made, while it can still be redone.
    made: Option<DrawOp>,
    /// Edit Skin: the Skin group being edited and its frame.
    editing: Option<(DefinitionId, Vec<[DVec3; 2]>)>,
}

impl Default for SkinTool {
    fn default() -> Self {
        Self {
            edges: Vec::new(),
            sampling: 8,
            edges_only: false,
            made: None,
            editing: None,
        }
    }
}

impl SkinTool {
    /// The skin again with the current values: the group being edited
    /// rebuilt, or the last one made redone.
    fn redo(&mut self) -> Outcome {
        if let Some((definition, segments)) = &self.editing {
            return Outcome::Commit(DrawOp::EditSweep {
                definition: *definition,
                recipe: SweepRecipe::Skin {
                    segments: segments.clone(),
                    sampling: self.sampling,
                    edges_only: self.edges_only,
                },
            });
        }
        let Some(DrawOp::Skin { segments, .. }) = self.made.clone() else {
            return Outcome::Nothing;
        };
        let op = DrawOp::Skin {
            segments,
            sampling: self.sampling,
            edges_only: self.edges_only,
        };
        self.made = Some(op.clone());
        Outcome::Redefine(op)
    }

    /// The edges a click on `e` picks: its curve's whole run, or itself.
    fn run_of(e: EdgeId, ctx: &Ctx<'_>) -> Vec<EdgeId> {
        ctx.entities
            .edge(e)
            .and_then(|x| x.curve())
            .and_then(|c| ctx.entities.curve(c))
            .map_or_else(|| vec![e], |c| c.edges().to_vec())
    }
}

impl Tool for SkinTool {
    fn name(&self) -> &'static str {
        "Skinning"
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        self.edges = items
            .iter()
            .filter_map(|p| match p {
                Pickable::Edge(e) => Some(*e),
                _ => None,
            })
            .collect();
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Click(_) if self.editing.is_some() => {
                Outcome::Message("Change It in Operation Inputs".into())
            }
            Input::Click(inf) => {
                let Some(Pickable::Edge(e)) = inf.entity else {
                    return Outcome::Message("Click the Frame's Edges".into());
                };
                let run = Self::run_of(e, ctx);
                if self.edges.contains(&e) {
                    self.edges.retain(|x| !run.contains(x));
                } else {
                    for x in run {
                        if !self.edges.contains(&x) {
                            self.edges.push(x);
                        }
                    }
                }
                self.made = None;
                Outcome::Nothing
            }
            Input::Finish => {
                if self.editing.is_some() {
                    return Outcome::Nothing;
                }
                let segments: Vec<[DVec3; 2]> = self
                    .edges
                    .iter()
                    .filter_map(|e| ctx.entities.edge_points(*e))
                    .map(|(a, b)| [a, b])
                    .collect();
                if segments.is_empty() {
                    return Outcome::Nothing;
                }
                if smith_ops::skin::frame_cells(&segments).cells == 0 {
                    return Outcome::Message("No Cell of Three or Four Sides".into());
                }
                self.edges.clear();
                let op = DrawOp::Skin {
                    segments,
                    sampling: self.sampling,
                    edges_only: self.edges_only,
                };
                self.made = Some(op.clone());
                Outcome::Commit(op)
            }
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        if self.editing.take().is_some() {
            return true;
        }
        let busy = !self.edges.is_empty();
        self.edges.clear();
        busy
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn wants_finish(&self) -> bool {
        !self.edges.is_empty()
    }

    fn use_sweep(&mut self, definition: DefinitionId, recipe: SweepRecipe, _placed: DMat4) {
        if let SweepRecipe::Skin {
            segments,
            sampling,
            edges_only,
        } = recipe
        {
            self.edges.clear();
            self.made = None;
            self.sampling = sampling;
            self.edges_only = edges_only;
            self.editing = Some((definition, segments));
        }
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        Preview {
            lines: self
                .edges
                .iter()
                .filter_map(|e| ctx.entities.edge_points(*e))
                .map(|(a, b)| [a, b])
                .collect(),
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.editing.is_some() {
            return "Change It in Operation Inputs · Esc Ends".into();
        }
        if self.edges.is_empty() {
            "Click the Frame's Edges".into()
        } else {
            "Click More Edges, or Enter Skins".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::count("Sampling", self.sampling, true),
            Field::switch("Edges Only", self.edges_only, true),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 => self.sampling = as_count(value).clamp(1, 64),
            1 => self.edges_only = value != 0.0,
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

    /// A square of four lines, bent up at one corner.
    fn frame() -> Entities {
        let mut m = Entities::new();
        m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
        m.add_line(v(10.0, 0.0, 0.0), v(10.0, 10.0, 4.0)).unwrap();
        m.add_line(v(10.0, 10.0, 4.0), v(0.0, 10.0, 0.0)).unwrap();
        m.add_line(v(0.0, 10.0, 0.0), v(0.0, 0.0, 0.0)).unwrap();
        m
    }

    #[test]
    fn picked_edges_and_enter_skin_and_sampling_redoes_it() {
        let m = frame();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = SkinTool::default();
        assert_eq!(t.status(), "Click the Frame's Edges");
        let edges: Vec<EdgeId> = m.edge_ids().collect();
        for e in &edges[..3] {
            let mut inf = at(v(0.0, 0.0, 0.0));
            inf.entity = Some(Pickable::Edge(*e));
            t.handle(Input::Click(inf), &c);
        }
        // Three sides of a square close no cell.
        assert!(matches!(t.handle(Input::Finish, &c), Outcome::Message(_)));
        let mut inf = at(v(0.0, 0.0, 0.0));
        inf.entity = Some(Pickable::Edge(edges[3]));
        t.handle(Input::Click(inf), &c);
        let out = t.handle(Input::Finish, &c);
        let Outcome::Commit(DrawOp::Skin {
            segments, sampling, ..
        }) = out
        else {
            panic!("no skin: {out:?}");
        };
        assert_eq!(segments.len(), 4);
        assert_eq!(sampling, 8);
        let out = t.edit(0, 3.0, &c);
        assert!(matches!(
            out,
            Outcome::Redefine(DrawOp::Skin { sampling: 3, .. })
        ));
        let out = t.edit(1, 1.0, &c);
        assert!(matches!(
            out,
            Outcome::Redefine(DrawOp::Skin {
                edges_only: true,
                ..
            })
        ));
    }

    #[test]
    fn edit_skin_rebuilds_its_group() {
        let m = frame();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = SkinTool::default();
        let mut doc = smith_model::Document::new();
        let def = doc.create_definition("Skin", smith_model::DefinitionKind::Group);
        t.use_sweep(
            def,
            SweepRecipe::Skin {
                segments: vec![[v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0)]],
                sampling: 5,
                edges_only: false,
            },
            DMat4::IDENTITY,
        );
        assert_eq!(t.fields()[0].value, Some(5.0));
        let out = t.edit(0, 2.0, &c);
        assert!(matches!(
            out,
            Outcome::Commit(DrawOp::EditSweep {
                recipe: SweepRecipe::Skin { sampling: 2, .. },
                ..
            })
        ));
    }
}
