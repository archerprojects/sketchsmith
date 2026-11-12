//! SketchSmith — groups and components | v1.0.0 | Thursday 12 November 2026
//!
//! **Make Group / Make Component** ([`make_group`]) moves the chosen faces and
//! edges of a context into a new definition and places one instance of it,
//! where the geometry was. The definition keeps the model's coordinates (the
//! instance's transform is the identity). An edge that still bounds a face
//! left outside stays outside too, so both sides keep it, as in SketchUp;
//! inside the group nothing sticks to the outside, because a definition has
//! its own geometry.
//!
//! **Explode** ([`explode`]) puts an instance's geometry back into its
//! context, placed by the instance's transform, where it sticks to what is
//! there, and removes the instance. The geometry is copied across as it is
//! (holes, flags, marks, styles and paint kept, quick on large meshes):
//! points meeting points already there merge, so edges and faces that
//! meet join up; where it crosses other geometry without meeting at a
//! point, nothing is cut (Intersect Faces does that). The definition is removed when no other
//! instance uses it. Instances inside the definition are placed in the
//! context with their transforms combined.
//!
//! **Copy and Paste** ([`copy_items`], [`paste_clip`]) lift a selection into
//! a [`Clip`] — its loose geometry as plain data, and its groups and
//! components as placements of their definitions (a pasted component is
//! another instance of the same component) — and draw it back into any
//! context through a transform, where it sticks as drawing would.
//!
//! **Reset Scale** ([`reset_scale`]) removes any scaling and mirroring from
//! an instance, keeping its turn and its insertion point.
//!
//! **Change Axes** ([`change_axes`]) moves a definition's origin and axes to
//! a new frame without moving its geometry in the model: the geometry is
//! re-expressed in the new frame and every instance of it is adjusted to
//! match.
//!
//! Faces keep their direction both ways. Moving geometry only redraws its
//! lines and curves, so a loop that had no face in the source has none in
//! the copy, and a face the copy would close by itself is removed again.

use std::collections::HashSet;

use smith_geom::curve::Curve;
use smith_geom::{DMat3, DMat4, DVec3};
use smith_model::{
    DefinitionId, DefinitionKind, Document, EdgeId, Entities, FaceId, InstanceId, ModelError,
    Pickable, VertexId,
};

/// A whole curve lifted out: the curve, its segment count, and its edges'
/// ends in case a transform cannot carry the curve (a stretch).
type CurvePiece = (Curve, u32, Vec<(DVec3, DVec3)>);

/// Geometry lifted out of a context, as plain data.
#[derive(Debug, Default)]
struct Piece {
    /// Straight edges: ends and the hidden flag.
    lines: Vec<(DVec3, DVec3, bool)>,
    /// Whole curves.
    curves: Vec<CurvePiece>,
    /// Each face's outline points and normal.
    faces: Vec<(Vec<DVec3>, DVec3)>,
}

/// Lifts `faces` and `edges` (and every edge of those faces) out of `src`.
fn take_piece(src: &Entities, faces: &[FaceId], edges: &[EdgeId]) -> (Piece, Vec<EdgeId>) {
    let mut all: Vec<EdgeId> = Vec::new();
    let mut seen = HashSet::new();
    let face_edges = faces
        .iter()
        .filter_map(|f| src.face(*f))
        .flat_map(|f| f.loops().flatten().map(|u| u.edge).collect::<Vec<_>>());
    for e in edges.iter().copied().chain(face_edges) {
        if src.edge(e).is_some() && seen.insert(e) {
            all.push(e);
        }
    }

    let mut piece = Piece::default();
    let mut whole = Vec::new();
    for e in &all {
        let Some(edge) = src.edge(*e) else {
            continue;
        };
        if let Some(c) = edge.curve()
            && src
                .curve(c)
                .is_some_and(|r| r.edges().iter().all(|x| seen.contains(x)))
        {
            if !whole.contains(&c) {
                whole.push(c);
            }
            continue;
        }
        if let Some((a, b)) = src.edge_points(*e) {
            piece.lines.push((a, b, edge.flags.hidden));
        }
    }
    for c in whole {
        if let Some(r) = src.curve(c) {
            let segments = r
                .edges()
                .iter()
                .filter_map(|e| src.edge_points(*e))
                .collect();
            piece
                .curves
                .push((r.curve().clone(), r.segments(), segments));
        }
    }
    for f in faces {
        if let Some(face) = src.face(*f) {
            piece
                .faces
                .push((src.loop_points(face.outer()), face.plane().normal()));
        }
    }
    (piece, all)
}

/// Draws `piece` into `dst` through `m`, keeping face directions, and
/// removing any face the drawing closed that the piece did not have.
fn paste(dst: &mut Entities, piece: &Piece, m: &DMat4) -> Result<(), ModelError> {
    let before: HashSet<FaceId> = dst.face_ids().collect();
    let at = |p: DVec3| m.transform_point3(p);
    for (a, b, _) in &piece.lines {
        dst.add_line(at(*a), at(*b))?;
    }
    for (curve, segments, edges) in &piece.curves {
        match curve.transformed(m) {
            Some(c) => {
                dst.add_curve(c, *segments)?;
            }
            None => {
                for (a, b) in edges {
                    dst.add_line(at(*a), at(*b))?;
                }
            }
        }
    }

    // Face directions follow the transform (normals by the inverse transpose).
    let normals = DMat3::from_mat4(*m).inverse().transpose();
    let mut kept = HashSet::new();
    for (pts, n) in &piece.faces {
        let moved: Vec<DVec3> = pts.iter().map(|p| at(*p)).collect();
        if let Some(f) = face_through(dst, &moved) {
            let dir = normals * *n;
            if dst
                .face(f)
                .is_some_and(|x| x.plane().normal().dot(dir) < 0.0)
            {
                dst.reverse_face(f)?;
            }
            kept.insert(f);
        }
    }

    // Faces closed by the drawing alone, made only of pasted points.
    let pasted: HashSet<VertexId> = piece
        .lines
        .iter()
        .map(|(a, b, _)| (*a, *b))
        .chain(piece.curves.iter().flat_map(|c| c.2.iter().copied()))
        .flat_map(|(a, b)| [at(a), at(b)])
        .filter_map(|p| dst.find_vertex(p))
        .collect();
    let extra: Vec<FaceId> = dst
        .face_ids()
        .filter(|f| !before.contains(f) && !kept.contains(f))
        .filter(|f| {
            dst.face(*f).is_some_and(|face| {
                dst.loop_vertices(face.outer())
                    .iter()
                    .all(|v| pasted.contains(v))
            })
        })
        .collect();
    for f in extra {
        dst.erase_face(f)?;
    }

    for (a, b, hidden) in &piece.lines {
        if *hidden
            && let (Some(va), Some(vb)) = (dst.find_vertex(at(*a)), dst.find_vertex(at(*b)))
            && let Some(e) = dst.edge_between(va, vb)
        {
            dst.set_edge_hidden(e, true)?;
        }
    }
    Ok(())
}

/// The face whose outer loop has exactly the vertices at `points`.
fn face_through(entities: &Entities, points: &[DVec3]) -> Option<FaceId> {
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

/// A copied selection (see the module documentation).
#[derive(Debug)]
pub struct Clip {
    piece: Piece,
    instances: Vec<smith_model::Instance>,
}

/// Copies `items` of context `ctx`; `None` when there is nothing to copy.
pub fn copy_items(doc: &Document, ctx: Option<DefinitionId>, items: &[Pickable]) -> Option<Clip> {
    let src = doc.context(ctx)?;
    let faces: Vec<FaceId> = items
        .iter()
        .filter_map(|p| match p {
            Pickable::Face(f) => Some(*f),
            _ => None,
        })
        .collect();
    let edges: Vec<EdgeId> = items
        .iter()
        .filter_map(|p| match p {
            Pickable::Edge(e) => Some(*e),
            _ => None,
        })
        .collect();
    let instances: Vec<smith_model::Instance> = items
        .iter()
        .filter_map(|p| match p {
            Pickable::Instance(i) => src.instance(*i).cloned(),
            _ => None,
        })
        .map(|mut i| {
            i.locked = false;
            i
        })
        .collect();
    let (piece, taken) = take_piece(src, &faces, &edges);
    (!taken.is_empty() || !instances.is_empty()).then_some(Clip { piece, instances })
}

/// Draws `clip` into context `ctx` through `m` and returns what it made:
/// the edges and faces whose points are all pasted ones, and the new
/// groups and components. A group or component that would contain itself
/// is left out.
///
/// # Errors
/// [`ModelError::NoSuchEntity`] for a missing context; drawing errors.
pub fn paste_clip(
    doc: &mut Document,
    ctx: Option<DefinitionId>,
    clip: &Clip,
    m: &DMat4,
) -> Result<Vec<Pickable>, ModelError> {
    let dst = doc
        .context_entities_mut(ctx)
        .ok_or(ModelError::NoSuchEntity)?;
    paste(dst, &clip.piece, m)?;
    let at: HashSet<VertexId> = clip
        .piece
        .lines
        .iter()
        .map(|(a, b, _)| (*a, *b))
        .chain(clip.piece.curves.iter().flat_map(|c| c.2.iter().copied()))
        .flat_map(|(a, b)| [m.transform_point3(a), m.transform_point3(b)])
        .filter_map(|p| dst.find_vertex(p))
        .collect();
    let mut made: Vec<Pickable> = dst
        .edge_ids()
        .filter(|e| {
            dst.edge(*e)
                .is_some_and(|x| x.vertices().iter().all(|v| at.contains(v)))
        })
        .map(Pickable::Edge)
        .collect();
    made.extend(
        dst.face_ids()
            .filter(|f| {
                dst.face(*f)
                    .is_some_and(|x| dst.loop_vertices(x.outer()).iter().all(|v| at.contains(v)))
            })
            .map(Pickable::Face),
    );
    for inst in &clip.instances {
        if doc.definition(inst.definition).is_none() {
            continue;
        }
        match doc.add_instance(ctx, inst.definition, *m * inst.transform, inst.name.clone()) {
            Ok(id) => {
                let dst = doc
                    .context_entities_mut(ctx)
                    .ok_or(ModelError::NoSuchEntity)?;
                dst.set_layer(&[Pickable::Instance(id)], inst.layer)?;
                made.push(Pickable::Instance(id));
            }
            Err(ModelError::RecursiveInstance) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(made)
}

/// Makes a group (or a component, by `kind`) named `name` from `items` in
/// context `ctx` (`None` = the model root) and returns its instance.
///
/// # Errors
/// [`ModelError::NoSuchEntity`] for a missing context or nothing to group;
/// drawing errors.
pub fn make_group(
    doc: &mut Document,
    ctx: Option<DefinitionId>,
    items: &[Pickable],
    kind: DefinitionKind,
    name: &str,
) -> Result<InstanceId, ModelError> {
    let faces: Vec<FaceId> = items
        .iter()
        .filter_map(|p| match p {
            Pickable::Face(f) => Some(*f),
            _ => None,
        })
        .collect();
    let edges: Vec<EdgeId> = items
        .iter()
        .filter_map(|p| match p {
            Pickable::Edge(e) => Some(*e),
            _ => None,
        })
        .collect();
    let src = doc.context(ctx).ok_or(ModelError::NoSuchEntity)?;
    let (piece, taken) = take_piece(src, &faces, &edges);
    if taken.is_empty() {
        return Err(ModelError::NoSuchEntity);
    }

    let def = doc.create_definition(name, kind);
    let inside = doc
        .context_entities_mut(Some(def))
        .ok_or(ModelError::NoSuchEntity)?;
    paste(inside, &piece, &DMat4::IDENTITY)?;

    // Remove the source: the faces, then edges no face outside still uses.
    let src = doc
        .context_entities_mut(ctx)
        .ok_or(ModelError::NoSuchEntity)?;
    for f in &faces {
        if src.face(*f).is_some() {
            src.erase_face(*f)?;
        }
    }
    for e in &taken {
        if src.edge(*e).is_some_and(|x| x.faces().is_empty()) {
            src.erase_edge(*e)?;
        }
    }
    doc.add_instance(ctx, def, DMat4::IDENTITY, "")
}

/// Explodes instance `instance` of context `ctx` (`None` = the model root):
/// its geometry joins the context and the instance goes.
///
/// # Errors
/// [`ModelError::NoSuchEntity`]; drawing errors.
pub fn explode(
    doc: &mut Document,
    ctx: Option<DefinitionId>,
    instance: InstanceId,
) -> Result<(), ModelError> {
    let inst = doc
        .context(ctx)
        .and_then(|c| c.instance(instance))
        .cloned()
        .ok_or(ModelError::NoSuchEntity)?;
    let inside = doc
        .context(Some(inst.definition))
        .ok_or(ModelError::NoSuchEntity)?;
    let nested: Vec<_> = inside
        .instance_ids()
        .filter_map(|i| inside.instance(i).cloned())
        .collect();
    // Copied across as it is; points meeting points here merge.
    let inside = inside.clone();
    let dst = doc
        .context_entities_mut(ctx)
        .ok_or(ModelError::NoSuchEntity)?;
    dst.place_copy(&inside, &inst.transform);
    for n in nested {
        doc.add_instance(ctx, n.definition, inst.transform * n.transform, n.name)?;
    }
    doc.remove_instance(ctx, instance)?;
    if doc.instance_users(inst.definition) == 0 {
        doc.remove_definition(inst.definition)?;
    }
    Ok(())
}

/// Removes scaling and mirroring from instance `id` of context `ctx`,
/// keeping its rotation and insertion point.
///
/// # Errors
/// [`ModelError::NoSuchEntity`].
pub fn reset_scale(
    doc: &mut Document,
    ctx: Option<DefinitionId>,
    id: InstanceId,
) -> Result<(), ModelError> {
    let t = doc
        .context(ctx)
        .and_then(|c| c.instance(id))
        .map(|i| i.transform)
        .ok_or(ModelError::NoSuchEntity)?;
    // The rotation nearest the linear part: orthonormalise its columns.
    let l = DMat3::from_mat4(t);
    let x = l.x_axis.normalize_or(DVec3::X);
    let y = (l.y_axis - x * x.dot(l.y_axis)).normalize_or(x.any_orthonormal_vector());
    let z = x.cross(y);
    let r = DMat4::from_mat3(DMat3::from_cols(x, y, z));
    doc.set_instance_transform(ctx, id, DMat4::from_translation(t.w_axis.truncate()) * r)
}

/// Moves the origin and axes of instance `id`'s definition to `frame`
/// (given in the context's coordinates: columns red, green, blue and the
/// origin), leaving its geometry where it is in the model.
///
/// # Errors
/// [`ModelError::NoSuchEntity`]; [`ModelError::NonFinite`] for a frame that
/// cannot be inverted; errors from moving the geometry.
pub fn change_axes(
    doc: &mut Document,
    ctx: Option<DefinitionId>,
    id: InstanceId,
    frame: DMat4,
) -> Result<(), ModelError> {
    let inst = doc
        .context(ctx)
        .and_then(|c| c.instance(id))
        .cloned()
        .ok_or(ModelError::NoSuchEntity)?;
    if !frame.is_finite() || DMat3::from_mat4(frame).determinant().abs() < 1e-12 {
        return Err(ModelError::NonFinite);
    }
    // The new frame in the definition's own coordinates.
    let f = inst.transform.inverse() * frame;
    let into = f.inverse();
    let inside = doc
        .context_entities_mut(Some(inst.definition))
        .ok_or(ModelError::NoSuchEntity)?;
    let vertices: Vec<VertexId> = inside.vertex_ids().collect();
    if !vertices.is_empty() {
        inside.transform_vertices(&vertices, &into)?;
    }
    let nested: Vec<InstanceId> = inside.instance_ids().collect();
    for n in nested {
        inside.transform_instance(n, &into)?;
    }
    doc.retransform_users(inst.definition, &f);
    Ok(())
}

#[cfg(test)]
mod tests {
    use smith_geom::curve::{Circle, CurveFrame};

    use super::*;
    use crate::modify::extrude;

    fn rect(e: &mut Entities, x0: f64, y0: f64, x1: f64, y1: f64) {
        e.add_polyline(
            &[
                DVec3::new(x0, y0, 0.0),
                DVec3::new(x1, y0, 0.0),
                DVec3::new(x1, y1, 0.0),
                DVec3::new(x0, y1, 0.0),
            ],
            true,
        )
        .unwrap();
    }

    fn all_items(e: &Entities) -> Vec<Pickable> {
        e.face_ids()
            .map(Pickable::Face)
            .chain(e.edge_ids().map(Pickable::Edge))
            .collect()
    }

    /// A 10 × 10 × 10 mm box in the root.
    fn boxed(doc: &mut Document) {
        rect(doc.root_mut(), 0.0, 0.0, 10.0, 10.0);
        let f = doc.root().face_ids().next().unwrap();
        extrude(doc.root_mut(), f, 10.0, false).unwrap();
    }

    fn signed_volume(e: &Entities) -> f64 {
        e.face_ids()
            .flat_map(|f| e.face_triangles(f))
            .map(|[a, b, c]| a.dot(b.cross(c)) / 6.0)
            .sum()
    }

    #[test]
    fn grouping_a_box_moves_it_into_a_definition() {
        let mut doc = Document::new();
        boxed(&mut doc);
        let items = all_items(doc.root());
        let i = make_group(&mut doc, None, &items, DefinitionKind::Group, "").unwrap();
        assert_eq!(doc.root().face_count(), 0);
        assert_eq!(doc.root().edge_count(), 0);
        let def = doc.root().instance(i).unwrap().definition;
        let inside = doc.context(Some(def)).unwrap();
        assert_eq!(inside.face_count(), 6);
        assert_eq!(inside.edge_count(), 12);
        assert!((signed_volume(inside) - 1000.0).abs() < 1e-9);
        inside.validate().unwrap();
    }

    #[test]
    fn a_line_drawn_across_a_group_does_not_split_it() {
        let mut doc = Document::new();
        rect(doc.root_mut(), 0.0, 0.0, 10.0, 10.0);
        let items = all_items(doc.root());
        let i = make_group(&mut doc, None, &items, DefinitionKind::Group, "").unwrap();
        doc.root_mut()
            .add_line(DVec3::new(5.0, -5.0, 0.0), DVec3::new(5.0, 15.0, 0.0))
            .unwrap();
        let def = doc.root().instance(i).unwrap().definition;
        assert_eq!(doc.context(Some(def)).unwrap().edge_count(), 4);
        assert_eq!(doc.root().edge_count(), 1);
    }

    #[test]
    fn grouping_a_face_keeps_edges_shared_with_the_outside() {
        let mut doc = Document::new();
        rect(doc.root_mut(), 0.0, 0.0, 10.0, 10.0);
        rect(doc.root_mut(), 10.0, 0.0, 20.0, 10.0);
        let r = doc.root();
        let left = r
            .face_ids()
            .find(|f| {
                let outer = r.face(*f).unwrap().outer();
                r.loop_points(outer).iter().all(|p| p.x <= 10.0 + 1e-9)
            })
            .unwrap();
        let mut items = vec![Pickable::Face(left)];
        let f = doc.root().face(left).unwrap();
        items.extend(f.outer().iter().map(|u| Pickable::Edge(u.edge)));
        make_group(&mut doc, None, &items, DefinitionKind::Group, "").unwrap();
        // The right square keeps all four of its edges, the shared one too.
        assert_eq!(doc.root().face_count(), 1);
        assert_eq!(doc.root().edge_count(), 4);
        doc.root().validate().unwrap();
    }

    #[test]
    fn exploding_puts_the_geometry_back_where_the_instance_is() {
        let mut doc = Document::new();
        boxed(&mut doc);
        let items = all_items(doc.root());
        let i = make_group(&mut doc, None, &items, DefinitionKind::Group, "").unwrap();
        let def = doc.root().instance(i).unwrap().definition;
        // Move the instance 20 mm along red, then explode it.
        doc.remove_instance(None, i).unwrap();
        let i = doc
            .add_instance(None, def, DMat4::from_translation(DVec3::X * 20.0), "")
            .unwrap();
        explode(&mut doc, None, i).unwrap();
        assert_eq!(doc.root().instance_count(), 0);
        assert_eq!(doc.definition_ids().count(), 0);
        assert_eq!(doc.root().face_count(), 6);
        assert!((signed_volume(doc.root()) - 1000.0).abs() < 1e-9);
        let (lo, _) = doc.root().bounds().unwrap();
        assert!((lo.x - 20.0).abs() < 1e-9);
        doc.root().validate().unwrap();
    }

    #[test]
    fn a_mirrored_explode_keeps_faces_pointing_out() {
        let mut doc = Document::new();
        boxed(&mut doc);
        let items = all_items(doc.root());
        let i = make_group(&mut doc, None, &items, DefinitionKind::Group, "").unwrap();
        let def = doc.root().instance(i).unwrap().definition;
        doc.remove_instance(None, i).unwrap();
        let flip = DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0));
        let i = doc.add_instance(None, def, flip, "").unwrap();
        explode(&mut doc, None, i).unwrap();
        assert!((signed_volume(doc.root()) - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn explode_keeps_holes_and_edge_flags() {
        let mut doc = Document::new();
        let e = doc.root_mut();
        rect(e, 0.0, 0.0, 20.0, 20.0);
        rect(e, 5.0, 5.0, 10.0, 10.0);
        let edge = e.edge_ids().next().unwrap();
        e.set_edge_hidden(edge, true).unwrap();
        let holes = |e: &Entities| {
            e.face_ids()
                .filter_map(|f| e.face(f))
                .map(|f| f.inner().len())
                .sum::<usize>()
        };
        assert_eq!(holes(doc.root()), 1);
        let items = all_items(doc.root());
        let i = make_group(&mut doc, None, &items, DefinitionKind::Group, "").unwrap();
        let moved = DMat4::from_translation(DVec3::new(100.0, 0.0, 0.0));
        doc.set_instance_transform(None, i, moved).unwrap();
        explode(&mut doc, None, i).unwrap();
        let r = doc.root();
        assert_eq!(r.face_count(), 2);
        assert_eq!(holes(r), 1);
        assert_eq!(
            r.edge_ids()
                .filter(|x| r.edge(*x).is_some_and(|g| g.flags.hidden))
                .count(),
            1
        );
        assert!(
            r.vertex_ids()
                .all(|v| r.vertex(v).unwrap().position().x >= 100.0)
        );
    }

    #[test]
    fn a_circle_stays_a_curve_through_group_and_explode() {
        let mut doc = Document::new();
        let frame = CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap();
        doc.root_mut()
            .add_curve(Curve::Circle(Circle::new(frame, 5.0).unwrap()), 24)
            .unwrap();
        let items = all_items(doc.root());
        let i = make_group(&mut doc, None, &items, DefinitionKind::Component, "Disc").unwrap();
        let def = doc.root().instance(i).unwrap().definition;
        assert_eq!(doc.context(Some(def)).unwrap().curve_count(), 1);
        assert_eq!(doc.definition(def).unwrap().name, "Disc");
        explode(&mut doc, None, i).unwrap();
        assert_eq!(doc.root().curve_count(), 1);
        assert_eq!(doc.root().face_count(), 1);
    }

    #[test]
    fn a_group_moves_and_copies_whole() {
        use crate::modify::{InstanceBoxes, items_bounds, move_items};
        let mut doc = Document::new();
        boxed(&mut doc);
        let items = all_items(doc.root());
        let i = make_group(&mut doc, None, &items, DefinitionKind::Group, "").unwrap();
        let picked = [Pickable::Instance(i)];
        move_items(doc.root_mut(), &picked, DVec3::X * 5.0, false, 1, false).unwrap();
        move_items(doc.root_mut(), &picked, DVec3::Y * 20.0, true, 3, false).unwrap();
        assert_eq!(doc.root().instance_count(), 4);
        assert_eq!(doc.root().edge_count(), 0);
        let boxes: InstanceBoxes = doc.instance_boxes(None);
        let (lo, hi) = items_bounds(doc.root(), &picked, &boxes).unwrap();
        assert!((lo - DVec3::new(5.0, 0.0, 0.0)).length() < 1e-9);
        assert!((hi - DVec3::new(15.0, 10.0, 10.0)).length() < 1e-9);
        let (_, all_hi) = doc.model_bounds().unwrap();
        assert!((all_hi.y - 70.0).abs() < 1e-9);
    }

    #[test]
    fn a_unique_copy_no_longer_follows_the_original() {
        let mut doc = Document::new();
        boxed(&mut doc);
        let items = all_items(doc.root());
        let a = doc
            .transaction("Make Component", |d| {
                make_group(d, None, &items, DefinitionKind::Component, "Block")
            })
            .unwrap();
        let b = doc
            .transaction("Copy", |d| {
                d.root_mut()
                    .copy_instance(a, &DMat4::from_translation(DVec3::X * 20.0))
            })
            .unwrap();
        let def = doc.root().instance(a).unwrap().definition;
        assert_eq!(doc.instance_users(def), 2);
        let new = doc
            .transaction("Make Unique", |d| d.make_unique(None, b))
            .unwrap();
        assert_eq!(doc.definition(new).unwrap().name, "Block#2");
        assert_eq!(doc.instance_users(def), 1);
        // Editing the unique copy leaves the original alone.
        doc.transaction("Edit", |d| {
            d.context_entities_mut(Some(new))
                .unwrap()
                .add_line(DVec3::new(0.0, 0.0, 10.0), DVec3::new(10.0, 10.0, 10.0))
                .map(|_| ())
        })
        .unwrap();
        assert_eq!(doc.context(Some(def)).unwrap().face_count(), 6);
        assert_eq!(doc.context(Some(new)).unwrap().face_count(), 7);
        doc.undo();
        doc.undo();
        assert_eq!(doc.root().instance(b).unwrap().definition, def);
        assert!(doc.definition(new).is_none());
    }

    #[test]
    fn reset_scale_keeps_the_insertion_point() {
        let mut doc = Document::new();
        boxed(&mut doc);
        let items = all_items(doc.root());
        let i = make_group(&mut doc, None, &items, DefinitionKind::Component, "B").unwrap();
        let placed = DMat4::from_translation(DVec3::new(3.0, 4.0, 5.0))
            * DMat4::from_rotation_z(0.5)
            * DMat4::from_scale(DVec3::splat(2.0));
        doc.set_instance_transform(None, i, placed).unwrap();
        reset_scale(&mut doc, None, i).unwrap();
        let t = doc.root().instance(i).unwrap().transform;
        let want = DMat4::from_translation(DVec3::new(3.0, 4.0, 5.0)) * DMat4::from_rotation_z(0.5);
        assert!(t.abs_diff_eq(want, 1e-12), "{t:?}");
    }

    #[test]
    fn change_axes_moves_no_point_of_the_model() {
        let mut doc = Document::new();
        boxed(&mut doc);
        let items = all_items(doc.root());
        let i = make_group(&mut doc, None, &items, DefinitionKind::Component, "B").unwrap();
        let def = doc.root().instance(i).unwrap().definition;
        let copy = doc
            .root_mut()
            .copy_instance(i, &DMat4::from_translation(DVec3::Y * 30.0))
            .unwrap();
        let world = |doc: &Document, inst: InstanceId| -> Vec<DVec3> {
            let t = doc.root().instance(inst).unwrap().transform;
            let e = doc.context(Some(def)).unwrap();
            let mut v: Vec<DVec3> = e
                .vertex_ids()
                .map(|v| t.transform_point3(e.vertex(v).unwrap().position()))
                .collect();
            v.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).unwrap());
            v
        };
        let (before_a, before_b) = (world(&doc, i), world(&doc, copy));
        // Origin at the box's top corner, turned a quarter about blue.
        let frame = DMat4::from_translation(DVec3::new(10.0, 10.0, 10.0))
            * DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2);
        change_axes(&mut doc, None, i, frame).unwrap();
        for (a, b) in world(&doc, i).iter().zip(&before_a) {
            assert!(a.distance(*b) < 1e-9);
        }
        for (a, b) in world(&doc, copy).iter().zip(&before_b) {
            assert!(a.distance(*b) < 1e-9);
        }
        // The definition's origin is now that corner.
        let e = doc.context(Some(def)).unwrap();
        assert!(e.find_vertex(DVec3::ZERO).is_some());
        e.validate().unwrap();
    }

    #[test]
    fn copy_and_paste_geometry_and_groups() {
        let mut doc = Document::new();
        boxed(&mut doc);
        let items = all_items(doc.root());
        let g = make_group(&mut doc, None, &items, DefinitionKind::Component, "B").unwrap();
        rect(doc.root_mut(), 20.0, 0.0, 25.0, 5.0);
        let picked: Vec<Pickable> = all_items(doc.root())
            .into_iter()
            .chain([Pickable::Instance(g)])
            .collect();
        let clip = copy_items(&doc, None, &picked).unwrap();
        let made = paste_clip(
            &mut doc,
            None,
            &clip,
            &DMat4::from_translation(DVec3::Y * 50.0),
        )
        .unwrap();
        // One more square (4 edges, 1 face) and one more instance of B.
        assert_eq!(made.len(), 6);
        assert_eq!(doc.root().face_count(), 2);
        assert_eq!(doc.root().instance_count(), 2);
        let def = doc.root().instance(g).unwrap().definition;
        assert_eq!(doc.instance_users(def), 2);
        assert!(
            doc.root()
                .find_vertex(DVec3::new(25.0, 55.0, 0.0))
                .is_some()
        );
        doc.root().validate().unwrap();
    }

    #[test]
    fn half_model_symmetry_edits_both_halves() {
        use crate::modify::{Transform, transform_items};
        // The right half of a part as a component; a mirrored copy is the
        // left half, and drawing on one shows on the other.
        let mut doc = Document::new();
        boxed(&mut doc);
        let items = all_items(doc.root());
        let half = make_group(&mut doc, None, &items, DefinitionKind::Component, "Half").unwrap();
        let mirror = Transform::Mirror {
            point: DVec3::ZERO,
            normal: DVec3::X,
        };
        transform_items(
            doc.root_mut(),
            &[Pickable::Instance(half)],
            &mirror,
            true,
            1,
            false,
        )
        .unwrap();
        assert_eq!(doc.root().instance_count(), 2);
        let def = doc.root().instance(half).unwrap().definition;
        let flipped = doc.root().instance_ids().find(|i| *i != half).unwrap();
        let t = doc.root().instance(flipped).unwrap().transform;
        assert_eq!(doc.root().instance(flipped).unwrap().definition, def);
        assert!(DMat3::from_mat4(t).determinant() < 0.0);
        // Drawing inside the shared definition appears in both placements.
        doc.context_entities_mut(Some(def))
            .unwrap()
            .add_line(DVec3::new(2.0, 0.0, 10.0), DVec3::new(2.0, 10.0, 10.0))
            .unwrap();
        let world = |i: InstanceId, p: DVec3| {
            doc.root()
                .instance(i)
                .unwrap()
                .transform
                .transform_point3(p)
        };
        assert!((world(half, DVec3::new(2.0, 0.0, 10.0)).x - 2.0).abs() < 1e-12);
        assert!((world(flipped, DVec3::new(2.0, 0.0, 10.0)).x + 2.0).abs() < 1e-12);
        assert_eq!(doc.context(Some(def)).unwrap().face_count(), 7);
    }

    #[test]
    fn group_and_explode_are_one_undo_step_each() {
        let mut doc = Document::new();
        doc.transaction("Box", |d| {
            boxed(d);
            Ok(())
        })
        .unwrap();
        let items = all_items(doc.root());
        let i = doc
            .transaction("Make Group", |d| {
                make_group(d, None, &items, DefinitionKind::Group, "")
            })
            .unwrap();
        doc.transaction("Explode", |d| explode(d, None, i)).unwrap();
        doc.undo();
        assert_eq!(doc.root().instance_count(), 1);
        assert_eq!(doc.root().face_count(), 0);
        doc.undo();
        assert_eq!(doc.root().instance_count(), 0);
        assert_eq!(doc.root().face_count(), 6);
        assert_eq!(doc.definition_ids().count(), 0);
    }
}
