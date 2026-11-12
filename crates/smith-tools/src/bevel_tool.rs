//! SketchSmith — Bevel tool | v1.0.0 | Thursday 12 November 2026
//!
//! Click edges to bevel (click one again to leave it out), or select them
//! before starting the tool, then press Enter. Operation Inputs: Width (how
//! far the bevel cuts back along each face) and Segments (1 a flat
//! chamfer, more a round fillet). Setting either while edges or corners are
//! picked bevels them at once with it; changing either right after does it
//! again (one undo step). Straight edges between two flat faces only; two
//! bevelled edges meeting at a corner are refused, naming the corner.
//!
//! Click a corner point of a flat outline instead (two edges meeting) and
//! the corner is rounded: the first field reads Radius, the true radius of
//! the round. Edges and corners are bevelled separately, not mixed.

use smith_geom::DVec3;
use smith_model::{EdgeId, Pickable};
use smith_ops::draw::DrawOp;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count};

/// Bevel.
#[derive(Debug)]
pub struct BevelTool {
    /// The edges picked.
    edges: Vec<EdgeId>,
    /// The corners picked (points of flat outlines).
    corners: Vec<DVec3>,
    /// Width, mm.
    width: f64,
    /// Segments: 1 a chamfer, more a fillet.
    segments: u32,
    /// The last bevel made, while it can still be redone.
    made: Option<DrawOp>,
}

impl Default for BevelTool {
    fn default() -> Self {
        Self {
            edges: Vec::new(),
            corners: Vec::new(),
            width: 1.0,
            segments: 1,
            made: None,
        }
    }
}

impl BevelTool {
    /// Corners are picked (or were, in the bevel just made): the value is a
    /// radius.
    fn rounding(&self) -> bool {
        !self.corners.is_empty()
            || matches!(&self.made, Some(DrawOp::Bevel { corners, .. }) if !corners.is_empty())
    }

    /// The last bevel again with the current values.
    fn redo(&mut self) -> Outcome {
        let Some(DrawOp::Bevel { edges, corners, .. }) = self.made.clone() else {
            return Outcome::Nothing;
        };
        let op = DrawOp::Bevel {
            edges,
            corners,
            width: self.width,
            segments: self.segments,
        };
        self.made = Some(op.clone());
        Outcome::Redefine(op)
    }
}

impl Tool for BevelTool {
    fn name(&self) -> &'static str {
        "Bevel"
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
            Input::Click(inf) => {
                // A corner: the click lands on a point where edges meet.
                if let Some(v) = ctx.entities.find_vertex(inf.point) {
                    if !self.edges.is_empty() {
                        return Outcome::Message("Bevel Edges or Corners, Not Both".into());
                    }
                    let at = ctx.entities.vertex(v).map_or(inf.point, |x| x.position());
                    if let Some(i) = self.corners.iter().position(|c| c.distance(at) < 1e-9) {
                        self.corners.remove(i);
                    } else {
                        self.corners.push(at);
                    }
                    self.made = None;
                    return Outcome::Nothing;
                }
                let Some(Pickable::Edge(e)) = inf.entity else {
                    return Outcome::Message("Click an Edge or a Corner".into());
                };
                if !self.corners.is_empty() {
                    return Outcome::Message("Bevel Edges or Corners, Not Both".into());
                }
                if let Some(i) = self.edges.iter().position(|x| *x == e) {
                    self.edges.remove(i);
                } else {
                    self.edges.push(e);
                }
                self.made = None;
                Outcome::Nothing
            }
            Input::Finish => {
                let edges: Vec<(DVec3, DVec3)> = self
                    .edges
                    .iter()
                    .filter_map(|e| ctx.entities.edge_points(*e))
                    .collect();
                if edges.is_empty() && self.corners.is_empty() {
                    return Outcome::Nothing;
                }
                self.edges.clear();
                let op = DrawOp::Bevel {
                    edges,
                    corners: std::mem::take(&mut self.corners),
                    width: self.width,
                    segments: self.segments,
                };
                self.made = Some(op.clone());
                Outcome::Commit(op)
            }
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = !self.edges.is_empty() || !self.corners.is_empty();
        self.edges.clear();
        self.corners.clear();
        busy
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn wants_finish(&self) -> bool {
        !self.edges.is_empty() || !self.corners.is_empty()
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
        match (self.edges.is_empty(), self.corners.is_empty()) {
            (true, true) => "Click Edges or Corners to Bevel".into(),
            (false, _) => "Click More Edges, or Enter Bevels".into(),
            (_, false) => "Click More Corners, or Enter Rounds Them".into(),
        }
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::length(
                if self.rounding() { "Radius" } else { "Width" },
                Some(self.width),
                false,
                true,
            ),
            Field::count("Segments", self.segments, true),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 if value > 0.0 && value.is_finite() => self.width = value,
            1 => self.segments = as_count(value).clamp(1, 64),
            _ => return Outcome::Nothing,
        }
        // Picks waiting: the value set bevels them (Enter went to the field).
        if self.made.is_none() && self.wants_finish() {
            return self.handle(Input::Finish, ctx);
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
    fn picked_edges_and_enter_bevel_and_segments_redo_it() {
        let mut m = Entities::new();
        let e = m.add_line(v(0.0, 0.0, 0.0), v(0.0, 0.0, 10.0)).unwrap()[0];
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = BevelTool::default();
        assert_eq!(t.status(), "Click Edges or Corners to Bevel");
        let mut inf = at(v(0.0, 0.0, 5.0));
        inf.entity = Some(Pickable::Edge(e));
        t.handle(Input::Click(inf), &c);
        assert!(t.wants_finish());
        let out = t.handle(Input::Finish, &c);
        assert!(matches!(out, Outcome::Commit(DrawOp::Bevel { .. })));
        let out = t.edit(1, 4.0, &c);
        assert!(matches!(
            out,
            Outcome::Redefine(DrawOp::Bevel { segments: 4, .. })
        ));
    }

    /// A 10 mm box and one of its edges along Z.
    fn boxed() -> (Entities, EdgeId) {
        let mut m = Entities::new();
        smith_ops::shapes::build_shape(
            &mut m,
            smith_model::ShapeSpec::Box {
                width: 10.0,
                depth: 10.0,
                height: 10.0,
            },
        )
        .unwrap();
        let e = m
            .edge_ids()
            .find(|x| {
                m.edge_points(*x)
                    .is_some_and(|(p, q)| (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9)
            })
            .unwrap();
        (m, e)
    }

    #[test]
    fn an_edge_selected_first_bevels_when_segments_are_set() {
        for (first, then) in [(4, 5), (5, 4)] {
            let (mut m, e) = boxed();
            let axes = DrawingAxes::default();
            let mut t = BevelTool::default();
            t.use_selection(&[Pickable::Edge(e)]);
            // Segments typed in Operation Inputs, Enter taken by the field.
            let out = t.edit(1, f64::from(first), &ctx(&m, &axes));
            let Outcome::Commit(op) = out else {
                panic!("not bevelled: {out:?}");
            };
            assert!(matches!(op, DrawOp::Bevel { segments, .. } if segments == first));
            smith_ops::draw::draw(&mut m, &op).unwrap();
            assert_eq!(m.face_count(), 6 + first as usize);
            // Changed right after: done again.
            let out = t.edit(1, f64::from(then), &ctx(&m, &axes));
            assert!(matches!(
                out,
                Outcome::Redefine(DrawOp::Bevel { segments, .. }) if segments == then
            ));
            assert!(!t.wants_finish());
        }
    }

    #[test]
    fn an_edge_clicked_bevels_when_the_width_is_set() {
        let (m, e) = boxed();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = BevelTool::default();
        let (p, q) = m.edge_points(e).unwrap();
        let mut inf = at((p + q) / 2.0);
        inf.entity = Some(Pickable::Edge(e));
        t.handle(Input::Click(inf), &c);
        let out = t.edit(0, 2.0, &c);
        assert!(matches!(
            out,
            Outcome::Commit(DrawOp::Bevel { width, segments: 1, .. }) if (width - 2.0).abs() < 1e-12
        ));
        // Nothing picked: a value only waits for the next bevel.
        let mut idle = BevelTool::default();
        assert!(matches!(idle.edit(1, 4.0, &c), Outcome::Nothing));
        assert_eq!(idle.segments, 4);
    }
}
