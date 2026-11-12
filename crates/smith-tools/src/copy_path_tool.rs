//! SketchSmith — Copy Along Path tool | v1.0.0 | Thursday 12 November 2026
//!
//! With one group or component selected, click the path (an edge chain or
//! curve, open or closed): copies of it, as instances of its definition,
//! are spread along the path, each carried and turned by the path's frames
//! as a sweep's profile is. The original stays where it is; it is the copy
//! at the path's start. Loose geometry is refused: group it first.
//!
//! Operation Inputs: Count (copies spread evenly to the path's end; type
//! `5x` or a number), or Spacing (a copy every so far along it; type a
//! length). Changing either right after does it again (one undo step).

use smith_geom::DVec3;
use smith_infer::units::TypedValue;
use smith_model::{InstanceId, Pickable};
use smith_ops::draw::DrawOp;

use crate::sweep_tool::SweepTool;
use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count};

/// Copy Along Path.
#[derive(Debug)]
pub struct CopyAlongPathTool {
    /// The one group or component selected.
    instance: Option<InstanceId>,
    /// The selection was something else (loose geometry, several items).
    not_one: bool,
    /// Copies spread evenly to the end (with no spacing).
    count: u32,
    /// A copy every this far, mm (0: use the count).
    spacing: f64,
    /// The path picked: points, closed.
    path: Option<(Vec<DVec3>, bool)>,
    /// The last op made, while it can still be redone.
    made: Option<DrawOp>,
}

impl Default for CopyAlongPathTool {
    fn default() -> Self {
        Self {
            instance: None,
            not_one: false,
            count: 4,
            spacing: 0.0,
            path: None,
            made: None,
        }
    }
}

impl CopyAlongPathTool {
    /// The copies for the current path and values.
    fn op(&self, up: DVec3) -> Option<DrawOp> {
        let (path, closed) = self.path.clone()?;
        Some(DrawOp::CopyAlongPath {
            instance: self.instance?,
            path,
            closed,
            count: self.count,
            spacing: self.spacing,
            up,
        })
    }

    /// The last copies done again with the current values.
    fn redo(&mut self) -> Outcome {
        let Some(DrawOp::CopyAlongPath { up, .. }) = self.made else {
            return Outcome::Nothing;
        };
        match self.op(up) {
            Some(op) => {
                self.made = Some(op.clone());
                Outcome::Redefine(op)
            }
            None => Outcome::Nothing,
        }
    }
}

impl Tool for CopyAlongPathTool {
    fn name(&self) -> &'static str {
        "Copy Along Path"
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        match items {
            [Pickable::Instance(i)] => {
                self.instance = Some(*i);
                self.not_one = false;
            }
            [] => {
                self.instance = None;
                self.not_one = false;
            }
            _ => {
                self.instance = None;
                self.not_one = true;
            }
        }
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Click(inf) => {
                if self.instance.is_none() {
                    return Outcome::Message(self.status());
                }
                let Some(Pickable::Edge(e)) = inf.entity else {
                    return Outcome::Message("Click the Path".into());
                };
                self.path = SweepTool::path_from(e, inf.point, ctx);
                match self.op(ctx.axes.z) {
                    Some(op) => {
                        self.made = Some(op.clone());
                        Outcome::Commit(op)
                    }
                    None => Outcome::Nothing,
                }
            }
            Input::Typed(TypedValue::Copies(n) | TypedValue::Divide(n)) => {
                self.count = n.max(1);
                self.spacing = 0.0;
                self.redo()
            }
            Input::Typed(TypedValue::Length(d)) if d > 0.0 => {
                self.spacing = d;
                self.redo()
            }
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = self.path.is_some();
        self.path = None;
        self.made = None;
        busy
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        let lines = self
            .path
            .as_ref()
            .map(|(pts, closed)| {
                let n = pts.len();
                let segs = if *closed { n } else { n.saturating_sub(1) };
                (0..segs).map(|i| [pts[i], pts[(i + 1) % n]]).collect()
            })
            .unwrap_or_default();
        Preview {
            lines,
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.not_one {
            return "Select One Group or Component (Group Loose Geometry First)".into();
        }
        if self.instance.is_none() {
            return "Select One Group or Component First".into();
        }
        if self.made.is_some() {
            return "Type a Count or Spacing to Change It · Click Another Path".into();
        }
        "Click the Path".into()
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::count("Count", self.count, true),
            Field::length(
                "Spacing",
                (self.spacing > 0.0).then_some(self.spacing),
                false,
                true,
            ),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 => {
                self.count = as_count(value).max(1);
                self.spacing = 0.0;
            }
            1 => self.spacing = value.max(0.0),
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
    use smith_model::{DefinitionKind, Document, Entities};

    #[test]
    fn loose_geometry_is_refused() {
        let mut m = Entities::new();
        let e = m.add_line(v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0)).unwrap()[0];
        let mut t = CopyAlongPathTool::default();
        t.use_selection(&[Pickable::Edge(e)]);
        assert!(t.status().starts_with("Select One Group or Component"));
    }

    #[test]
    fn a_selected_group_and_a_path_make_copies_and_a_count_redoes_them() {
        let mut doc = Document::new();
        let def = doc.create_definition("Box", DefinitionKind::Group);
        let inst = doc
            .add_instance(None, def, smith_geom::DMat4::IDENTITY, "")
            .unwrap();
        let mut m = Entities::new();
        m.add_polyline(&[v(0.0, 0.0, 0.0), v(30.0, 0.0, 0.0)], false)
            .unwrap();
        let e = m.edge_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = CopyAlongPathTool::default();
        t.use_selection(&[Pickable::Instance(inst)]);
        assert_eq!(t.status(), "Click the Path");
        let mut inf = at(v(1.0, 0.0, 0.0));
        inf.entity = Some(Pickable::Edge(e));
        let out = t.handle(Input::Click(inf), &c);
        assert!(matches!(
            out,
            Outcome::Commit(DrawOp::CopyAlongPath { count: 4, .. })
        ));
        let out = t.handle(Input::Typed(TypedValue::Copies(6)), &c);
        assert!(matches!(
            out,
            Outcome::Redefine(DrawOp::CopyAlongPath { count: 6, .. })
        ));
        let out = t.edit(1, 5.0, &c);
        let Outcome::Redefine(DrawOp::CopyAlongPath { spacing, .. }) = out else {
            panic!("no spacing: {out:?}");
        };
        assert!((spacing - 5.0).abs() < 1e-12);
    }
}
