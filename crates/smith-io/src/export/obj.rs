//! SketchSmith — OBJ export | v1.0.0 | Thursday 12 November 2026
//!
//! Wavefront OBJ for Blender: `name.obj`, `name.mtl` beside it, and the
//! texture images in `name_textures/` (only when there are textures), all
//! referred to by relative paths so the set moves together.
//!
//! * Y up (Blender's OBJ import defaults are Forward −Z, Up Y), units as
//!   chosen.
//! * One object (`o`) per part: the loose geometry, named after the file,
//!   then each group or component copy.
//! * Faces are n-gons; a face with holes, which OBJ cannot hold, is written
//!   as its triangles.
//! * Normals are the smoothed ones Present mode shows; texture coordinates
//!   are in repeats, v up.
//! * Each material: `Kd` (clamped to 0–1), `d`, `Pm` for metal (the MTL
//!   PBR extension Blender reads and writes), `map_Kd` for its image, kept
//!   byte for byte. Unpainted faces use the plain face colour.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use glam::{DVec2, DVec3};
use smith_model::{Document, MaterialId, VertexId};

use super::{
    ExportOptions, Format, Gathered, clear_textures, num, safe_name, used_materials, write_file,
    y_up,
};
use crate::error::IoError;

/// The plain face colour (linear), as the native file's viewer layer has it.
const FACE_COLOR: [f32; 3] = [0.62, 0.60, 0.56];
/// The material name for unpainted faces.
const PLAIN: &str = "SketchSmith_Face";

/// The files of one OBJ export, before they are written.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObjFiles {
    /// The `.obj` text.
    pub obj: String,
    /// The `.mtl` text.
    pub mtl: String,
    /// Each texture image: file name in the textures folder, and its bytes.
    pub images: Vec<(String, Vec<u8>)>,
}

/// Writes `gathered` from `doc` as `path` (an `.obj`), with its `.mtl` and
/// textures folder. Files left by an earlier export of the same name are
/// replaced.
///
/// # Errors
/// [`IoError::Os`] when a file cannot be written.
pub fn write_obj(
    path: &Path,
    doc: &Document,
    gathered: &Gathered,
    options: &ExportOptions,
) -> Result<(), IoError> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".into());
    let files = obj_files(&stem, doc, gathered, options);
    let mtl = path.with_extension("mtl");
    let textures = path.with_file_name(textures_folder(&stem));
    clear_textures(&textures)?;
    if !files.images.is_empty() {
        std::fs::create_dir_all(&textures)?;
        for (name, bytes) in &files.images {
            write_file(&textures.join(name), bytes)?;
        }
    }
    write_file(&mtl, files.mtl.as_bytes())?;
    write_file(path, files.obj.as_bytes())
}

/// The textures folder's name for an export named `stem`.
fn textures_folder(stem: &str) -> String {
    format!("{stem}_textures")
}

/// Builds the OBJ, MTL and images for an export named `stem`.
pub(crate) fn obj_files(
    stem: &str,
    doc: &Document,
    gathered: &Gathered,
    options: &ExportOptions,
) -> ObjFiles {
    let scale = options.units_per_mm(Format::Obj);
    let materials = used_materials(gathered);
    let names = material_names(doc, &materials);
    let plain_used = gathered
        .parts
        .iter()
        .flat_map(|p| &p.faces)
        .any(|f| f.material.is_none());

    // The MTL file, and the images it points to.
    let mut mtl = format!("# {}\n", crate::native::generator());
    let mut images = Vec::new();
    if plain_used {
        write_material(&mut mtl, PLAIN, FACE_COLOR, 1.0, false, None);
    }
    for m in &materials {
        let Some(material) = doc.material(*m) else {
            continue;
        };
        let name = &names[m];
        let map = material.texture.as_ref().map(|t| {
            let ext = match t.image.format {
                smith_model::ImageFormat::Png => "png",
                smith_model::ImageFormat::Jpeg => "jpg",
            };
            let file = format!("{name}.{ext}");
            images.push((file.clone(), t.image.bytes.clone()));
            format!("{}/{file}", textures_folder(stem))
        });
        let color = material.color.map(|c| c.clamp(0.0, 1.0));
        write_material(
            &mut mtl,
            name,
            color,
            material.opacity,
            material.metallic,
            map.as_deref(),
        );
    }

    // The OBJ file. Indices count from 1 across the whole file.
    let mut obj = format!("# {}\nmtllib {}.mtl\n", crate::native::generator(), stem);
    let (mut nv, mut nt, mut nn) = (0usize, 0usize, 0usize);
    for part in &gathered.parts {
        let name = if part.name.is_empty() {
            stem.to_owned()
        } else {
            part.name.clone()
        };
        let _ = writeln!(obj, "o {}", safe_name(&name));
        // Each vertex once per part, so faces stay joined.
        let mut vindex: BTreeMap<VertexId, usize> = BTreeMap::new();
        let mut tindex: BTreeMap<[u64; 2], usize> = BTreeMap::new();
        let mut nindex: BTreeMap<[u64; 3], usize> = BTreeMap::new();
        let mut current: Option<&str> = None;
        for face in &part.faces {
            let refs: Vec<String> = face
                .corners
                .iter()
                .map(|c| {
                    let v = *vindex.entry(c.vertex).or_insert_with(|| {
                        let p = y_up(c.point) * scale;
                        let _ = writeln!(obj, "v {} {} {}", num(p.x), num(p.y), num(p.z));
                        nv += 1;
                        nv
                    });
                    let n = *nindex.entry(bits3(c.normal)).or_insert_with(|| {
                        let q = y_up(c.normal);
                        let _ = writeln!(obj, "vn {} {} {}", num(q.x), num(q.y), num(q.z));
                        nn += 1;
                        nn
                    });
                    match c.uv {
                        Some(uv) => {
                            let t = *tindex.entry(bits2(uv)).or_insert_with(|| {
                                let _ = writeln!(obj, "vt {} {}", num(uv.x), num(uv.y));
                                nt += 1;
                                nt
                            });
                            format!("{v}/{t}/{n}")
                        }
                        None => format!("{v}//{n}"),
                    }
                })
                .collect();
            let material = face.material.map_or(PLAIN, |m| names[&m].as_str());
            if current != Some(material) {
                let _ = writeln!(obj, "usemtl {material}");
                current = Some(material);
            }
            if face.has_holes() {
                for t in &face.triangles {
                    let _ = writeln!(obj, "f {} {} {}", refs[t[0]], refs[t[1]], refs[t[2]]);
                }
            } else {
                let _ = writeln!(obj, "f {}", refs.join(" "));
            }
        }
    }
    ObjFiles { obj, mtl, images }
}

/// One `newmtl` block.
fn write_material(
    out: &mut String,
    name: &str,
    color: [f32; 3],
    opacity: f32,
    metallic: bool,
    map: Option<&str>,
) {
    let [r, g, b] = color;
    let _ = writeln!(out, "\nnewmtl {name}");
    let _ = writeln!(
        out,
        "Kd {} {} {}",
        num(r.into()),
        num(g.into()),
        num(b.into())
    );
    let _ = writeln!(out, "d {}", num(opacity.into()));
    let _ = writeln!(out, "illum 2");
    if metallic {
        let _ = writeln!(out, "Pm 1");
    }
    if let Some(map) = map {
        let _ = writeln!(out, "map_Kd {map}");
    }
}

/// A file-safe, unique MTL name for each material.
fn material_names(doc: &Document, materials: &[MaterialId]) -> BTreeMap<MaterialId, String> {
    let mut taken: Vec<String> = vec![PLAIN.to_owned()];
    let mut out = BTreeMap::new();
    for m in materials {
        let base = safe_name(doc.material(*m).map_or("", |x| x.name.as_str()));
        let mut name = base.clone();
        let mut n = 1;
        while taken.contains(&name) {
            n += 1;
            name = format!("{base}_{n}");
        }
        taken.push(name.clone());
        out.insert(*m, name);
    }
    out
}

fn bits2(v: DVec2) -> [u64; 2] {
    [v.x.to_bits(), v.y.to_bits()]
}

fn bits3(v: DVec3) -> [u64; 3] {
    [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{Scope, UnitPreset, gather};
    use glam::DMat4;
    use smith_geom::curve::{Circle, Curve, CurveFrame};
    use smith_model::{
        DefinitionKind, ImageFormat, Mapping, Material, Paint, Side, Texture, TextureFrame,
        TextureImage,
    };

    fn p(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    /// A 10 mm cube drawn as 12 lines in `e`.
    fn cube(e: &mut smith_model::Entities) -> Result<(), smith_model::ModelError> {
        let c = [
            p(0.0, 0.0, 0.0),
            p(10.0, 0.0, 0.0),
            p(10.0, 10.0, 0.0),
            p(0.0, 10.0, 0.0),
        ];
        let up = p(0.0, 0.0, 10.0);
        for i in 0..4 {
            let (a, b) = (c[i], c[(i + 1) % 4]);
            e.add_line(a, b)?;
            e.add_line(a + up, b + up)?;
            e.add_line(a, a + up)?;
        }
        Ok(())
    }

    fn lines(text: &str, start: &str) -> Vec<String> {
        text.lines()
            .filter(|l| l.starts_with(start))
            .map(str::to_owned)
            .collect()
    }

    fn print() -> ExportOptions {
        ExportOptions {
            units: UnitPreset::Print,
            ..ExportOptions::default()
        }
    }

    #[test]
    fn cube_is_six_quads_sharing_eight_vertices() {
        let mut doc = Document::new();
        doc.transaction("cube", |d| cube(d.root_mut())).unwrap();
        let g = gather(&doc, Scope::Model);
        let files = obj_files("box", &doc, &g, &print());
        assert_eq!(lines(&files.obj, "v ").len(), 8);
        let faces = lines(&files.obj, "f ");
        assert_eq!(faces.len(), 6);
        assert!(faces.iter().all(|f| f.split(' ').count() == 5));
        assert_eq!(lines(&files.obj, "o "), vec!["o box"]);
        assert!(files.obj.contains("mtllib box.mtl"));
        assert!(files.mtl.contains(&format!("newmtl {PLAIN}")));
        // Print (mm), Y up: the top corner (10, 10, 10) is (10, 10, −10).
        assert!(lines(&files.obj, "v ").contains(&"v 10 10 -10".to_owned()));
    }

    #[test]
    fn blender_units_are_metres() {
        let mut doc = Document::new();
        doc.transaction("cube", |d| cube(d.root_mut())).unwrap();
        let g = gather(&doc, Scope::Model);
        let files = obj_files("box", &doc, &g, &ExportOptions::default());
        assert!(lines(&files.obj, "v ").contains(&"v 0.01 0.01 -0.01".to_owned()));
    }

    #[test]
    fn corners_turn_about_the_normal() {
        // Each quad's corners turn counter-clockwise about its normal.
        let mut doc = Document::new();
        doc.transaction("cube", |d| cube(d.root_mut())).unwrap();
        let g = gather(&doc, Scope::Model);
        for f in &g.parts[0].faces {
            let c: Vec<DVec3> = f.corners.iter().map(|c| c.point).collect();
            let turn = (c[1] - c[0]).cross(c[2] - c[1]);
            assert!(turn.dot(f.normal) > 0.0);
        }
    }

    #[test]
    fn a_face_with_a_hole_is_triangulated_alone() {
        let mut doc = Document::new();
        doc.transaction("plate", |d| {
            let r = d.root_mut();
            let s = [
                p(0.0, 0.0, 0.0),
                p(50.0, 0.0, 0.0),
                p(50.0, 50.0, 0.0),
                p(0.0, 50.0, 0.0),
            ];
            for i in 0..4 {
                r.add_line(s[i], s[(i + 1) % 4])?;
            }
            let frame = CurveFrame::new(p(25.0, 25.0, 0.0), DVec3::X, DVec3::Z)?;
            r.add_curve(Curve::Circle(Circle::new(frame, 10.0)?), 24)?;
            Ok(())
        })
        .unwrap();
        let g = gather(&doc, Scope::Model);
        assert_eq!(g.holed_faces(), 1);
        let files = obj_files("plate", &doc, &g, &print());
        let faces = lines(&files.obj, "f ");
        // The disc inside is one 24-gon; the plate round it is triangles.
        assert_eq!(
            faces.iter().filter(|f| f.split(' ').count() == 25).count(),
            1
        );
        let tris = faces.iter().filter(|f| f.split(' ').count() == 4).count();
        assert_eq!(tris, faces.len() - 1);
        assert!(tris >= 24);
    }

    #[test]
    fn copies_are_objects_and_mirrors_keep_faces_outward() {
        let mut doc = Document::new();
        doc.transaction("copies", |d| {
            let def = d.create_definition("Block", DefinitionKind::Component);
            cube(
                d.definition_entities_mut(def)
                    .ok_or(smith_model::ModelError::NoSuchEntity)?,
            )?;
            d.add_instance(None, def, DMat4::IDENTITY, "Left")?;
            d.add_instance(
                None,
                def,
                DMat4::from_translation(p(-20.0, 0.0, 0.0)) * DMat4::from_scale(p(-1.0, 1.0, 1.0)),
                "",
            )?;
            Ok(())
        })
        .unwrap();
        let g = gather(&doc, Scope::Model);
        assert_eq!(g.parts.len(), 2);
        let files = obj_files("pair", &doc, &g, &print());
        assert_eq!(lines(&files.obj, "o "), vec!["o Left", "o Block"]);
        // Each face of the mirrored copy (x −30 to −20) faces in or out as
        // the same face of the plain one (x 0 to 10) does, and its corners
        // still turn counter-clockwise about its normal.
        let outward = |f: &crate::export::OutFace, centre: DVec3| {
            let mid = f.corners.iter().map(|c| c.point).sum::<DVec3>() / f.corners.len() as f64;
            (mid - centre).dot(f.normal) > 0.0
        };
        for (a, b) in g.parts[0].faces.iter().zip(&g.parts[1].faces) {
            assert_eq!(outward(a, p(5.0, 5.0, 5.0)), outward(b, p(-25.0, 5.0, 5.0)));
            let c: Vec<DVec3> = b.corners.iter().map(|c| c.point).collect();
            let turn = (c[1] - c[0]).cross(c[2] - c[1]);
            assert!(turn.dot(b.normal) > 0.0);
        }
    }

    #[test]
    fn hidden_things_stay_out() {
        let mut doc = Document::new();
        doc.transaction("hidden", |d| {
            let def = d.create_definition("Block", DefinitionKind::Group);
            cube(
                d.definition_entities_mut(def)
                    .ok_or(smith_model::ModelError::NoSuchEntity)?,
            )?;
            let i = d.add_instance(None, def, DMat4::IDENTITY, "")?;
            d.root_mut().set_instance_hidden(i, true)?;
            Ok(())
        })
        .unwrap();
        assert!(gather(&doc, Scope::Model).is_empty());
    }

    #[test]
    fn selection_only_takes_the_picked_faces() {
        let mut doc = Document::new();
        doc.transaction("cube", |d| cube(d.root_mut())).unwrap();
        let picked: Vec<_> = doc.root().face_ids().take(2).collect();
        let g = gather(
            &doc,
            Scope::Selection {
                context: None,
                local: DMat4::IDENTITY,
                faces: &picked,
                instances: &[],
            },
        );
        assert_eq!(g.parts[0].faces.len(), 2);
        let none = gather(
            &doc,
            Scope::Selection {
                context: None,
                local: DMat4::IDENTITY,
                faces: &[],
                instances: &[],
            },
        );
        assert!(none.is_empty());
    }

    #[test]
    fn paint_goes_to_the_mtl_with_its_image() {
        let mut doc = Document::new();
        let wood = doc
            .add_material(Material {
                name: "Oak wood".into(),
                color: [1.2, 0.5, 0.25],
                texture: Some(Texture {
                    image: std::sync::Arc::new(TextureImage {
                        name: "oak.png".into(),
                        format: ImageFormat::Png,
                        bytes: vec![1, 2, 3],
                        pixels: [2, 1],
                    }),
                    width_mm: 20.0,
                }),
                ..Material::default()
            })
            .unwrap();
        doc.transaction("paint", |d| {
            cube(d.root_mut())?;
            let r = d.root_mut();
            let f = r
                .face_ids()
                .next()
                .ok_or(smith_model::ModelError::NoSuchEntity)?;
            let n = r.face(f).map_or(DVec3::Z, |x| x.plane().normal());
            r.set_face_paint(
                f,
                Side::Front,
                Some(Paint {
                    material: wood,
                    mapping: Mapping::Face(TextureFrame::for_side(n, DVec2::ZERO, 0.0)),
                }),
            )
        })
        .unwrap();
        let g = gather(&doc, Scope::Model);
        let files = obj_files("crate", &doc, &g, &print());
        assert!(files.mtl.contains("newmtl Oak_wood"));
        assert!(files.mtl.contains("Kd 1 0.5 0.25"));
        assert!(files.mtl.contains("map_Kd crate_textures/Oak_wood.png"));
        assert_eq!(files.images, vec![("Oak_wood.png".into(), vec![1, 2, 3])]);
        assert!(files.obj.contains("usemtl Oak_wood"));
        assert_eq!(lines(&files.obj, "vt ").len(), 4);
    }

    #[test]
    fn writing_replaces_the_companion_files() {
        let dir = std::env::temp_dir().join(format!("smith-io-obj-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("box_textures")).unwrap();
        std::fs::write(dir.join("box_textures/old.png"), b"x").unwrap();
        let mut doc = Document::new();
        doc.transaction("cube", |d| cube(d.root_mut())).unwrap();
        let g = gather(&doc, Scope::Model);
        write_obj(&dir.join("box.obj"), &doc, &g, &print()).unwrap();
        assert!(dir.join("box.obj").exists());
        assert!(dir.join("box.mtl").exists());
        assert!(!dir.join("box_textures").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
