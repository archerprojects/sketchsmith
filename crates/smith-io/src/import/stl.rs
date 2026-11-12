//! SketchSmith — STL reading | v1.0.0 | Thursday 12 November 2026
//!
//! STL holds triangles only, Z up, in no named unit (Print (mm) by
//! default). Binary and text files are told apart by size, not by the word
//! `solid` (many binary files begin with it too): a binary file is exactly
//! 84 bytes plus 50 per triangle. A binary file is one object named after
//! the file; each `solid` of a text file is an object named after it.
//! Per-triangle colour bytes and stored normals are ignored: each
//! triangle's winding is its front.

use std::path::Path;

use glam::DVec3;

use super::{ImportGroup, ImportOptions, Imported, Polygon};
use crate::error::IoError;

/// Reads the STL `path`.
pub(super) fn read_stl(path: &Path, options: &ImportOptions) -> Result<Imported, IoError> {
    let bytes = std::fs::read(path)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Import".into());
    let k = options.mm_per_unit();
    let solids = if is_binary(&bytes) {
        vec![(String::new(), binary_triangles(&bytes))]
    } else {
        text_solids(&String::from_utf8_lossy(&bytes))?
    };
    let groups = solids
        .into_iter()
        .enumerate()
        .map(|(i, (name, tris))| ImportGroup {
            name: if !name.is_empty() {
                name
            } else if i == 0 {
                stem.clone()
            } else {
                format!("{stem} {}", i + 1)
            },
            polygons: tris
                .into_iter()
                .map(|t| Polygon {
                    points: t.iter().map(|p| *p * k).collect(),
                    uvs: None,
                    material: None,
                    ..Polygon::default()
                })
                .collect(),
            ..ImportGroup::default()
        })
        .collect::<Vec<_>>();
    let groups = if options.separate_shells {
        groups.into_iter().flat_map(super::separate).collect()
    } else {
        groups
    };
    Ok(Imported {
        name: stem,
        groups,
        merge_coplanar: options.merge_coplanar,
        soften: options.soften,
        ..Imported::default()
    })
}

/// Whether `bytes` is a binary STL: its triangle count matches its size.
fn is_binary(bytes: &[u8]) -> bool {
    if bytes.len() < 84 {
        return false;
    }
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as u64;
    84 + 50 * n == bytes.len() as u64
}

/// The triangles of a binary STL.
fn binary_triangles(bytes: &[u8]) -> Vec<[DVec3; 3]> {
    let f = |i: usize| {
        f64::from(f32::from_le_bytes([
            bytes[i],
            bytes[i + 1],
            bytes[i + 2],
            bytes[i + 3],
        ]))
    };
    let p = |i: usize| DVec3::new(f(i), f(i + 4), f(i + 8));
    let n = (bytes.len() - 84) / 50;
    (0..n)
        .map(|t| {
            let at = 84 + t * 50 + 12;
            [p(at), p(at + 12), p(at + 24)]
        })
        .collect()
}

/// A text STL's solid: its name and triangles.
type Solid = (String, Vec<[DVec3; 3]>);

/// The solids of a text STL.
fn text_solids(text: &str) -> Result<Vec<Solid>, IoError> {
    let mut out: Vec<Solid> = Vec::new();
    let mut corners: Vec<DVec3> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let (key, rest) = line
            .split_once(char::is_whitespace)
            .map_or((line, ""), |(k, r)| (k, r.trim()));
        match key {
            "solid" => out.push((rest.to_owned(), Vec::new())),
            "vertex" => {
                let v: Vec<f64> = rest
                    .split_whitespace()
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .map_err(|_| {
                        IoError::Damaged(format!("line {}: a vertex is not three numbers", n + 1))
                    })?;
                let [x, y, z] = v[..] else {
                    return Err(IoError::Damaged(format!(
                        "line {}: a vertex is not three numbers",
                        n + 1
                    )));
                };
                corners.push(DVec3::new(x, y, z));
            }
            "endloop" => {
                if let [a, b, c] = corners[..] {
                    if out.is_empty() {
                        out.push((String::new(), Vec::new()));
                    }
                    if let Some((_, tris)) = out.last_mut() {
                        tris.push([a, b, c]);
                    }
                }
                corners.clear();
            }
            _ => {}
        }
    }
    if out.iter().all(|(_, t)| t.is_empty()) {
        return Err(IoError::Damaged("no triangles found".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::UnitPreset;

    /// The 12 triangles of a 10 mm box.
    fn box_triangles() -> Vec<[DVec3; 3]> {
        let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z) * 10.0;
        let c = [
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(1.0, 1.0, 0.0),
            p(0.0, 1.0, 0.0),
        ];
        let t = |v: DVec3| v + p(0.0, 0.0, 1.0);
        let mut quads = vec![
            [c[3], c[2], c[1], c[0]],
            [t(c[0]), t(c[1]), t(c[2]), t(c[3])],
        ];
        for i in 0..4 {
            let (a, b) = (c[i], c[(i + 1) % 4]);
            quads.push([a, b, t(b), t(a)]);
        }
        quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect()
    }

    fn binary(tris: &[[DVec3; 3]]) -> Vec<u8> {
        // A header that begins "solid", as many binary files do.
        let mut out = b"solid but binary".to_vec();
        out.resize(80, b' ');
        out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
        for t in tris {
            out.extend_from_slice(&[0; 12]);
            for p in t {
                for c in p.to_array() {
                    out.extend_from_slice(&(c as f32).to_le_bytes());
                }
            }
            out.extend_from_slice(&[0; 2]);
        }
        out
    }

    fn text(tris: &[[DVec3; 3]], name: &str) -> String {
        let mut s = format!("solid {name}\n");
        for t in tris {
            s.push_str("  facet normal 0 0 0\n    outer loop\n");
            for p in t {
                s.push_str(&format!("      vertex {} {} {}\n", p.x, p.y, p.z));
            }
            s.push_str("    endloop\n  endfacet\n");
        }
        s.push_str(&format!("endsolid {name}\n"));
        s
    }

    fn read(name: &str, bytes: &[u8]) -> Imported {
        let d = std::env::temp_dir().join(format!("smith-io-stl-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join(format!("{name}.stl"));
        std::fs::write(&path, bytes).unwrap();
        let out = read_stl(
            &path,
            &ImportOptions::for_format(super::super::ImportFormat::Stl),
        )
        .unwrap();
        std::fs::remove_dir_all(&d).unwrap();
        out
    }

    #[test]
    fn binary_and_text_boxes_come_back_as_six_faces() {
        for (name, bytes) in [
            ("bin", binary(&box_triangles())),
            ("txt", text(&box_triangles(), "Box").into_bytes()),
        ] {
            let imported = read(name, &bytes);
            assert_eq!(imported.groups.len(), 1);
            assert_eq!(imported.groups[0].polygons.len(), 12);
            let mut doc = smith_model::Document::new();
            let (_, report) = doc
                .transaction("Import", |d| {
                    super::super::place(d, None, glam::DMat4::IDENTITY, &imported)
                })
                .unwrap();
            assert_eq!(report.counts.faces - report.counts.merged, 6, "{name}");
            let (lo, hi) = doc.model_bounds().unwrap();
            assert!((hi - lo - DVec3::splat(10.0)).length() < 1e-5);
        }
        assert_eq!(read("bin", &binary(&box_triangles())).groups[0].name, "bin");
        assert_eq!(
            read("txt", text(&box_triangles(), "Box").as_bytes()).groups[0].name,
            "Box"
        );
    }

    #[test]
    fn two_solids_are_two_objects_and_units_apply() {
        let tris = box_triangles();
        let mut s = text(&tris[..6], "A");
        s.push_str(&text(&tris[6..], "B"));
        let imported = read("two", s.as_bytes());
        let names: Vec<&str> = imported.groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["A", "B"]);
        let inch = ImportOptions {
            units: UnitPreset::Custom,
            unit: crate::export::FileUnit::In,
            ..ImportOptions::default()
        };
        assert!((inch.mm_per_unit() - 25.4).abs() < 1e-12);
        assert!(text_solids("solid x\nendsolid x\n").is_err());
        assert!(text_solids("solid x\nvertex 1 2\n").is_err());
    }
}
