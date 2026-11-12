//! SketchSmith — hidden-line drawings | v1.0.0 | Thursday 12 November 2026
//!
//! The lines a 2D drawing shows, for a parallel view of the model (or the
//! selection), worked out exactly from the geometry rather than from a
//! picture: the base of PDF, SVG and 2D DXF.
//!
//! Three kinds of line:
//! * **Edges** that Present mode draws: not soft, not hidden, not smoothed
//!   away, on a shown layer (loose edges included).
//! * **Outlines** of smoothed surfaces: a soft or smoothed edge whose two
//!   faces lie on the same side of the plane through it along the view, so
//!   the surface turns away there (a cylinder's sides, a sphere's rim). The
//!   test needs no face orientation, so a reversed face cannot fool it.
//! * **Cuts**: with a section plane, everything on the side its normal
//!   points to is removed, and the plane's crossing of each face is drawn.
//!
//! Every line is split exactly where the faces in front hide it; both the
//! shown and the hidden pieces are kept, so a drawing can leave hidden
//! lines out or draw them dashed. A line lying on a face is not hidden by
//! it. Only what is shown takes part: hidden faces, layers and groups
//! neither appear nor hide anything. A 2D grid over the faces keeps each
//! line to the faces near it.
//!
//! Lines come out in model millimetres on the drawing plane: x to the
//! view's right, y up, the view's centre at 0, 0.

use glam::{DMat3, DMat4, DVec2, DVec3};
use smith_model::{
    Document, Entities, FaceId, InstanceId, LayerId, LineStyle, SectionPlane, VertexId,
};

use super::{Scope, gather};

/// A parallel view: looking along `direction` at `centre`, `up` showing as
/// up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawingView {
    /// The point drawn at 0, 0.
    pub centre: DVec3,
    /// The way the view looks (from the eye into the model).
    pub direction: DVec3,
    /// The direction drawn upward (need not be square to `direction`).
    pub up: DVec3,
}

impl DrawingView {
    /// The view's right, up and forward directions, unit and square.
    fn basis(&self) -> (DVec3, DVec3, DVec3) {
        let f = self.direction.normalize_or(DVec3::NEG_Z);
        let mut r = f.cross(self.up);
        if r.length_squared() < 1e-18 {
            r = f.any_orthonormal_vector();
        }
        let r = r.normalize();
        (r, r.cross(f), f)
    }

    /// Projects world points onto the drawing plane, mm.
    pub(crate) fn flatten(&self) -> impl Fn(DVec3) -> DVec2 + use<> {
        let (r, u, _) = self.basis();
        let centre = self.centre;
        move |p: DVec3| {
            let q = p - centre;
            DVec2::new(q.dot(r), q.dot(u))
        }
    }
}

/// What a line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// A model edge.
    Edge,
    /// The outline of a smoothed surface.
    Outline,
    /// Where a section plane cuts a face.
    Cut,
}

/// One piece of a line, wholly shown or wholly hidden.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawnLine {
    /// Start, mm on the drawing plane.
    pub a: DVec2,
    /// End, mm on the drawing plane.
    pub b: DVec2,
    /// What it is.
    pub kind: LineKind,
    /// Shown (`false`: hidden behind faces).
    pub visible: bool,
    /// The name of its layer (an edge's; a face's for outlines and cuts).
    pub layer: String,
    /// Its Line Style: the edge's own, else its layer's (edges only).
    pub style: Option<LineStyle>,
}

/// A drawing: its lines, and its dimensions and text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Drawing {
    /// Every piece, shown and hidden.
    pub lines: Vec<DrawnLine>,
    /// Dimensions and leader text (see [`super::marks`]).
    pub marks: Vec<super::Mark>,
}

impl Drawing {
    /// The box round every line and mark, `(min, max)`; `None` when empty.
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        let marks = self.marks.iter().flat_map(|m| {
            m.lines
                .iter()
                .flatten()
                .copied()
                .chain(m.text.as_ref().map(|t| t.0))
        });
        let mut it = self.lines.iter().flat_map(|l| [l.a, l.b]).chain(marks);
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }
}

/// A line before hiding: world ends, and what it carries.
struct Candidate {
    a: DVec3,
    b: DVec3,
    kind: LineKind,
    layer: String,
    style: Option<LineStyle>,
}

/// A face triangle in view space: x right, y up, z depth (larger farther).
struct Tri {
    p: [DVec3; 3],
    lo: DVec2,
    hi: DVec2,
}

/// Draws what `scope` takes of `doc` from `view`, cut by `section` (world
/// coordinates) when given.
pub fn draw(
    doc: &Document,
    scope: Scope<'_>,
    view: &DrawingView,
    section: Option<&SectionPlane>,
) -> Drawing {
    let (r, u, f) = view.basis();
    let to_view = |p: DVec3| {
        let q = p - view.centre;
        DVec3::new(q.dot(r), q.dot(u), q.dot(f))
    };
    // The faces that hide, cut by the section plane.
    let mut tris = Vec::new();
    for face in gather(doc, scope).parts.iter().flat_map(|p| &p.faces) {
        for t in &face.triangles {
            let w = t.map(|k| face.corners[k].point);
            for piece in clip_triangle(w, section) {
                let p = piece.map(to_view);
                let flat = |q: DVec3| q.truncate();
                let area = (flat(p[1]) - flat(p[0])).perp_dot(flat(p[2]) - flat(p[0]));
                // Seen edge-on, a triangle hides nothing.
                if area.abs() < 1e-12 {
                    continue;
                }
                let lo = flat(p[0]).min(flat(p[1])).min(flat(p[2]));
                let hi = flat(p[0]).max(flat(p[1])).max(flat(p[2]));
                tris.push(Tri { p, lo, hi });
            }
        }
    }

    // The lines to draw.
    let mut lines = Vec::new();
    let mut walk = |e: &Entities, faces: Option<&[FaceId]>, world: DMat4| {
        candidates(doc, e, faces, world, f, section, &mut lines);
    };
    match scope {
        Scope::Model => {
            walk(doc.root(), None, DMat4::IDENTITY);
            let all: Vec<InstanceId> = doc.root().instance_ids().collect();
            instances(doc, doc.root(), &all, DMat4::IDENTITY, &mut walk);
        }
        Scope::Selection {
            context,
            local,
            faces,
            instances: picked,
        } => {
            let entities = match context {
                None => Some(doc.root()),
                Some(id) => doc.definition_entities(id),
            };
            if let Some(e) = entities {
                walk(e, Some(faces), local);
                instances(doc, e, picked, local, &mut walk);
            }
        }
    }

    // Hide them.
    let grid = Grid::new(&tris);
    let mut out = Drawing::default();
    for c in lines {
        // Only the part on the kept side of the section plane.
        let (wa, wb) = match section {
            None => (c.a, c.b),
            Some(s) => {
                let (da, db) = (s.distance(c.a), s.distance(c.b));
                match (da <= 0.0, db <= 0.0) {
                    (true, true) => (c.a, c.b),
                    (false, false) => continue,
                    (true, false) => (c.a, c.a + (c.b - c.a) * (da / (da - db))),
                    (false, true) => (c.a + (c.b - c.a) * (da / (da - db)), c.b),
                }
            }
        };
        let (a, b) = (to_view(wa), to_view(wb));
        let hidden = grid.hidden(&tris, a, b);
        let drawn = (b - a).truncate().length();
        let mut push = |t0: f64, t1: f64, visible: bool| {
            // Pieces shorter than the merge tolerance are slivers.
            if (t1 - t0) * drawn > 1e-4 {
                let at = |t: f64| (a + (b - a) * t).truncate();
                out.lines.push(DrawnLine {
                    a: at(t0),
                    b: at(t1),
                    kind: c.kind,
                    visible,
                    layer: c.layer.clone(),
                    style: c.style,
                });
            }
        };
        let mut t = 0.0;
        for (h0, h1) in hidden {
            push(t, h0, true);
            push(h0, h1, false);
            t = h1;
        }
        push(t, 1.0, true);
    }
    out
}

/// Walks the shown `instances` of `e` placed by `parent`, and everything in
/// them.
fn instances(
    doc: &Document,
    e: &Entities,
    ids: &[InstanceId],
    parent: DMat4,
    walk: &mut impl FnMut(&Entities, Option<&[FaceId]>, DMat4),
) {
    for id in ids {
        let Some(inst) = e.instance(*id) else {
            continue;
        };
        if inst.hidden || !doc.layer_visible(inst.layer) {
            continue;
        }
        let Some(inner) = doc.definition_entities(inst.definition) else {
            continue;
        };
        let world = parent * inst.transform;
        walk(inner, None, world);
        let nested: Vec<InstanceId> = inner.instance_ids().collect();
        instances(doc, inner, &nested, world, walk);
    }
}

/// A layer's name (`None`: Base).
fn layer_name(doc: &Document, layer: Option<LayerId>) -> String {
    layer
        .or_else(|| doc.base_layer())
        .and_then(|l| doc.layer(l))
        .map(|l| l.name.clone())
        .unwrap_or_default()
}

/// The edges, outlines and cuts of `e` (only round `faces`, when given),
/// placed by `world`, seen along `forward`.
fn candidates(
    doc: &Document,
    e: &Entities,
    faces: Option<&[FaceId]>,
    world: DMat4,
    forward: DVec3,
    section: Option<&SectionPlane>,
    out: &mut Vec<Candidate>,
) {
    let cos = doc.look().smoothing_angle_deg.to_radians().cos();
    let shown_face = |f: FaceId| {
        e.face(f).is_some_and(|x| {
            !x.attributes.hidden
                && doc.layer_visible(x.attributes.layer)
                && faces.is_none_or(|s| s.contains(&f))
        })
    };
    let pos = |v: VertexId| {
        world.transform_point3(
            e.vertex(v)
                .map_or(DVec3::ZERO, smith_model::Vertex::position),
        )
    };
    let centre = |f: FaceId| {
        let pts: Vec<DVec3> = e
            .face(f)
            .map(|x| e.loop_points(x.outer()))
            .unwrap_or_default();
        world.transform_point3(pts.iter().copied().sum::<DVec3>() / pts.len().max(1) as f64)
    };
    for id in e.edge_ids() {
        let Some(edge) = e.edge(id) else { continue };
        let flags = edge.flags;
        if flags.hidden || !doc.layer_visible(flags.layer) {
            continue;
        }
        let users: Vec<FaceId> = edge
            .faces()
            .iter()
            .copied()
            .filter(|f| shown_face(*f))
            .collect();
        // With a selection, only the edges round its faces.
        if faces.is_some() && users.is_empty() {
            continue;
        }
        let [va, vb] = edge.vertices();
        let (a, b) = (pos(va), pos(vb));
        let smoothed = flags.soft || e.edge_is_smooth(id, cos);
        if !smoothed {
            out.push(Candidate {
                a,
                b,
                kind: LineKind::Edge,
                layer: layer_name(doc, flags.layer),
                style: flags.style.or_else(|| {
                    flags
                        .layer
                        .or_else(|| doc.base_layer())
                        .and_then(|l| doc.layer(l))
                        .and_then(|l| l.style)
                }),
            });
            continue;
        }
        // A smoothed edge is an outline where its surface turns away.
        if let [fa, fb] = users[..] {
            let m = (b - a).cross(forward);
            if m.length_squared() < 1e-18 {
                continue;
            }
            let (sa, sb) = (m.dot(centre(fa) - a), m.dot(centre(fb) - a));
            if sa * sb > 0.0 {
                let layer = e.face(fa).map(|x| x.attributes.layer).unwrap_or_default();
                out.push(Candidate {
                    a,
                    b,
                    kind: LineKind::Outline,
                    layer: layer_name(doc, layer),
                    style: None,
                });
            }
        }
    }
    // The section plane's crossing of each face, worked in the context's
    // own coordinates.
    if let Some(s) = section {
        let inverse = world.inverse();
        let normal = (DMat3::from_mat4(world).transpose() * s.normal).normalize_or(DVec3::Z);
        let Some(local) = SectionPlane::new(inverse.transform_point3(s.point), normal) else {
            return;
        };
        let slice = e.slice(&local, shown_face);
        for [a, b] in slice.segments {
            // The face it crosses gives the layer: the face whose plane
            // holds the segment's middle.
            let mid = (a + b) * 0.5;
            let layer = e
                .face_ids()
                .filter(|f| shown_face(*f))
                .find(|f| {
                    e.face(*f)
                        .is_some_and(|x| x.plane().signed_distance(mid).abs() < 1e-6)
                })
                .and_then(|f| e.face(f))
                .map(|x| x.attributes.layer)
                .unwrap_or_default();
            out.push(Candidate {
                a: world.transform_point3(a),
                b: world.transform_point3(b),
                kind: LineKind::Cut,
                layer: layer_name(doc, layer),
                style: None,
            });
        }
    }
}

/// The parts of triangle `t` on the kept side of `section` (all of it with
/// none): none, one or two triangles.
fn clip_triangle(t: [DVec3; 3], section: Option<&SectionPlane>) -> Vec<[DVec3; 3]> {
    let Some(s) = section else {
        return vec![t];
    };
    let d = t.map(|p| s.distance(p));
    let mut poly: Vec<DVec3> = Vec::new();
    for i in 0..3 {
        let j = (i + 1) % 3;
        if d[i] <= 0.0 {
            poly.push(t[i]);
        }
        if (d[i] <= 0.0) != (d[j] <= 0.0) {
            poly.push(t[i] + (t[j] - t[i]) * (d[i] / (d[i] - d[j])));
        }
    }
    (1..poly.len().saturating_sub(1))
        .map(|k| [poly[0], poly[k], poly[k + 1]])
        .collect()
}

/// A grid over the drawing plane, each cell listing the triangles whose
/// box reaches it.
struct Grid {
    lo: DVec2,
    cell: DVec2,
    n: usize,
    cells: Vec<Vec<usize>>,
}

impl Grid {
    fn new(tris: &[Tri]) -> Self {
        let lo = tris.iter().fold(DVec2::splat(f64::MAX), |a, t| a.min(t.lo));
        let hi = tris.iter().fold(DVec2::splat(f64::MIN), |a, t| a.max(t.hi));
        let n = ((tris.len() as f64).sqrt().ceil() as usize).clamp(1, 512);
        let size = (hi - lo).max(DVec2::splat(1e-9));
        let mut grid = Self {
            lo,
            cell: size / n as f64,
            n,
            cells: vec![Vec::new(); n * n],
        };
        for (i, t) in tris.iter().enumerate() {
            let (c0, c1) = (grid.at(t.lo), grid.at(t.hi));
            for y in c0.1..=c1.1 {
                for x in c0.0..=c1.0 {
                    grid.cells[y * n + x].push(i);
                }
            }
        }
        grid
    }

    /// The cell holding `p`, clamped into the grid.
    fn at(&self, p: DVec2) -> (usize, usize) {
        let c = ((p - self.lo) / self.cell).floor();
        let k = |v: f64| (v.max(0.0) as usize).min(self.n - 1);
        (k(c.x), k(c.y))
    }

    /// The parts of segment `a`–`b` (view space) hidden behind the
    /// triangles, as sorted, merged `(t0, t1)` spans of 0–1.
    fn hidden(&self, tris: &[Tri], a: DVec3, b: DVec3) -> Vec<(f64, f64)> {
        if tris.is_empty() {
            return Vec::new();
        }
        let (lo, hi) = (
            a.truncate().min(b.truncate()),
            a.truncate().max(b.truncate()),
        );
        let (c0, c1) = (self.at(lo), self.at(hi));
        let mut seen = std::collections::BTreeSet::new();
        let mut spans = Vec::new();
        for y in c0.1..=c1.1 {
            for x in c0.0..=c1.0 {
                for &i in &self.cells[y * self.n + x] {
                    let t = &tris[i];
                    if t.hi.x < lo.x || t.lo.x > hi.x || t.hi.y < lo.y || t.lo.y > hi.y {
                        continue;
                    }
                    if seen.insert(i)
                        && let Some(span) = behind(t, a, b)
                    {
                        spans.push(span);
                    }
                }
            }
        }
        spans.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (s0, s1) in spans {
            match merged.last_mut() {
                Some(last) if s0 <= last.1 + 1e-12 => last.1 = last.1.max(s1),
                _ => merged.push((s0, s1)),
            }
        }
        merged
    }
}

/// The span of segment `a`–`b` (view space) that triangle `t` hides: inside
/// its outline on the drawing plane and behind it.
fn behind(t: &Tri, a: DVec3, b: DVec3) -> Option<(f64, f64)> {
    let (pa, pb) = (a.truncate(), b.truncate());
    let p = t.p.map(|q| q.truncate());
    let ccw = (p[1] - p[0]).perp_dot(p[2] - p[0]) > 0.0;
    // Clip the segment to the triangle's three sides, its outline
    // included: an edge lying along the outline of a face in front is
    // covered by it (the depth test below spares the face's own edges).
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for i in 0..3 {
        let (s, e) = (p[i], p[(i + 1) % 3]);
        let side = |q: DVec2| {
            let v = (e - s).perp_dot(q - s) + 1e-9 * (e - s).length();
            if ccw { v } else { -v + 2e-9 * (e - s).length() }
        };
        let (da, db) = (side(pa), side(pb));
        match (da >= 0.0, db >= 0.0) {
            (true, true) => {}
            (false, false) => return None,
            _ => {
                let cross = da / (da - db);
                if da >= 0.0 {
                    t1 = t1.min(cross);
                } else {
                    t0 = t0.max(cross);
                }
            }
        }
    }
    if t1 - t0 <= 1e-12 {
        return None;
    }
    // Depth along the segment, against the triangle's plane there.
    let n = (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0]);
    if n.z.abs() < 1e-15 {
        return None;
    }
    let plane_z = |q: DVec2| t.p[0].z - (n.x * (q.x - t.p[0].x) + n.y * (q.y - t.p[0].y)) / n.z;
    // How far the segment lies behind the plane at a parameter.
    let gap = |s: f64| {
        let q = a + (b - a) * s;
        q.z - plane_z(q.truncate())
    };
    let tol = 1e-6;
    let (g0, g1) = (gap(t0) - tol, gap(t1) - tol);
    match (g0 > 0.0, g1 > 0.0) {
        (true, true) => Some((t0, t1)),
        (false, false) => None,
        (true, false) => Some((t0, t0 + (t1 - t0) * g0 / (g0 - g1))),
        (false, true) => Some((t0 + (t1 - t0) * g0 / (g0 - g1), t1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smith_geom::curve::{Circle, Curve, CurveFrame};
    use smith_model::{DefinitionKind, ModelError};

    fn p(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    /// A box `size` at `at`, drawn as 12 lines in `e`.
    fn cuboid(e: &mut Entities, at: DVec3, size: DVec3) -> Result<(), ModelError> {
        let c = [
            at,
            at + p(size.x, 0.0, 0.0),
            at + p(size.x, size.y, 0.0),
            at + p(0.0, size.y, 0.0),
        ];
        let up = p(0.0, 0.0, size.z);
        for i in 0..4 {
            let (a, b) = (c[i], c[(i + 1) % 4]);
            e.add_line(a, b)?;
            e.add_line(a + up, b + up)?;
            e.add_line(a, a + up)?;
        }
        Ok(())
    }

    fn model(build: impl FnOnce(&mut Document) -> Result<(), ModelError>) -> Document {
        let mut doc = Document::new();
        doc.transaction("build", build).unwrap();
        doc
    }

    fn front() -> DrawingView {
        DrawingView {
            centre: DVec3::ZERO,
            direction: DVec3::Y,
            up: DVec3::Z,
        }
    }

    /// Total length of the pieces matching `visible` and `kind`.
    fn length(d: &Drawing, visible: bool, kind: LineKind) -> f64 {
        d.lines
            .iter()
            .filter(|l| l.visible == visible && l.kind == kind)
            .map(|l| l.a.distance(l.b))
            .sum()
    }

    #[test]
    fn a_cube_from_the_front_hides_its_back() {
        let doc = model(|d| cuboid(d.root_mut(), DVec3::ZERO, DVec3::splat(10.0)));
        let d = draw(&doc, Scope::Model, &front(), None);
        // Seen face-on: the front square (40 mm) shows; the back square lies
        // exactly behind it, hidden; the four edges running back are seen
        // end-on (points), so add nothing.
        assert!((length(&d, true, LineKind::Edge) - 40.0).abs() < 1e-6);
        assert!((length(&d, false, LineKind::Edge) - 40.0).abs() < 1e-6);
        let (lo, hi) = d.bounds().unwrap();
        assert!((hi - lo - DVec2::splat(10.0)).length() < 1e-9);
    }

    #[test]
    fn a_cube_from_a_corner_shows_nine_edges() {
        let doc = model(|d| cuboid(d.root_mut(), DVec3::ZERO, DVec3::splat(10.0)));
        let view = DrawingView {
            centre: p(5.0, 5.0, 5.0),
            direction: p(-1.0, 1.0, -1.0),
            up: DVec3::Z,
        };
        let d = draw(&doc, Scope::Model, &view, None);
        // Each edge is wholly shown or wholly hidden from here.
        assert_eq!(d.lines.iter().filter(|l| l.visible).count(), 9);
        assert_eq!(d.lines.iter().filter(|l| !l.visible).count(), 3);
    }

    #[test]
    fn a_box_behind_a_wall_is_hidden() {
        let doc = model(|d| {
            let r = d.root_mut();
            cuboid(r, p(-20.0, 0.0, -20.0), p(40.0, 1.0, 40.0))?;
            cuboid(r, p(0.0, 10.0, 0.0), DVec3::splat(5.0))
        });
        let d = draw(&doc, Scope::Model, &front(), None);
        // The wall's front square shows; nothing of the small box does.
        assert!((length(&d, true, LineKind::Edge) - 160.0).abs() < 1e-6);
        assert!(
            d.lines
                .iter()
                .filter(|l| l.visible)
                .all(|l| l.a.x.abs() > 19.0 || l.a.y.abs() > 19.0 || l.b.x.abs() > 19.0)
        );
    }

    #[test]
    fn an_edge_on_a_face_stays_shown() {
        let doc = model(|d| {
            let r = d.root_mut();
            cuboid(r, DVec3::ZERO, DVec3::splat(10.0))?;
            // A line drawn across the front face splits it; marked Sharp,
            // as Present mode would otherwise smooth it away.
            for e in r.add_line(p(0.0, 0.0, 5.0), p(10.0, 0.0, 5.0))? {
                r.set_edge_mark(e, smith_model::EdgeMark::Sharp)?;
            }
            Ok(())
        });
        let d = draw(&doc, Scope::Model, &front(), None);
        assert!((length(&d, true, LineKind::Edge) - 50.0).abs() < 1e-6);
    }

    #[test]
    fn a_cylinder_from_the_side_has_its_outlines() {
        let doc = model(|d| {
            let r = d.root_mut();
            let base = CurveFrame::new(DVec3::ZERO, DVec3::X, DVec3::Z)?;
            r.add_curve(Curve::Circle(Circle::new(base, 5.0)?), 24)?;
            let disc = r.face_ids().next().ok_or(ModelError::NoSuchEntity)?;
            let up = r.face(disc).map_or(1.0, |f| f.plane().normal().z.signum());
            smith_ops::modify::extrude(r, disc, 20.0 * up, false)
        });
        let d = draw(&doc, Scope::Model, &front(), None);
        let outlines: Vec<&DrawnLine> = d
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Outline && l.visible)
            .collect();
        // The two sides, each 20 mm, at x = ±5.
        assert_eq!(outlines.len(), 2);
        for l in outlines {
            assert!((l.a.x.abs() - 5.0).abs() < 1e-9);
            assert!((l.a.distance(l.b) - 20.0).abs() < 1e-9);
        }
    }

    #[test]
    fn a_section_cuts_away_the_front() {
        let doc = model(|d| cuboid(d.root_mut(), DVec3::ZERO, DVec3::splat(10.0)));
        // A level cut at half height, removing the top, seen from the front.
        let cut = SectionPlane::new(p(0.0, 0.0, 5.0), DVec3::Z).unwrap();
        let d = draw(&doc, Scope::Model, &front(), Some(&cut));
        let (lo, hi) = d.bounds().unwrap();
        assert!((hi.y - lo.y - 5.0).abs() < 1e-9);
        // The cut runs round the box: its front side shows, the back is
        // behind the faces below it.
        assert!((length(&d, true, LineKind::Cut) - 10.0).abs() < 1e-6);
        assert!((length(&d, false, LineKind::Cut) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn hidden_things_neither_show_nor_hide() {
        let doc = model(|d| {
            let wall = d.create_definition("Wall", DefinitionKind::Group);
            cuboid(
                d.definition_entities_mut(wall)
                    .ok_or(ModelError::NoSuchEntity)?,
                p(-20.0, 0.0, -20.0),
                p(40.0, 1.0, 40.0),
            )?;
            let i = d.add_instance(None, wall, DMat4::IDENTITY, "")?;
            d.root_mut().set_instance_hidden(i, true)?;
            cuboid(d.root_mut(), p(0.0, 10.0, 0.0), DVec3::splat(5.0))
        });
        let d = draw(&doc, Scope::Model, &front(), None);
        assert!((length(&d, true, LineKind::Edge) - 20.0).abs() < 1e-6);
    }

    #[test]
    fn groups_mirrored_and_selection_only() {
        let mut keep = None;
        let doc = model(|d| {
            let def = d.create_definition("Block", DefinitionKind::Component);
            cuboid(
                d.definition_entities_mut(def)
                    .ok_or(ModelError::NoSuchEntity)?,
                DVec3::ZERO,
                DVec3::splat(10.0),
            )?;
            keep = Some(d.add_instance(
                None,
                def,
                DMat4::from_translation(p(-20.0, 0.0, 0.0)) * DMat4::from_scale(p(-1.0, 1.0, 1.0)),
                "",
            )?);
            d.add_instance(None, def, DMat4::from_translation(p(30.0, 0.0, 0.0)), "")?;
            Ok(())
        });
        let all = draw(&doc, Scope::Model, &front(), None);
        assert!((length(&all, true, LineKind::Edge) - 80.0).abs() < 1e-6);
        let picked = [keep.unwrap()];
        let one = draw(
            &doc,
            Scope::Selection {
                context: None,
                local: DMat4::IDENTITY,
                faces: &[],
                instances: &picked,
            },
            &front(),
            None,
        );
        assert!((length(&one, true, LineKind::Edge) - 40.0).abs() < 1e-6);
        let (lo, hi) = one.bounds().unwrap();
        assert!((lo.x + 30.0).abs() < 1e-9 && (hi.x + 20.0).abs() < 1e-9);
    }
}
