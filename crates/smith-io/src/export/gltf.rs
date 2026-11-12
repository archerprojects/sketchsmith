//! SketchSmith — plain glTF export | v1.0.0 | Thursday 12 November 2026
//!
//! The model as plain glTF 2.0 for Blender and viewers: `name.gltf` (JSON),
//! `name.bin` beside it, and the texture images in `name_textures/` (only
//! when there are textures), all by relative paths, as Blender's glTF
//! Separate form. It is the native file's viewer layer without the
//! SketchSmith extension: one mesh per group or component definition shared
//! by its copies, metres, Y up, the Present-mode look. Each face is drawn
//! once, double-sided, with the front's paint, else the back's; no edges
//! and no camera. glTF flips the winding of a mirrored copy itself.

use std::cell::RefCell;
use std::path::Path;

use smith_model::{Document, ImageFormat, TextureImage};

use super::{Scope, clear_textures, safe_name, write_file};
use crate::error::IoError;

/// The files of one glTF export, before they are written.
pub(crate) struct GltfFiles {
    /// The `.gltf` JSON.
    pub json: Vec<u8>,
    /// The `.bin` data.
    pub bin: Vec<u8>,
    /// Each texture image: file name in the textures folder, and its bytes.
    pub images: Vec<(String, Vec<u8>)>,
}

/// Writes what `scope` exports from `doc` as `path` (a `.gltf`), with its
/// `.bin` and textures folder. Files left by an earlier export of the same
/// name are replaced.
///
/// # Errors
/// [`IoError::Os`] when a file cannot be written; [`IoError::TooLarge`].
pub fn write_gltf(path: &Path, doc: &Document, scope: Scope<'_>) -> Result<(), IoError> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".into());
    let files = gltf_files(&stem, doc, scope)?;
    let textures = path.with_file_name(textures_folder(&stem));
    clear_textures(&textures)?;
    if !files.images.is_empty() {
        std::fs::create_dir_all(&textures)?;
        for (name, bytes) in &files.images {
            write_file(&textures.join(name), bytes)?;
        }
    }
    let bin = path.with_extension("bin");
    if files.bin.is_empty() {
        let _ = std::fs::remove_file(&bin);
    } else {
        write_file(&bin, &files.bin)?;
    }
    write_file(path, &files.json)
}

/// The textures folder's name for an export named `stem`.
fn textures_folder(stem: &str) -> String {
    format!("{stem}_textures")
}

/// Builds the glTF, binary data and images for an export named `stem`.
pub(crate) fn gltf_files(
    stem: &str,
    doc: &Document,
    scope: Scope<'_>,
) -> Result<GltfFiles, IoError> {
    // Each image is named after the file it was loaded from, made safe and
    // unique.
    let names: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let image_uri = |_: usize, image: &TextureImage| {
        let base = safe_name(
            Path::new(&image.name)
                .file_stem()
                .map(|s| s.to_string_lossy())
                .as_deref()
                .unwrap_or(""),
        );
        let ext = match image.format {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
        };
        let mut names = names.borrow_mut();
        let mut file = format!("{base}.{ext}");
        let mut n = 1;
        while names.contains(&file) {
            n += 1;
            file = format!("{base}_{n}.{ext}");
        }
        names.push(file.clone());
        format!("{}/{file}", textures_folder(stem))
    };
    let (json, bin, images) =
        crate::native::plain_gltf(doc, scope, stem, &format!("{stem}.bin"), &image_uri)?;
    let images = names
        .into_inner()
        .into_iter()
        .zip(images.iter().map(|i| i.bytes.clone()))
        .collect();
    Ok(GltfFiles { json, bin, images })
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{DMat4, DVec2, DVec3};
    use gltf_json as gj;
    use smith_model::{DefinitionKind, Mapping, Material, Paint, Side, Texture, TextureFrame};

    fn p(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    fn cube(e: &mut smith_model::Entities, s: f64) -> Result<(), smith_model::ModelError> {
        let c = [
            p(0.0, 0.0, 0.0),
            p(s, 0.0, 0.0),
            p(s, s, 0.0),
            p(0.0, s, 0.0),
        ];
        let up = p(0.0, 0.0, s);
        for i in 0..4 {
            let (a, b) = (c[i], c[(i + 1) % 4]);
            e.add_line(a, b)?;
            e.add_line(a + up, b + up)?;
            e.add_line(a, a + up)?;
        }
        Ok(())
    }

    fn root(files: &GltfFiles) -> gj::Root {
        serde_json::from_slice(&files.json).unwrap()
    }

    #[test]
    fn plain_gltf_has_no_model_data_and_shares_meshes() {
        let mut doc = Document::new();
        doc.transaction("copies", |d| {
            let def = d.create_definition("Block", DefinitionKind::Component);
            cube(
                d.definition_entities_mut(def)
                    .ok_or(smith_model::ModelError::NoSuchEntity)?,
                50.0,
            )?;
            d.add_instance(None, def, DMat4::IDENTITY, "Left")?;
            d.add_instance(None, def, DMat4::from_translation(p(80.0, 0.0, 0.0)), "")?;
            Ok(())
        })
        .unwrap();
        let files = gltf_files("pair", &doc, Scope::Model).unwrap();
        let text = String::from_utf8(files.json.clone()).unwrap();
        assert!(!text.contains(crate::native::EXTENSION));
        let r = root(&files);
        assert!(r.extensions_used.is_empty());
        assert!(r.cameras.is_empty());
        assert_eq!(r.buffers.len(), 1);
        assert_eq!(r.buffers[0].uri.as_deref(), Some("pair.bin"));
        assert_eq!(r.buffers[0].byte_length.0 as usize, files.bin.len());
        // Both copies use the one Block mesh.
        let used: Vec<u32> = r
            .nodes
            .iter()
            .filter_map(|n| n.mesh.map(|m| m.value() as u32))
            .collect();
        assert_eq!(used.len(), 2);
        assert_eq!(used[0], used[1]);
        // Each face once (12 triangles), and no edge lines.
        let mesh = &r.meshes[used[0] as usize];
        assert!(
            mesh.primitives
                .iter()
                .all(|p| p.mode == gj::validation::Checked::Valid(gj::mesh::Mode::Triangles))
        );
        let triangles: u64 = mesh
            .primitives
            .iter()
            .map(|p| r.accessors[p.indices.unwrap().value()].count.0 / 3)
            .sum();
        assert_eq!(triangles, 12);
        // The top node turns mm, Z up into metres, Y up.
        let top = &r.nodes[r.scenes[0].nodes[0].value()];
        let m = top.matrix.unwrap();
        assert!((m[0] - 0.001).abs() < 1e-9 && (m[6] + 0.001).abs() < 1e-9);
    }

    #[test]
    fn selection_only_keeps_the_picked_copy() {
        let mut doc = Document::new();
        let mut keep = None;
        doc.transaction("copies", |d| {
            let def = d.create_definition("Block", DefinitionKind::Group);
            cube(
                d.definition_entities_mut(def)
                    .ok_or(smith_model::ModelError::NoSuchEntity)?,
                10.0,
            )?;
            keep = Some(d.add_instance(None, def, DMat4::IDENTITY, "Keep")?);
            d.add_instance(
                None,
                def,
                DMat4::from_translation(p(30.0, 0.0, 0.0)),
                "Drop",
            )?;
            Ok(())
        })
        .unwrap();
        let picked = [keep.unwrap()];
        let files = gltf_files(
            "one",
            &doc,
            Scope::Selection {
                context: None,
                local: DMat4::IDENTITY,
                faces: &[],
                instances: &picked,
            },
        )
        .unwrap();
        let r = root(&files);
        let names: Vec<_> = r.nodes.iter().filter_map(|n| n.name.as_deref()).collect();
        assert!(names.contains(&"Keep"));
        assert!(!names.contains(&"Drop"));
    }

    #[test]
    fn textures_go_beside_with_their_names() {
        let mut doc = Document::new();
        let wood = doc
            .add_material(Material {
                name: "Oak".into(),
                color: [1.0, 1.0, 1.0],
                texture: Some(Texture {
                    image: std::sync::Arc::new(TextureImage {
                        name: "oak grain.jpg".into(),
                        format: ImageFormat::Jpeg,
                        bytes: vec![9, 8, 7],
                        pixels: [4, 4],
                    }),
                    width_mm: 20.0,
                }),
                ..Material::default()
            })
            .unwrap();
        doc.transaction("paint", |d| {
            cube(d.root_mut(), 10.0)?;
            let r = d.root_mut();
            let f = r
                .face_ids()
                .next()
                .ok_or(smith_model::ModelError::NoSuchEntity)?;
            r.set_face_paint(
                f,
                Side::Back,
                Some(Paint {
                    material: wood,
                    mapping: Mapping::Face(TextureFrame::for_side(DVec3::Z, DVec2::ZERO, 0.0)),
                }),
            )
        })
        .unwrap();
        let files = gltf_files("crate", &doc, Scope::Model).unwrap();
        assert_eq!(files.images, vec![("oak_grain.jpg".into(), vec![9, 8, 7])]);
        let r = root(&files);
        assert_eq!(
            r.images[0].uri.as_deref(),
            Some("crate_textures/oak_grain.jpg")
        );
        assert!(r.images[0].buffer_view.is_none());
        // Painted on the back only: still each face once, the painted one
        // double-sided with its material.
        let painted = r
            .materials
            .iter()
            .position(|m| m.name.as_deref() == Some("Oak"));
        let painted = painted.unwrap();
        assert!(r.materials[painted].double_sided);
        let triangles: u64 = r.meshes[0]
            .primitives
            .iter()
            .map(|p| r.accessors[p.indices.unwrap().value()].count.0 / 3)
            .sum();
        assert_eq!(triangles, 12);
    }

    #[test]
    fn writing_puts_the_files_beside() {
        let dir = std::env::temp_dir().join(format!("smith-io-gltf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut doc = Document::new();
        doc.transaction("cube", |d| cube(d.root_mut(), 10.0))
            .unwrap();
        write_gltf(&dir.join("box.gltf"), &doc, Scope::Model).unwrap();
        assert!(dir.join("box.gltf").exists());
        assert!(dir.join("box.bin").exists());
        assert!(!dir.join("box_textures").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
