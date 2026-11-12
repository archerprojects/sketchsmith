//! SketchSmith — Intersect Faces | v1.0.0 | Thursday 12 November 2026
//!
//! [`intersect`] runs Intersect Faces on a context's geometry
//! ([`smith_model::Entities::intersect_faces`]) and reports what it did:
//! the cut lines added, the faces split, and where faces on one plane
//! overlap. Overlaps are left stacked; combining them is another tool's
//! job, so the report's hint says where to go: Fuse or Clean Up for loose
//! faces, Fuse alone for a face lying on a closed group or outside the
//! open group (Clean Up never reaches there).
//!
//! With Context and With Model need the document to gather what to cut
//! against ([`smith_model::Document::intersect_against`]); the canvas does
//! that and passes it in. Without the document, With Context and With
//! Model cut against the context's loose faces that are not hidden.

use smith_model::{Against, Entities, FaceId, InstanceId, ModelError, Pickable};

use crate::cleanup::ReportLine;

/// The hint after a run that found faces overlapping on one plane.
pub const COMBINE_HINT: &str = "Use Fuse or Clean Up to Combine the Geometry";

/// The hint after a run that found a face lying on a closed group or
/// outside the open group, and no loose overlaps.
pub const FUSE_HINT: &str = "Use Fuse to Combine the Geometry";

/// What the faces are cut against.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum With {
    /// Each selected face against every other.
    #[default]
    Selection,
    /// Also against every loose face in the open context.
    Context,
    /// Also, while a group is open, against the loose faces outside it.
    Model,
}

impl With {
    /// The choices, in drop-down order.
    pub const ALL: [With; 3] = [With::Selection, With::Context, With::Model];

    /// The drop-down names, in [`With::ALL`] order.
    pub const NAMES: [&'static str; 3] = ["Selection", "Context", "Model"];
}

/// One Intersect Faces run.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntersectFaces {
    /// What the faces are cut against.
    pub with: With,
    /// The faces taken.
    pub faces: Vec<FaceId>,
    /// The groups open, outermost first (empty: the top level).
    pub path: Vec<InstanceId>,
}

/// What `op` cuts against in `entities` without the document: nothing for
/// With Selection, else the loose faces that are not hidden.
pub fn against_here(entities: &Entities, op: &IntersectFaces) -> Against {
    match op.with {
        With::Selection => Against::default(),
        With::Context | With::Model => Against {
            here: entities
                .face_ids()
                .filter(|f| entities.face(*f).is_some_and(|x| !x.attributes.hidden))
                .collect(),
            ..Against::default()
        },
    }
}

/// Runs Intersect Faces against `against`. Returns the report lines and
/// the hint to show, if any.
///
/// # Errors
/// As [`Entities::intersect_faces`].
pub fn intersect(
    entities: &mut Entities,
    op: &IntersectFaces,
    against: &Against,
) -> Result<(Vec<ReportLine>, Option<&'static str>), ModelError> {
    let cuts = entities.intersect_faces(&op.faces, against)?;
    let edges = cuts.edges.iter().copied().map(Pickable::Edge).collect();
    let faces = cuts.faces.iter().copied().map(Pickable::Face).collect();
    let flat_on: Vec<FaceId> = cuts
        .flat_on
        .iter()
        .copied()
        .filter(|f| entities.face(*f).is_some())
        .collect();
    let overlapping = cuts
        .stacked
        .iter()
        .chain(&flat_on)
        .copied()
        .map(Pickable::Face)
        .collect();
    let hint = if cuts.overlaps > 0 {
        Some(COMBINE_HINT)
    } else if !cuts.flat_on.is_empty() {
        Some(FUSE_HINT)
    } else {
        None
    };
    Ok((
        vec![
            ReportLine::count("Cut Lines Added", cuts.edges.len(), edges),
            ReportLine::count("Faces Split", cuts.splits, faces),
            ReportLine::count(
                "Overlaps Found",
                cuts.overlaps + cuts.flat_on.len(),
                overlapping,
            ),
        ],
        hint,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_geom::DVec3;

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    #[test]
    fn crossing_faces_report_the_cut_and_the_split() {
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
        m.add_polyline(
            &[
                v(5.0, -2.0, 20.0),
                v(5.0, 12.0, 20.0),
                v(5.0, 12.0, 25.0),
                v(5.0, -2.0, 25.0),
            ],
            true,
        )
        .unwrap();
        let up: Vec<_> = m
            .vertex_ids()
            .filter(|id| m.vertex(*id).unwrap().position().z >= 20.0)
            .collect();
        m.move_vertices(&up, v(0.0, 0.0, -22.0)).unwrap();
        let op = IntersectFaces {
            with: With::Selection,
            faces: m.face_ids().collect(),
            path: Vec::new(),
        };
        let (report, hint) = intersect(&mut m, &op, &Against::default()).unwrap();
        let texts: Vec<&str> = report.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            ["Cut Lines Added: 1", "Faces Split: 1", "Overlaps Found: 0"]
        );
        assert_eq!(hint, None);
        assert_eq!(report[0].items.len(), 1);
    }

    #[test]
    fn nothing_taken_does_nothing() {
        let mut m = Entities::new();
        m.add_polyline(
            &[v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0)],
            true,
        )
        .unwrap();
        let before = m.clone();
        let (report, _) =
            intersect(&mut m, &IntersectFaces::default(), &Against::default()).unwrap();
        assert!(report.iter().all(|l| l.text.ends_with(": 0")));
        assert_eq!(m, before);
    }
}
