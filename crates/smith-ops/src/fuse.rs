//! SketchSmith — Fuse | v1.0.0 | Thursday 12 November 2026
//!
//! [`fuse`] runs Fuse on a base and a target picked in one context: two
//! closed groups ([`Document::fuse_groups`]) or two faces on one plane
//! ([`smith_model::Entities::fuse_faces`]). A group result is checked
//! before it is kept: if it is not a clean solid the step is refused with
//! what is wrong, e.g. "Couldn't Fuse These Cleanly: 4 Open Edges", and
//! nothing changes.

use smith_model::{BooleanOp, DefinitionId, Document, ModelError, Pickable};

/// One Fuse.
#[derive(Debug, Clone, PartialEq)]
pub struct Fuse {
    /// What is kept and changed.
    pub base: Pickable,
    /// What it is combined with; it goes.
    pub target: Pickable,
    /// Union, Difference or Intersect.
    pub op: BooleanOp,
}

/// Refused: a group and a face, or anything else.
pub const PICK_TWO: &str = "Pick Two Groups or Two Faces";

/// Runs Fuse in context `ctx`.
///
/// # Errors
/// [`ModelError::Refused`] with the reason as shown on screen; otherwise as
/// the booleans.
pub fn fuse(doc: &mut Document, ctx: Option<DefinitionId>, f: &Fuse) -> Result<(), ModelError> {
    match (f.base, f.target) {
        (Pickable::Instance(b), Pickable::Instance(t)) => {
            doc.fuse_groups(ctx, b, t, f.op)?;
            let def = doc
                .context(ctx)
                .and_then(|e| e.instance(b))
                .map(|i| i.definition)
                .ok_or(ModelError::NoSuchEntity)?;
            let e = doc.context(Some(def)).ok_or(ModelError::NoSuchEntity)?;
            let mut wrong = Vec::new();
            let count = |n: usize, one: &str, many: &str| {
                format!("{n} {}", if n == 1 { one } else { many })
            };
            let open = e
                .edge_ids()
                .filter(|x| e.edge(*x).is_some_and(|edge| edge.faces().len() == 1))
                .count();
            if open > 0 {
                wrong.push(count(open, "Open Edge", "Open Edges"));
            }
            let shared = e
                .edge_ids()
                .filter(|x| e.edge(*x).is_some_and(|edge| edge.faces().len() >= 3))
                .count();
            if shared > 0 {
                wrong.push(count(shared, "Shared Face", "Shared Faces"));
            }
            let faces: Vec<_> = e.face_ids().collect();
            let turned = crate::solid::reversed_faces(e, &faces).len();
            if turned > 0 {
                wrong.push(count(
                    turned,
                    "Inverse Oriented Face",
                    "Inverse Oriented Faces",
                ));
            }
            if wrong.is_empty() {
                Ok(())
            } else {
                Err(ModelError::Refused(format!(
                    "Couldn't Fuse These Cleanly: {}",
                    wrong.join(", ")
                )))
            }
        }
        (Pickable::Face(b), Pickable::Face(t)) => doc
            .context_entities_mut(ctx)
            .ok_or(ModelError::NoSuchEntity)?
            .fuse_faces(b, t, f.op),
        _ => Err(ModelError::Refused(PICK_TWO.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_geom::{DMat4, DVec3};
    use smith_model::DefinitionKind;

    fn box_group(doc: &mut Document, at: DVec3) -> smith_model::InstanceId {
        let def = doc.create_definition("Box", DefinitionKind::Group);
        closed_box(doc.definition_entities_mut(def).unwrap());
        doc.add_instance(None, def, DMat4::from_translation(at), "")
            .unwrap()
    }

    /// A closed 10 mm box, faces outward.
    fn closed_box(m: &mut smith_model::Entities) {
        let c = |x: bool, y: bool, z: bool| {
            DVec3::new(
                if x { 10.0 } else { 0.0 },
                if y { 10.0 } else { 0.0 },
                if z { 10.0 } else { 0.0 },
            )
        };
        for q in [
            [
                c(false, false, false),
                c(true, false, false),
                c(true, true, false),
                c(false, true, false),
            ],
            [
                c(false, false, true),
                c(true, false, true),
                c(true, true, true),
                c(false, true, true),
            ],
            [
                c(false, false, false),
                c(true, false, false),
                c(true, false, true),
                c(false, false, true),
            ],
            [
                c(false, true, false),
                c(true, true, false),
                c(true, true, true),
                c(false, true, true),
            ],
            [
                c(false, false, false),
                c(false, true, false),
                c(false, true, true),
                c(false, false, true),
            ],
            [
                c(true, false, false),
                c(true, true, false),
                c(true, true, true),
                c(true, false, true),
            ],
        ] {
            m.add_polyline(&q, true).unwrap();
        }
        for f in m.face_ids().collect::<Vec<_>>() {
            let face = m.face(f).unwrap();
            let p = m.loop_points(face.outer())[0];
            if face.plane().normal().dot(p - DVec3::splat(5.0)) < 0.0 {
                m.reverse_face(f).unwrap();
            }
        }
    }

    #[test]
    fn groups_fuse_and_a_group_with_a_face_is_refused() {
        let mut doc = Document::new();
        let b = box_group(&mut doc, DVec3::ZERO);
        let t = box_group(&mut doc, DVec3::splat(5.0));
        let f = Fuse {
            base: Pickable::Instance(b),
            target: Pickable::Instance(t),
            op: BooleanOp::Difference,
        };
        fuse(&mut doc, None, &f).unwrap();
        assert_eq!(doc.root().instance_count(), 1);
        doc.root_mut()
            .add_polyline(
                &[
                    DVec3::new(50.0, 0.0, 0.0),
                    DVec3::new(51.0, 0.0, 0.0),
                    DVec3::new(51.0, 1.0, 0.0),
                ],
                true,
            )
            .unwrap();
        let face = doc.root().face_ids().next().unwrap();
        let mixed = Fuse {
            base: Pickable::Instance(b),
            target: Pickable::Face(face),
            op: BooleanOp::Union,
        };
        assert!(
            matches!(fuse(&mut doc, None, &mixed), Err(ModelError::Refused(ref m)) if m == PICK_TWO)
        );
    }
}
