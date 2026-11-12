//! SketchSmith — inference engine | v1.0.0 | Thursday 12 November 2026
//!
//! Finds the point the cursor means: snapping to geometry,
//! axis directions, parallels and perpendiculars, tracked points, and locks.
//!
//! **Candidates, strongest first**
//! 1. Point snaps within the point radius (10 px): Endpoint, Midpoint,
//!    Intersection (of edges and guide lines), Centre, Quadrant, Face centre
//!    and Guide point, then Perpendicular (the foot
//!    of the perpendicular from the last point to an edge), Tangent (where a line
//!    from the last point touches a circle or arc) and From Points (where axis
//!    lines from two tracked points cross). Centres and quadrants come from true
//!    curves, so a circle's centre snaps without hovering its edge.
//! 2. On edge or on a guide line, within the edge radius (6 px); the nearer
//!    wins.
//! 3. Direction lines from the last point (and from tracked points): the red,
//!    green and blue drawing axes, parallel / perpendicular to the referenced
//!    edge, and Tangent to Edge (continuing the arc or edge the operation
//!    started from), within 8 px of the line.
//! 4. On face (the face under the cursor).
//! 5. Fallback: the drawing ground plane, or the caller's fallback point.
//!
//! Direction lines rank above On face, as in SketchUp: drawing along the red axis
//! across a face keeps the red-axis inference.
//!
//! **Locks** constrain the result to a line or plane: the caller builds a
//! [`Lock`] from the current inference (Shift) or an arrow key, and the
//! engine projects the cursor or the snapped point onto it.
//!
//! Points hidden behind the face under the cursor are not offered. Guides
//! are offered only while they are shown ([`Query::guides`]).

use smith_geom::intersect::{SegmentIntersection, closest_points_segments, segment_segment};
use smith_geom::plane::Plane;
use smith_geom::{DVec2, DVec3};
use smith_model::{EdgeId, Entities, Guide, Pickable};

use crate::bvh::FaceBvh;

/// Point-snap radius in pixels.
pub const POINT_PX: f64 = 10.0;
/// Edge-snap radius in pixels.
pub const EDGE_PX: f64 = 6.0;
/// Direction-line snap radius in pixels.
pub const LINE_PX: f64 = 8.0;

/// A drawing axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// X.
    Red,
    /// Y.
    Green,
    /// Z.
    Blue,
}

impl Axis {
    /// All three, in order.
    pub const ALL: [Axis; 3] = [Axis::Red, Axis::Green, Axis::Blue];

    /// Display name: the axis letter, X, Y or Z (shown on screen in the
    /// axis's colour, red, green or blue).
    pub fn name(self) -> &'static str {
        match self {
            Axis::Red => "X",
            Axis::Green => "Y",
            Axis::Blue => "Z",
        }
    }
}

/// The drawing axes: an origin and an orthonormal right-handed frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawingAxes {
    /// Origin.
    pub origin: DVec3,
    /// Red direction.
    pub x: DVec3,
    /// Green direction.
    pub y: DVec3,
    /// Blue direction.
    pub z: DVec3,
}

impl Default for DrawingAxes {
    fn default() -> Self {
        Self {
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::Y,
            z: DVec3::Z,
        }
    }
}

impl DrawingAxes {
    /// Direction of an axis.
    pub fn dir(&self, a: Axis) -> DVec3 {
        match a {
            Axis::Red => self.x,
            Axis::Green => self.y,
            Axis::Blue => self.z,
        }
    }

    /// Axes from an origin, a point along red, and a point toward green
    /// (Axes tool). `None` if the points are degenerate.
    pub fn from_points(origin: DVec3, red: DVec3, green: DVec3) -> Option<Self> {
        let x = (red - origin).try_normalize()?;
        let g = green - origin;
        let y = (g - x * g.dot(x)).try_normalize()?;
        Some(Self {
            origin,
            x,
            y,
            z: x.cross(y),
        })
    }

    /// Axes on a face (Align Axes): origin at `point`, blue along `normal`, red
    /// along the face's most horizontal direction.
    pub fn on_face(point: DVec3, normal: DVec3) -> Option<Self> {
        let z = normal.try_normalize()?;
        let x = DVec3::Z.cross(z).try_normalize().unwrap_or(DVec3::X);
        let y = z.cross(x);
        Some(Self {
            origin: point,
            x,
            y,
            z,
        })
    }

    /// The drawing ground plane (red–green plane).
    pub fn ground(&self) -> Option<Plane> {
        Plane::from_point_normal(self.origin, self.z).ok()
    }
}

/// What kind of inference produced a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A vertex.
    Endpoint,
    /// The middle of an edge.
    Midpoint,
    /// Where two edges meet, or a guide line meets an edge or another
    /// guide line.
    Intersection,
    /// The centre of a circle or arc.
    Centre,
    /// A 0°/90°/180°/270° point of a circle or arc.
    Quadrant,
    /// The centroid of a face.
    FaceCentre,
    /// The centre of a group's or component's bounding box.
    BoxCentre,
    /// A guide point.
    GuidePoint,
    /// The foot of the perpendicular from the last point to an edge.
    PerpendicularPoint,
    /// Where a line from the last point touches a circle or arc.
    TangentPoint,
    /// Where axis lines from two tracked points (or the last point) cross.
    Crossing,
    /// Somewhere along an edge.
    OnEdge,
    /// Somewhere along a guide line.
    OnGuide,
    /// Along a drawing axis.
    OnAxis(Axis),
    /// Parallel to the referenced edge.
    Parallel,
    /// Perpendicular to the referenced edge.
    Perpendicular,
    /// Continuing tangent to the arc or edge the operation started from.
    Tangent,
    /// On the face under the cursor.
    OnFace,
    /// Nothing nearby: in empty space.
    Free,
}

impl Kind {
    /// Tooltip label.
    pub fn label(self) -> String {
        match self {
            Kind::Endpoint => "Endpoint".into(),
            Kind::Midpoint => "Midpoint".into(),
            Kind::Intersection => "Intersection".into(),
            Kind::Centre => "Centre".into(),
            Kind::Quadrant => "Quadrant".into(),
            Kind::FaceCentre => "Face Centre".into(),
            Kind::BoxCentre => "Bounding Box Centre".into(),
            Kind::GuidePoint => "Guide Point".into(),
            Kind::PerpendicularPoint => "Perpendicular".into(),
            Kind::TangentPoint => "Tangent".into(),
            Kind::Crossing => "From Points".into(),
            Kind::OnEdge => "On Edge".into(),
            Kind::OnGuide => "On Guide".into(),
            Kind::OnAxis(a) => format!("On {} Axis", a.name()),
            Kind::Parallel => "Parallel to Edge".into(),
            Kind::Perpendicular => "Perpendicular to Edge".into(),
            Kind::Tangent => "Tangent to Edge".into(),
            Kind::OnFace => "On Face".into(),
            Kind::Free => String::new(),
        }
    }

    fn rank(self) -> u8 {
        match self {
            Kind::Endpoint => 0,
            Kind::Midpoint => 1,
            Kind::Intersection => 2,
            Kind::Centre => 3,
            Kind::Quadrant => 4,
            Kind::FaceCentre | Kind::BoxCentre | Kind::GuidePoint => 5,
            Kind::PerpendicularPoint | Kind::TangentPoint | Kind::Crossing => 6,
            Kind::OnEdge | Kind::OnGuide => 7,
            Kind::OnAxis(_) | Kind::Parallel | Kind::Perpendicular | Kind::Tangent => 8,
            Kind::OnFace => 9,
            Kind::Free => 10,
        }
    }
}

/// Which snap types are switched on (the snap filter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SnapFilter {
    /// Vertices.
    pub endpoint: bool,
    /// Edge midpoints.
    pub midpoint: bool,
    /// Edge–edge intersections.
    pub intersection: bool,
    /// Curve centres.
    pub centre: bool,
    /// Curve quadrant points.
    pub quadrant: bool,
    /// Face centroids.
    pub face_centre: bool,
    /// On edges.
    pub on_edge: bool,
    /// On faces.
    pub on_face: bool,
    /// Axis directions.
    pub axes: bool,
    /// Parallel / perpendicular directions.
    pub parallel: bool,
    /// Perpendicular foot on an edge.
    pub perpendicular: bool,
    /// Tangent points on curves, and the tangent-to-edge direction.
    pub tangent: bool,
    /// Guide points and guide lines.
    pub guides: bool,
}

impl Default for SnapFilter {
    fn default() -> Self {
        Self {
            endpoint: true,
            midpoint: true,
            intersection: true,
            centre: true,
            quadrant: true,
            face_centre: true,
            on_edge: true,
            on_face: true,
            axes: true,
            parallel: true,
            perpendicular: true,
            tangent: true,
            guides: true,
        }
    }
}

/// The snap types the snap-only key can restrict to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum OnlyKind {
    /// Endpoints only.
    Endpoint,
    /// Midpoints only.
    Midpoint,
    /// Centres only (default — placing stones).
    #[default]
    Centre,
    /// Quadrants only.
    Quadrant,
    /// Face centres only.
    FaceCentre,
}

impl OnlyKind {
    fn kind(self) -> Kind {
        match self {
            OnlyKind::Endpoint => Kind::Endpoint,
            OnlyKind::Midpoint => Kind::Midpoint,
            OnlyKind::Centre => Kind::Centre,
            OnlyKind::Quadrant => Kind::Quadrant,
            OnlyKind::FaceCentre => Kind::FaceCentre,
        }
    }
}

impl SnapFilter {
    fn allows(&self, k: Kind) -> bool {
        match k {
            Kind::Endpoint => self.endpoint,
            Kind::Midpoint => self.midpoint,
            Kind::Intersection => self.intersection,
            Kind::Centre => self.centre,
            Kind::Quadrant => self.quadrant,
            Kind::FaceCentre | Kind::BoxCentre => self.face_centre,
            Kind::OnEdge => self.on_edge,
            Kind::OnFace => self.on_face,
            Kind::OnAxis(_) | Kind::Crossing => self.axes,
            Kind::Parallel | Kind::Perpendicular => self.parallel,
            Kind::PerpendicularPoint => self.perpendicular,
            Kind::TangentPoint | Kind::Tangent => self.tangent,
            Kind::GuidePoint | Kind::OnGuide => self.guides,
            Kind::Free => true,
        }
    }
}

/// A constraint on the result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lock {
    /// No constraint.
    None,
    /// Stay on the line through `origin` along `dir`, reported as `kind`.
    Line {
        /// A point on the line.
        origin: DVec3,
        /// Unit direction.
        dir: DVec3,
        /// The inference the lock came from.
        kind: Kind,
    },
    /// Stay on a plane (locked On Face).
    Plane(Plane),
}

/// The inferred point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inference {
    /// The point.
    pub point: DVec3,
    /// What produced it.
    pub kind: Kind,
    /// For direction inferences: where the guiding line starts (drawn dotted).
    pub from: Option<DVec3>,
    /// For a [`Kind::Crossing`]: where the second guiding line starts.
    pub from2: Option<DVec3>,
    /// The entity snapped to, if any.
    pub entity: Option<Pickable>,
    /// `true` when the result came through a lock.
    pub locked: bool,
    /// The normal of the face the point lies on, when the caller knows it
    /// better than `entity` can say (a face inside a group).
    pub normal: Option<DVec3>,
}

/// Everything the engine needs for one query.
pub struct Query<'a> {
    /// Geometry of the active context.
    pub entities: &'a Entities,
    /// Face BVH of the same context.
    pub bvh: &'a FaceBvh,
    /// Cursor ray origin.
    pub origin: DVec3,
    /// Cursor ray unit direction.
    pub dir: DVec3,
    /// Cursor position in screen pixels.
    pub cursor: DVec2,
    /// World → screen pixels (`None` behind the camera).
    pub to_screen: &'a dyn Fn(DVec3) -> Option<DVec2>,
    /// The previous point of the operation in progress, if any.
    pub last: Option<DVec3>,
    /// Drawing axes.
    pub axes: &'a DrawingAxes,
    /// Edge giving parallel / perpendicular directions.
    pub reference_edge: Option<EdgeId>,
    /// Tracked points (hover dwell), extra origins for axis lines.
    pub tracked: &'a [DVec3],
    /// Direction continuing the arc or edge the operation started from, offered
    /// from the last point as Tangent to Edge (see [`continuation`]).
    pub tangent: Option<DVec3>,
    /// Active constraint.
    pub lock: Lock,
    /// Enabled snap types.
    pub filter: &'a SnapFilter,
    /// Snap-only restriction (held key), if active.
    pub only: Option<OnlyKind>,
    /// Point to use when nothing else applies and no plane is hit.
    pub fallback: DVec3,
    /// Centres of groups' and components' boxes, each with its group,
    /// offered as [`Kind::BoxCentre`] even inside the solid: the caller
    /// passes only the group under the cursor.
    pub box_centres: &'a [(DVec3, Pickable)],
    /// Whether guides are shown, and so offered.
    pub guides: bool,
}

#[derive(Debug, Clone, Copy)]
struct Cand {
    point: DVec3,
    kind: Kind,
    px: f64,
    from: Option<DVec3>,
    from2: Option<DVec3>,
    entity: Option<Pickable>,
}

/// How far from the cursor ray, at depth `t` along it, a point can lie and
/// still be within snapping range on screen (with a margin): world units
/// per screen pixel measured at two depths, so it holds for perspective and
/// parallel views alike. `None` when it cannot be measured (the ray's
/// points off screen): nothing is passed over then.
fn ray_reach(q: &Query<'_>) -> Option<impl Fn(f64) -> f64> {
    let side = q.dir.any_orthonormal_vector();
    let per_px = |t: f64| -> Option<f64> {
        let p = q.origin + q.dir * t;
        let (a, b) = ((q.to_screen)(p)?, (q.to_screen)(p + side)?);
        let px = a.distance(b);
        (px > 1e-12).then(|| 1.0 / px)
    };
    let (near, far) = (per_px(1.0)?, per_px(1001.0)?);
    // Units per pixel grow linearly with depth (none in a parallel view).
    let slope = ((far - near) / 1000.0).max(0.0);
    let base = near - slope;
    let px = POINT_PX.max(EDGE_PX) * 2.0;
    Some(move |t: f64| (base + slope * t.max(0.0)).max(0.0) * px + 1e-6)
}

/// The closest approach of the ray from `o` along unit `d` to segment
/// `a`–`b`: the ray's parameter there, the segment's (0 to 1), the gap,
/// and the segment's point.
fn ray_segment(o: DVec3, d: DVec3, a: DVec3, b: DVec3) -> (f64, f64, f64, DVec3) {
    let ab = b - a;
    let w = a - o;
    let (aa, ab_d, ww) = (ab.dot(ab), ab.dot(d), w.dot(d));
    let denom = aa - ab_d * ab_d;
    let mut s = if denom > 1e-12 {
        ((ab_d * ww - ab.dot(w)) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut t = (a + ab * s - o).dot(d);
    if t < 0.0 {
        t = 0.0;
        s = if aa > 1e-12 {
            ((o - a).dot(ab) / aa).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    let p = a + ab * s;
    (t, s, p.distance(o + d * t), p)
}

/// Runs inference for one cursor position.
pub fn infer(q: &Query<'_>) -> Inference {
    let face_hit = q.bvh.ray_cast(q.origin, q.dir);
    let depth_limit = face_hit.map_or(f64::INFINITY, |h| h.t);
    let visible = |p: DVec3| {
        let t = (p - q.origin).dot(q.dir);
        t > 0.0 && t <= depth_limit * (1.0 + 1e-6) + 1e-6
    };
    let allowed = |k: Kind| match q.only {
        Some(only) => k == only.kind(),
        None => q.filter.allows(k),
    };

    let mut points: Vec<Cand> = Vec::new();
    // A box centre lies inside its group, so it is offered without the depth
    // test (the caller passes only the group under the cursor).
    let push_point = |p: DVec3, kind: Kind, entity: Option<Pickable>, points: &mut Vec<Cand>| {
        if !allowed(kind) || (kind != Kind::BoxCentre && !visible(p)) {
            return;
        }
        if let Some(s) = (q.to_screen)(p) {
            let px = s.distance(q.cursor);
            if px <= POINT_PX {
                points.push(Cand {
                    point: p,
                    kind,
                    px,
                    from: None,
                    from2: None,
                    entity,
                });
            }
        }
    };

    // Point snaps on edges, and on-edge candidates. Edges too far from the
    // cursor ray to come within snapping range are passed over before
    // anything is projected (the costly part on large models).
    let reach = ray_reach(q);
    let mut near_edges: Vec<(EdgeId, DVec3, DVec3)> = Vec::new();
    let mut on_edge: Option<Cand> = None;
    for e in q.entities.edge_ids() {
        let Some((a, b)) = q.entities.edge_points(e) else {
            continue;
        };
        if let Some(reach) = &reach {
            let (t, _, gap, _) = ray_segment(q.origin, q.dir, a, b);
            if gap > reach(t) {
                continue;
            }
        }
        let pe = Some(Pickable::Edge(e));
        push_point(a, Kind::Endpoint, pe, &mut points);
        push_point(b, Kind::Endpoint, pe, &mut points);
        push_point((a + b) * 0.5, Kind::Midpoint, pe, &mut points);
        let (Some(sa), Some(sb)) = ((q.to_screen)(a), (q.to_screen)(b)) else {
            continue;
        };
        let px = point_segment_px(q.cursor, sa, sb);
        if px <= EDGE_PX {
            near_edges.push((e, a, b));
            let far = q.origin + q.dir * 1e9;
            let (_, _, _, on) = closest_points_segments(q.origin, far, a, b);
            if allowed(Kind::OnEdge) && visible(on) && on_edge.is_none_or(|c| px < c.px) {
                on_edge = Some(Cand {
                    point: on,
                    kind: Kind::OnEdge,
                    px,
                    from: None,
                    from2: None,
                    entity: pe,
                });
            }
            if let Some(last) = q.last
                && let Some(foot) = perpendicular_foot(last, a, b)
            {
                push_point(foot, Kind::PerpendicularPoint, pe, &mut points);
            }
        }
    }
    // Intersections of edges near the cursor.
    for (i, (_, a0, a1)) in near_edges.iter().enumerate() {
        for (_, b0, b1) in &near_edges[i + 1..] {
            // Edges meeting at a shared vertex are an Endpoint, not an Intersection.
            if let SegmentIntersection::Point { point, .. } = segment_segment(*a0, *a1, *b0, *b1)
                && ![a0, a1, b0, b1]
                    .iter()
                    .any(|e| smith_geom::tolerance::coincident(**e, point))
            {
                push_point(point, Kind::Intersection, None, &mut points);
            }
        }
    }
    // Centres and quadrants of true curves.
    for c in q.entities.curve_ids() {
        let Some(rec) = q.entities.curve(c) else {
            continue;
        };
        let Some((centre, quads)) = curve_snaps(rec.curve()) else {
            continue;
        };
        let pe = rec.edges().first().map(|e| Pickable::Edge(*e));
        push_point(centre, Kind::Centre, pe, &mut points);
        for p in quads {
            push_point(p, Kind::Quadrant, pe, &mut points);
        }
        if let Some(last) = q.last {
            for p in tangent_points(rec.curve(), last) {
                push_point(p, Kind::TangentPoint, pe, &mut points);
            }
        }
    }
    // Centres of groups' and components' boxes.
    for (c, owner) in q.box_centres {
        push_point(*c, Kind::BoxCentre, Some(*owner), &mut points);
    }
    // Guides: guide points, and guide lines near the cursor (for On Guide
    // and their crossings with edges and with each other).
    let mut on_guide: Option<Cand> = None;
    let mut near_guides: Vec<(DVec3, DVec3)> = Vec::new();
    if q.guides {
        for (g, guide) in q.entities.guides() {
            let pg = Some(Pickable::Guide(g));
            match *guide {
                Guide::Point { point } => push_point(point, Kind::GuidePoint, pg, &mut points),
                Guide::Line { point, direction } => {
                    let on = closest_on_line_to_ray(point, direction, q.origin, q.dir);
                    let Some(s) = (q.to_screen)(on) else {
                        continue;
                    };
                    let px = s.distance(q.cursor);
                    if px > EDGE_PX || !visible(on) || !allowed(Kind::OnGuide) {
                        continue;
                    }
                    near_guides.push((point, direction));
                    if on_guide.is_none_or(|c| px < c.px) {
                        on_guide = Some(Cand {
                            point: on,
                            kind: Kind::OnGuide,
                            px,
                            from: None,
                            from2: None,
                            entity: pg,
                        });
                    }
                }
            }
        }
        for (i, (p0, d0)) in near_guides.iter().enumerate() {
            for (_, a, b) in &near_edges {
                if let Some(x) = line_segment_meet(*p0, *d0, *a, *b) {
                    push_point(x, Kind::Intersection, None, &mut points);
                }
            }
            for (p1, d1) in &near_guides[i + 1..] {
                if let Some(x) = lines_meet(*p0, *d0, *p1, *d1) {
                    push_point(x, Kind::Intersection, None, &mut points);
                }
            }
        }
    }
    // Centre of the face under the cursor.
    if let Some(hit) = face_hit
        && let Some(c) = face_centroid(q.entities, hit.face)
    {
        push_point(
            c,
            Kind::FaceCentre,
            Some(Pickable::Face(hit.face)),
            &mut points,
        );
    }

    // Where axis lines from two tracking origins cross ("From Points").
    let origins: Vec<DVec3> = q
        .last
        .into_iter()
        .chain(q.tracked.iter().copied())
        .collect();
    if allowed(Kind::Crossing) {
        for (i, o1) in origins.iter().enumerate() {
            for o2 in &origins[i + 1..] {
                for a1 in Axis::ALL {
                    for a2 in Axis::ALL {
                        let Some(p) = line_crossing(*o1, q.axes.dir(a1), *o2, q.axes.dir(a2))
                        else {
                            continue;
                        };
                        if !visible(p) {
                            continue;
                        }
                        if let Some(s) = (q.to_screen)(p) {
                            let px = s.distance(q.cursor);
                            if px <= POINT_PX {
                                points.push(Cand {
                                    point: p,
                                    kind: Kind::Crossing,
                                    px,
                                    from: Some(*o1),
                                    from2: Some(*o2),
                                    entity: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    let best_point = points.into_iter().min_by(|a, b| {
        (a.kind.rank(), a.px)
            .partial_cmp(&(b.kind.rank(), b.px))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // A lock constrains whatever was found (or the bare cursor ray).
    match q.lock {
        Lock::Line { origin, dir, kind } => {
            let point = match best_point {
                Some(c) => origin + dir * (c.point - origin).dot(dir),
                None => closest_on_line_to_ray(origin, dir, q.origin, q.dir),
            };
            return Inference {
                point,
                kind,
                from: Some(origin),
                from2: None,
                entity: best_point.and_then(|c| c.entity),
                locked: true,
                normal: None,
            };
        }
        Lock::Plane(plane) => {
            let point = match best_point {
                Some(c) => plane.project(c.point),
                None => ray_plane(q.origin, q.dir, &plane).unwrap_or(q.fallback),
            };
            return Inference {
                point,
                kind: Kind::OnFace,
                from: None,
                from2: None,
                entity: None,
                locked: true,
                normal: None,
            };
        }
        Lock::None => {}
    }

    if let Some(c) = best_point {
        return c.into_inference();
    }
    // On edge or on guide: the nearer of the two.
    let on_line = match (on_edge, on_guide) {
        (Some(e), Some(g)) => Some(if g.px < e.px { g } else { e }),
        (e, g) => e.or(g),
    };
    if let Some(c) = on_line {
        return c.into_inference();
    }

    // Direction lines from the last point and from tracked points.
    if q.only.is_none() {
        let mut best_line: Option<Cand> = None;
        let mut consider = |origin: DVec3, dir: DVec3, kind: Kind| {
            if !allowed(kind) {
                return;
            }
            let p = closest_on_line_to_ray(origin, dir, q.origin, q.dir);
            if (p - origin).length() < 1e-9 {
                return;
            }
            if let Some(s) = (q.to_screen)(p) {
                let px = s.distance(q.cursor);
                if px <= LINE_PX && best_line.is_none_or(|b| px < b.px) {
                    best_line = Some(Cand {
                        point: p,
                        kind,
                        px,
                        from: Some(origin),
                        from2: None,
                        entity: None,
                    });
                }
            }
        };
        for o in &origins {
            for a in Axis::ALL {
                consider(*o, q.axes.dir(a), Kind::OnAxis(a));
            }
        }
        if let (Some(last), Some(e)) = (q.last, q.reference_edge)
            && let Some((a, b)) = q.entities.edge_points(e)
            && let Some(d) = (b - a).try_normalize()
        {
            consider(last, d, Kind::Parallel);
            let n = face_hit
                .and_then(|h| q.entities.face(h.face))
                .map_or(q.axes.z, |f| f.plane().normal());
            if let Some(perp) = d.cross(n).try_normalize() {
                consider(last, perp, Kind::Perpendicular);
            }
        }
        if let (Some(last), Some(t)) = (q.last, q.tangent.and_then(DVec3::try_normalize)) {
            consider(last, t, Kind::Tangent);
        }
        if let Some(c) = best_line {
            return c.into_inference();
        }
    }

    if let Some(hit) = face_hit
        && allowed(Kind::OnFace)
    {
        return Inference {
            point: hit.point,
            kind: Kind::OnFace,
            from: None,
            from2: None,
            entity: Some(Pickable::Face(hit.face)),
            locked: false,
            normal: None,
        };
    }

    // Empty space: the drawing ground plane, else the fallback.
    let point = q
        .axes
        .ground()
        .and_then(|g| ray_plane(q.origin, q.dir, &g))
        .unwrap_or(q.fallback);
    Inference {
        point,
        kind: Kind::Free,
        from: None,
        from2: None,
        entity: None,
        locked: false,
        normal: None,
    }
}

impl Cand {
    fn into_inference(self) -> Inference {
        Inference {
            point: self.point,
            kind: self.kind,
            from: self.from,
            from2: self.from2,
            entity: self.entity,
            locked: false,
            normal: None,
        }
    }
}

/// Builds the Shift lock for an inference, given the last point.
pub fn lock_for(
    inf: &Inference,
    last: Option<DVec3>,
    entities: &Entities,
    entity_plane: Option<Plane>,
) -> Lock {
    match inf.kind {
        Kind::OnAxis(_) | Kind::Parallel | Kind::Perpendicular | Kind::Tangent => match (
            inf.from,
            (inf.point - inf.from.unwrap_or(inf.point)).try_normalize(),
        ) {
            (Some(origin), Some(dir)) => Lock::Line {
                origin,
                dir,
                kind: inf.kind,
            },
            _ => Lock::None,
        },
        Kind::OnEdge => match inf.entity {
            Some(Pickable::Edge(e)) => match entities.edge_points(e) {
                Some((a, b)) => match (b - a).try_normalize() {
                    Some(dir) => Lock::Line {
                        origin: a,
                        dir,
                        kind: Kind::OnEdge,
                    },
                    None => Lock::None,
                },
                None => Lock::None,
            },
            _ => Lock::None,
        },
        Kind::OnGuide => match inf.entity {
            Some(Pickable::Guide(g)) => match entities.guide(g) {
                Some(Guide::Line { point, direction }) => Lock::Line {
                    origin: *point,
                    dir: *direction,
                    kind: Kind::OnGuide,
                },
                _ => Lock::None,
            },
            _ => Lock::None,
        },
        Kind::OnFace => entity_plane.map_or(Lock::None, Lock::Plane),
        _ => match last.and_then(|l| (inf.point - l).try_normalize().map(|d| (l, d))) {
            Some((origin, dir)) => Lock::Line {
                origin,
                dir,
                kind: inf.kind,
            },
            None => Lock::None,
        },
    }
}

/// The lock for an arrow key: Right red, Left green, Up blue.
pub fn axis_lock(axis: Axis, last: DVec3, axes: &DrawingAxes) -> Lock {
    Lock::Line {
        origin: last,
        dir: axes.dir(axis),
        kind: Kind::OnAxis(axis),
    }
}

/// Centre and quadrant points of circles and arcs.
fn curve_snaps(curve: &smith_geom::curve::Curve) -> Option<(DVec3, Vec<DVec3>)> {
    use smith_geom::curve::Curve;
    use std::f64::consts::FRAC_PI_2;
    match curve {
        Curve::Circle(c) => {
            let f = c.frame();
            let quads = (0..4)
                .map(|i| f.point_at(c.radius(), FRAC_PI_2 * f64::from(i)))
                .collect();
            Some((f.center(), quads))
        }
        Curve::Arc(a) => {
            let f = a.frame();
            let quads = (0..8)
                .map(|i| FRAC_PI_2 * f64::from(i - 4))
                .filter(|ang| *ang >= a.start_angle() - 1e-12 && *ang <= a.end_angle() + 1e-12)
                .map(|ang| f.point_at(a.radius(), ang))
                .collect();
            Some((f.center(), quads))
        }
        Curve::Polygon(p) => Some((p.frame().center(), Vec::new())),
        _ => None,
    }
}

/// The foot of the perpendicular from `p` to segment `a`–`b`, if it falls
/// strictly inside the segment and is not `p` itself.
fn perpendicular_foot(p: DVec3, a: DVec3, b: DVec3) -> Option<DVec3> {
    use smith_geom::tolerance::coincident;
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 == 0.0 {
        return None;
    }
    let t = (p - a).dot(ab) / len2;
    let foot = a + ab * t;
    let inside = t > 0.0 && t < 1.0 && !coincident(foot, a) && !coincident(foot, b);
    (inside && !coincident(foot, p)).then_some(foot)
}

/// Points where a line from `p` touches a circle or arc (classic tangent
/// construction: the tangent points lie at angle ±acos(r/d) from the direction
/// centre → p). `p` must lie in the curve's plane and outside the circle.
fn tangent_points(curve: &smith_geom::curve::Curve, p: DVec3) -> Vec<DVec3> {
    use smith_geom::curve::Curve;
    use smith_geom::tolerance::TOL_MERGE;
    let (frame, radius, range) = match curve {
        Curve::Circle(c) => (*c.frame(), c.radius(), None),
        Curve::Arc(a) => (
            *a.frame(),
            a.radius(),
            Some((a.start_angle(), a.end_angle())),
        ),
        _ => return Vec::new(),
    };
    let n = frame.normal();
    let w = p - frame.center();
    if w.dot(n).abs() > TOL_MERGE {
        return Vec::new();
    }
    let d = w.length();
    if d <= radius + TOL_MERGE {
        return Vec::new();
    }
    let u = w / d;
    let side = n.cross(u);
    let alpha = (radius / d).acos();
    [alpha, -alpha]
        .into_iter()
        .map(|a| frame.center() + (u * a.cos() + side * a.sin()) * radius)
        .filter(|t| match range {
            None => true,
            Some((start, end)) => {
                let v = *t - frame.center();
                let ang = v.dot(frame.y_axis()).atan2(v.dot(frame.x_axis()));
                let ang = start + (ang - start).rem_euclid(std::f64::consts::TAU);
                ang <= end + 1e-12
            }
        })
        .collect()
}

/// Where the infinite lines (`o1`, `d1`) and (`o2`, `d2`) cross, if they are
/// not parallel and actually meet (closest approach within a relative
/// tolerance of their separation).
fn line_crossing(o1: DVec3, d1: DVec3, o2: DVec3, d2: DVec3) -> Option<DVec3> {
    let n = d1.cross(d2);
    let n2 = n.length_squared();
    if n2 < 1e-12 {
        return None;
    }
    let w = o2 - o1;
    let t = w.cross(d2).dot(n) / n2;
    let s = w.cross(d1).dot(n) / n2;
    let p1 = o1 + d1 * t;
    let p2 = o2 + d2 * s;
    let tol = 1e-9 * (1.0 + w.length());
    (p1.distance(p2) <= tol && t.abs() > 1e-9 && s.abs() > 1e-9).then_some(p1)
}

/// The unit direction that continues the geometry ending at `p`, used for
/// Tangent to Edge. Defined only when exactly one edge ends
/// at `p`: for an edge of a true arc it is the arc's tangent at that end
/// (pointing away from the arc); for a plain edge it is the edge's own
/// direction. `None` otherwise.
pub fn continuation(entities: &Entities, p: DVec3) -> Option<DVec3> {
    let (pos, edge, v) = single_edge_at(entities, p)?;
    if let Some(t) = arc_end_tangent(entities, edge, pos) {
        return Some(t);
    }
    let other = entities.vertex(edge.other(v))?.position();
    (pos - other).try_normalize()
}

/// Like [`continuation`], but only for the end of a true arc: a straight
/// continuation of a line is already offered by the axis and parallel lines.
pub fn arc_continuation(entities: &Entities, p: DVec3) -> Option<DVec3> {
    let (pos, edge, _) = single_edge_at(entities, p)?;
    arc_end_tangent(entities, edge, pos)
}

/// The vertex at `p`, if exactly one edge ends there: (position, edge, vertex).
fn single_edge_at(
    entities: &Entities,
    p: DVec3,
) -> Option<(DVec3, &smith_model::Edge, smith_model::VertexId)> {
    let v = entities.find_vertex(p)?;
    let vertex = entities.vertex(v)?;
    let [e] = vertex.edges() else {
        return None;
    };
    Some((vertex.position(), entities.edge(*e)?, v))
}

/// The outward tangent of an arc-curve edge at `pos`, if `pos` is the arc's
/// start or end. The arc runs counter-clockwise about its normal, so its
/// direction at angle θ is −sin θ · x + cos θ · y.
fn arc_end_tangent(entities: &Entities, edge: &smith_model::Edge, pos: DVec3) -> Option<DVec3> {
    use smith_geom::curve::Curve;
    use smith_geom::tolerance::coincident;
    let rec = entities.curve(edge.curve()?)?;
    let Curve::Arc(arc) = rec.curve() else {
        return None;
    };
    let f = arc.frame();
    let dir = |a: f64| f.y_axis() * a.cos() - f.x_axis() * a.sin();
    if coincident(pos, arc.end_point()) {
        Some(dir(arc.end_angle()))
    } else if coincident(pos, arc.start_point()) {
        Some(-dir(arc.start_angle()))
    } else {
        None
    }
}

/// Area centroid of a face's outer loop minus holes, from its triangles.
pub fn face_centroid(entities: &Entities, f: smith_model::FaceId) -> Option<DVec3> {
    let mut area = 0.0;
    let mut sum = DVec3::ZERO;
    for [a, b, c] in entities.face_triangles(f) {
        let w = (b - a).cross(c - a).length() * 0.5;
        area += w;
        sum += (a + b + c) / 3.0 * w;
    }
    (area > 0.0).then(|| sum / area)
}

fn point_segment_px(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 == 0.0 {
        0.0
    } else {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    };
    p.distance(a + ab * t)
}

/// Point on the line (origin, dir) closest to the ray (ro, rd).
/// Where the endless lines `o1 + t·d1` and `o2 + s·d2` meet (closest points
/// within 1e-9 of each other, scaled by the distance between the origins),
/// or `None` for parallel or skew lines.
fn lines_meet(o1: DVec3, d1: DVec3, o2: DVec3, d2: DVec3) -> Option<DVec3> {
    let n = d1.cross(d2);
    let n2 = n.length_squared();
    if n2 < 1e-12 {
        return None;
    }
    let w = o2 - o1;
    let t = w.cross(d2).dot(n) / n2;
    let s = w.cross(d1).dot(n) / n2;
    let p1 = o1 + d1 * t;
    let p2 = o2 + d2 * s;
    let tol = 1e-9 * (1.0 + w.length());
    (p1.distance(p2) <= tol).then_some(p1)
}

/// Where the endless line `o + t·d` crosses segment `a`–`b` (ends
/// included), or `None`.
fn line_segment_meet(o: DVec3, d: DVec3, a: DVec3, b: DVec3) -> Option<DVec3> {
    let v = b - a;
    let len = v.length();
    if len < 1e-12 {
        return None;
    }
    let p = lines_meet(o, d, a, v / len)?;
    let u = (p - a).dot(v) / (len * len);
    (-1e-9..=1.0 + 1e-9).contains(&u).then_some(p)
}

fn closest_on_line_to_ray(origin: DVec3, dir: DVec3, ro: DVec3, rd: DVec3) -> DVec3 {
    let w = origin - ro;
    let b = dir.dot(rd);
    let d = dir.dot(w);
    let e = rd.dot(w);
    let denom = 1.0 - b * b;
    if denom.abs() < 1e-12 {
        return origin;
    }
    let s = (b * e - d) / denom;
    origin + dir * s
}

fn ray_plane(o: DVec3, d: DVec3, plane: &Plane) -> Option<DVec3> {
    let denom = plane.normal().dot(d);
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = -plane.signed_distance(o) / denom;
    (t > 0.0).then(|| o + d * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_geom::curve::{Circle, Curve, CurveFrame};

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    /// Top-down view, 10 px per mm, y up on screen mapped to −y pixels.
    fn top(p: DVec3) -> Option<DVec2> {
        Some(DVec2::new(p.x * 10.0, -p.y * 10.0))
    }

    struct Scene {
        m: Entities,
        bvh: FaceBvh,
    }

    fn scene() -> Scene {
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
        let frame = CurveFrame::new(v(30.0, 30.0, 0.0), DVec3::X, DVec3::Z).unwrap();
        m.add_curve(Curve::Circle(Circle::new(frame, 5.0).unwrap()), 24)
            .unwrap();
        let bvh = FaceBvh::build(&m);
        Scene { m, bvh }
    }

    fn at<'a>(
        s: &'a Scene,
        x: f64,
        y: f64,
        last: Option<DVec3>,
        filter: &'a SnapFilter,
        axes: &'a DrawingAxes,
    ) -> Query<'a> {
        Query {
            entities: &s.m,
            bvh: &s.bvh,
            origin: v(x, y, 100.0),
            dir: -DVec3::Z,
            cursor: DVec2::new(x * 10.0, -y * 10.0),
            to_screen: &top,
            last,
            axes,
            reference_edge: None,
            tracked: &[],
            tangent: None,
            lock: Lock::None,
            filter,
            only: None,
            fallback: DVec3::ZERO,
            box_centres: &[],
            guides: true,
        }
    }

    #[test]
    fn guide_lines_points_and_their_crossings_snap() {
        let mut s = scene();
        // A guide line 2.5 mm above the square's top edge, a vertical guide
        // through x = 5, and a guide point.
        s.m.add_guide(Guide::line(v(0.0, 12.5, 0.0), DVec3::X).unwrap())
            .unwrap();
        s.m.add_guide(Guide::line(v(5.0, 0.0, 0.0), DVec3::Y).unwrap())
            .unwrap();
        s.m.add_guide(Guide::point(v(20.0, 5.0, 0.0)).unwrap())
            .unwrap();
        s.bvh = FaceBvh::build(&s.m);
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());

        let r = infer(&at(&s, 30.0, 12.6, None, &f, &a));
        assert_eq!(r.kind, Kind::OnGuide);
        assert_eq!(r.point.y, 12.5);
        assert!(matches!(r.entity, Some(Pickable::Guide(_))));

        let r = infer(&at(&s, 20.2, 5.1, None, &f, &a));
        assert_eq!(r.kind, Kind::GuidePoint);
        assert_eq!(r.point, v(20.0, 5.0, 0.0));

        // The two guide lines cross at (5, 12.5); the vertical guide crosses
        // the top edge at (5, 10) — the edge's midpoint ranks first there.
        let r = infer(&at(&s, 5.2, 12.4, None, &f, &a));
        assert_eq!(r.kind, Kind::Intersection);
        assert_eq!(r.point, v(5.0, 12.5, 0.0));

        // Hidden guides, or guides switched off in the filter, are not offered.
        let mut q = at(&s, 30.0, 12.6, None, &f, &a);
        q.guides = false;
        assert_ne!(infer(&q).kind, Kind::OnGuide);
        let off = SnapFilter {
            guides: false,
            ..SnapFilter::default()
        };
        assert_ne!(
            infer(&at(&s, 30.0, 12.6, None, &off, &a)).kind,
            Kind::OnGuide
        );
    }

    #[test]
    fn shift_on_a_guide_locks_to_it() {
        let mut m = Entities::new();
        let g = m
            .add_guide(Guide::line(v(0.0, 2.0, 0.0), DVec3::X).unwrap())
            .unwrap();
        let inf = Inference {
            point: v(4.0, 2.0, 0.0),
            kind: Kind::OnGuide,
            from: None,
            from2: None,
            entity: Some(Pickable::Guide(g)),
            locked: false,
            normal: None,
        };
        assert_eq!(
            lock_for(&inf, None, &m, None),
            Lock::Line {
                origin: v(0.0, 2.0, 0.0),
                dir: DVec3::X,
                kind: Kind::OnGuide
            }
        );
    }

    #[test]
    fn endpoint_beats_on_edge_and_midpoint_is_found() {
        let s = scene();
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());
        let r = infer(&at(&s, 10.3, 0.2, None, &f, &a));
        assert_eq!(r.kind, Kind::Endpoint);
        assert_eq!(r.point, v(10.0, 0.0, 0.0));
        let r = infer(&at(&s, 5.2, 0.1, None, &f, &a));
        assert_eq!(r.kind, Kind::Midpoint);
        let r = infer(&at(&s, 3.0, 0.3, None, &f, &a));
        assert_eq!(r.kind, Kind::OnEdge);
        assert!((r.point - v(3.0, 0.0, 0.0)).length() < 1e-9);
    }

    #[test]
    fn a_box_centre_is_offered_even_below_the_surface() {
        let s = scene();
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());
        // A centre 3 mm below the plane, under the cursor.
        let owner = Pickable::Face(s.m.face_ids().next().expect("face"));
        let centres = [(v(3.0, 3.0, -3.0), owner)];
        let mut q = at(&s, 3.0, 3.0, None, &f, &a);
        q.box_centres = &centres;
        let r = infer(&q);
        assert_eq!(r.kind, Kind::BoxCentre);
        assert_eq!(r.entity, Some(owner));
    }

    #[test]
    fn centre_without_hovering_the_circle() {
        // Acceptance: the centre is offered with the cursor near it.
        let s = scene();
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());
        let r = infer(&at(&s, 30.4, 29.7, None, &f, &a));
        assert_eq!(r.kind, Kind::Centre);
        assert!((r.point - v(30.0, 30.0, 0.0)).length() < 1e-12);
        // The 0° quadrant coincides with a circle vertex, and a segment midpoint is
        // close by; both rank higher. With those filtered out the quadrant is offered.
        let no_ends = SnapFilter {
            endpoint: false,
            midpoint: false,
            ..SnapFilter::default()
        };
        let r = infer(&at(&s, 35.2, 30.1, None, &no_ends, &a));
        assert_eq!(r.kind, Kind::Quadrant);
        assert!((r.point - v(35.0, 30.0, 0.0)).length() < 1e-12);
    }

    #[test]
    fn crossing_edges_give_an_intersection_but_corners_do_not() {
        let mut m = Entities::new();
        m.add_line(v(0.0, 0.0, 0.0), v(10.0, 10.0, 0.0)).unwrap();
        m.add_line(v(0.0, 10.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
        let s = Scene {
            bvh: FaceBvh::build(&m),
            m,
        };
        // The crossing vertex is a real vertex after sticky splitting: Endpoint.
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());
        assert_eq!(infer(&at(&s, 5.1, 5.0, None, &f, &a)).kind, Kind::Endpoint);
    }

    #[test]
    fn snap_only_restricts_to_centres() {
        let s = scene();
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());
        let mut q = at(&s, 10.2, 0.2, None, &f, &a);
        q.only = Some(OnlyKind::Centre);
        assert_ne!(infer(&q).kind, Kind::Endpoint);
    }

    #[test]
    fn axis_inference_and_lock() {
        let s = scene();
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());
        let last = Some(v(-20.0, -20.0, 0.0));
        let r = infer(&at(&s, -5.0, -19.6, last, &f, &a));
        assert_eq!(r.kind, Kind::OnAxis(Axis::Red));
        assert!((r.point.y + 20.0).abs() < 1e-9);
        // Locked to red, a cursor far off the axis still lands on it.
        let mut q = at(&s, -5.0, 0.0, last, &f, &a);
        q.lock = axis_lock(Axis::Red, v(-20.0, -20.0, 0.0), &a);
        let r = infer(&q);
        assert!(r.locked);
        assert!((r.point - v(-5.0, -20.0, 0.0)).length() < 1e-9);
    }

    #[test]
    fn on_face_and_free_space() {
        let s = scene();
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());
        let r = infer(&at(&s, 3.0, 6.0, None, &f, &a));
        assert_eq!(r.kind, Kind::OnFace);
        let r = infer(&at(&s, -40.0, -40.0, None, &f, &a));
        assert_eq!(r.kind, Kind::Free);
        assert!(
            (r.point - v(-40.0, -40.0, 0.0)).length() < 1e-9,
            "ground plane"
        );
    }

    #[test]
    fn filter_turns_snaps_off() {
        let s = scene();
        let a = DrawingAxes::default();
        let f = SnapFilter {
            endpoint: false,
            ..SnapFilter::default()
        };
        assert_ne!(infer(&at(&s, 10.2, 0.2, None, &f, &a)).kind, Kind::Endpoint);
    }

    #[test]
    fn axes_from_points_and_face() {
        let ax =
            DrawingAxes::from_points(v(1.0, 1.0, 0.0), v(2.0, 2.0, 0.0), v(0.0, 2.0, 0.0)).unwrap();
        assert!((ax.x - v(1.0, 1.0, 0.0).normalize()).length() < 1e-12);
        assert!((ax.z - DVec3::Z).length() < 1e-12);
        let tilt = v(0.0, -0.5, 3f64.sqrt() / 2.0);
        let f = DrawingAxes::on_face(v(0.0, 0.0, 5.0), tilt).unwrap();
        assert!((f.z - tilt).length() < 1e-12);
        assert!(
            f.x.dot(f.z).abs() < 1e-12 && f.x.z.abs() < 1e-12,
            "red stays horizontal"
        );
    }
    #[test]
    fn perpendicular_foot_from_the_last_point() {
        let s = scene();
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());
        // From (15, 3) the perpendicular meets the square's right edge at (10, 3).
        let r = infer(&at(&s, 10.05, 3.1, Some(v(15.0, 3.0, 0.0)), &f, &a));
        assert_eq!(r.kind, Kind::PerpendicularPoint);
        assert!((r.point - v(10.0, 3.0, 0.0)).length() < 1e-12);
    }

    #[test]
    fn tangent_point_on_a_circle() {
        let s = scene();
        let a = DrawingAxes::default();
        // Circle centre (30, 30), r 5; from (30, 20) the tangent points lie at
        // ±60° from the downward direction. They coincide with circle vertices, so
        // endpoints and midpoints are switched off to see the tangent snap.
        let f = SnapFilter {
            endpoint: false,
            midpoint: false,
            ..SnapFilter::default()
        };
        let x = 30.0 + 5.0 * 60f64.to_radians().sin();
        let r = infer(&at(&s, x + 0.05, 27.45, Some(v(30.0, 20.0, 0.0)), &f, &a));
        assert_eq!(r.kind, Kind::TangentPoint);
        let t = r.point - v(30.0, 30.0, 0.0);
        assert!((t.length() - 5.0).abs() < 1e-12, "on the circle");
        assert!(
            t.dot(r.point - v(30.0, 20.0, 0.0)).abs() < 1e-9,
            "radius ⟂ tangent"
        );
    }

    #[test]
    fn axis_lines_from_two_points_cross() {
        let s = scene();
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());
        let tracked = [v(20.0, 0.0, 0.0)];
        let mut q = at(&s, 20.1, 19.9, Some(v(0.0, 20.0, 0.0)), &f, &a);
        q.tracked = &tracked;
        let r = infer(&q);
        assert_eq!(r.kind, Kind::Crossing);
        assert!((r.point - v(20.0, 20.0, 0.0)).length() < 1e-12);
        assert!(r.from.is_some() && r.from2.is_some());
    }

    #[test]
    fn tangent_direction_from_the_last_point() {
        let s = scene();
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());
        let mut q = at(&s, 45.0, 5.05, Some(v(40.0, 0.0, 0.0)), &f, &a);
        q.tangent = Some(v(1.0, 1.0, 0.0));
        let r = infer(&q);
        assert_eq!(r.kind, Kind::Tangent);
        assert!((r.point - v(45.025, 5.025, 0.0)).length() < 1e-9);
    }

    #[test]
    fn continuation_follows_arc_ends_and_plain_edges() {
        use smith_geom::curve::Arc;
        let mut m = Entities::new();
        let frame = CurveFrame::new(v(50.0, 0.0, 0.0), DVec3::X, DVec3::Z).unwrap();
        let arc = Arc::new(frame, 5.0, 0.0, std::f64::consts::FRAC_PI_2).unwrap();
        m.add_curve(Curve::Arc(arc), 12).unwrap();
        m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
        // End of the arc at (50, 5): the arc is heading in −x.
        let t = continuation(&m, v(50.0, 5.0, 0.0)).unwrap();
        assert!((t - v(-1.0, 0.0, 0.0)).length() < 1e-12);
        // Start at (55, 0): continuing backwards means heading −y.
        let t = arc_continuation(&m, v(55.0, 0.0, 0.0)).unwrap();
        assert!((t - v(0.0, -1.0, 0.0)).length() < 1e-12);
        // A plain line continues straight; arc_continuation ignores it.
        let t = continuation(&m, v(10.0, 0.0, 0.0)).unwrap();
        assert!((t - v(1.0, 0.0, 0.0)).length() < 1e-12);
        assert!(arc_continuation(&m, v(10.0, 0.0, 0.0)).is_none());
        assert!(continuation(&m, v(5.0, 5.0, 0.0)).is_none());
    }

    #[test]
    #[ignore = "timing, run by hand: cargo test -p smith-infer --release -- --ignored --nocapture"]
    fn inference_on_a_large_model_is_quick() {
        // A 40 × 40 grid of separate squares: 6 400 edges.
        let mut m = Entities::new();
        for i in 0..40 {
            for j in 0..40 {
                let (x, y) = (f64::from(i) * 3.0, f64::from(j) * 3.0);
                m.add_polyline(
                    &[
                        v(x, y, 0.0),
                        v(x + 1.0, y, 0.0),
                        v(x + 1.0, y + 1.0, 0.0),
                        v(x, y + 1.0, 0.0),
                    ],
                    true,
                )
                .unwrap();
            }
        }
        let s = Scene {
            bvh: FaceBvh::build(&m),
            m,
        };
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());
        let start = std::time::Instant::now();
        for k in 0..20 {
            let q = at(&s, 1.0 + f64::from(k), 1.0, None, &f, &a);
            let _ = infer(&q);
        }
        let each = start.elapsed().as_secs_f64() * 1000.0 / 20.0;
        println!("inference on 6 400 edges: {each:.2} ms per frame");
    }

    #[test]
    fn far_edges_are_passed_over_but_near_ones_still_snap() {
        let s = scene();
        let (f, a) = (SnapFilter::default(), DrawingAxes::default());
        let q = at(&s, 10.0, 10.0, None, &f, &a);
        assert!(ray_reach(&q).is_some());
        let r = infer(&q);
        assert_eq!(r.kind, Kind::Endpoint);
    }
}
