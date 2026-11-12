//! SketchSmith — Close Gaps | v1.0.0 | Thursday 12 November 2026
//!
//! [`close_gaps`] closes the gaps sketching leaves: an open end (a vertex
//! with one edge) within the Gap distance of other geometry is joined to
//! it, nearest first — merged onto a vertex, or onto the nearest point of
//! an edge, which is split there (a vertex wins over an edge at the same
//! distance). An open end never closes onto its own edge or that edge's far
//! end, so a short line is never collapsed. A loop a closed gap completes
//! gets its face when it is flat, as drawing the missing line would; a move
//! that would bend a face is refused ([`ModelError::NotFlat`]) unless
//! Auto-Tri is on. Open ends with nothing near are left and counted.
//!
//! Then, if asked, Join Split Edges (Clean Up's pass) simplifies what is in
//! scope. The scope works as Clean Up's: the selection's points, or
//! everything in the context.

use smith_geom::DVec3;
use smith_geom::intersect::closest_point_on_segment;
use smith_geom::tolerance::TOL_MERGE;
use smith_model::{EdgeId, Entities, ModelError, Pickable, VertexId};

use crate::cleanup::{CleanUp, ReportLine};

/// One Close Gaps run.
#[derive(Debug, Clone, PartialEq)]
pub struct CloseGaps {
    /// The points it works on, by position; `None`: everything in the
    /// context.
    pub scope: Option<Vec<DVec3>>,
    /// How far an open end may be from what it closes onto, mm.
    pub gap: f64,
    /// Join Split Edges afterwards.
    pub join: bool,
    /// Join Split Edges' Angle, radians.
    pub angle: f64,
}

impl Default for CloseGaps {
    fn default() -> Self {
        Self {
            scope: None,
            gap: 1.0,
            join: false,
            angle: 0.0,
        }
    }
}

/// What an open end closes onto.
enum Target {
    Vertex(VertexId),
    Edge(EdgeId, DVec3),
}

/// Runs Close Gaps. Returns its report lines.
///
/// # Errors
/// [`ModelError::NotFlat`] if closing a gap would bend faces (nothing
/// changes), unless Auto-Tri is on; otherwise as the merging and splitting.
pub fn close_gaps(entities: &mut Entities, op: &CloseGaps) -> Result<Vec<ReportLine>, ModelError> {
    let mut closed: Vec<DVec3> = Vec::new();
    // One end at a time: each closing changes what the others see.
    let mut tried: Vec<DVec3> = Vec::new();
    while let Some((end, target)) = next_gap(entities, op, &tried) {
        let here = entities
            .vertex(end)
            .map(smith_model::Vertex::position)
            .unwrap_or_default();
        tried.push(here);
        let onto = match target {
            Target::Vertex(v) => v,
            Target::Edge(e, p) => {
                entities.split_edge_at(e, &[p])?;
                match entities.find_vertex(p) {
                    Some(v) => v,
                    None => continue,
                }
            }
        };
        let at = entities
            .vertex(onto)
            .map(smith_model::Vertex::position)
            .unwrap_or_default();
        entities.merge_vertices(&[(end, onto)])?;
        // The vertex left is one of the two; its edges may close a loop.
        let Some(kept) = entities
            .find_vertex(at)
            .or_else(|| entities.find_vertex(here))
        else {
            continue;
        };
        let edges: Vec<EdgeId> = entities
            .vertex(kept)
            .map(|x| x.edges().to_vec())
            .unwrap_or_default();
        entities.fill_faces(&edges);
        if let Some(p) = entities.vertex(kept).map(smith_model::Vertex::position) {
            closed.push(p);
        }
    }
    let mut report = vec![ReportLine::count(
        "Gaps Closed",
        closed.len(),
        closed
            .iter()
            .filter_map(|p| entities.find_vertex(*p))
            .map(Pickable::Vertex)
            .collect(),
    )];
    if op.join {
        let clean = CleanUp {
            scope: op.scope.clone(),
            angle: op.angle,
            ..CleanUp::default()
        };
        let joined = crate::cleanup::join_split_edges(entities, &clean);
        let items: Vec<Pickable> = joined.iter().copied().map(Pickable::Edge).collect();
        report.push(ReportLine::count("Split Edges Joined", joined.len(), items));
    }
    let left: Vec<Pickable> = open_ends(entities, op)
        .into_iter()
        .map(Pickable::Vertex)
        .collect();
    report.insert(
        1,
        ReportLine::count("Open Ends Left", left.len(), left.clone()),
    );
    Ok(report)
}

/// The open ends in scope: vertices with exactly one edge.
fn open_ends(entities: &Entities, op: &CloseGaps) -> Vec<VertexId> {
    entities
        .vertex_ids()
        .filter(|v| entities.vertex(*v).is_some_and(|x| x.edges().len() == 1))
        .filter(|v| in_scope(entities, op, *v))
        .collect()
}

fn in_scope(entities: &Entities, op: &CloseGaps, v: VertexId) -> bool {
    match (&op.scope, entities.vertex(v)) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(s), Some(x)) => s.iter().any(|p| p.distance(x.position()) <= TOL_MERGE),
    }
}

/// The nearest gap still to close: an open end not yet tried and what it
/// closes onto.
fn next_gap(entities: &Entities, op: &CloseGaps, tried: &[DVec3]) -> Option<(VertexId, Target)> {
    let mut best: Option<(f64, VertexId, Target)> = None;
    for end in open_ends(entities, op) {
        let p = entities.vertex(end)?.position();
        if tried.iter().any(|q| q.distance(p) <= TOL_MERGE) {
            continue;
        }
        if let Some((d, t)) = nearest(entities, end, op.gap)
            && best.as_ref().is_none_or(|(b, _, _)| d < *b)
        {
            best = Some((d, end, t));
        }
    }
    best.map(|(_, v, t)| (v, t))
}

/// Whether open end `end` has something to close onto within `gap`
/// (Inspect Faces' Small Gaps: what Close Gaps would close).
pub(crate) fn has_gap(entities: &Entities, end: VertexId, gap: f64) -> bool {
    nearest(entities, end, gap).is_some()
}

/// What open end `end` would close onto within `gap`, and how far.
fn nearest(entities: &Entities, end: VertexId, gap: f64) -> Option<(f64, Target)> {
    let p = entities.vertex(end)?.position();
    let own = *entities.vertex(end)?.edges().first()?;
    let far = entities.edge(own)?.other(end);
    let mut near: Option<(f64, Target)> = None;
    for v in entities.vertex_ids() {
        if v == end || v == far {
            continue;
        }
        let d = entities.vertex(v)?.position().distance(p);
        if d <= gap && near.as_ref().is_none_or(|(n, _)| d < *n) {
            near = Some((d, Target::Vertex(v)));
        }
    }
    for e in entities.edge_ids() {
        if e == own {
            continue;
        }
        let (a, b) = entities.edge_points(e)?;
        let (_, q) = closest_point_on_segment(p, a, b);
        // An end of the edge is a vertex, and vertices win.
        if q.distance(a) <= TOL_MERGE || q.distance(b) <= TOL_MERGE {
            continue;
        }
        let d = q.distance(p);
        if d <= gap && near.as_ref().is_none_or(|(n, _)| d + TOL_MERGE < *n) {
            near = Some((d, Target::Edge(e, q)));
        }
    }
    near
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    fn texts(r: &[ReportLine]) -> Vec<&str> {
        r.iter().map(|l| l.text.as_str()).collect()
    }

    #[test]
    fn a_small_gap_at_a_corner_closes_and_the_face_appears() {
        // A 10 mm square whose last line stops 0.5 mm short of the start.
        let mut m = Entities::new();
        m.add_polyline(
            &[
                v(0.0, 0.0, 0.0),
                v(0.0, 10.0, 0.0),
                v(10.0, 10.0, 0.0),
                v(10.0, 0.0, 0.0),
                v(0.5, 0.0, 0.0),
            ],
            false,
        )
        .unwrap();
        assert_eq!(m.face_count(), 0);
        let report = close_gaps(&mut m, &CloseGaps::default()).unwrap();
        m.validate().unwrap();
        assert_eq!(texts(&report), ["Gaps Closed: 1", "Open Ends Left: 0"]);
        assert_eq!(m.face_count(), 1);
    }

    #[test]
    fn an_end_near_the_middle_of_an_edge_lands_on_it() {
        let mut m = Entities::new();
        m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
        m.add_line(v(5.0, 0.5, 0.0), v(5.0, 8.0, 0.0)).unwrap();
        let report = close_gaps(&mut m, &CloseGaps::default()).unwrap();
        m.validate().unwrap();
        assert_eq!(report[0].text, "Gaps Closed: 1");
        assert!(m.find_vertex(v(5.0, 0.0, 0.0)).is_some());
        assert_eq!(m.edge_count(), 3);
    }

    #[test]
    fn far_ends_are_left_and_counted() {
        let mut m = Entities::new();
        m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
        m.add_line(v(5.0, 3.0, 0.0), v(5.0, 8.0, 0.0)).unwrap();
        let before = m.clone();
        let report = close_gaps(&mut m, &CloseGaps::default()).unwrap();
        assert_eq!(texts(&report), ["Gaps Closed: 0", "Open Ends Left: 4"]);
        assert_eq!(report[1].items.len(), 4);
        assert_eq!(m, before);
    }

    #[test]
    fn a_short_line_is_not_collapsed() {
        let mut m = Entities::new();
        m.add_line(v(0.0, 0.0, 0.0), v(0.5, 0.0, 0.0)).unwrap();
        let before = m.clone();
        close_gaps(&mut m, &CloseGaps::default()).unwrap();
        assert_eq!(m, before);
    }

    #[test]
    fn only_the_scope_is_closed() {
        let mut m = Entities::new();
        m.add_line(v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)).unwrap();
        m.add_line(v(5.0, 0.5, 0.0), v(5.0, 8.0, 0.0)).unwrap();
        m.add_line(v(0.0, 20.0, 0.0), v(10.0, 20.0, 0.0)).unwrap();
        m.add_line(v(5.0, 20.5, 0.0), v(5.0, 28.0, 0.0)).unwrap();
        let op = CloseGaps {
            scope: Some(vec![v(5.0, 20.5, 0.0), v(5.0, 28.0, 0.0)]),
            ..CloseGaps::default()
        };
        let report = close_gaps(&mut m, &op).unwrap();
        assert_eq!(report[0].text, "Gaps Closed: 1");
        assert!(m.find_vertex(v(5.0, 20.0, 0.0)).is_some());
        assert!(m.find_vertex(v(5.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn join_split_edges_simplifies_straight_runs() {
        let op = CloseGaps {
            join: true,
            ..CloseGaps::default()
        };
        let mut m = Entities::new();
        m.add_polyline(
            &[v(0.0, 0.0, 0.0), v(5.0, 0.0, 0.0), v(10.0, 0.0, 0.0)],
            false,
        )
        .unwrap();
        let report = close_gaps(&mut m, &op).unwrap();
        assert_eq!(
            texts(&report),
            [
                "Gaps Closed: 0",
                "Open Ends Left: 2",
                "Split Edges Joined: 1"
            ]
        );
        assert_eq!(m.edge_count(), 1);
        m.validate().unwrap();
    }
}
