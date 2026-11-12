//! SketchSmith — Extrude Edges along Path tool | v1.0.0 | Thursday 12 November 2026
//!
//! Bare edges carried along a path, making a surface (no caps). Set the
//! counts in Operation Inputs first, and they set the picks: Paths (1 or
//! 2) and Edges (1 or 2: a start profile, or a start and an end). The hints
//! then ask for Path 1, Path 2, Profile A and Profile B in turn, and the
//! surface is made on the last click. Each pick is an edge: its connected
//! chain, open or closed, is taken whole.
//!
//! With two paths the profile is turned and scaled to keep spanning them;
//! with two profiles the shape blends from A to B (points added along the
//! chains to match, never moved). Both need open paths. Vertices, Segments
//! and Twist work as in Loft along Path. Changing a value right after does
//! it again (one undo step). The result is a group named "Extrude" that
//! keeps what it was made from; Edit Extrude reopens it here to change its
//! values.

use smith_geom::{DMat4, DVec3};
use smith_model::{DefinitionId, LoftOptions, Pickable, SweepRecipe};
use smith_ops::draw::DrawOp;
use smith_ops::sweep::edge_chain;

use crate::sweep_tool::SweepTool;
use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count};

/// Extrude Edges along Path.
#[derive(Debug)]
pub struct EdgesTool {
    /// Paths to pick: 1 or 2.
    paths: u32,
    /// Edge profiles to pick: 1 or 2.
    edges: u32,
    /// Path 1: points and whether it closes.
    path: Option<(Vec<DVec3>, bool)>,
    /// Path 2, running the same way as Path 1.
    path2: Option<Vec<DVec3>>,
    /// The profiles picked: chain points and whether each closes.
    profiles: Vec<(Vec<DVec3>, bool)>,
    /// Vertices, Segments and Twist.
    options: LoftOptions,
    /// The last surface made, while it can still be redone.
    made: Option<DrawOp>,
    /// Edit Extrude: the group and its recipe.
    editing: Option<(DefinitionId, SweepRecipe)>,
}

impl Default for EdgesTool {
    fn default() -> Self {
        Self {
            paths: 1,
            edges: 1,
            path: None,
            path2: None,
            profiles: Vec::new(),
            options: LoftOptions::default(),
            made: None,
            editing: None,
        }
    }
}

impl EdgesTool {
    /// Starts the picks again.
    fn restart(&mut self) {
        self.path = None;
        self.path2 = None;
        self.profiles.clear();
    }

    /// The surface for the picks so far, once they are all made.
    fn ready(&mut self, up: DVec3) -> Outcome {
        let Some((path, closed)) = self.path.clone() else {
            return Outcome::Nothing;
        };
        if self.profiles.len() < self.edges as usize {
            return Outcome::Nothing;
        }
        let loops = self.profiles[0].1;
        if self.profiles.iter().any(|(_, c)| *c != loops) {
            self.profiles.pop();
            return Outcome::Message("Both Profiles Open, or Both Closed".into());
        }
        let op = DrawOp::ExtrudeEdges {
            path,
            closed,
            path2: self.path2.clone(),
            profiles: self.profiles.iter().map(|(p, _)| p.clone()).collect(),
            loops,
            up,
            options: self.options,
        };
        self.restart();
        self.made = Some(op.clone());
        Outcome::Commit(op)
    }

    /// The last surface again with the current values, or the edited group
    /// rebuilt with them.
    fn redo(&mut self) -> Outcome {
        if let Some((definition, recipe)) = self.editing.clone() {
            let mut recipe = recipe;
            match &mut recipe {
                SweepRecipe::Sweep { twist, .. } => *twist = self.options.twist,
                SweepRecipe::Loft { options, .. } => *options = self.options,
                SweepRecipe::Revolve { .. } | SweepRecipe::Skin { .. } => {}
            }
            self.editing = Some((definition, recipe.clone()));
            return Outcome::Commit(DrawOp::EditSweep { definition, recipe });
        }
        let Some(DrawOp::ExtrudeEdges {
            path,
            closed,
            path2,
            profiles,
            loops,
            up,
            ..
        }) = self.made.clone()
        else {
            return Outcome::Nothing;
        };
        let op = DrawOp::ExtrudeEdges {
            path,
            closed,
            path2,
            profiles,
            loops,
            up,
            options: self.options,
        };
        self.made = Some(op.clone());
        Outcome::Redefine(op)
    }
}

/// A chain split at its sharpest corner into a path (from the corner) and
/// the profile holding the clicked edge `ends` (also from the corner).
/// `None` when the chain has no corner.
fn split_at_corner(pts: &[DVec3], ends: (DVec3, DVec3)) -> Option<(Vec<DVec3>, Vec<DVec3>)> {
    let turn = |i: usize| {
        let (a, b) = (pts[i] - pts[i - 1], pts[i + 1] - pts[i]);
        a.angle_between(b)
    };
    let corner = (1..pts.len().checked_sub(1)?).max_by(|a, b| turn(*a).total_cmp(&turn(*b)))?;
    // Which side the clicked edge is on.
    let edge_at = (0..pts.len() - 1).find(|j| {
        let (p, q) = (pts[*j], pts[j + 1]);
        (p.distance(ends.0) < 1e-9 && q.distance(ends.1) < 1e-9)
            || (p.distance(ends.1) < 1e-9 && q.distance(ends.0) < 1e-9)
    })?;
    let mut before: Vec<DVec3> = pts[..=corner].to_vec();
    before.reverse();
    let after: Vec<DVec3> = pts[corner..].to_vec();
    Some(if edge_at < corner {
        (after, before)
    } else {
        (before, after)
    })
}

impl Tool for EdgesTool {
    fn name(&self) -> &'static str {
        "Extrude Edges along Path"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        let Input::Click(inf) = input else {
            return Outcome::Nothing;
        };
        if self.editing.is_some() {
            return Outcome::Message("Change It in Operation Inputs".into());
        }
        let Some(Pickable::Edge(e)) = inf.entity else {
            return Outcome::Message("Click an Edge".into());
        };
        self.made = None;
        // Path 1.
        let Some((p1, closed)) = self.path.clone() else {
            let Some(path) = SweepTool::path_from(e, inf.point, ctx) else {
                return Outcome::Nothing;
            };
            if path.1 && (self.paths == 2 || self.edges == 2) {
                return Outcome::Message("Click an Open Path".into());
            }
            self.path = Some(path);
            return Outcome::Nothing;
        };
        // Path 2, running the same way as Path 1.
        if self.paths == 2 && self.path2.is_none() {
            let Some((mut p2, false)) = SweepTool::path_from(e, inf.point, ctx) else {
                return Outcome::Message("Click an Open Path".into());
            };
            if p2[0].distance(p1[0]) > p2[p2.len() - 1].distance(p1[0]) {
                p2.reverse();
            }
            self.path2 = Some(p2);
            return Outcome::Nothing;
        }
        // Profiles A and B.
        let Some(chain) = edge_chain(ctx.entities, e) else {
            return Outcome::Nothing;
        };
        // A profile drawn on from the path's end makes one chain with it:
        // split them at the chain's sharpest corner, the path starting
        // there.
        let same = !closed
            && !chain.1
            && chain.0.len() == p1.len()
            && chain
                .0
                .iter()
                .all(|p| p1.iter().any(|q| q.distance(*p) < 1e-9));
        let chain = if same {
            let Some(ends) = ctx.entities.edge_points(e) else {
                return Outcome::Nothing;
            };
            match split_at_corner(&chain.0, ends) {
                Some((path, profile)) => {
                    self.path = Some((path, false));
                    (profile, false)
                }
                None => return Outcome::Message("Click the Profile's Own Edges".into()),
            }
        } else {
            chain
        };
        self.profiles.push(chain);
        self.ready(ctx.axes.z)
    }

    fn cancel(&mut self) -> bool {
        if self.editing.take().is_some() {
            return true;
        }
        let busy = self.path.is_some();
        self.restart();
        busy
    }

    fn use_sweep(&mut self, definition: DefinitionId, recipe: SweepRecipe, _placed: DMat4) {
        match &recipe {
            SweepRecipe::Sweep { twist, .. } => {
                self.options = LoftOptions {
                    twist: *twist,
                    ..LoftOptions::default()
                };
            }
            SweepRecipe::Loft { options, .. } => self.options = *options,
            SweepRecipe::Revolve { .. } | SweepRecipe::Skin { .. } => return,
        }
        self.restart();
        self.made = None;
        self.editing = Some((definition, recipe));
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        let mut lines: Vec<[DVec3; 2]> = Vec::new();
        if let Some((pts, closed)) = &self.path {
            let n = pts.len();
            let segs = if *closed { n } else { n.saturating_sub(1) };
            lines.extend((0..segs).map(|i| [pts[i], pts[(i + 1) % n]]));
        }
        if let Some(pts) = &self.path2 {
            lines.extend(pts.windows(2).map(|w| [w[0], w[1]]));
        }
        Preview {
            lines,
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.editing.is_some() {
            return "Change It in Operation Inputs · Esc Ends".into();
        }
        match (&self.path, &self.path2, self.profiles.len()) {
            (None, _, _) if self.paths == 2 => "Click Path 1".into(),
            (None, _, _) => "Click the Path".into(),
            (Some(_), None, _) if self.paths == 2 => "Click Path 2".into(),
            (_, _, 0) => "Click Profile A".into(),
            _ => "Click Profile B".into(),
        }
    }

    fn fields(&self) -> Vec<Field> {
        let picking = self.editing.is_none();
        let two = self.edges == 2;
        vec![
            Field::count("Paths", self.paths, picking),
            Field::count("Edges", self.edges, picking),
            Field::count("Vertices", self.options.vertices, two),
            Field::count("Segments", self.options.segments, two),
            Field::number("Twist", Some(self.options.twist.to_degrees()), true),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        match index {
            // Two at most for now; changing a count starts the picks again.
            0 if self.editing.is_none() => {
                self.paths = as_count(value).clamp(1, 2);
                self.restart();
                return Outcome::Nothing;
            }
            1 if self.editing.is_none() => {
                self.edges = as_count(value).clamp(1, 2);
                self.restart();
                return Outcome::Nothing;
            }
            2 => self.options.vertices = as_count(value),
            3 => self.options.segments = as_count(value).min(256),
            4 if value.is_finite() => self.options.twist = value.to_radians(),
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

    fn click(t: &mut EdgesTool, e: smith_model::EdgeId, c: &Ctx<'_>) -> Outcome {
        let mut inf = at(v(0.0, 0.0, 0.0));
        inf.entity = Some(Pickable::Edge(e));
        t.handle(Input::Click(inf), c)
    }

    #[test]
    fn a_profile_drawn_on_from_the_path_end_is_split_from_it() {
        // An L: the path along X, the profile up Y from its start.
        let mut m = Entities::new();
        let path = m.add_line(v(10.0, 0.0, 0.0), v(0.0, 0.0, 0.0)).unwrap()[0];
        let prof = m.add_line(v(0.0, 0.0, 0.0), v(0.0, 5.0, 0.0)).unwrap()[0];
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = EdgesTool::default();
        click(&mut t, path, &c);
        let out = click(&mut t, prof, &c);
        let Outcome::Commit(DrawOp::ExtrudeEdges { path, profiles, .. }) = out else {
            panic!("no surface: {out:?}");
        };
        assert_eq!(path, vec![v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)]);
        assert_eq!(profiles[0], vec![v(0.0, 0.0, 0.0), v(0.0, 5.0, 0.0)]);
    }

    #[test]
    fn the_counts_set_the_picks_and_the_last_click_makes_it() {
        let mut m = Entities::new();
        let path = m.add_line(v(0.0, 0.0, 0.0), v(0.0, 0.0, 20.0)).unwrap()[0];
        let path2 = m.add_line(v(5.0, 0.0, 0.0), v(8.0, 0.0, 20.0)).unwrap()[0];
        let a = m.add_line(v(0.0, 0.0, 0.0), v(5.0, 0.0, 0.0)).unwrap()[0];
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = EdgesTool::default();
        t.edit(0, 2.0, &c);
        assert_eq!(t.status(), "Click Path 1");
        click(&mut t, path, &c);
        assert_eq!(t.status(), "Click Path 2");
        click(&mut t, path2, &c);
        assert_eq!(t.status(), "Click Profile A");
        let out = click(&mut t, a, &c);
        let Outcome::Commit(DrawOp::ExtrudeEdges {
            path2: Some(_),
            profiles,
            loops: false,
            ..
        }) = out
        else {
            panic!("no surface: {out:?}");
        };
        assert_eq!(profiles.len(), 1);
        // Twist right after does it again.
        assert!(matches!(t.edit(4, 10.0, &c), Outcome::Redefine(_)));
    }
}
