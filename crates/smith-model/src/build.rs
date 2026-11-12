//! SketchSmith — adding geometry | v1.0.0 | Thursday 12 November 2026
//!
//! [`Entities::add_line`] implements SketchUp's sticky drawing:
//!
//! 1. Both endpoints merge with any vertex within `TOL_MERGE`.
//! 2. The new segment is intersected with every existing edge
//!    ([`segment_segment`]). Crossing and touching points split *both* the
//!    existing edge and the new line; collinear overlaps reuse the existing
//!    stretch instead of duplicating it.
//! 3. The new line is created as a chain of edges between its split points.
//! 4. Each new edge is handed to face resolution (`faces.rs`): auto-face,
//!    face splitting, and holes.
//!
//! The edge search is a linear scan; a spatial query can replace it without
//! changing behaviour.

use smith_geom::DVec3;
use smith_geom::intersect::{SegmentIntersection, closest_point_on_segment, segment_segment};
use smith_geom::tolerance::{TOL_MERGE, TOL_MERGE_SQ, coincident};

use crate::entities::Entities;
use crate::error::ModelError;
use crate::ids::EdgeId;

impl Entities {
    /// Draws a line from `a` to `b` with full sticky behaviour. Returns the edges
    /// created (an existing edge that the line runs along is not repeated).
    ///
    /// # Errors
    /// [`ModelError::NonFinite`] for NaN/infinite input;
    /// [`ModelError::ZeroLength`] if both ends merge to one vertex.
    pub fn add_line(&mut self, a: DVec3, b: DVec3) -> Result<Vec<EdgeId>, ModelError> {
        Self::check_finite(a)?;
        Self::check_finite(b)?;
        let va = self.merge_vertex(a);
        let vb = self.merge_vertex(b);
        if va == vb {
            return Err(ModelError::ZeroLength);
        }
        let pa = self.vertices[va].position();
        let pb = self.vertices[vb].position();

        // Parameters along the new line where it must be split, and points on
        // existing edges where they must be split.
        let mut ts = vec![0.0, 1.0];
        let mut edge_splits: Vec<DVec3> = Vec::new();
        for (_, edge) in self.edges.iter() {
            let [e0, e1] = edge.vertices();
            let p0 = self.vertices[e0].position();
            let p1 = self.vertices[e1].position();
            match segment_segment(pa, pb, p0, p1) {
                SegmentIntersection::None => {}
                SegmentIntersection::Point { point, t_a, .. } => {
                    ts.push(t_a);
                    if !coincident(point, p0) && !coincident(point, p1) {
                        edge_splits.push(point);
                    }
                }
                SegmentIntersection::Overlap { t_a, t_b } => {
                    ts.push(t_a.0);
                    ts.push(t_a.1);
                    for tb in [t_b.0, t_b.1] {
                        let point = p0 + tb * (p1 - p0);
                        if !coincident(point, p0) && !coincident(point, p1) {
                            edge_splits.push(point);
                        }
                    }
                }
            }
        }

        // Split existing edges. Each point is re-located against the current
        // edges, because an earlier split may have shortened the original edge.
        for p in edge_splits {
            if let Some(e) = self.edge_containing(p) {
                self.split_edge(e, p);
            }
        }

        // Build the new line as a chain through its split points.
        ts.sort_by(f64::total_cmp);
        let dir = pb - pa;
        let mut chain = vec![va];
        for t in ts {
            let v = if t <= 0.0 {
                va
            } else if t >= 1.0 {
                vb
            } else {
                self.merge_vertex(pa + t * dir)
            };
            if *chain.last().unwrap_or(&va) != v {
                chain.push(v);
            }
        }
        if *chain.last().unwrap_or(&va) != vb {
            chain.push(vb);
        }

        let mut created = Vec::new();
        for pair in chain.windows(2) {
            let (v0, v1) = (pair[0], pair[1]);
            if v0 != v1 && self.edge_between(v0, v1).is_none() {
                created.push(self.insert_edge(v0, v1));
            }
        }
        for e in &created {
            if self.edges.contains_key(*e) {
                self.resolve_faces_for_edge(*e);
            }
        }
        Ok(created)
    }

    /// Draws a chain of lines through `points`, closing it if `closed`.
    /// Convenience for tools and tests; each segment is an [`Entities::add_line`].
    ///
    /// # Errors
    /// As [`Entities::add_line`].
    pub fn add_polyline(
        &mut self,
        points: &[DVec3],
        closed: bool,
    ) -> Result<Vec<EdgeId>, ModelError> {
        let mut created = Vec::new();
        let n = points.len();
        let segments = if closed { n } else { n.saturating_sub(1) };
        for i in 0..segments {
            created.extend(self.add_line(points[i], points[(i + 1) % n])?);
        }
        Ok(created)
    }

    /// An edge whose interior contains `p` (within `TOL_MERGE`), excluding
    /// points that coincide with the edge's endpoints.
    fn edge_containing(&self, p: DVec3) -> Option<EdgeId> {
        self.edges.iter().find_map(|(id, edge)| {
            let [v0, v1] = edge.vertices();
            let p0 = self.vertices[v0].position();
            let p1 = self.vertices[v1].position();
            let (_, q) = closest_point_on_segment(p, p0, p1);
            (q.distance_squared(p) <= TOL_MERGE_SQ
                && p.distance(p0) > TOL_MERGE
                && p.distance(p1) > TOL_MERGE)
                .then_some(id)
        })
    }
}
