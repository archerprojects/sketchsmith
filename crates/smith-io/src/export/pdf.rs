//! SketchSmith — PDF drawings | v1.0.0 | Thursday 12 November 2026
//!
//! A one-page PDF 1.4 of a drawing laid on paper, written by hand: the page
//! is the paper's size in points, and the drawing is in millimetres through
//! a scaling matrix, so a line 50 mm long on paper prints 50 mm long. Each
//! line keeps its pen width and its ISO line type as a dash pattern; lines
//! are grouped by layer. Dimensions come last, on top: their lines, filled
//! arrowheads, then each text on a white box, in PDF's built-in Helvetica
//! (WinAnsiEncoding, nothing embedded; characters it lacks print as `?`).
//! The page content is deflated.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use super::page::DIMENSIONS;
use super::{Sheet, helvetica, num, write_file};
use crate::error::IoError;

/// Points in a millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;

/// Writes `sheet` as the PDF `path`, titled after the file. Returns how many
/// characters Helvetica lacks, printed as `?`.
///
/// # Errors
/// [`IoError::Os`] when the file cannot be written.
pub fn write_pdf(path: &Path, sheet: &Sheet) -> Result<usize, IoError> {
    let title = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (bytes, missing) = pdf_bytes(&title, sheet)?;
    write_file(path, &bytes)?;
    Ok(missing)
}

/// `s` as a PDF string in WinAnsiEncoding, and how many of its characters
/// became `?`.
fn win_ansi(s: &str) -> (String, usize) {
    let mut out = String::from("(");
    let mut missing = 0;
    for c in s.chars() {
        let code = helvetica::code(c).unwrap_or_else(|| {
            missing += 1;
            b'?'
        });
        match code {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(char::from(code));
            }
            0x20..=0x7E => out.push(char::from(code)),
            _ => {
                let _ = write!(out, "\\{code:03o}");
            }
        }
    }
    out.push(')');
    (out, missing)
}

/// The page's drawing commands, in millimetres, and how many characters
/// became `?`.
pub(crate) fn content(sheet: &Sheet) -> (String, usize) {
    let mut out = format!(
        "{} 0 0 {} 0 0 cm\n0 J 0 j 0 G\n",
        num(PT_PER_MM),
        num(PT_PER_MM)
    );
    let mut order: Vec<usize> = (0..sheet.lines.len()).collect();
    order.sort_by(|a, b| sheet.lines[*a].layer.cmp(&sheet.lines[*b].layer));
    let mut layer: Option<&str> = None;
    let mut pen: Option<(u64, smith_model::LinePattern)> = None;
    for i in order {
        let l = &sheet.lines[i];
        if layer != Some(l.layer.as_str()) {
            let _ = writeln!(out, "% {}", l.layer.replace(['\r', '\n'], " "));
            layer = Some(&l.layer);
        }
        if pen != Some((l.weight.to_bits(), l.pattern)) {
            let dashes: Vec<String> = l
                .pattern
                .elements()
                .iter()
                .map(|e| num(f64::from(*e) * l.weight))
                .collect();
            let _ = writeln!(out, "{} w [{}] 0 d", num(l.weight), dashes.join(" "));
            pen = Some((l.weight.to_bits(), l.pattern));
        }
        let _ = writeln!(
            out,
            "{} {} m {} {} l S",
            num(l.a.x),
            num(l.a.y),
            num(l.b.x),
            num(l.b.y)
        );
    }

    // Dimensions and text, on top.
    let mut missing = 0;
    if !sheet.mark_lines.is_empty() || !sheet.texts.is_empty() {
        let _ = writeln!(out, "% {DIMENSIONS}\n0.25 w [] 0 d 0 g");
        for [a, b] in &sheet.mark_lines {
            let _ = writeln!(
                out,
                "{} {} m {} {} l S",
                num(a.x),
                num(a.y),
                num(b.x),
                num(b.y)
            );
        }
        for [t, c1, c2] in &sheet.arrows {
            let _ = writeln!(
                out,
                "{} {} m {} {} l {} {} l h f",
                num(t.x),
                num(t.y),
                num(c1.x),
                num(c1.y),
                num(c2.x),
                num(c2.y)
            );
        }
        for t in &sheet.texts {
            // A white box, then the words centred on the point (Helvetica's
            // capitals stand 0.718 of the size, so their middle is 0.359 up).
            let pad = 0.15 * t.size;
            let _ = writeln!(
                out,
                "1 g {} {} {} {} re f 0 g",
                num(t.at.x - t.width / 2.0 - pad),
                num(t.at.y - 0.5 * t.size - pad),
                num(t.width + 2.0 * pad),
                num(t.size + 2.0 * pad)
            );
            let (s, m) = win_ansi(&t.text);
            missing += m;
            let _ = writeln!(
                out,
                "BT /F1 {} Tf {} {} Td {s} Tj ET",
                num(t.size),
                num(t.at.x - t.width / 2.0),
                num(t.at.y - 0.359 * t.size)
            );
        }
    }
    (out, missing)
}

/// A PDF text string: ASCII as it is (parentheses and backslashes
/// escaped), anything else as UTF-16 with its byte-order mark.
fn text(s: &str) -> String {
    if s.chars().all(|c| (' '..='~').contains(&c)) {
        let mut out = String::from("(");
        for c in s.chars() {
            if matches!(c, '(' | ')' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push(')');
        out
    } else {
        let mut out = String::from("<FEFF");
        for u in s.encode_utf16() {
            let _ = write!(out, "{u:04X}");
        }
        out.push('>');
        out
    }
}

/// The whole file, and how many characters became `?`.
pub(crate) fn pdf_bytes(title: &str, sheet: &Sheet) -> Result<(Vec<u8>, usize), IoError> {
    let (page, missing) = content(sheet);
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(page.as_bytes())?;
    let stream = z.finish()?;
    let (w, h) = (sheet.size.x * PT_PER_MM, sheet.size.y * PT_PER_MM);

    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        let n = offsets.len();
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    object(
        &mut out,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>",
            num(w),
            num(h)
        )
        .as_bytes(),
    );
    let mut body = format!(
        "<< /Length {} /Filter /FlateDecode >>\nstream\n",
        stream.len()
    )
    .into_bytes();
    body.extend_from_slice(&stream);
    body.extend_from_slice(b"\nendstream");
    object(&mut out, &body);
    object(
        &mut out,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    object(
        &mut out,
        format!(
            "<< /Title {} /Producer {} >>",
            text(title),
            text(&crate::native::generator())
        )
        .as_bytes(),
    );
    let xref = out.len();
    let count = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {count}\n0000000000 65535 f \n").as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {count} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n")
            .as_bytes(),
    );
    Ok((out, missing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{SheetLine, sheet};
    use glam::DVec2;
    use smith_model::LinePattern;
    use std::io::Read as _;

    fn one_line(len: f64, pattern: LinePattern) -> Sheet {
        Sheet {
            size: DVec2::new(210.0, 297.0),
            lines: vec![SheetLine {
                a: DVec2::new(80.0, 100.0),
                b: DVec2::new(80.0 + len, 100.0),
                weight: 0.35,
                pattern,
                layer: "Base".into(),
            }],
            mark_lines: Vec::new(),
            arrows: Vec::new(),
            texts: Vec::new(),
            fits: true,
        }
    }

    /// The file's objects found where its cross-reference table says, and
    /// its page content inflated.
    fn read(pdf: &[u8]) -> String {
        let find = |what: &[u8]| pdf.windows(what.len()).rposition(|w| w == what).unwrap();
        let tail = String::from_utf8(pdf[find(b"startxref\n")..].to_vec()).unwrap();
        let start: usize = tail.lines().nth(1).unwrap().parse().unwrap();
        let table = String::from_utf8(pdf[start..].to_vec()).unwrap();
        assert!(table.starts_with("xref\n0 7\n"));
        let entries: Vec<usize> = table
            .lines()
            .skip(3)
            .take(6)
            .map(|l| l[..10].parse().unwrap())
            .collect();
        for (i, at) in entries.iter().enumerate() {
            let head = format!("{} 0 obj", i + 1);
            assert!(pdf[*at..].starts_with(head.as_bytes()), "object {}", i + 1);
        }
        let s = pdf.windows(7).position(|w| w == b"stream\n").unwrap() + 7;
        let e = pdf.windows(10).position(|w| w == b"\nendstream").unwrap();
        let mut inflated = String::new();
        flate2::read::ZlibDecoder::new(&pdf[s..e])
            .read_to_string(&mut inflated)
            .unwrap();
        inflated
    }

    #[test]
    fn a4_in_points_and_lines_in_mm() {
        let (pdf, _) = pdf_bytes("part", &one_line(50.0, LinePattern::Solid)).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.4"));
        let text = String::from_utf8_lossy(&pdf).into_owned();
        // A4 is 595.28 × 841.89 points.
        assert!(text.contains("/MediaBox [0 0 595.275591 841.889764]"));
        assert!(text.contains("/Title (part)"));
        let content = read(&pdf);
        // Millimetres through the matrix: the line runs 80 → 130 mm.
        assert!(content.starts_with("2.834646 0 0 2.834646 0 0 cm"));
        assert!(content.contains("0.35 w [] 0 d"));
        assert!(content.contains("80 100 m 130 100 l S"));
    }

    #[test]
    fn dashes_follow_the_pen() {
        let (content, _) = content(&one_line(50.0, LinePattern::Dashed));
        // ISO 128 type 02: dash 12, gap 3 line widths.
        assert!(content.contains("0.35 w [4.2 1.05] 0 d"));
    }

    #[test]
    fn titles_are_escaped() {
        assert_eq!(text("a (b) \\c"), "(a \\(b\\) \\\\c)");
        assert_eq!(text("Ø"), "<FEFF00D8>");
    }

    #[test]
    fn a_drawing_goes_through_whole() {
        let d = crate::export::Drawing::default();
        let s = sheet(&d, &crate::export::ExportOptions::default());
        let (pdf, _) = pdf_bytes("empty", &s).unwrap();
        read(&pdf);
    }

    #[test]
    fn dimension_text_is_helvetica_on_a_white_box() {
        let mut sheet = one_line(50.0, LinePattern::Solid);
        sheet.arrows.push([
            DVec2::new(80.0, 110.0),
            DVec2::new(83.0, 110.5),
            DVec2::new(83.0, 109.5),
        ]);
        sheet.texts.push(crate::export::SheetText {
            at: DVec2::new(105.0, 110.0),
            text: "Ø 50 (±0.1) 中".into(),
            size: 3.5,
            width: 20.0,
        });
        let (pdf, missing) = pdf_bytes("part", &sheet).unwrap();
        assert_eq!(missing, 1);
        let text = String::from_utf8_lossy(&pdf).into_owned();
        assert!(text.contains("/BaseFont /Helvetica /Encoding /WinAnsiEncoding"));
        let content = read(&pdf);
        assert!(content.contains("80 110 m 83 110.5 l 83 109.5 l h f"));
        assert!(content.contains("1 g "));
        // Ø is WinAnsi 216 (octal 330), ± is 177 (261); 中 becomes ?.
        assert!(content.contains("BT /F1 3.5 Tf 95 108.7435 Td (\\330 50 \\(\\2610.1\\) ?) Tj ET"));
    }
}
