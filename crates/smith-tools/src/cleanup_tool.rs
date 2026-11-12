//! SketchSmith — Clean Up tool | v1.0.0 | Thursday 12 November 2026
//!
//! Set the passes and values in Operation Inputs, then press Enter: Clean
//! Up runs on the selection's points, or on everything in the open context
//! when nothing is selected (closed groups are left alone; their contents
//! belong to their own context). Changing a value right after does it
//! again (one undo step). What each pass did appears in the report under
//! Statistics while the tool is active.

use std::collections::HashSet;

use smith_geom::DVec3;
use smith_model::{EdgeId, FaceId, Pickable, VertexId};
use smith_ops::cleanup::{CleanUp, Passes};
use smith_ops::draw::DrawOp;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool};

/// The Clean Up tool.
#[derive(Debug, Default)]
pub struct CleanUpTool {
    /// The values, and the scope once taken.
    clean: CleanUp,
    /// Edges selected when the tool started.
    edges: Vec<EdgeId>,
    /// Faces selected when the tool started.
    faces: Vec<FaceId>,
    /// Vertices selected when the tool started.
    vertices: Vec<VertexId>,
    /// A run was made and can still be redone.
    made: bool,
}

impl CleanUpTool {
    /// The tool with its default passes and values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the selection's points (or everything) and runs.
    fn run(&mut self, ctx: &Ctx<'_>) -> Outcome {
        self.clean.scope = if self.takes_everything() {
            None
        } else {
            let mut points: Vec<DVec3> = Vec::new();
            let mut seen: HashSet<[u64; 3]> = HashSet::new();
            let mut take = |p: DVec3| {
                if seen.insert([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]) {
                    points.push(p);
                }
            };
            for e in &self.edges {
                if let Some((a, b)) = ctx.entities.edge_points(*e) {
                    take(a);
                    take(b);
                }
            }
            for f in &self.faces {
                if let Some(face) = ctx.entities.face(*f) {
                    for p in face.loops().flat_map(|l| ctx.entities.loop_points(l)) {
                        take(p);
                    }
                }
            }
            for v in &self.vertices {
                if let Some(x) = ctx.entities.vertex(*v) {
                    take(x.position());
                }
            }
            Some(points)
        };
        self.made = true;
        Outcome::Commit(DrawOp::CleanUp(self.clean.clone()))
    }

    /// Whether nothing was selected (Clean Up then covers everything).
    fn takes_everything(&self) -> bool {
        self.edges.is_empty() && self.faces.is_empty() && self.vertices.is_empty()
    }

    /// The last run again with the current values.
    fn redo(&mut self) -> Outcome {
        if !self.made {
            return Outcome::Nothing;
        }
        Outcome::Redefine(DrawOp::CleanUp(self.clean.clone()))
    }

    /// The pass switch behind each label.
    fn switch<'a>(passes: &'a mut Passes, label: &str) -> Option<&'a mut bool> {
        Some(match label {
            "Merge by Distance" => &mut passes.merge_by_distance,
            "Remove Zero-Size" => &mut passes.zero_size,
            "Duplicate Faces" => &mut passes.duplicate_faces,
            "Fuse Overlapped Faces" => &mut passes.fuse_overlaps,
            "Merge Coplanar" => &mut passes.merge_coplanar,
            "Stray Edges" => &mut passes.stray_edges,
            "Join Split Edges" => &mut passes.join_split_edges,
            "Reset to Base" => &mut passes.reset_to_base,
            "Purge Unused" => &mut passes.purge_unused,
            _ => return None,
        })
    }
}

impl Tool for CleanUpTool {
    fn name(&self) -> &'static str {
        "Clean Up"
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        self.edges.clear();
        self.faces.clear();
        self.vertices.clear();
        for p in items {
            match p {
                Pickable::Edge(e) => self.edges.push(*e),
                Pickable::Face(f) => self.faces.push(*f),
                Pickable::Vertex(v) => self.vertices.push(*v),
                // Groups and guides are not cleaned from outside.
                _ => {}
            }
        }
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Finish => self.run(ctx),
            Input::Click(_) => Outcome::Message(self.status()),
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = !self.takes_everything();
        self.edges.clear();
        self.faces.clear();
        self.vertices.clear();
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
        if self.takes_everything() {
            "Enter Applies to All Geometry Open".into()
        } else {
            "Enter Applies to the Selection".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        let p = &self.clean.passes;
        vec![
            Field::switch("Merge by Distance", p.merge_by_distance, true),
            Field::length("Merge Distance", Some(self.clean.distance), false, true),
            Field::switch("Remove Zero-Size", p.zero_size, true),
            Field::switch("Duplicate Faces", p.duplicate_faces, true),
            Field::switch("Fuse Overlapped Faces", p.fuse_overlaps, true),
            Field::switch("Merge Coplanar", p.merge_coplanar, true),
            Field::switch("Stray Edges", p.stray_edges, true),
            Field::switch("Join Split Edges", p.join_split_edges, true),
            Field::number("Angle", Some(self.clean.angle.to_degrees()), true),
            Field::switch("Reset to Base", p.reset_to_base, true),
            Field::switch("Purge Unused", p.purge_unused, true),
        ]
    }

    fn primary_field(&self) -> usize {
        1
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        if !value.is_finite() {
            return Outcome::Nothing;
        }
        let Some(label) = self.fields().get(index).map(|f| f.label) else {
            return Outcome::Nothing;
        };
        match label {
            "Merge Distance" if value > 0.0 => self.clean.distance = value,
            "Angle" => self.clean.angle = value.clamp(0.0, 90.0).to_radians(),
            _ => match Self::switch(&mut self.clean.passes, label) {
                Some(on) => *on = value != 0.0,
                None => return Outcome::Nothing,
            },
        }
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
        let mut t = CleanUpTool::new();
        let Outcome::Commit(DrawOp::CleanUp(run)) = t.handle(Input::Finish, &c) else {
            panic!("no clean up");
        };
        assert!(run.scope.is_none());
        let Outcome::Redefine(DrawOp::CleanUp(run)) = t.edit(1, 0.5, &c) else {
            panic!("no redo");
        };
        assert!((run.distance - 0.5).abs() < 1e-12);
        let Outcome::Redefine(DrawOp::CleanUp(run)) = t.edit(9, 1.0, &c) else {
            panic!("no redo");
        };
        assert!(run.passes.reset_to_base);
        assert!(!run.passes.fuse_overlaps);
        let Outcome::Redefine(DrawOp::CleanUp(run)) = t.edit(4, 1.0, &c) else {
            panic!("no redo");
        };
        assert!(run.passes.fuse_overlaps);
    }

    #[test]
    fn a_selection_becomes_its_points() {
        let mut m = Entities::new();
        m.add_polyline(
            &[v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(10.0, 10.0, 0.0)],
            true,
        )
        .unwrap();
        let f = m.face_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = CleanUpTool::new();
        assert_eq!(t.status(), "Enter Applies to All Geometry Open");
        t.use_selection(&[Pickable::Face(f)]);
        assert_eq!(t.status(), "Enter Applies to the Selection");
        let Outcome::Commit(DrawOp::CleanUp(run)) = t.handle(Input::Finish, &c) else {
            panic!("no clean up");
        };
        assert_eq!(run.scope.map(|s| s.len()), Some(3));
    }
}
