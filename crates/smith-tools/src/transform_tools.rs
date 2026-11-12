//! SketchSmith — Rotate, Scale and Mirror tools | v1.0.0 | Thursday 12 November 2026
//!
//! All three work on the selection; with nothing selected, the first click
//! takes the edge or face under the cursor. None pauses after an operation:
//! the last one's values stay in Operation Inputs and can be changed (one
//! undo step) until the next one starts. Ctrl (a tap) switches Copy.
//!
//! * **Rotate** — click the centre (on a face the axis is its normal,
//!   otherwise the blue drawing axis), click a point to start the angle, then
//!   click where it ends, or type the angle in degrees. The angle snaps to
//!   15° steps near them. After a copy, `11x` makes a radial array.
//!   A protractor shows the plane of turning: it lies on the face under the
//!   cursor, or on the plane the arrow keys choose (Right red, Left green,
//!   Up blue; again to release). Once the angle has started, it follows the
//!   cursor freely: axis lines from the centre no longer pull it, and only
//!   the 15° steps and points right under the cursor do.
//! * **Scale** — Operation Inputs shows the selection's Width, Depth and
//!   Height: type exact sizes, with Lock Proportions keeping the shape. Or
//!   click a point and move away from or towards the centre, or type a plain
//!   number as the factor. Always about the centre of the selection. Sizes
//!   and factors run along the drawing axes.
//! * **Mirror** — click a face to mirror in its plane, or click a point to
//!   mirror in the red, green or blue plane through it (chosen in Operation
//!   Inputs).
//!
//! **Scale grips.** The selection's box shows grips at its corners, the
//! middles of its edges and the centres of its faces. Dragging a corner
//! scales evenly, an edge grip the two axes it moves on (evenly with Lock
//! Proportions), a face grip its one axis, each with the opposite grip
//! staying put (the box's centre with Around Centre).
//!
//! **Around Centre** (Operation Inputs, all three tools): the operation
//! works about the centre of the items' bounding box instead of the Pivot or
//! the point clicked. Switching it after the operation does the operation
//! again about the other point, as one undo step.
//!
//! **Pivot Active.** While it is on, Rotate turns about the Pivot (the first
//! click starts the angle; the axis is the normal of a face clicked, else
//! blue), Scale scales about it, and Mirror mirrors in the chosen plane
//! through it (a click only picks what to mirror).

use smith_geom::plane::Plane;
use smith_geom::{DMat3, DVec3};
use smith_infer::infer::Lock;
use smith_infer::units::TypedValue;
use smith_model::Pickable;
use smith_ops::draw::DrawOp;
use smith_ops::modify::{Transform, items_bounds_in};

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count, face_normal, item_lines};

/// Angle snapping step and how near it pulls, in degrees.
pub(crate) const SNAP_STEP: f64 = 15.0;
pub(crate) const SNAP_NEAR: f64 = 1.5;

/// Radius of the protractor on screen, in pixels.
const PROTRACTOR_PX: f64 = 70.0;

/// A protractor at `c` lying in the plane with normal `axis`: a circle with
/// ticks every 15°, longer every 90°, sized to the screen.
pub(crate) fn protractor(c: DVec3, axis: DVec3, ctx: &Ctx<'_>) -> Vec<[DVec3; 2]> {
    let n = axis.normalize_or(DVec3::Z);
    let r = PROTRACTOR_PX * ctx.pixel.max(1e-9);
    // Start the circle along red where it lies in the plane.
    let x = ctx.axes.x - n * n.dot(ctx.axes.x);
    let u = if x.length() > 1e-9 {
        x.normalize()
    } else {
        n.any_orthonormal_vector()
    };
    let v = n.cross(u);
    let at = |deg: f64, k: f64| {
        let a = deg.to_radians();
        c + (u * a.cos() + v * a.sin()) * r * k
    };
    let mut lines: Vec<[DVec3; 2]> = (0..48)
        .map(|i| [at(f64::from(i) * 7.5, 1.0), at(f64::from(i + 1) * 7.5, 1.0)])
        .collect();
    for i in 0..24 {
        let deg = f64::from(i) * SNAP_STEP;
        let inner = if i % 6 == 0 { 0.7 } else { 0.85 };
        lines.push([at(deg, inner), at(deg, 1.0)]);
    }
    lines
}

/// State shared by the three tools.
#[derive(Debug, Default)]
struct Common {
    items: Vec<Pickable>,
    hover: Option<DVec3>,
    copy: bool,
    copies: u32,
    divide: bool,
    done: Option<Transform>,
    /// Operation Inputs shows `done`.
    shown: bool,
    /// Around Centre (Operation Inputs): the operation turns, scales or
    /// mirrors about the centre of the items' bounding box instead of the
    /// Pivot (or the point clicked).
    around_centre: bool,
    /// The Pivot or clicked point the operation would use, kept so Around
    /// can be switched after the operation.
    picked: Option<DVec3>,
}

/// `t` moved to work about `c`: a rotation's axis point, a scale's fixed
/// point, a mirror plane's point.
fn recentre(t: Transform, c: DVec3) -> Transform {
    match t {
        Transform::Rotate { axis, angle, .. } => Transform::Rotate {
            center: c,
            axis,
            angle,
        },
        Transform::Scale { factors, axes, .. } => Transform::Scale {
            center: c,
            factors,
            axes,
        },
        Transform::Mirror { normal, .. } => Transform::Mirror { point: c, normal },
        other => other,
    }
}

impl Common {
    /// The centre of the items' bounding box.
    fn items_centre(&self, ctx: &Ctx<'_>) -> Option<DVec3> {
        items_bounds_in(ctx.entities, &self.items, ctx.instances, DMat3::IDENTITY)
            .map(|(lo, hi)| (lo + hi) * 0.5)
    }

    /// The point to work about, given the Pivot or the clicked point.
    fn about(&mut self, picked: DVec3, ctx: &Ctx<'_>) -> DVec3 {
        self.picked = Some(picked);
        if self.around_centre {
            self.items_centre(ctx).unwrap_or(picked)
        } else {
            picked
        }
    }

    /// Switches Around Centre; after an operation it is done again about
    /// the new point (one undo step). Returns the new point too.
    fn set_around(&mut self, on: bool, ctx: &Ctx<'_>) -> (Outcome, Option<DVec3>) {
        if on == self.around_centre {
            return (Outcome::Nothing, None);
        }
        self.around_centre = on;
        let Some(picked) = self.picked else {
            return (Outcome::Nothing, None);
        };
        let c = if on {
            self.items_centre(ctx).unwrap_or(picked)
        } else {
            picked
        };
        match self.shown_done() {
            Some(t) => (self.redefine(recentre(t, c)), Some(c)),
            None => (Outcome::Nothing, Some(c)),
        }
    }

    fn around_field(&self) -> Field {
        Field::switch("Around Centre", self.around_centre, true)
    }

    fn shown_done(&self) -> Option<Transform> {
        self.done.filter(|_| self.shown)
    }

    /// Takes the clicked entity when there is no selection (or a new
    /// operation starts after one is done). `false` if there is nothing.
    fn take_items(&mut self, entity: Option<Pickable>) -> bool {
        if self.items.is_empty() || self.done.is_some() {
            match entity {
                Some(p) => self.items = vec![p],
                None if self.items.is_empty() => return false,
                None => {}
            }
        }
        true
    }

    fn op(&self, t: Transform) -> DrawOp {
        DrawOp::Transform {
            items: self.items.clone(),
            transform: t,
            copy: self.copy,
            copies: self.copies.max(1),
            divide: self.divide,
        }
    }

    fn commit(&mut self, t: Transform) -> Outcome {
        self.copies = 1;
        self.divide = false;
        self.done = Some(t);
        self.shown = true;
        Outcome::Commit(self.op(t))
    }

    fn redefine(&mut self, t: Transform) -> Outcome {
        self.done = Some(t);
        self.shown = true;
        Outcome::Redefine(self.op(t))
    }

    /// Copy, `Nx` and `/N` after an operation.
    fn after(&mut self, input: Input) -> Option<Outcome> {
        let done = self.shown_done();
        match input {
            Input::Toggle => {
                self.copy = !self.copy;
                Some(done.map_or(Outcome::Nothing, |t| self.redefine(t)))
            }
            Input::Typed(TypedValue::Copies(n) | TypedValue::Divide(n)) => {
                let divide = matches!(input, Input::Typed(TypedValue::Divide(_)));
                Some(match done {
                    Some(t) if self.copy && n >= 1 => {
                        self.copies = n;
                        self.divide = divide;
                        self.redefine(t)
                    }
                    _ => Outcome::Message("Copy first (Ctrl), then type the count".into()),
                })
            }
            _ => None,
        }
    }

    fn preview(&self, ctx: &Ctx<'_>, t: Option<Transform>, band: Option<[DVec3; 2]>) -> Preview {
        let lines = t
            .map(|t| {
                let m = t.matrix(1.0);
                item_lines(ctx, &self.items)
                    .into_iter()
                    .map(|[a, b]| [m.transform_point3(a), m.transform_point3(b)])
                    .collect()
            })
            .unwrap_or_default();
        Preview {
            lines,
            band,
            note: None,
        }
    }

    fn copy_fields(&self, editable: bool) -> [Field; 2] {
        [
            Field::switch("Copy", self.copy, true),
            Field::count("Copies", self.copies.max(1), editable && self.copy),
        ]
    }

    fn accept(&mut self) {
        self.shown = false;
    }

    fn reopen(&mut self) -> bool {
        if self.done.is_some() {
            self.shown = true;
        }
        self.shown
    }

    fn forget(&mut self) {
        self.done = None;
        self.shown = false;
    }
}

// ----- Rotate -----------------------------------------------------------------

/// The Rotate tool.
#[derive(Debug, Default)]
pub struct RotateTool {
    c: Common,
    pivot: Option<DVec3>,
    /// Centre and unit axis.
    centre: Option<(DVec3, DVec3)>,
    /// Where the angle starts.
    from: Option<DVec3>,
    /// The plane chosen with the arrow keys, as its normal.
    axis_lock: Option<DVec3>,
    /// The plane under the cursor before the centre is placed.
    hover_axis: Option<DVec3>,
}

impl RotateTool {
    /// The angle from `from` to `p` about the axis, radians, snapped near
    /// 15° steps.
    fn angle_to(&self, p: DVec3) -> Option<f64> {
        let (c, axis) = self.centre?;
        let flat = |v: DVec3| v - axis * v.dot(axis);
        let a = flat(self.from? - c);
        let b = flat(p - c);
        if a.length() < 1e-9 || b.length() < 1e-9 {
            return None;
        }
        let deg = a.cross(b).dot(axis).atan2(a.dot(b)).to_degrees();
        let near = (deg / SNAP_STEP).round() * SNAP_STEP;
        let deg = if (deg - near).abs() <= SNAP_NEAR {
            near
        } else {
            deg
        };
        Some(deg.to_radians())
    }

    fn transform(&self, angle: f64) -> Option<Transform> {
        let (center, axis) = self.centre?;
        Some(Transform::Rotate {
            center,
            axis,
            angle,
        })
    }

    fn set_angle(&mut self, angle: f64) -> Outcome {
        if self.from.is_some() {
            let Some(t) = self.transform(angle) else {
                return Outcome::Nothing;
            };
            self.centre = None;
            self.from = None;
            return self.c.commit(t);
        }
        match self.c.shown_done() {
            Some(Transform::Rotate { center, axis, .. }) => self.c.redefine(Transform::Rotate {
                center,
                axis,
                angle,
            }),
            _ => Outcome::Message("Click the Centre".into()),
        }
    }
}

impl Tool for RotateTool {
    fn name(&self) -> &'static str {
        "Rotate"
    }

    fn use_pivot(&mut self, pivot: Option<DVec3>) {
        self.pivot = pivot;
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        self.c.items = items.to_vec();
    }

    fn pauses(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        if let Some(out) = self.c.after(input) {
            return out;
        }
        match input {
            Input::Hover(inf) => {
                self.c.hover = Some(inf.point);
                self.hover_axis = self.axis_lock.or_else(|| face_normal(&inf, ctx.entities));
                Outcome::Nothing
            }
            Input::Click(inf) => match (self.centre, self.from) {
                (None, _) => {
                    if !self.c.take_items(inf.entity) {
                        return Outcome::Message("Click an Edge or Face".into());
                    }
                    self.c.done = None;
                    self.c.shown = false;
                    let axis = self
                        .axis_lock
                        .or_else(|| face_normal(&inf, ctx.entities))
                        .unwrap_or(ctx.axes.z);
                    // Around the Pivot or the items' centre, this click
                    // starts the angle, unless it is on that point, which
                    // only confirms it; otherwise it places the centre.
                    let c = self.c.about(self.pivot.unwrap_or(inf.point), ctx);
                    self.centre = Some((c, axis));
                    let near = (ctx.pixel * 8.0).max(1e-6);
                    if inf.point.distance(c) > near {
                        self.from = Some(inf.point);
                    }
                    Outcome::Nothing
                }
                (Some((c, _)), None) => {
                    if inf.point.distance(c) > (ctx.pixel * 8.0).max(1e-6) {
                        self.from = Some(inf.point);
                    }
                    Outcome::Nothing
                }
                (Some(_), Some(_)) => match self.angle_to(inf.point) {
                    Some(a) if a.abs() > 1e-12 => self.set_angle(a),
                    _ => Outcome::Nothing,
                },
            },
            Input::Typed(TypedValue::Length(deg)) => self.set_angle(deg.to_radians()),
            Input::Typed(_) => Outcome::Message("Type the angle in degrees".into()),
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = self.centre.is_some();
        self.centre = None;
        self.from = None;
        self.axis_lock = None;
        busy
    }

    /// The centre while the start of the angle is chosen, so axis lines
    /// from it guide that click; none once the angle is turning, so the
    /// angle follows the cursor freely.
    fn last_point(&self) -> Option<DVec3> {
        match (self.centre, self.from) {
            (Some((c, _)), None) => Some(c),
            _ => None,
        }
    }

    fn use_axis_lock(&mut self, axis: Option<DVec3>) -> bool {
        self.axis_lock = axis;
        // Before the angle starts, the plane can still be changed.
        if let (Some((c, _)), None, Some(a)) = (self.centre, self.from, axis) {
            self.centre = Some((c, a));
        }
        true
    }

    fn lock(&self) -> Option<Lock> {
        let (c, axis) = self.centre?;
        Plane::from_point_normal(c, axis).ok().map(Lock::Plane)
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        let Some((c, axis)) = self.centre else {
            // Before the first click, the protractor follows the cursor.
            let lines = self
                .c
                .hover
                .map(|h| protractor(h, self.hover_axis.unwrap_or(ctx.axes.z), ctx))
                .unwrap_or_default();
            return Preview {
                lines,
                band: None,
                note: None,
            };
        };
        let t = self
            .c
            .hover
            .and_then(|h| self.angle_to(h))
            .and_then(|a| self.transform(a));
        let band = self.c.hover.map(|h| [c, h]);
        let mut p = self.c.preview(ctx, t, band);
        p.lines.extend(protractor(c, axis, ctx));
        // The lever: from the centre to where the angle starts.
        if let Some(f) = self.from {
            p.lines.push([c, f]);
        }
        p
    }

    fn status(&self) -> String {
        match (self.centre, self.from) {
            (None, _) if self.pivot.is_some() => "Click to Start the Angle".into(),
            (None, _) => "Click the Centre".into(),
            (Some(_), None) => "Click to Start the Angle".into(),
            (Some(_), Some(_)) => "Click to End the Angle, or Type it in Degrees".into(),
        }
    }

    fn fields(&self) -> Vec<Field> {
        let live = self.from.is_some();
        let angle = if live {
            self.c.hover.and_then(|h| self.angle_to(h))
        } else {
            match self.c.shown_done() {
                Some(Transform::Rotate { angle, .. }) => Some(angle),
                _ => None,
            }
        };
        let editable = live || self.c.shown_done().is_some();
        let mut f = vec![Field::number("Angle", angle.map(f64::to_degrees), editable)];
        f.extend(self.c.copy_fields(editable));
        f.push(self.c.around_field());
        f
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 => self.set_angle(value.to_radians()),
            1 if (value != 0.0) != self.c.copy => self.handle(Input::Toggle, ctx),
            2 => self.handle(Input::Typed(TypedValue::Copies(as_count(value))), ctx),
            3 => {
                let (out, c) = self.c.set_around(value != 0.0, ctx);
                if let (Some(c), Some((_, axis))) = (c, self.centre) {
                    self.centre = Some((c, axis));
                }
                out
            }
            _ => Outcome::Nothing,
        }
    }

    fn accept(&mut self) {
        self.c.accept();
    }

    fn reopen(&mut self) -> bool {
        self.c.reopen()
    }

    fn forget(&mut self) {
        self.c.forget();
    }
}

// ----- Scale ------------------------------------------------------------------

/// The Scale tool.
#[derive(Debug)]
pub struct ScaleTool {
    c: Common,
    pivot: Option<DVec3>,
    /// Centre and size of the items before scaling.
    base: Option<(DVec3, DVec3)>,
    /// The drawing axes the sizes are measured along (unit columns).
    frame: DMat3,
    /// The grip clicked to scale by moving.
    grip: Option<DVec3>,
    lock: bool,
    /// The items' box in the drawing axes (low and high corners).
    bounds: Option<(DVec3, DVec3)>,
    /// For a box grip: which side of the box it is on along each axis
    /// (-1 low, 0 middle, 1 high), and the point that stays put.
    held: Option<([i8; 3], DVec3)>,
}

/// How near (in screen pixels) a click must be to a box grip to take it.
const GRIP_PX: f64 = 10.0;
/// Half the size of a drawn box grip, in screen pixels.
const GRIP_DRAW_PX: f64 = 4.0;

impl Default for ScaleTool {
    fn default() -> Self {
        Self {
            c: Common::default(),
            pivot: None,
            base: None,
            frame: DMat3::IDENTITY,
            grip: None,
            lock: true,
            bounds: None,
            held: None,
        }
    }
}

impl ScaleTool {
    fn measure(&mut self, ctx: &Ctx<'_>) {
        if self.base.is_some() {
            return;
        }
        let frame = DMat3::from_cols(ctx.axes.x, ctx.axes.y, ctx.axes.z);
        if let Some((lo, hi)) = items_bounds_in(ctx.entities, &self.c.items, ctx.instances, frame) {
            self.frame = frame;
            let picked = self.pivot.unwrap_or(frame * ((lo + hi) * 0.5));
            let c = self.c.about(picked, ctx);
            self.base = Some((c, hi - lo));
            self.bounds = Some((lo, hi));
        }
    }

    fn factors(&self) -> DVec3 {
        match self.c.shown_done() {
            Some(Transform::Scale { factors, .. }) => factors,
            _ => DVec3::ONE,
        }
    }

    /// Scales by `factors` about `center` (a box grip's fixed point).
    fn apply_about(&mut self, factors: DVec3, center: DVec3) -> Outcome {
        let t = Transform::Scale {
            center,
            factors,
            axes: self.frame,
        };
        self.grip = None;
        if self.c.shown_done().is_some() {
            self.c.redefine(t)
        } else {
            self.c.commit(t)
        }
    }

    fn apply(&mut self, factors: DVec3) -> Outcome {
        let Some((center, _)) = self.base else {
            return Outcome::Message("Select something to scale".into());
        };
        let t = Transform::Scale {
            center,
            factors,
            axes: self.frame,
        };
        self.grip = None;
        if self.c.shown_done().is_some() {
            self.c.redefine(t)
        } else {
            self.c.commit(t)
        }
    }

    /// The box's 26 grips: its corners, the middles of its edges and the
    /// centres of its faces, each with its side along each axis.
    fn grips(&self) -> Vec<([i8; 3], DVec3)> {
        let Some((lo, hi)) = self.bounds else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for x in -1i8..=1 {
            for y in -1i8..=1 {
                for z in -1i8..=1 {
                    // A flat box has no grips off its flat side.
                    let flat = |side: i8, i: usize| side != 0 && hi[i] - lo[i] < 1e-9;
                    if (x, y, z) == (0, 0, 0) || flat(x, 0) || flat(y, 1) || flat(z, 2) {
                        continue;
                    }
                    let pick = |s: i8, i: usize| match s {
                        -1 => lo[i],
                        1 => hi[i],
                        _ => (lo[i] + hi[i]) * 0.5,
                    };
                    let local = DVec3::new(pick(x, 0), pick(y, 1), pick(z, 2));
                    out.push(([x, y, z], self.frame * local));
                }
            }
        }
        out
    }

    /// The grip opposite `side`, which stays put while it is dragged.
    fn opposite(&self, side: [i8; 3]) -> Option<DVec3> {
        let back = [-side[0], -side[1], -side[2]];
        self.grips()
            .into_iter()
            .find(|(s, _)| *s == back)
            .map(|(_, p)| p)
    }

    /// The factors a box grip being dragged sets: along each axis it moves
    /// on, how far the cursor is from the fixed point compared with the
    /// grip. A corner grip, and an edge grip with Lock Proportions, scale
    /// evenly, by the cursor's distance along the line from the fixed point
    /// through the grip.
    fn grip_factors(&self) -> Option<DVec3> {
        let (side, fixed) = self.held?;
        let (g, h) = (self.grip?, self.c.hover?);
        let moving = side.iter().filter(|s| **s != 0).count();
        if moving == 3 || (moving == 2 && self.lock) {
            let d = g - fixed;
            let f = (h - fixed).dot(d) / d.length_squared().max(1e-18);
            let mut out = DVec3::ONE;
            for i in 0..3 {
                if side[i] != 0 {
                    out[i] = f;
                }
            }
            return Some(out);
        }
        let mut out = DVec3::ONE;
        for i in 0..3 {
            if side[i] != 0 {
                let a = self.frame.col(i);
                let span = (g - fixed).dot(a);
                if span.abs() < 1e-12 {
                    return None;
                }
                out[i] = (h - fixed).dot(a) / span;
            }
        }
        Some(out)
    }

    /// The factors and fixed point the cursor sets now: a box grip's, or an
    /// even factor about the centre.
    fn live(&self) -> Option<(DVec3, DVec3)> {
        match self.held {
            Some((_, fixed)) => self.grip_factors().map(|f| (f, fixed)),
            None => {
                let (c, _) = self.base?;
                self.live_factor().map(|f| (DVec3::splat(f), c))
            }
        }
    }

    fn live_factor(&self) -> Option<f64> {
        let (c, _) = self.base?;
        let g = self.grip?;
        let r0 = (g - c).length();
        let h = self.c.hover?;
        (r0 > 1e-9).then(|| (h - c).length() / r0)
    }
}

impl Tool for ScaleTool {
    fn name(&self) -> &'static str {
        "Scale"
    }

    fn use_pivot(&mut self, pivot: Option<DVec3>) {
        self.pivot = pivot;
        self.base = None;
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        self.c.items = items.to_vec();
    }

    fn pauses(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        self.measure(ctx);
        if let Some(out) = self.c.after(input) {
            return out;
        }
        match input {
            Input::Hover(inf) => {
                self.c.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Click(inf) => match self.grip {
                None => {
                    if !self.c.take_items(inf.entity) {
                        return Outcome::Message("Click an Edge or Face".into());
                    }
                    if self.c.done.is_some() {
                        self.base = None;
                        self.c.done = None;
                        self.c.shown = false;
                        self.measure(ctx);
                    }
                    // A click on a box grip drags that grip, the opposite
                    // one (or the centre, with Around Centre) staying put;
                    // anywhere else scales evenly about the centre.
                    let near = GRIP_PX * ctx.pixel.max(1e-9);
                    let hit = self
                        .grips()
                        .into_iter()
                        .filter(|(_, p)| p.distance(inf.point) <= near)
                        .min_by(|a, b| a.1.distance(inf.point).total_cmp(&b.1.distance(inf.point)));
                    match hit {
                        Some((side, p)) => {
                            let fixed = if self.c.around_centre {
                                self.base.map(|(c, _)| c)
                            } else {
                                self.opposite(side)
                            };
                            self.grip = Some(p);
                            self.held = fixed.map(|f| (side, f));
                        }
                        None => {
                            self.grip = Some(inf.point);
                            self.held = None;
                        }
                    }
                    Outcome::Nothing
                }
                Some(_) => match self.live() {
                    Some((f, fixed)) if f.min_element() > 1e-9 => {
                        self.c.done = None;
                        let held = self.held.take();
                        if held.is_some() {
                            self.apply_about(f, fixed)
                        } else {
                            self.apply(f)
                        }
                    }
                    _ => Outcome::Nothing,
                },
            },
            Input::Typed(TypedValue::Length(f)) => {
                if f <= 0.0 {
                    return Outcome::Message("The factor must be above 0".into());
                }
                self.apply(DVec3::splat(f))
            }
            Input::Typed(_) => {
                Outcome::Message("Type a factor, or sizes in Operation Inputs".into())
            }
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        self.held = None;
        self.grip.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        self.grip
    }

    /// While scaling, the scaled items and a band from the fixed point;
    /// before, the box grips as small squares.
    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        if self.grip.is_none() {
            let r = GRIP_DRAW_PX * ctx.pixel.max(1e-9);
            let (u, v) = (self.frame.col(0) * r, self.frame.col(1) * r);
            let lines = self
                .grips()
                .into_iter()
                .flat_map(|(_, p)| {
                    let c = [p - u - v, p + u - v, p + u + v, p - u + v];
                    (0..4).map(move |i| [c[i], c[(i + 1) % 4]])
                })
                .collect();
            return Preview {
                lines,
                band: None,
                note: None,
            };
        }
        let Some((factors, center)) = self.live() else {
            return Preview::default();
        };
        let t = Transform::Scale {
            center,
            factors,
            axes: self.frame,
        };
        self.c
            .preview(ctx, Some(t), self.c.hover.map(|h| [center, h]))
    }

    fn status(&self) -> String {
        if self.grip.is_some() {
            "Move to Scale and Click, or Type a Factor".into()
        } else if self.c.items.is_empty() {
            "Click an Edge or Face to Start".into()
        } else {
            "Drag a Box Grip, Click a Point to Scale From, or Type Sizes".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        let Some((_, size)) = self.base else {
            return vec![
                Field::length("X", None, false, false),
                Field::length("Y", None, false, false),
                Field::length("Z", None, false, false),
                Field::switch("Lock Proportions", self.lock, true),
                self.c.around_field(),
            ];
        };
        let f = self.live().map_or_else(|| self.factors(), |(f, _)| f);
        let live = self.grip.is_some();
        vec![
            Field::length("X", Some(size.x * f.x), live, size.x > 1e-9),
            Field::length("Y", Some(size.y * f.y), live, size.y > 1e-9),
            Field::length("Z", Some(size.z * f.z), live, size.z > 1e-9),
            Field::switch("Lock Proportions", self.lock, true),
            self.c.around_field(),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        self.measure(ctx);
        if index == 3 {
            self.lock = value != 0.0;
            return Outcome::Nothing;
        }
        if index == 4 {
            let (out, c) = self.c.set_around(value != 0.0, ctx);
            if let (Some(c), Some((_, size))) = (c, self.base) {
                self.base = Some((c, size));
            }
            return out;
        }
        let Some((_, size)) = self.base else {
            return Outcome::Nothing;
        };
        if index > 2 || size[index] <= 1e-9 || value <= 0.0 {
            return Outcome::Nothing;
        }
        let f = value / size[index];
        let mut factors = self.factors();
        if self.lock {
            factors = DVec3::splat(f);
        } else {
            factors[index] = f;
        }
        self.apply(factors)
    }

    fn accept(&mut self) {
        self.c.accept();
    }

    fn reopen(&mut self) -> bool {
        self.c.reopen()
    }

    fn forget(&mut self) {
        self.c.forget();
    }
}

// ----- Mirror -----------------------------------------------------------------

/// The Mirror tool.
#[derive(Debug, Default)]
pub struct MirrorTool {
    c: Common,
    pivot: Option<DVec3>,
    /// Which drawing axis is the mirror plane's normal: 0 red, 1 green, 2 blue.
    axis: usize,
    /// The plane is given by three clicked points instead.
    three_points: bool,
    /// The points clicked so far in three-point mode.
    points: Vec<DVec3>,
}

impl MirrorTool {
    fn normal(&self, ctx: &Ctx<'_>) -> DVec3 {
        [ctx.axes.x, ctx.axes.y, ctx.axes.z][self.axis.min(2)]
    }
}

impl Tool for MirrorTool {
    fn name(&self) -> &'static str {
        "Mirror"
    }

    fn use_pivot(&mut self, pivot: Option<DVec3>) {
        self.pivot = pivot;
    }

    fn use_selection(&mut self, items: &[Pickable]) {
        self.c.items = items.to_vec();
    }

    fn pauses(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        if let Some(out) = self.c.after(input) {
            return out;
        }
        match input {
            Input::Hover(inf) => {
                self.c.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Click(inf) if self.three_points => {
                if self.points.is_empty() {
                    if !self.c.take_items(inf.entity) {
                        return Outcome::Message("Click an Edge or Face".into());
                    }
                    self.c.done = None;
                }
                self.points.push(inf.point);
                if self.points.len() < 3 {
                    return Outcome::Nothing;
                }
                let p = std::mem::take(&mut self.points);
                let n = (p[1] - p[0]).cross(p[2] - p[0]);
                if n.length() < 1e-9 {
                    return Outcome::Message("The three points are in a line".into());
                }
                self.c.commit(Transform::Mirror {
                    point: p[0],
                    normal: n.normalize(),
                })
            }
            Input::Click(inf) => {
                // A click on a face of the items themselves picks the plane
                // through the point; on another face, that face's plane.
                let own = inf.entity.is_some_and(|p| self.c.items.contains(&p));
                let normal = face_normal(&inf, ctx.entities)
                    .filter(|_| !own && !self.c.items.is_empty() && self.pivot.is_none())
                    .unwrap_or_else(|| self.normal(ctx));
                if !self.c.take_items(inf.entity) {
                    return Outcome::Message("Click an Edge or Face".into());
                }
                self.c.done = None;
                let point = self.c.about(self.pivot.unwrap_or(inf.point), ctx);
                self.c.commit(Transform::Mirror { point, normal })
            }
            Input::Typed(_) => Outcome::Message("Click a Face or a Point".into()),
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        let busy = !self.points.is_empty();
        self.points.clear();
        busy
    }

    fn last_point(&self) -> Option<DVec3> {
        self.points.last().copied()
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        let Some(h) = self.c.hover.filter(|_| !self.c.items.is_empty()) else {
            return Preview::default();
        };
        if self.three_points {
            // The points so far and the cursor; with two, the mirrored shape.
            let mut pts = self.points.clone();
            pts.push(h);
            let mut lines: Vec<[DVec3; 2]> = pts.windows(2).map(|w| [w[0], w[1]]).collect();
            if pts.len() == 3 {
                lines.push([pts[2], pts[0]]);
                let n = (pts[1] - pts[0]).cross(pts[2] - pts[0]);
                if n.length() > 1e-9 {
                    let t = Transform::Mirror {
                        point: pts[0],
                        normal: n.normalize(),
                    };
                    let mut p = self.c.preview(ctx, Some(t), None);
                    p.lines.extend(lines);
                    return p;
                }
            }
            return Preview {
                lines,
                band: None,
                note: None,
            };
        }
        let t = Transform::Mirror {
            point: self.pivot.unwrap_or(h),
            normal: self.normal(ctx),
        };
        self.c.preview(ctx, Some(t), None)
    }

    fn status(&self) -> String {
        if self.three_points {
            format!(
                "Click Point {} of 3 on the Mirror Plane",
                self.points.len() + 1
            )
        } else {
            "Click a Face to Mirror in it, or a Point for the Chosen Plane".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::switch("X", self.axis == 0, true),
            Field::switch("Y", self.axis == 1, true),
            Field::switch("Z", self.axis == 2, true),
            Field::switch("3 Points", self.three_points, true),
            Field::switch("Copy", self.c.copy, true),
            self.c.around_field(),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        match index {
            0..=2 if value != 0.0 => {
                self.axis = index;
                match self.c.shown_done() {
                    Some(Transform::Mirror { point, .. }) => self.c.redefine(Transform::Mirror {
                        point,
                        normal: self.normal(ctx),
                    }),
                    _ => Outcome::Nothing,
                }
            }
            3 => {
                self.three_points = value != 0.0;
                self.points.clear();
                Outcome::Nothing
            }
            4 if (value != 0.0) != self.c.copy => self.handle(Input::Toggle, ctx),
            5 => self.c.set_around(value != 0.0, ctx).0,
            _ => Outcome::Nothing,
        }
    }

    fn accept(&mut self) {
        self.c.accept();
    }

    fn reopen(&mut self) -> bool {
        self.c.reopen()
    }

    fn forget(&mut self) {
        self.c.forget();
    }
}

#[cfg(test)]
mod tests {
    use smith_infer::infer::{DrawingAxes, Inference, Kind};
    use smith_model::Entities;

    use super::*;

    fn inf(point: DVec3, entity: Option<Pickable>) -> Inference {
        Inference {
            point,
            kind: Kind::Free,
            from: None,
            from2: None,
            entity,
            locked: false,
            normal: None,
        }
    }

    #[test]
    fn mirror_through_three_points() {
        let e = square();
        let axes = DrawingAxes::default();
        let ctx = crate::testing::ctx(&e, &axes);
        let mut t = MirrorTool::default();
        let face = e.face_ids().next().unwrap();
        t.edit(3, 1.0, &ctx);
        // A vertical plane x = 50 through three points.
        for p in [DVec3::new(50.0, 0.0, 0.0), DVec3::new(50.0, 10.0, 0.0)] {
            let pick = if p.y == 0.0 {
                Some(Pickable::Face(face))
            } else {
                None
            };
            assert!(matches!(
                t.handle(Input::Click(inf(p, pick)), &ctx),
                Outcome::Nothing
            ));
        }
        match t.handle(Input::Click(inf(DVec3::new(50.0, 0.0, 5.0), None)), &ctx) {
            Outcome::Commit(smith_ops::draw::DrawOp::Transform {
                transform: Transform::Mirror { point, normal },
                ..
            }) => {
                assert!((point.x - 50.0).abs() < 1e-12);
                assert!((normal.x.abs() - 1.0).abs() < 1e-12);
            }
            other => panic!("{other:?}"),
        }
    }

    fn square() -> Entities {
        let mut e = Entities::new();
        e.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(40.0, 0.0, 0.0),
                DVec3::new(40.0, 20.0, 0.0),
                DVec3::new(0.0, 20.0, 0.0),
            ],
            true,
        )
        .unwrap();
        e
    }

    #[test]
    fn rotate_three_clicks_snaps_to_fifteen_degrees() {
        let e = square();
        let f = e.face_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let ctx = Ctx {
            entities: &e,
            axes: &axes,
            instances: &smith_ops::modify::InstanceBoxes::new(),
            instance_lines: &smith_ops::modify::InstanceLines::new(),
            pixel: 0.01,
            cursor: smith_geom::DVec2::ZERO,
        };
        let mut t = RotateTool::default();
        t.use_selection(&[Pickable::Face(f)]);
        t.handle(Input::Click(inf(DVec3::ZERO, None)), &ctx);
        t.handle(Input::Click(inf(DVec3::X * 10.0, None)), &ctx);
        let a = 31f64.to_radians();
        let out = t.handle(
            Input::Click(inf(DVec3::new(a.cos(), a.sin(), 0.0) * 10.0, None)),
            &ctx,
        );
        match out {
            Outcome::Commit(DrawOp::Transform {
                transform: Transform::Rotate { angle, .. },
                ..
            }) => assert!((angle.to_degrees() - 30.0).abs() < 1e-9),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn dragging_a_face_grip_stretches_one_axis_from_the_opposite_side() {
        let e = square();
        let f = e.face_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let ctx = Ctx {
            entities: &e,
            axes: &axes,
            instances: &smith_ops::modify::InstanceBoxes::new(),
            instance_lines: &smith_ops::modify::InstanceLines::new(),
            pixel: 0.01,
            cursor: smith_geom::DVec2::ZERO,
        };
        let mut t = ScaleTool::default();
        t.use_selection(&[Pickable::Face(f)]);
        // A flat square: 8 grips (4 corners, 4 edge middles).
        t.handle(Input::Hover(inf(DVec3::new(90.0, 90.0, 0.0), None)), &ctx);
        assert_eq!(t.grips().len(), 8);
        // The middle of the right side, dragged from x 40 to x 60.
        t.handle(Input::Click(inf(DVec3::new(40.0, 10.0, 0.0), None)), &ctx);
        t.handle(Input::Hover(inf(DVec3::new(60.0, 10.0, 0.0), None)), &ctx);
        let out = t.handle(Input::Click(inf(DVec3::new(60.0, 10.0, 0.0), None)), &ctx);
        let Outcome::Commit(DrawOp::Transform {
            transform: Transform::Scale {
                center, factors, ..
            },
            ..
        }) = out
        else {
            panic!("no scale: {out:?}");
        };
        assert_eq!(center, DVec3::new(0.0, 10.0, 0.0));
        assert!((factors - DVec3::new(1.5, 1.0, 1.0)).length() < 1e-12);
    }

    #[test]
    fn around_centre_redoes_a_rotation_about_the_items_centre() {
        let e = square();
        let f = e.face_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let ctx = Ctx {
            entities: &e,
            axes: &axes,
            instances: &smith_ops::modify::InstanceBoxes::new(),
            instance_lines: &smith_ops::modify::InstanceLines::new(),
            pixel: 0.01,
            cursor: smith_geom::DVec2::ZERO,
        };
        let centre = items_bounds_in(&e, &[Pickable::Face(f)], ctx.instances, DMat3::IDENTITY)
            .map(|(lo, hi)| (lo + hi) * 0.5)
            .unwrap();
        let mut t = RotateTool::default();
        t.use_selection(&[Pickable::Face(f)]);
        t.use_pivot(Some(DVec3::ZERO));
        t.handle(
            Input::Click(inf(DVec3::ZERO, Some(Pickable::Face(f)))),
            &ctx,
        );
        t.handle(Input::Click(inf(DVec3::new(40.0, 0.0, 0.0), None)), &ctx);
        let out = t.handle(Input::Click(inf(DVec3::new(0.0, 40.0, 0.0), None)), &ctx);
        assert!(matches!(out, Outcome::Commit(_)));
        // Switched after the operation: the same turn, about the centre.
        let out = t.edit(3, 1.0, &ctx);
        let Outcome::Redefine(DrawOp::Transform {
            transform: Transform::Rotate { center, angle, .. },
            ..
        }) = out
        else {
            panic!("not redone: {out:?}");
        };
        assert_eq!(center, centre);
        assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn rotate_around_a_pivot_turns_a_rectangle() {
        let mut e = square();
        let f = e.face_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let mut t = RotateTool::default();
        t.use_selection(&[Pickable::Face(f)]);
        t.use_pivot(Some(DVec3::ZERO));
        let out = {
            let ctx = Ctx {
                entities: &e,
                axes: &axes,
                instances: &smith_ops::modify::InstanceBoxes::new(),
                instance_lines: &smith_ops::modify::InstanceLines::new(),
                pixel: 0.01,
                cursor: smith_geom::DVec2::ZERO,
            };
            // A first click on the Pivot only confirms it.
            t.handle(
                Input::Click(inf(DVec3::ZERO, Some(Pickable::Face(f)))),
                &ctx,
            );
            t.handle(
                Input::Click(inf(DVec3::new(40.0, 0.0, 0.0), Some(Pickable::Face(f)))),
                &ctx,
            );
            t.handle(Input::Click(inf(DVec3::new(0.0, 40.0, 0.0), None)), &ctx)
        };
        let Outcome::Commit(op) = out else {
            panic!("no rotate: {out:?}");
        };
        smith_ops::draw::draw(&mut e, &op).unwrap();
        // Turned 90° about the origin: (40, 0) goes to (0, 40).
        assert!(e.find_vertex(DVec3::new(-20.0, 40.0, 0.0)).is_some());
    }

    #[test]
    fn typed_resize_with_and_without_the_lock() {
        let e = square();
        let f = e.face_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let ctx = Ctx {
            entities: &e,
            axes: &axes,
            instances: &smith_ops::modify::InstanceBoxes::new(),
            instance_lines: &smith_ops::modify::InstanceLines::new(),
            pixel: 0.01,
            cursor: smith_geom::DVec2::ZERO,
        };
        let mut t = ScaleTool::default();
        t.use_selection(&[Pickable::Face(f)]);
        let out = t.edit(0, 60.0, &ctx);
        match out {
            Outcome::Commit(DrawOp::Transform {
                transform:
                    Transform::Scale {
                        center, factors, ..
                    },
                ..
            }) => {
                assert!((factors - DVec3::splat(1.5)).length() < 1e-12);
                assert!((center - DVec3::new(20.0, 10.0, 0.0)).length() < 1e-12);
            }
            other => panic!("{other:?}"),
        }
        t.edit(3, 0.0, &ctx);
        let out = t.edit(1, 20.0, &ctx);
        match out {
            Outcome::Redefine(DrawOp::Transform {
                transform: Transform::Scale { factors, .. },
                ..
            }) => assert!((factors - DVec3::new(1.5, 1.0, 1.5)).length() < 1e-12),
            other => panic!("{other:?}"),
        }
    }
}
