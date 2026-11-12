//! SketchSmith — 3MF reading | v1.0.0 | Thursday 12 November 2026
//!
//! A core 3MF package, as slicers and our own export write it: a zip
//! holding `_rels/.rels`, which names the model part (`3D/3dmodel.model`
//! when it names none), and that XML model.
//!
//! * Units: the model's `unit` (micron, millimeter, centimeter, inch, foot,
//!   meter; millimetre when absent) with From File, else the window's
//!   choice. Z up, as the specification sets out.
//! * Each build item is a group named after its object (`Object 3` when it
//!   has no name), placed by the item's transform. An object of triangles
//!   is that group's faces; an object of components holds a group per
//!   component, placed by the component's transform. An object used more
//!   than once (by items or components) comes in as one component, copied
//!   where each use places it.
//! * Colour: a triangle's `pid`/`p1` (else its object's `pid`/`pindex`)
//!   picks a base material (its name and colour) or a colour group's colour,
//!   sRGB with alpha as opacity; one material per colour used. Per-corner
//!   colours (`p2`, `p3`) are not read: the triangle takes its first.
//!   Textures (the Materials extension's) are not read.
//! * Then Merge Coplanar Faces and Soften Curved Surfaces, as for STL.
//!
//! The zip is read here: stored or deflated entries, no ZIP64 (a part past
//! 4 GiB is refused), no encryption.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;

use glam::{DMat4, DVec3, DVec4};
use smith_model::Material;

use super::{ImportGroup, ImportOptions, Imported, Polygon};
use crate::error::IoError;
use crate::xml::{Element, parse, text_of};

/// Deepest nesting of components followed (a cycle stops here too).
const MAX_DEPTH: usize = 32;

/// Reads the 3MF `path`.
pub(super) fn read_3mf(path: &Path, options: &ImportOptions) -> Result<Imported, IoError> {
    let bytes = std::fs::read(path)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Import".into());
    let entries = zip_entries(&bytes)?;
    let model_path = entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case("_rels/.rels"))
        .and_then(|e| zip_read(&bytes, e).ok())
        .and_then(|rels| model_target(&text_of(&rels)))
        .unwrap_or_else(|| "3D/3dmodel.model".into());
    let want = model_path.trim_start_matches('/');
    let entry = entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(want))
        .ok_or_else(|| IoError::Damaged("the package has no 3D model part".into()))?;
    let root = parse(&text_of(&zip_read(&bytes, entry)?))?;
    let model = if root.name == "model" {
        &root
    } else {
        root.child("model")
            .ok_or_else(|| IoError::Damaged("the 3D model part has no model".into()))?
    };
    let unit_mm = match model.attr("unit").unwrap_or("millimeter") {
        "micron" => 0.001,
        "centimeter" => 10.0,
        "inch" => 25.4,
        "foot" => 304.8,
        "meter" => 1000.0,
        _ => 1.0,
    };
    let k = if options.from_file {
        unit_mm
    } else {
        options.mm_per_unit()
    };
    let mut reader = Reader::new(model, k);
    let groups = reader.build(&stem)?;
    let groups = if options.separate_shells {
        groups.into_iter().flat_map(super::separate).collect()
    } else {
        groups
    };
    Ok(Imported {
        name: stem,
        groups,
        components: reader.components,
        merge_coplanar: options.merge_coplanar,
        soften: options.soften,
        materials: reader.materials,
        ..Imported::default()
    })
}

/// The model part `_rels/.rels` names, if it names one.
fn model_target(rels: &str) -> Option<String> {
    let root = parse(rels).ok()?;
    root.descendants()
        .into_iter()
        .filter(|e| e.name == "Relationship")
        .find(|e| e.attr("Type").is_some_and(|t| t.ends_with("/3dmodel")))
        .and_then(|e| e.attr("Target"))
        .map(str::to_owned)
}

/// A 3MF transform, `m00 m01 m02 m10 m11 m12 m20 m21 m22 m30 m31 m32`
/// (a point is a row times it), its moves scaled by `k`; the identity when
/// absent or unreadable.
fn transform(text: Option<&str>, k: f64) -> DMat4 {
    let Some(t) = text else {
        return DMat4::IDENTITY;
    };
    let n: Vec<f64> = t
        .split_ascii_whitespace()
        .filter_map(|v| v.parse().ok())
        .collect();
    if n.len() != 12 || n.iter().any(|v| !v.is_finite()) {
        return DMat4::IDENTITY;
    }
    DMat4::from_cols(
        DVec4::new(n[0], n[1], n[2], 0.0),
        DVec4::new(n[3], n[4], n[5], 0.0),
        DVec4::new(n[6], n[7], n[8], 0.0),
        DVec4::new(n[9] * k, n[10] * k, n[11] * k, 1.0),
    )
}

/// `#RRGGBB` or `#RRGGBBAA` as linear colour and opacity.
fn colour(text: &str) -> Option<([f32; 3], f32)> {
    let h = text.trim().strip_prefix('#')?;
    if !(h.len() == 6 || h.len() == 8) || !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    let linear = |b: u8| {
        let c = f32::from(b) / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let rgb = [linear(byte(0)?), linear(byte(2)?), linear(byte(4)?)];
    let alpha = if h.len() == 8 {
        f32::from(byte(6)?) / 255.0
    } else {
        1.0
    };
    Some((rgb, alpha))
}

/// One colour resource entry: its name (if any), linear colour and opacity.
type Swatch = (Option<String>, [f32; 3], f32);

/// Reading one model: its objects and colours, and what has been made.
struct Reader<'a> {
    /// Objects by id.
    objects: HashMap<&'a str, &'a Element>,
    /// Colour resources by id: each entry's name (if any) and colour.
    colours: HashMap<&'a str, Vec<Swatch>>,
    /// How many times each object is used, by items and components.
    uses: HashMap<&'a str, usize>,
    /// Model millimetres per file unit.
    k: f64,
    materials: Vec<Material>,
    material_index: HashMap<(String, usize), usize>,
    components: Vec<ImportGroup>,
    component_index: HashMap<&'a str, usize>,
    /// The build's items: object id and transform.
    items: Vec<(&'a str, Option<&'a str>)>,
}

impl<'a> Reader<'a> {
    fn new(model: &'a Element, k: f64) -> Self {
        let mut objects = HashMap::new();
        let mut colours = HashMap::new();
        if let Some(resources) = model.child("resources") {
            for r in &resources.children {
                let Some(id) = r.attr("id") else { continue };
                match r.name.as_str() {
                    "object" => {
                        objects.insert(id, r);
                    }
                    "basematerials" => {
                        let list = r
                            .children_named("base")
                            .map(|b| {
                                let (rgb, a) = b
                                    .attr("displaycolor")
                                    .and_then(colour)
                                    .unwrap_or(([0.8, 0.8, 0.8], 1.0));
                                (b.attr("name").map(str::to_owned), rgb, a)
                            })
                            .collect();
                        colours.insert(id, list);
                    }
                    "colorgroup" => {
                        let list = r
                            .children_named("color")
                            .map(|c| {
                                let (rgb, a) = c
                                    .attr("color")
                                    .and_then(colour)
                                    .unwrap_or(([0.8, 0.8, 0.8], 1.0));
                                (None, rgb, a)
                            })
                            .collect();
                        colours.insert(id, list);
                    }
                    _ => {}
                }
            }
        }
        let items: Vec<(&str, Option<&str>)> = model
            .child("build")
            .map(|b| {
                b.children_named("item")
                    .filter_map(|i| Some((i.attr("objectid")?, i.attr("transform"))))
                    .collect()
            })
            .unwrap_or_default();
        let mut uses: HashMap<&str, usize> = HashMap::new();
        for (id, _) in &items {
            *uses.entry(id).or_default() += 1;
        }
        for o in objects.values() {
            if let Some(cs) = o.child("components") {
                for c in cs.children_named("component") {
                    if let Some(id) = c.attr("objectid") {
                        *uses.entry(id).or_default() += 1;
                    }
                }
            }
        }
        Self {
            objects,
            colours,
            uses,
            k,
            materials: Vec::new(),
            material_index: HashMap::new(),
            components: Vec::new(),
            component_index: HashMap::new(),
            items,
        }
    }

    /// The groups the build places.
    fn build(&mut self, stem: &str) -> Result<Vec<ImportGroup>, IoError> {
        if self.items.is_empty() {
            return Err(IoError::Damaged(
                "the model places nothing on its build".into(),
            ));
        }
        let items = self.items.clone();
        let mut out = Vec::new();
        for (id, t) in items {
            let m = transform(t, self.k);
            let name = self.name_of(id, stem);
            if self.shared(id) {
                let c = self.component(id, 0);
                out.push(ImportGroup {
                    name: name.clone(),
                    uses: c.map(|c| (c, m, name, None)).into_iter().collect(),
                    ..ImportGroup::default()
                });
            } else {
                let mut g = self.group(id, 0);
                g.name = name;
                g.transform = m;
                out.push(g);
            }
        }
        Ok(out)
    }

    fn shared(&self, id: &str) -> bool {
        self.uses.get(id).copied().unwrap_or(0) > 1
    }

    fn name_of(&self, id: &str, stem: &str) -> String {
        self.objects
            .get(id)
            .and_then(|o| o.attr("name"))
            .filter(|n| !n.trim().is_empty())
            .map_or_else(
                || {
                    if self.items.len() == 1 {
                        stem.to_owned()
                    } else {
                        format!("Object {id}")
                    }
                },
                str::to_owned,
            )
    }

    /// Object `id` as a component (made once), its index.
    fn component(&mut self, id: &'a str, depth: usize) -> Option<usize> {
        if let Some(i) = self.component_index.get(id) {
            return Some(*i);
        }
        if depth > MAX_DEPTH || !self.objects.contains_key(id) {
            return None;
        }
        let mut g = self.group(id, depth + 1);
        g.name = self.name_of(id, "Component");
        self.components.push(g);
        let i = self.components.len() - 1;
        self.component_index.insert(id, i);
        Some(i)
    }

    /// Object `id` as a group in its own coordinates.
    fn group(&mut self, id: &'a str, depth: usize) -> ImportGroup {
        let mut g = ImportGroup::default();
        let Some(object) = self.objects.get(id).copied() else {
            return g;
        };
        if depth > MAX_DEPTH {
            return g;
        }
        if let Some(mesh) = object.child("mesh") {
            g.polygons = self.polygons(object, mesh);
        }
        if let Some(cs) = object.child("components") {
            for c in cs.children_named("component") {
                let Some(cid) = c.attr("objectid") else {
                    continue;
                };
                let m = transform(c.attr("transform"), self.k);
                let name = self.name_of(cid, "Component");
                if self.shared(cid) {
                    if let Some(ci) = self.component(cid, depth + 1) {
                        g.uses.push((ci, m, name, None));
                    }
                } else {
                    let mut child = self.group(cid, depth + 1);
                    child.name = name;
                    child.transform = m;
                    g.children.push(child);
                }
            }
        }
        g
    }

    /// An object's triangles, in model millimetres, with their colours.
    fn polygons(&mut self, object: &Element, mesh: &Element) -> Vec<Polygon> {
        let k = self.k;
        let num = |e: &Element, a: &str| e.attr(a).and_then(|v| v.trim().parse::<f64>().ok());
        let points: Vec<DVec3> = mesh
            .child("vertices")
            .map(|vs| {
                vs.children_named("vertex")
                    .map(|v| {
                        DVec3::new(
                            num(v, "x").unwrap_or(0.0),
                            num(v, "y").unwrap_or(0.0),
                            num(v, "z").unwrap_or(0.0),
                        ) * k
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (opid, oindex) = (object.attr("pid"), object.attr("pindex"));
        let Some(tris) = mesh.child("triangles") else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for t in tris.children_named("triangle") {
            let corner = |a: &str| {
                t.attr(a)
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .and_then(|i| points.get(i).copied())
            };
            let (Some(a), Some(b), Some(c)) = (corner("v1"), corner("v2"), corner("v3")) else {
                continue;
            };
            let pid = t.attr("pid").or(opid);
            let index = t.attr("p1").or(oindex);
            let material = match (pid, index.and_then(|i| i.trim().parse::<usize>().ok())) {
                (Some(p), Some(i)) => self.material(p, i),
                _ => None,
            };
            out.push(Polygon {
                points: vec![a, b, c],
                material,
                ..Polygon::default()
            });
        }
        out
    }

    /// The material for entry `index` of colour resource `pid`.
    fn material(&mut self, pid: &str, index: usize) -> Option<usize> {
        let key = (pid.to_owned(), index);
        if let Some(m) = self.material_index.get(&key) {
            return Some(*m);
        }
        let (name, color, opacity) = self.colours.get(pid)?.get(index)?.clone();
        self.materials.push(Material {
            name: name.unwrap_or_else(|| format!("Colour {}", self.materials.len() + 1)),
            color,
            opacity,
            ..Material::default()
        });
        let m = self.materials.len() - 1;
        self.material_index.insert(key, m);
        Some(m)
    }
}

/// One zip entry: its name, compression method, sizes and where its local
/// header starts.
#[derive(Debug, Clone)]
struct ZipEntry {
    name: String,
    method: u16,
    compressed: u64,
    size: u64,
    local: u64,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The entries of the zip `bytes`, from its central directory.
fn zip_entries(bytes: &[u8]) -> Result<Vec<ZipEntry>, IoError> {
    let not_zip = || IoError::Damaged("not a 3MF package (no zip directory)".into());
    // The end record: in the last 64 KiB plus its own 22 bytes.
    let from = bytes.len().saturating_sub(22 + 65_535);
    let end = (from..bytes.len().saturating_sub(21))
        .rev()
        .find(|i| u32_at(bytes, *i) == Some(0x0605_4b50))
        .ok_or_else(not_zip)?;
    let count = usize::from(u16_at(bytes, end + 10).ok_or_else(not_zip)?);
    let start = u32_at(bytes, end + 16).ok_or_else(not_zip)?;
    if start == u32::MAX {
        return Err(IoError::Damaged("ZIP64 packages are not read".into()));
    }
    let mut at = start as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(bytes, at) != Some(0x0201_4b50) {
            return Err(not_zip());
        }
        let field = |o: usize| u32_at(bytes, at + o).ok_or_else(not_zip);
        let short = |o: usize| u16_at(bytes, at + o).ok_or_else(not_zip);
        let method = short(10)?;
        let compressed = field(20)?;
        let size = field(24)?;
        let name_len = usize::from(short(28)?);
        let extra_len = usize::from(short(30)?);
        let comment_len = usize::from(short(32)?);
        let local = field(42)?;
        let name = bytes
            .get(at + 46..at + 46 + name_len)
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .ok_or_else(not_zip)?;
        if compressed == u32::MAX || size == u32::MAX || local == u32::MAX {
            return Err(IoError::Damaged("ZIP64 packages are not read".into()));
        }
        out.push(ZipEntry {
            name,
            method,
            compressed: u64::from(compressed),
            size: u64::from(size),
            local: u64::from(local),
        });
        at += 46 + name_len + extra_len + comment_len;
    }
    Ok(out)
}

/// The contents of `entry` in the zip `bytes`.
fn zip_read(bytes: &[u8], entry: &ZipEntry) -> Result<Vec<u8>, IoError> {
    let damaged = || IoError::Damaged(format!("the package part {} is cut short", entry.name));
    let at = usize::try_from(entry.local).map_err(|_| damaged())?;
    if u32_at(bytes, at) != Some(0x0403_4b50) {
        return Err(damaged());
    }
    let name_len = usize::from(u16_at(bytes, at + 26).ok_or_else(damaged)?);
    let extra_len = usize::from(u16_at(bytes, at + 28).ok_or_else(damaged)?);
    let start = at + 30 + name_len + extra_len;
    let len = usize::try_from(entry.compressed).map_err(|_| damaged())?;
    let data = bytes.get(start..start + len).ok_or_else(damaged)?;
    match entry.method {
        0 => Ok(data.to_vec()),
        8 => {
            let mut out = Vec::with_capacity(usize::try_from(entry.size).unwrap_or(0));
            flate2::read::DeflateDecoder::new(data)
                .read_to_end(&mut out)
                .map_err(|_| damaged())?;
            Ok(out)
        }
        m => Err(IoError::Damaged(format!(
            "the package part {} uses compression {m}, which is not read",
            entry.name
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_read_as_rows() {
        // A move of 10, 20, 30 file units, millimetres to the model ×2.
        let m = transform(Some("1 0 0 0 1 0 0 0 1 10 20 30"), 2.0);
        assert_eq!(
            m.transform_point3(DVec3::ZERO),
            DVec3::new(20.0, 40.0, 60.0)
        );
        // A quarter turn about Z: X goes to Y.
        let r = transform(Some("0 1 0 -1 0 0 0 0 1 0 0 0"), 1.0);
        let p = r.transform_point3(DVec3::X);
        assert!((p - DVec3::Y).length() < 1e-12);
        assert_eq!(transform(Some("1 2 3"), 1.0), DMat4::IDENTITY);
    }

    #[test]
    fn colours_are_srgb_with_alpha() {
        let (rgb, a) = colour("#FF000080").unwrap();
        assert!((rgb[0] - 1.0).abs() < 1e-6 && rgb[1] == 0.0);
        assert!((a - 128.0 / 255.0).abs() < 1e-6);
        let (_, a) = colour("#336699").unwrap();
        assert_eq!(a, 1.0);
        assert!(colour("red").is_none());
    }
}
