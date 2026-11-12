//! SketchSmith — DAE (COLLADA) reading | v1.0.0 | Thursday 12 November 2026
//!
//! Reads a COLLADA file's visual scene into groups, as SketchUp and other
//! programs write it:
//!
//! * **Units and axes**: the file's `<unit meter>` (with From File; else
//!   the Units chosen) and `<up_axis>` (X, Y or Z up). Positions are moved
//!   into model millimetres, Z up, as they are read, and every node's
//!   transform into the same space, so each group's geometry is true size.
//! * **Nodes**: each scene node holding geometry or other nodes is a group
//!   named after it (`name`, else `id`), at its transform (`matrix`,
//!   `translate`, `rotate`, `scale`, in order), nested as in the file. A
//!   node placed by `instance_node` (SketchUp's components) is made once as
//!   a component and copied wherever it is placed.
//! * **Geometry**: a mesh's `triangles`, `tristrips`, `trifans`,
//!   `polylist` and `polygons` (holes, `ph`, are counted and only the
//!   outline kept), and `lines` and `linestrips` as edges; its `VERTEX`
//!   (through `<vertices>`) and first `TEXCOORD` inputs, by offset. A
//!   skinned mesh (`instance_controller`) comes in as it stands, unposed.
//!   Decimal commas and UTF-16 files are read too.
//! * **Materials**: `bind_material` maps each primitive's symbol to a
//!   material, its effect to a diffuse colour or image (through
//!   `sampler2D` and `surface`, or straight to an image) and its
//!   `transparency`. Images are found by their path (`file://` and `%20`
//!   undone), else by name beside the file.
//! * Cameras and lights are counted and skipped; so is everything else.

use std::collections::HashMap;
use std::path::Path;

use glam::{DMat3, DMat4, DVec2, DVec3};
use smith_model::{Material, Texture};

use super::{ImportGroup, ImportOptions, Imported, Polygon, split, url_path};
use crate::error::IoError;
use crate::xml::{self, Element};

/// Reads the DAE `path`.
pub(super) fn read_dae(path: &Path, options: &ImportOptions) -> Result<Imported, IoError> {
    let bytes = std::fs::read(path)?;
    let root = xml::parse(&xml::text_of(&bytes))?;
    if root.name != "COLLADA" {
        return Err(IoError::Damaged("this is not a COLLADA file".into()));
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Import".into());
    let mut reader = Dae::new(&root, path.parent().unwrap_or(Path::new(".")), options);
    let scene = reader
        .scene()
        .ok_or_else(|| IoError::Damaged("the file has no scene".into()))?;
    let groups: Vec<ImportGroup> = scene
        .children_named("node")
        .filter_map(|n| reader.node(n, 0))
        .map(|g| {
            if options.separate_shells && !g.polygons.is_empty() {
                split(g)
            } else {
                g
            }
        })
        .collect();
    Ok(Imported {
        name: stem,
        groups,
        components: reader.components,
        merge_coplanar: options.merge_coplanar,
        soften: options.soften,
        materials: reader.materials,
        missing: reader.missing,
        holes: reader.holes,
        skipped: reader.skipped,
        ..Imported::default()
    })
}

/// Reading one file: its elements by id, and what has been gathered.
struct Dae<'a> {
    ids: HashMap<&'a str, &'a Element>,
    dir: &'a Path,
    options: &'a ImportOptions,
    /// File coordinates to model mm, Z up.
    to_model: DMat4,
    materials: Vec<Material>,
    /// Material index of each material id.
    material_index: HashMap<String, Option<usize>>,
    missing: Vec<String>,
    components: Vec<ImportGroup>,
    /// Component index of each node id placed by `instance_node`.
    component_index: HashMap<String, Option<usize>>,
    holes: usize,
    skipped: usize,
}

/// The deepest nesting followed, so a file cannot loop forever.
const MAX_DEPTH: usize = 64;

impl<'a> Dae<'a> {
    fn new(root: &'a Element, dir: &'a Path, options: &'a ImportOptions) -> Self {
        let mut ids = HashMap::new();
        for e in root.descendants() {
            if let Some(id) = e.attr("id") {
                ids.insert(id, e);
            }
        }
        let asset = root.child("asset");
        let metre = asset
            .and_then(|a| a.child("unit"))
            .and_then(|u| u.attr("meter"))
            .and_then(|m| m.trim().parse::<f64>().ok())
            .filter(|m| m.is_finite() && *m > 0.0)
            .unwrap_or(1.0);
        let k = if options.from_file {
            metre * 1000.0
        } else {
            options.mm_per_unit()
        };
        let up = asset
            .and_then(|a| a.child("up_axis"))
            .map_or("Y_UP", |u| u.text.trim());
        // The file's right, up and in directions onto the model's X, Z, −Y.
        let turn = match up {
            "Z_UP" => DMat3::IDENTITY,
            "X_UP" => DMat3::from_cols(DVec3::Z, -DVec3::X, -DVec3::Y),
            _ => DMat3::from_cols(DVec3::X, DVec3::Z, -DVec3::Y),
        };
        Self {
            ids,
            dir,
            options,
            to_model: DMat4::from_mat3(turn * k),
            materials: Vec::new(),
            material_index: HashMap::new(),
            missing: Vec::new(),
            components: Vec::new(),
            component_index: HashMap::new(),
            holes: 0,
            skipped: 0,
        }
    }

    /// The element `url` (`#id`) names.
    fn find(&self, url: &str) -> Option<&'a Element> {
        self.ids.get(url.trim_start_matches('#')).copied()
    }

    /// The visual scene the file shows (its `<scene>`'s, else the first).
    fn scene(&self) -> Option<&'a Element> {
        self.ids
            .values()
            .find(|e| e.name == "instance_visual_scene")
            .and_then(|i| i.attr("url"))
            .and_then(|u| self.find(u))
            .or_else(|| {
                self.ids
                    .values()
                    .copied()
                    .find(|e| e.name == "visual_scene")
            })
    }

    /// A node's transform in file space: its transform elements, in order.
    fn file_transform(node: &Element) -> DMat4 {
        let mut m = DMat4::IDENTITY;
        for t in &node.children {
            let v = t.numbers();
            m *= match (t.name.as_str(), v.len()) {
                ("matrix", 16) => DMat4::from_cols_array(&[
                    v[0], v[4], v[8], v[12], v[1], v[5], v[9], v[13], v[2], v[6], v[10], v[14],
                    v[3], v[7], v[11], v[15],
                ]),
                ("translate", 3) => DMat4::from_translation(DVec3::new(v[0], v[1], v[2])),
                ("scale", 3) => DMat4::from_scale(DVec3::new(v[0], v[1], v[2])),
                ("rotate", 4) => {
                    let axis = DVec3::new(v[0], v[1], v[2]).normalize_or(DVec3::Z);
                    DMat4::from_axis_angle(axis, v[3].to_radians())
                }
                _ => DMat4::IDENTITY,
            };
        }
        m
    }

    /// A file-space transform as the model sees it.
    fn model_transform(&self, file: DMat4) -> DMat4 {
        self.to_model * file * self.to_model.inverse()
    }

    /// `node` as a group, or `None` when it holds nothing.
    fn node(&mut self, node: &'a Element, depth: usize) -> Option<ImportGroup> {
        if depth > MAX_DEPTH {
            return None;
        }
        let mut g = ImportGroup {
            name: node
                .attr("name")
                .or_else(|| node.attr("id"))
                .unwrap_or("Group")
                .to_owned(),
            transform: self.model_transform(Self::file_transform(node)),
            ..ImportGroup::default()
        };
        self.contents(node, &mut g, depth);
        let holds = !g.polygons.is_empty()
            || !g.lines.is_empty()
            || !g.children.is_empty()
            || !g.uses.is_empty();
        holds.then_some(g)
    }

    /// Puts what `node` holds into `g`: its geometry, its nodes, its
    /// component copies.
    fn contents(&mut self, node: &'a Element, g: &mut ImportGroup, depth: usize) {
        for c in &node.children {
            match c.name.as_str() {
                "instance_geometry" | "instance_controller" => {
                    let Some(mut geo) = c.attr("url").and_then(|u| self.find(u)) else {
                        continue;
                    };
                    // A skin's (or morph's) mesh is its source geometry.
                    if geo.name == "controller" {
                        let Some(src) = geo
                            .children
                            .iter()
                            .find_map(|k| k.attr("source"))
                            .and_then(|u| self.find(u))
                        else {
                            continue;
                        };
                        geo = src;
                    }
                    let binds = self.bindings(c);
                    self.geometry(geo, &binds, g);
                }
                "node" => {
                    if let Some(child) = self.node(c, depth + 1) {
                        if self.options.separate_shells && !child.polygons.is_empty() {
                            g.children.push(split(child));
                        } else {
                            g.children.push(child);
                        }
                    }
                }
                "instance_node" => {
                    let Some(url) = c.attr("url") else { continue };
                    if let Some(k) = self.component(url, depth) {
                        let target = self.find(url).map_or(DMat4::IDENTITY, Self::file_transform);
                        let name = c
                            .attr("name")
                            .map(str::to_owned)
                            .unwrap_or_else(|| self.components[k].name.clone());
                        g.uses.push((k, self.model_transform(target), name, None));
                    }
                }
                "instance_camera" | "instance_light" => self.skipped += 1,
                _ => {}
            }
        }
    }

    /// The component made from the node `url` names (made the first time).
    fn component(&mut self, url: &str, depth: usize) -> Option<usize> {
        let id = url.trim_start_matches('#').to_owned();
        if let Some(k) = self.component_index.get(&id) {
            return *k;
        }
        // Mark it first, so a node placing itself does not loop.
        self.component_index.insert(id.clone(), None);
        let node = self.find(url)?;
        let mut g = ImportGroup {
            name: node.attr("name").unwrap_or(&id).to_owned(),
            ..ImportGroup::default()
        };
        self.contents(node, &mut g, depth + 1);
        let k = self.components.len();
        self.components.push(g);
        self.component_index.insert(id, Some(k));
        Some(k)
    }

    /// Each material symbol of an `instance_geometry`, as a material index.
    fn bindings(&mut self, inst: &'a Element) -> HashMap<String, usize> {
        let mut out = HashMap::new();
        let found: Vec<(&str, &str)> = inst
            .descendants()
            .into_iter()
            .filter(|e| e.name == "instance_material")
            .filter_map(|e| Some((e.attr("symbol")?, e.attr("target")?)))
            .collect();
        for (symbol, target) in found {
            if let Some(m) = self.material(target) {
                out.insert(symbol.to_owned(), m);
            }
        }
        out
    }

    /// The material `url` names, read the first time.
    fn material(&mut self, url: &str) -> Option<usize> {
        let id = url.trim_start_matches('#').to_owned();
        if let Some(m) = self.material_index.get(&id) {
            return *m;
        }
        let made = self.read_material(url);
        self.material_index.insert(id, made);
        made
    }

    fn read_material(&mut self, url: &str) -> Option<usize> {
        let mat = self.find(url)?;
        let effect = mat
            .child("instance_effect")
            .and_then(|i| i.attr("url"))
            .and_then(|u| self.find(u))?;
        let technique = effect
            .descendants()
            .into_iter()
            .find(|e| matches!(e.name.as_str(), "lambert" | "phong" | "blinn" | "constant"))?;
        let diffuse = technique
            .child("diffuse")
            .or_else(|| technique.child("emission"));
        let mut color = [0.8_f32, 0.8, 0.8];
        let mut texture = None;
        if let Some(d) = diffuse {
            if let Some(c) = d.child("color") {
                let v = c.numbers();
                if v.len() >= 3 {
                    color = [v[0] as f32, v[1] as f32, v[2] as f32].map(|x| x.clamp(0.0, 2.0));
                }
            }
            if let Some(t) = d.child("texture").and_then(|t| t.attr("texture")) {
                texture = self.image_for(effect, t);
                if texture.is_some() {
                    color = [1.0; 3];
                }
            }
        }
        let opacity = technique
            .child("transparency")
            .and_then(|t| t.child("float"))
            .and_then(|f| f.numbers().first().copied())
            .map_or(1.0, |v| v.clamp(0.0, 1.0)) as f32;
        let name = mat
            .attr("name")
            .or_else(|| mat.attr("id"))
            .unwrap_or("Material")
            .to_owned();
        self.materials.push(Material {
            name,
            color,
            opacity,
            metallic: false,
            texture,
        });
        Some(self.materials.len() - 1)
    }

    /// The image a `<texture texture="…">` names: a sampler (1.4: through
    /// its surface) in `effect`, or an image straight away.
    fn image_for(&mut self, effect: &'a Element, name: &str) -> Option<Texture> {
        let param = |sid: &str| {
            effect
                .descendants()
                .into_iter()
                .find(|e| e.name == "newparam" && e.attr("sid") == Some(sid))
        };
        let mut image_id = None;
        if let Some(p) = param(name) {
            let source = p
                .descendants()
                .into_iter()
                .find(|e| e.name == "source")
                .map(|s| s.text.trim().to_owned());
            let direct = p
                .descendants()
                .into_iter()
                .find(|e| e.name == "instance_image")
                .and_then(|i| i.attr("url"))
                .map(|u| u.trim_start_matches('#').to_owned());
            image_id = direct.or_else(|| {
                let surface = param(&source?)?;
                surface
                    .descendants()
                    .into_iter()
                    .find(|e| e.name == "init_from")
                    .map(|i| i.text.trim().to_owned())
            });
        }
        let image = self.find(image_id.as_deref().unwrap_or(name))?;
        let file = image
            .descendants()
            .into_iter()
            .find(|e| e.name == "init_from")
            .map(|i| {
                // 1.5 puts the path in a <ref> inside.
                i.child("ref")
                    .map_or(i.text.trim(), |r| r.text.trim())
                    .to_owned()
            })?;
        let path = url_path(&file).replace('\\', "/");
        let found = [
            self.dir.join(&path),
            self.dir.join(Path::new(&path).file_name()?),
        ]
        .into_iter()
        .find_map(|p| crate::textures::load_image(&p).ok());
        if found.is_none() && !self.missing.contains(&path) {
            self.missing.push(path.clone());
        }
        found.map(|image| Texture {
            image: std::sync::Arc::new(image),
            width_mm: 100.0,
        })
    }

    /// Adds `geo`'s mesh to `g`, its primitives' symbols bound by `binds`.
    fn geometry(&mut self, geo: &'a Element, binds: &HashMap<String, usize>, g: &mut ImportGroup) {
        let Some(mesh) = geo.child("mesh") else {
            return;
        };
        for prim in &mesh.children {
            let kind = prim.name.as_str();
            if !matches!(
                kind,
                "triangles"
                    | "tristrips"
                    | "trifans"
                    | "polylist"
                    | "polygons"
                    | "lines"
                    | "linestrips"
            ) {
                continue;
            }
            let inputs: Vec<(&str, usize, &str)> = prim
                .children_named("input")
                .filter_map(|i| {
                    Some((
                        i.attr("semantic")?,
                        i.attr("offset")?.parse().ok()?,
                        i.attr("source")?,
                    ))
                })
                .collect();
            let stride = inputs.iter().map(|(_, o, _)| o + 1).max().unwrap_or(1);
            let Some(&(_, vo, vsrc)) = inputs.iter().find(|(s, ..)| *s == "VERTEX") else {
                continue;
            };
            let Some(positions) = self
                .find(vsrc)
                .and_then(|v| {
                    v.children_named("input")
                        .find(|i| i.attr("semantic") == Some("POSITION"))
                })
                .and_then(|i| i.attr("source"))
                .and_then(|s| self.source(s))
            else {
                continue;
            };
            let uvs = inputs
                .iter()
                .find(|(s, ..)| *s == "TEXCOORD")
                .and_then(|&(_, o, src)| Some((o, self.source(src)?)));
            let material = prim.attr("material").and_then(|m| binds.get(m).copied());
            let corner = |idx: &[usize], at: usize| -> Option<(DVec3, Option<DVec2>)> {
                let base = at * stride;
                let p = positions.get(*idx.get(base + vo)?)?;
                let p = self.to_model.transform_point3(DVec3::new(
                    p[0],
                    p[1],
                    p.get(2).copied().unwrap_or(0.0),
                ));
                let t = uvs.as_ref().and_then(|(o, s)| {
                    let v = s.get(*idx.get(base + o)?)?;
                    Some(DVec2::new(v[0], v.get(1).copied().unwrap_or(0.0)))
                });
                Some((p, t))
            };
            let polygon = |idx: &[usize], from: usize, n: usize, g: &mut ImportGroup| {
                let corners: Option<Vec<(DVec3, Option<DVec2>)>> =
                    (from..from + n).map(|i| corner(idx, i)).collect();
                let Some(corners) = corners else { return };
                let uv: Option<Vec<DVec2>> = corners.iter().map(|c| c.1).collect();
                g.polygons.push(Polygon {
                    points: corners.iter().map(|c| c.0).collect(),
                    uvs: uv,
                    material,
                    ..Polygon::default()
                });
            };
            let ints = |e: &Element| -> Vec<usize> {
                e.text
                    .split_ascii_whitespace()
                    .filter_map(|t| t.parse().ok())
                    .collect()
            };
            match kind {
                "triangles" => {
                    for p in prim.children_named("p") {
                        let idx = ints(p);
                        for t in 0..idx.len() / (3 * stride) {
                            polygon(&idx, t * 3, 3, g);
                        }
                    }
                }
                "tristrips" | "trifans" => {
                    // Each p is one strip or fan; strips alternate winding.
                    for p in prim.children_named("p") {
                        let idx = ints(p);
                        let n = idx.len() / stride;
                        let pick =
                            |i: usize| -> Vec<usize> { idx[i * stride..(i + 1) * stride].to_vec() };
                        for t in 2..n {
                            let corners = if kind == "trifans" {
                                [0, t - 1, t]
                            } else if t % 2 == 0 {
                                [t - 2, t - 1, t]
                            } else {
                                [t - 1, t - 2, t]
                            };
                            let tri: Vec<usize> = corners.iter().flat_map(|&i| pick(i)).collect();
                            polygon(&tri, 0, 3, g);
                        }
                    }
                }
                "polylist" => {
                    let counts = prim.child("vcount").map(ints).unwrap_or_default();
                    let idx = prim.child("p").map(ints).unwrap_or_default();
                    let mut at = 0;
                    for n in counts {
                        polygon(&idx, at, n, g);
                        at += n;
                    }
                }
                "polygons" => {
                    for p in prim.children_named("p") {
                        let idx = ints(p);
                        polygon(&idx, 0, idx.len() / stride, g);
                    }
                    for ph in prim.children_named("ph") {
                        if let Some(p) = ph.child("p") {
                            let idx = ints(p);
                            polygon(&idx, 0, idx.len() / stride, g);
                        }
                        self.holes += ph.children_named("h").count();
                    }
                }
                _ => {
                    // Lines in pairs; line strips run on.
                    for p in prim.children_named("p") {
                        let idx = ints(p);
                        let pts: Vec<DVec3> = (0..idx.len() / stride)
                            .filter_map(|i| corner(&idx, i).map(|c| c.0))
                            .collect();
                        if kind == "lines" {
                            g.lines.extend(pts.as_chunks::<2>().0.iter().copied());
                        } else {
                            g.lines.extend(pts.windows(2).map(|w| [w[0], w[1]]));
                        }
                    }
                }
            }
        }
    }

    /// A `<source>`'s values, one entry per item (its accessor's stride).
    fn source(&self, url: &str) -> Option<Vec<Vec<f64>>> {
        let src = self.find(url)?;
        let values = src.child("float_array")?.numbers();
        let stride = src
            .child("technique_common")
            .and_then(|t| t.child("accessor"))
            .and_then(|a| a.attr("stride"))
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|s| *s > 0)
            .unwrap_or(3);
        Some(values.chunks_exact(stride).map(<[f64]>::to_vec).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{ImportFormat, place};

    /// A SketchUp-style file: inches, Z up; a "Table" group holding its top
    /// (a coloured quad), and a "Leg" component (a textured box, one inch
    /// square, two tall) placed twice; a line; a camera.
    const TABLE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <asset><unit meter="0.0254" name="inch"/><up_axis>Z_UP</up_axis></asset>
  <library_images><image id="wood-image"><init_from>tex/wood%20grain.png</init_from></image></library_images>
  <library_effects>
    <effect id="red-fx"><profile_COMMON><technique sid="c"><lambert>
      <diffuse><color>0.8 0.1 0.1 1</color></diffuse>
      <transparency><float>0.5</float></transparency>
    </lambert></technique></profile_COMMON></effect>
    <effect id="wood-fx"><profile_COMMON>
      <newparam sid="wood-surface"><surface type="2D"><init_from>wood-image</init_from></surface></newparam>
      <newparam sid="wood-sampler"><sampler2D><source>wood-surface</source></sampler2D></newparam>
      <technique sid="c"><lambert><diffuse><texture texture="wood-sampler" texcoord="UVSET0"/></diffuse></lambert></technique>
    </profile_COMMON></effect>
  </library_effects>
  <library_materials>
    <material id="red-mat" name="Red"><instance_effect url="#red-fx"/></material>
    <material id="wood-mat" name="Wood"><instance_effect url="#wood-fx"/></material>
  </library_materials>
  <library_geometries>
    <geometry id="top-mesh"><mesh>
      <source id="top-pos"><float_array id="top-pos-a" count="12">0 0 2 10 0 2 10 5 2 0 5 2</float_array>
        <technique_common><accessor source="#top-pos-a" count="4" stride="3"/></technique_common></source>
      <vertices id="top-v"><input semantic="POSITION" source="#top-pos"/></vertices>
      <polylist count="1" material="RedSym"><input semantic="VERTEX" source="#top-v" offset="0"/>
        <vcount>4</vcount><p>0 1 2 3</p></polylist>
      <lines count="1"><input semantic="VERTEX" source="#top-v" offset="0"/><p>0 2</p></lines>
    </mesh></geometry>
    <geometry id="leg-mesh"><mesh>
      <source id="leg-pos"><float_array id="leg-pos-a" count="24">0 0 0 1 0 0 1 1 0 0 1 0 0 0 2 1 0 2 1 1 2 0 1 2</float_array>
        <technique_common><accessor source="#leg-pos-a" count="8" stride="3"/></technique_common></source>
      <source id="leg-uv"><float_array id="leg-uv-a" count="8">0 0 1 0 1 1 0 1</float_array>
        <technique_common><accessor source="#leg-uv-a" count="4" stride="2"/></technique_common></source>
      <vertices id="leg-v"><input semantic="POSITION" source="#leg-pos"/></vertices>
      <triangles count="12" material="WoodSym">
        <input semantic="VERTEX" source="#leg-v" offset="0"/>
        <input semantic="TEXCOORD" source="#leg-uv" offset="1" set="0"/>
        <p>0 0 3 1 2 2  0 0 2 2 1 3  4 0 5 1 6 2  4 0 6 2 7 3
           0 0 1 1 5 2  0 0 5 2 4 3  1 0 2 1 6 2  1 0 6 2 5 3
           2 0 3 1 7 2  2 0 7 2 6 3  3 0 0 1 4 2  3 0 4 2 7 3</p>
      </triangles>
    </mesh></geometry>
  </library_geometries>
  <library_nodes>
    <node id="leg-node" name="Leg">
      <instance_geometry url="#leg-mesh"><bind_material><technique_common>
        <instance_material symbol="WoodSym" target="#wood-mat"/>
      </technique_common></bind_material></instance_geometry>
    </node>
  </library_nodes>
  <library_visual_scenes><visual_scene id="scene">
    <node id="cam"><instance_camera url="#c"/></node>
    <node id="table" name="Table">
      <instance_geometry url="#top-mesh"><bind_material><technique_common>
        <instance_material symbol="RedSym" target="#red-mat"/>
      </technique_common></bind_material></instance_geometry>
      <node name="Leg A"><translate>0 0 0</translate><instance_node url="#leg-node"/></node>
      <node name="Leg B"><matrix>1 0 0 9 0 1 0 4 0 0 1 0 0 0 0 1</matrix><instance_node url="#leg-node"/></node>
    </node>
  </visual_scene></library_visual_scenes>
  <scene><instance_visual_scene url="#scene"/></scene>
</COLLADA>"##;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("smith-io-dae-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(d.join("tex")).unwrap();
        d
    }

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([150, 100, 50, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn a_sketchup_style_file_keeps_its_groups_components_and_size() {
        let d = dir("table");
        std::fs::write(d.join("tex/wood grain.png"), png()).unwrap();
        let path = d.join("table.dae");
        std::fs::write(&path, TABLE).unwrap();
        let options = ImportOptions::for_format(ImportFormat::Dae);
        let imported = read_dae(&path, &options).unwrap();
        assert_eq!(imported.skipped, 1);
        assert!(imported.missing.is_empty());
        assert_eq!(imported.groups.len(), 1);
        assert_eq!(imported.components.len(), 1);
        assert_eq!(imported.materials.len(), 2);
        let red = &imported.materials[0];
        assert_eq!(red.name, "Red");
        assert!((red.opacity - 0.5).abs() < 1e-6);
        assert!(imported.materials[1].texture.is_some());
        let mut doc = smith_model::Document::new();
        let (_, report) = doc
            .transaction("Import", |dd| place(dd, None, DMat4::IDENTITY, &imported))
            .unwrap();
        // The top (1 face) and the leg made once (12 triangles → 6 faces).
        assert_eq!(report.counts.faces - report.counts.merged, 7);
        assert_eq!(report.counts.lines, 1);
        assert_eq!(report.groups, 1);
        // Inches to mm: the table is 10 × 5 × 2 in.
        let (lo, hi) = doc.model_bounds().unwrap();
        assert!(
            (hi - lo - DVec3::new(254.0, 127.0, 50.8)).length() < 1e-6,
            "{lo} {hi}"
        );
        // One component, used twice.
        let comps: Vec<_> = doc
            .definition_ids()
            .filter(|k| {
                doc.definition(*k)
                    .is_some_and(|x| x.kind == smith_model::DefinitionKind::Component)
            })
            .collect();
        assert_eq!(comps.len(), 1);
        let uses = doc
            .definition_ids()
            .filter_map(|k| doc.definition_entities(k))
            .chain([doc.root()])
            .flat_map(|e| {
                e.instance_ids()
                    .filter_map(move |i| e.instance(i))
                    .collect::<Vec<_>>()
            })
            .filter(|i| i.definition == comps[0])
            .count();
        assert_eq!(uses, 2);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn y_up_metres_holes_and_a_missing_image() {
        let text = TABLE
            .replace(r#"<unit meter="0.0254" name="inch"/><up_axis>Z_UP</up_axis>"#, "<up_axis>Y_UP</up_axis>")
            .replace(
                r##"<polylist count="1" material="RedSym"><input semantic="VERTEX" source="#top-v" offset="0"/>
        <vcount>4</vcount><p>0 1 2 3</p></polylist>"##,
                r##"<polygons count="1" material="RedSym"><input semantic="VERTEX" source="#top-v" offset="0"/>
        <ph><p>0 1 2 3</p><h>0 1 2</h></ph></polygons>"##,
            );
        let d = dir("yup");
        let path = d.join("yup.dae");
        std::fs::write(&path, text).unwrap();
        let imported = read_dae(&path, &ImportOptions::for_format(ImportFormat::Dae)).unwrap();
        assert_eq!(imported.holes, 1);
        assert_eq!(imported.missing, ["tex/wood grain.png"]);
        let mut doc = smith_model::Document::new();
        doc.transaction("Import", |dd| place(dd, None, DMat4::IDENTITY, &imported))
            .unwrap();
        // Metres, and the file's 2 up (its Z) is the model's −Y depth:
        // 10 m wide, 2 m tall from the file's Y, 5 m deep.
        let (lo, hi) = doc.model_bounds().unwrap();
        assert!(
            (hi - lo - DVec3::new(10_000.0, 2_000.0, 5_000.0)).length() < 1e-6,
            "{lo} {hi}"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn strips_fans_and_skins() {
        // The top as a triangle strip, then as a fan; the leg's geometry
        // behind a skin.
        for prim in [
            r##"<tristrips count="1" material="RedSym"><input semantic="VERTEX" source="#top-v" offset="0"/>
        <p>0 1 3 2</p></tristrips>"##,
            r##"<trifans count="1" material="RedSym"><input semantic="VERTEX" source="#top-v" offset="0"/>
        <p>0 1 2 3</p></trifans>"##,
        ] {
            let text = TABLE
                .replace(
                    r##"<polylist count="1" material="RedSym"><input semantic="VERTEX" source="#top-v" offset="0"/>
        <vcount>4</vcount><p>0 1 2 3</p></polylist>"##,
                    prim,
                )
                .replace(
                    r##"<instance_geometry url="#leg-mesh">"##,
                    r##"<instance_controller url="#leg-skin">"##,
                )
                .replace(
                    "</instance_geometry>\n    </node>\n  </library_nodes>",
                    "</instance_controller>\n    </node>\n  </library_nodes>",
                )
                .replace(
                    "<library_nodes>",
                    r##"<library_controllers><controller id="leg-skin"><skin source="#leg-mesh"/></controller></library_controllers>
  <library_nodes>"##,
                );
            let d = dir("strips");
            let path = d.join("strips.dae");
            std::fs::write(&path, text).unwrap();
            let imported = read_dae(&path, &ImportOptions::for_format(ImportFormat::Dae)).unwrap();
            let mut doc = smith_model::Document::new();
            let (_, report) = doc
                .transaction("Import", |dd| place(dd, None, DMat4::IDENTITY, &imported))
                .unwrap();
            // Two triangles making the top one face again, and the leg's six.
            assert_eq!(report.counts.faces - report.counts.merged, 7);
            std::fs::remove_dir_all(&d).unwrap();
        }
    }

    #[test]
    fn paths_and_non_collada() {
        assert_eq!(url_path("file:///home/a%20b/c.png"), "/home/a b/c.png");
        assert_eq!(url_path("tex/x.png"), "tex/x.png");
        let d = dir("bad");
        let path = d.join("bad.dae");
        std::fs::write(&path, "<svg/>").unwrap();
        assert!(read_dae(&path, &ImportOptions::default()).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
