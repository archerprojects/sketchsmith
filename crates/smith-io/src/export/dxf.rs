//! SketchSmith — DXF export | v1.0.0 | Thursday 12 November 2026
//!
//! ASCII DXF, version R2000 (AC1015), so the header can name the units
//! (`$INSUNITS`). Every face goes out as `3DFACE` triangles whose edges
//! inside the face are invisible, so CAD programs show only its real
//! outline; holes stay open. Z up; units as chosen. Each face goes on a
//! layer named after its SketchSmith layer.
//!
//! A **2D Drawing** instead holds the hidden-line drawing of a view as
//! `LINE`s in the drawing plane (x right, y up), each on its layer with its
//! pen as a DXF line weight and its ISO line type (`DASHED`, `DASHDOT`,
//! `DASHDOTDOT`, defined at the 0.25 mm pen and scaled per line), with its
//! dimensions and text on the `Dimensions` layer: lines, filled `SOLID`
//! arrowheads and centred `TEXT` in the `Standard` style, sized on paper
//! (text 3.5 mm, arrowheads 3 mm) through the drawing's scale.
//!
//! The file holds what R2000 readers require: the header, an empty classes
//! section, the nine tables (with the `0` layer, the `ByBlock`, `ByLayer`
//! and `Continuous` line types, the `Standard` text and dimension styles,
//! the `ACAD` application and the model and paper space block records), the
//! two space blocks, the faces, and the root dictionary with its group
//! dictionary.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use glam::DVec3;

use smith_model::LinePattern;

use super::page::{ARROW, DIMENSIONS, TEXT, arrow, pen, scale_n, weight_hundredths};
use super::{
    Drawing, DrawnLine, ExportOptions, FileUnit, Format, Gathered, UnitPreset, write_file,
};
use crate::error::IoError;

/// Handles of the fixed records; entities and layers are numbered after
/// them.
const BLOCK_RECORDS: &str = "1";
const LAYERS: &str = "2";
const STYLES: &str = "3";
const LTYPES: &str = "5";
const VIEWS: &str = "6";
const UCSS: &str = "7";
const VPORTS: &str = "8";
const APPIDS: &str = "9";
const DIMSTYLES: &str = "A";
const ROOT_DICT: &str = "C";
const GROUP_DICT: &str = "D";
const MODEL_SPACE: &str = "1F";
const PAPER_SPACE: &str = "1E";
/// The first handle given out as the file is written.
const FIRST_HANDLE: u32 = 0x100;

/// Writes `gathered` as the DXF `path`.
///
/// # Errors
/// [`IoError::Os`] when the file cannot be written.
pub fn write_dxf(path: &Path, gathered: &Gathered, options: &ExportOptions) -> Result<(), IoError> {
    write_file(path, dxf_text(gathered, options).as_bytes())
}

/// The file's text, as group code and value lines.
struct Dxf {
    out: String,
    next: u32,
}

impl Dxf {
    fn pair(&mut self, code: u16, value: impl std::fmt::Display) {
        let _ = write!(self.out, "{code:>3}\n{value}\n");
    }

    fn point(&mut self, base: u16, p: DVec3) {
        self.pair(base, real(p.x));
        self.pair(base + 10, real(p.y));
        self.pair(base + 20, real(p.z));
    }

    /// A new handle.
    fn handle(&mut self) -> String {
        let h = format!("{:X}", self.next);
        self.next += 1;
        h
    }

    fn table(&mut self, name: &str, handle: &str, count: usize) {
        self.pair(0, "TABLE");
        self.pair(2, name);
        self.pair(5, handle);
        self.pair(330, 0);
        self.pair(100, "AcDbSymbolTable");
        self.pair(70, count);
    }

    /// The start of a table record, up to its own subclass marker.
    fn record(&mut self, kind: &str, owner: &str, subclass: &str) {
        self.pair(0, kind);
        let h = self.handle();
        self.pair(if kind == "DIMSTYLE" { 105 } else { 5 }, h);
        self.pair(330, owner);
        self.pair(100, "AcDbSymbolTableRecord");
        self.pair(100, subclass);
    }
}

/// The `$INSUNITS` code for the chosen units.
fn insunits(options: &ExportOptions) -> u8 {
    let unit = match options.units {
        UnitPreset::Blender => FileUnit::M,
        UnitPreset::Print => FileUnit::Mm,
        UnitPreset::Custom => options.unit,
    };
    match unit {
        FileUnit::In => 1,
        FileUnit::Ft => 2,
        FileUnit::Mm => 4,
        FileUnit::Cm => 5,
        FileUnit::M => 6,
    }
}

/// A layer name DXF accepts: its forbidden characters made `_`; `0` when
/// empty.
fn layer_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if "<>/\\\":;?*|=`".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim().to_owned();
    if s.is_empty() { "0".into() } else { s }
}

/// The DXF text for `gathered`.
pub(crate) fn dxf_text(gathered: &Gathered, options: &ExportOptions) -> String {
    let scale = options.units_per_mm(Format::Dxf);
    let faces = || gathered.parts.iter().flat_map(|p| &p.faces);
    let layers: BTreeSet<String> = faces()
        .map(|f| layer_name(&f.layer))
        .filter(|l| l != "0")
        .collect();
    // The faces first, into their own text; the layers take the handles
    // before theirs, and the table records the ones after.
    let mut body = Dxf {
        out: String::new(),
        next: FIRST_HANDLE + u32::try_from(layers.len()).unwrap_or(0),
    };
    for f in faces() {
        let layer = layer_name(&f.layer);
        // Pairs of corners that are the face's own edges.
        let mut outline = BTreeSet::new();
        let mut start = 0;
        for &len in &f.loops {
            for k in 0..len {
                let (a, b) = (start + k, start + (k + 1) % len);
                outline.insert((a.min(b), a.max(b)));
            }
            start += len;
        }
        let inside = |a: usize, b: usize| !outline.contains(&(a.min(b), a.max(b)));
        for &[a, b, c] in &f.triangles {
            body.pair(0, "3DFACE");
            let h = body.handle();
            body.pair(5, h);
            body.pair(330, MODEL_SPACE);
            body.pair(100, "AcDbEntity");
            body.pair(8, &layer);
            body.pair(100, "AcDbFace");
            let p = |i: usize| f.corners[i].point * scale;
            body.point(10, p(a));
            body.point(11, p(b));
            body.point(12, p(c));
            body.point(13, p(c));
            // Edges: a–b, b–c, c–c (none), c–a.
            let flags = u8::from(inside(a, b))
                | (u8::from(inside(b, c)) << 1)
                | 4
                | (u8::from(inside(c, a)) << 3);
            body.pair(70, flags);
        }
    }

    assemble(body, &layers, &[], options)
}

/// Writes `drawing` as a 2D DXF drawing `path`.
///
/// # Errors
/// [`IoError::Os`] when the file cannot be written.
pub fn write_dxf_drawing(
    path: &Path,
    drawing: &Drawing,
    options: &ExportOptions,
) -> Result<(), IoError> {
    write_file(path, dxf_drawing_text(drawing, options).as_bytes())
}

/// A patterned line type's DXF name and description.
fn ltype(pattern: LinePattern) -> Option<(&'static str, &'static str)> {
    match pattern {
        LinePattern::Solid => None,
        LinePattern::Dashed => Some(("DASHED", "ISO dashed __ __ __")),
        LinePattern::DashDot => Some(("DASHDOT", "ISO long dash dot __ . __ .")),
        LinePattern::DashDoubleDot => Some(("DASHDOTDOT", "ISO long dash double dot __ .. __")),
    }
}

/// The pen the patterned line types are defined at, mm.
const LTYPE_PEN: f64 = 0.25;

/// The DXF text for a 2D drawing.
pub(crate) fn dxf_drawing_text(drawing: &Drawing, options: &ExportOptions) -> String {
    let k = options.units_per_mm(Format::Dxf);
    // Pens are paper sizes; on the drawing they are this many file units
    // per paper millimetre (the scale only counts with Custom units).
    let n = if options.units == UnitPreset::Custom {
        scale_n(options)
    } else {
        1.0
    };
    let pens: Vec<(&DrawnLine, f64, LinePattern)> = drawing
        .lines
        .iter()
        .filter_map(|l| {
            let (w, p) = pen(l.kind, l.visible, l.style, options.hidden_lines)?;
            Some((l, w, p))
        })
        .collect();
    let mut layers: BTreeSet<String> = pens
        .iter()
        .map(|(l, ..)| layer_name(&l.layer))
        .filter(|l| l != "0")
        .collect();
    if !drawing.marks.is_empty() {
        layers.insert(DIMENSIONS.into());
    }
    let used: BTreeSet<u8> = pens.iter().map(|(_, _, p)| p.index()).collect();
    let ltypes: Vec<(&str, &str, Vec<f64>)> = LinePattern::ALL
        .iter()
        .filter(|p| used.contains(&p.index()))
        .filter_map(|p| {
            let (name, text) = ltype(*p)?;
            // Dashes on, gaps off: alternate signs.
            let elements = p
                .elements()
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let len = f64::from(*e) * LTYPE_PEN * n * k;
                    if i % 2 == 0 { len } else { -len }
                })
                .collect();
            Some((name, text, elements))
        })
        .collect();
    let mut body = Dxf {
        out: String::new(),
        next: FIRST_HANDLE + u32::try_from(layers.len()).unwrap_or(0),
    };
    for (l, weight, pattern) in pens {
        body.pair(0, "LINE");
        let h = body.handle();
        body.pair(5, h);
        body.pair(330, MODEL_SPACE);
        body.pair(100, "AcDbEntity");
        body.pair(8, layer_name(&l.layer));
        if let Some((name, _)) = ltype(pattern) {
            body.pair(6, name);
            let scale = weight / LTYPE_PEN;
            if (scale - 1.0).abs() > 1e-9 {
                body.pair(48, real(scale));
            }
        }
        body.pair(370, weight_hundredths(weight));
        body.pair(100, "AcDbLine");
        body.point(10, l.a.extend(0.0) * k);
        body.point(11, l.b.extend(0.0) * k);
    }
    // Dimensions and text: paper sizes made file units through the scale.
    let paper = n * k;
    let start = |body: &mut Dxf, kind: &str| {
        body.pair(0, kind);
        let h = body.handle();
        body.pair(5, h);
        body.pair(330, MODEL_SPACE);
        body.pair(100, "AcDbEntity");
        body.pair(8, DIMENSIONS);
    };
    for m in &drawing.marks {
        for [a, b] in &m.lines {
            start(&mut body, "LINE");
            body.pair(370, 25);
            body.pair(100, "AcDbLine");
            body.point(10, a.extend(0.0) * k);
            body.point(11, b.extend(0.0) * k);
        }
        for [tip, from] in &m.arrows {
            let [t, c1, c2] = arrow(*tip * k, *from * k, ARROW * paper);
            start(&mut body, "SOLID");
            body.pair(100, "AcDbTrace");
            body.point(10, t.extend(0.0));
            body.point(11, c1.extend(0.0));
            body.point(12, c2.extend(0.0));
            body.point(13, c2.extend(0.0));
        }
        if let Some((at, words)) = &m.text {
            let p = (*at * k).extend(0.0);
            start(&mut body, "TEXT");
            body.pair(100, "AcDbText");
            body.point(10, p);
            body.pair(40, real(TEXT * paper));
            body.pair(1, dxf_text_value(words));
            body.pair(7, "Standard");
            // Centred both ways on the second alignment point.
            body.pair(72, 1);
            body.point(11, p);
            body.pair(100, "AcDbText");
            body.pair(73, 2);
        }
    }
    assemble(body, &layers, &ltypes, options)
}

/// The finished file round `body` (the entities, whose handles follow the
/// layers'): the header, tables with `layers` and the patterned line types
/// `ltypes` (name, description, element lengths), the blocks and objects.
fn assemble(
    body: Dxf,
    layers: &BTreeSet<String>,
    ltypes: &[(&str, &str, Vec<f64>)],
    options: &ExportOptions,
) -> String {
    let mut d = Dxf {
        out: String::new(),
        next: body.next,
    };

    // Tables.
    d.pair(0, "SECTION");
    d.pair(2, "TABLES");
    d.table("VPORT", VPORTS, 0);
    d.pair(0, "ENDTAB");
    d.table("LTYPE", LTYPES, 3 + ltypes.len());
    for (name, text) in [
        ("ByBlock", ""),
        ("ByLayer", ""),
        ("Continuous", "Solid line"),
    ] {
        d.record("LTYPE", LTYPES, "AcDbLinetypeTableRecord");
        d.pair(2, name);
        d.pair(70, 0);
        d.pair(3, text);
        d.pair(72, 65);
        d.pair(73, 0);
        d.pair(40, "0.0");
    }
    // Patterned line types: dashes positive, gaps negative.
    for (name, text, elements) in ltypes {
        d.record("LTYPE", LTYPES, "AcDbLinetypeTableRecord");
        d.pair(2, name);
        d.pair(70, 0);
        d.pair(3, text);
        d.pair(72, 65);
        d.pair(73, elements.len());
        d.pair(40, real(elements.iter().map(|e| e.abs()).sum()));
        for e in elements {
            d.pair(49, real(*e));
            d.pair(74, 0);
        }
    }
    d.pair(0, "ENDTAB");
    d.table("LAYER", LAYERS, layers.len() + 1);
    let mut next_layer = FIRST_HANDLE;
    for name in std::iter::once("0".to_owned()).chain(layers.iter().cloned()) {
        d.pair(0, "LAYER");
        if name == "0" {
            let h = d.handle();
            d.pair(5, h);
        } else {
            d.pair(5, format!("{next_layer:X}"));
            next_layer += 1;
        }
        d.pair(330, LAYERS);
        d.pair(100, "AcDbSymbolTableRecord");
        d.pair(100, "AcDbLayerTableRecord");
        d.pair(2, name);
        d.pair(70, 0);
        d.pair(62, 7);
        d.pair(6, "Continuous");
    }
    d.pair(0, "ENDTAB");
    d.table("STYLE", STYLES, 1);
    d.record("STYLE", STYLES, "AcDbTextStyleTableRecord");
    d.pair(2, "Standard");
    d.pair(70, 0);
    d.pair(40, "0.0");
    d.pair(41, "1.0");
    d.pair(50, "0.0");
    d.pair(71, 0);
    d.pair(42, "2.5");
    d.pair(3, "txt");
    d.pair(4, "");
    d.pair(0, "ENDTAB");
    d.table("VIEW", VIEWS, 0);
    d.pair(0, "ENDTAB");
    d.table("UCS", UCSS, 0);
    d.pair(0, "ENDTAB");
    d.table("APPID", APPIDS, 1);
    d.record("APPID", APPIDS, "AcDbRegAppTableRecord");
    d.pair(2, "ACAD");
    d.pair(70, 0);
    d.pair(0, "ENDTAB");
    d.table("DIMSTYLE", DIMSTYLES, 1);
    d.pair(100, "AcDbDimStyleTable");
    d.record("DIMSTYLE", DIMSTYLES, "AcDbDimStyleTableRecord");
    d.pair(2, "Standard");
    d.pair(70, 0);
    d.pair(0, "ENDTAB");
    d.table("BLOCK_RECORD", BLOCK_RECORDS, 2);
    for (handle, name) in [(MODEL_SPACE, "*Model_Space"), (PAPER_SPACE, "*Paper_Space")] {
        d.pair(0, "BLOCK_RECORD");
        d.pair(5, handle);
        d.pair(330, BLOCK_RECORDS);
        d.pair(100, "AcDbSymbolTableRecord");
        d.pair(100, "AcDbBlockTableRecord");
        d.pair(2, name);
    }
    d.pair(0, "ENDTAB");
    d.pair(0, "ENDSEC");

    // The two space blocks.
    d.pair(0, "SECTION");
    d.pair(2, "BLOCKS");
    for (owner, begin, end, name) in [
        (MODEL_SPACE, "20", "21", "*Model_Space"),
        (PAPER_SPACE, "1C", "1D", "*Paper_Space"),
    ] {
        d.pair(0, "BLOCK");
        d.pair(5, begin);
        d.pair(330, owner);
        d.pair(100, "AcDbEntity");
        d.pair(8, "0");
        d.pair(100, "AcDbBlockBegin");
        d.pair(2, name);
        d.pair(70, 0);
        d.point(10, DVec3::ZERO);
        d.pair(3, name);
        d.pair(1, "");
        d.pair(0, "ENDBLK");
        d.pair(5, end);
        d.pair(330, owner);
        d.pair(100, "AcDbEntity");
        d.pair(8, "0");
        d.pair(100, "AcDbBlockEnd");
    }
    d.pair(0, "ENDSEC");

    // The faces.
    d.pair(0, "SECTION");
    d.pair(2, "ENTITIES");
    d.out.push_str(&body.out);
    d.pair(0, "ENDSEC");

    // The root dictionary and its (empty) group dictionary.
    d.pair(0, "SECTION");
    d.pair(2, "OBJECTS");
    d.pair(0, "DICTIONARY");
    d.pair(5, ROOT_DICT);
    d.pair(330, 0);
    d.pair(100, "AcDbDictionary");
    d.pair(281, 1);
    d.pair(3, "ACAD_GROUP");
    d.pair(350, GROUP_DICT);
    d.pair(0, "DICTIONARY");
    d.pair(5, GROUP_DICT);
    d.pair(330, ROOT_DICT);
    d.pair(100, "AcDbDictionary");
    d.pair(281, 1);
    d.pair(0, "ENDSEC");
    d.pair(0, "EOF");

    // The header, now the next free handle is known, and the classes.
    let mut head = Dxf {
        out: format!("999\n{}\n", crate::native::generator()),
        next: 0,
    };
    head.pair(0, "SECTION");
    head.pair(2, "HEADER");
    head.pair(9, "$ACADVER");
    head.pair(1, "AC1015");
    head.pair(9, "$HANDSEED");
    head.pair(5, format!("{:X}", d.next));
    head.pair(9, "$INSUNITS");
    head.pair(70, insunits(options));
    // Metric drawing defaults unless the units are inches or feet.
    head.pair(9, "$MEASUREMENT");
    head.pair(70, u8::from(!matches!(insunits(options), 1 | 2)));
    head.pair(0, "ENDSEC");
    head.pair(0, "SECTION");
    head.pair(2, "CLASSES");
    head.pair(0, "ENDSEC");
    head.out + &d.out
}

/// Text as a DXF value: anything outside printable ASCII as `\U+hhhh`.
fn dxf_text_value(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if (' '..='~').contains(&c) {
            out.push(c);
        } else {
            let _ = write!(out, "\\U+{:04X}", u32::from(c));
        }
    }
    out
}

/// A coordinate: up to ten decimals, trailing zeros dropped, always with a
/// decimal point.
fn real(v: f64) -> String {
    let s = format!("{v:.10}");
    let s = s.trim_end_matches('0');
    let s = if s == "-." || s == "-0." { "0." } else { s };
    if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_owned()
    }
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

    /// The file as (code, value) pairs, checking every code is a number.
    fn pairs(t: &str) -> Vec<(u16, String)> {
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines.len() % 2, 0);
        lines
            .chunks(2)
            .map(|c| (c[0].trim().parse().unwrap(), c[1].to_owned()))
            .collect()
    }

    fn values(t: &str, code: u16, after: &str) -> Vec<String> {
        let ps = pairs(t);
        ps.windows(2)
            .filter(|w| w[0].1 == after && w[1].0 == code)
            .map(|w| w[1].1.clone())
            .collect()
    }

    fn plate(doc: &mut Document) {
        doc.transaction("plate", |d| {
            let red = d.add_layer("Red: parts", [200, 30, 30]);
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
            let disc = r
                .face_ids()
                .find(|f| {
                    r.face(*f)
                        .is_some_and(|x| x.inner().is_empty() && x.outer().len() == 24)
                })
                .ok_or(smith_model::ModelError::NoSuchEntity)?;
            r.set_layer(&[smith_model::Pickable::Face(disc)], Some(red))
        })
        .unwrap();
    }

    #[test]
    fn the_file_is_well_formed_r2000_in_mm() {
        let mut doc = Document::new();
        plate(&mut doc);
        let g = gather(&doc, Scope::Model);
        let t = dxf_text(&g, &ExportOptions::for_format(Format::Dxf));
        let ps = pairs(&t);
        assert_eq!(ps.last().unwrap(), &(0, "EOF".to_owned()));
        let sections = ps.iter().filter(|p| p.0 == 0 && p.1 == "SECTION").count();
        let ends = ps.iter().filter(|p| p.0 == 0 && p.1 == "ENDSEC").count();
        assert_eq!((sections, ends), (6, 6));
        assert_eq!(values(&t, 1, "$ACADVER"), ["AC1015"]);
        assert_eq!(values(&t, 70, "$INSUNITS"), ["4"]);
        // Handles are unique.
        let mut handles: Vec<&String> = ps
            .iter()
            .filter(|p| p.0 == 5 || p.0 == 105)
            .map(|p| &p.1)
            .collect();
        let n = handles.len();
        handles.sort();
        handles.dedup();
        assert_eq!(handles.len(), n);
        // The disc's layer, its name made safe, is in the table and used.
        assert!(values(&t, 2, "AcDbLayerTableRecord").contains(&"Red_ parts".to_owned()));
        assert!(values(&t, 8, "AcDbEntity").contains(&"Red_ parts".to_owned()));
    }

    #[test]
    fn faces_show_only_their_outlines() {
        let mut doc = Document::new();
        plate(&mut doc);
        let g = gather(&doc, Scope::Model);
        let tris: usize = g
            .parts
            .iter()
            .flat_map(|p| &p.faces)
            .map(|f| f.triangles.len())
            .sum();
        let t = dxf_text(&g, &ExportOptions::for_format(Format::Dxf));
        let ps = pairs(&t);
        assert_eq!(
            ps.iter().filter(|p| p.0 == 0 && p.1 == "3DFACE").count(),
            tris
        );
        // Every visible edge drawn is one of the faces' own: 4 square sides
        // and 24 circle sides, the circle drawn by both faces.
        let mut face_flags = Vec::new();
        let mut in_face = false;
        for (code, value) in &ps {
            if *code == 0 {
                in_face = value == "3DFACE";
            } else if in_face && *code == 70 {
                face_flags.push(value.parse::<u8>().unwrap());
            }
        }
        assert_eq!(face_flags.len(), tris);
        let mut shown = 0;
        for f in &face_flags {
            assert_ne!(f & 4, 0);
            shown += [1u8, 2, 8].iter().filter(|b| f & **b == 0).count();
        }
        assert_eq!(shown, 4 + 24 + 24);
    }

    #[test]
    fn units_follow_the_choice() {
        let mut doc = Document::new();
        plate(&mut doc);
        let g = gather(&doc, Scope::Model);
        let metres = dxf_text(&g, &ExportOptions::default());
        assert_eq!(values(&metres, 70, "$INSUNITS"), ["6"]);
        assert!(metres.contains("\n0.05\n"));
        let inches = ExportOptions {
            units: UnitPreset::Custom,
            unit: FileUnit::In,
            ..ExportOptions::default()
        };
        assert_eq!(values(&dxf_text(&g, &inches), 70, "$INSUNITS"), ["1"]);
    }

    #[test]
    fn a_drawing_is_lines_with_their_pens() {
        use crate::export::{HiddenLines, LineKind};
        let line = |visible, style: Option<smith_model::LineStyle>| DrawnLine {
            a: glam::DVec2::new(0.0, 0.0),
            b: glam::DVec2::new(50.0, 0.0),
            kind: LineKind::Edge,
            visible,
            layer: "Walls".into(),
            style,
        };
        let drawing = Drawing {
            lines: vec![
                line(true, None),
                line(
                    true,
                    Some(smith_model::LineStyle {
                        weight: smith_model::PenWeight::W050,
                        pattern: LinePattern::DashDot,
                    }),
                ),
                line(false, None),
            ],
            ..Drawing::default()
        };
        let options = ExportOptions {
            hidden_lines: HiddenLines::Dashed,
            ..ExportOptions::for_format(Format::Dxf)
        };
        let t = dxf_drawing_text(&drawing, &options);
        let ps = pairs(&t);
        assert_eq!(ps.iter().filter(|p| p.0 == 0 && p.1 == "LINE").count(), 3);
        assert!(values(&t, 2, "AcDbLinetypeTableRecord").contains(&"DASHED".to_owned()));
        assert!(values(&t, 2, "AcDbLinetypeTableRecord").contains(&"DASHDOT".to_owned()));
        // Pens as DXF line weights: 0.25, 0.5 and the hidden 0.18 mm.
        let weights: Vec<String> = ps
            .iter()
            .filter(|p| p.0 == 370)
            .map(|p| p.1.clone())
            .collect();
        assert_eq!(weights, ["25", "50", "18"]);
        // The 0.5 mm dash-dot runs at twice the 0.25 mm pattern.
        assert!(t.contains("\n 48\n2.0\n"));
        assert_eq!(
            values(&t, 8, "AcDbEntity")
                .iter()
                .filter(|l| *l == "Walls")
                .count(),
            3
        );
        // Hidden lines off: two lines.
        let off = dxf_drawing_text(&drawing, &ExportOptions::for_format(Format::Dxf));
        assert_eq!(
            pairs(&off)
                .iter()
                .filter(|p| p.0 == 0 && p.1 == "LINE")
                .count(),
            2
        );
    }

    #[test]
    fn dimensions_keep_their_paper_size() {
        let drawing = Drawing {
            marks: vec![crate::export::Mark {
                lines: vec![[glam::DVec2::ZERO, glam::DVec2::new(50.0, 0.0)]],
                arrows: vec![[glam::DVec2::ZERO, glam::DVec2::new(50.0, 0.0)]],
                text: Some((glam::DVec2::new(25.0, 0.0), "Ø 50".into())),
            }],
            ..Drawing::default()
        };
        let tenth = ExportOptions {
            units: UnitPreset::Custom,
            unit: FileUnit::Mm,
            scale: 10.0,
            ..ExportOptions::for_format(Format::Dxf)
        };
        for (options, height) in [
            (ExportOptions::for_format(Format::Dxf), "3.5"),
            (tenth, "3.5"),
        ] {
            let t = dxf_drawing_text(&drawing, &options);
            let ps = pairs(&t);
            assert_eq!(ps.iter().filter(|p| p.0 == 0 && p.1 == "TEXT").count(), 1);
            assert_eq!(ps.iter().filter(|p| p.0 == 0 && p.1 == "SOLID").count(), 1);
            assert!(t.contains(&format!(" 40\n{height}\n")));
            assert!(t.contains("\\U+00D8 50"));
            assert!(values(&t, 2, "AcDbLayerTableRecord").contains(&"Dimensions".to_owned()));
        }
    }

    #[test]
    fn names_and_numbers() {
        assert_eq!(layer_name("a/b:c"), "a_b_c");
        assert_eq!(layer_name("  "), "0");
        assert_eq!(real(10.0), "10.0");
        assert_eq!(real(-0.0), "0.0");
        assert_eq!(real(0.25), "0.25");
    }
}
