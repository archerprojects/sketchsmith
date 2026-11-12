//! SketchSmith — Intersect Faces tool | v1.0.0 | Thursday 12 November 2026
//!
//! Select faces, then press Enter: each selected face is cut against every
//! other, and a cut line goes in wherever two cross (one undo step). With
//! (Operation Inputs) says what they are cut against: each other
//! (Selection), every loose face around them too (Context), and while a
//! group is open, the loose faces outside it as well (Model). Closed
//! groups are never cut against; the canvas fills in which groups are
//! open. What it did appears
//! in the report under Statistics while the tool is active.

use smith_geom::DVec3;
use smith_model::{FaceId, Pickable};
use smith_ops::draw::DrawOp;
use smith_ops::intersect::{IntersectFaces, With};

use crate::{Ctx, Field, Input, Outcome, Preview, Tool};

/// The Intersect Faces tool.
#[derive(Debug, Default)]
pub struct IntersectTool {
    /// What the faces are cut against.
    with: With,
    /// Faces selected when the tool started.
    faces: Vec<FaceId>,
    /// A run was made and can still be redone.
    made: bool,
}

impl IntersectTool {
    /// The tool, cutting the selection.
    pub fn new() -> Self {
        Self::default()
    }

    fn op(&self) -> DrawOp {
        DrawOp::Intersect(IntersectFaces {
            with: self.with,
            faces: self.faces.clone(),
            path: Vec::new(),
        })
    }
}

impl Tool for IntersectTool {
    fn name(&self) -> &'static str {
        "Intersect Faces"
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        self.faces = items
            .iter()
            .filter_map(|p| match p {
                Pickable::Face(f) => Some(*f),
                // Only faces make cut lines; groups are cut from inside.
                _ => None,
            })
            .collect();
    }

    fn handle(&mut self, input: Input, _ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Finish if self.faces.is_empty() => Outcome::Message(self.status()),
            Input::Finish => {
                self.made = true;
                Outcome::Commit(self.op())
            }
            Input::Click(_) => Outcome::Message(self.status()),
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = !self.faces.is_empty();
        self.faces.clear();
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
        if self.faces.is_empty() {
            "Select Faces to Intersect".into()
        } else {
            "Enter Applies to the Selection".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        let index = With::ALL.iter().position(|w| *w == self.with).unwrap_or(0);
        vec![Field::choice("With", &With::NAMES, index, true)]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        if index != 0 || !value.is_finite() {
            return Outcome::Nothing;
        }
        let Some(with) = With::ALL.get(value.max(0.0) as usize) else {
            return Outcome::Nothing;
        };
        self.with = *with;
        if self.made {
            Outcome::Redefine(self.op())
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
    fn nothing_selected_asks_for_faces() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = IntersectTool::new();
        assert_eq!(t.status(), "Select Faces to Intersect");
        assert!(matches!(t.handle(Input::Finish, &c), Outcome::Message(_)));
        assert_eq!(t.fields()[0].label, "With");
    }

    #[test]
    fn enter_cuts_the_selected_faces() {
        let mut m = Entities::new();
        m.add_polyline(
            &[v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(10.0, 10.0, 0.0)],
            true,
        )
        .unwrap();
        let f = m.face_ids().next().unwrap();
        let e = m.edge_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = IntersectTool::new();
        t.use_selection(&[Pickable::Face(f), Pickable::Edge(e)]);
        assert_eq!(t.status(), "Enter Applies to the Selection");
        let Outcome::Commit(DrawOp::Intersect(run)) = t.handle(Input::Finish, &c) else {
            panic!("no intersect");
        };
        assert_eq!(run.faces, [f]);
        assert_eq!(run.with, With::Selection);
        let Outcome::Redefine(DrawOp::Intersect(run)) = t.edit(0, 2.0, &c) else {
            panic!("no redo");
        };
        assert_eq!(run.with, With::Model);
        assert_eq!(t.fields()[0].value, Some(2.0));
    }
}
