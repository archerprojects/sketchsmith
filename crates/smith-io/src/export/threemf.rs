//! SketchSmith — 3MF export | v1.0.0 | Thursday 12 November 2026
//!
//! A core 3MF package: `[Content_Types].xml`, `_rels/.rels` and
//! `3D/3dmodel.model`, zipped. The model is in millimetres (Scale 1:N
//! divides), Z up with the bed's front-left corner as origin, as the core
//! specification sets out.
//!
//! * One object per piece: the loose geometry, named after the file, then
//!   each top-level group or component copy with everything nested in it.
//!   Each object is placed once on the build, where it is in the model.
//! * Every face as its triangles, counter-clockwise seen from outside.
//! * Colour only: one base material per material used (its colour, sRGB,
//!   with its opacity), plus the plain face grey for unpainted faces. A
//!   textured material goes out as its colour.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use smith_model::{Document, MaterialId, VertexId};

use super::zip::ZipWriter;
use super::{ExportOptions, Format, Gathered, num, used_materials, write_file};
use crate::error::IoError;

/// The plain face colour (linear), as the native file's viewer layer has it.
const FACE_COLOR: [f32; 3] = [0.62, 0.60, 0.56];

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>
"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>
"#;

/// Writes `gathered` from `doc` as the 3MF `path`.
///
/// # Errors
/// [`IoError::Os`] when the file cannot be written; [`IoError::TooLarge`]
/// past 4 GiB.
pub fn write_3mf(
    path: &Path,
    doc: &Document,
    gathered: &Gathered,
    options: &ExportOptions,
) -> Result<(), IoError> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".into());
    write_file(path, &package(&stem, doc, gathered, options)?)
}

/// The zipped package.
pub(crate) fn package(
    stem: &str,
    doc: &Document,
    gathered: &Gathered,
    options: &ExportOptions,
) -> Result<Vec<u8>, IoError> {
    let mut zip = ZipWriter::default();
    zip.add("[Content_Types].xml", CONTENT_TYPES.as_bytes())?;
    zip.add("_rels/.rels", RELS.as_bytes())?;
    zip.add(
        "3D/3dmodel.model",
        model_xml(stem, doc, gathered, options).as_bytes(),
    )?;
    zip.finish()
}

/// The 3D model part.
pub(crate) fn model_xml(
    stem: &str,
    doc: &Document,
    gathered: &Gathered,
    options: &ExportOptions,
) -> String {
    let scale = options.units_per_mm(Format::ThreeMf);
    let faces = || gathered.parts.iter().flat_map(|p| &p.faces);

    // Base materials: the plain grey first when used, then each material.
    let plain_used = faces().any(|f| f.material.is_none());
    let mut bases: Vec<(String, String)> = Vec::new();
    if plain_used {
        bases.push(("SketchSmith Face".into(), display_color(FACE_COLOR, 1.0)));
    }
    let mut base_index: BTreeMap<MaterialId, usize> = BTreeMap::new();
    for m in used_materials(gathered) {
        let Some(material) = doc.material(m) else {
            continue;
        };
        base_index.insert(m, bases.len());
        bases.push((
            material.name.clone(),
            display_color(material.color, material.opacity),
        ));
    }
    let plain = 0;
    let painted = !base_index.is_empty();
    let index_of = |m: Option<MaterialId>| m.and_then(|m| base_index.get(&m).copied());

    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <model unit=\"millimeter\" xml:lang=\"en-us\" \
         xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n",
    );
    let _ = writeln!(
        out,
        "  <metadata name=\"Application\">{}</metadata>",
        escape(&crate::native::generator())
    );
    out.push_str("  <resources>\n");
    if painted {
        out.push_str("    <basematerials id=\"1\">\n");
        for (name, color) in &bases {
            let _ = writeln!(
                out,
                "      <base name=\"{}\" displaycolor=\"{color}\"/>",
                escape(name)
            );
        }
        out.push_str("    </basematerials>\n");
    }

    // One object per piece, in the order pieces first appear.
    let mut pieces: Vec<usize> = Vec::new();
    for p in &gathered.parts {
        if !pieces.contains(&p.piece) {
            pieces.push(p.piece);
        }
    }
    let mut objects = Vec::new();
    for (n, piece) in pieces.iter().enumerate() {
        let id = n + 2;
        let parts: Vec<(usize, &super::Part)> = gathered
            .parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.piece == *piece)
            .collect();
        let name = match parts.first() {
            Some((_, p)) if !p.name.is_empty() => p.name.clone(),
            _ => stem.to_owned(),
        };
        // Each vertex once per part, so faces stay joined.
        let mut index: BTreeMap<(usize, VertexId), usize> = BTreeMap::new();
        let mut vertices = String::new();
        let mut triangles = String::new();
        for (pi, part) in &parts {
            for f in &part.faces {
                let ids: Vec<usize> = f
                    .corners
                    .iter()
                    .map(|c| {
                        let next = index.len();
                        *index.entry((*pi, c.vertex)).or_insert_with(|| {
                            let p = c.point * scale;
                            let _ = writeln!(
                                vertices,
                                "          <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>",
                                num(p.x),
                                num(p.y),
                                num(p.z)
                            );
                            next
                        })
                    })
                    .collect();
                let material = index_of(f.material);
                for t in &f.triangles {
                    let [a, b, c] = t.map(|k| ids[k]);
                    // The specification asks for three different vertices.
                    if a == b || b == c || a == c {
                        continue;
                    }
                    let _ = write!(
                        triangles,
                        "          <triangle v1=\"{a}\" v2=\"{b}\" v3=\"{c}\""
                    );
                    if let Some(m) = material {
                        let _ = write!(triangles, " pid=\"1\" p1=\"{m}\"");
                    }
                    triangles.push_str("/>\n");
                }
            }
        }
        let _ = write!(
            out,
            "    <object id=\"{id}\" type=\"model\" name=\"{}\"",
            escape(&name)
        );
        if painted {
            // The object's own values cover its unpainted triangles.
            let _ = write!(out, " pid=\"1\" pindex=\"{plain}\"");
        }
        out.push_str(">\n      <mesh>\n        <vertices>\n");
        out.push_str(&vertices);
        out.push_str("        </vertices>\n        <triangles>\n");
        out.push_str(&triangles);
        out.push_str("        </triangles>\n      </mesh>\n    </object>\n");
        objects.push(id);
    }
    out.push_str("  </resources>\n  <build>\n");
    for id in objects {
        let _ = writeln!(out, "    <item objectid=\"{id}\"/>");
    }
    out.push_str("  </build>\n</model>\n");
    out
}

/// `#RRGGBBAA` from a linear colour (clamped to 0–1) and an opacity.
fn display_color(linear: [f32; 3], opacity: f32) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let srgb = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    };
    let [r, g, b] = linear.map(|c| byte(srgb(c)));
    format!("#{r:02X}{g:02X}{b:02X}{:02X}", byte(opacity))
}

/// `text` with XML's special characters escaped.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{Scope, gather};
    use glam::{DMat4, DVec2, DVec3};
    use smith_model::{DefinitionKind, Mapping, Material, Paint, Side, TextureFrame};

    fn p(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    /// A cube of side `s` at the origin, drawn as 12 lines in `e`.
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

    fn count(text: &str, what: &str) -> usize {
        text.matches(what).count()
    }

    /// The largest z of any vertex in the model text.
    fn top(xml: &str) -> f64 {
        xml.split("z=\"")
            .skip(1)
            .filter_map(|s| s.split('"').next()?.parse::<f64>().ok())
            .fold(f64::MIN, f64::max)
    }

    #[test]
    fn a_50_mm_part_is_50_mm() {
        let mut doc = Document::new();
        doc.transaction("cube", |d| cube(d.root_mut(), 50.0))
            .unwrap();
        let g = gather(&doc, Scope::Model);
        let xml = model_xml("part", &doc, &g, &ExportOptions::default());
        assert!(xml.contains("unit=\"millimeter\""));
        assert_eq!(count(&xml, "<vertex "), 8);
        assert_eq!(count(&xml, "<triangle "), 12);
        assert_eq!(count(&xml, "<object "), 1);
        assert!(xml.contains("name=\"part\""));
        assert!((top(&xml) - 50.0).abs() < 1e-9);
        // Unpainted: no materials at all.
        assert!(!xml.contains("basematerials"));
        // Scale 1:10 makes it 5 mm.
        let tenth = ExportOptions {
            scale: 10.0,
            ..ExportOptions::default()
        };
        assert!((top(&model_xml("part", &doc, &g, &tenth)) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn nested_copies_merge_into_their_top_object() {
        let mut doc = Document::new();
        doc.transaction("nested", |d| {
            let inner = d.create_definition("Peg", DefinitionKind::Group);
            cube(
                d.definition_entities_mut(inner)
                    .ok_or(smith_model::ModelError::NoSuchEntity)?,
                5.0,
            )?;
            let outer = d.create_definition("Base", DefinitionKind::Group);
            cube(
                d.definition_entities_mut(outer)
                    .ok_or(smith_model::ModelError::NoSuchEntity)?,
                20.0,
            )?;
            d.add_instance(
                Some(outer),
                inner,
                DMat4::from_translation(p(0.0, 0.0, 20.0)),
                "",
            )?;
            d.add_instance(None, outer, DMat4::IDENTITY, "Jig")?;
            d.add_instance(None, outer, DMat4::from_translation(p(40.0, 0.0, 0.0)), "")?;
            Ok(())
        })
        .unwrap();
        let g = gather(&doc, Scope::Model);
        assert_eq!(g.parts.len(), 4);
        let xml = model_xml("jig", &doc, &g, &ExportOptions::default());
        assert_eq!(count(&xml, "<object "), 2);
        assert_eq!(count(&xml, "<item "), 2);
        assert!(xml.contains("name=\"Jig\""));
        assert!(xml.contains("name=\"Base\""));
        assert_eq!(count(&xml, "<triangle "), 48);
        assert!((top(&xml) - 25.0).abs() < 1e-9);
    }

    #[test]
    fn paint_becomes_base_materials() {
        let mut doc = Document::new();
        let red = doc
            .add_material(Material {
                name: "Red & white".into(),
                color: [1.0, 0.0, 0.0],
                opacity: 0.5,
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
                Side::Front,
                Some(Paint {
                    material: red,
                    mapping: Mapping::Face(TextureFrame::for_side(DVec3::Z, DVec2::ZERO, 0.0)),
                }),
            )
        })
        .unwrap();
        let g = gather(&doc, Scope::Model);
        let xml = model_xml("box", &doc, &g, &ExportOptions::default());
        assert!(xml.contains("<base name=\"SketchSmith Face\" displaycolor=\"#CECBC5FF\"/>"));
        assert!(xml.contains("<base name=\"Red &amp; white\" displaycolor=\"#FF000080\"/>"));
        assert!(xml.contains("pid=\"1\" pindex=\"0\""));
        assert_eq!(count(&xml, "p1=\"1\""), 2);
    }

    #[test]
    fn the_package_holds_its_three_parts() {
        let mut doc = Document::new();
        doc.transaction("cube", |d| cube(d.root_mut(), 10.0))
            .unwrap();
        let g = gather(&doc, Scope::Model);
        let zip = package("box", &doc, &g, &ExportOptions::default()).unwrap();
        let back = crate::export::zip::read_back(&zip);
        let names: Vec<&str> = back.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["[Content_Types].xml", "_rels/.rels", "3D/3dmodel.model"]
        );
        let model = String::from_utf8(back[2].1.clone()).unwrap();
        assert!(model.ends_with("</model>\n"));
    }

    #[test]
    fn colours_are_srgb() {
        assert_eq!(display_color([0.0, 1.0, 0.5], 1.0), "#00FFBCFF");
    }

    /// Faces placed in `doc`, through every group.
    fn faces_placed(doc: &Document, e: &smith_model::Entities) -> usize {
        e.face_ids().count()
            + e.instance_ids()
                .filter_map(|i| e.instance(i))
                .filter_map(|i| doc.context(Some(i.definition)))
                .map(|x| faces_placed(doc, x))
                .sum::<usize>()
    }

    /// Writes `doc` as 3MF and imports it into a new model.
    fn round_trip(doc: &Document, tag: &str) -> (Document, crate::import::ImportReport) {
        use crate::import::{ImportFormat, ImportOptions, place, read_import};
        let dir = std::env::temp_dir().join(format!("smith-io-3mf-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{tag}.3mf"));
        write_3mf(
            &path,
            doc,
            &gather(doc, Scope::Model),
            &ExportOptions::default(),
        )
        .unwrap();
        let data = read_import(
            &path,
            ImportFormat::ThreeMf,
            &ImportOptions::for_format(ImportFormat::ThreeMf),
        )
        .unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let mut back = Document::new();
        let (_, report) = back
            .transaction("Import", |d| place(d, None, DMat4::IDENTITY, &data))
            .unwrap();
        (back, report)
    }

    #[test]
    fn a_50_mm_part_comes_back_50_mm_with_6_faces() {
        let mut doc = Document::new();
        doc.transaction("cube", |d| cube(d.root_mut(), 50.0))
            .unwrap();
        let (back, _) = round_trip(&doc, "part");
        assert_eq!(faces_placed(&back, back.root()), 6);
        let (lo, hi) = back.model_bounds().unwrap();
        assert!((hi - lo - DVec3::splat(50.0)).length() < 1e-6);
    }

    #[test]
    fn copies_come_back_where_they_were_and_paint_comes_back() {
        let mut doc = Document::new();
        doc.transaction("two", |d| {
            let def = d.create_definition("Block", DefinitionKind::Group);
            cube(
                d.definition_entities_mut(def)
                    .ok_or(smith_model::ModelError::NoSuchEntity)?,
                10.0,
            )?;
            d.add_instance(None, def, DMat4::IDENTITY, "A")?;
            d.add_instance(None, def, DMat4::from_translation(p(40.0, 0.0, 0.0)), "B")?;
            let m = d.add_material(Material {
                name: "Red".into(),
                color: [1.0, 0.0, 0.0],
                ..Material::default()
            })?;
            let e = d
                .definition_entities_mut(def)
                .ok_or(smith_model::ModelError::NoSuchEntity)?;
            let f = e
                .face_ids()
                .next()
                .ok_or(smith_model::ModelError::NoSuchEntity)?;
            e.set_face_paint(
                f,
                Side::Front,
                Some(Paint {
                    material: m,
                    mapping: Mapping::Face(TextureFrame::for_side(DVec3::Z, DVec2::ZERO, 0.0)),
                }),
            )
        })
        .unwrap();
        let (back, _) = round_trip(&doc, "two");
        assert_eq!(faces_placed(&back, back.root()), 12);
        let (lo, hi) = back.model_bounds().unwrap();
        assert!((hi.x - lo.x - 50.0).abs() < 1e-6);
        assert!(
            back.material_ids()
                .any(|m| back.material(m).is_some_and(|x| x.name == "Red"))
        );
    }
}
