//! SketchSmith — Extrude along Path and Revolve tools | v1.0.0 | Thursday 12 November 2026
//!
//! **Extrude along Path:** click the profile face and the path, in either order; or click the
//! path and type a tube: `2r` (round, radius 2), `24s` (its sides, set
//! first or after), or `w,h` (rectangular). A typed tube starts at the path
//! end nearest the click; a face profile must lie square to the path at
//! one of its ends, which becomes the start — or, with Align to Path, it is
//! placed square at the start wherever it is. The sweep is one undo step.
//!
//! Operation Inputs: Sides, Taper (end scale), Twist (degrees), Upright,
//! Align to Path, and Paths: with 2, the hints ask for Path 2 after the
//! profile and Path 1, and the profile is turned and scaled to keep
//! spanning the two (both open). Changing any of them after an extrusion does it again
//! with the new values (one undo step).
//!
//! **Revolve:** click the profile face, then an edge whose line is the
//! axis. Operation Inputs: Sides (steps round) and Angle.
//!
//! After a sweep, the status warns when the profile is wider than the
//! path's tightest bend, where the sides cross.
//!
//! **Edit Extrude** and **Edit Revolve** reopen such a group: Operation Inputs shows
//! what it was made with, and each change rebuilds the group (one undo step
//! each). Clicking an edge gives a sweep a new path (a revolve a new axis),
//! clicking a face gives it a new profile, and typing a tube replaces a
//! sweep's profile. Esc ends it.

use smith_geom::{DMat4, DVec3};
use smith_infer::units::TypedValue;
use smith_model::{DefinitionId, FaceId, Pickable, SweepRecipe, SweepSection};
use smith_ops::draw::DrawOp;
use smith_ops::sweep::{SweepOptions, SweepProfile, edge_chain, profile_outline};

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count};

/// Extrude along Path, or Revolve (the same engine and options).
#[derive(Debug)]
pub struct SweepTool {
    profile: Option<FaceId>,
    /// The path's points (start first) and whether it closes.
    path: Option<(Vec<DVec3>, bool)>,
    /// Sides of a typed round tube, or a revolve's steps round.
    sides: u32,
    options: SweepOptions,
    revolve: bool,
    /// Revolve angle, radians.
    angle: f64,
    /// The last sweep or revolve, for changing its options afterwards, and
    /// whether Operation Inputs still shows it.
    made: Option<(DrawOp, bool)>,
    /// The last face sweep's profile was wider than the tightest bend.
    tight: bool,
    /// Edit Sweep: the group being edited.
    editing: Option<Editing>,
    /// Paths: 1, or 2 for a second path the profile keeps spanning.
    paths: u32,
    /// With two paths: the extrusion waiting for Path 2.
    pending: Option<DrawOp>,
}

/// A Sweep or Revolve group reopened by Edit Sweep.
#[derive(Debug, Clone)]
struct Editing {
    definition: DefinitionId,
    /// What it is made from now, in the group's coordinates.
    recipe: SweepRecipe,
    /// Context to group coordinates.
    to_group: DMat4,
}

impl Default for SweepTool {
    fn default() -> Self {
        Self {
            profile: None,
            path: None,
            sides: 24,
            options: SweepOptions::default(),
            revolve: false,
            angle: std::f64::consts::TAU,
            made: None,
            tight: false,
            editing: None,
            paths: 1,
            pending: None,
        }
    }
}

impl SweepTool {
    /// Revolve: a face turned about an axis.
    pub fn revolve() -> Self {
        Self {
            revolve: true,
            ..Self::default()
        }
    }

    /// The path through `edge`, starting at the end nearest `near` (an
    /// open path only).
    pub(crate) fn path_from(
        edge: smith_model::EdgeId,
        near: DVec3,
        ctx: &Ctx<'_>,
    ) -> Option<(Vec<DVec3>, bool)> {
        let (mut pts, closed) = edge_chain(ctx.entities, edge)?;
        if !closed && pts.last()?.distance(near) < pts.first()?.distance(near) {
            pts.reverse();
        }
        Some((pts, closed))
    }

    /// For a face profile: an open path turned to start at the end nearer
    /// the face.
    fn start_at_face(&mut self, face: FaceId, ctx: &Ctx<'_>) {
        let Some(plane) = ctx.entities.face(face).map(|f| *f.plane()) else {
            return;
        };
        if let Some((pts, false)) = &mut self.path
            && let (Some(first), Some(last)) = (pts.first(), pts.last())
            && plane.signed_distance(*last).abs() < plane.signed_distance(*first).abs()
        {
            pts.reverse();
        }
    }

    /// Whether `face` is wider (from the path's start) than the path's
    /// tightest bend, where the sides would cross.
    fn too_tight(&self, face: FaceId, ctx: &Ctx<'_>) -> bool {
        let (Some((pts, closed)), Some(f)) = (&self.path, ctx.entities.face(face)) else {
            return false;
        };
        let width = ctx
            .entities
            .loop_vertices(f.outer())
            .into_iter()
            .filter_map(|v| ctx.entities.vertex(v).map(|x| x.position()))
            .map(|p| p.distance(pts[0]))
            .fold(0.0, f64::max);
        tightest_bend(pts, *closed).is_some_and(|r| width > r)
    }

    fn commit(&mut self, op: DrawOp) -> Outcome {
        // Two paths: the extrusion waits for Path 2.
        if self.paths == 2 && matches!(op, DrawOp::Sweep { path2: None, .. }) {
            self.pending = Some(op);
            return Outcome::Nothing;
        }
        if !matches!(op, DrawOp::Sweep { .. }) {
            self.tight = false;
        }
        self.path = None;
        self.profile = None;
        self.made = Some((op.clone(), true));
        Outcome::Commit(op)
    }

    fn sweep_op(
        &self,
        path: Vec<DVec3>,
        closed: bool,
        profile: SweepProfile,
        ctx: &Ctx<'_>,
    ) -> DrawOp {
        DrawOp::Sweep {
            path,
            closed,
            path2: None,
            profile,
            up: ctx.axes.z,
            options: self.options,
        }
    }

    /// The last sweep or revolve again with the current options.
    fn redo(&mut self) -> Outcome {
        let Some((op, true)) = self.made.clone() else {
            return Outcome::Nothing;
        };
        let op = match op {
            DrawOp::Sweep {
                path,
                closed,
                path2,
                profile,
                up,
                ..
            } => DrawOp::Sweep {
                path,
                closed,
                path2,
                profile: match profile {
                    SweepProfile::Round { radius, .. } => SweepProfile::Round {
                        radius,
                        sides: self.sides,
                    },
                    other => other,
                },
                up,
                options: self.options,
            },
            DrawOp::Revolve {
                face,
                axis_point,
                axis_dir,
                ..
            } => DrawOp::Revolve {
                face,
                axis_point,
                axis_dir,
                angle: self.angle,
                steps: self.sides,
            },
            other => other,
        };
        self.made = Some((op.clone(), true));
        Outcome::Redefine(op)
    }
}

impl SweepTool {
    /// The group being edited, rebuilt from its recipe with `change`
    /// applied.
    fn rebuild(&mut self, change: impl FnOnce(&mut SweepRecipe)) -> Outcome {
        let Some(ed) = self.editing.as_mut() else {
            return Outcome::Nothing;
        };
        change(&mut ed.recipe);
        Outcome::Commit(DrawOp::EditSweep {
            definition: ed.definition,
            recipe: ed.recipe.clone(),
        })
    }

    /// The group being edited, rebuilt with the current Operation Inputs
    /// values.
    fn rebuild_values(&mut self) -> Outcome {
        let Some(mut recipe) = self.editing.as_ref().map(|e| e.recipe.clone()) else {
            return Outcome::Nothing;
        };
        self.apply_values(&mut recipe);
        self.rebuild(|r| *r = recipe)
    }

    /// The current Operation Inputs values written into the recipe being
    /// edited.
    fn apply_values(&self, recipe: &mut SweepRecipe) {
        match recipe {
            SweepRecipe::Sweep {
                section,
                taper,
                twist,
                upright,
                align,
                ..
            } => {
                if let SweepSection::Round { sides, .. } = section {
                    *sides = self.sides;
                }
                *taper = self.options.taper;
                *twist = self.options.twist;
                *upright = self.options.upright;
                *align = self.options.align;
            }
            SweepRecipe::Revolve { angle, steps, .. } => {
                *angle = self.angle;
                *steps = self.sides;
            }
            SweepRecipe::Loft { .. } | SweepRecipe::Skin { .. } => {}
        }
    }

    /// A click while editing: a new path or axis (an edge) or a new profile
    /// (a face), taken into the group's coordinates.
    fn edit_click(&mut self, entity: Option<Pickable>, near: DVec3, ctx: &Ctx<'_>) -> Outcome {
        let Some(m) = self.editing.as_ref().map(|e| e.to_group) else {
            return Outcome::Nothing;
        };
        let local = |p: &DVec3| m.transform_point3(*p);
        match entity {
            Some(Pickable::Face(f)) => {
                let Ok(pts) = profile_outline(ctx.entities, f) else {
                    return Outcome::Message("Click a Face With No Holes".into());
                };
                let outline: Vec<DVec3> = pts.iter().map(local).collect();
                self.rebuild(|r| match r {
                    SweepRecipe::Sweep {
                        path,
                        closed,
                        section,
                        ..
                    } => {
                        if !*closed {
                            start_near(path, &outline);
                        }
                        *section = SweepSection::Outline(outline);
                    }
                    SweepRecipe::Revolve { outline: o, .. } => *o = outline,
                    SweepRecipe::Loft { .. } | SweepRecipe::Skin { .. } => {}
                })
            }
            Some(Pickable::Edge(e)) => {
                let revolve = matches!(
                    self.editing.as_ref().map(|x| &x.recipe),
                    Some(SweepRecipe::Revolve { .. })
                );
                if revolve {
                    let Some((a, b)) = ctx.entities.edge_points(e) else {
                        return Outcome::Nothing;
                    };
                    let (a, b) = (local(&a), local(&b));
                    return self.rebuild(|r| {
                        if let SweepRecipe::Revolve {
                            axis_point,
                            axis_dir,
                            ..
                        } = r
                        {
                            *axis_point = a;
                            *axis_dir = b - a;
                        }
                    });
                }
                let Some((pts, is_closed)) = Self::path_from(e, near, ctx) else {
                    return Outcome::Nothing;
                };
                let mut new_path: Vec<DVec3> = pts.iter().map(local).collect();
                self.rebuild(|r| {
                    if let SweepRecipe::Sweep {
                        path,
                        closed,
                        section,
                        ..
                    } = r
                    {
                        if let (SweepSection::Outline(o), false) = (&*section, is_closed) {
                            start_near(&mut new_path, o);
                        }
                        *path = new_path;
                        *closed = is_closed;
                    }
                })
            }
            _ => Outcome::Message("Click a New Path Edge or Profile Face".into()),
        }
    }
}

/// Turns an open `path` to start at the end nearer the plane of `outline`.
fn start_near(path: &mut [DVec3], outline: &[DVec3]) {
    let Ok(plane) = smith_geom::plane::Plane::fit_loop(outline) else {
        return;
    };
    if let (Some(first), Some(last)) = (path.first(), path.last())
        && plane.signed_distance(*last).abs() < plane.signed_distance(*first).abs()
    {
        path.reverse();
    }
}

/// The radius of the tightest bend along `points`: at each joint, half the
/// shorter neighbouring segment over the tangent of half its turn.
fn tightest_bend(points: &[DVec3], closed: bool) -> Option<f64> {
    let n = points.len();
    let joints: Vec<usize> = if closed {
        (0..n).collect()
    } else {
        (1..n.saturating_sub(1)).collect()
    };
    joints
        .into_iter()
        .filter_map(|j| {
            let (a, b, c) = (points[(j + n - 1) % n], points[j], points[(j + 1) % n]);
            let (d1, d2) = ((b - a).try_normalize()?, (c - b).try_normalize()?);
            let turn = d1.dot(d2).clamp(-1.0, 1.0).acos();
            (turn > 1e-6).then(|| {
                let half = a.distance(b).min(b.distance(c)) * 0.5;
                half / (turn * 0.5).tan()
            })
        })
        .reduce(f64::min)
}

impl Tool for SweepTool {
    fn name(&self) -> &'static str {
        if self.revolve {
            "Revolve"
        } else {
            "Extrude along Path"
        }
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(_) | Input::Toggle | Input::Finish => Outcome::Nothing,
            Input::Click(inf) if self.editing.is_some() => {
                self.edit_click(inf.entity, inf.point, ctx)
            }
            Input::Click(inf) if self.pending.is_some() => {
                // Path 2.
                let Some(Pickable::Edge(e)) = inf.entity else {
                    return Outcome::Message("Click Path 2".into());
                };
                let Some((p2, false)) = Self::path_from(e, inf.point, ctx) else {
                    return Outcome::Message("Click an Open Path".into());
                };
                let Some(DrawOp::Sweep {
                    path,
                    closed,
                    profile,
                    up,
                    options,
                    ..
                }) = self.pending.take()
                else {
                    return Outcome::Nothing;
                };
                // Path 2 runs the same way as Path 1.
                let mut p2 = p2;
                if p2[0].distance(path[0]) > p2[p2.len() - 1].distance(path[0]) {
                    p2.reverse();
                }
                self.commit(DrawOp::Sweep {
                    path,
                    closed,
                    path2: Some(p2),
                    profile,
                    up,
                    options,
                })
            }
            Input::Click(inf) => {
                if let Some((_, shown)) = &mut self.made {
                    *shown = false;
                }
                // Revolve: the face, then the axis edge.
                if self.revolve {
                    return match (inf.entity, self.profile) {
                        (Some(Pickable::Face(f)), _) => {
                            self.profile = Some(f);
                            Outcome::Nothing
                        }
                        (Some(Pickable::Edge(e)), Some(face)) => {
                            let Some((a, b)) = ctx.entities.edge_points(e) else {
                                return Outcome::Nothing;
                            };
                            self.commit(DrawOp::Revolve {
                                face,
                                axis_point: a,
                                axis_dir: b - a,
                                angle: self.angle,
                                steps: self.sides,
                            })
                        }
                        _ => Outcome::Message("Click the Profile Face, Then the Axis Edge".into()),
                    };
                }
                match (inf.entity, self.profile, self.path.is_some()) {
                    (Some(Pickable::Face(f)), None, false) => {
                        self.profile = Some(f);
                        Outcome::Nothing
                    }
                    (Some(Pickable::Face(f)), None, true) => {
                        self.start_at_face(f, ctx);
                        let Some((path, closed)) = self.path.clone() else {
                            return Outcome::Nothing;
                        };
                        self.tight = self.too_tight(f, ctx);
                        let op = self.sweep_op(path, closed, SweepProfile::Face(f), ctx);
                        self.commit(op)
                    }
                    (Some(Pickable::Edge(e)), profile, false) => {
                        self.path = Self::path_from(e, inf.point, ctx);
                        match profile {
                            Some(f) => {
                                self.start_at_face(f, ctx);
                                let Some((path, closed)) = self.path.clone() else {
                                    return Outcome::Nothing;
                                };
                                self.tight = self.too_tight(f, ctx);
                                let op = self.sweep_op(path, closed, SweepProfile::Face(f), ctx);
                                self.commit(op)
                            }
                            None => Outcome::Nothing,
                        }
                    }
                    _ => Outcome::Message("Click a Profile Face or a Path Edge".into()),
                }
            }
            Input::Typed(TypedValue::Segments(n)) => {
                self.sides = n.max(3);
                if self.editing.is_some() {
                    return self.rebuild_values();
                }
                self.redo()
            }
            Input::Typed(TypedValue::Radius(r) | TypedValue::Length(r))
                if self.editing.is_some() =>
            {
                let sides = self.sides;
                self.rebuild(|rc| {
                    if let SweepRecipe::Sweep { section, .. } = rc {
                        *section = SweepSection::Round { radius: r, sides };
                    }
                })
            }
            Input::Typed(TypedValue::Pair(w, h)) if self.editing.is_some() => self.rebuild(|rc| {
                if let SweepRecipe::Sweep { section, .. } = rc {
                    *section = SweepSection::Rect {
                        width: w,
                        height: h,
                    };
                }
            }),
            Input::Typed(TypedValue::Radius(r) | TypedValue::Length(r)) if self.path.is_some() => {
                let Some((path, closed)) = self.path.clone() else {
                    return Outcome::Nothing;
                };
                self.tight = tightest_bend(&path, closed).is_some_and(|b| r > b);
                let op = self.sweep_op(
                    path,
                    closed,
                    SweepProfile::Round {
                        radius: r,
                        sides: self.sides,
                    },
                    ctx,
                );
                self.commit(op)
            }
            Input::Typed(TypedValue::Pair(w, h)) if self.path.is_some() => {
                let Some((path, closed)) = self.path.clone() else {
                    return Outcome::Nothing;
                };
                self.tight = tightest_bend(&path, closed).is_some_and(|b| w.hypot(h) * 0.5 > b);
                let op = self.sweep_op(
                    path,
                    closed,
                    SweepProfile::Rect {
                        width: w,
                        height: h,
                    },
                    ctx,
                );
                self.commit(op)
            }
            Input::Typed(_) => Outcome::Message("Click the Path, Then Type 2r, 24s or w,h".into()),
        }
    }

    fn cancel(&mut self) -> bool {
        if self.editing.take().is_some() {
            return true;
        }
        if self.pending.take().is_some() {
            return true;
        }
        let busy = self.profile.is_some() || self.path.is_some();
        self.profile = None;
        self.path = None;
        busy
    }

    fn use_sweep(&mut self, definition: DefinitionId, recipe: SweepRecipe, placed: DMat4) {
        match &recipe {
            // A loft is edited by Loft along Path, a skin by Skinning.
            SweepRecipe::Loft { .. } | SweepRecipe::Skin { .. } => return,
            SweepRecipe::Sweep {
                section,
                taper,
                twist,
                upright,
                align,
                ..
            } => {
                if let SweepSection::Round { sides, .. } = section {
                    self.sides = *sides;
                }
                self.options = SweepOptions {
                    taper: *taper,
                    twist: *twist,
                    upright: *upright,
                    align: *align,
                };
                self.revolve = false;
            }
            SweepRecipe::Revolve { angle, steps, .. } => {
                self.angle = *angle;
                self.sides = *steps;
                self.revolve = true;
            }
        }
        self.profile = None;
        self.path = None;
        self.made = None;
        self.tight = false;
        self.editing = Some(Editing {
            definition,
            recipe,
            to_group: placed.inverse(),
        });
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn warns(&self) -> bool {
        // The profile is wider than the path's tightest bend: its sides
        // cross there. Red while Sides, a typed tube or a new path can still
        // fix it.
        self.tight && matches!(self.made, Some((_, true)))
    }

    fn pauses(&self) -> bool {
        false
    }

    /// The chosen path, traced.
    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        // While editing, the group's own path, placed where the group is.
        let edited = self.editing.as_ref().and_then(|e| match &e.recipe {
            SweepRecipe::Sweep { path, closed, .. } => {
                let back = e.to_group.inverse();
                Some((
                    path.iter().map(|p| back.transform_point3(*p)).collect(),
                    *closed,
                ))
            }
            SweepRecipe::Revolve { .. } | SweepRecipe::Loft { .. } | SweepRecipe::Skin { .. } => {
                None
            }
        });
        // Just made, too tight a bend: its path, shown in warning red.
        let made = match &self.made {
            Some((DrawOp::Sweep { path, closed, .. }, true)) if self.tight => {
                Some((path.clone(), *closed))
            }
            _ => None,
        };
        let lines = edited
            .as_ref()
            .or(self.path.as_ref())
            .or(made.as_ref())
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
        match self.editing.as_ref().map(|e| &e.recipe) {
            Some(SweepRecipe::Sweep { .. }) => {
                return "Change It in Operation Inputs · Click a New Path or Profile Face · Esc Ends".into();
            }
            Some(SweepRecipe::Revolve { .. }) => {
                return "Change It in Operation Inputs · Click a New Profile Face or Axis Edge · Esc Ends".into();
            }
            Some(SweepRecipe::Loft { .. } | SweepRecipe::Skin { .. }) | None => {}
        }
        if self.pending.is_some() {
            return "Click Path 2".into();
        }
        if self.tight && matches!(self.made, Some((_, true))) {
            return "Wider Than the Tightest Bend: the Sides Cross · Undo or Change It".into();
        }
        if self.revolve {
            return match self.profile {
                None => "Click the Profile Face".into(),
                Some(_) => "Click an Edge on the Axis".into(),
            };
        }
        match (self.profile.is_some(), self.path.is_some()) {
            (false, false) => "Click the Profile Face, or the Path".into(),
            (true, false) => "Click the Path".into(),
            _ => "Click the Profile Face, or Type a Tube: 2r, 24s or w,h".into(),
        }
    }

    fn fields(&self) -> Vec<Field> {
        if self.revolve {
            return vec![
                Field::count("Sides", self.sides, true),
                Field::number("Angle", Some(self.angle.to_degrees()), true),
            ];
        }
        vec![
            Field::count("Sides", self.sides, true),
            Field::number("Taper", Some(self.options.taper), true),
            Field::number("Twist", Some(self.options.twist.to_degrees()), true),
            Field::switch("Upright", self.options.upright, true),
            Field::switch("Align to Path", self.options.align, true),
            Field::count("Paths", self.paths, self.editing.is_none()),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        // Revolve's fields are Sides and Angle.
        let index = match (self.revolve, index) {
            (true, 1) => 6,
            (true, i) if i > 1 => return Outcome::Nothing,
            (_, i) => i,
        };
        match index {
            0 => self.sides = as_count(value).max(3),
            1 if value > 0.0 => self.options.taper = value,
            2 => self.options.twist = value.to_radians(),
            3 => self.options.upright = value != 0.0,
            4 => self.options.align = value != 0.0,
            // Two at most for now.
            5 if self.editing.is_none() => {
                self.paths = as_count(value).clamp(1, 2);
                self.pending = None;
                return Outcome::Nothing;
            }
            6 if value != 0.0 => {
                self.angle = value
                    .to_radians()
                    .clamp(-std::f64::consts::TAU, std::f64::consts::TAU)
            }
            _ => return Outcome::Nothing,
        }
        if self.editing.is_some() {
            return self.rebuild_values();
        }
        self.redo()
    }

    fn accept(&mut self) {
        if let Some((_, shown)) = &mut self.made {
            *shown = false;
        }
    }

    fn reopen(&mut self) -> bool {
        match &mut self.made {
            Some((_, shown)) => {
                *shown = true;
                true
            }
            None => false,
        }
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
    fn a_path_then_a_typed_radius_sweeps_a_round_tube_and_twist_redoes_it() {
        let mut m = Entities::new();
        m.add_polyline(
            &[v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(10.0, 10.0, 0.0)],
            false,
        )
        .unwrap();
        let e = m.edge_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = SweepTool::default();
        t.handle(Input::Typed(TypedValue::Segments(8)), &c);
        let mut inf = at(v(9.0, 0.0, 0.0));
        inf.entity = Some(Pickable::Edge(e));
        t.handle(Input::Click(inf), &c);
        let out = t.handle(Input::Typed(TypedValue::Radius(0.5)), &c);
        let Outcome::Commit(DrawOp::Sweep { path, profile, .. }) = out else {
            panic!("no sweep: {out:?}");
        };
        assert_eq!(path.len(), 3);
        assert_eq!(
            profile,
            SweepProfile::Round {
                radius: 0.5,
                sides: 8
            }
        );
        // Twist set afterwards: the same sweep again, twisted.
        let out = t.edit(2, 45.0, &c);
        let Outcome::Redefine(DrawOp::Sweep { options, .. }) = out else {
            panic!("not redone: {out:?}");
        };
        assert!((options.twist - 45f64.to_radians()).abs() < 1e-12);
    }

    #[test]
    fn edit_sweep_rebuilds_its_group_with_new_values_a_new_path_and_a_typed_tube() {
        let mut m = Entities::new();
        m.add_polyline(&[v(0.0, 0.0, 0.0), v(0.0, 0.0, 20.0)], false)
            .unwrap();
        let e = m.edge_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = SweepTool::default();
        let recipe = SweepRecipe::Sweep {
            path: vec![v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)],
            closed: false,
            path2: None,
            section: SweepSection::Round {
                radius: 1.0,
                sides: 12,
            },
            up: v(0.0, 0.0, 1.0),
            taper: 1.0,
            twist: 0.0,
            upright: false,
            align: false,
        };
        let def = smith_model::Document::new()
            .create_definition("Sweep", smith_model::DefinitionKind::Group);
        // The group sits 5 mm along X.
        let placed = DMat4::from_translation(v(5.0, 0.0, 0.0));
        t.use_sweep(def, recipe, placed);
        assert!(t.status().starts_with("Change It"));
        // Taper: rebuilt, not redone.
        let out = t.edit(1, 0.5, &c);
        let Outcome::Commit(DrawOp::EditSweep {
            recipe: SweepRecipe::Sweep { taper, .. },
            ..
        }) = out
        else {
            panic!("not rebuilt: {out:?}");
        };
        assert!((taper - 0.5).abs() < 1e-12);
        // A new path, taken into the group's coordinates.
        let mut inf = at(v(0.0, 0.0, 1.0));
        inf.entity = Some(Pickable::Edge(e));
        let out = t.handle(Input::Click(inf), &c);
        let Outcome::Commit(DrawOp::EditSweep {
            recipe: SweepRecipe::Sweep { path, taper, .. },
            ..
        }) = out
        else {
            panic!("no new path: {out:?}");
        };
        assert_eq!(path, vec![v(-5.0, 0.0, 0.0), v(-5.0, 0.0, 20.0)]);
        assert!((taper - 0.5).abs() < 1e-12);
        // A typed rectangular tube replaces the profile.
        let out = t.handle(Input::Typed(TypedValue::Pair(2.0, 3.0)), &c);
        let Outcome::Commit(DrawOp::EditSweep {
            recipe: SweepRecipe::Sweep { section, .. },
            ..
        }) = out
        else {
            panic!("no tube: {out:?}");
        };
        assert_eq!(
            section,
            SweepSection::Rect {
                width: 2.0,
                height: 3.0
            }
        );
        // Esc ends editing.
        assert!(t.cancel());
        assert!(!t.status().starts_with("Change It"));
    }

    #[test]
    fn the_tightest_bend_of_a_square_corner() {
        let pts = [v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(10.0, 4.0, 0.0)];
        // Half the shorter side (2) over tan 45°.
        assert!((tightest_bend(&pts, false).unwrap() - 2.0).abs() < 1e-12);
    }
}
