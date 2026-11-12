//! SketchSmith — Offset (Inset), Divide, Loop Cut, Knife, Slide and Dissolve tools | v1.0.0 | Thursday 12 November 2026
//!
//! Neither pauses: the last operation stays adjustable in Operation Inputs
//! until the next one starts.
//!
//! * **Offset (Inset)** — click a face, move towards its middle (or away) and
//!   click, or type the distance (positive goes in). `3x` afterwards makes
//!   three offsets one step apart. Operation Inputs: Distance and Count.
//! * **Divide** — click an edge: it splits in two, measured from the end
//!   nearer the click. Then type the number of parts (`/5` or `5x`), or a
//!   length to place a point every that far. Operation Inputs: Parts, Every
//!   (with Even Ends to split the remainder between both ends), and At (one
//!   point at a distance).
//! * **Loop Cut** — hover an edge between four-sided faces to see the ring
//!   it cuts, click to cut it; type a number for that many even cuts.
//!   Operation Inputs: Cuts.
//! * **Knife** — click points across the model; the cut follows the line
//!   over every face turned towards you, around corners. Enter cuts; Esc
//!   abandons. Cut Through (Operation Inputs, or Ctrl) also cuts the faces
//!   behind. Each segment snaps to 0°, 45°, 90° … from the drawing axes as
//!   seen, and typing a length places the next point exactly that far.
//! * **Slide** — click a point (it snaps to end points) to slide it along
//!   one of its edges, or the middle of an edge to slide the edge along the
//!   edges at its ends; move and click, or type the distance. Attached faces
//!   must stay flat. While moving, the slid edges show where they would go.
//! * **Dissolve** — click a point between two edges in a straight line to
//!   remove it, joining the edges.

use smith_geom::DVec3;
use smith_infer::infer::Kind;
use smith_infer::units::TypedValue;
use smith_model::{EdgeId, FaceId, Pickable, VertexId};
use smith_ops::cut::{Spacing, View, knife_lines, loop_cut_lines};
use smith_ops::draw::DrawOp;
use smith_ops::shape::{
    Division, SlideTarget, chain_side, division_points, edge_slide_rails, offset_chain_preview,
    offset_preview, slide_targets,
};

use crate::{Ctx, Field, Input, Outcome, Preview, Tool, as_count};

// ----- Offset (Inset) ---------------------------------------------------------

/// What an offset applies to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum OffsetTarget {
    /// A face's outline and holes.
    Face(FaceId),
    /// The chain of edges through an edge, in the plane with this normal
    /// when it is a straight line.
    Chain(EdgeId, DVec3),
}

/// The last offset, kept adjustable in Operation Inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
struct OffsetDone {
    target: OffsetTarget,
    distance: f64,
    count: u32,
}

impl OffsetDone {
    fn op(self) -> DrawOp {
        match self.target {
            OffsetTarget::Face(face) => DrawOp::Offset {
                face,
                distance: self.distance,
                count: self.count,
            },
            OffsetTarget::Chain(edge, normal) => DrawOp::OffsetChain {
                edge,
                distance: self.distance,
                count: self.count,
                normal,
            },
        }
    }
}

/// The Offset (Inset) tool: a face's outline and holes, or the chain of
/// edges through a clicked edge, to the side the cursor is on.
#[derive(Debug, Default)]
pub struct OffsetTool {
    /// What is being offset, and where it was clicked; for a face, also its
    /// outline's centre (moving towards it offsets inward).
    target: Option<(OffsetTarget, DVec3, DVec3)>,
    hover: Option<DVec3>,
    /// The distance the cursor set at its last move, for Operation Inputs.
    hover_distance: Option<f64>,
    done: Option<OffsetDone>,
    shown: bool,
}

impl OffsetTool {
    fn shown_done(&self) -> Option<OffsetDone> {
        self.done.filter(|_| self.shown)
    }

    /// The distance the cursor sets. For a face: how far it has moved from
    /// the click, positive towards the face's middle. For a chain: its
    /// distance from the chain, positive on the chain's left.
    fn live(&self, ctx: &Ctx<'_>) -> Option<f64> {
        let (target, start, centre) = self.target?;
        let h = self.hover?;
        match target {
            OffsetTarget::Face(_) => {
                let d = h.distance(start);
                Some(if h.distance(centre) < start.distance(centre) {
                    d
                } else {
                    -d
                })
            }
            OffsetTarget::Chain(edge, normal) => chain_side(ctx.entities, edge, h, normal),
        }
    }

    fn set_distance(&mut self, distance: f64) -> Outcome {
        if let Some((target, _, _)) = self.target.take() {
            let done = OffsetDone {
                target,
                distance,
                count: 1,
            };
            self.done = Some(done);
            self.shown = true;
            return Outcome::Commit(done.op());
        }
        match self.shown_done() {
            Some(d) => self.redefine(OffsetDone { distance, ..d }),
            None => Outcome::Message("Click a Face or an Edge".into()),
        }
    }

    fn redefine(&mut self, d: OffsetDone) -> Outcome {
        self.done = Some(d);
        self.shown = true;
        Outcome::Redefine(d.op())
    }
}

impl Tool for OffsetTool {
    fn name(&self) -> &'static str {
        "Offset (Inset)"
    }

    fn pauses(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                self.hover_distance = self.live(ctx);
                Outcome::Nothing
            }
            Input::Click(inf) => match self.target {
                None => match inf.entity {
                    Some(Pickable::Face(f)) => {
                        let Some(face) = ctx.entities.face(f) else {
                            return Outcome::Nothing;
                        };
                        let pts = ctx.entities.loop_points(face.outer());
                        let centre = pts.iter().copied().sum::<DVec3>() / pts.len().max(1) as f64;
                        self.done = None;
                        self.shown = false;
                        self.target = Some((OffsetTarget::Face(f), inf.point, centre));
                        self.hover = Some(inf.point);
                        Outcome::Nothing
                    }
                    Some(Pickable::Edge(e)) => {
                        self.done = None;
                        self.shown = false;
                        self.target =
                            Some((OffsetTarget::Chain(e, ctx.axes.z), inf.point, inf.point));
                        self.hover = Some(inf.point);
                        Outcome::Nothing
                    }
                    _ => Outcome::Message("Click a Face or an Edge".into()),
                },
                Some(_) => match self.live(ctx) {
                    Some(d) if d.abs() > 1e-9 => self.set_distance(d),
                    _ => Outcome::Nothing,
                },
            },
            Input::Typed(TypedValue::Length(d)) => self.set_distance(d),
            Input::Typed(TypedValue::Copies(n)) => match self.shown_done() {
                Some(d) if n >= 1 => self.redefine(OffsetDone { count: n, ..d }),
                _ => Outcome::Message("Offset first, then type the count".into()),
            },
            Input::Typed(_) => Outcome::Message("Type a distance".into()),
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        self.target.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        self.target.map(|(_, p, _)| p)
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        let (Some((target, start, _)), Some(d)) = (self.target, self.live(ctx)) else {
            return Preview::default();
        };
        let lines = match target {
            OffsetTarget::Face(f) => offset_preview(ctx.entities, f, d)
                .iter()
                .flat_map(|pts| {
                    let k = pts.len();
                    (0..k).map(move |i| [pts[i], pts[(i + 1) % k]])
                })
                .collect(),
            OffsetTarget::Chain(edge, normal) => {
                offset_chain_preview(ctx.entities, edge, d, normal)
            }
        };
        Preview {
            lines,
            band: self.hover.map(|h| [start, h]),
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.target.is_some() {
            "Move In or Out and Click, or Type the Distance".into()
        } else {
            "Click a Face or an Edge".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        match (self.target.is_some(), self.shown_done()) {
            (true, _) => vec![
                Field::length("Distance", self.hover_distance, true, true),
                Field::count("Count", 1, false),
            ],
            (false, Some(d)) => vec![
                Field::length("Distance", Some(d.distance), false, true),
                Field::count("Count", d.count, true),
            ],
            (false, None) => vec![
                Field::length("Distance", None, false, false),
                Field::count("Count", 1, false),
            ],
        }
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 => self.set_distance(value),
            1 => self.handle(Input::Typed(TypedValue::Copies(as_count(value))), ctx),
            _ => Outcome::Nothing,
        }
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

// ----- Divide -------------------------------------------------------------------

/// A committed division.
#[derive(Debug, Clone, Copy, PartialEq)]
struct DivideDone {
    edge: EdgeId,
    from_start: bool,
    division: Division,
}

impl DivideDone {
    fn op(self) -> DrawOp {
        DrawOp::Divide {
            edge: self.edge,
            from_start: self.from_start,
            division: self.division,
        }
    }
}

/// The Divide tool.
#[derive(Debug, Default)]
pub struct DivideTool {
    hover: Option<(EdgeId, DVec3)>,
    done: Option<DivideDone>,
    shown: bool,
    even_ends: bool,
}

impl DivideTool {
    fn shown_done(&self) -> Option<DivideDone> {
        self.done.filter(|_| self.shown)
    }

    fn redefine(&mut self, division: Division) -> Outcome {
        match self.shown_done() {
            Some(d) => {
                let d = DivideDone { division, ..d };
                self.done = Some(d);
                Outcome::Redefine(d.op())
            }
            None => Outcome::Message("Click an Edge".into()),
        }
    }
}

/// Whether the click at `p` is nearer the edge's start than its end.
fn nearer_start(ctx: &Ctx<'_>, e: EdgeId, p: DVec3) -> bool {
    ctx.entities
        .edge_points(e)
        .is_none_or(|(a, b)| p.distance(a) <= p.distance(b))
}

impl Tool for DivideTool {
    fn name(&self) -> &'static str {
        "Divide"
    }

    fn pauses(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = match inf.entity {
                    Some(Pickable::Edge(e)) => Some((e, inf.point)),
                    _ => None,
                };
                Outcome::Nothing
            }
            Input::Click(inf) => match inf.entity {
                Some(Pickable::Edge(e)) => {
                    let d = DivideDone {
                        edge: e,
                        from_start: nearer_start(ctx, e, inf.point),
                        division: Division::Parts(2),
                    };
                    self.done = Some(d);
                    self.shown = true;
                    Outcome::Commit(d.op())
                }
                _ => Outcome::Message("Click an Edge".into()),
            },
            Input::Typed(
                TypedValue::Divide(n) | TypedValue::Copies(n) | TypedValue::Segments(n),
            ) if n >= 2 => self.redefine(Division::Parts(n)),
            Input::Typed(TypedValue::Length(l)) => self.redefine(Division::Every {
                length: l,
                even_ends: self.even_ends,
            }),
            Input::Typed(_) => Outcome::Message("Type the parts (/5) or a length".into()),
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        false
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        let Some((e, p)) = self.hover else {
            return Preview::default();
        };
        let Some((a, b)) = ctx.entities.edge_points(e) else {
            return Preview::default();
        };
        let (from, to) = if nearer_start(ctx, e, p) {
            (a, b)
        } else {
            (b, a)
        };
        let len = from.distance(to);
        let dir = (to - from) / len;
        let side = dir.any_orthonormal_vector() * (ctx.pixel * 6.0).max(1e-6);
        let lines = division_points(len, Division::Parts(2))
            .unwrap_or_default()
            .into_iter()
            .map(|t| from + dir * t)
            .map(|q| [q - side, q + side])
            .collect();
        Preview {
            lines,
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        "Click an Edge, then Type the Parts or a Length".into()
    }

    fn fields(&self) -> Vec<Field> {
        let d = self.shown_done();
        let editable = d.is_some();
        let (parts, every, at) = match d.map(|d| d.division) {
            Some(Division::Parts(n)) => (n, None, None),
            Some(Division::Every { length, .. }) => (0, Some(length), None),
            Some(Division::At(x)) => (0, None, Some(x)),
            None => (2, None, None),
        };
        vec![
            Field::count("Parts", parts, editable),
            Field::length("Every", every, false, editable),
            Field::switch("Even Ends", self.even_ends, true),
            Field::length("At", at, false, editable),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 if as_count(value) >= 2 => self.redefine(Division::Parts(as_count(value))),
            1 if value > 0.0 => self.redefine(Division::Every {
                length: value,
                even_ends: self.even_ends,
            }),
            2 => {
                self.even_ends = value != 0.0;
                match self.shown_done().map(|d| d.division) {
                    Some(Division::Every { length, .. }) => self.redefine(Division::Every {
                        length,
                        even_ends: self.even_ends,
                    }),
                    _ => Outcome::Nothing,
                }
            }
            3 if value > 0.0 => self.redefine(Division::At(value)),
            _ => Outcome::Nothing,
        }
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

// ----- Loop Cut -------------------------------------------------------------------

/// The Loop Cut tool.
#[derive(Debug)]
pub struct LoopCutTool {
    hover: Option<EdgeId>,
    cuts: u32,
    /// Even cuts, or measured from one end of the side edges.
    spacing: Spacing,
    done: Option<EdgeId>,
    shown: bool,
}

impl Default for LoopCutTool {
    fn default() -> Self {
        Self {
            hover: None,
            cuts: 1,
            spacing: Spacing::default(),
            done: None,
            shown: false,
        }
    }
}

impl LoopCutTool {
    fn set_cuts(&mut self, n: u32) -> Outcome {
        if n < 1 {
            return Outcome::Nothing;
        }
        self.cuts = n;
        self.redefine()
    }

    /// Re-does the last cut with the current values, while it is shown.
    fn redefine(&self) -> Outcome {
        match self.done.filter(|_| self.shown) {
            Some(edge) => Outcome::Redefine(DrawOp::LoopCut {
                edge,
                cuts: self.cuts,
                spacing: self.spacing,
            }),
            None => Outcome::Nothing,
        }
    }
}

impl Tool for LoopCutTool {
    fn name(&self) -> &'static str {
        "Loop Cut"
    }

    fn pauses(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, _ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = match inf.entity {
                    Some(Pickable::Edge(e)) => Some(e),
                    _ => None,
                };
                Outcome::Nothing
            }
            Input::Click(inf) => match inf.entity {
                Some(Pickable::Edge(edge)) => {
                    self.done = Some(edge);
                    self.shown = true;
                    Outcome::Commit(DrawOp::LoopCut {
                        edge,
                        cuts: self.cuts,
                        spacing: self.spacing,
                    })
                }
                _ => Outcome::Message("Click an Edge Between Four-Sided Faces".into()),
            },
            Input::Typed(
                TypedValue::Divide(n) | TypedValue::Copies(n) | TypedValue::Segments(n),
            ) => self.set_cuts(n),
            Input::Typed(TypedValue::Length(n)) => self.set_cuts(as_count(n)),
            Input::Typed(_) => Outcome::Message("Type the number of cuts".into()),
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        false
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        Preview {
            lines: self
                .hover
                .map(|e| loop_cut_lines(ctx.entities, e, self.cuts, self.spacing))
                .unwrap_or_default(),
            band: None,
            note: None,
        }
    }

    fn status(&self) -> String {
        "Hover an Edge to See the Ring, Click to Cut · Type the Number of Cuts".into()
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::count("Cuts", self.cuts, true),
            Field::length("Every", Some(self.spacing.every), false, true),
            Field::switch("From End", self.spacing.from_end, true),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 => self.set_cuts(as_count(value)),
            // 0 goes back to even cuts.
            1 if value >= 0.0 => {
                self.spacing.every = value;
                self.redefine()
            }
            2 => {
                self.spacing.from_end = value != 0.0;
                self.redefine()
            }
            _ => Outcome::Nothing,
        }
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

// ----- Knife ------------------------------------------------------------------------

/// Knife angle snapping: the cut snaps to every 45° step (0°, 45°, 90°, …)
/// within this many degrees of it.
const KNIFE_SNAP_DEG: f64 = 3.0;

/// The Knife tool. Each segment after the first point snaps to 0°, 45°,
/// 90° and so on from the drawing axes' X, as seen (within
/// [`KNIFE_SNAP_DEG`]); a point picked on a vertex, an edge or another
/// snap is kept as it is. Typing a length places the next point exactly
/// that far along the segment shown.
#[derive(Debug, Default)]
pub struct KnifeTool {
    points: Vec<DVec3>,
    /// The cursor's point, after angle snapping.
    hover: Option<DVec3>,
    view: Option<View>,
    through: bool,
}

impl KnifeTool {
    /// `p` with the segment from `from` turned onto the nearest 45° step
    /// when it is within [`KNIFE_SNAP_DEG`], measured in the plane square to
    /// the view from the drawing axes' X (Y when X points along the view).
    fn snap_angle(&self, from: DVec3, p: DVec3, ctx: &Ctx<'_>) -> DVec3 {
        let Some(view) = self.view else {
            return p;
        };
        let f = view.forward;
        let flat = |v: DVec3| v - f * v.dot(f);
        let Some(x) = flat(ctx.axes.x)
            .try_normalize()
            .or_else(|| flat(ctx.axes.y).try_normalize())
        else {
            return p;
        };
        let y = f.cross(x);
        let d = p - from;
        let (along, depth) = (flat(d), f * d.dot(f));
        let len = along.length();
        if len < 1e-9 {
            return p;
        }
        let angle = along.dot(y).atan2(along.dot(x)).to_degrees();
        let step = (angle / 45.0).round() * 45.0;
        if (angle - step).abs() > KNIFE_SNAP_DEG {
            return p;
        }
        let r = step.to_radians();
        from + (x * r.cos() + y * r.sin()) * len + depth
    }

    /// The point the cursor sets: snapped in angle unless it is on a snap
    /// point already.
    fn place(&self, inf: &smith_infer::infer::Inference, ctx: &Ctx<'_>) -> DVec3 {
        match (self.points.last(), inf.kind) {
            (Some(from), Kind::Free | Kind::OnFace) => self.snap_angle(*from, inf.point, ctx),
            _ => inf.point,
        }
    }

    /// Places the next point `length` along the segment shown.
    fn add_length(&mut self, length: f64) -> Outcome {
        let (Some(from), Some(h)) = (self.points.last().copied(), self.hover) else {
            return Outcome::Message("Click to Start the Cut".into());
        };
        match (h - from).try_normalize() {
            Some(d) => {
                self.points.push(from + d * length);
                Outcome::Nothing
            }
            None => Outcome::Message("Move the Cursor to Show the Direction".into()),
        }
    }
}

impl Tool for KnifeTool {
    fn name(&self) -> &'static str {
        "Knife"
    }

    fn pauses(&self) -> bool {
        false
    }

    fn use_view(&mut self, view: View) {
        self.view = Some(view);
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(self.place(&inf, ctx));
                Outcome::Nothing
            }
            Input::Click(inf) => {
                let p = self.place(&inf, ctx);
                self.points.push(p);
                Outcome::Nothing
            }
            Input::Finish => {
                let points = std::mem::take(&mut self.points);
                match self.view {
                    Some(view) if points.len() >= 2 => Outcome::Commit(DrawOp::Knife {
                        points,
                        view,
                        through: self.through,
                    }),
                    _ => Outcome::Nothing,
                }
            }
            Input::Toggle => {
                self.through = !self.through;
                Outcome::Nothing
            }
            Input::Typed(TypedValue::Length(l)) => self.add_length(l),
            Input::Typed(_) => Outcome::Message("Type a Length".into()),
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
        let mut pts = self.points.clone();
        if let Some(h) = self.hover.filter(|_| !pts.is_empty()) {
            pts.push(h);
        }
        let lines = match self.view {
            Some(view) => knife_lines(ctx.entities, &pts, &view, self.through),
            None => Vec::new(),
        };
        Preview {
            lines,
            band: self.points.last().zip(self.hover).map(|(a, b)| [*a, b]),
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.points.is_empty() {
            "Click to Start the Cut".into()
        } else {
            "Click the Next Point, or Type a Length · Enter Cuts · Ctrl: Cut Through".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        let live = self
            .points
            .last()
            .zip(self.hover)
            .map(|(a, b)| a.distance(b));
        vec![
            Field::length("Length", live, true, !self.points.is_empty()),
            Field::switch("Cut Through", self.through, true),
        ]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 => self.add_length(value),
            1 => {
                self.through = value != 0.0;
                Outcome::Nothing
            }
            _ => Outcome::Nothing,
        }
    }

    fn accept(&mut self) {}

    fn reopen(&mut self) -> bool {
        false
    }

    fn forget(&mut self) {}
}

#[cfg(test)]
mod knife_tests {
    use super::*;
    use crate::testing::{at, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    fn top_view() -> View {
        View {
            eye: v(0.0, 0.0, 100.0),
            forward: v(0.0, 0.0, -1.0),
            parallel: true,
        }
    }

    #[test]
    fn a_cut_near_45_degrees_snaps_to_it_and_far_from_it_does_not() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = KnifeTool::default();
        t.use_view(top_view());
        t.handle(Input::Click(at(v(0.0, 0.0, 0.0))), &c);
        // 43° pulls to 45°, keeping the length.
        let a = 43f64.to_radians();
        t.handle(Input::Hover(at(v(10.0 * a.cos(), 10.0 * a.sin(), 0.0))), &c);
        let h = t.hover.unwrap();
        let s = 0.5f64.sqrt() * 10.0;
        assert!((h - v(s, s, 0.0)).length() < 1e-9);
        // 30° is left alone.
        let a = 30f64.to_radians();
        let p = v(10.0 * a.cos(), 10.0 * a.sin(), 0.0);
        t.handle(Input::Hover(at(p)), &c);
        assert!((t.hover.unwrap() - p).length() < 1e-12);
    }

    #[test]
    fn a_typed_length_places_the_next_point_exactly() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = KnifeTool::default();
        t.use_view(top_view());
        t.handle(Input::Click(at(v(1.0, 1.0, 0.0))), &c);
        t.handle(Input::Hover(at(v(1.0, 7.3, 0.0))), &c);
        t.handle(Input::Typed(TypedValue::Length(2.5)), &c);
        // The 90° snap leaves a cos 90° rounding in X.
        let p = t.points.last().copied().unwrap();
        assert!((p - v(1.0, 3.5, 0.0)).length() < 1e-12);
    }
}

// ----- Slide ------------------------------------------------------------------------

/// What is being slid, and from where.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Sliding {
    Point { vertex: VertexId, at: DVec3 },
    Edge { edge: EdgeId, at: DVec3, dir: DVec3 },
}

/// The Slide tool.
#[derive(Debug, Default)]
pub struct SlideTool {
    sliding: Option<Sliding>,
    hover: Option<DVec3>,
    done: Option<SlideTarget>,
    shown: bool,
}

impl SlideTool {
    /// The target and distance the cursor sets.
    fn live(&self, ctx: &Ctx<'_>) -> Option<(SlideTarget, f64)> {
        let h = self.hover?;
        match self.sliding? {
            Sliding::Point { vertex, at } => {
                let v = ctx.entities.vertex(vertex)?;
                // The edge that runs most nearly towards the cursor.
                let (along, dir) = v
                    .edges()
                    .iter()
                    .filter_map(|e| {
                        let far = ctx.entities.vertex(ctx.entities.edge(*e)?.other(vertex))?;
                        Some((*e, (far.position() - at).normalize()))
                    })
                    .max_by(|x, y| x.1.dot(h - at).total_cmp(&y.1.dot(h - at)))?;
                Some((SlideTarget::Point { vertex, along }, (h - at).dot(dir)))
            }
            Sliding::Edge { edge, at, dir } => Some((SlideTarget::Edge(edge), (h - at).dot(dir))),
        }
    }
}

impl Tool for SlideTool {
    fn name(&self) -> &'static str {
        "Slide"
    }

    fn pauses(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Click(inf) => match self.sliding {
                None => {
                    let point = (inf.kind == Kind::Endpoint)
                        .then(|| ctx.entities.find_vertex(inf.point))
                        .flatten();
                    self.done = None;
                    self.shown = false;
                    if let Some(vertex) = point {
                        self.sliding = Some(Sliding::Point {
                            vertex,
                            at: inf.point,
                        });
                        return Outcome::Nothing;
                    }
                    match inf.entity {
                        Some(Pickable::Edge(edge)) => match edge_slide_rails(ctx.entities, edge) {
                            Some([(_, dir), _]) => {
                                self.sliding = Some(Sliding::Edge {
                                    edge,
                                    at: inf.point,
                                    dir,
                                });
                                Outcome::Nothing
                            }
                            None => Outcome::Message("Nothing to Slide Along".into()),
                        },
                        _ => Outcome::Message("Click a Point or an Edge".into()),
                    }
                }
                Some(_) => match self.live(ctx) {
                    Some((target, d)) if d.abs() > 1e-9 => {
                        self.sliding = None;
                        self.done = Some(target);
                        self.shown = true;
                        Outcome::Commit(DrawOp::Slide {
                            target,
                            distance: d,
                        })
                    }
                    _ => Outcome::Nothing,
                },
            },
            Input::Typed(TypedValue::Length(d)) => {
                match (self.live(ctx), self.done.filter(|_| self.shown)) {
                    (Some((target, _)), _) if self.sliding.is_some() => {
                        self.sliding = None;
                        self.done = Some(target);
                        self.shown = true;
                        Outcome::Commit(DrawOp::Slide {
                            target,
                            distance: d,
                        })
                    }
                    (_, Some(target)) => Outcome::Redefine(DrawOp::Slide {
                        target,
                        distance: d,
                    }),
                    _ => Outcome::Message("Click a Point or an Edge".into()),
                }
            }
            Input::Typed(_) => Outcome::Message("Type a distance".into()),
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        self.sliding.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        match self.sliding? {
            Sliding::Point { at, .. } | Sliding::Edge { at, .. } => Some(at),
        }
    }

    /// The band from the start to the cursor, and the slid geometry: every
    /// edge at a moved point, drawn to where the point would go.
    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        let (Some(start), Some(h)) = (self.last_point(), self.hover) else {
            return Preview::default();
        };
        let e = ctx.entities;
        let lines = self
            .live(ctx)
            .and_then(|(target, d)| slide_targets(e, target, d).ok())
            .map(|moved| {
                let at = |v| {
                    moved
                        .iter()
                        .find(|(m, _)| *m == v)
                        .map(|(_, p)| *p)
                        .or_else(|| e.vertex(v).map(|x| x.position()))
                };
                let mut lines = Vec::new();
                for (v, p) in &moved {
                    for edge in e.vertex(*v).map(|x| x.edges().to_vec()).unwrap_or_default() {
                        if let Some(q) = e.edge(edge).and_then(|x| at(x.other(*v))) {
                            lines.push([q, *p]);
                        }
                    }
                }
                lines
            })
            .unwrap_or_default();
        Preview {
            lines,
            band: Some([start, h]),
            note: None,
        }
    }

    fn status(&self) -> String {
        if self.sliding.is_some() {
            "Move and Click, or Type the Distance".into()
        } else {
            "Click a Point or an Edge to Slide".into()
        }
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::length("Distance", None, false, self.shown)]
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        if index == 0 {
            self.handle(Input::Typed(TypedValue::Length(value)), ctx)
        } else {
            Outcome::Nothing
        }
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

// ----- Dissolve -----------------------------------------------------------------------

/// The Dissolve tool.
#[derive(Debug, Default)]
pub struct DissolveTool;

impl Tool for DissolveTool {
    fn name(&self) -> &'static str {
        "Dissolve"
    }

    fn pauses(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Click(inf) => {
                let vertex = (inf.kind == Kind::Endpoint)
                    .then(|| ctx.entities.find_vertex(inf.point))
                    .flatten();
                match vertex {
                    Some(vertex) => Outcome::Commit(DrawOp::Dissolve { vertex }),
                    None => Outcome::Message("Click a Point Between Two Straight Edges".into()),
                }
            }
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        false
    }

    fn last_point(&self) -> Option<DVec3> {
        None
    }

    fn preview(&self, _ctx: &Ctx<'_>) -> Preview {
        Preview::default()
    }

    fn status(&self) -> String {
        "Click a Point Between Two Straight Edges".into()
    }

    fn fields(&self) -> Vec<Field> {
        Vec::new()
    }

    fn edit(&mut self, _index: usize, _value: f64, _ctx: &Ctx<'_>) -> Outcome {
        Outcome::Nothing
    }

    fn accept(&mut self) {}

    fn reopen(&mut self) -> bool {
        false
    }

    fn forget(&mut self) {}
}

#[cfg(test)]
mod slide_tests {
    use super::*;
    use crate::testing::{at, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    #[test]
    fn the_preview_shows_a_slid_corner_at_its_new_place() {
        let mut m = Entities::new();
        m.add_polyline(
            &[
                v(0.0, 0.0, 0.0),
                v(10.0, 0.0, 0.0),
                v(10.0, 10.0, 0.0),
                v(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        let corner = m.find_vertex(v(10.0, 0.0, 0.0)).unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = SlideTool::default();
        let mut inf = at(v(10.0, 0.0, 0.0));
        inf.kind = Kind::Endpoint;
        inf.entity = m
            .vertex(corner)
            .and_then(|x| x.edges().first().copied())
            .map(Pickable::Edge);
        t.handle(Input::Click(inf), &c);
        // Towards the corner at (0, 0): along the bottom edge.
        t.handle(Input::Hover(at(v(6.0, 0.0, 0.0))), &c);
        let p = t.preview(&c);
        assert_eq!(p.lines.len(), 2);
        assert!(
            p.lines
                .iter()
                .all(|[_, b]| (*b - v(6.0, 0.0, 0.0)).length() < 1e-9)
        );
    }
}
