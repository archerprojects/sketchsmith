//! SketchSmith — Loft and Loft along Path tools | v1.0.0 | Thursday 12 November 2026
//!
//! **Loft along Path:** with Paths at 2 (Operation Inputs, set before
//! picking), click Path 1 then Path 2, and the profiles are turned and
//! scaled to keep spanning the two. Otherwise click the path (an edge chain
//! or curve, open or closed), then
//! profile faces lying across it (A, B, then more), and press Enter: the
//! shape is lofted along the path from the first profile to the last,
//! blended between them. The profiles are placed by where they sit on the
//! path, so their click order does not matter. On a closed path the loft
//! runs round through every profile and back into the first, with no caps,
//! and Twist does not apply (it shows greyed). A face clicked before the
//! path is refused.
//!
//! **Loft:** click profile faces (A, B, then more) and press
//! Enter: the loft runs along a smooth curve through their centres, in the
//! order clicked, leaving each square to its face.
//!
//! In both, click a profile again to leave it out. Operation Inputs:
//! Vertices (points round every profile; the others get points added along
//! their edges, never moved), Start Shift (turns each profile's pairing
//! with the one before), Segments (rings between profiles: 0 straight, more
//! curved), Twist (degrees over the length), Edges Only (the rings and the
//! lines along them, no faces: a cage), the profile count, then Twist B,
//! Twist C and on: each profile's own turn from the first, in degrees, the
//! loft turning from one to the next as it blends (round a loop as well).
//! Changing a value right after a loft does it again (one undo step). The
//! result is a group named "Loft" that keeps its path, profiles and values.
//!
//! **Edit Loft** on a Loft group reopens it in its own tool: Operation
//! Inputs shows its values and each change rebuilds it; clicking a face adds
//! it as a profile (or takes one already in it out), and along a path
//! clicking an edge gives it a new path. Each change is one undo step. Esc
//! ends it.

use smith_geom::{DMat4, DVec3};
use smith_model::{DefinitionId, FaceId, LoftOptions, Pickable, SweepRecipe};
use smith_ops::draw::DrawOp;
use smith_ops::sweep::profile_outline;

use crate::sweep_tool::SweepTool;
use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count};

/// Loft along Path, or Loft.
#[derive(Debug)]
pub struct LoftTool {
    /// Loft (no path).
    spline: bool,
    /// The path picked (Loft along Path): points, start first.
    path: Option<Vec<DVec3>>,
    /// The path picked closes on itself.
    closed: bool,
    /// Paths (Loft along Path): 1, or 2 for a second path the profiles
    /// keep spanning.
    paths: u32,
    /// The second path picked, running the same way as the first.
    path2: Option<Vec<DVec3>>,
    /// The profile faces picked, in the order clicked.
    profiles: Vec<FaceId>,
    /// Each profile's own turn, radians, in click order.
    twists: Vec<f64>,
    /// The most points any picked profile has.
    largest: u32,
    /// Vertices, Start Shift, Segments and Twist.
    options: LoftOptions,
    /// The last loft made, while it can still be redone.
    made: Option<DrawOp>,
    /// Edit Loft: the Loft group being edited.
    editing: Option<Editing>,
}

/// A Loft group reopened by Edit Loft.
#[derive(Debug, Clone)]
struct Editing {
    definition: DefinitionId,
    /// Its path (along a path), profiles and up, in the group's coordinates.
    path: Option<Vec<DVec3>>,
    closed: bool,
    path2: Option<Vec<DVec3>>,
    profiles: Vec<Vec<DVec3>>,
    twists: Vec<f64>,
    up: DVec3,
    /// Context to group coordinates.
    to_group: DMat4,
}

/// The centre of an outline.
fn centre(o: &[DVec3]) -> DVec3 {
    o.iter().copied().sum::<DVec3>() / o.len().max(1) as f64
}

/// The per-profile twist fields' labels, from the second profile on.
const TWIST_LABELS: [&str; 25] = [
    "Twist B", "Twist C", "Twist D", "Twist E", "Twist F", "Twist G", "Twist H", "Twist I",
    "Twist J", "Twist K", "Twist L", "Twist M", "Twist N", "Twist O", "Twist P", "Twist Q",
    "Twist R", "Twist S", "Twist T", "Twist U", "Twist V", "Twist W", "Twist X", "Twist Y",
    "Twist Z",
];

/// Points round an outline, as a count.
fn count_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl LoftTool {
    /// Loft along Path.
    pub fn along_path() -> Self {
        Self::new(false)
    }

    /// Loft (Segments start at 8, so the curve shows).
    pub fn by_spline() -> Self {
        Self::new(true)
    }

    fn new(spline: bool) -> Self {
        Self {
            spline,
            path: None,
            closed: false,
            paths: 1,
            path2: None,
            profiles: Vec::new(),
            twists: Vec::new(),
            largest: 0,
            options: LoftOptions {
                segments: if spline { 8 } else { 0 },
                ..LoftOptions::default()
            },
            made: None,
            editing: None,
        }
    }

    /// The rebuild of the group being edited with its current path,
    /// profiles and values.
    fn rebuild(&self) -> Outcome {
        let Some(ed) = &self.editing else {
            return Outcome::Nothing;
        };
        Outcome::Commit(DrawOp::EditSweep {
            definition: ed.definition,
            recipe: SweepRecipe::Loft {
                path: ed.path.clone(),
                profiles: ed.profiles.clone(),
                up: ed.up,
                options: self.options,
                path2: ed.path2.clone(),
                edges: None,
                closed: ed.closed,
                twists: ed.twists.clone(),
            },
        })
    }

    /// The profiles' own turns in play (as many as profiles), radians.
    fn twists_in_play(&self) -> Vec<f64> {
        let (mut t, n) = match (&self.editing, &self.made) {
            (Some(ed), _) => (ed.twists.clone(), ed.profiles.len()),
            (
                None,
                Some(DrawOp::Loft {
                    twists, profiles, ..
                }),
            ) if self.profiles.is_empty() => (twists.clone(), profiles.len()),
            _ => (self.twists.clone(), self.profiles.len()),
        };
        t.resize(n, 0.0);
        t
    }

    /// Where the per-profile twist fields start in Operation Inputs.
    fn first_twist_field(&self) -> usize {
        if self.spline { 6 } else { 7 }
    }

    /// Sets profile `i`'s own turn (radians) where it is in play; whether
    /// a loft made or being edited is to be built again.
    fn set_twist(&mut self, i: usize, turn: f64) -> bool {
        let set = |t: &mut Vec<f64>, n: usize| {
            t.resize(n.max(i + 1), 0.0);
            t[i] = turn;
        };
        if let Some(ed) = self.editing.as_mut() {
            set(&mut ed.twists, ed.profiles.len());
            return true;
        }
        if self.profiles.is_empty()
            && let Some(DrawOp::Loft {
                twists, profiles, ..
            }) = self.made.as_mut()
        {
            set(twists, profiles.len());
            return true;
        }
        let n = self.profiles.len();
        set(&mut self.twists, n);
        false
    }

    /// Whether the path in play closes on itself (Twist does not apply).
    fn closed_in_play(&self) -> bool {
        match (&self.editing, &self.made) {
            (Some(ed), _) => ed.closed,
            (None, Some(DrawOp::Loft { closed, .. })) if self.path.is_none() => *closed,
            _ => self.closed,
        }
    }

    /// The last loft done again with the current values.
    fn redo(&mut self) -> Outcome {
        if self.editing.is_some() {
            return self.rebuild();
        }
        let Some(DrawOp::Loft {
            path,
            closed,
            profiles,
            up,
            path2,
            twists,
            ..
        }) = self.made.clone()
        else {
            return Outcome::Nothing;
        };
        let op = DrawOp::Loft {
            path,
            closed,
            profiles,
            up,
            options: self.options,
            path2,
            twists,
        };
        self.made = Some(op.clone());
        Outcome::Redefine(op)
    }

    /// A click while editing.
    fn edit_click(&mut self, entity: Option<Pickable>, near: DVec3, ctx: &Ctx<'_>) -> Outcome {
        let spline = self.spline;
        let Some(ed) = self.editing.as_mut() else {
            return Outcome::Nothing;
        };
        let m = ed.to_group;
        match entity {
            Some(Pickable::Face(f)) => {
                let Ok(pts) = profile_outline(ctx.entities, f) else {
                    return Outcome::Message("Click a Face With No Holes".into());
                };
                let outline: Vec<DVec3> = pts.iter().map(|p| m.transform_point3(*p)).collect();
                let c = centre(&outline);
                // A profile already there comes out; any other goes in.
                if let Some(i) = ed
                    .profiles
                    .iter()
                    .position(|o| centre(o).distance(c) < 1e-6)
                {
                    if ed.profiles.len() <= 2 {
                        return Outcome::Message("A Loft Keeps Two Profiles at Least".into());
                    }
                    ed.profiles.remove(i);
                    if i < ed.twists.len() {
                        ed.twists.remove(i);
                    }
                } else {
                    ed.twists.resize(ed.profiles.len(), 0.0);
                    ed.profiles.push(outline);
                    ed.twists.push(0.0);
                }
                self.rebuild()
            }
            Some(Pickable::Edge(e)) if !spline => match SweepTool::path_from(e, near, ctx) {
                Some((_, true)) if ed.path2.is_some() => {
                    Outcome::Message("Two Paths Must Both Be Open".into())
                }
                Some((pts, closed)) => {
                    ed.path = Some(pts.iter().map(|p| m.transform_point3(*p)).collect());
                    ed.closed = closed;
                    self.rebuild()
                }
                None => Outcome::Nothing,
            },
            _ if spline => Outcome::Message("Click a Profile Face".into()),
            _ => Outcome::Message("Click a Profile Face or a New Path".into()),
        }
    }

    /// The most points any profile in play has.
    fn most_points(&self) -> u32 {
        match &self.editing {
            Some(ed) => ed
                .profiles
                .iter()
                .map(|o| count_of(o.len()))
                .max()
                .unwrap_or(0),
            None => self.largest,
        }
    }
}

impl Tool for LoftTool {
    fn name(&self) -> &'static str {
        if self.spline {
            "Loft"
        } else {
            "Loft along Path"
        }
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Click(inf) if self.editing.is_some() => {
                self.edit_click(inf.entity, inf.point, ctx)
            }
            Input::Click(inf) => match inf.entity {
                Some(Pickable::Edge(e)) if !self.spline => {
                    match SweepTool::path_from(e, inf.point, ctx) {
                        Some((_, true)) if self.paths == 2 => {
                            Outcome::Message("Two Paths Must Both Be Open".into())
                        }
                        Some((pts, closed)) => {
                            match &self.path {
                                // Path 2, running the same way as Path 1.
                                Some(p1) if self.paths == 2 && self.path2.is_none() => {
                                    let mut p2 = pts;
                                    if p2[0].distance(p1[0]) > p2[p2.len() - 1].distance(p1[0]) {
                                        p2.reverse();
                                    }
                                    self.path2 = Some(p2);
                                }
                                _ => {
                                    self.path = Some(pts);
                                    self.closed = closed;
                                    self.path2 = None;
                                }
                            }
                            self.made = None;
                            Outcome::Nothing
                        }
                        None => Outcome::Nothing,
                    }
                }
                Some(Pickable::Face(_)) if !self.spline && self.path.is_none() => {
                    Outcome::Message("Click the Path First".into())
                }
                Some(Pickable::Face(_))
                    if !self.spline && self.paths == 2 && self.path2.is_none() =>
                {
                    Outcome::Message("Click Path 2 First".into())
                }
                Some(Pickable::Face(f)) => {
                    if let Some(i) = self.profiles.iter().position(|p| *p == f) {
                        self.profiles.remove(i);
                        if i < self.twists.len() {
                            self.twists.remove(i);
                        }
                    } else {
                        if profile_outline(ctx.entities, f).is_err() {
                            return Outcome::Message("Click a Face With No Holes".into());
                        }
                        self.twists.resize(self.profiles.len(), 0.0);
                        self.profiles.push(f);
                        self.twists.push(0.0);
                    }
                    self.made = None;
                    self.largest = self
                        .profiles
                        .iter()
                        .filter_map(|p| profile_outline(ctx.entities, *p).ok())
                        .map(|o| count_of(o.len()))
                        .max()
                        .unwrap_or(0);
                    Outcome::Nothing
                }
                _ => Outcome::Nothing,
            },
            Input::Finish => {
                if !self.spline && self.path.is_none() {
                    return Outcome::Nothing;
                }
                if self.profiles.len() < 2 {
                    return Outcome::Message("Click Two or More Profile Faces".into());
                }
                let mut twists = std::mem::take(&mut self.twists);
                twists.resize(self.profiles.len(), 0.0);
                let op = DrawOp::Loft {
                    path: self.path.take(),
                    closed: std::mem::take(&mut self.closed),
                    profiles: std::mem::take(&mut self.profiles),
                    up: ctx.axes.z,
                    options: self.options,
                    path2: self.path2.take(),
                    twists,
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
        let busy = self.path.is_some() || !self.profiles.is_empty();
        self.path = None;
        self.closed = false;
        self.twists.clear();
        self.path2 = None;
        self.profiles.clear();
        self.largest = 0;
        busy
    }

    fn use_sweep(&mut self, definition: DefinitionId, recipe: SweepRecipe, placed: DMat4) {
        if let SweepRecipe::Loft {
            path,
            profiles,
            up,
            options,
            path2,
            edges: None,
            closed,
            twists,
        } = recipe
        {
            self.path = None;
            self.profiles.clear();
            self.made = None;
            self.options = options;
            self.editing = Some(Editing {
                definition,
                path,
                closed,
                path2,
                profiles,
                twists,
                up,
                to_group: placed.inverse(),
            });
        }
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn wants_finish(&self) -> bool {
        self.editing.is_none() && !self.profiles.is_empty()
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        let path = match &self.editing {
            Some(ed) => {
                let back = ed.to_group.inverse();
                ed.path
                    .as_ref()
                    .map(|p| p.iter().map(|x| back.transform_point3(*x)).collect())
            }
            None => self.path.clone(),
        };
        let lines = path
            .map(|pts: Vec<DVec3>| pts.windows(2).map(|w| [w[0], w[1]]).collect())
            .unwrap_or_default();
        Preview {
            lines,
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.editing.is_some() {
            return if self.spline {
                "Change the Loft in Operation Inputs · Click a Face to Add or Remove a Profile · Esc Ends".into()
            } else {
                "Change the Loft in Operation Inputs · Click a Face to Add or Remove a Profile, or a New Path · Esc Ends".into()
            };
        }
        if !self.spline && self.path.is_none() {
            return if self.paths == 2 {
                "Click Path 1".into()
            } else {
                "Click the Path".into()
            };
        }
        if !self.spline && self.paths == 2 && self.path2.is_none() {
            return "Click Path 2".into();
        }
        match self.profiles.len() {
            0 => "Click Profile A".into(),
            1 => "Click Profile B".into(),
            _ => "Click More Profiles, or Enter Lofts".into(),
        }
    }

    fn fields(&self) -> Vec<Field> {
        let n = match (&self.editing, &self.made) {
            (Some(ed), _) => ed.profiles.len(),
            (None, Some(DrawOp::Loft { profiles, .. })) if self.profiles.is_empty() => {
                profiles.len()
            }
            _ => self.profiles.len(),
        };
        let mut fields = vec![
            Field::count(
                "Vertices",
                self.options.vertices.max(self.most_points()),
                true,
            ),
            Field::count("Start Shift", self.options.start_shift, true),
            Field::count("Segments", self.options.segments, true),
            // Round a closed path Twist does not apply.
            Field::number(
                "Twist",
                Some(self.options.twist.to_degrees()),
                !self.closed_in_play(),
            ),
            Field::switch("Edges Only", self.options.edges_only, true),
        ];
        // Along a path: Paths, set before picking.
        if !self.spline {
            fields.push(Field::count("Paths", self.paths, self.editing.is_none()));
        }
        fields.push(Field::count("Profiles", count_of(n), false));
        // Each profile's own turn, from the second on.
        for (label, turn) in TWIST_LABELS
            .iter()
            .zip(self.twists_in_play().iter().skip(1))
        {
            fields.push(Field::number(label, Some(turn.to_degrees()), true));
        }
        fields
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        match index {
            // Never below the most any profile has.
            0 => self.options.vertices = as_count(value).max(self.most_points()),
            1 => self.options.start_shift = as_count(value),
            2 => self.options.segments = as_count(value).min(256),
            3 if value.is_finite() => self.options.twist = value.to_radians(),
            4 => self.options.edges_only = value != 0.0,
            // Two at most for now; changing it starts the picks again.
            5 if !self.spline && self.editing.is_none() => {
                self.paths = as_count(value).clamp(1, 2);
                self.path = None;
                self.path2 = None;
                self.profiles.clear();
                self.twists.clear();
                return Outcome::Nothing;
            }
            i if i >= self.first_twist_field() && value.is_finite() => {
                let profile = i - self.first_twist_field() + 1;
                if !self.set_twist(profile, value.to_radians()) {
                    return Outcome::Nothing;
                }
            }
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

    /// A vertical path and two square faces across it.
    fn model() -> Entities {
        let mut m = Entities::new();
        m.add_polyline(&[v(0.0, 0.0, 0.0), v(0.0, 0.0, 20.0)], false)
            .unwrap();
        m.add_polyline(
            &[
                v(-1.0, -1.0, 0.0),
                v(1.0, -1.0, 0.0),
                v(1.0, 1.0, 0.0),
                v(-1.0, 1.0, 0.0),
            ],
            true,
        )
        .unwrap();
        m.add_polyline(
            &[v(-2.0, -2.0, 20.0), v(2.0, -2.0, 20.0), v(2.0, 2.0, 20.0)],
            true,
        )
        .unwrap();
        m
    }

    fn click(t: &mut LoftTool, p: Pickable, c: &Ctx<'_>) -> Outcome {
        let mut inf = at(v(0.0, 0.0, 5.0));
        inf.entity = Some(p);
        t.handle(Input::Click(inf), c)
    }

    #[test]
    fn along_a_path_the_path_comes_first_then_a_and_b() {
        let m = model();
        let e = m.edge_ids().next().unwrap();
        let faces: Vec<FaceId> = m.face_ids().collect();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = LoftTool::along_path();
        assert_eq!(t.status(), "Click the Path");
        assert!(matches!(
            click(&mut t, Pickable::Face(faces[0]), &c),
            Outcome::Message(_)
        ));
        click(&mut t, Pickable::Edge(e), &c);
        assert_eq!(t.status(), "Click Profile A");
        click(&mut t, Pickable::Face(faces[0]), &c);
        assert_eq!(t.status(), "Click Profile B");
        click(&mut t, Pickable::Face(faces[1]), &c);
        assert_eq!(t.status(), "Click More Profiles, or Enter Lofts");
        // Vertices shows the most points any profile has.
        assert_eq!(t.fields()[0].value, Some(4.0));
        let out = t.handle(Input::Finish, &c);
        let Outcome::Commit(DrawOp::Loft {
            path: Some(path),
            profiles,
            ..
        }) = out
        else {
            panic!("no loft: {out:?}");
        };
        assert_eq!(path.len(), 2);
        assert_eq!(profiles, faces);
        // Segments right after redoes it.
        let out = t.edit(2, 3.0, &c);
        assert!(matches!(
            out,
            Outcome::Redefine(DrawOp::Loft {
                options: LoftOptions { segments: 3, .. },
                ..
            })
        ));
        // Vertices never drops below the largest profile.
        t.edit(0, 2.0, &c);
        assert_eq!(t.options.vertices, 4);
    }

    #[test]
    fn a_closed_path_is_taken_and_its_loft_closes_with_twist_greyed() {
        let mut m = model();
        // A loop off to the side.
        m.add_polyline(
            &[
                v(10.0, 0.0, 0.0),
                v(30.0, 0.0, 0.0),
                v(30.0, 0.0, 20.0),
                v(10.0, 0.0, 20.0),
            ],
            true,
        )
        .unwrap();
        let loop_edge = m
            .edge_ids()
            .find(|e| {
                m.edge_points(*e)
                    .is_some_and(|(a, b)| a.x >= 10.0 && b.x >= 10.0)
            })
            .unwrap();
        let faces: Vec<FaceId> = m
            .face_ids()
            .filter(|f| m.face(*f).is_some_and(|x| x.plane().normal().z.abs() > 0.5))
            .collect();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        // Two paths must both be open.
        let mut two = LoftTool::along_path();
        two.edit(5, 2.0, &c);
        assert!(matches!(
            click(&mut two, Pickable::Edge(loop_edge), &c),
            Outcome::Message(_)
        ));
        let mut t = LoftTool::along_path();
        click(&mut t, Pickable::Edge(loop_edge), &c);
        assert_eq!(t.status(), "Click Profile A");
        // Twist does not apply round a loop.
        assert!(!t.fields()[3].editable);
        click(&mut t, Pickable::Face(faces[0]), &c);
        click(&mut t, Pickable::Face(faces[1]), &c);
        let out = t.handle(Input::Finish, &c);
        assert!(
            matches!(out, Outcome::Commit(DrawOp::Loft { closed: true, .. })),
            "{out:?}"
        );
        assert!(!t.fields()[3].editable);
        // An open path leaves it on.
        let mut open = LoftTool::along_path();
        click(&mut open, Pickable::Edge(m.edge_ids().next().unwrap()), &c);
        assert!(open.fields()[3].editable);
    }

    #[test]
    fn each_profile_after_the_first_has_its_own_twist() {
        let m = model();
        let e = m.edge_ids().next().unwrap();
        let faces: Vec<FaceId> = m.face_ids().collect();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = LoftTool::along_path();
        click(&mut t, Pickable::Edge(e), &c);
        click(&mut t, Pickable::Face(faces[0]), &c);
        // One profile: no twist of its own yet.
        assert!(t.fields().iter().all(|f| f.label != "Twist B"));
        click(&mut t, Pickable::Face(faces[1]), &c);
        let fields = t.fields();
        assert_eq!(fields[7].label, "Twist B");
        // Set while picking: kept for the loft.
        assert!(matches!(t.edit(7, 45.0, &c), Outcome::Nothing));
        let out = t.handle(Input::Finish, &c);
        let Outcome::Commit(DrawOp::Loft { twists, .. }) = out else {
            panic!("no loft: {out:?}");
        };
        assert_eq!(twists.len(), 2);
        assert!((twists[1] - 45f64.to_radians()).abs() < 1e-12);
        // Changed right after: done again.
        let out = t.edit(7, -30.0, &c);
        let Outcome::Redefine(DrawOp::Loft { twists, .. }) = out else {
            panic!("not redone: {out:?}");
        };
        assert!((twists[1] + 30f64.to_radians()).abs() < 1e-12);
        assert!(t.fields()[7].value.is_some_and(|v| (v + 30.0).abs() < 1e-9));
    }

    #[test]
    fn by_spline_needs_no_path_and_two_profiles() {
        let m = model();
        let faces: Vec<FaceId> = m.face_ids().collect();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = LoftTool::by_spline();
        assert_eq!(t.status(), "Click Profile A");
        click(&mut t, Pickable::Face(faces[1]), &c);
        assert!(matches!(t.handle(Input::Finish, &c), Outcome::Message(_)));
        click(&mut t, Pickable::Face(faces[0]), &c);
        let out = t.handle(Input::Finish, &c);
        let Outcome::Commit(DrawOp::Loft {
            path: None,
            profiles,
            ..
        }) = out
        else {
            panic!("no spline loft: {out:?}");
        };
        // In the order clicked.
        assert_eq!(profiles, vec![faces[1], faces[0]]);
    }
}
