//! SketchSmith — SVG drawings | v1.0.0 | Thursday 12 November 2026
//!
//! A drawing laid on paper as SVG, sized in millimetres (`width="210mm"`,
//! one user unit per millimetre), so it prints and cuts at its scale. Each
//! SketchSmith layer is a group that Inkscape shows as a layer; within it,
//! the lines sharing a pen are one path, stroked at the pen's width with its
//! ISO line type as a dash pattern. Black, no fill. Dimensions and text
//! are a last layer, `Dimensions`, on top: their lines, filled arrowheads,
//! and each text centred on a white box (a sans-serif font, Helvetica
//! first).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use super::page::DIMENSIONS;
use super::{Sheet, num, write_file};
use crate::error::IoError;

/// Writes `sheet` as the SVG `path`, titled after the file.
///
/// # Errors
/// [`IoError::Os`] when the file cannot be written.
pub fn write_svg(path: &Path, sheet: &Sheet) -> Result<(), IoError> {
    let title = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    write_file(path, svg_text(&title, sheet).as_bytes())
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
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// The SVG text.
pub(crate) fn svg_text(title: &str, sheet: &Sheet) -> String {
    let (w, h) = (num(sheet.size.x), num(sheet.size.y));
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <svg xmlns=\"http://www.w3.org/2000/svg\" \
         xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" \
         width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\">\n\
         <title>{}</title>\n\
         <desc>{}</desc>\n",
        escape(title),
        escape(&crate::native::generator())
    );
    // Layer, then pen (width, pattern), then the path's moves and lines.
    let mut layers: BTreeMap<&str, BTreeMap<(u64, u8), String>> = BTreeMap::new();
    for l in &sheet.lines {
        let d = layers
            .entry(l.layer.as_str())
            .or_default()
            .entry((l.weight.to_bits(), l.pattern.index()))
            .or_default();
        // SVG's y runs down the page.
        let _ = write!(
            d,
            "M{} {}L{} {}",
            num(l.a.x),
            num(sheet.size.y - l.a.y),
            num(l.b.x),
            num(sheet.size.y - l.b.y)
        );
    }
    for (i, (layer, pens)) in layers.iter().enumerate() {
        let _ = writeln!(
            out,
            "<g id=\"layer{}\" inkscape:groupmode=\"layer\" inkscape:label=\"{}\" \
             fill=\"none\" stroke=\"#000000\" stroke-linecap=\"butt\">",
            i + 1,
            escape(layer)
        );
        for ((bits, pattern), d) in pens {
            let weight = f64::from_bits(*bits);
            let pattern = smith_model::LinePattern::from_index(*pattern).unwrap_or_default();
            let dashes: Vec<String> = pattern
                .elements()
                .iter()
                .map(|e| num(f64::from(*e) * weight))
                .collect();
            let dash = if dashes.is_empty() {
                String::new()
            } else {
                format!(" stroke-dasharray=\"{}\"", dashes.join(" "))
            };
            let _ = writeln!(
                out,
                "<path stroke-width=\"{}\"{dash} d=\"{d}\"/>",
                num(weight)
            );
        }
        out.push_str("</g>\n");
    }

    // Dimensions and text, on top.
    if !sheet.mark_lines.is_empty() || !sheet.texts.is_empty() {
        let y = |v: f64| num(sheet.size.y - v);
        let _ = writeln!(
            out,
            "<g id=\"layer{}\" inkscape:groupmode=\"layer\" inkscape:label=\"{DIMENSIONS}\">",
            layers.len() + 1
        );
        let mut d = String::new();
        for [a, b] in &sheet.mark_lines {
            let _ = write!(d, "M{} {}L{} {}", num(a.x), y(a.y), num(b.x), y(b.y));
        }
        if !d.is_empty() {
            let _ = writeln!(
                out,
                "<path fill=\"none\" stroke=\"#000000\" stroke-width=\"0.25\" d=\"{d}\"/>"
            );
        }
        let mut heads = String::new();
        for [t, c1, c2] in &sheet.arrows {
            let _ = write!(
                heads,
                "M{} {}L{} {}L{} {}Z",
                num(t.x),
                y(t.y),
                num(c1.x),
                y(c1.y),
                num(c2.x),
                y(c2.y)
            );
        }
        if !heads.is_empty() {
            let _ = writeln!(
                out,
                "<path fill=\"#000000\" stroke=\"none\" d=\"{heads}\"/>"
            );
        }
        for t in &sheet.texts {
            let pad = 0.15 * t.size;
            let _ = writeln!(
                out,
                "<rect fill=\"#ffffff\" stroke=\"none\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                num(t.at.x - t.width / 2.0 - pad),
                y(t.at.y + 0.5 * t.size + pad),
                num(t.width + 2.0 * pad),
                num(t.size + 2.0 * pad)
            );
            // The baseline sits so the capitals' middle is on the point.
            let _ = writeln!(
                out,
                "<text x=\"{}\" y=\"{}\" font-family=\"Helvetica, Arial, sans-serif\" \
                 font-size=\"{}\" text-anchor=\"middle\" fill=\"#000000\">{}</text>",
                num(t.at.x),
                y(t.at.y - 0.359 * t.size),
                num(t.size),
                escape(&t.text)
            );
        }
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::SheetLine;
    use glam::DVec2;
    use smith_model::LinePattern;

    #[test]
    fn millimetres_layers_and_pens() {
        let line = |layer: &str, pattern, y| SheetLine {
            a: DVec2::new(80.0, y),
            b: DVec2::new(130.0, y),
            weight: 0.25,
            pattern,
            layer: layer.into(),
        };
        let sheet = Sheet {
            size: DVec2::new(210.0, 297.0),
            lines: vec![
                line("Base", LinePattern::Solid, 100.0),
                line("Walls & doors", LinePattern::Dashed, 50.0),
            ],
            mark_lines: vec![[DVec2::new(80.0, 120.0), DVec2::new(130.0, 120.0)]],
            arrows: vec![[
                DVec2::new(80.0, 120.0),
                DVec2::new(83.0, 120.5),
                DVec2::new(83.0, 119.5),
            ]],
            texts: vec![crate::export::SheetText {
                at: DVec2::new(105.0, 120.0),
                text: "50 & <more>".into(),
                size: 3.5,
                width: 15.0,
            }],
            fits: true,
        };
        let t = svg_text("plan", &sheet);
        assert!(t.contains("inkscape:label=\"Dimensions\""));
        assert!(t.contains("text-anchor=\"middle\""));
        assert!(t.contains(">50 &amp; &lt;more&gt;</text>"));
        assert!(t.contains("fill=\"#ffffff\""));
        assert!(t.contains("width=\"210mm\" height=\"297mm\" viewBox=\"0 0 210 297\""));
        assert!(t.contains("inkscape:label=\"Base\""));
        assert!(t.contains("inkscape:label=\"Walls &amp; doors\""));
        // 100 mm up from the bottom is 197 mm down from the top.
        assert!(t.contains("d=\"M80 197L130 197\""));
        assert!(t.contains("stroke-dasharray=\"3 0.75\""));
        assert_eq!(t.matches("<g ").count(), 3);
        assert!(t.trim_end().ends_with("</svg>"));
    }
}
