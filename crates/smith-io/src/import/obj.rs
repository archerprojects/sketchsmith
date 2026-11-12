//! SketchSmith — OBJ reading | v1.0.0 | Thursday 12 November 2026
//!
//! Reads Wavefront OBJ text and its MTL material libraries into plain
//! values, in the file's own units and axes:
//!
//! * `v` positions (a fourth weight is ignored), `vt` texture coordinates,
//!   `vn` normals (unused: faces take their winding);
//! * `f` faces of any size, corners as `v`, `v/vt`, `v//vn` or `v/vt/vn`,
//!   indices counted from 1 or, negative, back from the latest;
//! * `l` lines (a polyline is its segments), `p` points (counted, unused);
//! * `o` and `g` start a new object; `usemtl` names the material of the
//!   faces after it; `mtllib` names libraries; everything else is skipped.
//!
//! An MTL library gives each `newmtl` its `Kd` colour, `d` opacity (or
//! `Tr`, one minus it), `Pm` metallic and `map_Kd` image (options before
//! the file name are skipped).
//!
//! A face or line naming a position or texture coordinate the file does not
//! have refuses the file, saying where.

use glam::{DVec2, DVec3};

use crate::error::IoError;

/// One OBJ face: its corners and, when every corner has one, their texture
/// coordinates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ObjFace {
    pub points: Vec<DVec3>,
    pub uvs: Option<Vec<DVec2>>,
    pub material: Option<String>,
}

/// One OBJ object: its faces and line segments.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ObjObject {
    pub name: String,
    pub faces: Vec<ObjFace>,
    pub lines: Vec<[DVec3; 2]>,
}

/// A whole OBJ file.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ObjData {
    pub objects: Vec<ObjObject>,
    pub mtllibs: Vec<String>,
    pub points: usize,
}

/// One MTL material.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MtlMaterial {
    pub name: String,
    pub color: [f32; 3],
    pub opacity: f32,
    pub metallic: bool,
    pub map: Option<String>,
}

/// The 0-based index of OBJ index `token` among `count` items.
fn index(token: &str, count: usize, line: usize, what: &str) -> Result<usize, IoError> {
    let bad = || IoError::Damaged(format!("line {line}: no {what} {token}"));
    let i: i64 = token.parse().map_err(|_| bad())?;
    let at = match i {
        0 => return Err(bad()),
        i if i > 0 => usize::try_from(i - 1).map_err(|_| bad())?,
        i => count
            .checked_sub(usize::try_from(-i).map_err(|_| bad())?)
            .ok_or_else(bad)?,
    };
    if at < count { Ok(at) } else { Err(bad()) }
}

/// The numbers after a keyword.
fn numbers<const N: usize>(rest: &str, line: usize) -> Result<[f64; N], IoError> {
    let mut out = [0.0; N];
    let mut it = rest.split_whitespace();
    for slot in &mut out {
        let t = it
            .next()
            .ok_or_else(|| IoError::Damaged(format!("line {line}: a number is missing")))?;
        *slot = t
            .parse()
            .map_err(|_| IoError::Damaged(format!("line {line}: {t} is not a number")))?;
    }
    Ok(out)
}

/// Reads OBJ `text`.
pub(crate) fn parse_obj(text: &str) -> Result<ObjData, IoError> {
    let mut data = ObjData::default();
    let mut positions: Vec<DVec3> = Vec::new();
    let mut uvs: Vec<DVec2> = Vec::new();
    let mut current = ObjObject::default();
    let mut material: Option<String> = None;
    for (n, raw) in text.lines().enumerate() {
        let line = n + 1;
        let content = raw.split('#').next().unwrap_or("").trim();
        let (key, rest) = content
            .split_once(char::is_whitespace)
            .map_or((content, ""), |(k, r)| (k, r.trim()));
        match key {
            "v" => {
                let [x, y, z] = numbers::<3>(rest, line)?;
                positions.push(DVec3::new(x, y, z));
            }
            "vt" => {
                // A one-coordinate vt (u only) has v 0.
                let mut it = rest.split_whitespace().map(str::parse::<f64>);
                let u = it
                    .next()
                    .and_then(Result::ok)
                    .ok_or_else(|| IoError::Damaged(format!("line {line}: a number is missing")))?;
                let v = it.next().and_then(Result::ok).unwrap_or(0.0);
                uvs.push(DVec2::new(u, v));
            }
            "f" => {
                let mut points = Vec::new();
                let mut face_uvs = Vec::new();
                for corner in rest.split_whitespace() {
                    let mut parts = corner.split('/');
                    let v = parts.next().unwrap_or("");
                    points.push(positions[index(v, positions.len(), line, "position")?]);
                    match parts.next() {
                        Some(t) if !t.is_empty() => {
                            face_uvs.push(uvs[index(t, uvs.len(), line, "texture coordinate")?]);
                        }
                        _ => {}
                    }
                }
                let uvs = (face_uvs.len() == points.len()).then_some(face_uvs);
                current.faces.push(ObjFace {
                    points,
                    uvs,
                    material: material.clone(),
                });
            }
            "l" => {
                let mut at = Vec::new();
                for corner in rest.split_whitespace() {
                    let v = corner.split('/').next().unwrap_or("");
                    at.push(positions[index(v, positions.len(), line, "position")?]);
                }
                current.lines.extend(at.windows(2).map(|w| [w[0], w[1]]));
            }
            "p" => data.points += rest.split_whitespace().count(),
            "o" | "g" => {
                let done = std::mem::take(&mut current);
                if !done.faces.is_empty() || !done.lines.is_empty() {
                    data.objects.push(done);
                }
                current.name = rest.to_owned();
            }
            "usemtl" => material = (!rest.is_empty()).then(|| rest.to_owned()),
            "mtllib" if !rest.is_empty() => data.mtllibs.push(rest.to_owned()),
            _ => {}
        }
    }
    if !current.faces.is_empty() || !current.lines.is_empty() {
        data.objects.push(current);
    }
    Ok(data)
}

/// Reads MTL `text`; a malformed value is skipped (the material keeps its
/// default).
pub(crate) fn parse_mtl(text: &str) -> Vec<MtlMaterial> {
    let mut out: Vec<MtlMaterial> = Vec::new();
    for raw in text.lines() {
        let content = raw.split('#').next().unwrap_or("").trim();
        let (key, rest) = content
            .split_once(char::is_whitespace)
            .map_or((content, ""), |(k, r)| (k, r.trim()));
        if key == "newmtl" {
            out.push(MtlMaterial {
                name: rest.to_owned(),
                color: [0.8, 0.8, 0.8],
                opacity: 1.0,
                metallic: false,
                map: None,
            });
            continue;
        }
        let Some(m) = out.last_mut() else { continue };
        let floats: Vec<f32> = rest
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        match key {
            "Kd" if floats.len() >= 3 => m.color = [floats[0], floats[1], floats[2]],
            "d" if !floats.is_empty() => m.opacity = floats[0].clamp(0.0, 1.0),
            "Tr" if !floats.is_empty() => m.opacity = (1.0 - floats[0]).clamp(0.0, 1.0),
            "Pm" if !floats.is_empty() => m.metallic = floats[0] >= 0.5,
            "map_Kd" => m.map = map_file(rest),
            _ => {}
        }
    }
    out
}

/// The file name of a `map_Kd` line, its options (`-o 1 1 1`, `-clamp on`)
/// skipped.
fn map_file(rest: &str) -> Option<String> {
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() && tokens[i].starts_with('-') {
        i += 1;
        // The option's arguments: numbers, or on/off.
        while i < tokens.len()
            && (tokens[i].parse::<f64>().is_ok() || matches!(tokens[i], "on" | "off"))
        {
            i += 1;
        }
    }
    let name = tokens[i.min(tokens.len())..].join(" ");
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_lines_objects_and_materials() {
        let text = "\
# a test
mtllib box.mtl
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
v 0 0 1
vt 0 0
vt 1 0
vt 1 1
vt 0 1
o Base
usemtl Oak
f 1/1 2/2 3/3 4/4
o Post
usemtl
f -5//1 -4//1 -1//1
l 1 5 3
p 1 2
";
        let d = parse_obj(text).unwrap();
        assert_eq!(d.mtllibs, ["box.mtl"]);
        assert_eq!(d.points, 2);
        assert_eq!(d.objects.len(), 2);
        let base = &d.objects[0];
        assert_eq!(base.name, "Base");
        assert_eq!(base.faces[0].points.len(), 4);
        assert_eq!(base.faces[0].material.as_deref(), Some("Oak"));
        assert_eq!(base.faces[0].uvs.as_ref().unwrap()[2], DVec2::new(1.0, 1.0));
        let post = &d.objects[1];
        assert_eq!(post.faces[0].points[2], DVec3::new(0.0, 0.0, 1.0));
        assert!(post.faces[0].uvs.is_none());
        assert_eq!(post.faces[0].material, None);
        assert_eq!(post.lines.len(), 2);
    }

    #[test]
    fn a_missing_position_says_where() {
        let e = parse_obj("v 0 0 0\nf 1 2 3\n").unwrap_err();
        assert_eq!(e.to_string(), "the file is damaged: line 2: no position 2");
        assert!(parse_obj("v 0 0 0\nf 0 1 1\n").is_err());
        assert!(parse_obj("v x 0 0\n").is_err());
    }

    #[test]
    fn mtl_values_and_map_options() {
        let m = parse_mtl(
            "newmtl Oak\nKd 0.5 0.25 0.125\nd 0.75\nPm 1\nmap_Kd -s 2 2 1 -clamp on tex/oak grain.png\nnewmtl Glass\nTr 0.6\n",
        );
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].color, [0.5, 0.25, 0.125]);
        assert!((m[0].opacity - 0.75).abs() < 1e-6);
        assert!(m[0].metallic);
        assert_eq!(m[0].map.as_deref(), Some("tex/oak grain.png"));
        assert!((m[1].opacity - 0.4).abs() < 1e-6);
    }
}
