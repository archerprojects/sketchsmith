//! SketchSmith — the Paint tool | v1.0.0 | Thursday 12 November 2026
//!
//! Present-mode painting, apart from the UI so it can be tested.
//!
//! * **Picking** ([`deep_hit`]): the nearest face under the cursor in the
//!   context being edited, reaching into groups and components without
//!   opening them; also which side of it is seen.
//! * **Face** ([`paint_face`]): paints the side seen of the face clicked, with
//!   a flat frame anchored at its context's origin, turned and shifted by the
//!   Operation Inputs.
//! * **Wrap** ([`paint_wrap`]): paints every face of the selection, or of the
//!   connected piece or group clicked, as one, with one projection (Box,
//!   Cylinder or Planar) about one of the drawing axes, centred on what was
//!   painted. A group's faces are painted in its definition (every copy of a
//!   component changes), each context getting the projection carried into
//!   its own coordinates.
//! * The **None** material unpaints. **Alt-click** samples ([`sample`]).
//!
//! Each paint is one undo step.

use std::collections::BTreeMap;

use smith_geom::{DMat3, DMat4, DVec2, DVec3};
use smith_model::{
    Axis, DefinitionId, Document, FaceId, InstanceId, Mapping, MaterialId, ModelError, Paint,
    Pickable, Projection, Side, TextureFrame, Wrap,
};

/// How a click paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApplyTo {
    /// The face clicked, on the side seen.
    #[default]
    Face,
    /// The selection, or the piece or group clicked, as one.
    Wrap,
}

/// The Paint tool's Operation Inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintSettings {
    /// Face or Wrap.
    pub apply_to: ApplyTo,
    /// Wrap's projection.
    pub projection: Projection,
    /// Wrap's axis (not used by Box).
    pub axis: Axis,
    /// Offset along the texture, mm.
    pub offset: DVec2,
    /// Turn of the texture, degrees.
    pub rotation_deg: f64,
    /// The current material; `None` unpaints.
    pub material: Option<MaterialId>,
}

impl Default for PaintSettings {
    fn default() -> Self {
        Self {
            apply_to: ApplyTo::Face,
            projection: Projection::Box,
            axis: Axis::Z,
            offset: DVec2::ZERO,
            rotation_deg: 0.0,
            material: None,
        }
    }
}

/// A face under the cursor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// The context the face is in (`None`: the root).
    pub ctx: Option<DefinitionId>,
    /// The face.
    pub face: FaceId,
    /// That context's coordinates to the world.
    pub world_from_ctx: DMat4,
    /// The side seen.
    pub side: Side,
    /// The group or component of the context being edited it lies in.
    pub owner: Option<InstanceId>,
    /// Distance along the ray.
    pub t: f64,
}

/// Ray–triangle distance (Möller–Trumbore), if the ray meets it.
fn ray_triangle(o: DVec3, d: DVec3, t: &[DVec3; 3]) -> Option<f64> {
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    let dist = e2.dot(q) * inv;
    (dist > 0.0).then_some(dist)
}

/// The nearest visible face along the world ray `origin` + t·`dir` in
/// context `ctx` (placed in the world by `world_from_ctx`) and every group
/// and component inside it, however deep.
pub fn deep_hit(
    doc: &Document,
    ctx: Option<DefinitionId>,
    world_from_ctx: DMat4,
    origin: DVec3,
    dir: DVec3,
) -> Option<Hit> {
    fn walk(
        doc: &Document,
        ctx: Option<DefinitionId>,
        m: DMat4,
        owner: Option<InstanceId>,
        ray: (DVec3, DVec3),
        depth: u32,
        best: &mut Option<Hit>,
    ) {
        let Some(e) = doc.context(ctx) else { return };
        let (origin, dir) = ray;
        for f in e.face_ids() {
            let Some(face) = e.face(f) else { continue };
            if face.attributes.hidden || !doc.layer_visible(face.attributes.layer) {
                continue;
            }
            for t in e.face_triangles(f) {
                let w = t.map(|p| m.transform_point3(p));
                if let Some(dist) = ray_triangle(origin, dir, &w)
                    && best.is_none_or(|b| dist < b.t)
                {
                    let n = (DMat3::from_mat4(m).inverse().transpose() * face.plane().normal())
                        .normalize_or_zero();
                    *best = Some(Hit {
                        ctx,
                        face: f,
                        world_from_ctx: m,
                        side: if n.dot(dir) < 0.0 {
                            Side::Front
                        } else {
                            Side::Back
                        },
                        owner,
                        t: dist,
                    });
                }
            }
        }
        if depth >= 64 {
            return;
        }
        for id in e.instance_ids() {
            let Some(inst) = e.instance(id) else { continue };
            if inst.hidden || !doc.layer_visible(inst.layer) {
                continue;
            }
            let owner = owner.or(Some(id));
            walk(
                doc,
                Some(inst.definition),
                m * inst.transform,
                owner,
                ray,
                depth + 1,
                best,
            );
        }
    }
    let mut best = None;
    walk(doc, ctx, world_from_ctx, None, (origin, dir), 0, &mut best);
    best
}

/// The paint for `side` of a face whose front normal (in its context) is
/// `normal`, painted with `settings` in Face mode.
fn face_paint(material: MaterialId, normal: DVec3, side: Side, s: &PaintSettings) -> Paint {
    let n = if side == Side::Front { normal } else { -normal };
    Paint {
        material,
        mapping: Mapping::Face(TextureFrame::for_side(
            n,
            s.offset,
            s.rotation_deg.to_radians(),
        )),
    }
}

/// Face mode: paints the side seen of the face `hit`, as one undo step.
///
/// # Errors
/// As the model's paint calls.
pub fn paint_face(doc: &mut Document, hit: &Hit, s: &PaintSettings) -> Result<(), ModelError> {
    doc.transaction("Paint", |d| {
        let e = d
            .context_entities_mut(hit.ctx)
            .ok_or(ModelError::NoSuchEntity)?;
        let normal = e
            .face(hit.face)
            .ok_or(ModelError::NoSuchEntity)?
            .plane()
            .normal();
        let paint = s.material.map(|m| face_paint(m, normal, hit.side, s));
        e.set_face_paint(hit.face, hit.side, paint)
    })
}

/// Every face to paint for a Wrap, by context, with each context's
/// coordinates to the world.
pub type WrapTargets = BTreeMap<Option<DefinitionId>, (DMat4, Vec<FaceId>)>;

/// Adds every face of definition `def` placed by `m`, and of every group
/// and component inside it.
fn add_definition(doc: &Document, def: DefinitionId, m: DMat4, out: &mut WrapTargets, depth: u32) {
    let Some(e) = doc.context(Some(def)) else {
        return;
    };
    let entry = out.entry(Some(def)).or_insert((m, Vec::new()));
    entry.1.extend(e.face_ids());
    if depth >= 64 {
        return;
    }
    for id in e.instance_ids() {
        if let Some(inst) = e.instance(id) {
            add_definition(doc, inst.definition, m * inst.transform, out, depth + 1);
        }
    }
}

/// What a Wrap paints: the `selection` in context `ctx` (its faces, and the
/// whole of its groups and components), or with nothing selected, the group
/// or connected piece `hit` lies in.
pub fn wrap_targets(
    doc: &Document,
    ctx: Option<DefinitionId>,
    world_from_ctx: DMat4,
    selection: &[Pickable],
    hit: Option<&Hit>,
) -> WrapTargets {
    let mut out = WrapTargets::new();
    let Some(e) = doc.context(ctx) else {
        return out;
    };
    let add_items = |items: &[Pickable], out: &mut WrapTargets| {
        for p in items {
            match p {
                Pickable::Face(f) => {
                    out.entry(ctx)
                        .or_insert((world_from_ctx, Vec::new()))
                        .1
                        .push(*f);
                }
                Pickable::Instance(i) => {
                    if let Some(inst) = e.instance(*i) {
                        add_definition(
                            doc,
                            inst.definition,
                            world_from_ctx * inst.transform,
                            out,
                            0,
                        );
                    }
                }
                _ => {}
            }
        }
    };
    if !selection.is_empty() {
        add_items(selection, &mut out);
    } else if let Some(h) = hit {
        match h.owner {
            Some(i) => add_items(&[Pickable::Instance(i)], &mut out),
            None => add_items(&e.connected_set(Pickable::Face(h.face)), &mut out),
        }
    }
    for (_, faces) in out.values_mut() {
        faces.sort();
        faces.dedup();
    }
    out
}

/// Wrap: paints the fronts of all `targets` as one, with one projection
/// about drawing axis `s.axis` (`axes` are the X, Y, Z directions in the
/// world), as one undo step.
///
/// # Errors
/// [`ModelError::NoSuchEntity`] when there is nothing to paint; as the
/// model's paint calls.
pub fn paint_wrap(
    doc: &mut Document,
    targets: &WrapTargets,
    axes: [DVec3; 3],
    s: &PaintSettings,
) -> Result<(), ModelError> {
    // Every corner in the world: the centre is the middle of their box, the
    // radius their mean distance from the axis.
    let mut points: Vec<DVec3> = Vec::new();
    for (ctx, (m, faces)) in targets {
        let Some(e) = doc.context(*ctx) else { continue };
        for f in faces {
            if let Some(face) = e.face(*f) {
                for l in face.loops() {
                    for v in e.loop_vertices(l) {
                        if let Some(x) = e.vertex(v) {
                            points.push(m.transform_point3(x.position()));
                        }
                    }
                }
            }
        }
    }
    if points.is_empty() {
        return Err(ModelError::NoSuchEntity);
    }
    let lo = points.iter().fold(DVec3::splat(f64::MAX), |a, p| a.min(*p));
    let hi = points.iter().fold(DVec3::splat(f64::MIN), |a, p| a.max(*p));
    let centre = (lo + hi) * 0.5;
    let w = axes[s.axis.index()].normalize_or(DVec3::Z);
    let radius = points
        .iter()
        .map(|p| {
            let d = *p - centre;
            (d - w * d.dot(w)).length()
        })
        .sum::<f64>()
        / points.len() as f64;
    doc.transaction("Paint", |d| {
        for (ctx, (m, faces)) in targets {
            let e = d
                .context_entities_mut(*ctx)
                .ok_or(ModelError::NoSuchEntity)?;
            let paint = match s.material {
                None => None,
                Some(material) => {
                    let to_ctx = m.inverse();
                    let m3 = DMat3::from_mat4(to_ctx);
                    let mut wrap = Wrap::new(
                        s.projection,
                        s.axis,
                        axes.map(|a| (m3 * a).normalize_or(a)),
                        to_ctx.transform_point3(centre),
                        // Radius measured in this context's units.
                        radius * (m3 * w.any_orthonormal_vector()).length(),
                    );
                    wrap.offset = s.offset;
                    wrap.rotation = s.rotation_deg.to_radians();
                    Some(Paint {
                        material,
                        mapping: Mapping::Wrap(e.add_wrap(wrap)?),
                    })
                }
            };
            for f in faces {
                e.set_face_paint(*f, Side::Front, paint)?;
            }
        }
        Ok(())
    })
}

/// Alt-click: the material on the side `hit` sees, if painted.
pub fn sample(doc: &Document, hit: &Hit) -> Option<MaterialId> {
    doc.context(hit.ctx)?
        .face_paint(hit.face, hit.side)
        .map(|p| p.material)
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_model::{DefinitionKind, Entities, Material};

    fn square(e: &mut Entities, z: f64) {
        e.add_polyline(
            &[
                DVec3::new(0.0, 0.0, z),
                DVec3::new(50.0, 0.0, z),
                DVec3::new(50.0, 50.0, z),
                DVec3::new(0.0, 50.0, z),
            ],
            true,
        )
        .unwrap();
    }

    fn red(doc: &mut Document) -> MaterialId {
        doc.add_material(Material {
            name: "Red".into(),
            color: [1.0, 0.0, 0.0],
            ..Material::default()
        })
        .unwrap()
    }

    #[test]
    fn a_click_from_below_paints_the_back_and_alt_click_samples_it() {
        let mut doc = Document::new();
        let m = red(&mut doc);
        square(doc.root_mut(), 0.0);
        let hit = deep_hit(
            &doc,
            None,
            DMat4::IDENTITY,
            DVec3::new(10.0, 10.0, -100.0),
            DVec3::Z,
        )
        .expect("hit");
        assert_eq!(hit.side, Side::Back);
        let s = PaintSettings {
            material: Some(m),
            ..PaintSettings::default()
        };
        paint_face(&mut doc, &hit, &s).unwrap();
        assert_eq!(sample(&doc, &hit), Some(m));
        assert!(doc.root().face_paint(hit.face, Side::Front).is_none());
        // None unpaints, and each paint is one undo step.
        paint_face(&mut doc, &hit, &PaintSettings::default()).unwrap();
        assert_eq!(sample(&doc, &hit), None);
        doc.undo();
        assert_eq!(sample(&doc, &hit), Some(m));
    }

    #[test]
    fn a_click_reaches_into_a_group_and_wrap_paints_the_whole_group() {
        let mut doc = Document::new();
        let m = red(&mut doc);
        let def = doc.create_definition("Box", DefinitionKind::Group);
        square(doc.context_entities_mut(Some(def)).unwrap(), 0.0);
        square(doc.context_entities_mut(Some(def)).unwrap(), 10.0);
        let place = DMat4::from_translation(DVec3::new(100.0, 0.0, 0.0));
        doc.add_instance(None, def, place, "Box").unwrap();
        let hit = deep_hit(
            &doc,
            None,
            DMat4::IDENTITY,
            DVec3::new(110.0, 10.0, 100.0),
            -DVec3::Z,
        )
        .expect("hit");
        assert_eq!(hit.ctx, Some(def));
        assert_eq!(hit.side, Side::Front);
        assert!(hit.owner.is_some());
        let targets = wrap_targets(&doc, None, DMat4::IDENTITY, &[], Some(&hit));
        assert_eq!(targets[&Some(def)].1.len(), 2);
        let s = PaintSettings {
            apply_to: ApplyTo::Wrap,
            projection: Projection::Planar,
            material: Some(m),
            ..PaintSettings::default()
        };
        paint_wrap(&mut doc, &targets, [DVec3::X, DVec3::Y, DVec3::Z], &s).unwrap();
        let g = doc.context(Some(def)).unwrap();
        assert!(g.face_ids().all(|f| g.face_paint(f, Side::Front).is_some()));
        // One wrap, centred on the group in its own coordinates.
        assert_eq!(g.wrap_ids().count(), 1);
        let w = g.wrap(g.wrap_ids().next().unwrap()).unwrap();
        assert!(w.origin.distance(DVec3::new(25.0, 25.0, 5.0)) < 1e-9);
    }

    #[test]
    fn wrap_takes_the_connected_piece_clicked() {
        let mut doc = Document::new();
        let m = red(&mut doc);
        square(doc.root_mut(), 0.0);
        let e = doc.root_mut();
        e.add_polyline(
            &[
                DVec3::new(200.0, 0.0, 0.0),
                DVec3::new(210.0, 0.0, 0.0),
                DVec3::new(210.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        let hit = deep_hit(
            &doc,
            None,
            DMat4::IDENTITY,
            DVec3::new(10.0, 10.0, 100.0),
            -DVec3::Z,
        )
        .expect("hit");
        let targets = wrap_targets(&doc, None, DMat4::IDENTITY, &[], Some(&hit));
        assert_eq!(targets[&None].1, vec![hit.face]);
        let s = PaintSettings {
            apply_to: ApplyTo::Wrap,
            material: Some(m),
            ..PaintSettings::default()
        };
        paint_wrap(&mut doc, &targets, [DVec3::X, DVec3::Y, DVec3::Z], &s).unwrap();
        let painted = doc
            .root()
            .face_ids()
            .filter(|f| doc.root().face_paint(*f, Side::Front).is_some())
            .count();
        assert_eq!(painted, 1);
    }
}
