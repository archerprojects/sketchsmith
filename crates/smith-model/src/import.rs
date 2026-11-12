//! SketchSmith — building imported geometry | v1.0.0 | Thursday 12 November 2026
//!
//! Imports build straight into an empty group: no sticky drawing, no search
//! for crossings, since nothing else is there. [`Entities::add_polygon`]
//! makes one face from a polygon's corners (with its holes, through
//! [`Entities::add_polygon_with_holes`]) and [`Entities::add_segment`] one
//! edge:
//!
//! * corners merge with vertices already there (the merge grid), so faces
//!   that share corners share edges, each edge made once;
//! * the polygon's winding is the face's front;
//! * a polygon that is not flat within the merge tolerance is split into
//!   flat triangles whose inside edges are soft and smooth, so it still
//!   reads as one surface;
//! * a polygon with no area, or one repeating a face already there, is
//!   skipped ([`Entities::repeated_face`] finds the face it repeats, so a
//!   reversed repeat can paint that face's back);
//! * a hole is kept as the face's inner loop; one off the face's plane is
//!   left out.
//!
//! A file that keeps its own edge flags and curves (SKP) then sets them:
//! [`Entities::import_edge`] gives an edge its soft, smooth and hidden flags
//! and its layer, and [`Entities::import_curve`] ties edges already built to
//! the curve they were drawn from.
//!
//! Meshes of triangles (STL, DAE) are then tidied, as Clean Up would:
//! [`Entities::merge_coplanar_faces`] joins neighbours with the same paint
//! on one plane and takes out points left mid-way along straight edges, and
//! [`Entities::soften_curved`] softens and smooths the edges where faces
//! meet at a shallow angle, so tessellated curves read as surfaces.
//!
//! What was made and skipped is counted in an [`ImportCounts`].

use smith_geom::DVec3;
use smith_geom::curve::Curve;
use smith_geom::plane::{Plane, planar_loop};

use crate::curves::CurveRecord;
use crate::entities::{EdgeUse, Entities, FaceAttributes};
use crate::ids::{CurveId, EdgeId, FaceId, LayerId, VertexId};

/// What building an import made and skipped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportCounts {
    /// Faces made (a split polygon counts each triangle).
    pub faces: usize,
    /// Polygons not flat, split into triangles.
    pub bent: usize,
    /// Polygons with no area, skipped.
    pub zero_area: usize,
    /// Polygons repeating a face, skipped.
    pub repeated: usize,
    /// Edges made from lines.
    pub lines: usize,
    /// Faces joined into a coplanar neighbour.
    pub merged: usize,
    /// Edges softened on curved surfaces.
    pub softened: usize,
}

/// An imported edge's own flags and layer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportedEdge {
    /// Soft.
    pub soft: bool,
    /// Smooth.
    pub smooth: bool,
    /// Hidden.
    pub hidden: bool,
    /// Its layer (`None`: Base).
    pub layer: Option<LayerId>,
}

impl Entities {
    /// Makes a face from `points` (in order; their winding is the front),
    /// painted and layered as `attributes`, as the module documentation
    /// describes. Returns the faces made: one, the triangles of a bent
    /// polygon, or none when it was skipped.
    pub fn add_polygon(
        &mut self,
        points: &[DVec3],
        attributes: &FaceAttributes,
        counts: &mut ImportCounts,
    ) -> Vec<FaceId> {
        self.add_polygon_with_holes(points, &[], attributes, counts)
    }

    /// [`Entities::add_polygon`] with `holes` (each a loop of corners) cut
    /// out of the face.
    pub fn add_polygon_with_holes(
        &mut self,
        points: &[DVec3],
        holes: &[Vec<DVec3>],
        attributes: &FaceAttributes,
        counts: &mut ImportCounts,
    ) -> Vec<FaceId> {
        let Some(ids) = self.corner_ids(points) else {
            counts.zero_area += 1;
            return Vec::new();
        };
        let corners: Vec<DVec3> = ids.iter().map(|v| self.vertices[*v].position()).collect();
        let Ok(fit) = Plane::fit_loop(&corners) else {
            counts.zero_area += 1;
            return Vec::new();
        };
        if holes.is_empty() && self.repeats_a_face(&ids) {
            counts.repeated += 1;
            return Vec::new();
        }
        let hole_ids: Vec<Vec<VertexId>> = holes
            .iter()
            .filter_map(|h| self.corner_ids(h))
            .filter(|h| h.len() >= 3)
            .collect();
        if let Ok(Some(plane)) = planar_loop(&corners) {
            let Some(f) = self.face_from(&ids, fit.normal(), attributes) else {
                counts.zero_area += 1;
                return Vec::new();
            };
            counts.faces += 1;
            for h in &hole_ids {
                let pts: Vec<DVec3> = h.iter().map(|v| self.vertices[*v].position()).collect();
                if !plane.contains_all(&pts) {
                    continue;
                }
                if let Some(l) = self.loop_uses(h) {
                    self.add_inner_loop(f, l);
                }
            }
            return vec![f];
        }
        // Bent: flat triangles, inside edges soft and smooth.
        let flat: Vec<smith_geom::DVec2> = corners.iter().map(|p| fit.to_local(*p)).collect();
        let flat_holes: Vec<Vec<smith_geom::DVec2>> = hole_ids
            .iter()
            .map(|h| {
                h.iter()
                    .map(|v| fit.to_local(self.vertices[*v].position()))
                    .collect()
            })
            .collect();
        let all: Vec<VertexId> = ids
            .iter()
            .chain(hole_ids.iter().flatten())
            .copied()
            .collect();
        let tris = smith_geom::triangulate::triangulate_polygon(&flat, &flat_holes);
        if tris.is_empty() {
            counts.zero_area += 1;
            return Vec::new();
        }
        if tris
            .iter()
            .all(|&[a, b, c]| self.repeats_a_face(&[all[a], all[b], all[c]]))
        {
            counts.repeated += 1;
            return Vec::new();
        }
        counts.bent += 1;
        let mut made = Vec::new();
        for [a, b, c] in tris {
            let tri = [all[a], all[b], all[c]];
            if let Some(f) = self.face_from(&tri, fit.normal(), attributes) {
                counts.faces += 1;
                made.push(f);
            }
        }
        let rings: Vec<&[VertexId]> = std::iter::once(ids.as_slice())
            .chain(hole_ids.iter().map(Vec::as_slice))
            .collect();
        let outline = |a: VertexId, b: VertexId| {
            rings.iter().any(|r| {
                let n = r.len();
                (0..n).any(|i| {
                    let (x, y) = (r[i], r[(i + 1) % n]);
                    (x, y) == (a, b) || (y, x) == (a, b)
                })
            })
        };
        let inside: Vec<EdgeId> = made
            .iter()
            .flat_map(|f| self.faces[*f].outer().to_vec())
            .map(|u| u.edge)
            .filter(|e| {
                let [a, b] = self.edges[*e].vertices();
                !outline(a, b)
            })
            .collect();
        for e in inside {
            self.edges[e].flags.soft = true;
            self.edges[e].flags.smooth = true;
        }
        made
    }

    /// `points` as vertices (merged), dropping repeats in a row and a
    /// closing one; `None` when a point is not finite.
    fn corner_ids(&mut self, points: &[DVec3]) -> Option<Vec<VertexId>> {
        let mut ids: Vec<VertexId> = Vec::with_capacity(points.len());
        for p in points {
            if !p.is_finite() {
                return None;
            }
            let v = self.merge_vertex(*p);
            if ids.last() != Some(&v) {
                ids.push(v);
            }
        }
        while ids.len() > 1 && ids.first() == ids.last() {
            ids.pop();
        }
        Some(ids)
    }

    /// The loop of edges round `ids` (each edge made if missing); `None`
    /// when two corners in a row are one.
    pub(crate) fn loop_uses(&mut self, ids: &[VertexId]) -> Option<Vec<EdgeUse>> {
        let n = ids.len();
        let mut l = Vec::with_capacity(n);
        for i in 0..n {
            let (a, b) = (ids[i], ids[(i + 1) % n]);
            if a == b {
                return None;
            }
            let e = match self.edge_between(a, b) {
                Some(e) => e,
                None => self.insert_edge(a, b),
            };
            l.push(EdgeUse {
                edge: e,
                reversed: self.edges[e].vertices()[0] != a,
            });
        }
        Some(l)
    }

    /// Makes an edge from `a` to `b` (an imported line), merging its ends
    /// with vertices there. `None` when the ends meet or the edge exists.
    pub fn add_segment(&mut self, a: DVec3, b: DVec3, counts: &mut ImportCounts) -> Option<EdgeId> {
        if !a.is_finite() || !b.is_finite() {
            return None;
        }
        let (va, vb) = (self.merge_vertex(a), self.merge_vertex(b));
        if va == vb || self.edge_between(va, vb).is_some() {
            return None;
        }
        counts.lines += 1;
        Some(self.insert_edge(va, vb))
    }

    /// Gives the edge from `a` to `b` the file's own `flags` and layer,
    /// making it (a loose edge) if it is not there. `None` when the ends
    /// meet or are not finite.
    pub fn import_edge(&mut self, a: DVec3, b: DVec3, flags: ImportedEdge) -> Option<EdgeId> {
        if !a.is_finite() || !b.is_finite() {
            return None;
        }
        let (va, vb) = (self.merge_vertex(a), self.merge_vertex(b));
        if va == vb {
            return None;
        }
        let e = match self.edge_between(va, vb) {
            Some(e) => e,
            None => self.insert_edge(va, vb),
        };
        let f = &mut self.edges[e].flags;
        f.soft = flags.soft;
        f.smooth = flags.smooth;
        f.hidden = flags.hidden;
        f.layer = flags.layer;
        Some(e)
    }

    /// Ties the edges already built along `segments` (each by its two ends)
    /// to `curve`, drawn with `count` segments, without redrawing them.
    /// `None` (nothing changes) when an edge is missing or already on a
    /// curve.
    pub fn import_curve(
        &mut self,
        curve: Curve,
        count: u32,
        segments: &[[DVec3; 2]],
    ) -> Option<CurveId> {
        let mut edges = Vec::with_capacity(segments.len());
        for [a, b] in segments {
            let (va, vb) = (self.find_vertex(*a)?, self.find_vertex(*b)?);
            let e = self.edge_between(va, vb)?;
            if self.edges[e].curve.is_some() || edges.contains(&e) {
                return None;
            }
            edges.push(e);
        }
        if edges.is_empty() {
            return None;
        }
        let id = self.curves.insert(CurveRecord {
            curve,
            segments: count,
            edges: Vec::new(),
        });
        for e in &edges {
            self.edges[*e].curve = Some(id);
        }
        self.curves[id].edges = edges;
        Some(id)
    }

    /// Joins every pair of neighbouring faces on one plane with the same
    /// paint (both sides) across the edge between them, until none is left,
    /// then takes out every point left mid-way along a straight edge.
    /// Returns how many faces were joined into another.
    pub fn merge_coplanar_faces(&mut self) -> usize {
        let mut merged = 0;
        loop {
            let edges: Vec<EdgeId> = self.edge_ids().collect();
            let mut progress = false;
            for e in edges {
                let Some(edge) = self.edges.get(e) else {
                    continue;
                };
                let [f1, f2] = edge.faces()[..] else {
                    continue;
                };
                if edge.curve().is_some() {
                    continue;
                }
                let (a, b) = (&self.faces[f1].attributes, &self.faces[f2].attributes);
                if !same_paint(a.front, b.front) || !same_paint(a.back, b.back) {
                    continue;
                }
                if self.merge_faces_across(e).unwrap_or(false) {
                    merged += 1;
                    progress = true;
                }
            }
            if !progress {
                break;
            }
        }
        let points: Vec<VertexId> = self.vertex_ids().collect();
        for v in points {
            if self.vertices.get(v).is_some_and(|x| x.edges().len() == 2) {
                // Only where the two edges run straight on; else it stays.
                let _ = self.join_at_vertex(v, 0.0);
            }
        }
        merged
    }

    /// Softens and smooths every edge between two faces with the same
    /// material that meet at less than `max_angle` (radians; which way each faces
    /// does not matter). Returns how many.
    pub fn soften_curved(&mut self, max_angle: f64) -> usize {
        let cos = max_angle.cos();
        let edges: Vec<EdgeId> = self.edge_ids().collect();
        let mut softened = 0;
        for e in edges {
            let edge = &self.edges[e];
            let [f1, f2] = edge.faces()[..] else {
                continue;
            };
            if edge.flags.soft {
                continue;
            }
            // One material either side; how its image is laid can differ
            // face to face on a curved surface.
            let (a, b) = (&self.faces[f1], &self.faces[f2]);
            let material = |p: Option<crate::paint::Paint>| p.map(|p| p.material);
            if material(a.attributes.front) != material(b.attributes.front)
                || material(a.attributes.back) != material(b.attributes.back)
            {
                continue;
            }
            if a.plane().normal().dot(b.plane().normal()).abs() > cos {
                self.edges[e].flags.soft = true;
                self.edges[e].flags.smooth = true;
                softened += 1;
            }
        }
        softened
    }

    /// A face round `ids` (each edge made if missing), facing along `normal`.
    fn face_from(
        &mut self,
        ids: &[VertexId],
        normal: DVec3,
        attributes: &FaceAttributes,
    ) -> Option<FaceId> {
        let l = self.loop_uses(ids)?;
        self.create_face(l, Some(normal), attributes.clone())
    }

    /// Whether a face already there has exactly the corners `ids`.
    fn repeats_a_face(&self, ids: &[VertexId]) -> bool {
        self.face_with_corners(ids).is_some()
    }

    /// Whether a face already there, holes or not, has exactly the outer
    /// corners `ids`.
    pub(crate) fn face_with_corners_any(&self, ids: &[VertexId]) -> bool {
        let (Some(&a), Some(&b)) = (ids.first(), ids.get(1)) else {
            return false;
        };
        let Some(e) = self.edge_between(a, b) else {
            return false;
        };
        let mut want: Vec<VertexId> = ids.to_vec();
        want.sort();
        self.edges[e].faces().iter().any(|f| {
            let mut have = self.loop_vertices(self.faces[*f].outer());
            have.sort();
            have == want
        })
    }

    /// The face already there with exactly the corners `ids`, if any.
    fn face_with_corners(&self, ids: &[VertexId]) -> Option<FaceId> {
        let (Some(&a), Some(&b)) = (ids.first(), ids.get(1)) else {
            return None;
        };
        let e = self.edge_between(a, b)?;
        let mut want: Vec<VertexId> = ids.to_vec();
        want.sort();
        self.edges[e].faces().iter().copied().find(|f| {
            let face = &self.faces[*f];
            if !face.inner().is_empty() {
                return false;
            }
            let mut have = self.loop_vertices(face.outer());
            have.sort();
            have == want
        })
    }

    /// The face a polygon at `points` repeats (its corners, as
    /// [`Entities::add_polygon`] merges them, are exactly the face's), if
    /// any: what a polygon skipped as repeated repeats.
    pub fn repeated_face(&self, points: &[DVec3]) -> Option<FaceId> {
        let mut ids: Vec<VertexId> = Vec::with_capacity(points.len());
        for p in points {
            let v = self.find_vertex(*p)?;
            if ids.last() != Some(&v) {
                ids.push(v);
            }
        }
        while ids.len() > 1 && ids.first() == ids.last() {
            ids.pop();
        }
        self.face_with_corners(&ids)
    }
}

/// Whether two face sides show the same: the same material, laid the same
/// way. Two flat frames lay it the same when their directions agree and
/// their origins differ only across the image's repeat; tiny differences
/// from fitting each triangle on its own do not count (glTF's single
/// precision texture coordinates move a fitted origin by under 0.001 mm).
fn same_paint(a: Option<crate::paint::Paint>, b: Option<crate::paint::Paint>) -> bool {
    use crate::paint::Mapping;
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.material == b.material
                && match (a.mapping, b.mapping) {
                    (Mapping::Wrap(x), Mapping::Wrap(y)) => x == y,
                    (Mapping::Face(x), Mapping::Face(y)) => {
                        let d = x.origin - y.origin;
                        x.u.distance(y.u) < 1e-5
                            && x.v.distance(y.v) < 1e-5
                            && d.dot(x.u).abs() < 1e-2
                            && d.dot(x.v).abs() < 1e-2
                    }
                    _ => false,
                }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    #[test]
    fn an_octagon_stays_one_face_and_shares_its_edges() {
        let mut e = Entities::new();
        let mut counts = ImportCounts::default();
        let oct: Vec<DVec3> = (0..8)
            .map(|k| {
                let a = std::f64::consts::TAU * f64::from(k) / 8.0;
                p(10.0 * a.cos(), 10.0 * a.sin(), 0.0)
            })
            .collect();
        let f = e.add_polygon(&oct, &FaceAttributes::default(), &mut counts);
        assert_eq!(f.len(), 1);
        assert_eq!(e.face(f[0]).unwrap().outer().len(), 8);
        // Its winding is its front: counter-clockwise from above, so up.
        assert!(e.face(f[0]).unwrap().plane().normal().z > 0.99);
        // A quad standing on one side shares that edge.
        let quad = [
            oct[0],
            oct[1],
            oct[1] + p(0.0, 0.0, 5.0),
            oct[0] + p(0.0, 0.0, 5.0),
        ];
        e.add_polygon(&quad, &FaceAttributes::default(), &mut counts);
        assert_eq!(e.edge_ids().count(), 8 + 3);
        assert_eq!(counts.faces, 2);
        e.validate_links().unwrap();
    }

    #[test]
    fn a_face_the_other_way_faces_down() {
        let mut e = Entities::new();
        let mut counts = ImportCounts::default();
        let sq = [
            p(0.0, 0.0, 0.0),
            p(0.0, 5.0, 0.0),
            p(5.0, 5.0, 0.0),
            p(5.0, 0.0, 0.0),
        ];
        let f = e.add_polygon(&sq, &FaceAttributes::default(), &mut counts);
        assert!(e.face(f[0]).unwrap().plane().normal().z < -0.99);
    }

    #[test]
    fn a_bent_quad_is_two_soft_split_triangles() {
        let mut e = Entities::new();
        let mut counts = ImportCounts::default();
        let bent = [
            p(0.0, 0.0, 0.0),
            p(10.0, 0.0, 0.0),
            p(10.0, 10.0, 1.0),
            p(0.0, 10.0, 0.0),
        ];
        let f = e.add_polygon(&bent, &FaceAttributes::default(), &mut counts);
        assert_eq!(f.len(), 2);
        assert_eq!(counts.bent, 1);
        let soft: Vec<EdgeId> = e
            .edge_ids()
            .filter(|x| e.edge(*x).is_some_and(|d| d.flags.soft && d.flags.smooth))
            .collect();
        assert_eq!(soft.len(), 1);
        e.validate_links().unwrap();
    }

    #[test]
    fn nothing_flat_or_repeated_is_skipped_and_counted() {
        let mut e = Entities::new();
        let mut counts = ImportCounts::default();
        let a = FaceAttributes::default();
        let line = [p(0.0, 0.0, 0.0), p(5.0, 0.0, 0.0), p(10.0, 0.0, 0.0)];
        assert!(e.add_polygon(&line, &a, &mut counts).is_empty());
        let tri = [p(0.0, 0.0, 0.0), p(5.0, 0.0, 0.0), p(0.0, 5.0, 0.0)];
        e.add_polygon(&tri, &a, &mut counts);
        // The same face again, starting elsewhere and wound the other way.
        let again = [tri[2], tri[1], tri[0]];
        assert!(e.add_polygon(&again, &a, &mut counts).is_empty());
        assert_eq!((counts.zero_area, counts.repeated, counts.faces), (1, 1, 1));
        // The face it repeats is found; a polygon off it is not.
        let f = e.face_ids().next();
        assert_eq!(e.repeated_face(&again), f);
        assert_eq!(e.repeated_face(&[tri[0], tri[1], p(9.0, 9.0, 0.0)]), None);
        // A line becomes an edge; one already there does not.
        assert!(
            e.add_segment(p(0.0, 0.0, 0.0), p(0.0, 0.0, 9.0), &mut counts)
                .is_some()
        );
        assert!(e.add_segment(tri[0], tri[1], &mut counts).is_none());
        assert_eq!(counts.lines, 1);
        e.validate_links().unwrap();
    }

    #[test]
    fn a_triangulated_box_merges_back_to_six_quads() {
        let mut e = Entities::new();
        let mut counts = ImportCounts::default();
        let c = [
            p(0.0, 0.0, 0.0),
            p(10.0, 0.0, 0.0),
            p(10.0, 10.0, 0.0),
            p(0.0, 10.0, 0.0),
        ];
        let t = |v: DVec3| v + p(0.0, 0.0, 10.0);
        let mut quads = vec![
            [c[3], c[2], c[1], c[0]],
            [t(c[0]), t(c[1]), t(c[2]), t(c[3])],
        ];
        for i in 0..4 {
            let (a, b) = (c[i], c[(i + 1) % 4]);
            quads.push([a, b, t(b), t(a)]);
        }
        for q in quads {
            e.add_polygon(&[q[0], q[1], q[2]], &FaceAttributes::default(), &mut counts);
            e.add_polygon(&[q[0], q[2], q[3]], &FaceAttributes::default(), &mut counts);
        }
        assert_eq!(e.face_ids().count(), 12);
        assert_eq!(e.merge_coplanar_faces(), 6);
        assert_eq!(e.face_ids().count(), 6);
        assert_eq!(e.edge_ids().count(), 12);
        assert_eq!(e.soften_curved(30f64.to_radians()), 0);
        e.validate_links().unwrap();
    }

    #[test]
    fn merging_stops_at_paint_and_curves_soften() {
        let mut e = Entities::new();
        let mut counts = ImportCounts::default();
        let painted = FaceAttributes {
            front: Some(crate::paint::Paint {
                material: <crate::ids::MaterialId as crate::ids::Key>::from_raw(1),
                mapping: crate::paint::Mapping::Face(crate::paint::TextureFrame::for_side(
                    DVec3::Z,
                    smith_geom::DVec2::ZERO,
                    0.0,
                )),
            }),
            ..FaceAttributes::default()
        };
        let a = [p(0.0, 0.0, 0.0), p(5.0, 0.0, 0.0), p(5.0, 5.0, 0.0)];
        let b = [p(0.0, 0.0, 0.0), p(5.0, 5.0, 0.0), p(0.0, 5.0, 0.0)];
        e.add_polygon(&a, &FaceAttributes::default(), &mut counts);
        e.add_polygon(&b, &painted, &mut counts);
        assert_eq!(e.merge_coplanar_faces(), 0);
        // A fan of strips round an axis, 15° apart: their edges soften.
        let mut f = Entities::new();
        let ring: Vec<DVec3> = (0..=6)
            .map(|k| {
                let t = (15.0 * f64::from(k)).to_radians();
                p(10.0 * t.cos(), 10.0 * t.sin(), 0.0)
            })
            .collect();
        for w in ring.windows(2) {
            let up = p(0.0, 0.0, 20.0);
            f.add_polygon(
                &[w[0], w[1], w[1] + up, w[0] + up],
                &FaceAttributes::default(),
                &mut counts,
            );
        }
        assert_eq!(f.soften_curved(30f64.to_radians()), 5);
        assert_eq!(f.soften_curved(10f64.to_radians()), 0);
    }

    #[test]
    fn a_face_keeps_its_holes_and_a_pane_can_fill_one() {
        let mut e = Entities::new();
        let mut counts = ImportCounts::default();
        let wall = [
            p(0.0, 0.0, 0.0),
            p(100.0, 0.0, 0.0),
            p(100.0, 0.0, 80.0),
            p(0.0, 0.0, 80.0),
        ];
        let window = vec![
            p(20.0, 0.0, 20.0),
            p(40.0, 0.0, 20.0),
            p(40.0, 0.0, 60.0),
            p(20.0, 0.0, 60.0),
        ];
        let off_plane = vec![p(60.0, 5.0, 20.0), p(80.0, 5.0, 20.0), p(80.0, 5.0, 60.0)];
        let f = e.add_polygon_with_holes(
            &wall,
            &[window.clone(), off_plane],
            &FaceAttributes::default(),
            &mut counts,
        );
        assert_eq!(f.len(), 1);
        let face = e.face(f[0]).unwrap();
        assert_eq!(face.inner().len(), 1);
        assert_eq!(face.inner()[0].len(), 4);
        // The pane on the window's outline shares its edges.
        let pane = e.add_polygon(&window, &FaceAttributes::default(), &mut counts);
        assert_eq!(pane.len(), 1);
        assert_eq!(counts.faces, 2);
        assert_eq!(e.edge_ids().count(), 8);
        e.validate_links().unwrap();
    }

    #[test]
    fn imported_edges_take_their_flags_and_curves_tie_to_built_edges() {
        let mut e = Entities::new();
        let mut counts = ImportCounts::default();
        let ring: Vec<DVec3> = (0..12)
            .map(|k| {
                let a = std::f64::consts::TAU * f64::from(k) / 12.0;
                p(10.0 * a.cos(), 10.0 * a.sin(), 0.0)
            })
            .collect();
        e.add_polygon(&ring, &FaceAttributes::default(), &mut counts);
        let soft = ImportedEdge {
            soft: true,
            smooth: true,
            ..ImportedEdge::default()
        };
        let x = e.import_edge(ring[0], ring[1], soft).unwrap();
        assert!(e.edge(x).is_some_and(|d| d.flags.soft && d.flags.smooth));
        // A loose edge is made.
        let loose = e
            .import_edge(p(0.0, 0.0, 0.0), p(0.0, 0.0, 9.0), ImportedEdge::default())
            .unwrap();
        assert!(e.edge(loose).is_some_and(|d| d.faces().is_empty()));
        let frame = smith_geom::curve::CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap();
        let circle = Curve::Circle(smith_geom::curve::Circle::new(frame, 10.0).unwrap());
        let segs: Vec<[DVec3; 2]> = (0..12).map(|i| [ring[i], ring[(i + 1) % 12]]).collect();
        let c = e.import_curve(circle.clone(), 12, &segs).unwrap();
        assert_eq!(e.curve(c).unwrap().edges().len(), 12);
        assert_eq!(e.edge(x).and_then(|d| d.curve()), Some(c));
        // Edges already on a curve, or missing, tie to nothing.
        assert!(e.import_curve(circle.clone(), 12, &segs).is_none());
        assert!(
            e.import_curve(circle, 1, &[[p(50.0, 0.0, 0.0), p(60.0, 0.0, 0.0)]])
                .is_none()
        );
        assert_eq!(e.edge_ids().count(), 13);
        e.validate_links().unwrap();
    }
}
