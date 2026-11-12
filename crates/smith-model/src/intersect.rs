//! SketchSmith — intersecting faces | v1.0.0 | Thursday 12 November 2026
//!
//! [`Entities::intersect_faces`] cuts faces against each other, as
//! SketchUp's Intersect Faces does:
//!
//! * **Faces on different planes:** along the line where the two planes
//!   meet, the stretches both faces cover become cut lines, drawn with
//!   [`Entities::add_line`], so they split the faces they cross exactly as
//!   a drawn line would.
//! * **Faces on one plane that overlap:** their outlines are split where
//!   they cross, and each face is rebuilt in pieces: the parts only it
//!   covers, and the parts it shares, where the faces are left stacked one
//!   on another (one piece each, with its own layer and paint). Nothing is
//!   joined or deleted.
//!
//! What the selected faces are cut against besides each other is an
//! [`Against`]: loose faces in the same context (cut and split like the
//! selection), and faces from the levels outside an open group (they cut
//! the selection, but are never changed). Closed groups are sealed: they
//! neither cut nor are cut. A selected face lying flat on a closed group's
//! face, or on a face outside the open group, is only counted
//! ([`FaceCuts::flat_on`]). [`Document::intersect_against`] gathers them
//! for With Context and With Model; hidden faces, faces on layers switched
//! off and hidden groups are left out.
//!
//! For Inspect Faces, [`Entities::overlapping_faces`] and
//! [`Entities::crossing_faces`] only find: faces that overlap on one plane,
//! and faces that cross with no edge where they do (where Intersect Faces
//! would add a cut line), by the same tests.
//!
//! [`Entities::fuse_overlaps`] is the same rebuild for overlapping faces on
//! one plane, but only the outline of everything they cover is kept: the
//! faces become one face covering both outlines, with the first face's
//! layer and paint, and the edges inside it go.
//!
//! The rebuild splits the outlines where they cross, makes edges lying on
//! one another one, traces the pieces with a left-most turn walk over the
//! faces' own edges (as face finding does), then checks that every face's
//! pieces add up to its area. If they don't (a pinched outline, say),
//! those faces are left as they were.

use std::collections::{HashMap, HashSet};

use smith_geom::intersect::{
    Crossing2, SegmentIntersection, classify_segments_2d, common_spans, line_in_loops, plane_plane,
    segment_segment,
};
use smith_geom::plane::Plane;
use smith_geom::tolerance::{TOL_MERGE, coincident};
use smith_geom::triangulate::triangulate_polygon;
use smith_geom::{DMat3, DMat4, DVec2, DVec3};

use crate::document::Document;
use crate::entities::{EdgeUse, Entities, FaceAttributes, point_in_polygon, signed_area_2d};
use crate::error::ModelError;
use crate::faces::same_plane;
use crate::ids::{DefinitionId, EdgeId, FaceId, InstanceId, VertexId};

/// The most edge splits one rebuild makes before giving up (a guard; real
/// outlines need far fewer).
const MAX_SPLITS: usize = 100_000;

/// What [`Entities::intersect_faces`] did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaceCuts {
    /// The cut lines added.
    pub edges: Vec<EdgeId>,
    /// How many faces splitting added.
    pub splits: usize,
    /// The faces splitting made.
    pub faces: Vec<FaceId>,
    /// Places where faces on one plane overlap.
    pub overlaps: usize,
    /// The faces left stacked in those places.
    pub stacked: Vec<FaceId>,
    /// Selected faces lying flat on a closed group's face or on a face
    /// outside the open group (nothing is cut there).
    pub flat_on: Vec<FaceId>,
}

/// A face from elsewhere (a closed group, or a level outside the open
/// group), carried into the open context's position.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceShape {
    /// Its plane.
    pub plane: Plane,
    /// Its loops, outer first.
    pub loops: Vec<Vec<DVec3>>,
}

/// What the selected faces are cut against besides each other.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Against {
    /// Loose faces in the same context: cut, and split where cut lines
    /// cross them, like the selection.
    pub here: Vec<FaceId>,
    /// Faces from the levels outside the open group: they cut the
    /// selection but are never changed.
    pub outside: Vec<FaceShape>,
    /// Faces of closed groups: checked for flat overlaps only.
    pub sealed: Vec<FaceShape>,
}

/// A face as it was before any change: its id, plane, normal, attributes and
/// loops (outer first).
#[derive(Clone)]
struct Shot {
    face: FaceId,
    plane: Plane,
    attributes: FaceAttributes,
    loops: Vec<Vec<DVec3>>,
}

/// How a rebuild treats a place several faces cover.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shared {
    /// One piece per face, stacked.
    Stack,
    /// One face over the places `Keep` keeps, with the first face's layer
    /// and paint.
    Fuse(Keep),
}

/// Which places a fusing rebuild keeps, by the faces covering them (the
/// first face is the base, the second the target).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// Covered by any (Union).
    Either,
    /// Covered by the base and not the target (Difference).
    BaseOnly,
    /// Covered by the base and the target (Intersect).
    Both,
}

impl Keep {
    fn keeps(self, by: &[usize]) -> bool {
        match self {
            Keep::Either => !by.is_empty(),
            Keep::BaseOnly => by.contains(&0) && !by.contains(&1),
            Keep::Both => by.contains(&0) && by.contains(&1),
        }
    }
}

/// A piece of a rebuilt region: its outer loop and holes.
struct Cell {
    outer: Vec<EdgeUse>,
    holes: Vec<Vec<EdgeUse>>,
}

impl Entities {
    /// Intersect Faces: cuts each of `faces` against every other and
    /// against `against` (see the module documentation). One call is one
    /// change; ids that no longer name a face are skipped.
    ///
    /// # Errors
    /// As [`Entities::add_line`].
    pub fn intersect_faces(
        &mut self,
        faces: &[FaceId],
        against: &Against,
    ) -> Result<FaceCuts, ModelError> {
        let mut shots = self.shots(faces);
        let picked = shots.len();
        let others: Vec<FaceId> = against
            .here
            .iter()
            .copied()
            .filter(|f| !faces.contains(f))
            .collect();
        shots.extend(self.shots(&others));
        let before: HashSet<FaceId> = self.faces.keys().collect();
        let count = self.faces.len();
        let mut cuts = Vec::new();
        let mut groups = Groups::new(shots.len());
        let mut flat_on = Vec::new();
        for i in 0..picked {
            let a = &shots[i];
            for (j, b) in shots.iter().enumerate().skip(i + 1) {
                if same_plane(&a.plane, &b.plane) {
                    if overlap(&a.plane, &a.loops, &b.loops) {
                        groups.join(i, j);
                    }
                } else {
                    cuts.extend(cut_spans(&a.plane, &a.loops, &b.plane, &b.loops));
                }
            }
            for b in &against.outside {
                if same_plane(&a.plane, &b.plane) {
                    if overlap(&a.plane, &a.loops, &b.loops) {
                        flat_on.push(a.face);
                    }
                } else {
                    cuts.extend(cut_spans(&a.plane, &a.loops, &b.plane, &b.loops));
                }
            }
            if against
                .sealed
                .iter()
                .any(|b| same_plane(&a.plane, &b.plane) && overlap(&a.plane, &a.loops, &b.loops))
            {
                flat_on.push(a.face);
            }
        }
        flat_on.dedup();
        let mut result = FaceCuts::default();
        for group in groups.sets() {
            let members: Vec<Shot> = group.iter().map(|i| shots[*i].clone()).collect();
            if let Some((places, made)) = self.rebuild(&members, Shared::Stack) {
                result.overlaps += places;
                result.stacked.extend(made);
            }
        }
        for (a, b) in cuts {
            match self.add_line(a, b) {
                Ok(edges) => result.edges.extend(edges),
                Err(ModelError::ZeroLength) => {}
                Err(e) => return Err(e),
            }
        }
        result.edges.retain(|e| self.edges.contains_key(*e));
        result.stacked.retain(|f| self.faces.contains_key(*f));
        result.flat_on = flat_on;
        result.faces = self.faces.keys().filter(|f| !before.contains(f)).collect();
        result.splits = self.faces.len().saturating_sub(count);
        Ok(result)
    }

    /// Fuse Overlapped Faces: each set of `faces` overlapping on one plane
    /// becomes one face covering them all, with the first one's layer and
    /// paint (see the module documentation). Returns how many faces were
    /// fused and the faces they became.
    pub fn fuse_overlaps(&mut self, faces: &[FaceId]) -> (usize, Vec<FaceId>) {
        let shots = self.shots(faces);
        let mut groups = Groups::new(shots.len());
        for i in 0..shots.len() {
            for j in i + 1..shots.len() {
                if same_plane(&shots[i].plane, &shots[j].plane)
                    && overlap(&shots[i].plane, &shots[i].loops, &shots[j].loops)
                {
                    groups.join(i, j);
                }
            }
        }
        let (mut fused, mut made) = (0, Vec::new());
        for group in groups.sets() {
            let members: Vec<Shot> = group.iter().map(|i| shots[*i].clone()).collect();
            if let Some((_, faces)) = self.rebuild(&members, Shared::Fuse(Keep::Either)) {
                fused += members.len();
                made.extend(faces);
            }
        }
        made.retain(|f| self.faces.contains_key(*f));
        (fused, made)
    }

    /// Of `faces`, those overlapping another of them on one plane.
    pub fn overlapping_faces(&self, faces: &[FaceId]) -> Vec<FaceId> {
        let shots = self.shots(faces);
        let mut found = HashSet::new();
        for i in 0..shots.len() {
            for j in i + 1..shots.len() {
                let (a, b) = (&shots[i], &shots[j]);
                if same_plane(&a.plane, &b.plane) && overlap(&a.plane, &a.loops, &b.loops) {
                    found.insert(a.face);
                    found.insert(b.face);
                }
            }
        }
        shots
            .iter()
            .map(|s| s.face)
            .filter(|f| found.contains(f))
            .collect()
    }

    /// Of `faces`, those crossing another of them with no edge where they
    /// cross: a stretch both cover whose middle no edge runs through.
    pub fn crossing_faces(&self, faces: &[FaceId]) -> Vec<FaceId> {
        let shots = self.shots(faces);
        let on_edge = |p: DVec3| {
            self.edges.iter().any(|(_, edge)| {
                let [a, b] = edge.vertices();
                let (pa, pb) = (self.vertices[a].position(), self.vertices[b].position());
                smith_geom::intersect::closest_point_on_segment(p, pa, pb)
                    .1
                    .distance(p)
                    <= TOL_MERGE
            })
        };
        let mut found = HashSet::new();
        for i in 0..shots.len() {
            for j in i + 1..shots.len() {
                let (a, b) = (&shots[i], &shots[j]);
                if same_plane(&a.plane, &b.plane) {
                    continue;
                }
                let cut = cut_spans(&a.plane, &a.loops, &b.plane, &b.loops)
                    .into_iter()
                    .any(|(p, q)| !on_edge((p + q) * 0.5));
                if cut {
                    found.insert(a.face);
                    found.insert(b.face);
                }
            }
        }
        shots
            .iter()
            .map(|s| s.face)
            .filter(|f| found.contains(f))
            .collect()
    }

    /// Fuse for two faces on one plane: `base` and `target` rebuilt as the
    /// places `keep` keeps, with the base's layer and paint; both originals
    /// go. Returns the faces made (none: nothing was kept), or `None` if the
    /// pieces could not be traced (nothing changes but split outlines).
    pub(crate) fn fuse_two_faces(
        &mut self,
        base: FaceId,
        target: FaceId,
        keep: Keep,
    ) -> Option<Vec<FaceId>> {
        let shots = self.shots(&[base, target]);
        if shots.len() != 2 {
            return None;
        }
        self.rebuild(&shots, Shared::Fuse(keep))
            .map(|(_, made)| made)
    }

    /// The faces named, as they are now, each once.
    fn shots(&self, faces: &[FaceId]) -> Vec<Shot> {
        let mut seen = HashSet::new();
        faces
            .iter()
            .filter(|f| seen.insert(**f))
            .filter_map(|f| {
                let face = self.faces.get(*f)?;
                Some(Shot {
                    face: *f,
                    plane: *face.plane(),
                    attributes: face.attributes.clone(),
                    loops: face.loops().map(|l| self.loop_points(l)).collect(),
                })
            })
            .collect()
    }

    /// Rebuilds overlapping faces on one plane in pieces. Returns how many
    /// places more than one face covered, and the faces left stacked there
    /// (Stack) or the faces made (Fuse); `None` if
    /// the pieces could not be traced (the faces are left as they were,
    /// with their outlines split where they cross).
    fn rebuild(&mut self, members: &[Shot], shared: Shared) -> Option<(usize, Vec<FaceId>)> {
        let plane = members[0].plane;
        let ids: Vec<FaceId> = members.iter().map(|m| m.face).collect();
        self.split_crossings(&ids);
        self.join_double_edges(&ids);
        let edges: Vec<EdgeId> = {
            let mut seen = HashSet::new();
            ids.iter()
                .filter_map(|f| self.faces.get(*f))
                .flat_map(|f| f.loops().flatten().map(|u| u.edge))
                .filter(|e| seen.insert(*e))
                .collect()
        };
        let cells = self.trace_cells(&edges, &plane)?;
        // Which faces cover each piece, judged against their outlines as
        // they were.
        let regions: Vec<Vec<Vec<DVec2>>> = members
            .iter()
            .map(|m| {
                m.loops
                    .iter()
                    .map(|l| l.iter().map(|p| plane.to_local(*p)).collect())
                    .collect()
            })
            .collect();
        let mut covers: Vec<Vec<usize>> = Vec::new();
        let mut areas = vec![0.0; members.len()];
        for cell in &cells {
            let (sample, area) = self.cell_sample(cell, &plane)?;
            let by: Vec<usize> = (0..members.len())
                .filter(|i| region_contains(&regions[*i], sample))
                .collect();
            for i in &by {
                areas[*i] += area;
            }
            covers.push(by);
        }
        for (i, r) in regions.iter().enumerate() {
            let whole = region_area(r);
            if (areas[i] - whole).abs() > whole.abs() * 1e-6 + TOL_MERGE * TOL_MERGE {
                return None;
            }
        }
        for f in &ids {
            self.remove_face(*f);
        }
        let places = covers.iter().filter(|by| by.len() > 1).count();
        let made = match shared {
            Shared::Stack => self.stack_cells(members, cells, &covers),
            Shared::Fuse(keep) => self.fuse_cells(members, &regions, &cells, &covers, &edges, keep),
        };
        Some((places, made))
    }

    /// One face per piece for each face covering it, with that face's
    /// layer and paint. Returns the faces stacked where several cover a
    /// piece.
    fn stack_cells(
        &mut self,
        members: &[Shot],
        cells: Vec<Cell>,
        covers: &[Vec<usize>],
    ) -> Vec<FaceId> {
        let mut stacked = Vec::new();
        for (cell, by) in cells.into_iter().zip(covers) {
            for i in by {
                let m = &members[*i];
                if let Some(f) = self.create_face(
                    cell.outer.clone(),
                    Some(m.plane.normal()),
                    m.attributes.clone(),
                ) {
                    for h in &cell.holes {
                        self.add_inner_loop(f, h.clone());
                    }
                    if by.len() > 1 {
                        stacked.push(f);
                    }
                }
            }
        }
        stacked
    }

    /// One face over every place `keep` keeps, with the first face's layer
    /// and paint: the edges between kept pieces go, so do edges with no kept
    /// piece beside them, and the outline left is traced again.
    fn fuse_cells(
        &mut self,
        members: &[Shot],
        regions: &[Vec<Vec<DVec2>>],
        cells: &[Cell],
        covers: &[Vec<usize>],
        edges: &[EdgeId],
        keep: Keep,
    ) -> Vec<FaceId> {
        let plane = members[0].plane;
        // How many kept pieces lie beside each edge (the outside counts as
        // not kept).
        let mut covered_sides: HashMap<EdgeId, usize> = HashMap::new();
        for (cell, by) in cells.iter().zip(covers) {
            if !keep.keeps(by) {
                continue;
            }
            for u in cell.outer.iter().chain(cell.holes.iter().flatten()) {
                *covered_sides.entry(u.edge).or_default() += 1;
            }
        }
        let outline: Vec<EdgeId> = edges
            .iter()
            .copied()
            .filter(|e| covered_sides.get(e) == Some(&1))
            .collect();
        for e in edges {
            if covered_sides.get(e) != Some(&1)
                && self.edges.get(*e).is_some_and(|x| x.faces().is_empty())
            {
                self.remove_bare_edge(*e);
            }
        }
        let Some(pieces) = self.trace_cells(&outline, &plane) else {
            return Vec::new();
        };
        let m = &members[0];
        let mut made = Vec::new();
        for cell in pieces {
            let Some((sample, _)) = self.cell_sample(&cell, &plane) else {
                continue;
            };
            let by: Vec<usize> = (0..regions.len())
                .filter(|i| region_contains(&regions[*i], sample))
                .collect();
            if !keep.keeps(&by) {
                continue;
            }
            if let Some(f) =
                self.create_face(cell.outer, Some(m.plane.normal()), m.attributes.clone())
            {
                for h in cell.holes {
                    self.add_inner_loop(f, h);
                }
                made.push(f);
            }
        }
        made
    }

    /// Makes edges of `faces` that join the same two points one edge, used
    /// by every face that used either.
    fn join_double_edges(&mut self, faces: &[FaceId]) {
        let edges: Vec<EdgeId> = {
            let mut seen = HashSet::new();
            faces
                .iter()
                .filter_map(|f| self.faces.get(*f))
                .flat_map(|f| f.loops().flatten().map(|u| u.edge))
                .filter(|e| seen.insert(*e))
                .collect()
        };
        let mut kept: HashMap<(VertexId, VertexId), EdgeId> = HashMap::new();
        for e in edges {
            let [a, b] = self.edges[e].vertices();
            let Some(keep) = kept.get(&(a.min(b), a.max(b))).copied() else {
                kept.insert((a.min(b), a.max(b)), e);
                continue;
            };
            let flipped = self.edges[keep].vertices()[0] != a;
            let users: Vec<FaceId> = self.edges[e].faces().to_vec();
            for f in &users {
                let face = &mut self.faces[*f];
                let loops = std::iter::once(&mut face.outer).chain(face.inner.iter_mut());
                for l in loops {
                    for u in l.iter_mut().filter(|u| u.edge == e) {
                        u.edge = keep;
                        u.reversed ^= flipped;
                    }
                }
                let list = self.edges[keep].faces_mut();
                if !list.contains(f) {
                    list.push(*f);
                }
            }
            self.edges[e].faces_mut().clear();
            self.remove_bare_edge(e);
        }
    }

    /// Splits the outlines of `faces` wherever an edge of one crosses or
    /// touches an edge of another.
    fn split_crossings(&mut self, faces: &[FaceId]) {
        for _ in 0..MAX_SPLITS {
            let owned: Vec<(EdgeId, FaceId)> = faces
                .iter()
                .filter_map(|f| self.faces.get(*f).map(|x| (f, x)))
                .flat_map(|(f, x)| x.loops().flatten().map(|u| (u.edge, *f)))
                .collect();
            let Some((e, p)) = self.first_split(&owned) else {
                return;
            };
            self.split_edge(e, p);
        }
    }

    /// The first place an edge of one face must be split where an edge of
    /// another meets it.
    fn first_split(&self, owned: &[(EdgeId, FaceId)]) -> Option<(EdgeId, DVec3)> {
        let ends = |e: EdgeId| {
            let [a, b] = self.edges[e].vertices();
            (self.vertices[a].position(), self.vertices[b].position())
        };
        let inside = |p: DVec3, (a, b): (DVec3, DVec3)| !coincident(p, a) && !coincident(p, b);
        for (i, (e1, f1)) in owned.iter().enumerate() {
            for (e2, f2) in &owned[i + 1..] {
                if f1 == f2 || e1 == e2 {
                    continue;
                }
                let (s1, s2) = (ends(*e1), ends(*e2));
                match segment_segment(s1.0, s1.1, s2.0, s2.1) {
                    SegmentIntersection::None => {}
                    SegmentIntersection::Point { point, .. } => {
                        if inside(point, s1) {
                            return Some((*e1, point));
                        }
                        if inside(point, s2) {
                            return Some((*e2, point));
                        }
                    }
                    SegmentIntersection::Overlap { .. } => {
                        for (e, s, q) in [
                            (*e1, s1, s2.0),
                            (*e1, s1, s2.1),
                            (*e2, s2, s1.0),
                            (*e2, s2, s1.1),
                        ] {
                            let on = smith_geom::intersect::closest_point_on_segment(q, s.0, s.1).1;
                            if on.distance(q) <= TOL_MERGE && inside(q, s) {
                                return Some((e, q));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// The pieces the edges `edges` divide their plane into: every closed
    /// walk turning furthest left, counter-clockwise ones as pieces and
    /// clockwise ones as holes in the smallest piece around them. `None`
    /// if a walk does not close as a simple loop.
    fn trace_cells(&self, edges: &[EdgeId], plane: &Plane) -> Option<Vec<Cell>> {
        let set: HashSet<EdgeId> = edges.iter().copied().collect();
        let mut at: HashMap<VertexId, Vec<EdgeId>> = HashMap::new();
        for e in edges {
            for v in self.edges[*e].vertices() {
                at.entry(v).or_default().push(*e);
            }
        }
        let local = |v: VertexId| plane.to_local(self.vertices[v].position());
        let mut used: HashSet<(EdgeId, bool)> = HashSet::new();
        let mut pieces = Vec::new();
        let mut holes = Vec::new();
        for e in edges {
            for reversed in [false, true] {
                if used.contains(&(*e, reversed)) {
                    continue;
                }
                let mut walk = Vec::new();
                let mut seen = HashSet::new();
                let (mut edge, mut rev) = (*e, reversed);
                loop {
                    if !used.insert((edge, rev)) {
                        if (edge, rev) == (*e, reversed) {
                            break;
                        }
                        return None;
                    }
                    walk.push(EdgeUse {
                        edge,
                        reversed: rev,
                    });
                    let [a, b] = self.edges[edge].vertices();
                    let (from, to) = if rev { (b, a) } else { (a, b) };
                    if !seen.insert(from) {
                        return None;
                    }
                    let d_in = local(to) - local(from);
                    let mut best: Option<(f64, EdgeId, bool)> = None;
                    for cand in at.get(&to).into_iter().flatten() {
                        if *cand == edge || !set.contains(cand) {
                            continue;
                        }
                        let w = self.edges[*cand].other(to);
                        let d_out = local(w) - local(to);
                        let turn = d_in.perp_dot(d_out).atan2(d_in.dot(d_out));
                        if best.is_none_or(|(t, _, _)| turn > t) {
                            best = Some((turn, *cand, self.edges[*cand].vertices()[0] != to));
                        }
                    }
                    let (_, next, next_rev) = best?;
                    edge = next;
                    rev = next_rev;
                }
                let area = signed_area_2d(&self.loop_local(&walk, plane));
                if area > 0.0 {
                    pieces.push((area, walk));
                } else {
                    holes.push(walk);
                }
            }
        }
        let mut cells: Vec<Cell> = pieces
            .iter()
            .map(|(_, l)| Cell {
                outer: l.clone(),
                holes: Vec::new(),
            })
            .collect();
        for h in holes {
            let q = local(self.use_start(h[0]));
            let around = pieces
                .iter()
                .enumerate()
                .filter(|(_, (_, l))| point_in_polygon(q, &self.loop_local(l, plane)))
                .min_by(|a, b| a.1.0.total_cmp(&b.1.0))
                .map(|(i, _)| i);
            if let Some(i) = around {
                cells[i].holes.push(h);
            }
        }
        Some(cells)
    }

    /// A point inside a piece (the middle of its largest triangle) and the
    /// piece's area, in the plane's coordinates.
    fn cell_sample(&self, cell: &Cell, plane: &Plane) -> Option<(DVec2, f64)> {
        let outer = self.loop_local(&cell.outer, plane);
        let holes: Vec<Vec<DVec2>> = cell
            .holes
            .iter()
            .map(|h| self.loop_local(h, plane))
            .collect();
        let mut all = outer.clone();
        for h in &holes {
            all.extend(h.iter().copied());
        }
        let best = triangulate_polygon(&outer, &holes)
            .into_iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (all[a], all[b], all[c]);
                ((b - a).perp_dot(c - a).abs(), (a + b + c) / 3.0)
            })
            .max_by(|x, y| x.0.total_cmp(&y.0))?;
        let area = signed_area_2d(&outer).abs()
            - holes.iter().map(|h| signed_area_2d(h).abs()).sum::<f64>();
        Some((best.1, area))
    }
}

/// The cut lines between two faces on different planes: the stretches of
/// the line their planes share that both cover.
fn cut_spans(pa: &Plane, la: &[Vec<DVec3>], pb: &Plane, lb: &[Vec<DVec3>]) -> Vec<(DVec3, DVec3)> {
    let Some((o, d)) = plane_plane(pa, pb) else {
        return Vec::new();
    };
    let in_a = line_in_loops(la, pb, o, d);
    let in_b = line_in_loops(lb, pa, o, d);
    common_spans(&in_a, &in_b)
        .into_iter()
        .map(|(t0, t1)| (o + d * t0, o + d * t1))
        .collect()
}

/// Whether two faces on `plane` (loops `a` and `b`) overlap: their
/// outlines cross, or a point inside one lies inside the other.
fn overlap(plane: &Plane, a: &[Vec<DVec3>], b: &[Vec<DVec3>]) -> bool {
    let flat = |loops: &[Vec<DVec3>]| -> Vec<Vec<DVec2>> {
        loops
            .iter()
            .map(|l| l.iter().map(|p| plane.to_local(*p)).collect())
            .collect()
    };
    let (ra, rb) = (flat(a), flat(b));
    let bounds = |r: &[Vec<DVec2>]| {
        r[0].iter().fold(
            (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        )
    };
    let ((alo, ahi), (blo, bhi)) = (bounds(&ra), bounds(&rb));
    if alo.cmpgt(bhi).any() || blo.cmpgt(ahi).any() {
        return false;
    }
    let segments = |r: &[Vec<DVec2>]| -> Vec<(DVec2, DVec2)> {
        r.iter()
            .flat_map(|l| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()])))
            .collect()
    };
    let (sa, sb) = (segments(&ra), segments(&rb));
    if sa.iter().any(|(a0, a1)| {
        sb.iter()
            .any(|(b0, b1)| classify_segments_2d(*a0, *a1, *b0, *b1) == Crossing2::Proper)
    }) {
        return true;
    }
    samples(&ra).iter().any(|q| region_contains(&rb, *q))
        || samples(&rb).iter().any(|q| region_contains(&ra, *q))
}

impl Document {
    /// What the selected faces in the group open along `path` (empty: the
    /// top level) are cut against: its loose faces, its closed groups'
    /// faces (sealed), and with `model`, the loose faces of the levels
    /// outside it and those levels' other groups (sealed), carried into the
    /// open group's position. What cannot be seen is left out. `None` if
    /// the path names no open group.
    pub fn intersect_against(&self, path: &[InstanceId], model: bool) -> Option<Against> {
        let mut ctxs: Vec<Option<DefinitionId>> = vec![None];
        let mut placed: Vec<DMat4> = vec![DMat4::IDENTITY];
        for id in path {
            let inst = self.context(*ctxs.last()?)?.instance(*id)?;
            placed.push(*placed.last()? * inst.transform);
            ctxs.push(Some(inst.definition));
        }
        let open = self.context(*ctxs.last()?)?;
        let mut against = Against {
            here: open
                .faces
                .iter()
                .filter(|(_, f)| self.face_shows(f))
                .map(|(k, _)| k)
                .collect(),
            ..Against::default()
        };
        self.sealed_in(open, &DMat4::IDENTITY, None, 0, &mut against.sealed);
        if model {
            let to_open = placed.last()?.inverse();
            for (level, id) in path.iter().enumerate() {
                let Some(e) = self.context(ctxs[level]) else {
                    continue;
                };
                let m = to_open * placed[level];
                against.outside.extend(
                    e.faces
                        .iter()
                        .map(|(_, f)| f)
                        .filter(|f| self.face_shows(f))
                        .filter_map(|f| carried(e, f, &m)),
                );
                self.sealed_in(e, &m, Some(*id), 0, &mut against.sealed);
            }
        }
        Some(against)
    }

    /// Whether a face can be seen: not hidden, on a layer switched on.
    fn face_shows(&self, f: &crate::entities::Face) -> bool {
        !f.attributes.hidden && self.layer_visible(f.attributes.layer)
    }

    /// The faces of the groups in `e` (but `skip`), nested ones too, placed
    /// by `m`, that can be seen.
    fn sealed_in(
        &self,
        e: &Entities,
        m: &DMat4,
        skip: Option<InstanceId>,
        depth: u32,
        out: &mut Vec<FaceShape>,
    ) {
        if depth > 64 {
            return;
        }
        for (id, inst) in e.instances.iter() {
            if Some(id) == skip || inst.hidden || !self.layer_visible(inst.layer) {
                continue;
            }
            let Some(inner) = self.context(Some(inst.definition)) else {
                continue;
            };
            let t = *m * inst.transform;
            out.extend(
                inner
                    .faces
                    .iter()
                    .map(|(_, f)| f)
                    .filter(|f| self.face_shows(f))
                    .filter_map(|f| carried(inner, f, &t)),
            );
            self.sealed_in(inner, &t, None, depth + 1, out);
        }
    }
}

/// Face `f` of `e` carried by `m`, or `None` if `m` flattens it.
pub(crate) fn carried(e: &Entities, f: &crate::entities::Face, m: &DMat4) -> Option<FaceShape> {
    let loops: Vec<Vec<DVec3>> = f
        .loops()
        .map(|l| {
            e.loop_points(l)
                .into_iter()
                .map(|p| m.transform_point3(p))
                .collect()
        })
        .collect();
    let normals = DMat3::from_mat4(*m).inverse().transpose();
    let point = m.transform_point3(f.plane().origin());
    let plane = Plane::from_point_normal(point, normals * f.plane().normal()).ok()?;
    Some(FaceShape { plane, loops })
}

/// Points inside a region: the middle of each of its triangles.
fn samples(region: &[Vec<DVec2>]) -> Vec<DVec2> {
    let mut all = region[0].clone();
    for h in &region[1..] {
        all.extend(h.iter().copied());
    }
    triangulate_polygon(&region[0], &region[1..])
        .into_iter()
        .map(|[a, b, c]| (all[a] + all[b] + all[c]) / 3.0)
        .collect()
}

/// Whether `q` is inside a region's outer loop and outside its holes.
fn region_contains(region: &[Vec<DVec2>], q: DVec2) -> bool {
    point_in_polygon(q, &region[0]) && !region[1..].iter().any(|h| point_in_polygon(q, h))
}

/// A region's area: outer loop less holes.
fn region_area(region: &[Vec<DVec2>]) -> f64 {
    signed_area_2d(&region[0]).abs()
        - region[1..]
            .iter()
            .map(|h| signed_area_2d(h).abs())
            .sum::<f64>()
}

/// Faces joined into sets (union–find), so a face overlapping two others
/// is rebuilt with both at once.
struct Groups {
    parent: Vec<usize>,
}

impl Groups {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn root(&mut self, i: usize) -> usize {
        let mut r = i;
        while self.parent[r] != r {
            r = self.parent[r];
        }
        self.parent[i] = r;
        r
    }

    fn join(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.root(a), self.root(b));
        if ra != rb {
            self.parent[ra.max(rb)] = ra.min(rb);
        }
    }

    /// Every set of more than one, each in its first member's order.
    fn sets(&mut self) -> Vec<Vec<usize>> {
        let mut by: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..self.parent.len() {
            let r = self.root(i);
            by.entry(r).or_default().push(i);
        }
        let mut sets: Vec<Vec<usize>> = by.into_values().filter(|s| s.len() > 1).collect();
        sets.sort();
        sets
    }
}
