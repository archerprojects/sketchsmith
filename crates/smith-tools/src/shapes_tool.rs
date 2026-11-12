//! SketchSmith — Shapes tool | v1.0.0 | Thursday 12 November 2026
//!
//! Click where a shape stands: on a face it stands out of the face, else on
//! the drawing axes. Ctrl (a tap) steps through Box, Cylinder, Cone, Sphere
//! and Torus. Operation Inputs holds the shape's sizes; changing them after
//! placing it rebuilds it in place (one undo step). Each shape is placed as
//! its own group, named after the shape, which remembers its sizes.
//!
//! **Edit Shape** (right-click a shape group, or the Items panel) opens this
//! tool on that group: Operation Inputs shows its sizes, and changing them
//! rebuilds it where it is (each change one undo step). Once the group's
//! geometry is edited by hand, it is no longer a shape and Edit Shape is gone.

use smith_geom::{DMat4, DVec3};
use smith_infer::units::TypedValue;
use smith_ops::draw::DrawOp;
use smith_ops::shapes::ShapeSpec;

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count, face_normal};

/// The shapes, in the order Ctrl steps through them, at their starting
/// sizes (mm).
const SHAPES: [ShapeSpec; 5] = [
    ShapeSpec::Box {
        width: 20.0,
        depth: 20.0,
        height: 20.0,
    },
    ShapeSpec::Cylinder {
        radius: 10.0,
        height: 20.0,
        sides: 24,
    },
    ShapeSpec::Cone {
        radius: 10.0,
        height: 20.0,
        sides: 24,
    },
    ShapeSpec::Sphere {
        radius: 10.0,
        segments: 24,
    },
    ShapeSpec::Torus {
        ring: 15.0,
        tube: 4.0,
        segments: 24,
        sides: 12,
    },
];

/// The Shapes tool.
#[derive(Debug)]
pub struct ShapesTool {
    /// Each shape's current sizes; `current` picks one.
    specs: [ShapeSpec; 5],
    current: usize,
    /// The last shape placed and where, and whether Operation Inputs shows it.
    placed: Option<(DMat4, bool)>,
    /// Edit Shape: the group being edited.
    editing: Option<smith_model::DefinitionId>,
}

impl Default for ShapesTool {
    fn default() -> Self {
        Self {
            specs: SHAPES,
            current: 0,
            placed: None,
            editing: None,
        }
    }
}

impl ShapesTool {
    fn spec(&self) -> ShapeSpec {
        self.specs[self.current]
    }

    /// Rebuilds the shape just placed (or the one being edited) with the
    /// current sizes.
    fn redo(&mut self) -> Outcome {
        if let Some(definition) = self.editing {
            return Outcome::Commit(DrawOp::EditShape {
                definition,
                spec: self.spec(),
            });
        }
        match self.placed {
            Some((transform, true)) => Outcome::Redefine(DrawOp::Shape {
                spec: self.spec(),
                transform,
            }),
            _ => Outcome::Nothing,
        }
    }
}

impl Tool for ShapesTool {
    fn name(&self) -> &'static str {
        "Shapes"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Click(inf) => {
                // A click ends Edit Shape and places a new shape.
                self.editing = None;
                let z = face_normal(&inf, ctx.entities).unwrap_or(ctx.axes.z);
                let x = (ctx.axes.x - z * ctx.axes.x.dot(z))
                    .try_normalize()
                    .unwrap_or_else(|| z.any_orthonormal_vector());
                let y = z.cross(x);
                let transform = DMat4::from_cols(
                    x.extend(0.0),
                    y.extend(0.0),
                    z.extend(0.0),
                    inf.point.extend(1.0),
                );
                self.placed = Some((transform, true));
                Outcome::Commit(DrawOp::Shape {
                    spec: self.spec(),
                    transform,
                })
            }
            Input::Toggle if self.editing.is_none() => {
                self.current = (self.current + 1) % self.specs.len();
                self.placed = None;
                Outcome::Nothing
            }
            Input::Typed(TypedValue::Length(l)) => self.edit(0, l, ctx),
            Input::Typed(TypedValue::Segments(n)) => {
                let sides = self.fields().len() - 1;
                self.edit(sides, f64::from(n), ctx)
            }
            Input::Typed(_) => Outcome::Message("Type a Size".into()),
            Input::Hover(_) | Input::Finish | Input::Toggle => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        self.editing.take().is_some()
    }

    fn use_shape(&mut self, definition: smith_model::DefinitionId, spec: ShapeSpec) {
        let kind = |s: &ShapeSpec| std::mem::discriminant(s);
        if let Some(i) = self.specs.iter().position(|s| kind(s) == kind(&spec)) {
            self.current = i;
            self.specs[i] = spec;
            self.editing = Some(definition);
            self.placed = None;
        }
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
        if self.editing.is_some() {
            return format!(
                "Change the {} in Operation Inputs · Esc Ends",
                self.spec().name()
            );
        }
        format!(
            "Click Where the {} Stands · Ctrl: Next Shape",
            self.spec().name()
        )
    }

    fn fields(&self) -> Vec<Field> {
        let len = |label, v: f64| Field::length(label, Some(v), false, true);
        match self.spec() {
            ShapeSpec::Box {
                width,
                depth,
                height,
            } => vec![
                len("Width", width),
                len("Depth", depth),
                len("Height", height),
            ],
            ShapeSpec::Cylinder {
                radius,
                height,
                sides,
            }
            | ShapeSpec::Cone {
                radius,
                height,
                sides,
            } => vec![
                len("Radius", radius),
                len("Height", height),
                Field::count("Sides", sides, true),
            ],
            ShapeSpec::Sphere { radius, segments } => vec![
                len("Radius", radius),
                Field::count("Segments", segments, true),
            ],
            ShapeSpec::Torus {
                ring,
                tube,
                segments,
                sides,
            } => vec![
                len("Ring Radius", ring),
                len("Tube Radius", tube),
                Field::count("Segments", segments, true),
                Field::count("Sides", sides, true),
            ],
        }
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        if value <= 0.0 || !value.is_finite() {
            return Outcome::Nothing;
        }
        let count = as_count(value).max(3);
        let spec = &mut self.specs[self.current];
        match (spec, index) {
            (ShapeSpec::Box { width, .. }, 0) => *width = value,
            (ShapeSpec::Box { depth, .. }, 1) => *depth = value,
            (ShapeSpec::Box { height, .. }, 2) => *height = value,
            (ShapeSpec::Cylinder { radius, .. } | ShapeSpec::Cone { radius, .. }, 0) => {
                *radius = value
            }
            (ShapeSpec::Cylinder { height, .. } | ShapeSpec::Cone { height, .. }, 1) => {
                *height = value
            }
            (ShapeSpec::Cylinder { sides, .. } | ShapeSpec::Cone { sides, .. }, 2) => {
                *sides = count;
            }
            (ShapeSpec::Sphere { radius, .. }, 0) => *radius = value,
            (ShapeSpec::Sphere { segments, .. }, 1) => *segments = count.max(4),
            (ShapeSpec::Torus { ring, .. }, 0) => *ring = value,
            (ShapeSpec::Torus { tube, .. }, 1) => *tube = value,
            (ShapeSpec::Torus { segments, .. }, 2) => *segments = count,
            (ShapeSpec::Torus { sides, .. }, 3) => *sides = count,
            _ => return Outcome::Nothing,
        }
        self.redo()
    }

    fn accept(&mut self) {
        if let Some((_, shown)) = &mut self.placed {
            *shown = false;
        }
    }

    fn reopen(&mut self) -> bool {
        match &mut self.placed {
            Some((_, shown)) => {
                *shown = true;
                true
            }
            None => false,
        }
    }

    fn forget(&mut self) {
        self.placed = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{at, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    #[test]
    fn a_click_places_a_box_and_a_new_height_rebuilds_it() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = ShapesTool::default();
        let out = t.handle(Input::Click(at(v(5.0, 6.0, 0.0))), &c);
        let Outcome::Commit(DrawOp::Shape { transform, .. }) = out else {
            panic!("no shape: {out:?}");
        };
        assert_eq!(transform.w_axis.truncate(), v(5.0, 6.0, 0.0));
        let out = t.edit(2, 35.0, &c);
        assert!(matches!(
            out,
            Outcome::Redefine(DrawOp::Shape {
                spec: ShapeSpec::Box { height, .. },
                ..
            }) if height == 35.0
        ));
    }

    #[test]
    fn ctrl_steps_through_the_shapes() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = ShapesTool::default();
        for name in ["Cylinder", "Cone", "Sphere", "Torus", "Box"] {
            t.handle(Input::Toggle, &c);
            assert_eq!(t.spec().name(), name);
        }
    }
}
