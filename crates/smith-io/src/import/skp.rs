//! SketchSmith — SKP import | v1.0.0 | Thursday 12 November 2026
//!
//! A 2013–2020 `.skp` file, read by `smith-skp`, as an [`Imported`]. The
//! file's root is built straight into the file's group; its groups come in as
//! groups and its components as components, each placed as in the file and
//! nested as in the file. Faces keep their holes and their front (the file's
//! normal); every edge keeps its soft, smooth and hidden flags and its layer;
//! and a curve's edges are tied back to the curve: a round arc or circle as
//! one (its segment count the file's edge count), a stretched arc or a plain
//! welded curve as a polyline. Points are the file's own, never redrawn.
//!
//! Paint: each face's front and back keep their material (colour, opacity
//! and the image the file holds; a textured material shows its image
//! untinted), and a side left at the default takes the paint of the group
//! or copy around it, as SketchUp shows it. A component's copies share one
//! definition, so their paint goes on it only when every copy shows the same;
//! otherwise its default sides stay unpainted and the copies carrying paint
//! are counted. Textures are laid by the default frame at the file's repeat
//! size; a texture the file positions on a face is counted as placed
//! approximately. An image the file does not hold, or not PNG or JPEG, is
//! listed as not found (its material keeps its colour).
//!
//! Guides: guide lines and guide points come in with the group or component
//! holding them. A guide line the file ends at one or both ends comes in
//! endless (the model's guide lines have no ends); guides carry no layer.
//!
//! Section planes come in with the group or component holding them, each
//! `a x + b y + c z + d = 0` as a plane through `−d·n` cutting toward
//! `n = (a, b, c)`, none active (the file's active plane is not read). The
//! side cut is taken from the plane as stored, not yet checked against a
//! file; planes keep no name or layer.
//!
//! Text: a leader text comes in as a label tied to what it marks (the
//! vertex there, else the edge it lies on, at its place along it, else the
//! point itself), its label where the file places it; a screen text, or a leader whose
//! point is not found (on a face), as a note (screen text where the file
//! places it). Dimensions and Image entities are left out; images are
//! counted.
//!
//! Units: the file stores inches (From File), or Custom's unit and scale.
//! Layers come in shown, the default layer (`Layer0`, shown as `Untagged`)
//! as Base; hidden faces, groups and copies come in shown.

use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;
use std::path::Path;
use std::sync::Arc as Shared;

use glam::{DMat4, DVec3, DVec4};
use smith_geom::curve::{Arc, Circle, Curve, CurveFrame, MAX_SEGMENTS, Polyline};
use smith_model::{Guide, Material, SectionPlane, Texture};
use smith_skp::{ArcCurve, Entities, SkpError, SkpModel};

use super::{
    EdgeCurve, ImportGroup, ImportOptions, ImportText, Imported, MarkedEdge, Polygon, winding,
};
use crate::error::IoError;

/// Millimetres per inch.
const MM_PER_INCH: f64 = 25.4;

/// How deep groups and components may nest before the rest is left out.
const MAX_NESTING: usize = 64;

/// The default layer's names.
const DEFAULT_LAYERS: [&str; 2] = ["Layer0", "Untagged"];

pub(super) fn read_skp(path: &Path, options: &ImportOptions) -> Result<Imported, IoError> {
    let bytes = std::fs::read(path)?;
    let model = smith_skp::read(&bytes).map_err(|e| match e {
        SkpError::NotSkp => IoError::NotSkp,
        SkpError::Newer => IoError::SkpNewer,
        SkpError::Unreadable(why) => {
            log::warn!("{}: {why}", path.display());
            IoError::SkpUnreadable
        }
    })?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Import".into());
    let k = if options.from_file {
        MM_PER_INCH
    } else {
        options.mm_per_unit()
    };
    Ok(convert(&model, &stem, k))
}

/// `model` as an import named `name`, at `k` mm per file unit.
fn convert(model: &SkpModel, name: &str, k: f64) -> Imported {
    let mut paints = HashMap::new();
    copy_paints(
        model,
        &model.root,
        None,
        0,
        &mut HashSet::new(),
        &mut paints,
    );
    let mut c = Converter {
        model,
        k,
        layers: HashMap::new(),
        imported: Imported {
            name: name.to_string(),
            ..Imported::default()
        },
        components: HashMap::new(),
        building: HashSet::new(),
        materials: HashMap::new(),
        textured: HashSet::new(),
        not_found: HashMap::new(),
        used: HashSet::new(),
        paints,
    };
    for m in &model.materials {
        let i = c.imported.materials.len();
        let (material, missing) = material(m, k);
        if material.texture.is_some() {
            c.textured.insert(m.id);
        }
        if let Some(file) = missing {
            c.not_found.insert(i, file);
        }
        c.materials.insert(m.id, i);
        c.imported.materials.push(material);
    }
    for l in &model.layers {
        if DEFAULT_LAYERS.contains(&l.name.as_str()) {
            continue;
        }
        c.layers.insert(l.id, c.imported.layers.len());
        c.imported
            .layers
            .push((l.name.clone(), [l.color[0], l.color[1], l.color[2]]));
    }
    let mut loose = c.group(&model.root, name, 0, None);
    loose.transform = DMat4::IDENTITY;
    c.imported.loose = loose;
    // Images not found, for the materials used.
    let mut missing: Vec<(usize, String)> = c
        .not_found
        .iter()
        .filter(|(i, _)| c.used.contains(*i))
        .map(|(i, f)| (*i, f.clone()))
        .collect();
    missing.sort();
    for (_, file) in missing {
        if !c.imported.missing.contains(&file) {
            c.imported.missing.push(file);
        }
    }
    c.imported
}

/// The paint a component's copies give its default sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopyPaint {
    /// Every copy shows this (`None`: unpainted).
    One(Option<usize>),
    /// Copies show different paint.
    Mixed,
}

/// Each component definition's [`CopyPaint`] under `e`, whose default sides
/// show `inherit` (a file material id), into `out`.
fn copy_paints(
    model: &SkpModel,
    e: &Entities,
    inherit: Option<usize>,
    depth: usize,
    seen: &mut HashSet<(usize, Option<usize>)>,
    out: &mut HashMap<usize, CopyPaint>,
) {
    if depth >= MAX_NESTING {
        return;
    }
    for inst in &e.instances {
        if inst.image {
            continue;
        }
        let Some(def) = model.definition(inst.definition) else {
            continue;
        };
        let shown = inst.material.or(inherit);
        if !inst.group {
            out.entry(def.id)
                .and_modify(|p| {
                    if *p != CopyPaint::One(shown) {
                        *p = CopyPaint::Mixed;
                    }
                })
                .or_insert(CopyPaint::One(shown));
        }
        if seen.insert((def.id, shown)) {
            copy_paints(model, &def.entities, shown, depth + 1, seen, out);
        }
    }
}

/// Whether `e` (or a group inside it) has a face side at the default.
fn has_default_side(model: &SkpModel, e: &Entities, depth: usize) -> bool {
    e.faces
        .iter()
        .any(|f| f.material.is_none() || f.back_material.is_none())
        || (depth < MAX_NESTING
            && e.instances.iter().any(|i| {
                i.group
                    && !i.image
                    && model
                        .definition(i.definition)
                        .is_some_and(|d| has_default_side(model, &d.entities, depth + 1))
            }))
}

/// A file material as the model's, and the image's name when the file
/// does not hold it as PNG or JPEG.
fn material(m: &smith_skp::Material, k: f64) -> (Material, Option<String>) {
    let mut missing = None;
    let texture = m.texture.as_ref().and_then(|t| {
        let file = t
            .file
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&t.file)
            .to_string();
        let image = t
            .data
            .clone()
            .and_then(|bytes| crate::textures::image_from_bytes(&file, bytes).ok());
        if image.is_none() {
            missing = Some(file);
        }
        let width = t.width * k;
        image.map(|image| Texture {
            image: Shared::new(image),
            width_mm: if width.is_finite() && width > 0.0 {
                width
            } else {
                100.0
            },
        })
    });
    let linear = |b: u8| {
        let c = f32::from(b) / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let color = if texture.is_some() {
        // The image as it is (the file's colour is its average).
        [1.0; 3]
    } else {
        [linear(m.color[0]), linear(m.color[1]), linear(m.color[2])]
    };
    #[allow(clippy::cast_possible_truncation)]
    let opacity = if m.opacity.is_finite() {
        m.opacity.clamp(0.0, 1.0) as f32
    } else {
        1.0
    };
    (
        Material {
            name: m.name.clone(),
            color,
            opacity,
            metallic: false,
            texture,
        },
        missing,
    )
}

struct Converter<'m> {
    model: &'m SkpModel,
    k: f64,
    /// File layer id → index into the import's layers (the default layer
    /// has none).
    layers: HashMap<usize, usize>,
    imported: Imported,
    /// Definition id → index into the import's components.
    components: HashMap<usize, usize>,
    /// Components being made (a file placing one inside itself).
    building: HashSet<usize>,
    /// File material id → index into the import's materials.
    materials: HashMap<usize, usize>,
    /// File material ids with an image.
    textured: HashSet<usize>,
    /// Import material index → its image's name, not found.
    not_found: HashMap<usize, String>,
    /// Import material indices used.
    used: HashSet<usize>,
    /// Component definition id → what its copies give its default sides.
    paints: HashMap<usize, CopyPaint>,
}

impl Converter<'_> {
    fn layer(&self, id: Option<usize>) -> Option<usize> {
        id.and_then(|i| self.layers.get(&i).copied())
    }

    fn point(&self, p: [f64; 3]) -> DVec3 {
        DVec3::from_array(p) * self.k
    }

    /// The import's material for file material `id`, marked used.
    fn paint(&mut self, id: Option<usize>) -> Option<usize> {
        let i = self.materials.get(&id?).copied()?;
        self.used.insert(i);
        Some(i)
    }

    /// One definition's (or the root's) entities as a group named `name`;
    /// sides at the default show `inherit` (a file material id).
    fn group(
        &mut self,
        e: &Entities,
        name: &str,
        depth: usize,
        inherit: Option<usize>,
    ) -> ImportGroup {
        let mut g = ImportGroup {
            name: name.to_string(),
            ..ImportGroup::default()
        };
        let vertices: HashMap<usize, DVec3> = e
            .vertices
            .iter()
            .map(|v| (v.id, self.point(v.position)))
            .collect();
        let ends: HashMap<usize, [DVec3; 2]> = e
            .edges
            .iter()
            .filter_map(|x| {
                let a = vertices.get(&x.start?)?;
                let b = vertices.get(&x.end?)?;
                Some((x.id, [*a, *b]))
            })
            .collect();

        for f in &e.faces {
            let loop_points = |l: &[smith_skp::CoEdge]| -> Option<Vec<DVec3>> {
                l.iter()
                    .map(|u| {
                        ends.get(&u.edge)
                            .map(|[a, b]| if u.reversed { *b } else { *a })
                    })
                    .collect()
            };
            let Some(mut points) = f.loops.first().and_then(|l| loop_points(l)) else {
                continue;
            };
            let mut holes: Vec<Vec<DVec3>> = f.loops[1..]
                .iter()
                .filter_map(|l| loop_points(l))
                .filter(|h| h.len() >= 3)
                .collect();
            // The file's normal is the front.
            if winding(&points).dot(DVec3::from_array(f.normal)) < 0.0 {
                points.reverse();
                for h in &mut holes {
                    h.reverse();
                }
            }
            // A texture the file positions is laid by the default frame.
            for (own, placed) in [(f.material, f.uv_front), (f.back_material, f.uv_back)] {
                if placed.is_some() && own.is_some_and(|m| self.textured.contains(&m)) {
                    self.imported.approximate += 1;
                }
            }
            let material = self.paint(f.material.or(inherit));
            let back_material = self.paint(f.back_material.or(inherit));
            g.polygons.push(Polygon {
                points,
                holes,
                layer: self.layer(f.layer),
                material,
                back_material,
                ..Polygon::default()
            });
        }

        for x in &e.edges {
            if let Some(ends) = ends.get(&x.id) {
                g.edges.push(MarkedEdge {
                    ends: *ends,
                    soft: x.soft,
                    smooth: x.smooth,
                    hidden: x.hidden,
                    layer: self.layer(x.layer),
                });
            }
        }

        for l in &e.guide_lines {
            if let Some(guide) = Guide::line(self.point(l.point), DVec3::from_array(l.direction)) {
                g.guides.push(guide);
            }
        }
        for p in &e.guide_points {
            if let Some(guide) = Guide::point(self.point(p.position)) {
                g.guides.push(guide);
            }
        }

        for t in &e.texts {
            if let Some(it) = import_text(t, self.k) {
                g.texts.push(it);
            }
        }
        for s in &e.section_planes {
            if let Some(plane) = section(s.plane, self.k) {
                g.sections.push(plane);
            }
        }

        // Curves, in the order their first edge comes.
        let mut order: Vec<usize> = Vec::new();
        let mut members: HashMap<usize, Vec<[DVec3; 2]>> = HashMap::new();
        for x in &e.edges {
            let (Some(c), Some(ends)) = (x.curve, ends.get(&x.id)) else {
                continue;
            };
            members
                .entry(c)
                .or_insert_with(|| {
                    order.push(c);
                    Vec::new()
                })
                .push(*ends);
        }
        for c in order {
            let segs = &members[&c];
            if let Some((curve, segments)) = edge_curve(e.arcs.get(&c), segs, self.k) {
                g.curves.push(EdgeCurve {
                    curve,
                    segments,
                    edges: segs.clone(),
                });
            }
        }

        if depth >= MAX_NESTING {
            return g;
        }
        for inst in &e.instances {
            if inst.image {
                self.imported.images += 1;
                continue;
            }
            let Some(def) = self.model.definition(inst.definition) else {
                continue;
            };
            let Some(transform) = placement(&inst.transform, self.k) else {
                continue;
            };
            let label = if inst.name.is_empty() {
                def.name.clone()
            } else {
                inst.name.clone()
            };
            let layer = self.layer(inst.layer);
            let shown = inst.material.or(inherit);
            if inst.group {
                let mut child = self.group(&def.entities, &label, depth + 1, shown);
                child.transform = transform;
                child.layer = layer;
                g.children.push(child);
            } else if let Some(i) = self.component(def.id, depth) {
                if shown.is_some()
                    && self.paints.get(&def.id) == Some(&CopyPaint::Mixed)
                    && has_default_side(self.model, &def.entities, 0)
                {
                    self.imported.copy_paint += 1;
                }
                g.uses.push((i, transform, inst.name.clone(), layer));
            }
        }
        g
    }

    /// The component made from definition `id`, made the first time.
    fn component(&mut self, id: usize, depth: usize) -> Option<usize> {
        if let Some(i) = self.components.get(&id) {
            return Some(*i);
        }
        if !self.building.insert(id) {
            return None;
        }
        let model = self.model;
        let def = model.definition(id)?;
        let inherit = match self.paints.get(&id) {
            Some(CopyPaint::One(p)) => *p,
            _ => None,
        };
        let g = self.group(&def.entities, &def.name, depth + 1, inherit);
        self.building.remove(&id);
        let i = self.imported.components.len();
        self.imported.components.push(g);
        self.components.insert(id, i);
        Some(i)
    }
}

/// A file text as a label or a note, at `k` mm per file unit; `None` for
/// one with nothing to say.
fn import_text(t: &smith_skp::Text, k: f64) -> Option<ImportText> {
    if t.text.trim().is_empty() {
        return None;
    }
    let text = t.text.clone();
    Some(match t.point {
        Some(p) => ImportText::Leader {
            anchor: DVec3::from_array(p) * k,
            // Where the file places the label (an inch up when it does not).
            label: t.label.map_or(DVec3::Z, DVec3::from_array) * k,
            text,
        },
        None => ImportText::Screen {
            at: t
                .screen
                .filter(|s| s.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c))),
            text,
        },
    })
}

/// The file's plane `a x + b y + c z + d = 0` (inches) as a section plane
/// in mm, through `−d·n / |n|²` cutting toward `n = (a, b, c)`. `None` when
/// it is degenerate.
fn section(plane: [f64; 4], k: f64) -> Option<SectionPlane> {
    let n = DVec3::new(plane[0], plane[1], plane[2]);
    let len2 = n.length_squared();
    if !(len2.is_finite() && len2 > 1e-24 && plane[3].is_finite()) {
        return None;
    }
    SectionPlane::new(n * (-plane[3] / len2) * k, n)
}

/// An instance's placement in mm: the 3 × 3 part and offset, with the
/// uniform scale folded in. `None` when it is not finite.
fn placement(m: &[f64; 13], k: f64) -> Option<DMat4> {
    let s = if m[12].is_finite() && m[12] != 0.0 {
        m[12]
    } else {
        1.0
    };
    let r = |i: usize| m[i] * s;
    let t = DMat4::from_cols(
        DVec4::new(r(0), r(3), r(6), 0.0),
        DVec4::new(r(1), r(4), r(7), 0.0),
        DVec4::new(r(2), r(5), r(8), 0.0),
        DVec4::new(r(9) * k, r(10) * k, r(11) * k, 1.0),
    );
    t.is_finite().then_some(t)
}

/// The curve a set of edges was drawn from, with its segment count: a
/// round `arc` as an arc or circle when every edge end lies on it; else
/// the edges chained in order as a polyline. `None` when they do not chain
/// (they stay plain edges).
fn edge_curve(arc: Option<&ArcCurve>, segs: &[[DVec3; 2]], k: f64) -> Option<(Curve, u32)> {
    let n = u32::try_from(segs.len()).ok()?;
    if n == 0 || n > MAX_SEGMENTS {
        return None;
    }
    if let Some(a) = arc
        && let Some(c) = round_curve(a, segs, k)
    {
        return Some((c, n));
    }
    let (points, closed) = chain(segs)?;
    let line = Polyline::new(points, closed).ok()?;
    Some((Curve::Polyline(line), n))
}

/// A round arc or circle from the file's frame, when its axes are equal and
/// square and every edge end lies on it.
fn round_curve(a: &ArcCurve, segs: &[[DVec3; 2]], k: f64) -> Option<Curve> {
    let center = DVec3::from_array(a.center) * k;
    let x = DVec3::from_array(a.x_axis) * k;
    let y = DVec3::from_array(a.y_axis) * k;
    let (rx, ry) = (x.length(), y.length());
    let tol = 1e-6 * rx.max(1.0);
    if (rx - ry).abs() > tol || x.dot(y).abs() > tol * rx.max(ry) {
        return None;
    }
    let normal = x.cross(y).normalize_or_zero();
    let frame = CurveFrame::new(center, x, normal).ok()?;
    // Every edge end on the circle and in its plane.
    let near = 1e-3 * rx.max(1.0);
    let on = |p: DVec3| {
        let d = p - center;
        (d.length() - rx).abs() <= near && d.dot(normal).abs() <= near
    };
    if !segs.iter().flatten().all(|p| on(*p)) {
        return None;
    }
    if (a.end - a.start).abs() < 1e-12 {
        return Circle::new(frame, rx).ok().map(Curve::Circle);
    }
    let mut sweep = a.end - a.start;
    while sweep <= 0.0 {
        sweep += TAU;
    }
    Arc::new(frame, rx, a.start, a.start + sweep.min(TAU))
        .ok()
        .map(Curve::Arc)
}

/// The edges' ends in order along them, and whether they close; `None`
/// when they branch or fall apart.
fn chain(segs: &[[DVec3; 2]]) -> Option<(Vec<DVec3>, bool)> {
    let key = |p: DVec3| {
        let q = (p / smith_geom::tolerance::TOL_MERGE).round();
        (q.x as i64, q.y as i64, q.z as i64)
    };
    let mut at: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        for p in s {
            at.entry(key(*p)).or_default().push(i);
        }
    }
    if at.values().any(|v| v.len() > 2) {
        return None;
    }
    // Start at an open end, if any.
    let start = segs
        .iter()
        .flatten()
        .copied()
        .find(|p| at[&key(*p)].len() == 1);
    let closed = start.is_none();
    let mut cur = start.unwrap_or(segs[0][0]);
    let mut used = vec![false; segs.len()];
    let mut points = vec![cur];
    for _ in 0..segs.len() {
        let next = at[&key(cur)].iter().copied().find(|i| !used[*i])?;
        used[next] = true;
        let [a, b] = segs[next];
        cur = if key(a) == key(cur) { b } else { a };
        points.push(cur);
    }
    if !used.iter().all(|u| *u) {
        return None;
    }
    if closed {
        points.pop();
    }
    Some((points, closed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_chain_in_order_open_or_closed() {
        let p = |x: f64, y: f64| DVec3::new(x, y, 0.0);
        let open = [[p(1.0, 0.0), p(2.0, 0.0)], [p(0.0, 0.0), p(1.0, 0.0)]];
        let (pts, closed) = chain(&open).unwrap();
        assert!(!closed);
        assert_eq!(pts.len(), 3);
        assert!(pts[0] == p(0.0, 0.0) || pts[0] == p(2.0, 0.0));
        let tri = [
            [p(0.0, 0.0), p(1.0, 0.0)],
            [p(1.0, 0.0), p(0.0, 1.0)],
            [p(0.0, 1.0), p(0.0, 0.0)],
        ];
        let (pts, closed) = chain(&tri).unwrap();
        assert!(closed);
        assert_eq!(pts.len(), 3);
        let branch = [
            [p(0.0, 0.0), p(1.0, 0.0)],
            [p(0.0, 0.0), p(0.0, 1.0)],
            [p(0.0, 0.0), p(-1.0, 0.0)],
        ];
        assert!(chain(&branch).is_none());
    }

    #[test]
    fn a_round_arc_is_an_arc_and_a_stretched_one_a_polyline() {
        let quarter: Vec<[DVec3; 2]> = (0..4)
            .map(|i| {
                let at = |j: u32| {
                    let t = std::f64::consts::FRAC_PI_2 * f64::from(j) / 4.0;
                    DVec3::new(t.cos(), t.sin(), 0.0) * 10.0 * MM_PER_INCH
                };
                [at(i), at(i + 1)]
            })
            .collect();
        let round = ArcCurve {
            center: [0.0; 3],
            x_axis: [10.0, 0.0, 0.0],
            y_axis: [0.0, 10.0, 0.0],
            start: 0.0,
            end: std::f64::consts::FRAC_PI_2,
        };
        let (c, n) = edge_curve(Some(&round), &quarter, MM_PER_INCH).unwrap();
        assert_eq!(n, 4);
        assert!(matches!(c, Curve::Arc(_)));
        let stretched = ArcCurve {
            y_axis: [0.0, 5.0, 0.0],
            ..round
        };
        let (c, _) = edge_curve(Some(&stretched), &quarter, MM_PER_INCH).unwrap();
        assert!(matches!(c, Curve::Polyline(_)));
    }

    #[test]
    fn a_placement_folds_its_scale_and_offset_into_mm() {
        let mut m = [0.0; 13];
        // A quarter turn about Z, moved 1 inch along X, scale 2.
        m[1] = -1.0;
        m[3] = 1.0;
        m[8] = 1.0;
        m[9] = 1.0;
        m[12] = 2.0;
        let t = placement(&m, MM_PER_INCH).unwrap();
        let p = t.transform_point3(DVec3::new(1.0, 0.0, 0.0));
        assert!((p - DVec3::new(2.0 * MM_PER_INCH, 2.0, 0.0)).length() < 1e-9);
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../smith-skp/tests/fixtures")
            .join(name)
    }

    fn import(name: &str) -> (smith_model::Document, super::super::ImportReport) {
        let options = ImportOptions::for_format(super::super::ImportFormat::Skp);
        let imported =
            super::super::read_import(&fixture(name), super::super::ImportFormat::Skp, &options)
                .unwrap();
        let mut doc = smith_model::Document::new();
        let (_, report) = doc
            .transaction("Import", |d| {
                super::super::place(d, None, DMat4::IDENTITY, &imported)
            })
            .unwrap();
        (doc, report)
    }

    #[test]
    fn a_2020_model_comes_in_at_true_size_with_its_layers_and_curves() {
        let (doc, report) = import("gondola_v20.skp");
        // 23 groups and copies at the root, faces from every one.
        assert_eq!(report.groups, 23);
        assert!(report.counts.faces > 1000);
        // A shop gondola about 3.82 m by 4.82 m, 3.14 m tall, as its faces
        // show (12 hidden edges at the file's origin lie well away).
        let corners: Vec<DVec3> = crate::export::gather(&doc, crate::export::Scope::Model)
            .parts
            .iter()
            .flat_map(|p| &p.faces)
            .flat_map(|f| f.corners.iter().map(|c| c.point))
            .collect();
        let lo = corners.iter().fold(DVec3::MAX, |a, b| a.min(*b));
        let hi = corners.iter().fold(DVec3::MIN, |a, b| a.max(*b));
        let size = hi - lo;
        assert!((size.x - 3820.0).abs() < 100.0, "{size}");
        assert!((size.y - 4820.0).abs() < 100.0, "{size}");
        assert!((size.z - 3140.0).abs() < 100.0, "{size}");
        assert!(
            doc.layer_ids()
                .any(|l| doc.layer(l).is_some_and(|x| x.name == "Gondulas Laterais"))
        );
        assert!(
            doc.layer_ids()
                .all(|l| doc.layer(l).is_some_and(|x| x.visible))
        );
        let curves: usize = doc
            .definition_ids()
            .filter_map(|d| doc.definition_entities(d))
            .map(|e| e.curve_count())
            .sum();
        assert!(curves > 0);
        for d in doc.definition_ids() {
            doc.definition_entities(d)
                .unwrap()
                .validate_links()
                .unwrap();
        }
    }

    #[test]
    fn groups_components_and_layers_come_in_as_placed() {
        let (doc, report) = import("legacy_v2019_synthetic.skp");
        assert_eq!(report.groups, 10);
        assert!(
            doc.layer_ids()
                .any(|l| doc.layer(l).is_some_and(|x| x.name == "Roof"))
        );
        // The default layer is Base: no layer named after it.
        assert!(doc.layer_ids().all(|l| {
            doc.layer(l)
                .is_some_and(|x| !DEFAULT_LAYERS.contains(&x.name.as_str()))
        }));
        // Panel holds InnerFrame.
        let named = |n: &str| {
            doc.definition_ids()
                .find(|d| doc.definition(*d).is_some_and(|x| x.name == n))
        };
        let panel = named("Panel").unwrap();
        let inner = named("InnerFrame").unwrap();
        let e = doc.definition_entities(panel).unwrap();
        assert!(
            e.instance_ids()
                .any(|i| e.instance(i).is_some_and(|x| x.definition == inner))
        );
    }

    /// Sides painted, front and back, through every definition.
    fn painted(doc: &smith_model::Document, side: smith_model::Side) -> Vec<String> {
        let mut out = Vec::new();
        for d in doc.definition_ids() {
            let e = doc.definition_entities(d).unwrap();
            for f in e.face_ids() {
                if let Some(p) = e.face_paint(f, side) {
                    out.push(doc.material(p.material).unwrap().name.clone());
                }
            }
        }
        out
    }

    #[test]
    fn a_textured_face_keeps_its_image_at_the_files_repeat_size() {
        let (doc, report) = import("legacy_v2020_textured.skp");
        assert_eq!(
            painted(&doc, smith_model::Side::Front),
            vec!["Checker".to_string()]
        );
        let id = doc
            .definition_ids()
            .find_map(|d| {
                let e = doc.definition_entities(d)?;
                e.face_ids()
                    .find_map(|f| e.face_paint(f, smith_model::Side::Front))
                    .map(|p| p.material)
            })
            .unwrap();
        let m = doc.material(id).unwrap();
        let t = m.texture.as_ref().unwrap();
        assert_eq!(t.image.format, smith_model::ImageFormat::Png);
        assert_eq!(t.image.name, "checker.png");
        // 10 inches a repeat.
        assert!((t.width_mm - 254.0).abs() < 1e-9, "{}", t.width_mm);
        // The image untinted.
        assert_eq!(m.color, [1.0; 3]);
        assert!(report.lines().is_empty(), "{:?}", report.lines());
    }

    #[test]
    fn a_2020_model_keeps_its_paint_front_and_back_and_its_glass() {
        let (doc, report) = import("gondola_v20.skp");
        let fronts = painted(&doc, smith_model::Side::Front);
        let backs = painted(&doc, smith_model::Side::Back);
        assert!(fronts.len() > 1000, "{}", fronts.len());
        assert!(!backs.is_empty());
        let glass = doc
            .material_ids()
            .filter_map(|m| doc.material(m))
            .find(|m| m.name == "Translucent_Glass_Blue")
            .unwrap();
        assert!((glass.opacity - 0.3).abs() < 1e-6);
        // sRGB (196, 242, 255) as linear.
        assert!((glass.color[2] - 1.0).abs() < 1e-6);
        assert!((glass.color[0] - 0.552).abs() < 1e-3, "{:?}", glass.color);
        // Its materials hold no images: none listed as not found.
        assert!(report.missing.is_empty());
    }

    const IDENTITY: [f64; 13] = [
        1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
    ];

    /// A 1-inch square at height `z` with its front and back material ids.
    fn square(e: &mut Entities, z: f64, front: Option<usize>, back: Option<usize>) {
        let base = e.vertices.len() + e.edges.len() + e.faces.len() + 1000 * (e.faces.len() + 1);
        let corners = [[0.0, 0.0, z], [1.0, 0.0, z], [1.0, 1.0, z], [0.0, 1.0, z]];
        let mut l = Vec::new();
        for (i, c) in corners.iter().enumerate() {
            e.vertices.push(smith_skp::Vertex {
                id: base + i,
                position: *c,
            });
        }
        for i in 0..4 {
            e.edges.push(smith_skp::Edge {
                id: base + 10 + i,
                start: Some(base + i),
                end: Some(base + (i + 1) % 4),
                soft: false,
                smooth: false,
                hidden: false,
                layer: None,
                curve: None,
            });
            l.push(smith_skp::CoEdge {
                edge: base + 10 + i,
                reversed: false,
            });
        }
        e.faces.push(smith_skp::Face {
            id: base + 20,
            loops: vec![l],
            normal: [0.0, 0.0, 1.0],
            material: front,
            back_material: back,
            hidden: false,
            layer: None,
            uv_front: None,
            uv_back: None,
        });
    }

    fn colour(id: usize, name: &str) -> smith_skp::Material {
        smith_skp::Material {
            id,
            name: name.into(),
            color: [255, 0, 0, 255],
            opacity: 1.0,
            texture: None,
        }
    }

    fn place_of(definition: usize, group: bool, material: Option<usize>) -> smith_skp::Instance {
        smith_skp::Instance {
            definition,
            group,
            image: false,
            name: String::new(),
            transform: IDENTITY,
            material,
            layer: None,
            hidden: false,
        }
    }

    fn definition(id: usize, entities: Entities) -> smith_skp::Definition {
        smith_skp::Definition {
            id,
            name: format!("D{id}"),
            guid: String::new(),
            image: false,
            entities,
        }
    }

    #[test]
    fn a_painted_group_paints_its_default_sides_and_a_face_keeps_its_own() {
        let mut inside = Entities::default();
        square(&mut inside, 0.0, None, None);
        square(&mut inside, 2.0, Some(2), None);
        let mut model = SkpModel {
            materials: vec![colour(1, "Group Paint"), colour(2, "Own Paint")],
            definitions: vec![definition(100, inside)],
            ..SkpModel::default()
        };
        model.root.instances.push(place_of(100, true, Some(1)));
        let imported = convert(&model, "t", MM_PER_INCH);
        let polys = &imported.loose.children[0].polygons;
        let name = |m: Option<usize>| m.map(|i| imported.materials[i].name.clone());
        assert_eq!(name(polys[0].material).as_deref(), Some("Group Paint"));
        assert_eq!(name(polys[0].back_material).as_deref(), Some("Group Paint"));
        assert_eq!(name(polys[1].material).as_deref(), Some("Own Paint"));
        assert_eq!(name(polys[1].back_material).as_deref(), Some("Group Paint"));
        assert_eq!(imported.copy_paint, 0);
    }

    #[test]
    fn copies_painted_alike_paint_the_definition_and_unlike_are_counted() {
        let mut inside = Entities::default();
        square(&mut inside, 0.0, None, None);
        let mut model = SkpModel {
            materials: vec![colour(1, "A"), colour(2, "B")],
            definitions: vec![definition(200, inside)],
            ..SkpModel::default()
        };
        model.root.instances.push(place_of(200, false, Some(1)));
        model.root.instances.push(place_of(200, false, Some(1)));
        let alike = convert(&model, "t", MM_PER_INCH);
        let m = alike.components[0].polygons[0].material.unwrap();
        assert_eq!(alike.materials[m].name, "A");
        assert_eq!(alike.copy_paint, 0);

        model.root.instances[1].material = Some(2);
        let unlike = convert(&model, "t", MM_PER_INCH);
        assert_eq!(unlike.components[0].polygons[0].material, None);
        assert_eq!(unlike.copy_paint, 2);
        let report = super::super::ImportReport {
            copy_paint: unlike.copy_paint,
            ..super::super::ImportReport::default()
        };
        assert_eq!(report.lines(), vec!["2 Component Copies' Paint Skipped"]);
    }

    #[test]
    fn a_texture_the_file_does_not_hold_is_listed_when_used() {
        let mut inside = Entities::default();
        square(&mut inside, 0.0, Some(1), None);
        let mut textured = colour(1, "Brick");
        textured.texture = Some(smith_skp::Texture {
            file: "C:\\maps\\brick.bmp".into(),
            width: 10.0,
            height: 10.0,
            data: None,
        });
        let mut unused = colour(2, "Stone");
        unused.texture = Some(smith_skp::Texture {
            file: "stone.jpg".into(),
            width: 10.0,
            height: 10.0,
            data: None,
        });
        let model = SkpModel {
            materials: vec![textured, unused],
            root: inside,
            ..SkpModel::default()
        };
        let imported = convert(&model, "t", MM_PER_INCH);
        assert_eq!(imported.missing, vec!["brick.bmp".to_string()]);
        // It keeps its colour.
        let m = &imported.materials[imported.loose.polygons[0].material.unwrap()];
        assert!(m.texture.is_none());
        assert_eq!(m.color, [1.0, 0.0, 0.0]);
    }

    /// Every guide in the model, through every definition.
    fn guides(doc: &smith_model::Document) -> Vec<Guide> {
        doc.definition_ids()
            .filter_map(|d| doc.definition_entities(d))
            .flat_map(|e| e.guides().map(|(_, g)| *g).collect::<Vec<_>>())
            .collect()
    }

    #[test]
    fn guide_lines_come_in_endless_at_true_size() {
        let (doc, _) = import("zero_material_custom_layer_v18.skp");
        let all = guides(&doc);
        assert_eq!(all.len(), 2);
        // Both run at 45° in the XZ plane; one passes 800 mm up (31.5 in).
        for g in &all {
            let Guide::Line { direction, .. } = g else {
                panic!("not a line: {g:?}");
            };
            assert!((direction.x.abs() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9);
            assert!(direction.y.abs() < 1e-9);
        }
        assert!(all.iter().any(|g| matches!(
            g,
            Guide::Line { point, .. } if (point.z - 31.496_062_992_125_974 * MM_PER_INCH).abs() < 1e-6
        )));
    }

    #[test]
    fn guide_lines_and_points_come_in_with_their_groups() {
        let mut inside = Entities::default();
        inside.guide_lines.push(smith_skp::GuideLine {
            point: [1.0, 0.0, 0.0],
            direction: [0.0, 0.0, 1.0],
            start: None,
            end: None,
            layer: None,
        });
        inside.guide_points.push(smith_skp::GuidePoint {
            position: [0.0, 2.0, 0.0],
            layer: None,
        });
        let mut model = SkpModel {
            definitions: vec![definition(300, inside)],
            ..SkpModel::default()
        };
        model.root.guide_points.push(smith_skp::GuidePoint {
            position: [0.0, 0.0, 1.0],
            layer: None,
        });
        model.root.instances.push(place_of(300, true, None));
        let imported = convert(&model, "t", MM_PER_INCH);
        assert_eq!(
            imported.loose.guides,
            vec![Guide::point(DVec3::new(0.0, 0.0, MM_PER_INCH)).unwrap()]
        );
        assert_eq!(
            imported.loose.children[0].guides,
            vec![
                Guide::line(DVec3::new(MM_PER_INCH, 0.0, 0.0), DVec3::Z).unwrap(),
                Guide::point(DVec3::new(0.0, 2.0 * MM_PER_INCH, 0.0)).unwrap(),
            ]
        );
    }

    #[test]
    fn a_section_plane_is_placed_at_true_size_facing_as_stored() {
        // z = 2 in, stored as 0x + 0y − 2z + 4 = 0 (n not unit, facing down).
        let s = section([0.0, 0.0, -2.0, 4.0], MM_PER_INCH).unwrap();
        assert!((s.point - DVec3::new(0.0, 0.0, 2.0 * MM_PER_INCH)).length() < 1e-9);
        assert!((s.normal - DVec3::NEG_Z).length() < 1e-12);
        assert!(!s.active);
        assert!(section([0.0, 0.0, 0.0, 1.0], MM_PER_INCH).is_none());
        assert!(section([0.0, 0.0, 1.0, f64::NAN], MM_PER_INCH).is_none());
    }

    #[test]
    fn section_planes_come_in_with_their_groups_none_active() {
        let plane = |z: f64| smith_skp::SectionPlane {
            plane: [0.0, 0.0, 1.0, -z],
            name: String::new(),
            layer: None,
        };
        let mut inside = Entities::default();
        square(&mut inside, 0.0, None, None);
        inside.section_planes.push(plane(1.0));
        let mut model = SkpModel {
            definitions: vec![definition(400, inside)],
            ..SkpModel::default()
        };
        square(&mut model.root, 0.0, None, None);
        model.root.section_planes.push(plane(0.5));
        model.root.section_planes.push(plane(3.0));
        model.root.instances.push(place_of(400, true, None));
        let imported = convert(&model, "t", MM_PER_INCH);
        assert_eq!(imported.loose.sections.len(), 2);
        assert_eq!(imported.loose.children[0].sections.len(), 1);

        let mut doc = smith_model::Document::new();
        doc.transaction("Import", |d| {
            super::super::place(d, None, DMat4::IDENTITY, &imported)
        })
        .unwrap();
        let mut planes = 0;
        for d in doc.definition_ids() {
            let e = doc.definition_entities(d).unwrap();
            planes += e.sections().count();
            assert!(e.active_section().is_none());
        }
        assert_eq!(planes, 3);
    }

    fn note(s: &str, point: Option<[f64; 3]>, screen: Option<[f64; 2]>) -> smith_skp::Text {
        smith_skp::Text {
            text: s.into(),
            point,
            label: point.map(|_| [0.0, 0.0, 1.0]),
            screen,
            layer: None,
        }
    }

    #[test]
    fn labels_tie_to_their_parts_and_notes_keep_their_place() {
        let mut inside = Entities::default();
        square(&mut inside, 0.0, None, None);
        // On the square's corner, and in the air.
        inside
            .texts
            .push(note("Bracket A", Some([1.0, 1.0, 0.0]), None));
        inside
            .texts
            .push(note("6 mm Plate", Some([5.0, 5.0, 5.0]), None));
        // Halfway along the square's first edge.
        inside.texts.push(note("Rail", Some([0.5, 0.0, 0.0]), None));
        let mut model = SkpModel {
            definitions: vec![definition(500, inside)],
            ..SkpModel::default()
        };
        model
            .root
            .texts
            .push(note("Steel, Brushed", None, Some([0.6, 0.25])));
        model.root.texts.push(note("Qty 4", None, None));
        model.root.texts.push(note("  ", None, None));
        model.root.instances.push(place_of(500, true, None));
        let imported = convert(&model, "t", MM_PER_INCH);
        assert_eq!(imported.loose.texts.len(), 2);

        let mut doc = smith_model::Document::new();
        doc.transaction("Import", |d| {
            super::super::place(d, None, DMat4::IDENTITY, &imported)
        })
        .unwrap();
        let mut leaders = Vec::new();
        let mut notes = Vec::new();
        for d in doc.definition_ids() {
            let e = doc.definition_entities(d).unwrap();
            for (_, a) in e.annotations() {
                match a {
                    smith_model::Annotation::Leader {
                        anchor,
                        label,
                        text: smith_model::LeaderText::Custom(t),
                    } => leaders.push((t.clone(), *anchor, *label)),
                    smith_model::Annotation::Screen { at, text } => {
                        notes.push((text.clone(), *at));
                    }
                    other => panic!("unexpected {other:?}"),
                }
            }
        }
        leaders.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(leaders.len(), 3);
        let (name, anchor, _) = &leaders[2];
        assert_eq!(name, "Rail");
        assert!(matches!(anchor, smith_model::Anchor::Edge(_, t, _) if (t - 0.5).abs() < 1e-9));
        let (name, anchor, label) = &leaders[1];
        assert_eq!(name, "Bracket A");
        assert!(matches!(anchor, smith_model::Anchor::Vertex(..)));
        // One inch out along the leader.
        assert!((*label - DVec3::new(0.0, 0.0, MM_PER_INCH)).length() < 1e-9);
        assert!(matches!(leaders[0].1, smith_model::Anchor::Point(_)));
        notes.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].0, "Qty 4");
        assert_eq!(notes[1], ("Steel, Brushed".to_string(), [0.6, 0.25]));
    }

    #[test]
    fn image_entities_are_counted_as_skipped() {
        let mut model = SkpModel {
            definitions: vec![definition(600, Entities::default())],
            ..SkpModel::default()
        };
        let mut picture = place_of(600, false, None);
        picture.image = true;
        model.root.instances.push(picture);
        let imported = convert(&model, "t", MM_PER_INCH);
        assert_eq!(imported.images, 1);
    }

    #[test]
    fn newer_and_foreign_files_are_refused_with_their_messages() {
        let options = ImportOptions::for_format(super::super::ImportFormat::Skp);
        let e = read_skp(&fixture("instance_scale_term_2021.skp"), &options).unwrap_err();
        assert_eq!(e.to_string(), "SKP 2021 or Later Not Read Yet");
        let dir = std::env::temp_dir().join(format!("smith-io-skp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let not = dir.join("box.skp");
        std::fs::write(&not, b"solid box\nendsolid box\n").unwrap();
        assert_eq!(
            read_skp(&not, &options).unwrap_err().to_string(),
            "Not an SKP File"
        );
        let mut cut = std::fs::read(fixture("legacy_v2019_synthetic.skp")).unwrap();
        cut.truncate(cut.len() / 2);
        let half = dir.join("half.skp");
        std::fs::write(&half, cut).unwrap();
        assert_eq!(
            read_skp(&half, &options).unwrap_err().to_string(),
            "Couldn't Read This SKP File"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
