//! SketchSmith — STL export | v1.0.0 | Thursday 12 November 2026
//!
//! Binary STL: geometry only, every face as its triangles, all in one mesh.
//! Z up, as slicers expect; units as chosen. Each triangle carries its
//! face's normal, and its corners run counter-clockwise about it.

use std::path::Path;

use glam::DVec3;

use super::{ExportOptions, Format, Gathered, write_file};
use crate::error::IoError;

/// Writes `gathered` as the binary STL `path`.
///
/// # Errors
/// [`IoError::Os`] when the file cannot be written; [`IoError::TooLarge`]
/// past 4 294 967 295 triangles.
pub fn write_stl(path: &Path, gathered: &Gathered, options: &ExportOptions) -> Result<(), IoError> {
    write_file(path, &stl_bytes(gathered, options)?)
}

/// The binary STL for `gathered`.
pub(crate) fn stl_bytes(gathered: &Gathered, options: &ExportOptions) -> Result<Vec<u8>, IoError> {
    let scale = options.units_per_mm(Format::Stl);
    let count: usize = gathered
        .parts
        .iter()
        .flat_map(|p| &p.faces)
        .map(|f| f.triangles.len())
        .sum();
    let count = u32::try_from(count).map_err(|_| IoError::TooLarge)?;
    let mut out = Vec::with_capacity(84 + 50 * count as usize);
    // The header must not begin with "solid", which marks a text STL.
    let mut header = format!("{} binary STL", crate::native::generator()).into_bytes();
    header.resize(80, b' ');
    out.extend_from_slice(&header);
    out.extend_from_slice(&count.to_le_bytes());
    for f in gathered.parts.iter().flat_map(|p| &p.faces) {
        for t in &f.triangles {
            push(&mut out, f.normal);
            for k in t {
                push(&mut out, f.corners[*k].point * scale);
            }
            out.extend_from_slice(&0u16.to_le_bytes()); // attribute bytes
        }
    }
    Ok(out)
}

/// Appends `v` as three little-endian `f32`s.
fn push(out: &mut Vec<u8>, v: DVec3) {
    for c in v.to_array() {
        out.extend_from_slice(&(c as f32).to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{Scope, UnitPreset, gather};
    use smith_model::Document;

    fn f32_at(b: &[u8], i: usize) -> f32 {
        f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    #[test]
    fn a_cube_is_twelve_triangles_in_mm() {
        let mut doc = Document::new();
        doc.transaction("cube", |d| {
            let r = d.root_mut();
            let c = [
                DVec3::new(0.0, 0.0, 0.0),
                DVec3::new(50.0, 0.0, 0.0),
                DVec3::new(50.0, 50.0, 0.0),
                DVec3::new(0.0, 50.0, 0.0),
            ];
            let up = DVec3::new(0.0, 0.0, 50.0);
            for i in 0..4 {
                let (a, b) = (c[i], c[(i + 1) % 4]);
                r.add_line(a, b)?;
                r.add_line(a + up, b + up)?;
                r.add_line(a, a + up)?;
            }
            Ok(())
        })
        .unwrap();
        let g = gather(&doc, Scope::Model);
        let options = ExportOptions {
            units: UnitPreset::Print,
            ..ExportOptions::default()
        };
        let b = stl_bytes(&g, &options).unwrap();
        assert!(!b.starts_with(b"solid"));
        assert_eq!(u32::from_le_bytes([b[80], b[81], b[82], b[83]]), 12);
        assert_eq!(b.len(), 84 + 12 * 50);
        let mut top = f32::MIN;
        for t in 0..12 {
            let at = 84 + t * 50;
            let n = DVec3::new(
                f32_at(&b, at).into(),
                f32_at(&b, at + 4).into(),
                f32_at(&b, at + 8).into(),
            );
            let corner = |k: usize| {
                let i = at + 12 + 12 * k;
                DVec3::new(
                    f32_at(&b, i).into(),
                    f32_at(&b, i + 4).into(),
                    f32_at(&b, i + 8).into(),
                )
            };
            let (a, c1, c2) = (corner(0), corner(1), corner(2));
            assert!((c1 - a).cross(c2 - a).dot(n) > 0.0);
            top = top.max(a.z as f32).max(c1.z as f32).max(c2.z as f32);
        }
        // Z up, true size: the top is at 50 mm.
        assert!((top - 50.0).abs() < 1e-4);
    }
}
