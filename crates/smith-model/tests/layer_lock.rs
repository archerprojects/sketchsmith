//! SketchSmith — layer lock and layer command tests | v1.0.0 | Thursday 12 November 2026
//!
//! Nothing on a locked layer changes: a line drawn across a locked face
//! (which would split it), a moved vertex it uses, hiding it, and moving a
//! group on a locked layer are each refused and leave the model and the
//! undo history as they were. Work elsewhere goes on. A locked layer
//! cannot be removed; once unlocked, the same edits go through.
//!
//! Duplicate makes a layer just above with the same look, unlocked, with a
//! copy of every group on it in every context, loose geometry left and
//! counted; one undo step.

use smith_geom::{DMat4, DVec3};
use smith_model::{
    DefinitionKind, Document, LayerId, ModelError, ON_LOCKED_LAYER, Pickable, UNLOCK_LAYER,
};

fn p(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

/// A 10 mm square face on layer "Kept", and that layer.
fn square_on_a_layer() -> (Document, LayerId) {
    let mut doc = Document::new();
    let kept = doc
        .transaction("Add Layer", |d| Ok(d.add_layer("Kept", [10, 20, 30])))
        .unwrap();
    doc.set_active_layer(Some(kept));
    doc.transaction("draw", |d| {
        d.root_mut().add_polyline(
            &[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)],
            true,
        )?;
        Ok(())
    })
    .unwrap();
    doc.set_active_layer(None);
    assert_eq!(doc.root().face_count(), 1);
    (doc, kept)
}

fn set_locked(doc: &mut Document, layer: LayerId, locked: bool) {
    let mut l = doc.layer(layer).unwrap().clone();
    l.locked = locked;
    doc.transaction("Change Layer", |d| d.set_layer(layer, l))
        .unwrap();
}

fn refused<T: std::fmt::Debug>(r: Result<T, ModelError>) -> bool {
    matches!(r, Err(ModelError::Refused(ref m)) if m == ON_LOCKED_LAYER)
}

#[test]
fn nothing_on_a_locked_layer_changes() {
    let (mut doc, kept) = square_on_a_layer();
    set_locked(&mut doc, kept, true);
    assert!(doc.layer_locked(Some(kept)));
    assert!(!doc.layer_locked(None));
    let before = doc.root().clone();
    let revision = doc.revision();

    // A line across the face would split it.
    assert!(refused(doc.transaction("Line", |d| {
        d.root_mut().add_line(p(5.0, -5.0), p(5.0, 15.0))
    })));
    // Moving one of its corners would reshape it.
    let corner = doc.root().vertex_ids().next().unwrap();
    assert!(refused(doc.transaction("Move", |d| {
        d.root_mut()
            .transform_vertices(&[corner], &DMat4::from_translation(DVec3::X))
    })));
    // Hiding it changes it too.
    let face = doc.root().face_ids().next().unwrap();
    assert!(refused(doc.transaction("Hide", |d| {
        d.root_mut().set_face_hidden(face, true)
    })));
    // Putting it on another layer is a change of it.
    assert!(refused(doc.transaction("Move Selection Here", |d| {
        d.root_mut().set_layer(&[Pickable::Face(face)], None)
    })));

    assert!(*doc.root() == before, "the model is as it was");
    assert_eq!(doc.revision(), revision, "no undo step was made");

    // Work clear of it goes on, on Base.
    doc.transaction("Line", |d| {
        d.root_mut().add_line(p(20.0, 0.0), p(30.0, 0.0))
    })
    .unwrap();
    assert_eq!(doc.root().face_count(), 1);

    // Unlocked, the split goes through.
    set_locked(&mut doc, kept, false);
    doc.transaction("Line", |d| {
        d.root_mut().add_line(p(5.0, -5.0), p(5.0, 15.0))
    })
    .unwrap();
    assert_eq!(doc.root().face_count(), 2);
}

#[test]
fn a_group_on_a_locked_layer_does_not_move() {
    let mut doc = Document::new();
    let (layer, group) = doc
        .transaction("setup", |d| {
            let layer = d.add_layer("Kept", [1, 2, 3]);
            let def = d.create_definition("Group", DefinitionKind::Group);
            let group = d.add_instance(None, def, DMat4::IDENTITY, "")?;
            d.root_mut()
                .set_layer(&[Pickable::Instance(group)], Some(layer))?;
            Ok((layer, group))
        })
        .unwrap();
    set_locked(&mut doc, layer, true);
    assert!(refused(doc.transaction("Move", |d| {
        d.root_mut()
            .transform_instance(group, &DMat4::from_translation(DVec3::Y))
    })));
    assert_eq!(
        doc.root().instance(group).unwrap().transform,
        DMat4::IDENTITY
    );
}

#[test]
fn a_locked_layer_cannot_be_removed() {
    let (mut doc, kept) = square_on_a_layer();
    set_locked(&mut doc, kept, true);
    let r = doc.transaction("Remove Layer", |d| d.remove_layer(kept));
    assert!(matches!(r, Err(ModelError::Refused(ref m)) if m == UNLOCK_LAYER));
    assert!(doc.layer(kept).is_some());
    set_locked(&mut doc, kept, false);
    doc.transaction("Remove Layer", |d| d.remove_layer(kept))
        .unwrap();
    assert!(doc.layer(kept).is_none());
}

#[test]
fn duplicate_copies_groups_onto_a_layer_just_above_and_counts_loose_geometry() {
    let (mut doc, kept) = square_on_a_layer();
    let (outer, inner) = doc
        .transaction("groups", |d| {
            let def = d.create_definition("Box", DefinitionKind::Component);
            let outer = d.add_instance(None, def, DMat4::IDENTITY, "")?;
            d.root_mut()
                .set_layer(&[Pickable::Instance(outer)], Some(kept))?;
            // A group on the layer inside the component too.
            let part = d.create_definition("Part", DefinitionKind::Group);
            let inner = d.add_instance(Some(def), part, DMat4::IDENTITY, "")?;
            d.context_entities_mut(Some(def))
                .unwrap()
                .set_layer(&[Pickable::Instance(inner)], Some(kept))?;
            Ok((outer, inner))
        })
        .unwrap();
    let top = doc
        .transaction("Add Layer", |d| Ok(d.add_layer("Top", [9, 9, 9])))
        .unwrap();
    set_locked(&mut doc, kept, true);
    let revision = doc.revision();

    let (copy, loose) = doc
        .transaction("Duplicate Layer", |d| d.duplicate_layer(kept))
        .unwrap();
    let l = doc.layer(copy).unwrap();
    assert_eq!(l.name, "Kept copy");
    assert_eq!(l.color, [10, 20, 30]);
    assert!(!l.locked, "the copy starts unlocked");
    let names: Vec<String> = doc
        .layer_ids()
        .filter_map(|x| doc.layer(x).map(|y| y.name.clone()))
        .collect();
    assert_eq!(names, ["Base", "Kept", "Kept copy", "Top"]);
    assert!(doc.layer(top).is_some());
    // The square's four edges and its face stay where they are.
    assert_eq!(loose, 5);
    assert_eq!(doc.root().face_count(), 1);
    // Each group has a copy in the same place, on the copy.
    let (_, _, root_groups) = doc.layer_contents(None, Some(copy));
    assert_eq!(root_groups.len(), 1);
    let g = doc.root().instance(root_groups[0]).unwrap();
    let o = doc.root().instance(outer).unwrap();
    assert_eq!(
        g.definition, o.definition,
        "a component's copy stays that component"
    );
    assert_eq!(g.transform, o.transform);
    let def = o.definition;
    let (_, _, inner_groups) = doc.layer_contents(Some(def), Some(copy));
    assert_eq!(inner_groups.len(), 1);
    assert_ne!(inner_groups[0], inner);

    // One undo step takes it all away.
    assert_eq!(doc.undo().as_deref(), Some("Duplicate Layer"));
    assert!(doc.layer(copy).is_none());
    assert_eq!(doc.revision(), revision);
    assert_eq!(doc.root().instance_ids().count(), 1);
}
