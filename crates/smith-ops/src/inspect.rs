//! SketchSmith — Inspect Faces | v1.0.0 | Thursday 12 November 2026
//!
//! [`inspect_faces`] only finds; it changes nothing. Each report line names
//! what it found and selects it when clicked; the fixes are other tools:
//!
//! * **Open Ends** — vertices with one edge.
//! * **Small Gaps** — open ends within the Gap of other geometry: what
//!   Close Gaps would close.
//! * **Off-Plane Points** — where two neighbouring faces are nearly flat
//!   (at an angle under the Angle) but not exactly: the vertices of each
//!   that lie off the other's plane, with the largest distance.
//! * **Overlapping Faces** — faces on one plane that overlap.
//! * **Crossing Faces** — faces that cross with no edge where they do.
//! * **Zero Faces** — faces with no area (narrower than twice the merge
//!   tolerance), as Clean Up finds them.
//! * **Zero Edges** — edges no longer than the Zero Size.
//!
//! The scope works as Clean Up's: the selection's points, or everything in
//! the context.

use std::collections::BTreeSet;

use smith_geom::DVec3;
use smith_geom::tolerance::TOL_MERGE;
use smith_model::{EdgeId, Entities, FaceId, ModelError, Pickable, VertexId};

use crate::cleanup::ReportLine;

/// One Inspect Faces run.
#[derive(Debug, Clone, PartialEq)]
pub struct InspectFaces {
    /// The points it looks at, by position; `None`: everything in the
    /// context.
    pub scope: Option<Vec<DVec3>>,
    /// Small Gaps: how far an open end may be from what it would close
    /// onto, mm.
    pub gap: f64,
    /// Zero Edges: the longest edge counted, mm.
    pub zero: f64,
    /// Off-Plane Points: the largest angle between neighbours counted as
    /// nearly flat, radians.
    pub angle: f64,
}

impl Default for InspectFaces {
    fn default() -> Self {
        Self {
            scope: None,
            gap: 1.0,
            zero: 0.01,
            angle: 1f64.to_radians(),
        }
    }
}

/// Runs Inspect Faces. Returns its seven report lines; nothing changes.
///
/// # Errors
/// None at present; the signature matches the other repair tools.
pub fn inspect_faces(
    entities: &Entities,
    op: &InspectFaces,
) -> Result<Vec<ReportLine>, ModelError> {
    let in_scope = |p: DVec3| {
        op.scope
            .as_ref()
            .is_none_or(|s| s.iter().any(|q| q.distance(p) <= TOL_MERGE))
    };
    let at = |v: VertexId| entities.vertex(v).map(smith_model::Vertex::position);
    let vertex_in = |v: VertexId| at(v).is_some_and(in_scope);
    let edge_in = |e: EdgeId| {
        entities
            .edge(e)
            .is_some_and(|x| x.vertices().iter().all(|v| vertex_in(*v)))
    };
    let face_in = |f: FaceId| {
        entities.face(f).is_some_and(|x| {
            entities
                .loop_vertices(x.outer())
                .iter()
                .all(|v| vertex_in(*v))
        })
    };
    let faces: Vec<FaceId> = entities.face_ids().filter(|f| face_in(*f)).collect();

    let open: Vec<VertexId> = entities
        .vertex_ids()
        .filter(|v| vertex_in(*v) && entities.vertex(*v).is_some_and(|x| x.edges().len() == 1))
        .collect();
    let gaps: Vec<VertexId> = open
        .iter()
        .copied()
        .filter(|v| crate::gaps::has_gap(entities, *v, op.gap))
        .collect();

    // Nearly flat neighbours.
    let mut off: BTreeSet<VertexId> = BTreeSet::new();
    let mut farthest = 0.0f64;
    for e in entities.edge_ids().filter(|e| edge_in(*e)) {
        let Some(edge) = entities.edge(e) else {
            continue;
        };
        let [f1, f2] = edge.faces()[..] else {
            continue;
        };
        let (Some(a), Some(b)) = (entities.face(f1), entities.face(f2)) else {
            continue;
        };
        let cos = a.plane().normal().dot(b.plane().normal()).abs().min(1.0);
        if cos.acos() >= op.angle {
            continue;
        }
        for (this, other) in [(a, b), (b, a)] {
            for v in this.loops().flat_map(|l| entities.loop_vertices(l)) {
                let Some(p) = at(v) else {
                    continue;
                };
                let d = other.plane().signed_distance(p).abs();
                if d > TOL_MERGE {
                    off.insert(v);
                    farthest = farthest.max(d);
                }
            }
        }
    }

    let overlapping = entities.overlapping_faces(&faces);
    let crossing = entities.crossing_faces(&faces);
    let zero_faces: Vec<FaceId> = faces
        .iter()
        .copied()
        .filter(|f| {
            let Some(face) = entities.face(*f) else {
                return false;
            };
            let pts = entities.loop_points(face.outer());
            let perimeter: f64 = (0..pts.len())
                .map(|i| pts[i].distance(pts[(i + 1) % pts.len()]))
                .sum();
            let area = entities.face_area(*f).unwrap_or(0.0);
            perimeter <= 0.0 || 2.0 * area / perimeter <= TOL_MERGE
        })
        .collect();
    let zero_edges: Vec<EdgeId> = entities
        .edge_ids()
        .filter(|e| edge_in(*e) && entities.edge_length(*e).is_some_and(|l| l <= op.zero))
        .collect();

    let vertices = |v: &[VertexId]| v.iter().copied().map(Pickable::Vertex).collect();
    let faces_of = |f: &[FaceId]| f.iter().copied().map(Pickable::Face).collect();
    let off: Vec<VertexId> = off.into_iter().collect();
    let mut off_line = ReportLine::count("Off-Plane Points", off.len(), vertices(&off));
    if !off.is_empty() {
        off_line.text = format!(
            "Off-Plane Points: {} (up to {} mm)",
            off.len(),
            mm(farthest)
        );
    }
    Ok(vec![
        ReportLine::count("Open Ends", open.len(), vertices(&open)),
        ReportLine::count("Small Gaps", gaps.len(), vertices(&gaps)),
        off_line,
        ReportLine::count(
            "Overlapping Faces",
            overlapping.len(),
            faces_of(&overlapping),
        ),
        ReportLine::count("Crossing Faces", crossing.len(), faces_of(&crossing)),
        ReportLine::count("Zero Faces", zero_faces.len(), faces_of(&zero_faces)),
        ReportLine::count(
            "Zero Edges",
            zero_edges.len(),
            zero_edges.into_iter().map(Pickable::Edge).collect(),
        ),
    ])
}

/// A distance in mm, to three places, without trailing zeros.
fn mm(d: f64) -> String {
    let s = format!("{d:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    fn line(r: &[ReportLine], start: &str) -> String {
        r.iter()
            .find(|l| l.text.starts_with(start))
            .map(|l| l.text.clone())
            .unwrap_or_default()
    }

    #[test]
    fn a_clean_square_reports_all_zeros_and_changes_nothing() {
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
        let before = m.clone();
        let r = inspect_faces(&m, &InspectFaces::default()).unwrap();
        assert_eq!(r.len(), 7);
        assert!(r.iter().all(|l| l.text.ends_with(": 0")), "{r:?}");
        assert_eq!(m, before);
    }

    #[test]
    fn open_ends_and_small_gaps() {
        let mut m = Entities::new();
        m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
        m.add_line(v(5.0, 0.5, 0.0), v(5.0, 8.0, 0.0)).unwrap();
        let r = inspect_faces(&m, &InspectFaces::default()).unwrap();
        assert_eq!(line(&r, "Open Ends"), "Open Ends: 4");
        // Only the end 0.5 mm from the first line has a gap to close.
        assert_eq!(line(&r, "Small Gaps"), "Small Gaps: 1");
    }

    #[test]
    fn nearly_flat_neighbours_show_off_plane_points() {
        let mut m = Entities::new();
        // Two faces sharing an edge, the second tilted by 0.02 mm over 10 mm.
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
                v(10.0, 0.0, 0.0),
                v(20.0, 0.0, 0.02),
                v(20.0, 10.0, 0.02),
                v(10.0, 10.0, 0.0),
            ],
            true,
        )
        .unwrap();
        let r = inspect_faces(&m, &InspectFaces::default()).unwrap();
        assert_eq!(
            line(&r, "Off-Plane Points"),
            "Off-Plane Points: 4 (up to 0.02 mm)"
        );
    }

    #[test]
    fn overlapping_crossing_and_zero_size() {
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
        // A wall through the square, moved there so nothing is split.
        m.add_polyline(
            &[
                v(5.0, -2.0, 18.0),
                v(5.0, 12.0, 18.0),
                v(5.0, 12.0, 23.0),
                v(5.0, -2.0, 23.0),
            ],
            true,
        )
        .unwrap();
        let up: Vec<_> = m
            .vertex_ids()
            .filter(|id| m.vertex(*id).unwrap().position().z > 10.0)
            .collect();
        m.move_vertices(&up, v(0.0, 0.0, -20.0)).unwrap();
        // A second square lying over part of the first, moved there too.
        m.add_polyline(
            &[
                v(2.0, 2.0, 30.0),
                v(4.0, 2.0, 30.0),
                v(4.0, 4.0, 30.0),
                v(2.0, 4.0, 30.0),
            ],
            true,
        )
        .unwrap();
        let up: Vec<_> = m
            .vertex_ids()
            .filter(|id| m.vertex(*id).unwrap().position().z > 25.0)
            .collect();
        m.move_vertices(&up, v(0.0, 0.0, -30.0)).unwrap();
        // A tiny edge.
        m.add_line(v(30.0, 0.0, 0.0), v(30.005, 0.0, 0.0)).unwrap();
        let r = inspect_faces(&m, &InspectFaces::default()).unwrap();
        assert_eq!(line(&r, "Overlapping Faces"), "Overlapping Faces: 2");
        assert_eq!(line(&r, "Crossing Faces"), "Crossing Faces: 2");
        assert_eq!(line(&r, "Zero Edges"), "Zero Edges: 1");
        // After Intersect Faces the crossing has its edge.
        let faces: Vec<_> = m.face_ids().collect();
        let wall_and_floor: Vec<_> = faces
            .iter()
            .copied()
            .filter(|f| m.face_area(*f).unwrap() > 50.0)
            .collect();
        m.intersect_faces(&wall_and_floor, &smith_model::Against::default())
            .unwrap();
        let r = inspect_faces(&m, &InspectFaces::default()).unwrap();
        assert_eq!(line(&r, "Crossing Faces"), "Crossing Faces: 0");
    }

    #[test]
    fn only_the_scope_is_looked_at() {
        let mut m = Entities::new();
        m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
        m.add_line(v(0.0, 20.0, 0.0), v(10.0, 20.0, 0.0)).unwrap();
        let op = InspectFaces {
            scope: Some(vec![v(0.0, 20.0, 0.0), v(10.0, 20.0, 0.0)]),
            ..InspectFaces::default()
        };
        let r = inspect_faces(&m, &op).unwrap();
        assert_eq!(line(&r, "Open Ends"), "Open Ends: 2");
    }
}
