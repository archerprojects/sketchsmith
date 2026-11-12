//! SketchSmith — glTF reading | v1.0.0 | Thursday 12 November 2026
//!
//! Reads another program's glTF 2.0 file, `.gltf` (JSON) or `.glb` (the
//! binary container), into groups:
//!
//! * **Data**: buffers from the `.glb`'s binary chunk, from files beside
//!   the `.gltf`, or from `data:` URIs (base64); accessors of every
//!   component type, normalized or not, interleaved by `byteStride`, and
//!   sparse ones.
//! * **Units and axes**: glTF is metres, Y up. With From File it comes in
//!   as metres; else the Units chosen. Positions and node transforms are
//!   moved into model millimetres, Z up, as they are read.
//! * **Scene**: the default scene's nodes (else the first scene's, else
//!   every node no other node holds) become groups named after them, at
//!   their `matrix` or translation, rotation and scale, nested as in the
//!   file. A mesh that several nodes show is made once as a component and
//!   copied into each.
//! * **Primitives**: triangles, strips and fans become faces, tidied as
//!   the window chooses; lines, line loops and line strips become edges;
//!   points are counted. Primitives compressed by Draco or meshopt cannot
//!   be read and are counted. A skinned or morphed mesh comes in as it
//!   stands, unposed.
//! * **Materials**: the base colour and its image (PNG or JPEG, from a
//!   file, a `data:` URI or a buffer), metallic when the metallic factor is
//!   0.5 or more, and the colour's alpha as opacity when the material
//!   blends.
//! * Cameras and lights are counted and skipped; so is everything else.

use std::collections::HashMap;
use std::path::Path;

use glam::{DMat3, DMat4, DQuat, DVec2, DVec3};
use gltf_json as gj;
use gltf_json::validation::Checked::Valid;
use smith_model::{Material, Texture};

use super::{ImportGroup, ImportOptions, Imported, Polygon, split, url_path};
use crate::error::IoError;

/// Reads the glTF `path`.
pub(super) fn read_gltf(path: &Path, options: &ImportOptions) -> Result<Imported, IoError> {
    let bytes = std::fs::read(path)?;
    let (json, bin) = if bytes.starts_with(b"glTF") {
        crate::glb::read(&bytes)?
    } else {
        (bytes.as_slice(), &[][..])
    };
    let root: gj::Root = serde_json::from_slice(json)
        .map_err(|e| IoError::Damaged(format!("this is not a glTF file ({e})")))?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Import".into());
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut reader = Gltf::new(&root, dir, bin, options)?;
    let mut groups = Vec::new();
    for n in reader.scene_nodes() {
        if let Some(g) = reader.node(n, 0, DMat4::IDENTITY, DMat4::IDENTITY) {
            groups.push(reader.shells(g));
        }
    }
    Ok(Imported {
        name: stem,
        groups,
        components: reader.components,
        merge_coplanar: options.merge_coplanar,
        soften: options.soften,
        materials: reader.materials,
        points: reader.points,
        missing: reader.missing,
        skipped: reader.skipped,
        compressed: reader.compressed.len(),
        ..Imported::default()
    })
}

/// Reading one file: its data, and what has been gathered.
struct Gltf<'a> {
    root: &'a gj::Root,
    dir: &'a Path,
    options: &'a ImportOptions,
    /// Each buffer's bytes (`None`: no data, a compressed file's stand-in).
    buffers: Vec<Option<Vec<u8>>>,
    /// File coordinates to model mm, Z up.
    to_model: DMat4,
    /// Each glTF material's material index, read the first time.
    material_index: HashMap<usize, Option<usize>>,
    materials: Vec<Material>,
    missing: Vec<String>,
    /// How many nodes in the scene show each mesh.
    uses: HashMap<usize, usize>,
    components: Vec<ImportGroup>,
    /// The component made from each shared mesh.
    component_index: HashMap<usize, usize>,
    /// The scale built into each component.
    component_bake: Vec<DMat4>,
    /// Meshes with a compressed primitive.
    compressed: Vec<usize>,
    points: usize,
    skipped: usize,
}

/// The deepest nesting followed, so a file cannot loop forever.
const MAX_DEPTH: usize = 64;

/// Extensions that compress a primitive's data, which cannot be read.
const DRACO: &str = "KHR_draco_mesh_compression";
const MESHOPT: [&str; 2] = ["EXT_meshopt_compression", "KHR_meshopt_compression"];

impl<'a> Gltf<'a> {
    fn new(
        root: &'a gj::Root,
        dir: &'a Path,
        bin: &[u8],
        options: &'a ImportOptions,
    ) -> Result<Self, IoError> {
        let mut buffers = Vec::with_capacity(root.buffers.len());
        for (i, b) in root.buffers.iter().enumerate() {
            buffers.push(match b.uri.as_deref() {
                None if i == 0 && !bin.is_empty() => Some(bin.to_vec()),
                None => None,
                Some(uri) if uri.starts_with("data:") => data_uri(uri),
                Some(uri) => {
                    let file = url_path(uri);
                    Some(std::fs::read(dir.join(&file)).map_err(|e| {
                        IoError::Os(std::io::Error::new(
                            e.kind(),
                            format!("its data file {file} could not be read"),
                        ))
                    })?)
                }
            });
        }
        let k = if options.from_file {
            1000.0
        } else {
            options.mm_per_unit()
        };
        // The file's right, up and in directions onto the model's X, Z, −Y.
        let turn = DMat3::from_cols(DVec3::X, DVec3::Z, -DVec3::Y);
        Ok(Self {
            root,
            dir,
            options,
            buffers,
            to_model: DMat4::from_mat3(turn * k),
            material_index: HashMap::new(),
            materials: Vec::new(),
            missing: Vec::new(),
            uses: HashMap::new(),
            components: Vec::new(),
            component_index: HashMap::new(),
            component_bake: Vec::new(),
            compressed: Vec::new(),
            points: 0,
            skipped: 0,
        })
    }

    /// The nodes the file shows, counting each mesh's uses on the way.
    fn scene_nodes(&mut self) -> Vec<usize> {
        let root = self.root;
        let scene = root
            .scene
            .map(|s| s.value())
            .or((!root.scenes.is_empty()).then_some(0))
            .and_then(|s| root.scenes.get(s));
        let tops: Vec<usize> = match scene {
            Some(s) => s.nodes.iter().map(|n| n.value()).collect(),
            None => {
                // No scene: every node no other node holds.
                let held: Vec<usize> = root
                    .nodes
                    .iter()
                    .flat_map(|n| n.children.iter().flatten().map(|c| c.value()))
                    .collect();
                (0..root.nodes.len())
                    .filter(|i| !held.contains(i))
                    .collect()
            }
        };
        let mut stack: Vec<(usize, usize)> = tops.iter().map(|&n| (n, 0)).collect();
        while let Some((n, depth)) = stack.pop() {
            let Some(node) = root.nodes.get(n) else {
                continue;
            };
            if depth > MAX_DEPTH {
                continue;
            }
            if let Some(m) = node.mesh {
                *self.uses.entry(m.value()).or_default() += 1;
            }
            for c in node.children.iter().flatten() {
                stack.push((c.value(), depth + 1));
            }
        }
        tops
    }

    /// A file-space transform as the model sees it.
    fn model_transform(&self, file: DMat4) -> DMat4 {
        self.to_model * file * self.to_model.inverse()
    }

    /// A node's own transform in file space.
    fn file_transform(node: &gj::Node) -> DMat4 {
        if let Some(m) = node.matrix {
            return DMat4::from_cols_array(&m.map(f64::from));
        }
        let t = node
            .translation
            .map_or(DVec3::ZERO, |v| v.map(f64::from).into());
        let r = node.rotation.map_or(DQuat::IDENTITY, |q| {
            let [x, y, z, w] = q.0.map(f64::from);
            DQuat::from_xyzw(x, y, z, w).normalize()
        });
        let s = node.scale.map_or(DVec3::ONE, |v| v.map(f64::from).into());
        DMat4::from_scale_rotation_translation(s, r, t)
    }

    /// Node `n` as a group, or `None` when it holds nothing. `above` is
    /// where the node's parent sits in the model, `placed` the parent
    /// group's placement: a group is placed by its node's position and
    /// turn only, its scale (and any mirror) built into its geometry.
    fn node(&mut self, n: usize, depth: usize, above: DMat4, placed: DMat4) -> Option<ImportGroup> {
        let node = self.root.nodes.get(n)?;
        if depth > MAX_DEPTH {
            return None;
        }
        let world = above * self.model_transform(Self::file_transform(node));
        let at = rigid(world);
        let bake = at.inverse() * world;
        let mesh = node.mesh.map(|m| m.value());
        let mut g = ImportGroup {
            name: node
                .name
                .clone()
                .or_else(|| mesh.and_then(|m| self.root.meshes.get(m)?.name.clone()))
                .unwrap_or_else(|| format!("Node {}", n + 1)),
            transform: placed.inverse() * at,
            ..ImportGroup::default()
        };
        if node.camera.is_some() {
            self.skipped += 1;
        }
        if node
            .extensions
            .as_ref()
            .is_some_and(|e| e.others.contains_key("KHR_lights_punctual"))
        {
            self.skipped += 1;
        }
        if let Some(m) = mesh {
            if self.uses.get(&m).copied().unwrap_or(0) > 1 {
                let k = self.component(m, bake);
                let name = self.components[k].name.clone();
                let copy = bake * self.component_bake[k].inverse();
                g.uses.push((k, copy, name, None));
            } else {
                self.mesh(m, &mut g, bake);
            }
        }
        for c in node.children.iter().flatten() {
            if let Some(child) = self.node(c.value(), depth + 1, world, at) {
                let child = self.shells(child);
                g.children.push(child);
            }
        }
        let holds = !g.polygons.is_empty()
            || !g.lines.is_empty()
            || !g.children.is_empty()
            || !g.uses.is_empty();
        holds.then_some(g)
    }

    /// `g` split into its connected pieces, with Separate Shells.
    fn shells(&self, g: ImportGroup) -> ImportGroup {
        if self.options.separate_shells && !g.polygons.is_empty() {
            split(g)
        } else {
            g
        }
    }

    /// The component made from mesh `m`, made the first time with the
    /// scale `bake` built in (its first copy then needs none).
    fn component(&mut self, m: usize, bake: DMat4) -> usize {
        if let Some(k) = self.component_index.get(&m) {
            return *k;
        }
        let mut g = ImportGroup {
            name: self
                .root
                .meshes
                .get(m)
                .and_then(|x| x.name.clone())
                .unwrap_or_else(|| format!("Mesh {}", m + 1)),
            ..ImportGroup::default()
        };
        self.mesh(m, &mut g, bake);
        let g = self.shells(g);
        let k = self.components.len();
        self.components.push(g);
        self.component_bake.push(bake);
        self.component_index.insert(m, k);
        k
    }

    /// Whether `prim` is compressed (its data cannot be read).
    fn is_compressed(&self, prim: &gj::mesh::Primitive) -> bool {
        if prim
            .extensions
            .as_ref()
            .is_some_and(|e| e.others.contains_key(DRACO))
        {
            return true;
        }
        let views = prim
            .attributes
            .values()
            .chain(prim.indices.iter())
            .filter_map(|a| self.root.accessors.get(a.value())?.buffer_view)
            .filter_map(|v| self.root.buffer_views.get(v.value()));
        views.into_iter().any(|v| {
            v.extensions
                .as_ref()
                .is_some_and(|e| MESHOPT.iter().any(|x| e.others.contains_key(*x)))
        })
    }

    /// Adds mesh `m`'s primitives to `g`, moved by `bake` (a mirror turns
    /// the faces' winding back).
    fn mesh(&mut self, m: usize, g: &mut ImportGroup, bake: DMat4) {
        let mirror = bake.determinant() < 0.0;
        let place = bake * self.to_model;
        let Some(mesh) = self.root.meshes.get(m) else {
            return;
        };
        for prim in &mesh.primitives {
            if self.is_compressed(prim) {
                if !self.compressed.contains(&m) {
                    self.compressed.push(m);
                }
                continue;
            }
            let Some(positions) = prim
                .attributes
                .get(&Valid(gj::mesh::Semantic::Positions))
                .and_then(|a| self.accessor(a.value()))
            else {
                continue;
            };
            let positions: Vec<DVec3> = positions
                .iter()
                .map(|v| {
                    let at = |i: usize| v.get(i).copied().unwrap_or(0.0);
                    place.transform_point3(DVec3::new(at(0), at(1), at(2)))
                })
                .collect();
            let indices: Vec<usize> = match prim.indices {
                Some(a) => match self.accessor(a.value()) {
                    Some(v) => v.iter().map(|x| x[0] as usize).collect(),
                    None => continue,
                },
                None => (0..positions.len()).collect(),
            };
            let material = prim.material.and_then(|x| self.material(x.value()));
            // The texture coordinates the material's image uses; glTF's v
            // runs down the image, the model's up.
            let set = prim
                .material
                .and_then(|x| self.root.materials.get(x.value()))
                .and_then(|x| x.pbr_metallic_roughness.base_color_texture.as_ref())
                .map_or(0, |t| t.tex_coord);
            let uvs: Option<Vec<DVec2>> = material
                .filter(|&x| self.materials[x].texture.is_some())
                .and_then(|_| {
                    prim.attributes
                        .get(&Valid(gj::mesh::Semantic::TexCoords(set)))
                })
                .and_then(|a| self.accessor(a.value()))
                .map(|v| {
                    v.iter()
                        .map(|t| DVec2::new(t[0], -t.get(1).copied().unwrap_or(0.0)))
                        .collect()
                });
            let triangle = |[a, b, c]: [usize; 3], g: &mut ImportGroup| {
                let c = if mirror { [a, c, b] } else { [a, b, c] };
                let Some(points) = c
                    .iter()
                    .map(|&i| positions.get(*indices.get(i)?).copied())
                    .collect::<Option<Vec<DVec3>>>()
                else {
                    return;
                };
                let uv = uvs.as_ref().and_then(|u| {
                    c.iter()
                        .map(|&i| u.get(*indices.get(i)?).copied())
                        .collect::<Option<Vec<DVec2>>>()
                });
                g.polygons.push(Polygon {
                    points,
                    uvs: uv,
                    material,
                    ..Polygon::default()
                });
            };
            let point = |i: usize| positions.get(*indices.get(i)?).copied();
            let n = indices.len();
            use gj::mesh::Mode;
            match prim.mode {
                Valid(Mode::Triangles) => {
                    for t in 0..n / 3 {
                        triangle([3 * t, 3 * t + 1, 3 * t + 2], g);
                    }
                }
                Valid(Mode::TriangleStrip) => {
                    // Strips alternate winding.
                    for t in 2..n {
                        if t % 2 == 0 {
                            triangle([t - 2, t - 1, t], g);
                        } else {
                            triangle([t - 1, t - 2, t], g);
                        }
                    }
                }
                Valid(Mode::TriangleFan) => {
                    for t in 2..n {
                        triangle([0, t - 1, t], g);
                    }
                }
                Valid(Mode::Lines) => {
                    for l in 0..n / 2 {
                        if let (Some(a), Some(b)) = (point(2 * l), point(2 * l + 1)) {
                            g.lines.push([a, b]);
                        }
                    }
                }
                Valid(Mode::LineStrip | Mode::LineLoop) => {
                    let pts: Vec<DVec3> = (0..n).filter_map(point).collect();
                    g.lines.extend(pts.windows(2).map(|w| [w[0], w[1]]));
                    if prim.mode == Valid(Mode::LineLoop)
                        && pts.len() > 2
                        && let (Some(a), Some(b)) = (pts.last(), pts.first())
                    {
                        g.lines.push([*a, *b]);
                    }
                }
                Valid(Mode::Points) => self.points += n,
                _ => {}
            }
        }
    }

    /// The material for glTF material `i`, read the first time.
    fn material(&mut self, i: usize) -> Option<usize> {
        if let Some(m) = self.material_index.get(&i) {
            return *m;
        }
        let made = self.read_material(i);
        self.material_index.insert(i, made);
        made
    }

    fn read_material(&mut self, i: usize) -> Option<usize> {
        let mat = self.root.materials.get(i)?;
        let pbr = &mat.pbr_metallic_roughness;
        let [r, g, b, a] = pbr.base_color_factor.0;
        let texture = pbr
            .base_color_texture
            .as_ref()
            .and_then(|t| self.texture(t.index.value()));
        let opacity = if mat.alpha_mode == Valid(gj::material::AlphaMode::Blend) {
            a.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.materials.push(Material {
            name: mat
                .name
                .clone()
                .unwrap_or_else(|| format!("Material {}", i + 1)),
            color: [r, g, b].map(|x| x.clamp(0.0, 1.0)),
            opacity,
            metallic: pbr.metallic_factor.0 >= 0.5,
            texture,
        });
        Some(self.materials.len() - 1)
    }

    /// The image of glTF texture `t`, loaded; a name that fails is noted.
    fn texture(&mut self, t: usize) -> Option<Texture> {
        let image_index = self.root.textures.get(t)?.source.value();
        let image = self.root.images.get(image_index)?;
        let (name, bytes) = match (&image.uri, image.buffer_view) {
            (Some(uri), _) if uri.starts_with("data:") => {
                (format!("Image {}", image_index + 1), data_uri(uri))
            }
            (Some(uri), _) => {
                let file = url_path(uri).replace('\\', "/");
                let found = [
                    Some(self.dir.join(&file)),
                    Path::new(&file).file_name().map(|f| self.dir.join(f)),
                ]
                .into_iter()
                .flatten()
                .find_map(|p| std::fs::read(p).ok());
                (file, found)
            }
            (None, Some(v)) => (
                format!("Image {}", image_index + 1),
                self.view(v.value()).map(|(b, _)| b.to_vec()),
            ),
            (None, None) => return None,
        };
        let name = image.name.clone().unwrap_or(name);
        let loaded = bytes.and_then(|b| crate::textures::image_from_bytes(&name, b).ok());
        if loaded.is_none() && !self.missing.contains(&name) {
            self.missing.push(name);
        }
        // The repeat size is set once a face shows it.
        loaded.map(|image| Texture {
            image: std::sync::Arc::new(image),
            width_mm: 100.0,
        })
    }

    /// Buffer view `v`'s bytes and its stride.
    fn view(&self, v: usize) -> Option<(&[u8], Option<usize>)> {
        let view = self.root.buffer_views.get(v)?;
        let buffer = self.buffers.get(view.buffer.value())?.as_deref()?;
        let from = view.byte_offset.map_or(0, |o| o.0 as usize);
        let bytes = buffer.get(from..from.checked_add(view.byte_length.0 as usize)?)?;
        Some((bytes, view.byte_stride.map(|s| s.0)))
    }

    /// Accessor `a`'s values, one entry per element, as numbers
    /// (normalized integers scaled into 0–1 or −1–1).
    fn accessor(&self, a: usize) -> Option<Vec<Vec<f64>>> {
        let acc = self.root.accessors.get(a)?;
        let Valid(kind) = acc.type_ else { return None };
        let Valid(gj::accessor::GenericComponentType(ct)) = acc.component_type else {
            return None;
        };
        let n = kind.multiplicity();
        let count = acc.count.0 as usize;
        let mut out = vec![vec![0.0; n]; count];
        if let Some(v) = acc.buffer_view {
            let (bytes, stride) = self.view(v.value())?;
            let offset = acc.byte_offset.map_or(0, |o| o.0 as usize);
            let stride = stride.unwrap_or(n * ct.size());
            for (i, e) in out.iter_mut().enumerate() {
                let at = offset + i * stride;
                for (c, x) in e.iter_mut().enumerate() {
                    *x = component(bytes, at + c * ct.size(), ct, acc.normalized)?;
                }
            }
        }
        if let Some(s) = &acc.sparse {
            let Valid(gj::accessor::IndexComponentType(it)) = s.indices.component_type else {
                return None;
            };
            let (ib, _) = self.view(s.indices.buffer_view.value())?;
            let (vb, _) = self.view(s.values.buffer_view.value())?;
            let (io, vo) = (
                s.indices.byte_offset.0 as usize,
                s.values.byte_offset.0 as usize,
            );
            for k in 0..s.count.0 as usize {
                let i = component(ib, io + k * it.size(), it, false)? as usize;
                let e = out.get_mut(i)?;
                for (c, x) in e.iter_mut().enumerate() {
                    *x = component(vb, vo + (k * n + c) * ct.size(), ct, acc.normalized)?;
                }
            }
        }
        Some(out)
    }
}

/// The position and turn of `m` (no scale, no mirror): its axes made
/// square to each other and unit length, right-handed.
fn rigid(m: DMat4) -> DMat4 {
    let x = m.x_axis.truncate().normalize_or(DVec3::X);
    let y = m.y_axis.truncate();
    let y = (y - x * x.dot(y)).normalize_or(x.any_orthonormal_vector());
    let z = x.cross(y);
    DMat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), m.w_axis)
}

/// One component of type `ct` at `at` in `bytes`, little-endian.
fn component(
    bytes: &[u8],
    at: usize,
    ct: gj::accessor::ComponentType,
    normalized: bool,
) -> Option<f64> {
    use gj::accessor::ComponentType as C;
    let b = bytes.get(at..at + ct.size())?;
    Some(match ct {
        C::I8 => {
            let v = f64::from(b[0] as i8);
            if normalized { (v / 127.0).max(-1.0) } else { v }
        }
        C::U8 => {
            let v = f64::from(b[0]);
            if normalized { v / 255.0 } else { v }
        }
        C::I16 => {
            let v = f64::from(i16::from_le_bytes([b[0], b[1]]));
            if normalized {
                (v / 32767.0).max(-1.0)
            } else {
                v
            }
        }
        C::U16 => {
            let v = f64::from(u16::from_le_bytes([b[0], b[1]]));
            if normalized { v / 65535.0 } else { v }
        }
        C::U32 => f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        C::F32 => f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
    })
}

/// The bytes of a base64 `data:` URI; `None` when it is not base64.
fn data_uri(uri: &str) -> Option<Vec<u8>> {
    let (_, data) = uri.split_once(";base64,")?;
    base64(data)
}

/// Standard base64 (RFC 4648 §4) decoded, white space ignored, padding
/// optional; `None` on any other character.
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{ImportFormat, ImportReport, place, read_import};
    use glam::DMat4;
    use smith_model::{Document, InstanceId, Mapping, ModelError, Paint, TextureFrame};

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("smith-io-gltf-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn options() -> ImportOptions {
        ImportOptions::for_format(ImportFormat::Gltf)
    }

    fn placed(imported: &Imported) -> (Document, InstanceId, ImportReport) {
        let mut doc = Document::new();
        let (id, report) = doc
            .transaction("Import", |d| place(d, None, DMat4::IDENTITY, imported))
            .unwrap();
        (doc, id, report)
    }

    /// A 4 × 2 pixel PNG.
    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(4, 2, image::Rgba([120, 80, 40, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    /// Standard base64, padded.
    fn encode(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for c in bytes.chunks(3) {
            let n = (u32::from(c[0]) << 16)
                | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
                | u32::from(*c.get(2).unwrap_or(&0));
            for i in 0..4 {
                if i <= c.len() {
                    out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    fn f32s(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn base64_decodes() {
        assert_eq!(base64("TWFu").unwrap(), b"Man");
        assert_eq!(base64("TWE=").unwrap(), b"Ma");
        assert_eq!(base64("TQ==\n").unwrap(), b"M");
        assert!(base64("T*E=").is_none());
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(base64(&encode(&bytes)).unwrap(), bytes);
    }

    /// A textured box of 50 × 30 × 20 mm.
    fn textured_box() -> Document {
        let mut doc = Document::new();
        let image = crate::textures::image_from_bytes("oak.png", png()).unwrap();
        let wood = doc
            .add_material(Material {
                name: "Oak".into(),
                texture: Some(Texture {
                    image: std::sync::Arc::new(image),
                    width_mm: 40.0,
                }),
                ..Material::default()
            })
            .unwrap();
        doc.transaction("box", |dd| {
            let r = dd.root_mut();
            let c = [
                DVec3::ZERO,
                DVec3::new(50.0, 0.0, 0.0),
                DVec3::new(50.0, 30.0, 0.0),
                DVec3::new(0.0, 30.0, 0.0),
            ];
            let up = DVec3::new(0.0, 0.0, 20.0);
            for i in 0..4 {
                let (a, b) = (c[i], c[(i + 1) % 4]);
                r.add_line(a, b)?;
                r.add_line(a + up, b + up)?;
                r.add_line(a, a + up)?;
            }
            let f = r.face_ids().next().ok_or(ModelError::NoSuchEntity)?;
            let n = r.face(f).map_or(DVec3::Z, |x| x.plane().normal());
            r.set_face_paint(
                f,
                smith_model::Side::Front,
                Some(Paint {
                    material: wood,
                    mapping: Mapping::Face(TextureFrame::for_side(n, DVec2::new(3.0, 4.0), 0.4)),
                }),
            )
        })
        .unwrap();
        doc
    }

    /// Every face and the size of `path` imported.
    fn faces_and_size(path: &Path) -> (Document, ImportReport) {
        let imported = read_import(path, ImportFormat::Gltf, &options()).unwrap();
        let (doc, _, report) = placed(&imported);
        assert_eq!(report.counts.faces - report.counts.merged, 6);
        let (lo, hi) = doc.model_bounds().unwrap();
        assert!((hi - lo - DVec3::new(50.0, 30.0, 20.0)).length() < 1e-3);
        (doc, report)
    }

    #[test]
    fn our_own_glb_comes_back_the_same_size() {
        let d = dir("own-glb");
        let path = d.join("box.glb");
        let bytes = crate::write_parts(&textured_box(), None, None, true).unwrap();
        std::fs::write(&path, bytes).unwrap();
        let (doc, report) = faces_and_size(&path);
        // The viewer layer draws the painted face's back first, reversed:
        // its Oak front comes in as that face's back, laid exactly.
        assert_eq!(report.counts.repeated, 0);
        assert_eq!(report.approximate, 0);
        let oak = doc
            .material_ids()
            .find(|m| doc.material(*m).is_some_and(|x| x.name == "Oak"))
            .unwrap();
        let backs: usize = doc
            .definition_ids()
            .filter_map(|d| doc.definition_entities(d))
            .map(|e| {
                e.face_ids()
                    .filter(|f| {
                        e.face_paint(*f, smith_model::Side::Back)
                            .is_some_and(|p| p.material == oak)
                    })
                    .count()
            })
            .sum();
        assert_eq!(backs, 1);
        let w = doc
            .material(oak)
            .unwrap()
            .texture
            .as_ref()
            .unwrap()
            .width_mm;
        assert!((w - 40.0).abs() < 1e-3);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn our_gltf_export_comes_back_the_same() {
        let d = dir("own-gltf");
        let path = d.join("box.gltf");
        crate::export::write_gltf(&path, &textured_box(), crate::export::Scope::Model).unwrap();
        let (doc, report) = faces_and_size(&path);
        // The file's metre scale is built into the geometry: no group is
        // scaled.
        let inside = doc
            .definition_ids()
            .filter_map(|d| doc.definition_entities(d));
        for e in std::iter::once(doc.root()).chain(inside) {
            for i in e.instance_ids() {
                let t = e.instance(i).unwrap().transform;
                assert!((t.determinant() - 1.0).abs() < 1e-9);
            }
        }
        // The texture is laid exactly, at its own repeat size.
        assert_eq!(report.approximate, 0);
        // The plain face colour comes too, as "Face".
        let m = doc
            .material_ids()
            .filter_map(|m| doc.material(m))
            .find(|m| m.name == "Oak")
            .unwrap();
        assert!((m.texture.as_ref().unwrap().width_mm - 40.0).abs() < 1e-3);
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// A hand-written file: a "Shelf" holding two legs that share the
    /// "Square" mesh (one plain, one turned, stretched and moved), and a
    /// "Panel" up 3 m by its matrix: a see-through red square, a line,
    /// three points and a Draco-compressed primitive; and a camera.
    fn shelf() -> String {
        let mut bin = f32s(&[0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.]);
        bin.extend([0u16, 1, 2, 0, 2, 3].iter().flat_map(|i| i.to_le_bytes()));
        bin.extend(f32s(&[0., 0., 0., -1., 0., 0.]));
        bin.extend(f32s(&[5., 5., 5., 6., 6., 6., 7., 7., 7.]));
        let uri = format!("data:application/octet-stream;base64,{}", encode(&bin));
        let s = std::f32::consts::FRAC_1_SQRT_2;
        format!(
            r#"{{
  "asset": {{"version": "2.0"}},
  "scene": 0,
  "scenes": [{{"nodes": [0, 3, 4]}}],
  "nodes": [
    {{"name": "Shelf", "children": [1, 2], "translation": [2, 0, 0]}},
    {{"name": "Leg A", "mesh": 0}},
    {{"name": "Leg B", "mesh": 0, "translation": [0, 0, -1], "rotation": [0, {s}, 0, {s}], "scale": [1, 2, 1]}},
    {{"name": "Panel", "mesh": 1, "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,3,0,1]}},
    {{"camera": 0}}
  ],
  "cameras": [{{"type": "perspective", "perspective": {{"yfov": 0.8, "znear": 0.1}}}}],
  "meshes": [
    {{"name": "Square", "primitives": [{{"attributes": {{"POSITION": 0}}, "indices": 1, "material": 1}}]}},
    {{"name": "Panel", "primitives": [
      {{"attributes": {{"POSITION": 0}}, "indices": 1, "material": 0}},
      {{"attributes": {{"POSITION": 2}}, "mode": 1}},
      {{"attributes": {{"POSITION": 3}}, "mode": 0}},
      {{"attributes": {{"POSITION": 4}}, "extensions": {{"KHR_draco_mesh_compression": {{"bufferView": 0, "attributes": {{"POSITION": 0}}}}}}}}
    ]}}
  ],
  "materials": [
    {{"name": "Red Glass", "alphaMode": "BLEND", "pbrMetallicRoughness": {{"baseColorFactor": [0.8, 0.1, 0.1, 0.5], "metallicFactor": 0}}}},
    {{"name": "Steel", "pbrMetallicRoughness": {{"baseColorFactor": [0.5, 0.5, 0.5, 1], "metallicFactor": 1}}}}
  ],
  "buffers": [{{"byteLength": {len}, "uri": "{uri}"}}],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": 48}},
    {{"buffer": 0, "byteOffset": 48, "byteLength": 12}},
    {{"buffer": 0, "byteOffset": 60, "byteLength": 24}},
    {{"buffer": 0, "byteOffset": 84, "byteLength": 36}}
  ],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3"}},
    {{"bufferView": 1, "componentType": 5123, "count": 6, "type": "SCALAR"}},
    {{"bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC3"}},
    {{"bufferView": 3, "componentType": 5126, "count": 3, "type": "VEC3"}},
    {{"componentType": 5126, "count": 100, "type": "VEC3"}}
  ],
  "extensionsUsed": ["KHR_draco_mesh_compression"]
}}"#,
            len = bin.len()
        )
    }

    #[test]
    fn a_hand_written_gltf_comes_in_whole() {
        let d = dir("shelf");
        let path = d.join("shelf.gltf");
        std::fs::write(&path, shelf()).unwrap();
        let imported = read_import(&path, ImportFormat::Gltf, &options()).unwrap();
        assert_eq!(imported.components.len(), 1);
        let (doc, _, report) = placed(&imported);
        assert_eq!(report.groups, 2);
        assert_eq!(
            report.summary("shelf.gltf"),
            "Imported shelf.gltf: 2 Faces, 2 Objects"
        );
        assert_eq!(
            report.lines(),
            [
                "2 Triangles Merged",
                "3 Points Skipped",
                "1 Line Imported as Edges",
                "1 Camera or Light Skipped",
                "1 Compressed Mesh Skipped",
            ]
        );
        // Metres to mm, Y up to Z up: the panel's top is 4 m up; the turned
        // leg runs 1–2 m into +Y (glTF's −Z) and is 2 m tall.
        let (lo, hi) = doc.model_bounds().unwrap();
        assert!(lo.z.abs() < 1e-6 && (hi.z - 4000.0).abs() < 1e-3);
        assert!(lo.y.abs() < 1e-3 && (hi.y - 2000.0).abs() < 1e-3);
        let mats: Vec<&Material> = doc.material_ids().filter_map(|m| doc.material(m)).collect();
        let glass = mats.iter().find(|m| m.name == "Red Glass").unwrap();
        assert!((glass.opacity - 0.5).abs() < 1e-6 && !glass.metallic);
        let steel = mats.iter().find(|m| m.name == "Steel").unwrap();
        assert!(steel.metallic && (steel.opacity - 1.0).abs() < 1e-6);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_foreign_glb_with_its_image_inside() {
        let d = dir("glb");
        let image = png();
        let mut bin = f32s(&[0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.]);
        // The 4 × 2 image's own proportions: two repeats up the tile.
        bin.extend(f32s(&[0., 0., 1., 0., 1., -2., 0., -2.]));
        bin.extend([0u16, 1, 2, 0, 2, 3].iter().flat_map(|i| i.to_le_bytes()));
        let at = bin.len();
        bin.extend(&image);
        let json = format!(
            r#"{{
  "asset": {{"version": "2.0", "generator": "Another Program"}},
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"name": "Tile", "mesh": 0}}],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0, "TEXCOORD_0": 1}}, "indices": 2, "material": 0}}]}}],
  "materials": [{{"name": "Brick", "pbrMetallicRoughness": {{"baseColorTexture": {{"index": 0}}}}}}],
  "textures": [{{"source": 0}}],
  "images": [{{"bufferView": 3, "mimeType": "image/png"}}],
  "buffers": [{{"byteLength": {total}}}],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": 48}},
    {{"buffer": 0, "byteOffset": 48, "byteLength": 32}},
    {{"buffer": 0, "byteOffset": 80, "byteLength": 12}},
    {{"buffer": 0, "byteOffset": {at}, "byteLength": {n}}}
  ],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3"}},
    {{"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC2"}},
    {{"bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR"}}
  ]
}}"#,
            total = bin.len(),
            n = image.len()
        );
        let bytes = crate::glb::write(json.as_bytes(), &bin).unwrap();
        // Open finds no model in it: the shell offers to import it instead.
        assert!(matches!(crate::read_glb(&bytes), Err(IoError::Foreign)));
        let path = d.join("tile.glb");
        std::fs::write(&path, &bytes).unwrap();
        let imported = read_import(&path, ImportFormat::Gltf, &options()).unwrap();
        let (doc, _, report) = placed(&imported);
        assert_eq!(
            report.summary("tile.glb"),
            "Imported tile.glb: 1 Face, 1 Object"
        );
        assert!(report.missing.is_empty());
        assert_eq!(report.approximate, 0);
        let m = doc.material(doc.material_ids().next().unwrap()).unwrap();
        let t = m.texture.as_ref().unwrap();
        assert_eq!(t.image.pixels, [4, 2]);
        // One repeat across the 1 m tile.
        assert!((t.width_mm - 1000.0).abs() < 1e-3);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_missing_data_file_is_named() {
        let d = dir("missing");
        let path = d.join("lost.gltf");
        std::fs::write(
            &path,
            r#"{"asset": {"version": "2.0"}, "buffers": [{"byteLength": 4, "uri": "lost%20data.bin"}]}"#,
        )
        .unwrap();
        let e = read_import(&path, ImportFormat::Gltf, &options()).unwrap_err();
        assert_eq!(
            e.to_string(),
            "its data file lost data.bin could not be read"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }
}
