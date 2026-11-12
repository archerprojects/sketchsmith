//! SketchSmith — layer merge and remove tests | v1.0.0 | Thursday 12 November 2026
//!
//! Remove deletes a layer and what is on it, keeping (on the face's own
//! layer) an edge that bounds a face on a layer that stays. Anchor to Base,
//! Merge Down, Merge Visible and Flatten move what is on layers and delete
//! the layers emptied; none changes geometry, and locked layers refuse.
//! Each is one undo step.

use smith_geom::DVec3;
use smith_model::{Document, LayerId, ModelError, UNLOCK_LAYER};

fn p(x: f64, y: f64) -> DVec3 {
    DVec3::new(x, y, 0.0)
}

/// Adds a layer (on top of the stack) and draws a 10 mm square on it at `x`.
fn square_layer(doc: &mut Document, name: &str, x: f64) -> LayerId {
    let l = doc
        .transaction("Add Layer", |d| Ok(d.add_layer(name, [10, 20, 30])))
        .unwrap();
    doc.set_active_layer(Some(l));
    doc.transaction("draw", |d| {
        d.root_mut().add_polyline(
            &[p(x, 0.0), p(x + 10.0, 0.0), p(x + 10.0, 10.0), p(x, 10.0)],
            true,
        )?;
        Ok(())
    })
    .unwrap();
    doc.set_active_layer(None);
    l
}

/// Faces on layer `key` (`None`: Base).
fn faces_on(doc: &Document, key: Option<LayerId>) -> usize {
    let r = doc.root();
    r.face_ids()
        .filter(|f| r.face(*f).unwrap().attributes.layer == key)
        .count()
}

fn set_locked(doc: &mut Document, layer: LayerId, locked: bool) {
    let mut l = doc.layer(layer).unwrap().clone();
    l.locked = locked;
    doc.transaction("Change Layer", |d| d.set_layer(layer, l))
        .unwrap();
}

#[test]
fn remove_deletes_the_layer_and_what_is_on_it() {
    let mut doc = Document::new();
    let a = square_layer(&mut doc, "A", 0.0);
    let _b = square_layer(&mut doc, "B", 20.0);
    assert_eq!(doc.layer_content_count(a), (1, 0, 0));
    doc.transaction("Remove Layer", |d| d.remove_layer(a))
        .unwrap();
    assert!(doc.layer(a).is_none());
    // A's square is gone, faces and edges; B's stays.
    assert_eq!(doc.root().face_count(), 1);
    assert_eq!(doc.root().edge_count(), 4);
    // One undo brings it all back.
    doc.undo();
    assert!(doc.layer(a).is_some());
    assert_eq!(doc.root().face_count(), 2);
}

#[test]
fn remove_keeps_an_edge_a_face_on_another_layer_needs() {
    let mut doc = Document::new();
    // A face on B whose boundary edges are on A.
    let a = doc
        .transaction("Add Layer", |d| Ok(d.add_layer("A", [0, 0, 0])))
        .unwrap();
    let b = doc
        .transaction("Add Layer", |d| Ok(d.add_layer("B", [0, 0, 0])))
        .unwrap();
    doc.set_active_layer(Some(a));
    doc.transaction("draw", |d| {
        d.root_mut().add_polyline(
            &[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)],
            true,
        )?;
        Ok(())
    })
    .unwrap();
    doc.set_active_layer(None);
    let f = doc.root().face_ids().next().unwrap();
    doc.transaction("Move Here", |d| {
        d.root_mut()
            .set_layer(&[smith_model::Pickable::Face(f)], Some(b))?;
        Ok(())
    })
    .unwrap();
    doc.transaction("Remove Layer", |d| d.remove_layer(a))
        .unwrap();
    // B's face keeps its four edges, now on B.
    let r = doc.root();
    assert_eq!(r.face_count(), 1);
    assert_eq!(r.edge_count(), 4);
    assert!(
        r.edge_ids()
            .all(|e| r.edge(e).unwrap().flags.layer == Some(b))
    );
}

#[test]
fn anchor_to_base_moves_everything_to_base_and_deletes_the_layer() {
    let mut doc = Document::new();
    let a = square_layer(&mut doc, "A", 0.0);
    doc.transaction("Anchor to Base", |d| d.anchor_layer(a))
        .unwrap();
    assert!(doc.layer(a).is_none());
    assert_eq!(faces_on(&doc, None), 1);
    assert_eq!(doc.root().edge_count(), 4);
}

#[test]
fn merge_down_moves_onto_the_layer_below() {
    let mut doc = Document::new();
    let a = square_layer(&mut doc, "A", 0.0);
    let b = square_layer(&mut doc, "B", 20.0);
    doc.set_active_layer(Some(b));
    doc.transaction("Merge Down", |d| d.merge_layer_down(b))
        .unwrap();
    assert!(doc.layer(b).is_none());
    assert_eq!(faces_on(&doc, Some(a)), 2);
    // The active layer falls to the one merged into.
    assert_eq!(doc.active_layer(), Some(a));
    // A merges down onto Base.
    doc.transaction("Merge Down", |d| d.merge_layer_down(a))
        .unwrap();
    assert_eq!(faces_on(&doc, None), 2);
    assert_eq!(doc.active_layer(), None);
    // Base has nothing below.
    let base = doc.base_layer().unwrap();
    assert!(doc.merge_layer_down(base).is_err());
}

#[test]
fn merge_visible_leaves_hidden_layers_alone() {
    let mut doc = Document::new();
    let a = square_layer(&mut doc, "A", 0.0);
    let b = square_layer(&mut doc, "B", 20.0);
    let c = square_layer(&mut doc, "C", 40.0);
    let mut l = doc.layer(b).unwrap().clone();
    l.visible = false;
    doc.transaction("Change Layer", |d| d.set_layer(b, l))
        .unwrap();
    let n = doc
        .transaction("Merge Visible", |d| d.merge_visible_layers())
        .unwrap();
    // Base is the lowest shown layer: A and C go into it; hidden B stays.
    assert_eq!(n, 2);
    assert!(doc.layer(a).is_none() && doc.layer(c).is_none());
    assert!(doc.layer(b).is_some());
    assert_eq!(faces_on(&doc, None), 2);
    assert_eq!(faces_on(&doc, Some(b)), 1);
}

#[test]
fn flatten_puts_everything_on_base() {
    let mut doc = Document::new();
    square_layer(&mut doc, "A", 0.0);
    let b = square_layer(&mut doc, "B", 20.0);
    let mut l = doc.layer(b).unwrap().clone();
    l.visible = false;
    doc.transaction("Change Layer", |d| d.set_layer(b, l))
        .unwrap();
    let n = doc.transaction("Flatten", |d| d.flatten_layers()).unwrap();
    assert_eq!(n, 2);
    assert_eq!(doc.layer_ids().count(), 1);
    assert_eq!(faces_on(&doc, None), 2);
    // One undo brings both layers back.
    doc.undo();
    assert_eq!(doc.layer_ids().count(), 3);
}

#[test]
fn locked_layers_refuse_every_merge() {
    let mut doc = Document::new();
    let a = square_layer(&mut doc, "A", 0.0);
    let b = square_layer(&mut doc, "B", 20.0);
    set_locked(&mut doc, a, true);
    let locked = |r: Result<_, ModelError>| matches!(r, Err(ModelError::Refused(ref m)) if m == UNLOCK_LAYER);
    assert!(locked(doc.transaction("Remove", |d| d.remove_layer(a))));
    assert!(locked(doc.transaction("Anchor", |d| d.anchor_layer(a))));
    // B merges down onto locked A: refused.
    assert!(locked(
        doc.transaction("Merge Down", |d| d.merge_layer_down(b))
    ));
    assert!(locked(doc.transaction("Merge Visible", |d| {
        d.merge_visible_layers().map(|_| ())
    })));
    assert!(locked(
        doc.transaction("Flatten", |d| d.flatten_layers().map(|_| ()))
    ));
    assert_eq!(doc.layer_ids().count(), 3);
    assert_eq!(doc.root().face_count(), 2);
}
