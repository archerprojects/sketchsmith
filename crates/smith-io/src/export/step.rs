//! SketchSmith — STEP export | v1.0.0 | Thursday 12 November 2026
//!
//! STEP (ISO 10303-21) in the AP214 schema (`AUTOMOTIVE_DESIGN`), written by
//! hand: one product whose shape holds a body for each connected shell of
//! each piece, in millimetres, Z up.
//!
//! * A closed shell (every edge used by exactly two faces, which can be
//!   turned to agree) is a `MANIFOLD_SOLID_BREP`, its faces turned outward
//!   in the file; an open one a `SHELL_BASED_SURFACE_MODEL`,
//!   so nothing is refused. With only solids the shape is an
//!   `ADVANCED_BREP_SHAPE_REPRESENTATION`; with any surface a plain
//!   `SHAPE_REPRESENTATION` holds both.
//! * Faces are exact: each a `PLANE` with its outer loop and holes as edge
//!   loops, not triangles. Edges are straight `LINE`s (arcs and circles go
//!   out as the segments the model stores). Vertices and edges are shared
//!   between the faces of a shell.
//! * The product, context and unit entities follow the AP214 files other
//!   CAD programs write.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use glam::DVec3;
use smith_model::VertexId;

use super::{Gathered, OutFace, write_file};
use crate::error::IoError;

/// What a STEP export wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepReport {
    /// Pieces with a shell that is not closed, written as surfaces.
    pub open_pieces: usize,
}

/// Writes `gathered` as the STEP file `path`.
///
/// # Errors
/// [`IoError::Os`] when the file cannot be written.
pub fn write_step(path: &Path, gathered: &Gathered) -> Result<StepReport, IoError> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".into());
    let file = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (text, report) = step_text(&stem, &file, &timestamp(), gathered);
    write_file(path, text.as_bytes())?;
    Ok(report)
}

/// The entities being written, numbered from 1.
#[derive(Default)]
struct Data {
    lines: Vec<String>,
}

impl Data {
    /// Adds an entity and returns its number.
    fn add(&mut self, entity: String) -> usize {
        self.lines.push(entity);
        self.lines.len()
    }

    fn point(&mut self, p: DVec3) -> usize {
        self.add(format!(
            "CARTESIAN_POINT('',({},{},{}))",
            real(p.x),
            real(p.y),
            real(p.z)
        ))
    }

    fn direction(&mut self, d: DVec3) -> usize {
        self.add(format!(
            "DIRECTION('',({},{},{}))",
            real(d.x),
            real(d.y),
            real(d.z)
        ))
    }

    fn placement(&mut self, origin: DVec3, axis: DVec3, reference: DVec3) -> usize {
        let (o, a, r) = (
            self.point(origin),
            self.direction(axis),
            self.direction(reference),
        );
        self.add(format!("AXIS2_PLACEMENT_3D('',#{o},#{a},#{r})"))
    }
}

/// One shell: faces of a piece joined through shared edges.
struct Shell {
    faces: Vec<(usize, OutFace)>,
    closed: bool,
}

/// The file's text and what it wrote. `stem` names the product, `file` and
/// `time` fill the header.
pub(crate) fn step_text(
    stem: &str,
    file: &str,
    time: &str,
    gathered: &Gathered,
) -> (String, StepReport) {
    let mut d = Data::default();

    // Product, units and context, as AP214 files have them.
    let app = d.add("APPLICATION_CONTEXT('automotive_design')".into());
    d.add(format!(
        "APPLICATION_PROTOCOL_DEFINITION('draft international standard','automotive_design',1998,#{app})"
    ));
    let pc = d.add(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let name = text(stem);
    let product = d.add(format!("PRODUCT({name},{name},'',(#{pc}))"));
    d.add(format!(
        "PRODUCT_RELATED_PRODUCT_CATEGORY('part','',(#{product}))"
    ));
    let pdf = d.add(format!(
        "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE('','',#{product},.NOT_KNOWN.)"
    ));
    let pdc = d.add(format!(
        "PRODUCT_DEFINITION_CONTEXT('detailed design',#{app},'design')"
    ));
    let pd = d.add(format!("PRODUCT_DEFINITION('design','',#{pdf},#{pdc})"));
    let pds = d.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
    let mm = d.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
    let rad = d.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
    let sr = d.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
    let tol = d.add(format!(
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-05),#{mm},'distance_accuracy_value','')"
    ));
    let ctx = d.add(format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{tol}))\
         GLOBAL_UNIT_ASSIGNED_CONTEXT((#{mm},#{rad},#{sr}))REPRESENTATION_CONTEXT('',''))"
    ));
    let origin = d.placement(DVec3::ZERO, DVec3::Z, DVec3::X);

    // The bodies, piece by piece.
    let mut items = vec![origin];
    let mut open_pieces = 0;
    let mut any_open = false;
    let mut pieces: Vec<usize> = Vec::new();
    for p in &gathered.parts {
        if !pieces.contains(&p.piece) {
            pieces.push(p.piece);
        }
    }
    for piece in pieces {
        let faces: Vec<(usize, &OutFace)> = gathered
            .parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.piece == piece)
            .flat_map(|(i, p)| p.faces.iter().map(move |f| (i, f)))
            .collect();
        let label = gathered
            .parts
            .iter()
            .find(|p| p.piece == piece && !p.name.is_empty())
            .map_or_else(|| stem.to_owned(), |p| p.name.clone());
        let shells = shells(faces);
        if shells.iter().any(|s| !s.closed) {
            open_pieces += 1;
        }
        for shell in shells {
            let shell_id = write_shell(&mut d, &shell, &label);
            let label = text(&label);
            items.push(if shell.closed {
                d.add(format!("MANIFOLD_SOLID_BREP({label},#{shell_id})"))
            } else {
                any_open = true;
                d.add(format!("SHELL_BASED_SURFACE_MODEL({label},(#{shell_id}))"))
            });
        }
    }
    let list = items
        .iter()
        .map(|i| format!("#{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let kind = if any_open {
        "SHAPE_REPRESENTATION"
    } else {
        "ADVANCED_BREP_SHAPE_REPRESENTATION"
    };
    let rep = d.add(format!("{kind}({name},({list}),#{ctx})"));
    d.add(format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})"));

    let mut out = String::from("ISO-10303-21;\nHEADER;\n");
    let _ = writeln!(out, "FILE_DESCRIPTION(('STEP AP214'),'2;1');");
    let _ = writeln!(
        out,
        "FILE_NAME({},'{time}',(''),(''),{},{},'');",
        text(file),
        text(&crate::native::generator()),
        text(&crate::native::generator())
    );
    out.push_str("FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n");
    for (i, line) in d.lines.iter().enumerate() {
        let _ = writeln!(out, "#{}={line};", i + 1);
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    (out, StepReport { open_pieces })
}

/// The faces split into shells joined through shared edges. A shell whose
/// every edge has exactly two faces is closed: its faces are turned to agree
/// with each other and to face outward (the model is not changed). A shell
/// whose faces cannot agree stays open.
fn shells(faces: Vec<(usize, &OutFace)>) -> Vec<Shell> {
    type Key = (usize, VertexId);
    let key = |part: usize, f: &OutFace, i: usize| (part, f.corners[i].vertex);
    // The faces using each edge, and which way each walks it.
    let mut by_edge: BTreeMap<(Key, Key), Vec<(usize, bool)>> = BTreeMap::new();
    for (n, (part, f)) in faces.iter().enumerate() {
        for (a, b) in loop_edges(f) {
            let (a, b) = (key(*part, f, a), key(*part, f, b));
            if a != b {
                by_edge
                    .entry((a.min(b), a.max(b)))
                    .or_default()
                    .push((n, a < b));
            }
        }
    }
    // Faces joined through shared edges.
    let mut group: Vec<usize> = (0..faces.len()).collect();
    fn root(group: &mut [usize], mut i: usize) -> usize {
        while group[i] != i {
            group[i] = group[group[i]];
            i = group[i];
        }
        i
    }
    for users in by_edge.values() {
        for pair in users.windows(2) {
            let (a, b) = (root(&mut group, pair[0].0), root(&mut group, pair[1].0));
            group[a] = b;
        }
    }
    let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for n in 0..faces.len() {
        let r = root(&mut group, n);
        members.entry(r).or_default().push(n);
    }
    // The edges round each face, to walk to its neighbours.
    let mut edges_of: Vec<Vec<(Key, Key)>> = vec![Vec::new(); faces.len()];
    for (e, users) in &by_edge {
        for (n, _) in users {
            edges_of[*n].push(*e);
        }
    }
    members
        .into_values()
        .map(|list| {
            let two_each = list
                .iter()
                .all(|n| edges_of[*n].iter().all(|e| by_edge[e].len() == 2));
            // Turn faces to agree, walking out from the first: neighbours
            // must walk their shared edge opposite ways.
            let mut flip: BTreeMap<usize, bool> = BTreeMap::new();
            let mut agree = two_each;
            if two_each {
                let mut todo = vec![list[0]];
                flip.insert(list[0], false);
                while let Some(n) = todo.pop() {
                    for e in &edges_of[n] {
                        let users = &by_edge[e];
                        let (Some(&(_, mine)), Some(&(other, theirs))) = (
                            users.iter().find(|u| u.0 == n),
                            users.iter().find(|u| u.0 != n),
                        ) else {
                            agree = false;
                            continue;
                        };
                        let want = flip[&n] ^ (mine == theirs);
                        match flip.get(&other) {
                            Some(have) if *have != want => agree = false,
                            Some(_) => {}
                            None => {
                                flip.insert(other, want);
                                todo.push(other);
                            }
                        }
                    }
                }
            }
            let closed = agree && flip.len() == list.len();
            let mut shell: Vec<(usize, OutFace)> = list
                .iter()
                .map(|n| {
                    let (part, f) = faces[*n];
                    let turn = closed && flip[n];
                    (part, if turn { turned(f) } else { f.clone() })
                })
                .collect();
            // Outward: the enclosed volume comes out positive.
            if closed && volume(&shell) < 0.0 {
                for (_, f) in &mut shell {
                    *f = turned(f);
                }
            }
            Shell {
                faces: shell,
                closed,
            }
        })
        .collect()
}

/// `f` facing the other way: every loop walked back, the normal reversed.
fn turned(f: &OutFace) -> OutFace {
    let mut out = f.clone();
    let mut start = 0;
    for &len in &f.loops {
        out.corners[start..start + len].reverse();
        start += len;
    }
    out.normal = -f.normal;
    out.triangles = Vec::new();
    out
}

/// Six times the volume the faces enclose, signed by their facing.
fn volume(faces: &[(usize, OutFace)]) -> f64 {
    let mut sum = 0.0;
    for (_, f) in faces {
        // A fan over each loop covers the face, holes taken away.
        let mut start = 0;
        for &len in &f.loops {
            let p = |k: usize| f.corners[start + k].point;
            for k in 1..len.saturating_sub(1) {
                sum += p(0).dot(p(k).cross(p(k + 1)));
            }
            start += len;
        }
    }
    sum
}

/// Each loop's edges as corner index pairs, in walk order.
fn loop_edges(f: &OutFace) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut start = 0;
    f.loops.iter().flat_map(move |&len| {
        let s = start;
        start += len;
        (0..len).map(move |k| (s + k, s + (k + 1) % len))
    })
}

/// Writes a shell's vertices, edges and faces; returns the shell entity.
fn write_shell(d: &mut Data, shell: &Shell, name: &str) -> usize {
    type Key = (usize, VertexId);
    let mut vertices: BTreeMap<Key, usize> = BTreeMap::new();
    let mut edges: BTreeMap<(Key, Key), usize> = BTreeMap::new();
    let mut face_ids = Vec::new();
    for (part, f) in &shell.faces {
        let key = |i: usize| (*part, f.corners[i].vertex);
        let mut bounds = Vec::new();
        let mut start = 0;
        for (n, &len) in f.loops.iter().enumerate() {
            let mut oriented = Vec::new();
            for k in 0..len {
                let (i, j) = (start + k, start + (k + 1) % len);
                let (a, b) = (key(i), key(j));
                if a == b {
                    continue;
                }
                for (v, c) in [(a, i), (b, j)] {
                    if let std::collections::btree_map::Entry::Vacant(slot) = vertices.entry(v) {
                        let pt = d.point(f.corners[c].point);
                        slot.insert(d.add(format!("VERTEX_POINT('',#{pt})")));
                    }
                }
                // Each edge once, running from its lower key to its higher.
                let forward = a < b;
                let (lo, hi, plo, phi) = if forward {
                    (a, b, f.corners[i].point, f.corners[j].point)
                } else {
                    (b, a, f.corners[j].point, f.corners[i].point)
                };
                let edge = match edges.get(&(lo, hi)) {
                    Some(e) => *e,
                    None => {
                        let along = phi - plo;
                        let pt = d.point(plo);
                        let dir = d.direction(along.normalize_or(DVec3::X));
                        let vec = d.add(format!("VECTOR('',#{dir},{})", real(along.length())));
                        let line = d.add(format!("LINE('',#{pt},#{vec})"));
                        let e = d.add(format!(
                            "EDGE_CURVE('',#{},#{},#{line},.T.)",
                            vertices[&lo], vertices[&hi]
                        ));
                        edges.insert((lo, hi), e);
                        e
                    }
                };
                let sense = if forward { ".T." } else { ".F." };
                oriented.push(d.add(format!("ORIENTED_EDGE('',*,*,#{edge},{sense})")));
            }
            start += len;
            if oriented.len() < 3 {
                continue;
            }
            let list = refs(&oriented);
            let lp = d.add(format!("EDGE_LOOP('',({list}))"));
            let kind = if n == 0 {
                "FACE_OUTER_BOUND"
            } else {
                "FACE_BOUND"
            };
            bounds.push(d.add(format!("{kind}('',#{lp},.T.)")));
        }
        if bounds.is_empty() {
            continue;
        }
        let o = f.corners[0].point;
        let reference = f
            .corners
            .iter()
            .map(|c| c.point - o)
            .find(|v| v.length() > 1e-9)
            .map_or_else(
                || f.normal.any_orthonormal_vector(),
                |v| {
                    (v - f.normal * f.normal.dot(v)).normalize_or(f.normal.any_orthonormal_vector())
                },
            );
        let place = d.placement(o, f.normal, reference);
        let plane = d.add(format!("PLANE('',#{place})"));
        face_ids.push(d.add(format!(
            "ADVANCED_FACE('',({}),#{plane},.T.)",
            refs(&bounds)
        )));
    }
    let kind = if shell.closed {
        "CLOSED_SHELL"
    } else {
        "OPEN_SHELL"
    };
    d.add(format!("{kind}({},({}))", text(name), refs(&face_ids)))
}

fn refs(ids: &[usize]) -> String {
    ids.iter()
        .map(|i| format!("#{i}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// A real as STEP writes it: always with a decimal point.
fn real(v: f64) -> String {
    let v = if v.abs() < 5e-13 { 0.0 } else { v };
    let s = format!("{v:.10}");
    let s = s.trim_end_matches('0');
    if s == "-." || s == "-0." {
        "0.".into()
    } else {
        s.to_owned()
    }
}

/// A STEP string: quoted, `'` and `\` doubled, anything outside printable
/// ASCII written as `\X2\hhhh\X0\`.
fn text(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            ' '..='~' => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\X2\\{u:04X}\\X0\\");
                }
            }
        }
    }
    out.push('\'');
    out
}

/// Now, as `YYYY-MM-DDThh:mm:ss` (UTC).
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rest = secs % 86_400;
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{Scope, gather};
    use smith_geom::curve::{Circle, Curve, CurveFrame};
    use smith_model::Document;

    fn p(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    fn cube(e: &mut smith_model::Entities, top: bool) -> Result<(), smith_model::ModelError> {
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
        if !top {
            let lid = e
                .face_ids()
                .find(|f| {
                    e.face(*f)
                        .is_some_and(|x| e.loop_points(x.outer()).iter().all(|q| q.z > 9.0))
                })
                .ok_or(smith_model::ModelError::NoSuchEntity)?;
            e.erase_face(lid)?;
        }
        Ok(())
    }

    fn count(t: &str, what: &str) -> usize {
        t.matches(what).count()
    }

    /// Every `#n` reference names an entity that exists.
    fn references_resolve(t: &str) {
        let defined: Vec<usize> = t
            .lines()
            .filter_map(|l| l.strip_prefix('#')?.split('=').next()?.parse().ok())
            .collect();
        let max = defined.len();
        assert_eq!(defined, (1..=max).collect::<Vec<_>>());
        for l in t.lines().filter(|l| l.starts_with('#')) {
            let body = l.split_once('=').unwrap().1;
            for r in body.split('#').skip(1) {
                let n: usize = r
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap();
                assert!(n >= 1 && n <= max, "#{n} in {l}");
            }
        }
    }

    #[test]
    fn a_box_is_one_solid_with_shared_edges() {
        let mut doc = Document::new();
        doc.transaction("cube", |d| cube(d.root_mut(), true))
            .unwrap();
        let g = gather(&doc, Scope::Model);
        let (t, report) = step_text("box", "box.step", "2026-10-22T00:00:00", &g);
        assert_eq!(report.open_pieces, 0);
        assert!(t.starts_with("ISO-10303-21;"));
        assert!(t.contains("FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));"));
        assert!(t.contains("SI_UNIT(.MILLI.,.METRE.)"));
        assert_eq!(count(&t, "MANIFOLD_SOLID_BREP("), 1);
        assert_eq!(count(&t, "CLOSED_SHELL("), 1);
        assert_eq!(count(&t, "ADVANCED_BREP_SHAPE_REPRESENTATION("), 1);
        assert_eq!(count(&t, "ADVANCED_FACE("), 6);
        assert_eq!(count(&t, "PLANE("), 6);
        assert_eq!(count(&t, "EDGE_CURVE("), 12);
        assert_eq!(count(&t, "VERTEX_POINT("), 8);
        assert_eq!(count(&t, "ORIENTED_EDGE("), 24);
        assert!(t.ends_with("END-ISO-10303-21;\n"));
        references_resolve(&t);
    }

    #[test]
    fn a_drawn_box_is_turned_outward() {
        // Drawn from lines, some faces come out facing in.
        let mut doc = Document::new();
        doc.transaction("cube", |d| cube(d.root_mut(), true))
            .unwrap();
        let g = gather(&doc, Scope::Model);
        let faces = g.parts[0].faces.iter().map(|f| (0, f)).collect();
        let shells = shells(faces);
        assert_eq!(shells.len(), 1);
        assert!(shells[0].closed);
        assert!((volume(&shells[0].faces) - 6000.0).abs() < 1e-6);
        for (_, f) in &shells[0].faces {
            let mid = f.corners.iter().map(|c| c.point).sum::<DVec3>() / f.corners.len() as f64;
            assert!((mid - p(5.0, 5.0, 5.0)).dot(f.normal) > 0.0);
            let c: Vec<DVec3> = f.corners.iter().map(|c| c.point).collect();
            assert!((c[1] - c[0]).cross(c[2] - c[1]).dot(f.normal) > 0.0);
        }
    }

    #[test]
    fn an_open_box_is_a_surface() {
        let mut doc = Document::new();
        doc.transaction("cube", |d| cube(d.root_mut(), false))
            .unwrap();
        let g = gather(&doc, Scope::Model);
        let (t, report) = step_text("tray", "tray.step", "2026-10-22T00:00:00", &g);
        assert_eq!(report.open_pieces, 1);
        assert_eq!(count(&t, "SHELL_BASED_SURFACE_MODEL("), 1);
        assert_eq!(count(&t, "OPEN_SHELL("), 1);
        assert_eq!(count(&t, "MANIFOLD_SOLID_BREP("), 0);
        assert!(t.contains("=SHAPE_REPRESENTATION("));
        assert_eq!(count(&t, "ADVANCED_FACE("), 5);
        references_resolve(&t);
    }

    #[test]
    fn a_hole_is_a_second_bound() {
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
        let (t, report) = step_text("plate", "plate.step", "2026-10-22T00:00:00", &g);
        // The plate and its disc: one open shell of two faces, three loops.
        assert_eq!(report.open_pieces, 1);
        assert_eq!(count(&t, "ADVANCED_FACE("), 2);
        assert_eq!(count(&t, "FACE_OUTER_BOUND("), 2);
        assert_eq!(count(&t, "FACE_BOUND("), 1);
        // The circle's 24 edges are shared by the plate and the disc.
        assert_eq!(count(&t, "EDGE_CURVE("), 28);
        references_resolve(&t);
    }

    #[test]
    fn text_and_numbers_follow_the_format() {
        assert_eq!(
            text("Joe's \\ Bürste"),
            "'Joe''s \\\\ B\\X2\\00FC\\X0\\rste'"
        );
        assert_eq!(real(10.0), "10.");
        assert_eq!(real(-0.0), "0.");
        assert_eq!(real(0.25), "0.25");
        assert_eq!(real(-1.5), "-1.5");
        assert_eq!(timestamp().len(), 19);
    }
}
