//! SketchSmith — curves | v1.0.0 | Thursday 12 November 2026
//!
//! The curve kinds, plus the Curve Maker helix. A curve is a *true* curve: it is stored
//! analytically, and turned into points only when edges are needed. The model
//! keeps the curve and regenerates its edges whenever the segment count
//! changes, which is what lets Sweep and Fillet work from smooth geometry
//! instead of SketchUp's frozen polylines.
//!
//! | Kind | Defined by | Segment count means |
//! |---|---|---|
//! | [`Arc`] | frame, radius, start/end angle | segments across the arc |
//! | [`Circle`] | frame, radius | segments around the circle |
//! | [`Polygon`] | frame, circumradius, sides | ignored — sides are fixed |
//! | [`Bezier`] | chained cubic control points | segments per cubic span |
//! | [`Polyline`] | explicit points | ignored — the points are the edges |
//! | [`Helix`] | frame, start/end radius, pitch, turns, hand | segments along the whole helix |
//!
//! Angles are measured counter-clockwise about the frame normal from the frame
//! x-axis. Every generated point is computed directly from the definition (never
//! accumulated), so endpoints and vertices are deterministic and repeatable.
//!
//! [`CurveFrame::from_stored`] restores a saved frame bit for bit (native file
//! format); the other constructors do not alter valid input, so a
//! saved curve reads back identical.

use std::f64::consts::TAU;

use glam::{DMat3, DMat4, DVec3};

use crate::error::GeomError;
use crate::plane::STORED_UNIT_TOLERANCE;
use crate::tolerance::TOL_MERGE;

/// Default segment count for a new circle (matches SketchUp's default).
pub const DEFAULT_CIRCLE_SEGMENTS: u32 = 24;

/// Default segment count for a new arc.
pub const DEFAULT_ARC_SEGMENTS: u32 = 12;

/// Default segment count per cubic span of a new Bézier spline.
pub const DEFAULT_BEZIER_SEGMENTS: u32 = 12;

/// Default segments per turn of a new helix or spiral.
pub const DEFAULT_HELIX_SEGMENTS_PER_TURN: u32 = 24;

/// Minimum segments for a closed round curve (circle).
pub const MIN_CLOSED_SEGMENTS: u32 = 3;

/// Upper limit on any segment count, to keep a typo such as `100000s` from
/// generating an unusable model. Generous for jewelry detail.
pub const MAX_SEGMENTS: u32 = 4096;

/// An orthonormal placement for planar curves: centre, x-axis and normal.
/// The y-axis is `normal × x_axis`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveFrame {
    center: DVec3,
    x_axis: DVec3,
    normal: DVec3,
}

impl CurveFrame {
    /// Builds a frame. `normal` is normalised; `x_axis` is made exactly
    /// perpendicular to it (Gram–Schmidt) and normalised.
    ///
    /// # Errors
    /// [`GeomError::NonFinite`], [`GeomError::DegenerateNormal`], or
    /// [`GeomError::DegenerateAxis`] if `x_axis` is zero or parallel to `normal`.
    pub fn new(center: DVec3, x_axis: DVec3, normal: DVec3) -> Result<Self, GeomError> {
        if !center.is_finite() || !x_axis.is_finite() || !normal.is_finite() {
            return Err(GeomError::NonFinite);
        }
        let normal = normal.try_normalize().ok_or(GeomError::DegenerateNormal)?;
        let x_axis = (x_axis - normal * x_axis.dot(normal))
            .try_normalize()
            .ok_or(GeomError::DegenerateAxis)?;
        Ok(Self {
            center,
            x_axis,
            normal,
        })
    }

    /// Restores a frame exactly as stored (native file reading). Unlike
    /// [`CurveFrame::new`] nothing is renormalised, so a saved curve regenerates
    /// bit-identical points; the axes must already be unit and perpendicular.
    ///
    /// # Errors
    /// [`GeomError::NonFinite`]; [`GeomError::DegenerateNormal`] or
    /// [`GeomError::DegenerateAxis`] when a vector is not unit, or the two are
    /// not perpendicular, within [`STORED_UNIT_TOLERANCE`].
    pub fn from_stored(center: DVec3, x_axis: DVec3, normal: DVec3) -> Result<Self, GeomError> {
        if !center.is_finite() || !x_axis.is_finite() || !normal.is_finite() {
            return Err(GeomError::NonFinite);
        }
        if (normal.length() - 1.0).abs() > STORED_UNIT_TOLERANCE {
            return Err(GeomError::DegenerateNormal);
        }
        if (x_axis.length() - 1.0).abs() > STORED_UNIT_TOLERANCE
            || x_axis.dot(normal).abs() > STORED_UNIT_TOLERANCE
        {
            return Err(GeomError::DegenerateAxis);
        }
        Ok(Self {
            center,
            x_axis,
            normal,
        })
    }

    /// The centre point.
    #[inline]
    pub fn center(&self) -> DVec3 {
        self.center
    }

    /// The unit x-axis (angle zero).
    #[inline]
    pub fn x_axis(&self) -> DVec3 {
        self.x_axis
    }

    /// The unit y-axis, `normal × x_axis`.
    #[inline]
    pub fn y_axis(&self) -> DVec3 {
        self.normal.cross(self.x_axis)
    }

    /// The unit normal.
    #[inline]
    pub fn normal(&self) -> DVec3 {
        self.normal
    }

    /// The point at `angle` radians on a circle of `radius` in this frame.
    #[inline]
    pub fn point_at(&self, radius: f64, angle: f64) -> DVec3 {
        let (sin, cos) = angle.sin_cos();
        self.center + radius * (cos * self.x_axis + sin * self.y_axis())
    }
}

fn check_radius(radius: f64) -> Result<(), GeomError> {
    if !radius.is_finite() {
        Err(GeomError::NonFinite)
    } else if radius < TOL_MERGE {
        Err(GeomError::NonPositiveRadius(radius))
    } else {
        Ok(())
    }
}

fn check_segments(segments: u32, minimum: u32) -> Result<(), GeomError> {
    if segments < minimum {
        Err(GeomError::TooFewSegments {
            minimum,
            given: segments,
        })
    } else {
        Ok(())
    }
}

/// A circular arc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    frame: CurveFrame,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
}

impl Arc {
    /// Creates an arc from `start_angle` to `end_angle` (radians,
    /// counter-clockwise about the frame normal).
    ///
    /// # Errors
    /// [`GeomError::NonPositiveRadius`] for a radius below `TOL_MERGE`;
    /// [`GeomError::InvalidSweep`] unless `0 < end − start ≤ 2π`.
    pub fn new(
        frame: CurveFrame,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    ) -> Result<Self, GeomError> {
        check_radius(radius)?;
        let sweep = end_angle - start_angle;
        if !sweep.is_finite() || sweep <= 0.0 || sweep > TAU {
            return Err(GeomError::InvalidSweep(sweep));
        }
        Ok(Self {
            frame,
            radius,
            start_angle,
            end_angle,
        })
    }

    /// The placement frame.
    pub fn frame(&self) -> &CurveFrame {
        &self.frame
    }

    /// The radius in mm.
    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// Start angle in radians.
    pub fn start_angle(&self) -> f64 {
        self.start_angle
    }

    /// End angle in radians.
    pub fn end_angle(&self) -> f64 {
        self.end_angle
    }

    /// Swept angle `end − start`, in `(0, 2π]`.
    pub fn sweep(&self) -> f64 {
        self.end_angle - self.start_angle
    }

    /// Arc length in mm.
    pub fn length(&self) -> f64 {
        self.radius * self.sweep()
    }

    /// The start point.
    pub fn start_point(&self) -> DVec3 {
        self.frame.point_at(self.radius, self.start_angle)
    }

    /// The end point.
    pub fn end_point(&self) -> DVec3 {
        self.frame.point_at(self.radius, self.end_angle)
    }
}

/// A full circle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    frame: CurveFrame,
    radius: f64,
}

impl Circle {
    /// Creates a circle.
    ///
    /// # Errors
    /// [`GeomError::NonPositiveRadius`] for a radius below `TOL_MERGE`.
    pub fn new(frame: CurveFrame, radius: f64) -> Result<Self, GeomError> {
        check_radius(radius)?;
        Ok(Self { frame, radius })
    }

    /// The placement frame.
    pub fn frame(&self) -> &CurveFrame {
        &self.frame
    }

    /// The radius in mm.
    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// Circumference in mm.
    pub fn length(&self) -> f64 {
        TAU * self.radius
    }
}

/// A regular polygon, defined by its circumradius. Its edges are never
/// smoothed, unlike a circle's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Polygon {
    frame: CurveFrame,
    radius: f64,
    sides: u32,
}

impl Polygon {
    /// Creates a regular polygon whose first vertex lies on the frame x-axis.
    ///
    /// # Errors
    /// [`GeomError::NonPositiveRadius`]; [`GeomError::TooFewSides`] below three.
    pub fn new(frame: CurveFrame, radius: f64, sides: u32) -> Result<Self, GeomError> {
        check_radius(radius)?;
        if sides < 3 {
            return Err(GeomError::TooFewSides(sides));
        }
        Ok(Self {
            frame,
            radius,
            sides: sides.min(MAX_SEGMENTS),
        })
    }

    /// The placement frame.
    pub fn frame(&self) -> &CurveFrame {
        &self.frame
    }

    /// The circumradius in mm.
    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// The number of sides.
    pub fn sides(&self) -> u32 {
        self.sides
    }
}

/// A chain of cubic Bézier spans sharing endpoints: control points
/// `P0 P1 P2 P3 P4 P5 P6 …`, where span *i* uses `P3i … P3i+3`.
#[derive(Debug, Clone, PartialEq)]
pub struct Bezier {
    control_points: Vec<DVec3>,
}

impl Bezier {
    /// Creates a Bézier chain.
    ///
    /// # Errors
    /// [`GeomError::BezierControlPoints`] unless there are `3n + 1` points with
    /// `n ≥ 1`; [`GeomError::NonFinite`] for NaN or infinite points.
    pub fn new(control_points: Vec<DVec3>) -> Result<Self, GeomError> {
        let n = control_points.len();
        if n < 4 || !(n - 1).is_multiple_of(3) {
            return Err(GeomError::BezierControlPoints(n));
        }
        if control_points.iter().any(|p| !p.is_finite()) {
            return Err(GeomError::NonFinite);
        }
        Ok(Self { control_points })
    }

    /// The control points.
    pub fn control_points(&self) -> &[DVec3] {
        &self.control_points
    }

    /// Number of cubic spans.
    pub fn spans(&self) -> usize {
        (self.control_points.len() - 1) / 3
    }

    /// Evaluates span `span` at parameter `t` in `[0, 1]` by de Casteljau's
    /// algorithm. `t = 0` and `t = 1` return the span's end control points exactly.
    pub fn evaluate(&self, span: usize, t: f64) -> DVec3 {
        let p = &self.control_points[span * 3..span * 3 + 4];
        if t <= 0.0 {
            return p[0];
        }
        if t >= 1.0 {
            return p[3];
        }
        let a = p[0].lerp(p[1], t);
        let b = p[1].lerp(p[2], t);
        let c = p[2].lerp(p[3], t);
        let d = a.lerp(b, t);
        let e = b.lerp(c, t);
        d.lerp(e, t)
    }
}

/// A helix, conical helix or flat spiral (Curve Maker).
///
/// The frame's centre is the start of the axis, its normal the axis direction,
/// its x-axis the direction of the start point. At parameter `t` in `[0, 1]`
/// the radius runs linearly from `start_radius` to `end_radius`, the height
/// rises `pitch × turns × t` along the axis, and the angle turns `turns × t`
/// full turns — counter-clockwise about the axis, or clockwise when
/// `left_handed`. A pitch of zero gives a flat spiral; equal radii a cylinder
/// helix. The angle is reduced to the fraction of a turn before taking sines,
/// so whole-turn points (including the end of an integer-turn helix) are exact.
#[derive(Debug, Clone, PartialEq)]
pub struct Helix {
    frame: CurveFrame,
    start_radius: f64,
    end_radius: f64,
    pitch: f64,
    turns: f64,
    left_handed: bool,
}

impl Helix {
    /// Creates a helix or spiral.
    ///
    /// # Errors
    /// [`GeomError::NonFinite`] for NaN or infinite values;
    /// [`GeomError::NonPositiveRadius`] if a radius is negative or both are
    /// below `TOL_MERGE`; [`GeomError::InvalidSweep`] unless `turns > 0`.
    pub fn new(
        frame: CurveFrame,
        start_radius: f64,
        end_radius: f64,
        pitch: f64,
        turns: f64,
        left_handed: bool,
    ) -> Result<Self, GeomError> {
        if ![start_radius, end_radius, pitch, turns]
            .iter()
            .all(|v| v.is_finite())
        {
            return Err(GeomError::NonFinite);
        }
        if start_radius < 0.0 || end_radius < 0.0 || start_radius.max(end_radius) < TOL_MERGE {
            return Err(GeomError::NonPositiveRadius(start_radius.min(end_radius)));
        }
        if turns <= 0.0 {
            return Err(GeomError::InvalidSweep(turns * TAU));
        }
        Ok(Self {
            frame,
            start_radius,
            end_radius,
            pitch,
            turns,
            left_handed,
        })
    }

    /// The frame (axis start, start direction, axis).
    pub fn frame(&self) -> &CurveFrame {
        &self.frame
    }

    /// Radius at the start, mm.
    pub fn start_radius(&self) -> f64 {
        self.start_radius
    }

    /// Radius at the end, mm.
    pub fn end_radius(&self) -> f64 {
        self.end_radius
    }

    /// Rise per turn along the axis, mm.
    pub fn pitch(&self) -> f64 {
        self.pitch
    }

    /// Number of turns.
    pub fn turns(&self) -> f64 {
        self.turns
    }

    /// Whether it turns clockwise about the axis.
    pub fn left_handed(&self) -> bool {
        self.left_handed
    }

    /// The point at parameter `t` (0 = start, 1 = end).
    pub fn point_at(&self, t: f64) -> DVec3 {
        let r = self.start_radius + (self.end_radius - self.start_radius) * t;
        let fraction = (self.turns * t).rem_euclid(1.0);
        let angle = TAU * fraction * if self.left_handed { -1.0 } else { 1.0 };
        let height = self.pitch * self.turns * t;
        self.frame.point_at(r, angle) + self.frame.normal() * height
    }

    /// The start point.
    pub fn start_point(&self) -> DVec3 {
        self.point_at(0.0)
    }

    /// The end point.
    pub fn end_point(&self) -> DVec3 {
        self.point_at(1.0)
    }
}

/// An explicit point chain, as produced by Weld.
#[derive(Debug, Clone, PartialEq)]
pub struct Polyline {
    points: Vec<DVec3>,
    closed: bool,
}

impl Polyline {
    /// Creates a polyline. A closed polyline joins its last point back to its first;
    /// the first point is not repeated at the end.
    ///
    /// # Errors
    /// [`GeomError::TooFewPoints`] for fewer than 2 points (3 if closed);
    /// [`GeomError::NonFinite`] for NaN or infinite points.
    pub fn new(points: Vec<DVec3>, closed: bool) -> Result<Self, GeomError> {
        let required = if closed { 3 } else { 2 };
        if points.len() < required {
            return Err(GeomError::TooFewPoints {
                required,
                given: points.len(),
            });
        }
        if points.iter().any(|p| !p.is_finite()) {
            return Err(GeomError::NonFinite);
        }
        Ok(Self { points, closed })
    }

    /// The points.
    pub fn points(&self) -> &[DVec3] {
        &self.points
    }

    /// Whether the chain is closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

/// Any curve the model can own.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    /// A circular arc.
    Arc(Arc),
    /// A full circle.
    Circle(Circle),
    /// A regular polygon.
    Polygon(Polygon),
    /// A chain of cubic Bézier spans.
    Bezier(Bezier),
    /// An explicit point chain.
    Polyline(Polyline),
    /// A helix, conical helix or flat spiral.
    Helix(Helix),
}

impl Curve {
    /// The curve under the transform `m`, if `m` keeps its shape: a rotation,
    /// mirror, uniform scale or move (any combination). `None` for a
    /// stretching transform, and for a mirrored helix, which would change
    /// hand; such curves become plain edges instead.
    #[must_use]
    pub fn transformed(&self, m: &DMat4) -> Option<Curve> {
        let l = DMat3::from_mat4(*m);
        let det = l.determinant();
        if !det.is_finite() || det.abs() < 1e-300 {
            return None;
        }
        let s = det.abs().cbrt();
        let r = l * (1.0 / s);
        // A similarity: r is orthonormal.
        let rt_r = r.transpose() * r;
        if !rt_r.abs_diff_eq(DMat3::IDENTITY, 1e-9) {
            return None;
        }
        let mirror = det < 0.0;
        let point = |p: DVec3| m.transform_point3(p);
        // A mirror swaps the frame's hand; flipping the normal keeps
        // `y = normal × x` mapping onto the mirrored y, so every point lands
        // where the mirror puts it.
        let frame = |f: &CurveFrame| CurveFrame {
            center: point(f.center),
            x_axis: (r * f.x_axis).normalize(),
            normal: if mirror {
                -(r * f.normal).normalize()
            } else {
                (r * f.normal).normalize()
            },
        };
        Some(match self {
            Curve::Arc(a) => Curve::Arc(Arc {
                frame: frame(&a.frame),
                radius: a.radius * s,
                ..*a
            }),
            Curve::Circle(c) => Curve::Circle(Circle {
                frame: frame(&c.frame),
                radius: c.radius * s,
            }),
            Curve::Polygon(pg) => Curve::Polygon(Polygon {
                frame: frame(&pg.frame),
                radius: pg.radius * s,
                ..*pg
            }),
            Curve::Bezier(b) => Curve::Bezier(Bezier {
                control_points: b.control_points.iter().map(|p| point(*p)).collect(),
            }),
            Curve::Polyline(pl) => Curve::Polyline(Polyline {
                points: pl.points.iter().map(|p| point(*p)).collect(),
                closed: pl.closed,
            }),
            Curve::Helix(h) => {
                if mirror {
                    return None;
                }
                Curve::Helix(Helix {
                    frame: frame(&h.frame),
                    start_radius: h.start_radius * s,
                    end_radius: h.end_radius * s,
                    pitch: h.pitch * s,
                    ..*h
                })
            }
        })
    }

    /// The same curve moved by `delta` (shape, size and orientation unchanged).
    #[must_use]
    pub fn translated(&self, delta: DVec3) -> Curve {
        let frame = |f: &CurveFrame| CurveFrame {
            center: f.center + delta,
            ..*f
        };
        match self {
            Curve::Arc(a) => Curve::Arc(Arc {
                frame: frame(&a.frame),
                ..*a
            }),
            Curve::Circle(c) => Curve::Circle(Circle {
                frame: frame(&c.frame),
                ..*c
            }),
            Curve::Polygon(p) => Curve::Polygon(Polygon {
                frame: frame(&p.frame),
                ..*p
            }),
            Curve::Bezier(b) => Curve::Bezier(Bezier {
                control_points: b.control_points.iter().map(|p| *p + delta).collect(),
            }),
            Curve::Polyline(p) => Curve::Polyline(Polyline {
                points: p.points.iter().map(|q| *q + delta).collect(),
                closed: p.closed,
            }),
            Curve::Helix(h) => Curve::Helix(Helix {
                frame: frame(&h.frame),
                ..*h
            }),
        }
    }
}

/// The points generated from a curve, ready to become edges.
#[derive(Debug, Clone, PartialEq)]
pub struct Tessellation {
    /// The generated points, in order. For a closed curve the first point is not
    /// repeated at the end; the closing edge runs from last to first.
    pub points: Vec<DVec3>,
    /// Whether the last point connects back to the first.
    pub closed: bool,
}

impl Tessellation {
    /// Number of edges this tessellation produces.
    pub fn edge_count(&self) -> usize {
        match (self.points.len(), self.closed) {
            (0 | 1, _) => 0,
            (n, true) => n,
            (n, false) => n - 1,
        }
    }
}

impl Curve {
    /// Whether the curve forms a closed loop.
    pub fn is_closed(&self) -> bool {
        match self {
            Curve::Arc(_) | Curve::Bezier(_) | Curve::Helix(_) => false,
            Curve::Circle(_) | Curve::Polygon(_) => true,
            Curve::Polyline(p) => p.is_closed(),
        }
    }

    /// Whether edges generated from this curve may be smoothed. Polygons may not
    ///: their facets are the intended shape.
    pub fn is_smoothable(&self) -> bool {
        !matches!(self, Curve::Polygon(_))
    }

    /// Whether the segment count is meaningful for this curve kind. Polygons and
    /// polylines have fixed edges.
    pub fn has_segment_count(&self) -> bool {
        matches!(
            self,
            Curve::Arc(_) | Curve::Circle(_) | Curve::Bezier(_) | Curve::Helix(_)
        )
    }

    /// The minimum valid segment count for this curve kind.
    pub fn min_segments(&self) -> u32 {
        match self {
            Curve::Circle(_) => MIN_CLOSED_SEGMENTS,
            _ => 1,
        }
    }

    /// The default segment count for a newly created curve of this kind.
    pub fn default_segments(&self) -> u32 {
        match self {
            Curve::Arc(_) => DEFAULT_ARC_SEGMENTS,
            Curve::Circle(_) => DEFAULT_CIRCLE_SEGMENTS,
            Curve::Bezier(_) => DEFAULT_BEZIER_SEGMENTS,
            Curve::Helix(h) => {
                ((f64::from(DEFAULT_HELIX_SEGMENTS_PER_TURN) * h.turns).ceil() as u32).max(1)
            }
            Curve::Polygon(p) => p.sides(),
            Curve::Polyline(p) => {
                let n = p.points().len() as u32;
                if p.is_closed() {
                    n
                } else {
                    n.saturating_sub(1)
                }
            }
        }
    }

    /// Generates the curve's points. `segments` is interpreted per the table in
    /// the module documentation and capped at [`MAX_SEGMENTS`].
    ///
    /// # Errors
    /// [`GeomError::TooFewSegments`] when `segments` is below
    /// [`Curve::min_segments`] for a kind that uses it.
    pub fn tessellate(&self, segments: u32) -> Result<Tessellation, GeomError> {
        if self.has_segment_count() {
            check_segments(segments, self.min_segments())?;
        }
        let segments = segments.min(MAX_SEGMENTS);

        let tess = match self {
            Curve::Arc(arc) => {
                let step = arc.sweep() / f64::from(segments);
                let mut points: Vec<DVec3> = (0..segments)
                    .map(|i| {
                        arc.frame
                            .point_at(arc.radius, arc.start_angle + step * f64::from(i))
                    })
                    .collect();
                // The end point is computed from the end angle itself, not by
                // accumulation, so it matches `end_point()` exactly.
                points.push(arc.end_point());
                Tessellation {
                    points,
                    closed: false,
                }
            }
            Curve::Circle(circle) => {
                let step = TAU / f64::from(segments);
                Tessellation {
                    points: (0..segments)
                        .map(|i| circle.frame.point_at(circle.radius, step * f64::from(i)))
                        .collect(),
                    closed: true,
                }
            }
            Curve::Polygon(poly) => {
                let step = TAU / f64::from(poly.sides);
                Tessellation {
                    points: (0..poly.sides)
                        .map(|i| poly.frame.point_at(poly.radius, step * f64::from(i)))
                        .collect(),
                    closed: true,
                }
            }
            Curve::Bezier(bez) => {
                let mut points = Vec::with_capacity(bez.spans() * segments as usize + 1);
                for span in 0..bez.spans() {
                    // Each span contributes its start and interior points; the
                    // shared endpoint is added once, by the next span or at the end.
                    for i in 0..segments {
                        points.push(bez.evaluate(span, f64::from(i) / f64::from(segments)));
                    }
                }
                points.push(*bez.control_points.last().unwrap_or(&DVec3::ZERO));
                Tessellation {
                    points,
                    closed: false,
                }
            }
            Curve::Helix(helix) => {
                let mut points: Vec<DVec3> = (0..segments)
                    .map(|i| helix.point_at(f64::from(i) / f64::from(segments)))
                    .collect();
                points.push(helix.end_point());
                Tessellation {
                    points,
                    closed: false,
                }
            }
            Curve::Polyline(line) => Tessellation {
                points: line.points.clone(),
                closed: line.closed,
            },
        };
        Ok(tess)
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, PI};

    use super::*;

    fn ground_frame() -> CurveFrame {
        CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap()
    }

    fn near(a: DVec3, b: DVec3, tol: f64) -> bool {
        a.distance(b) <= tol
    }

    #[test]
    fn frame_orthonormalises_input() {
        let f = CurveFrame::new(
            DVec3::ZERO,
            DVec3::new(1.0, 0.0, 0.5),
            DVec3::new(0.0, 0.0, 2.0),
        )
        .unwrap();
        assert_eq!(f.normal(), DVec3::Z);
        assert_eq!(f.x_axis(), DVec3::X);
        assert_eq!(f.y_axis(), DVec3::Y);
        assert_eq!(
            CurveFrame::new(DVec3::ZERO, DVec3::Z, DVec3::Z),
            Err(GeomError::DegenerateAxis)
        );
        assert_eq!(
            CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::ZERO),
            Err(GeomError::DegenerateNormal)
        );
    }

    #[test]
    fn circle_tessellation_counts_and_radius() {
        let c = Curve::Circle(Circle::new(ground_frame(), 10.0).unwrap());
        for segs in [3, 24, 48] {
            let t = c.tessellate(segs).unwrap();
            assert!(t.closed);
            assert_eq!(t.points.len(), segs as usize);
            assert_eq!(t.edge_count(), segs as usize);
            for p in &t.points {
                assert!((p.length() - 10.0).abs() < 1e-12);
            }
        }
        assert!(matches!(
            c.tessellate(2),
            Err(GeomError::TooFewSegments { .. })
        ));
    }

    #[test]
    fn circle_regenerates_deterministically() {
        // Changing 24 → 48 → 24 must give identical points .
        let c = Curve::Circle(Circle::new(ground_frame(), 3.0).unwrap());
        let a = c.tessellate(24).unwrap();
        let _ = c.tessellate(48).unwrap();
        assert_eq!(c.tessellate(24).unwrap(), a);
    }

    #[test]
    fn arc_endpoints_are_exact() {
        let arc = Arc::new(ground_frame(), 5.0, 0.0, FRAC_PI_2).unwrap();
        let t = Curve::Arc(arc).tessellate(12).unwrap();
        assert!(!t.closed);
        assert_eq!(t.points.len(), 13);
        assert_eq!(t.points[0], arc.start_point());
        assert_eq!(*t.points.last().unwrap(), arc.end_point());
        assert!(near(arc.end_point(), DVec3::new(0.0, 5.0, 0.0), 1e-15));
        assert!((arc.length() - 5.0 * FRAC_PI_2).abs() < 1e-15);
    }

    #[test]
    fn arc_sweep_validation() {
        assert!(matches!(
            Arc::new(ground_frame(), 1.0, 1.0, 1.0),
            Err(GeomError::InvalidSweep(_))
        ));
        assert!(matches!(
            Arc::new(ground_frame(), 1.0, PI, 0.0),
            Err(GeomError::InvalidSweep(_))
        ));
        assert!(matches!(
            Arc::new(ground_frame(), 1.0, 0.0, 7.0),
            Err(GeomError::InvalidSweep(_))
        ));
        assert!(Arc::new(ground_frame(), 1.0, 0.0, TAU).is_ok());
    }

    #[test]
    fn radius_limits() {
        assert!(matches!(
            Circle::new(ground_frame(), 0.0),
            Err(GeomError::NonPositiveRadius(_))
        ));
        assert!(matches!(
            Circle::new(ground_frame(), 5e-5),
            Err(GeomError::NonPositiveRadius(_))
        ));
        // A 0.001 mm circle is valid (no minimum face size above TOL_MERGE).
        let c = Curve::Circle(Circle::new(ground_frame(), 0.001).unwrap());
        let t = c.tessellate(24).unwrap();
        for p in &t.points {
            assert!((p.length() - 0.001).abs() < 1e-18);
        }
    }

    #[test]
    fn polygon_ignores_segment_count_and_is_not_smoothable() {
        let p = Curve::Polygon(Polygon::new(ground_frame(), 2.0, 6).unwrap());
        assert_eq!(p.tessellate(1).unwrap().points.len(), 6);
        assert_eq!(p.tessellate(99).unwrap().points.len(), 6);
        assert!(!p.is_smoothable());
        assert!(p.is_closed());
        assert_eq!(
            Polygon::new(ground_frame(), 2.0, 2),
            Err(GeomError::TooFewSides(2))
        );
        // First vertex on the x-axis.
        assert!(near(
            p.tessellate(6).unwrap().points[0],
            DVec3::new(2.0, 0.0, 0.0),
            1e-15
        ));
    }

    #[test]
    fn bezier_span_structure_and_endpoints() {
        let cps = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(1.0, 2.0, 0.0),
            DVec3::new(3.0, 2.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(5.0, -2.0, 0.0),
            DVec3::new(7.0, -2.0, 0.0),
            DVec3::new(8.0, 0.0, 0.0),
        ];
        let bez = Bezier::new(cps.clone()).unwrap();
        assert_eq!(bez.spans(), 2);
        let t = Curve::Bezier(bez.clone()).tessellate(10).unwrap();
        assert_eq!(t.points.len(), 21);
        assert_eq!(t.points[0], cps[0]);
        assert_eq!(t.points[10], cps[3], "shared span endpoint is exact");
        assert_eq!(t.points[20], cps[6]);
        // Symmetric span: midpoint at x = 2.
        assert!(near(bez.evaluate(0, 0.5), DVec3::new(2.0, 1.5, 0.0), 1e-15));
        assert_eq!(
            Bezier::new(cps[..5].to_vec()),
            Err(GeomError::BezierControlPoints(5))
        );
    }

    #[test]
    fn polyline_passthrough() {
        let pts = vec![DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0)];
        let open = Curve::Polyline(Polyline::new(pts.clone(), false).unwrap());
        let t = open.tessellate(0).unwrap();
        assert_eq!(t.points, pts);
        assert_eq!(t.edge_count(), 2);
        let closed = Curve::Polyline(Polyline::new(pts, true).unwrap());
        assert_eq!(closed.tessellate(0).unwrap().edge_count(), 3);
        assert!(matches!(
            Polyline::new(vec![DVec3::ZERO, DVec3::X], true),
            Err(GeomError::TooFewPoints {
                required: 3,
                given: 2
            })
        ));
    }

    #[test]
    fn tilted_circle_lies_in_its_plane() {
        let n = DVec3::new(1.0, -2.0, 3.0);
        let frame =
            CurveFrame::new(DVec3::new(10.0, 20.0, -5.0), DVec3::new(0.0, 3.0, 2.0), n).unwrap();
        let c = Curve::Circle(Circle::new(frame, 7.5).unwrap());
        let plane = crate::plane::Plane::from_point_normal(frame.center(), n).unwrap();
        for p in c.tessellate(36).unwrap().points {
            assert!(plane.signed_distance(p).abs() < 1e-12);
            assert!((p.distance(frame.center()) - 7.5).abs() < 1e-12);
        }
    }

    #[test]
    fn segment_cap() {
        let c = Curve::Circle(Circle::new(ground_frame(), 1.0).unwrap());
        assert_eq!(
            c.tessellate(1_000_000).unwrap().points.len(),
            MAX_SEGMENTS as usize
        );
    }

    #[test]
    fn helix_rises_exactly_and_ends_exactly() {
        // Acceptance: radius 5, pitch 2, 3 turns rises exactly 6 mm, with
        // endpoints exact in f64.
        let h = Helix::new(ground_frame(), 5.0, 5.0, 2.0, 3.0, false).unwrap();
        assert_eq!(h.start_point(), DVec3::new(5.0, 0.0, 0.0));
        assert_eq!(h.end_point(), DVec3::new(5.0, 0.0, 6.0));
        let t = Curve::Helix(h.clone()).tessellate(72).unwrap();
        assert_eq!(t.points.len(), 73);
        assert_eq!(t.points[72], DVec3::new(5.0, 0.0, 6.0));
        assert!(!t.closed);
        // A quarter turn in, counter-clockwise, half a pitch up / 4.
        assert!(near(
            h.point_at(1.0 / 12.0),
            DVec3::new(0.0, 5.0, 0.5),
            1e-12
        ));
        let left = Helix::new(ground_frame(), 5.0, 5.0, 2.0, 3.0, true).unwrap();
        assert!(near(
            left.point_at(1.0 / 12.0),
            DVec3::new(0.0, -5.0, 0.5),
            1e-12
        ));
    }

    #[test]
    fn flat_spiral_and_cone() {
        let spiral = Helix::new(ground_frame(), 1.0, 4.0, 0.0, 2.0, false).unwrap();
        assert_eq!(spiral.end_point(), DVec3::new(4.0, 0.0, 0.0));
        assert!(spiral.point_at(0.3).z.abs() < 1e-15, "flat");
        let cone = Helix::new(ground_frame(), 2.0, 0.0, 1.0, 1.0, false).unwrap();
        assert!(near(cone.end_point(), DVec3::new(0.0, 0.0, 1.0), 1e-15));
        assert!(Helix::new(ground_frame(), 0.0, 0.0, 1.0, 1.0, false).is_err());
        assert!(Helix::new(ground_frame(), 1.0, 1.0, 1.0, 0.0, false).is_err());
    }
}

#[cfg(test)]
mod stored_tests {
    use super::*;

    #[test]
    fn stored_frame_round_trips_bit_for_bit_and_rejects_damage() {
        let f = CurveFrame::new(
            DVec3::new(1.0, 2.0, 3.0),
            DVec3::new(1.0, 0.3, 0.0),
            DVec3::new(0.2, 0.1, 1.0),
        )
        .expect("frame");
        let g = CurveFrame::from_stored(f.center(), f.x_axis(), f.normal()).expect("stored");
        assert_eq!(f, g);
        assert!(CurveFrame::from_stored(DVec3::ZERO, DVec3::X, DVec3::X).is_err());
        assert!(CurveFrame::from_stored(DVec3::ZERO, DVec3::X * 2.0, DVec3::Z).is_err());
        assert!(CurveFrame::from_stored(DVec3::ZERO, DVec3::X, DVec3::Z * 0.5).is_err());
    }
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    fn circle() -> Curve {
        let f = CurveFrame::new(DVec3::new(1.0, 2.0, 0.0), DVec3::X, DVec3::Z).unwrap();
        Curve::Circle(Circle::new(f, 3.0).unwrap())
    }

    /// Every tessellated point of the transformed curve is the transformed
    /// point of the original.
    fn maps_points(c: &Curve, m: &DMat4) {
        let moved = c.transformed(m).expect("a similarity");
        let a = c.tessellate(24).unwrap().points;
        let b = moved.tessellate(24).unwrap().points;
        for (p, q) in a.iter().zip(&b) {
            assert!(m.transform_point3(*p).distance(*q) < 1e-9, "{p} → {q}");
        }
    }

    #[test]
    fn rotation_mirror_and_scale_keep_circles() {
        let c = circle();
        maps_points(&c, &DMat4::from_rotation_z(0.7));
        maps_points(&c, &DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0)));
        maps_points(&c, &DMat4::from_scale(DVec3::splat(2.5)));
    }

    #[test]
    fn stretching_is_refused() {
        assert!(
            circle()
                .transformed(&DMat4::from_scale(DVec3::new(2.0, 1.0, 1.0)))
                .is_none()
        );
    }
}
