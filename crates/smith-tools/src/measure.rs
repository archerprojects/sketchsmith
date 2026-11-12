//! SketchSmith — measuring tools | v1.0.0 | Thursday 12 November 2026
//!
//! The tools that leave guides (see `smith_model::Guide`):
//!
//! * **Tape Measure** — click a start point, then an end point; Operation
//!   Inputs shows the distance. Started on an edge or a guide line, it makes
//!   a guide line parallel to it, through the end point: the distance is
//!   measured square to the edge, so typing `2.5` puts the guide exactly
//!   2.5 mm off it. Started anywhere else, it makes a guide point at the
//!   end. Ctrl (a tap) switches between making guides and measuring only.
//!   Typing a length while measuring sets the distance along the direction
//!   the cursor shows; typing one after a guide is made moves that guide
//!   (one undo step).
//! * **Protractor** — click the centre (on a face the protractor lies on
//!   it, otherwise flat on the drawing axes, or on the plane the arrow keys
//!   choose), click a point to start the angle, then click where it ends or
//!   type it in degrees. The angle snaps to 15° steps near them. It makes a
//!   guide line through the centre at that angle; typing an angle after
//!   that turns the guide (one undo step).
//! * **Find Center** — click a circle, an arc or a polygon: a guide point
//!   goes at its centre.

use smith_geom::plane::Plane;
use smith_geom::{DMat3, DVec3};
use smith_infer::infer::{Kind, Lock};
use smith_infer::units::TypedValue;
use smith_model::{Guide, Pickable};
use smith_ops::draw::DrawOp;

use crate::transform_tools::{SNAP_NEAR, SNAP_STEP, protractor};
use crate::{Ctx, Field, Input, Outcome, Preview, Tool, face_normal, in_plane};

/// Half the length of a guide line's preview, in screen pixels.
const PREVIEW_PX: f64 = 400.0;

/// The preview of an endless guide line through `p` along unit `d`.
fn line_preview(p: DVec3, d: DVec3, ctx: &Ctx<'_>) -> [DVec3; 2] {
    let half = PREVIEW_PX * ctx.pixel.max(1e-9);
    [p - d * half, p + d * half]
}

// ----- Tape Measure -------------------------------------------------------------

/// Where a measurement starts.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Start {
    point: DVec3,
    /// The direction of the edge or guide line it started on: the guide
    /// made is a line parallel to it. `None`: the guide is a point.
    along: Option<DVec3>,
}

impl Start {
    /// The offset from the start to `p` that the measurement uses: square
    /// to `along` for a parallel guide, straight otherwise.
    fn offset(&self, p: DVec3) -> DVec3 {
        match self.along {
            Some(d) => in_plane(p - self.point, d),
            None => p - self.point,
        }
    }

    /// The guide `distance` along unit `unit` from the start.
    fn guide(&self, unit: DVec3, distance: f64) -> Option<Guide> {
        let at = self.point + unit * distance;
        match self.along {
            Some(d) => Guide::line(at, d),
            None => Guide::point(at),
        }
    }
}

/// A guide the Tape Measure made, kept so a typed length can move it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Made {
    start: Start,
    unit: DVec3,
    distance: f64,
    /// Operation Inputs shows it.
    shown: bool,
}

/// The Tape Measure tool.
#[derive(Debug)]
pub struct TapeMeasureTool {
    /// Make guides (otherwise only measure).
    guides: bool,
    start: Option<Start>,
    hover: Option<DVec3>,
    /// The last distance measured without a guide, shown until the next one.
    measured: Option<f64>,
    made: Option<Made>,
}

impl Default for TapeMeasureTool {
    fn default() -> Self {
        Self {
            guides: true,
            start: None,
            hover: None,
            measured: None,
            made: None,
        }
    }
}

impl TapeMeasureTool {
    /// The start of a measurement at `inf`: on an edge or a guide line it
    /// runs square to it.
    fn start_at(inf: &smith_infer::infer::Inference, ctx: &Ctx<'_>) -> Start {
        let along = match (inf.kind, inf.entity) {
            (Kind::OnEdge, Some(Pickable::Edge(e))) => ctx
                .entities
                .edge_points(e)
                .and_then(|(a, b)| (b - a).try_normalize()),
            (Kind::OnGuide, Some(Pickable::Guide(g))) => match ctx.entities.guide(g) {
                Some(Guide::Line { direction, .. }) => Some(*direction),
                _ => None,
            },
            _ => None,
        };
        Start {
            point: inf.point,
            along,
        }
    }

    /// The live distance from the start to the cursor.
    fn live(&self) -> Option<f64> {
        let s = self.start?;
        Some(s.offset(self.hover?).length())
    }

    /// Ends the measurement `distance` along `unit`.
    fn finish(&mut self, start: Start, unit: DVec3, distance: f64) -> Outcome {
        self.start = None;
        if !self.guides {
            self.measured = Some(distance.abs());
            return Outcome::Nothing;
        }
        let Some(guide) = start.guide(unit, distance) else {
            return Outcome::Nothing;
        };
        self.made = Some(Made {
            start,
            unit,
            distance,
            shown: true,
        });
        Outcome::Commit(DrawOp::Guide { guide })
    }

    /// Sets the distance: of the measurement under way, or of the guide
    /// just made.
    fn set_distance(&mut self, distance: f64) -> Outcome {
        if let Some(s) = self.start {
            let Some(unit) = self.hover.and_then(|h| s.offset(h).try_normalize()) else {
                return Outcome::Message("Move the Cursor to Show the Direction".into());
            };
            return self.finish(s, unit, distance);
        }
        match self.made {
            Some(m) if m.shown => match m.start.guide(m.unit, distance) {
                Some(guide) => {
                    self.made = Some(Made { distance, ..m });
                    Outcome::Redefine(DrawOp::Guide { guide })
                }
                None => Outcome::Nothing,
            },
            _ => Outcome::Message("Click a Start Point".into()),
        }
    }
}

impl Tool for TapeMeasureTool {
    fn name(&self) -> &'static str {
        "Tape Measure"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                Outcome::Nothing
            }
            Input::Click(inf) => match self.start {
                None => {
                    self.start = Some(Self::start_at(&inf, ctx));
                    self.hover = Some(inf.point);
                    self.measured = None;
                    if let Some(m) = &mut self.made {
                        m.shown = false;
                    }
                    Outcome::Nothing
                }
                Some(s) => {
                    let offset = s.offset(inf.point);
                    let distance = offset.length();
                    // No direction: a guide on the edge itself, or at the start.
                    let unit = offset.try_normalize().unwrap_or(DVec3::ZERO);
                    self.finish(s, unit, distance)
                }
            },
            Input::Typed(TypedValue::Length(l)) => self.set_distance(l),
            Input::Typed(_) => Outcome::Message("Type a Length".into()),
            Input::Toggle => {
                self.guides = !self.guides;
                Outcome::Nothing
            }
            Input::Finish => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        self.start.take().is_some()
    }

    fn last_point(&self) -> Option<DVec3> {
        self.start.map(|s| s.point)
    }

    fn pauses(&self) -> bool {
        false
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        let (Some(s), Some(h)) = (self.start, self.hover) else {
            return Preview::default();
        };
        let foot = s.point + s.offset(h);
        let mut lines = Vec::new();
        if let (Some(d), true) = (s.along, self.guides) {
            lines.push(line_preview(foot, d, ctx));
        }
        Preview {
            lines,
            band: Some([s.point, foot]),
            note: None,
        }
    }

    fn status(&self) -> String {
        let ctrl = if self.guides {
            "Ctrl: Measure Only"
        } else {
            "Ctrl: Make Guides"
        };
        match self.start {
            None => format!("Click a Start Point · {ctrl}"),
            Some(_) => format!("Click the End Point, or Type the Length · {ctrl}"),
        }
    }

    fn fields(&self) -> Vec<Field> {
        let length = match (self.start, self.made) {
            (Some(_), _) => Field::length("Length", self.live(), true, true),
            (None, Some(m)) if m.shown => Field::length("Length", Some(m.distance), false, true),
            _ => Field::length("Length", self.measured, false, false),
        };
        vec![length, Field::switch("Guides", self.guides, true)]
    }

    fn edit(&mut self, index: usize, value: f64, ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 => self.set_distance(value),
            1 if (value != 0.0) != self.guides => self.handle(Input::Toggle, ctx),
            _ => Outcome::Nothing,
        }
    }

    fn accept(&mut self) {
        if let Some(m) = &mut self.made {
            m.shown = false;
        }
    }

    fn reopen(&mut self) -> bool {
        match &mut self.made {
            Some(m) => {
                m.shown = true;
                true
            }
            None => false,
        }
    }

    fn forget(&mut self) {
        self.made = None;
    }
}

// ----- Protractor ---------------------------------------------------------------

/// A guide the Protractor made, kept so a typed angle can turn it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Turned {
    centre: DVec3,
    axis: DVec3,
    /// Unit direction the angle is measured from.
    base: DVec3,
    angle: f64,
    shown: bool,
}

impl Turned {
    fn guide(&self) -> Option<Guide> {
        Guide::line(
            self.centre,
            DMat3::from_axis_angle(self.axis, self.angle) * self.base,
        )
    }
}

/// The Protractor tool.
#[derive(Debug, Default)]
pub struct ProtractorTool {
    /// Centre and unit axis.
    centre: Option<(DVec3, DVec3)>,
    /// Where the angle starts.
    from: Option<DVec3>,
    hover: Option<DVec3>,
    /// The plane chosen with the arrow keys, as its normal.
    axis_lock: Option<DVec3>,
    /// The plane under the cursor before the centre is placed.
    hover_axis: Option<DVec3>,
    made: Option<Turned>,
}

impl ProtractorTool {
    /// The unit direction from the centre to where the angle starts, in
    /// the protractor's plane.
    fn base(&self) -> Option<DVec3> {
        let (c, axis) = self.centre?;
        in_plane(self.from? - c, axis).try_normalize()
    }

    /// The angle from the start to `p` about the axis, radians, snapped
    /// near 15° steps.
    fn angle_to(&self, p: DVec3) -> Option<f64> {
        let (c, axis) = self.centre?;
        let a = self.base()?;
        let b = in_plane(p - c, axis);
        if b.length() < 1e-9 {
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

    /// Sets the angle: of the one under way, or of the guide just made.
    fn set_angle(&mut self, angle: f64) -> Outcome {
        if let (Some((centre, axis)), Some(base)) = (self.centre, self.base()) {
            let t = Turned {
                centre,
                axis,
                base,
                angle,
                shown: true,
            };
            self.centre = None;
            self.from = None;
            let Some(guide) = t.guide() else {
                return Outcome::Nothing;
            };
            self.made = Some(t);
            return Outcome::Commit(DrawOp::Guide { guide });
        }
        match self.made {
            Some(m) if m.shown => {
                let t = Turned { angle, ..m };
                match t.guide() {
                    Some(guide) => {
                        self.made = Some(t);
                        Outcome::Redefine(DrawOp::Guide { guide })
                    }
                    None => Outcome::Nothing,
                }
            }
            _ => Outcome::Message("Click the Centre".into()),
        }
    }
}

impl Tool for ProtractorTool {
    fn name(&self) -> &'static str {
        "Protractor"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Hover(inf) => {
                self.hover = Some(inf.point);
                self.hover_axis = self.axis_lock.or_else(|| face_normal(&inf, ctx.entities));
                Outcome::Nothing
            }
            Input::Click(inf) => match (self.centre, self.from) {
                (None, _) => {
                    let axis = self
                        .axis_lock
                        .or_else(|| face_normal(&inf, ctx.entities))
                        .unwrap_or(ctx.axes.z);
                    self.centre = Some((inf.point, axis));
                    if let Some(m) = &mut self.made {
                        m.shown = false;
                    }
                    Outcome::Nothing
                }
                (Some((c, axis)), None) => {
                    let far = in_plane(inf.point - c, axis).length();
                    if far > (ctx.pixel * 8.0).max(1e-6) {
                        self.from = Some(inf.point);
                    }
                    Outcome::Nothing
                }
                (Some(_), Some(_)) => match self.angle_to(inf.point) {
                    Some(a) => self.set_angle(a),
                    None => Outcome::Nothing,
                },
            },
            Input::Typed(TypedValue::Length(deg)) => self.set_angle(deg.to_radians()),
            Input::Typed(_) => Outcome::Message("Type the Angle in Degrees".into()),
            Input::Toggle | Input::Finish => Outcome::Nothing,
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
    /// from it guide that click; none once the angle is turning.
    fn last_point(&self) -> Option<DVec3> {
        match (self.centre, self.from) {
            (Some((c, _)), None) => Some(c),
            _ => None,
        }
    }

    fn use_axis_lock(&mut self, axis: Option<DVec3>) -> bool {
        self.axis_lock = axis;
        if let (Some((c, _)), None, Some(a)) = (self.centre, self.from, axis) {
            self.centre = Some((c, a));
        }
        true
    }

    fn lock(&self) -> Option<Lock> {
        let (c, axis) = self.centre?;
        Plane::from_point_normal(c, axis).ok().map(Lock::Plane)
    }

    fn pauses(&self) -> bool {
        false
    }

    fn preview(&self, ctx: &Ctx<'_>) -> Preview {
        let Some((c, axis)) = self.centre else {
            let lines = self
                .hover
                .map(|h| protractor(h, self.hover_axis.unwrap_or(ctx.axes.z), ctx))
                .unwrap_or_default();
            return Preview {
                lines,
                band: None,
                note: None,
            };
        };
        let mut lines = protractor(c, axis, ctx);
        if let Some(f) = self.from {
            lines.push([c, f]);
        }
        let direction = self.hover.and_then(|h| {
            let base = self.base()?;
            let a = self.angle_to(h)?;
            Some(DMat3::from_axis_angle(axis, a) * base)
        });
        if let Some(d) = direction {
            lines.push(line_preview(c, d, ctx));
        }
        Preview {
            lines,
            band: self.hover.map(|h| [c, h]),
            note: None,
        }
    }

    fn status(&self) -> String {
        match (self.centre, self.from) {
            (None, _) => "Click the Centre".into(),
            (Some(_), None) => "Click to Start the Angle".into(),
            (Some(_), Some(_)) => "Click to End the Angle, or Type it in Degrees".into(),
        }
    }

    fn fields(&self) -> Vec<Field> {
        let (angle, live, editable) = match (self.from, self.made) {
            (Some(_), _) => (self.hover.and_then(|h| self.angle_to(h)), true, true),
            (None, Some(m)) if m.shown => (Some(m.angle), false, true),
            _ => (None, false, false),
        };
        let mut f = Field::number("Angle", angle.map(f64::to_degrees), editable);
        f.live = live;
        vec![f]
    }

    fn edit(&mut self, index: usize, value: f64, _ctx: &Ctx<'_>) -> Outcome {
        match index {
            0 => self.set_angle(value.to_radians()),
            _ => Outcome::Nothing,
        }
    }

    fn accept(&mut self) {
        if let Some(m) = &mut self.made {
            m.shown = false;
        }
    }

    fn reopen(&mut self) -> bool {
        match &mut self.made {
            Some(m) => {
                m.shown = true;
                true
            }
            None => false,
        }
    }

    fn forget(&mut self) {
        self.made = None;
    }
}

// ----- Find Center --------------------------------------------------------------

/// The Find Center tool.
#[derive(Debug, Default)]
pub struct FindCenterTool;

impl FindCenterTool {
    /// The centre of the circle, arc or polygon edge `p` belongs to.
    fn centre(p: Option<Pickable>, ctx: &Ctx<'_>) -> Option<DVec3> {
        use smith_geom::curve::Curve;
        let Some(Pickable::Edge(e)) = p else {
            return None;
        };
        let rec = ctx.entities.curve(ctx.entities.edge(e)?.curve()?)?;
        match rec.curve() {
            Curve::Circle(c) => Some(c.frame().center()),
            Curve::Arc(a) => Some(a.frame().center()),
            Curve::Polygon(p) => Some(p.frame().center()),
            _ => None,
        }
    }
}

impl Tool for FindCenterTool {
    fn name(&self) -> &'static str {
        "Find Center"
    }

    fn handle(&mut self, input: Input, ctx: &Ctx<'_>) -> Outcome {
        match input {
            Input::Click(inf) => match Self::centre(inf.entity, ctx).and_then(Guide::point) {
                Some(guide) => Outcome::Commit(DrawOp::Guide { guide }),
                None => Outcome::Message("Click a Circle, Arc or Polygon".into()),
            },
            _ => Outcome::Nothing,
        }
    }

    fn cancel(&mut self) -> bool {
        false
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
        "Click a Circle, Arc or Polygon".into()
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
mod tests {
    use super::*;
    use crate::testing::{at, ctx, v};
    use smith_infer::infer::DrawingAxes;
    use smith_model::Entities;

    fn guide_of(out: &Outcome) -> Guide {
        match out {
            Outcome::Commit(DrawOp::Guide { guide })
            | Outcome::Redefine(DrawOp::Guide { guide }) => *guide,
            other => panic!("expected a guide, got {other:?}"),
        }
    }

    /// The 10 mm edge along X on the ground, and an inference on it.
    fn edge() -> (Entities, smith_infer::infer::Inference) {
        let mut m = Entities::new();
        let e = m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap()[0];
        let mut inf = at(v(4.0, 0.0, 0.0));
        inf.kind = Kind::OnEdge;
        inf.entity = Some(Pickable::Edge(e));
        (m, inf)
    }

    #[test]
    fn a_typed_tape_guide_is_exactly_that_far_off_its_edge() {
        let (m, inf) = edge();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = TapeMeasureTool::default();
        t.handle(Input::Click(inf), &c);
        // The cursor wanders off at a slant; only the square offset counts.
        t.handle(Input::Hover(at(v(7.0, 3.1, 0.0))), &c);
        let out = t.handle(Input::Typed(TypedValue::Length(2.5)), &c);
        let g = guide_of(&out);
        assert_eq!(
            g,
            Guide::Line {
                point: v(4.0, 2.5, 0.0),
                direction: DVec3::X
            }
        );
        assert_eq!(g.distance(v(9.0, 0.0, 0.0)), 2.5);
        // Typing again moves that guide, as one step.
        let out = t.handle(Input::Typed(TypedValue::Length(1.25)), &c);
        assert!(matches!(out, Outcome::Redefine(_)));
        assert_eq!(guide_of(&out).distance(v(0.0, 0.0, 0.0)), 1.25);
    }

    #[test]
    fn from_a_point_the_tape_leaves_a_guide_point() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = TapeMeasureTool::default();
        t.handle(Input::Click(at(v(1.0, 1.0, 0.0))), &c);
        t.handle(Input::Hover(at(v(1.0, 5.0, 0.0))), &c);
        assert_eq!(t.fields()[0].value, Some(4.0));
        let out = t.handle(Input::Click(at(v(1.0, 5.0, 0.0))), &c);
        assert_eq!(guide_of(&out), Guide::point(v(1.0, 5.0, 0.0)).unwrap());
    }

    #[test]
    fn measure_only_makes_nothing_but_keeps_the_distance() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = TapeMeasureTool::default();
        t.handle(Input::Toggle, &c);
        t.handle(Input::Click(at(v(0.0, 0.0, 0.0))), &c);
        let out = t.handle(Input::Click(at(v(3.0, 4.0, 0.0))), &c);
        assert_eq!(out, Outcome::Nothing);
        assert_eq!(t.fields()[0].value, Some(5.0));
    }

    #[test]
    fn protractor_guide_snaps_to_fifteen_degrees_and_takes_typed_angles() {
        let m = Entities::new();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut t = ProtractorTool::default();
        t.handle(Input::Click(at(v(0.0, 0.0, 0.0))), &c);
        t.handle(Input::Click(at(v(10.0, 0.0, 0.0))), &c);
        // 44.2° pulls to 45°.
        let a = 44.2f64.to_radians();
        let p = v(10.0 * a.cos(), 10.0 * a.sin(), 0.0);
        t.handle(Input::Hover(at(p)), &c);
        let g = guide_of(&t.handle(Input::Click(at(p)), &c));
        let Guide::Line { point, direction } = g else {
            panic!("a line");
        };
        assert_eq!(point, DVec3::ZERO);
        assert!((direction - v(0.5f64.sqrt(), 0.5f64.sqrt(), 0.0)).length() < 1e-12);
        // Typing turns it to 30°.
        let g = guide_of(&t.handle(Input::Typed(TypedValue::Length(30.0)), &c));
        let Guide::Line { direction, .. } = g else {
            panic!("a line");
        };
        assert!((direction - v(0.75f64.sqrt(), 0.5, 0.0)).length() < 1e-12);
    }

    #[test]
    fn find_center_puts_a_guide_point_at_a_circles_centre() {
        use smith_geom::curve::{Circle, Curve, CurveFrame};
        let mut m = Entities::new();
        let frame = CurveFrame::new(v(3.0, 4.0, 0.0), DVec3::X, DVec3::Z).unwrap();
        m.add_curve(Curve::Circle(Circle::new(frame, 2.0).unwrap()), 24)
            .unwrap();
        let e = m.edge_ids().next().unwrap();
        let axes = DrawingAxes::default();
        let c = ctx(&m, &axes);
        let mut inf = at(v(5.0, 4.0, 0.0));
        inf.entity = Some(Pickable::Edge(e));
        let out = FindCenterTool.handle(Input::Click(inf), &c);
        assert_eq!(guide_of(&out), Guide::point(v(3.0, 4.0, 0.0)).unwrap());
        let out = FindCenterTool.handle(Input::Click(at(v(0.0, 0.0, 0.0))), &c);
        assert!(matches!(out, Outcome::Message(_)));
    }
}
