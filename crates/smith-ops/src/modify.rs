//! SketchSmith — modify operations | v1.0.0 | Thursday 12 November 2026
//!
//! **Extrude** ([`extrude`]) moves a flat face along its normal and
//! builds the solid between, as SketchUp does:
//!
//! * **Stretch.** When every face beside the face stands square to it (the top
//!   of a box), the face itself moves and its neighbours stretch. Pulling
//!   lengthens the box; pushing shortens it.
//! * **Extrude.** Otherwise new side faces and a cap are built from the face's
//!   outline and holes. A free face (nothing else on its edges) stays as the
//!   bottom of the new solid; a face set into other geometry (a rectangle
//!   drawn on a box) is removed, so the new part opens into the old, and
//!   coplanar faces left either side of a former edge heal into one.
//! * **New start face** (the Ctrl option) always keeps the original face.
//! * **To zero.** Stretching a face exactly onto the far ends of its side
//!   edges flattens the solid to nothing, and it is removed: the face, its
//!   sides and the face opposite go. A free box vanishes; a block standing on
//!   a face, or a pocket pulled back flush, leaves that face whole again.
//! * **Cut through.** When the cap lands exactly in an opposite face (a
//!   circle on a 3.2 mm slab pushed 3.2 mm), the cap is removed and an
//!   opening is left.
//!
//! Every face of the result points out of the solid. Pulling, or pushing a
//! free face, adds material: side faces face away from the outline and the
//! cap faces the direction of travel. Pushing a face set into a solid back
//! into it carves a pocket: side faces face into the pocket and the cap faces
//! back out of it. A kept original face is turned to face away from the new
//! material.
//!
//! **Erase** ([`erase`]) erases edges with SketchUp's healing, or hides them.
//!
//! **Move** ([`move_items`]) moves the selected edges and faces. Only the faces
//! attached to them stretch, and a move that would bend any face out of flat
//! is refused with nothing changed (see `Entities::move_vertices`). With Copy
//! (the Ctrl option) the geometry is copied instead: `copies` copies one step
//! apart (`5x`), or, with `divide`, `copies` copies spread evenly up to the
//! moved position (`/4`). A curve copied whole stays a curve. A copy keeps
//! what its original carries: layer, hidden, edge marks, line styles and
//! paint (carried with it, as a move carries it).
//!
//! **Proportional editing** ([`move_proportional`]): connected points within
//! a radius of what moves also move, by an amount that falls off with
//! distance (smooth, linear or sharp), as in Blender. Only points joined to
//! the moved geometry by edges are reached; faces must still stay flat.
//!
//! Groups and components among the items move, turn and copy whole: only
//! their instance transforms change.
//!
//! **Rotate, Scale, Mirror** ([`transform_items`]) work the same way with a
//! [`Transform`]: faces moved whole turn with the geometry, faces only
//! partly moved must stay flat, and Copy with `Nx` or `/N` makes radial
//! arrays (Rotate) or graded copies (Scale). Circles and arcs stay curves
//! under rotation, mirroring and uniform scaling; a stretch makes them plain
//! edges.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use smith_geom::tolerance::TOL_MERGE;
use smith_geom::{DMat3, DMat4, DVec3};
use smith_model::{CurveId, EdgeId, Entities, FaceId, InstanceId, ModelError, Pickable, VertexId};

/// Pushes or pulls face `face` by `distance` mm along its normal (negative
/// pushes back through the face). `new_start` keeps the original face (Ctrl).
///
/// # Errors
/// [`ModelError::NoSuchEntity`]; [`ModelError::ZeroLength`] for a distance
/// within the merge tolerance; [`ModelError::NonFinite`]; errors from moving
/// or drawing (for example [`ModelError::NotFlat`]).
pub fn extrude(
    entities: &mut Entities,
    face: FaceId,
    distance: f64,
    new_start: bool,
) -> Result<(), ModelError> {
    if !distance.is_finite() {
        return Err(ModelError::NonFinite);
    }
    if distance.abs() <= TOL_MERGE {
        return Err(ModelError::ZeroLength);
    }
    let f = entities.face(face).ok_or(ModelError::NoSuchEntity)?;
    let n = f.plane().normal();
    let offset = n * distance;
    let loop_vertices: Vec<Vec<VertexId>> = f.loops().map(|l| entities.loop_vertices(l)).collect();
    let loop_edges: Vec<EdgeId> = f.loops().flatten().map(|u| u.edge).collect();

    if !new_start && stretches(entities, face, &loop_edges, n) {
        if let Some(far) = collapse_target(entities, &loop_vertices, offset) {
            return collapse(entities, face, &loop_edges, &far);
        }
        let vs: Vec<VertexId> = loop_vertices.iter().flatten().copied().collect();
        return entities.move_vertices(&vs, offset);
    }
    extrude_face(
        entities,
        face,
        &loop_vertices,
        &loop_edges,
        n,
        distance,
        new_start,
    )
}

/// Whether the face can simply move: every other face on its edges stands
/// square to it, and there is at least one.
fn stretches(entities: &Entities, face: FaceId, edges: &[EdgeId], n: DVec3) -> bool {
    let mut any = false;
    for e in edges {
        let Some(edge) = entities.edge(*e) else {
            return false;
        };
        for g in edge.faces() {
            if *g == face {
                continue;
            }
            let Some(other) = entities.face(*g) else {
                return false;
            };
            if other.plane().normal().dot(n).abs() > 1e-9 {
                return false;
            }
            any = true;
        }
    }
    any
}

/// What stretching a face by `offset` would flatten: when every point of the
/// face has a side edge running exactly `offset` to a point, and those far
/// points are joined loop by loop as the face's are, the side edges and the
/// far loops' edges.
struct Collapse {
    sides: Vec<EdgeId>,
    far_loops: Vec<Vec<VertexId>>,
    far_edges: Vec<EdgeId>,
}

fn collapse_target(
    entities: &Entities,
    loop_vertices: &[Vec<VertexId>],
    offset: DVec3,
) -> Option<Collapse> {
    let mut sides = Vec::new();
    let mut far_loops = Vec::new();
    let mut far_edges = Vec::new();
    for l in loop_vertices {
        let mut far = Vec::with_capacity(l.len());
        for v in l {
            let p = entities.vertex(*v)?.position();
            let w = entities.find_vertex(p + offset)?;
            sides.push(entities.edge_between(*v, w)?);
            far.push(w);
        }
        for i in 0..far.len() {
            far_edges.push(entities.edge_between(far[i], far[(i + 1) % far.len()])?);
        }
        far_loops.push(far);
    }
    Some(Collapse {
        sides,
        far_loops,
        far_edges,
    })
}

/// Removes the solid between `face` and its far loops (see [`collapse_target`]).
/// A face lying on the far outline is the solid's other end and goes too,
/// with any of its edges left bare; when the far outline is set into other
/// geometry, it is filled again so that surface is whole.
fn collapse(
    entities: &mut Entities,
    face: FaceId,
    loop_edges: &[EdgeId],
    far: &Collapse,
) -> Result<(), ModelError> {
    let far_points: Vec<DVec3> = far.far_loops[0]
        .iter()
        .filter_map(|v| entities.vertex(*v).map(|x| x.position()))
        .collect();
    let opposite = face_through(entities, &far_points).filter(|f| *f != face);

    let mut side_faces: BTreeSet<FaceId> = BTreeSet::new();
    for e in &far.sides {
        if let Some(edge) = entities.edge(*e) {
            side_faces.extend(edge.faces().iter().copied());
        }
    }
    if let Some(o) = opposite {
        side_faces.remove(&o);
    }
    side_faces.insert(face);
    for f in side_faces {
        if entities.face(f).is_some() {
            entities.erase_face(f)?;
        }
    }
    let bare = |e: &Entities, id: EdgeId| e.edge(id).is_some_and(|x| x.faces().is_empty());
    for e in far.sides.iter().chain(loop_edges) {
        if bare(entities, *e) {
            entities.erase_edge(*e)?;
        }
    }

    // The far end: a face there was the solid's other end.
    let shared = far.far_edges.iter().any(|e| {
        entities
            .edge(*e)
            .is_some_and(|x| x.faces().iter().any(|g| Some(*g) != opposite))
    });
    match opposite {
        Some(o) if !shared => {
            entities.erase_face(o)?;
            for e in &far.far_edges {
                if bare(entities, *e) {
                    entities.erase_edge(*e)?;
                }
            }
        }
        Some(_) => {}
        None => entities.fill_faces(&far.far_edges),
    }
    Ok(())
}

fn extrude_face(
    entities: &mut Entities,
    face: FaceId,
    loop_vertices: &[Vec<VertexId>],
    loop_edges: &[EdgeId],
    n: DVec3,
    distance: f64,
    new_start: bool,
) -> Result<(), ModelError> {
    let offset = n * distance;
    // A face nothing else touches, before anything is built.
    let free = loop_edges.iter().all(|e| {
        entities
            .edge(*e)
            .is_some_and(|edge| edge.faces().iter().all(|g| *g == face))
    });
    // Pushing a face set into a solid back into it removes material.
    let carve = !free && !new_start && distance < 0.0;
    let pos = |e: &Entities, v: VertexId| e.vertex(v).map(|x| x.position());
    let loops: Vec<Vec<DVec3>> = loop_vertices
        .iter()
        .map(|l| l.iter().filter_map(|v| pos(entities, *v)).collect())
        .collect();

    // Sides, then caps.
    for l in &loops {
        for p in l {
            entities.add_line(*p, *p + offset)?;
        }
    }
    for l in &loops {
        let top: Vec<DVec3> = l.iter().map(|p| *p + offset).collect();
        entities.add_polyline(&top, true)?;
    }

    // The caps of hole loops come out as discs inside the cap: remove them.
    let cap = face_on(entities, &loops[0], offset);
    for hole in loops.iter().skip(1) {
        if let Some(disc) = face_on(entities, hole, offset)
            && Some(disc) != cap
        {
            entities.erase_face(disc)?;
        }
    }

    // Face directions: sides away from the outline, the cap along the travel.
    for l in &loops {
        let k = l.len();
        for i in 0..k {
            let (a, b) = (l[i], l[(i + 1) % k]);
            let out = if carve {
                n.cross(b - a)
            } else {
                (b - a).cross(n)
            };
            if let Some(side) = face_through(entities, &[a, b, b + offset, a + offset]) {
                orient(entities, side, out)?;
            }
        }
    }
    if let Some(cap) = cap {
        let dir = if carve { n } else { n * distance.signum() };
        orient(entities, cap, dir)?;
    }

    // The original face.
    if new_start || free {
        orient(entities, face, -n * distance.signum())?;
    } else {
        entities.erase_face(face)?;
        heal_coplanar(entities, loop_edges)?;
    }

    // Cut through: a cap lying in an opposite face is an opening.
    if let Some(cap) = cap
        && cuts_through(entities, cap)
    {
        entities.erase_face(cap)?;
    }
    Ok(())
}

/// The face whose outer loop runs through exactly `points` moved by `offset`.
fn face_on(entities: &Entities, points: &[DVec3], offset: DVec3) -> Option<FaceId> {
    let moved: Vec<DVec3> = points.iter().map(|p| *p + offset).collect();
    face_through(entities, &moved)
}

/// The face whose outer loop has exactly the vertices at `points`.
pub(crate) fn face_through(entities: &Entities, points: &[DVec3]) -> Option<FaceId> {
    let want: HashSet<VertexId> = points
        .iter()
        .map(|p| entities.find_vertex(*p))
        .collect::<Option<_>>()?;
    let first = *want.iter().next()?;
    entities.faces_at(&[first]).into_iter().find(|f| {
        entities.face(*f).is_some_and(|face| {
            let have: HashSet<VertexId> =
                entities.loop_vertices(face.outer()).into_iter().collect();
            have == want
        })
    })
}

/// Turns `face` so its normal points along `dir`.
pub(crate) fn orient(entities: &mut Entities, face: FaceId, dir: DVec3) -> Result<(), ModelError> {
    let facing = entities
        .face(face)
        .map(|f| f.plane().normal().dot(dir))
        .ok_or(ModelError::NoSuchEntity)?;
    if facing < 0.0 {
        entities.reverse_face(face)?;
    }
    Ok(())
}

/// Erases each edge left between exactly two coplanar faces, so they heal.
fn heal_coplanar(entities: &mut Entities, edges: &[EdgeId]) -> Result<(), ModelError> {
    for e in edges {
        let Some(edge) = entities.edge(*e) else {
            continue;
        };
        if let [a, b] = edge.faces() {
            let (Some(fa), Some(fb)) = (entities.face(*a), entities.face(*b)) else {
                continue;
            };
            let (pa, pb) = (fa.plane(), fb.plane());
            let parallel = pa.normal().dot(pb.normal()).abs() > 1.0 - 1e-12;
            if parallel && pa.signed_distance(pb.origin()).abs() <= TOL_MERGE {
                entities.erase_edge(*e)?;
            }
        }
    }
    Ok(())
}

/// Whether every edge of `cap` also bounds another face in the cap's plane.
fn cuts_through(entities: &Entities, cap: FaceId) -> bool {
    let Some(f) = entities.face(cap) else {
        return false;
    };
    let plane = *f.plane();
    f.outer().iter().all(|u| {
        entities.edge(u.edge).is_some_and(|edge| {
            edge.faces().iter().any(|g| {
                *g != cap
                    && entities.face(*g).is_some_and(|other| {
                        other.plane().normal().dot(plane.normal()).abs() > 1.0 - 1e-12
                            && plane.signed_distance(other.plane().origin()).abs() <= TOL_MERGE
                    })
            })
        })
    })
}

/// Erases `edges` (healing coplanar faces), or hides them when `hide`.
///
/// # Errors
/// [`ModelError::NoSuchEntity`] if an edge is already gone.
pub fn erase(entities: &mut Entities, edges: &[EdgeId], hide: bool) -> Result<(), ModelError> {
    for e in edges {
        if hide {
            entities.set_edge_hidden(*e, true)?;
        } else if entities.edge(*e).is_some() {
            entities.erase_edge(*e)?;
        }
    }
    Ok(())
}

/// The edges of `items`: selected edges, and every loop edge of selected faces.
pub fn item_edges(entities: &Entities, items: &[Pickable]) -> Vec<EdgeId> {
    let mut out = BTreeSet::new();
    for item in items {
        match item {
            Pickable::Edge(e) => {
                if entities.edge(*e).is_some() {
                    out.insert(*e);
                }
            }
            Pickable::Face(f) => {
                if let Some(face) = entities.face(*f) {
                    out.extend(face.loops().flatten().map(|u| u.edge));
                }
            }
            Pickable::Vertex(_) | Pickable::Instance(_) | Pickable::Guide(_) => {}
        }
    }
    out.into_iter().collect()
}

/// The vertices of `items`: selected vertices, and the ends of the edges of
/// [`item_edges`].
pub fn item_vertices(entities: &Entities, items: &[Pickable]) -> Vec<VertexId> {
    let mut out: BTreeSet<VertexId> = item_edges(entities, items)
        .into_iter()
        .filter_map(|e| entities.edge(e))
        .flat_map(|e| e.vertices())
        .collect();
    out.extend(items.iter().filter_map(|p| match p {
        Pickable::Vertex(v) if entities.vertex(*v).is_some() => Some(*v),
        _ => None,
    }));
    out.into_iter().collect()
}

/// Refuses with [`ModelError::Locked`] when any group or component among
/// `items` is locked.
pub fn check_unlocked(entities: &Entities, items: &[Pickable]) -> Result<(), ModelError> {
    let locked = item_instances(entities, items)
        .into_iter()
        .any(|i| entities.instance(i).is_some_and(|x| x.locked));
    if locked {
        Err(ModelError::Locked)
    } else {
        Ok(())
    }
}

/// The groups and components among `items` that still exist.
pub fn item_instances(entities: &Entities, items: &[Pickable]) -> Vec<InstanceId> {
    items
        .iter()
        .filter_map(|p| match p {
            Pickable::Instance(i) if entities.instance(*i).is_some() => Some(*i),
            _ => None,
        })
        .collect()
}

/// The corners of each group's or component's box, by instance, in the
/// context's coordinates (see `Document::instance_boxes`).
pub type InstanceBoxes = BTreeMap<InstanceId, [DVec3; 8]>;

/// The edges of each group or component, by instance, as end points in the
/// context's coordinates, for previewing them while they move.
pub type InstanceLines = BTreeMap<InstanceId, Vec<[DVec3; 2]>>;

/// The points that bound `items`: their edges' ends and their groups' and
/// components' box corners.
fn item_points(entities: &Entities, items: &[Pickable], boxes: &InstanceBoxes) -> Vec<DVec3> {
    let mut pts: Vec<DVec3> = item_vertices(entities, items)
        .into_iter()
        .filter_map(|v| entities.vertex(v).map(|x| x.position()))
        .collect();
    for i in item_instances(entities, items) {
        if let Some(c) = boxes.get(&i) {
            pts.extend(c.iter().copied());
        }
    }
    pts
}

/// The bounding box of `items` (edge ends and group boxes).
pub fn items_bounds(
    entities: &Entities,
    items: &[Pickable],
    boxes: &InstanceBoxes,
) -> Option<(DVec3, DVec3)> {
    let pts = item_points(entities, items, boxes);
    let first = *pts.first()?;
    Some(
        pts.iter()
            .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))),
    )
}

/// The bounding box of `items` measured along the unit axes `axes` (columns):
/// the lowest and highest coordinate along each, in that frame.
pub fn items_bounds_in(
    entities: &Entities,
    items: &[Pickable],
    boxes: &InstanceBoxes,
    axes: DMat3,
) -> Option<(DVec3, DVec3)> {
    let to_local = axes.transpose();
    let pts: Vec<DVec3> = item_points(entities, items, boxes)
        .into_iter()
        .map(|p| to_local * p)
        .collect();
    let first = *pts.first()?;
    Some(
        pts.iter()
            .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))),
    )
}

/// Moves (or copies) `items` by `delta`.
///
/// # Errors
/// [`ModelError::NotFlat`] or [`ModelError::Collides`] for a move that would
/// bend faces or merge points (nothing changes); [`ModelError::ZeroLength`]
/// for a move within the merge tolerance; drawing errors for copies.
pub fn move_items(
    entities: &mut Entities,
    items: &[Pickable],
    delta: DVec3,
    copy: bool,
    copies: u32,
    divide: bool,
) -> Result<(), ModelError> {
    if !delta.is_finite() {
        return Err(ModelError::NonFinite);
    }
    if delta.length() <= TOL_MERGE {
        return Err(ModelError::ZeroLength);
    }
    let edges = item_edges(entities, items);
    let vertices = item_vertices(entities, items);
    let instances = item_instances(entities, items);
    if vertices.is_empty() && instances.is_empty() {
        return Err(ModelError::NoSuchEntity);
    }
    if !copy {
        check_unlocked(entities, items)?;
        for i in &instances {
            entities.transform_instance(*i, &DMat4::from_translation(delta))?;
        }
        if vertices.is_empty() {
            return Ok(());
        }
        return entities.move_vertices(&vertices, delta);
    }
    let n = copies.max(1);
    for k in 1..=n {
        let step = if divide {
            delta * (f64::from(k) / f64::from(n))
        } else {
            delta * f64::from(k)
        };
        let m = DMat4::from_translation(step);
        copy_edges(entities, &edges, &m)?;
        for i in &instances {
            entities.copy_instance(*i, &m)?;
        }
    }
    Ok(())
}

/// How proportional editing falls off with distance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Falloff {
    /// Eases in and out (Blender's Smooth).
    #[default]
    Smooth,
    /// Straight line.
    Linear,
    /// Drops quickly away from the moved points.
    Sharp,
}

impl Falloff {
    /// The share of the move at `t` = 1 − distance / radius (0..1).
    pub fn weight(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Falloff::Smooth => t * t * (3.0 - 2.0 * t),
            Falloff::Linear => t,
            Falloff::Sharp => t * t,
        }
    }
}

/// Proportional editing settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Proportional {
    /// Reach, mm.
    pub radius: f64,
    /// Falloff shape.
    pub falloff: Falloff,
}

/// Moves `items` by `delta`, and connected points within the radius by a
/// falling-off share of it.
///
/// # Errors
/// As [`move_items`].
pub fn move_proportional(
    entities: &mut Entities,
    items: &[Pickable],
    delta: DVec3,
    p: Proportional,
) -> Result<(), ModelError> {
    if !delta.is_finite() || !p.radius.is_finite() {
        return Err(ModelError::NonFinite);
    }
    let moved: BTreeSet<VertexId> = item_vertices(entities, items).into_iter().collect();
    if moved.is_empty() {
        return Err(ModelError::NoSuchEntity);
    }
    let sources: Vec<DVec3> = moved
        .iter()
        .filter_map(|v| entities.vertex(*v).map(|x| x.position()))
        .collect();
    let mut targets: Vec<(VertexId, DVec3)> = moved
        .iter()
        .filter_map(|v| entities.vertex(*v).map(|x| (*v, x.position() + delta)))
        .collect();
    // Walk outward through edges, taking points inside the radius.
    let mut seen: BTreeSet<VertexId> = moved.clone();
    let mut queue: Vec<VertexId> = moved.iter().copied().collect();
    while let Some(v) = queue.pop() {
        let Some(vertex) = entities.vertex(v) else {
            continue;
        };
        for e in vertex.edges() {
            let Some(edge) = entities.edge(*e) else {
                continue;
            };
            let w = edge.other(v);
            if !seen.insert(w) {
                continue;
            }
            let Some(pos) = entities.vertex(w).map(|x| x.position()) else {
                continue;
            };
            let d = sources
                .iter()
                .map(|s| s.distance(pos))
                .fold(f64::INFINITY, f64::min);
            if d < p.radius {
                let share = p.falloff.weight(1.0 - d / p.radius);
                targets.push((w, pos + delta * share));
                queue.push(w);
            }
        }
    }
    entities.reposition_vertices(&targets)
}

/// A rotation, scale or mirror.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Transform {
    /// Turn by `angle` radians about the axis through `center` along `axis`
    /// (counter-clockwise looking against the axis).
    Rotate {
        /// A point on the axis.
        center: DVec3,
        /// Unit axis direction.
        axis: DVec3,
        /// Angle, radians.
        angle: f64,
    },
    /// Scale about `center` by `factors` along the axes `axes`.
    Scale {
        /// The fixed point.
        center: DVec3,
        /// Factor along each axis.
        factors: DVec3,
        /// The axes scaled along, as unit columns (the drawing axes; the
        /// identity for the world axes).
        axes: DMat3,
    },
    /// Mirror in the plane through `point` with unit normal `normal`.
    Mirror {
        /// A point on the plane.
        point: DVec3,
        /// Unit plane normal.
        normal: DVec3,
    },
    /// Any combination of the above, as one matrix (Repeat Transform). A step
    /// applies it that many whole times.
    Matrix {
        /// The combined transform.
        m: DMat4,
    },
}

impl Transform {
    /// The transform taken `step` times (a fraction for `/N` arrays).
    pub fn matrix(&self, step: f64) -> DMat4 {
        match *self {
            Transform::Rotate {
                center,
                axis,
                angle,
            } => {
                DMat4::from_translation(center)
                    * DMat4::from_axis_angle(axis, angle * step)
                    * DMat4::from_translation(-center)
            }
            Transform::Scale {
                center,
                factors,
                axes,
            } => {
                let f = DVec3::new(
                    factors.x.powf(step),
                    factors.y.powf(step),
                    factors.z.powf(step),
                );
                DMat4::from_translation(center)
                    * DMat4::from_mat3(axes * DMat3::from_diagonal(f) * axes.transpose())
                    * DMat4::from_translation(-center)
            }
            Transform::Mirror { point, normal } => {
                let n = normal;
                // Householder reflection about the plane through `point`.
                let l = DMat3::IDENTITY - 2.0 * DMat3::from_cols(n * n.x, n * n.y, n * n.z);
                DMat4::from_translation(point)
                    * DMat4::from_mat3(l)
                    * DMat4::from_translation(-point)
            }
            Transform::Matrix { m } => {
                let times = step.round().max(1.0) as u32;
                (1..times).fold(m, |acc, _| m * acc)
            }
        }
    }

    fn check(&self) -> Result<(), ModelError> {
        match *self {
            Transform::Rotate { axis, angle, .. } => {
                if !axis.is_finite() || !angle.is_finite() || axis.length() < 0.5 {
                    return Err(ModelError::NonFinite);
                }
                if angle.abs() < 1e-12 {
                    return Err(ModelError::ZeroLength);
                }
            }
            Transform::Scale { factors, axes, .. } => {
                if !factors.is_finite() || !axes.is_finite() {
                    return Err(ModelError::NonFinite);
                }
                if factors.min_element() <= 1e-9 {
                    return Err(ModelError::ZeroLength);
                }
            }
            Transform::Mirror { normal, .. } => {
                if !normal.is_finite() || normal.length() < 0.5 {
                    return Err(ModelError::NonFinite);
                }
            }
            Transform::Matrix { m } => {
                if !m.is_finite() {
                    return Err(ModelError::NonFinite);
                }
                if DMat3::from_mat4(m).determinant().abs() < 1e-12 {
                    return Err(ModelError::ZeroLength);
                }
            }
        }
        Ok(())
    }
}

/// Rotates, scales or mirrors `items`, or copies them (`copies` copies, each
/// one more step; with `divide`, spread up to one full step). Mirror copies
/// once.
///
/// # Errors
/// As [`move_items`]; [`ModelError::ZeroLength`] for a zero angle or a zero
/// or negative scale factor.
pub fn transform_items(
    entities: &mut Entities,
    items: &[Pickable],
    transform: &Transform,
    copy: bool,
    copies: u32,
    divide: bool,
) -> Result<(), ModelError> {
    transform.check()?;
    let edges = item_edges(entities, items);
    let vertices = item_vertices(entities, items);
    let instances = item_instances(entities, items);
    if vertices.is_empty() && instances.is_empty() {
        return Err(ModelError::NoSuchEntity);
    }
    if !copy {
        check_unlocked(entities, items)?;
        for i in &instances {
            entities.transform_instance(*i, &transform.matrix(1.0))?;
        }
        if vertices.is_empty() {
            return Ok(());
        }
        return entities.transform_vertices(&vertices, &transform.matrix(1.0));
    }
    let n = if matches!(transform, Transform::Mirror { .. }) {
        1
    } else {
        copies.max(1)
    };
    for k in 1..=n {
        let step = if divide {
            f64::from(k) / f64::from(n)
        } else {
            f64::from(k)
        };
        let m = transform.matrix(step);
        copy_edges(entities, &edges, &m)?;
        for i in &instances {
            entities.copy_instance(*i, &m)?;
        }
    }
    Ok(())
}

/// Draws a copy of `edges` under `m`; curves copied whole stay curves when `m`
/// keeps their shape. Closed flat loops fill in as faces, as when drawn, and
/// the copy takes the originals' layer, flags, marks, styles and paint.
fn copy_edges(entities: &mut Entities, edges: &[EdgeId], m: &DMat4) -> Result<(), ModelError> {
    let look = entities.capture_look(edges);
    let chosen: HashSet<EdgeId> = edges.iter().copied().collect();
    let mut whole: Vec<CurveId> = Vec::new();
    let mut lines: Vec<(DVec3, DVec3)> = Vec::new();
    for e in edges {
        let Some(edge) = entities.edge(*e) else {
            continue;
        };
        if let Some(c) = edge.curve()
            && entities.curve(c).is_some_and(|r| {
                r.edges().iter().all(|x| chosen.contains(x)) && r.curve().transformed(m).is_some()
            })
        {
            if !whole.contains(&c) {
                whole.push(c);
            }
            continue;
        }
        if let Some((a, b)) = entities.edge_points(*e) {
            lines.push((m.transform_point3(a), m.transform_point3(b)));
        }
    }
    let curves: Vec<_> = whole
        .iter()
        .filter_map(|c| entities.curve(*c))
        .filter_map(|r| Some((r.curve().transformed(m)?, r.segments())))
        .collect();
    for (a, b) in lines {
        entities.add_line(a, b)?;
    }
    for (curve, segments) in curves {
        entities.add_curve(curve, segments)?;
    }
    entities.apply_look(&look, m);
    Ok(())
}

/// The volume a closed solid encloses, mm³: `None` unless every edge has
/// exactly two faces (and there are at least four). Nested groups are not
/// counted.
pub fn solid_volume(entities: &Entities) -> Option<f64> {
    if entities.face_count() < 4
        || !entities
            .edge_ids()
            .all(|x| entities.edge(x).is_some_and(|e| e.faces().len() == 2))
    {
        return None;
    }
    let v: f64 = entities
        .face_ids()
        .flat_map(|f| entities.face_triangles(f))
        .map(|[a, b, c]| a.dot(b.cross(c)) / 6.0)
        .sum();
    Some(v.abs())
}

#[cfg(test)]
mod tests {
    use smith_geom::DVec3;

    use super::*;

    fn rect(e: &mut Entities, x0: f64, y0: f64, x1: f64, y1: f64, z: f64) {
        e.add_polyline(
            &[
                DVec3::new(x0, y0, z),
                DVec3::new(x1, y0, z),
                DVec3::new(x1, y1, z),
                DVec3::new(x0, y1, z),
            ],
            true,
        )
        .unwrap();
    }

    fn only_face(e: &Entities) -> FaceId {
        e.face_ids().next().unwrap()
    }

    /// Every edge has exactly two faces, and the two use it in opposite
    /// directions: a closed solid with consistent faces.
    fn closed_and_consistent(e: &Entities) {
        for id in e.edge_ids() {
            let edge = e.edge(id).unwrap();
            assert_eq!(edge.faces().len(), 2, "edge {id:?} faces");
            let dirs: Vec<bool> = edge
                .faces()
                .iter()
                .map(|f| {
                    e.face(*f)
                        .unwrap()
                        .loops()
                        .flatten()
                        .find(|u| u.edge == id)
                        .unwrap()
                        .reversed
                })
                .collect();
            assert_ne!(dirs[0], dirs[1], "edge {id:?} used the same way twice");
        }
        e.validate().unwrap();
    }

    /// Volume by the divergence theorem; positive when faces point outward.
    fn volume(e: &Entities) -> f64 {
        e.face_ids()
            .flat_map(|f| e.face_triangles(f))
            .map(|[a, b, c]| a.dot(b.cross(c)) / 6.0)
            .sum()
    }

    #[test]
    fn a_lone_selected_vertex_moves_and_its_edges_stretch() {
        let mut e = Entities::new();
        e.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::X * 10.0,
                DVec3::new(10.0, 10.0, 0.0),
                DVec3::Y * 10.0,
            ],
            true,
        )
        .unwrap();
        let corner = e.find_vertex(DVec3::new(10.0, 10.0, 0.0)).unwrap();
        move_items(
            &mut e,
            &[Pickable::Vertex(corner)],
            DVec3::new(0.0, 5.0, 0.0),
            false,
            1,
            false,
        )
        .unwrap();
        assert_eq!(
            e.vertex(corner).unwrap().position(),
            DVec3::new(10.0, 15.0, 0.0)
        );
        assert_eq!(e.edge_count(), 4);
        e.validate().unwrap();
    }

    #[test]
    fn pulling_a_free_face_makes_a_closed_box() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 20.0, 30.0, 0.0);
        let f = only_face(&e);
        extrude(&mut e, f, 10.0, false).unwrap();
        assert_eq!(e.face_count(), 6);
        closed_and_consistent(&e);
        assert!((volume(&e) - 6000.0).abs() < 1e-6);
    }

    #[test]
    fn pushing_back_builds_behind_the_face() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 10.0, 10.0, 0.0);
        let f = only_face(&e);
        extrude(&mut e, f, -4.0, false).unwrap();
        closed_and_consistent(&e);
        assert!((volume(&e) - 400.0).abs() < 1e-6);
        let (lo, _) = e.bounds().unwrap();
        assert!((lo.z + 4.0).abs() < 1e-9);
    }

    #[test]
    fn pulling_the_top_of_a_box_stretches_it() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 10.0, 10.0, 0.0);
        let f0 = only_face(&e);
        extrude(&mut e, f0, 10.0, false).unwrap();
        let top = e
            .face_ids()
            .find(|f| e.face(*f).unwrap().plane().normal().z > 0.5)
            .unwrap();
        extrude(&mut e, top, 2.7, false).unwrap();
        assert_eq!(e.face_count(), 6);
        closed_and_consistent(&e);
        assert!((volume(&e) - 1270.0).abs() < 1e-6);
    }

    fn top(e: &Entities) -> FaceId {
        e.face_ids()
            .max_by(|a, b| {
                let z = |f: &FaceId| e.face(*f).unwrap().plane().origin().z;
                z(a).total_cmp(&z(b))
            })
            .unwrap()
    }

    #[test]
    fn pushing_a_free_box_to_zero_removes_it() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 10.0, 10.0, 0.0);
        let f0 = only_face(&e);
        extrude(&mut e, f0, 6.0, false).unwrap();
        let t = top(&e);
        extrude(&mut e, t, -6.0, false).unwrap();
        assert_eq!(e.face_count(), 0);
        assert_eq!(e.edge_count(), 0);
        e.validate().unwrap();
    }

    #[test]
    fn pushing_a_block_back_to_zero_restores_the_face_it_stood_on() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 40.0, 40.0, 0.0);
        rect(&mut e, 10.0, 10.0, 20.0, 20.0, 0.0);
        let small = e
            .face_ids()
            .find(|f| (e.face_area(*f).unwrap() - 100.0).abs() < 1e-9)
            .unwrap();
        extrude(&mut e, small, 5.0, false).unwrap();
        let t = top(&e);
        extrude(&mut e, t, -5.0, false).unwrap();
        e.validate().unwrap();
        assert_eq!(e.face_count(), 2);
        let total: f64 = e.face_ids().map(|f| e.face_area(f).unwrap()).sum();
        assert!((total - 1600.0).abs() < 1e-9);
        assert!(
            e.face_ids()
                .all(|f| e.face(f).unwrap().plane().normal().z > 0.5)
        );
        let (_, hi) = e.bounds().unwrap();
        assert!(hi.z.abs() < 1e-9);
    }

    #[test]
    fn pulling_a_pocket_back_flush_closes_it() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 30.0, 30.0, 0.0);
        let f0 = only_face(&e);
        extrude(&mut e, f0, 10.0, false).unwrap();
        rect(&mut e, 10.0, 10.0, 20.0, 20.0, 10.0);
        let inset = e
            .face_ids()
            .find(|f| (e.face_area(*f).unwrap() - 100.0).abs() < 1e-9)
            .unwrap();
        extrude(&mut e, inset, -4.0, false).unwrap();
        assert!((volume(&e) - 8600.0).abs() < 1e-6);
        let floor = e
            .face_ids()
            .find(|f| {
                let face = e.face(*f).unwrap();
                (face.plane().origin().z - 6.0).abs() < 1e-9 && face.plane().normal().z.abs() > 0.5
            })
            .unwrap();
        extrude(&mut e, floor, 4.0, false).unwrap();
        closed_and_consistent(&e);
        assert!((volume(&e) - 9000.0).abs() < 1e-6);
    }

    #[test]
    fn new_start_face_keeps_the_original() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 10.0, 10.0, 0.0);
        let f0 = only_face(&e);
        extrude(&mut e, f0, 10.0, false).unwrap();
        let top = e
            .face_ids()
            .find(|f| e.face(*f).unwrap().plane().normal().z > 0.5)
            .unwrap();
        extrude(&mut e, top, 5.0, true).unwrap();
        // The old top stays as a face between the two blocks.
        assert_eq!(e.face_count(), 11);
        e.validate().unwrap();
    }

    #[test]
    fn a_rectangle_on_a_box_pulls_out_as_a_block() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 30.0, 30.0, 0.0);
        let f0 = only_face(&e);
        extrude(&mut e, f0, 10.0, false).unwrap();
        rect(&mut e, 10.0, 10.0, 20.0, 20.0, 10.0);
        let small = e
            .face_ids()
            .find(|f| (e.face_area(*f).unwrap() - 100.0).abs() < 1e-9)
            .unwrap();
        extrude(&mut e, small, 5.0, false).unwrap();
        closed_and_consistent(&e);
        assert!((volume(&e) - (9000.0 + 500.0)).abs() < 1e-6);
    }

    #[test]
    fn pushing_through_a_slab_cuts_an_opening() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 30.0, 30.0, 0.0);
        let f0 = only_face(&e);
        extrude(&mut e, f0, 3.2, false).unwrap();
        rect(&mut e, 10.0, 10.0, 20.0, 20.0, 3.2);
        let small = e
            .face_ids()
            .find(|f| (e.face_area(*f).unwrap() - 100.0).abs() < 1e-9)
            .unwrap();
        extrude(&mut e, small, -3.2, false).unwrap();
        closed_and_consistent(&e);
        assert!((volume(&e) - (30.0 * 30.0 - 100.0) * 3.2).abs() < 1e-6);
    }

    #[test]
    fn pushing_partway_into_a_slab_makes_a_pocket() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 30.0, 30.0, 0.0);
        let f0 = only_face(&e);
        extrude(&mut e, f0, 5.0, false).unwrap();
        rect(&mut e, 10.0, 10.0, 20.0, 20.0, 5.0);
        let small = e
            .face_ids()
            .find(|f| (e.face_area(*f).unwrap() - 100.0).abs() < 1e-9)
            .unwrap();
        extrude(&mut e, small, -2.0, false).unwrap();
        closed_and_consistent(&e);
        assert!((volume(&e) - (4500.0 - 200.0)).abs() < 1e-6);
    }

    #[test]
    fn zero_distance_is_refused() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 10.0, 10.0, 0.0);
        let f0 = only_face(&e);
        assert_eq!(extrude(&mut e, f0, 0.0, false), Err(ModelError::ZeroLength));
    }

    fn boxed(e: &mut Entities) {
        rect(e, 0.0, 0.0, 10.0, 10.0, 0.0);
        let f0 = only_face(e);
        extrude(e, f0, 10.0, false).unwrap();
    }

    fn edge_at(e: &Entities, a: DVec3, b: DVec3) -> EdgeId {
        let (va, vb) = (e.find_vertex(a).unwrap(), e.find_vertex(b).unwrap());
        e.edge_between(va, vb).unwrap()
    }

    #[test]
    fn lifting_a_ridge_stretches_only_the_attached_faces() {
        let mut e = Entities::new();
        boxed(&mut e);
        let top = edge_at(&e, DVec3::new(0.0, 0.0, 10.0), DVec3::new(10.0, 0.0, 10.0));
        // Sliding it sideways would bend the end faces: refused, nothing moves.
        let before = e.clone();
        let err = move_items(
            &mut e,
            &[Pickable::Edge(top)],
            DVec3::X * 5.0,
            false,
            1,
            false,
        );
        assert!(matches!(err, Err(ModelError::NotFlat(ref fs)) if fs.len() == 2));
        assert_eq!(e, before);
        // Lifting it 5 mm tilts the top and stretches the front: a ridge.
        move_items(
            &mut e,
            &[Pickable::Edge(top)],
            DVec3::Z * 5.0,
            false,
            1,
            false,
        )
        .unwrap();
        assert_eq!(e.face_count(), 6);
        assert!((volume(&e) - 1250.0).abs() < 1e-6);
        closed_and_consistent(&e);
    }

    #[test]
    fn moving_a_side_out_keeps_faces_flat() {
        let mut e = Entities::new();
        boxed(&mut e);
        let side = e
            .face_ids()
            .find(|f| e.face(*f).unwrap().plane().normal().x > 0.5)
            .unwrap();
        move_items(
            &mut e,
            &[Pickable::Face(side)],
            DVec3::X * 2.5,
            false,
            1,
            false,
        )
        .unwrap();
        assert!((volume(&e) - 1250.0).abs() < 1e-6);
        closed_and_consistent(&e);
    }

    #[test]
    fn copies_make_an_external_array() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 2.0, 2.0, 0.0);
        let f0 = only_face(&e);
        move_items(
            &mut e,
            &[Pickable::Face(f0)],
            DVec3::X * 5.0,
            true,
            5,
            false,
        )
        .unwrap();
        assert_eq!(e.face_count(), 6);
        let (_, hi) = e.bounds().unwrap();
        assert!((hi.x - 27.0).abs() < 1e-9);
        e.validate().unwrap();
    }

    #[test]
    fn divide_spreads_copies_up_to_the_move() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 2.0, 2.0, 0.0);
        let f0 = only_face(&e);
        move_items(
            &mut e,
            &[Pickable::Face(f0)],
            DVec3::X * 20.0,
            true,
            4,
            true,
        )
        .unwrap();
        assert_eq!(e.face_count(), 5);
        for x in [5.0, 10.0, 15.0, 20.0] {
            assert!(
                e.find_vertex(DVec3::new(x, 0.0, 0.0)).is_some(),
                "copy at {x}"
            );
        }
    }

    #[test]
    fn a_copied_circle_stays_a_curve() {
        let mut e = Entities::new();
        let frame = smith_geom::curve::CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap();
        let circle =
            smith_geom::curve::Curve::Circle(smith_geom::curve::Circle::new(frame, 3.0).unwrap());
        e.add_curve(circle, 24).unwrap();
        let f0 = only_face(&e);
        move_items(
            &mut e,
            &[Pickable::Face(f0)],
            DVec3::X * 10.0,
            true,
            1,
            false,
        )
        .unwrap();
        assert_eq!(e.curve_count(), 2);
        assert_eq!(e.face_count(), 2);
    }

    #[test]
    fn rotating_a_box_keeps_it_closed() {
        let mut e = Entities::new();
        boxed(&mut e);
        let items: Vec<Pickable> = e.face_ids().map(Pickable::Face).collect();
        let t = Transform::Rotate {
            center: DVec3::ZERO,
            axis: DVec3::Z,
            angle: 0.5,
        };
        transform_items(&mut e, &items, &t, false, 1, false).unwrap();
        closed_and_consistent(&e);
        assert!((volume(&e) - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn mirroring_a_box_keeps_faces_outward() {
        let mut e = Entities::new();
        boxed(&mut e);
        let items: Vec<Pickable> = e.face_ids().map(Pickable::Face).collect();
        let t = Transform::Mirror {
            point: DVec3::new(-5.0, 0.0, 0.0),
            normal: DVec3::X,
        };
        transform_items(&mut e, &items, &t, false, 1, false).unwrap();
        closed_and_consistent(&e);
        assert!((volume(&e) - 1000.0).abs() < 1e-6, "outward after mirror");
        let (lo, hi) = e.bounds().unwrap();
        assert!((lo.x + 20.0).abs() < 1e-9 && (hi.x + 10.0).abs() < 1e-9);
    }

    #[test]
    fn scaling_a_box_about_its_centre() {
        let mut e = Entities::new();
        boxed(&mut e);
        let items: Vec<Pickable> = e.face_ids().map(Pickable::Face).collect();
        let t = Transform::Scale {
            center: DVec3::splat(5.0),
            factors: DVec3::new(2.0, 1.0, 1.0),
            axes: DMat3::IDENTITY,
        };
        transform_items(&mut e, &items, &t, false, 1, false).unwrap();
        closed_and_consistent(&e);
        assert!((volume(&e) - 2000.0).abs() < 1e-6);
        let (lo, hi) = e.bounds().unwrap();
        assert!((lo.x + 5.0).abs() < 1e-9 && (hi.x - 15.0).abs() < 1e-9);
    }

    #[test]
    fn a_locked_group_does_not_move_but_copies() {
        use smith_model::{DefinitionKind, Document};
        let mut doc = Document::new();
        let def = doc.create_definition("G", DefinitionKind::Group);
        let i = doc.add_instance(None, def, DMat4::IDENTITY, "").unwrap();
        doc.root_mut().set_instance_locked(i, true).unwrap();
        let items = [Pickable::Instance(i)];
        assert_eq!(
            move_items(doc.root_mut(), &items, DVec3::X, false, 1, false),
            Err(ModelError::Locked)
        );
        // A copy of a locked group is allowed (the original stays put).
        move_items(doc.root_mut(), &items, DVec3::X, true, 1, false).unwrap();
        assert_eq!(doc.root().instance_count(), 2);
    }

    #[test]
    fn scaling_along_turned_axes() {
        let mut e = Entities::new();
        // A 10 mm square turned 45 degrees about blue, scaled 2x along its own
        // red side.
        let r = DVec3::new(1.0, 1.0, 0.0).normalize();
        let g = DVec3::new(-1.0, 1.0, 0.0).normalize();
        e.add_polyline(
            &[DVec3::ZERO, r * 10.0, r * 10.0 + g * 10.0, g * 10.0],
            true,
        )
        .unwrap();
        let items: Vec<Pickable> = e.face_ids().map(Pickable::Face).collect();
        let axes = DMat3::from_cols(r, g, DVec3::Z);
        let (lo, hi) = items_bounds_in(&e, &items, &InstanceBoxes::new(), axes).unwrap();
        assert!((hi - lo - DVec3::new(10.0, 10.0, 0.0)).length() < 1e-9);
        let t = Transform::Scale {
            center: DVec3::ZERO,
            factors: DVec3::new(2.0, 1.0, 1.0),
            axes,
        };
        transform_items(&mut e, &items, &t, false, 1, false).unwrap();
        let f = only_face(&e);
        assert!((e.face_area(f).unwrap() - 200.0).abs() < 1e-9);
        assert!(e.find_vertex(r * 20.0 + g * 10.0).is_some());
    }

    #[test]
    fn a_30_degree_rotate_copy_11x_closes_the_circle() {
        let mut e = Entities::new();
        rect(&mut e, 10.0, -1.0, 12.0, 1.0, 0.0);
        let f0 = only_face(&e);
        let t = Transform::Rotate {
            center: DVec3::ZERO,
            axis: DVec3::Z,
            angle: 30f64.to_radians(),
        };
        transform_items(&mut e, &[Pickable::Face(f0)], &t, true, 11, false).unwrap();
        assert_eq!(e.face_count(), 12);
        e.validate().unwrap();
    }

    /// Flip Along: a mirror in place through the selection's centre.
    fn flip(e: &mut Entities, items: &[Pickable], normal: DVec3) -> Result<(), ModelError> {
        let (lo, hi) = items_bounds(e, items, &InstanceBoxes::new()).unwrap();
        let t = Transform::Mirror {
            point: (lo + hi) * 0.5,
            normal,
        };
        transform_items(e, items, &t, false, 1, false)
    }

    #[test]
    fn flip_along_checks() {
        // A free L-shaped face flipped red stays in place, mirrored, facing up.
        let mut e = Entities::new();
        e.add_polyline(
            &[
                DVec3::ZERO,
                DVec3::new(10.0, 0.0, 0.0),
                DVec3::new(10.0, 4.0, 0.0),
                DVec3::new(4.0, 4.0, 0.0),
                DVec3::new(4.0, 10.0, 0.0),
                DVec3::new(0.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        let f = only_face(&e);
        flip(&mut e, &[Pickable::Face(f)], DVec3::X).unwrap();
        assert!(
            e.face(f).unwrap().plane().normal().z > 0.5,
            "still faces up"
        );
        assert!(
            e.find_vertex(DVec3::new(6.0, 4.0, 0.0)).is_some(),
            "mirrored notch"
        );
        e.validate().unwrap();

        // A whole box flipped on every axis stays closed and outward.
        let mut e = Entities::new();
        boxed(&mut e);
        let items: Vec<Pickable> = e.face_ids().map(Pickable::Face).collect();
        for n in [DVec3::X, DVec3::Y, DVec3::Z] {
            flip(&mut e, &items, n).unwrap();
            closed_and_consistent(&e);
            assert!((volume(&e) - 1000.0).abs() < 1e-6);
        }

        // One face of a box flipped along an axis in its plane: its corners
        // swap places, so it lands where it was and the box is unchanged.
        let top = e
            .face_ids()
            .find(|f| e.face(*f).unwrap().plane().normal().z > 0.5)
            .unwrap();
        flip(&mut e, &[Pickable::Face(top)], DVec3::X).unwrap();
        closed_and_consistent(&e);
        assert!((volume(&e) - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn repeating_a_move_and_turn_steps_the_same_way() {
        let mut e = Entities::new();
        rect(&mut e, 10.0, 0.0, 11.0, 1.0, 0.0);
        let f0 = only_face(&e);
        let step = Transform::Rotate {
            center: DVec3::ZERO,
            axis: DVec3::Z,
            angle: 15f64.to_radians(),
        }
        .matrix(1.0)
            * DMat4::from_translation(DVec3::Z * 2.0);
        for k in 1..=4u32 {
            let t = Transform::Matrix { m: step };
            let m = t.matrix(f64::from(k));
            let once = Transform::Matrix { m };
            transform_items(&mut e, &[Pickable::Face(f0)], &once, true, 1, false).unwrap();
        }
        assert_eq!(e.face_count(), 5);
        let a = 60f64.to_radians();
        let corner = DVec3::new(10.0 * a.cos(), 10.0 * a.sin(), 8.0);
        assert!(
            e.find_vertex(corner).is_some(),
            "fourth step at 60° and 8 mm up"
        );
    }

    #[test]
    fn proportional_pull_on_a_cylinder_side_keeps_faces_flat() {
        let mut e = Entities::new();
        let frame = smith_geom::curve::CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap();
        let circle =
            smith_geom::curve::Curve::Circle(smith_geom::curve::Circle::new(frame, 10.0).unwrap());
        e.add_curve(circle, 24).unwrap();
        let f0 = only_face(&e);
        extrude(&mut e, f0, 10.0, false).unwrap();
        // The vertical side line at angle 0.
        let side = edge_at(&e, DVec3::new(10.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 10.0));
        let p = Proportional {
            radius: 6.0,
            falloff: Falloff::Smooth,
        };
        move_proportional(&mut e, &[Pickable::Edge(side)], DVec3::X * 3.0, p).unwrap();
        assert!(e.find_vertex(DVec3::new(13.0, 0.0, 0.0)).is_some());
        // The next side line out (2.61 mm away) moved, but by less.
        let a = 15f64.to_radians();
        let next = DVec3::new(10.0 * a.cos(), 10.0 * a.sin(), 0.0);
        assert!(e.find_vertex(next).is_none(), "neighbour moved");
        closed_and_consistent(&e);
    }

    #[test]
    fn erase_heals_and_hide_keeps() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 10.0, 10.0, 0.0);
        e.add_line(DVec3::new(5.0, 0.0, 0.0), DVec3::new(5.0, 10.0, 0.0))
            .unwrap();
        assert_eq!(e.face_count(), 2);
        let split = e
            .edge_ids()
            .find(|id| {
                let (a, b) = e.edge_points(*id).unwrap();
                (a.x - 5.0).abs() < 1e-9 && (b.x - 5.0).abs() < 1e-9
            })
            .unwrap();
        erase(&mut e, &[split], true).unwrap();
        assert_eq!(e.face_count(), 2);
        erase(&mut e, &[split], false).unwrap();
        assert_eq!(e.face_count(), 1);
        e.validate().unwrap();
    }

    #[test]
    fn a_closed_box_has_a_volume_and_an_open_one_does_not() {
        let mut e = Entities::new();
        rect(&mut e, 0.0, 0.0, 20.0, 30.0, 0.0);
        let f = only_face(&e);
        assert!(solid_volume(&e).is_none());
        extrude(&mut e, f, 10.0, false).unwrap();
        assert!((solid_volume(&e).unwrap() - 6000.0).abs() < 1e-6);
    }
}
