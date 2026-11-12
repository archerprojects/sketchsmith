//! SketchSmith — drawing pages | v1.0.0 | Thursday 12 November 2026
//!
//! Lays a hidden-line drawing out on paper for PDF and SVG: the paper size
//! and orientation, the scale (1:N; under 1 enlarges), the drawing centred
//! and never shrunk to fit, and each line's pen: its Line Style at true
//! weight, unstyled lines 0.25 mm, section cuts 0.5 mm, hidden lines left
//! out or drawn 0.18 mm dashed (ISO 128 type 02). Dimensions and text keep
//! their paper size at any scale: text 3.5 mm high, filled arrowheads 3 mm
//! long and 1 mm wide, lines 0.25 mm, all on the `Dimensions` layer.

use glam::DVec2;
use serde::{Deserialize, Serialize};
use smith_model::{LinePattern, PenWeight};

use super::{Drawing, ExportOptions, LineKind};

/// The margin round a drawing on a page, mm.
pub const MARGIN: f64 = 10.0;
/// The pen of unstyled lines, mm.
const PLAIN: f64 = 0.25;
/// The pen of section cuts, mm.
const CUT: f64 = 0.5;
/// The pen of hidden lines, mm.
const HIDDEN: f64 = 0.18;
/// The height of dimension text on paper, mm.
pub const TEXT: f64 = 3.5;
/// The length of an arrowhead on paper, mm; its width is a third of it.
pub const ARROW: f64 = 3.0;
/// The layer dimensions and text go on.
pub const DIMENSIONS: &str = "Dimensions";

/// The paper a drawing is laid out on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Paper {
    /// ISO A4.
    #[default]
    A4,
    /// ISO A3.
    A3,
    /// ISO A2.
    A2,
    /// ISO A1.
    A1,
    /// ISO A0.
    A0,
    /// US Letter.
    Letter,
    /// US Legal.
    Legal,
    /// US Tabloid.
    Tabloid,
    /// A page just big enough for the drawing and its margin.
    Fit,
}

impl Paper {
    /// The papers, in list order.
    pub const ALL: [Paper; 9] = [
        Self::A4,
        Self::A3,
        Self::A2,
        Self::A1,
        Self::A0,
        Self::Letter,
        Self::Legal,
        Self::Tabloid,
        Self::Fit,
    ];

    /// The paper's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::A4 => "A4",
            Self::A3 => "A3",
            Self::A2 => "A2",
            Self::A1 => "A1",
            Self::A0 => "A0",
            Self::Letter => "Letter",
            Self::Legal => "Legal",
            Self::Tabloid => "Tabloid",
            Self::Fit => "Fit to Drawing",
        }
    }

    /// Width and height upright, mm (`None` for Fit to Drawing).
    pub fn size(self) -> Option<DVec2> {
        let (w, h) = match self {
            Self::A4 => (210.0, 297.0),
            Self::A3 => (297.0, 420.0),
            Self::A2 => (420.0, 594.0),
            Self::A1 => (594.0, 841.0),
            Self::A0 => (841.0, 1189.0),
            Self::Letter => (215.9, 279.4),
            Self::Legal => (215.9, 355.6),
            Self::Tabloid => (279.4, 431.8),
            Self::Fit => return None,
        };
        Some(DVec2::new(w, h))
    }
}

/// Which way up the paper is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    /// Whichever fits the drawing better.
    #[default]
    Auto,
    /// Wider than tall.
    Landscape,
    /// Taller than wide.
    Portrait,
}

impl Orientation {
    /// The orientations, in list order.
    pub const ALL: [Orientation; 3] = [Self::Auto, Self::Landscape, Self::Portrait];

    /// The orientation's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Landscape => "Landscape",
            Self::Portrait => "Portrait",
        }
    }
}

/// Whether hidden lines are drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HiddenLines {
    /// Left out.
    #[default]
    Off,
    /// Drawn thin and dashed.
    Dashed,
}

impl HiddenLines {
    /// The choices, in list order.
    pub const ALL: [HiddenLines; 2] = [Self::Off, Self::Dashed];

    /// The choice's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Dashed => "Dashed",
        }
    }
}

/// One line on paper.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetLine {
    /// Start, mm from the paper's bottom-left corner, y up.
    pub a: DVec2,
    /// End, mm from the paper's bottom-left corner, y up.
    pub b: DVec2,
    /// Pen width, mm.
    pub weight: f64,
    /// Line type.
    pub pattern: LinePattern,
    /// The SketchSmith layer it came from.
    pub layer: String,
}

/// Text on paper.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetText {
    /// Its centre, mm from the paper's bottom-left corner, y up.
    pub at: DVec2,
    /// The words.
    pub text: String,
    /// Its height, mm.
    pub size: f64,
    /// Its width in Helvetica, mm (for the white box behind it).
    pub width: f64,
}

/// A drawing laid out on paper.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    /// The paper, mm, as laid (width, height).
    pub size: DVec2,
    /// Its lines.
    pub lines: Vec<SheetLine>,
    /// Dimension and leader lines, 0.25 mm.
    pub mark_lines: Vec<[DVec2; 2]>,
    /// Filled arrowheads, as triangles.
    pub arrows: Vec<[DVec2; 3]>,
    /// Dimension and leader text.
    pub texts: Vec<SheetText>,
    /// Whether the drawing fits inside the margin at its scale.
    pub fits: bool,
}

/// An arrowhead `length` long pointing at `tip`, coming from `from`: the
/// tip and the two corners of its base.
pub(crate) fn arrow(tip: DVec2, from: DVec2, length: f64) -> [DVec2; 3] {
    let d = (tip - from).normalize_or(DVec2::X);
    let base = tip - d * length;
    let side = d.perp() * (length / 6.0);
    [tip, base + side, base - side]
}

/// The width of `text` in Helvetica at height `size`; characters it lacks
/// count as `?`.
pub(crate) fn text_width(text: &str, size: f64) -> f64 {
    let units: u32 = text
        .chars()
        .map(|c| {
            let code = super::helvetica::code(c).unwrap_or(b'?');
            u32::from(super::helvetica::width(code))
        })
        .sum();
    f64::from(units) / 1000.0 * size
}

/// The pen of a drawn line: its width (mm) and pattern, or `None` when it is
/// not drawn.
pub(crate) fn pen(
    kind: LineKind,
    visible: bool,
    style: Option<smith_model::LineStyle>,
    hidden: HiddenLines,
) -> Option<(f64, LinePattern)> {
    if !visible {
        return (hidden == HiddenLines::Dashed).then_some((HIDDEN, LinePattern::Dashed));
    }
    Some(match (kind, style) {
        (LineKind::Cut, _) => (CUT, LinePattern::Solid),
        (_, Some(s)) => (s.weight.mm(), s.pattern),
        (_, None) => (PLAIN, LinePattern::Solid),
    })
}

/// The scale's N, made usable (1 for nothing sensible).
pub(crate) fn scale_n(options: &ExportOptions) -> f64 {
    if options.scale.is_finite() && options.scale > 0.0 {
        options.scale
    } else {
        1.0
    }
}

/// Lays `drawing` out on paper as `options` choose.
pub fn sheet(drawing: &Drawing, options: &ExportOptions) -> Sheet {
    let n = scale_n(options);
    let (lo, hi) = drawing.bounds().unwrap_or((DVec2::ZERO, DVec2::ZERO));
    let drawn = (hi - lo) / n;
    let size = match options.paper.size() {
        None => drawn + DVec2::splat(2.0 * MARGIN),
        Some(upright) => {
            let landscape = match options.orientation {
                Orientation::Landscape => true,
                Orientation::Portrait => false,
                Orientation::Auto => drawn.x > drawn.y,
            };
            if landscape {
                DVec2::new(upright.y, upright.x)
            } else {
                upright
            }
        }
    };
    let room = size - DVec2::splat(2.0 * MARGIN);
    let fits = drawn.x <= room.x + 1e-9 && drawn.y <= room.y + 1e-9;
    let centre = (lo + hi) * 0.5;
    let place = |p: DVec2| (p - centre) / n + size * 0.5;
    let lines = drawing
        .lines
        .iter()
        .filter_map(|l| {
            let (weight, pattern) = pen(l.kind, l.visible, l.style, options.hidden_lines)?;
            Some(SheetLine {
                a: place(l.a),
                b: place(l.b),
                weight,
                pattern,
                layer: l.layer.clone(),
            })
        })
        .collect();
    let mut sheet = Sheet {
        size,
        lines,
        mark_lines: Vec::new(),
        arrows: Vec::new(),
        texts: Vec::new(),
        fits,
    };
    for m in &drawing.marks {
        sheet
            .mark_lines
            .extend(m.lines.iter().map(|[a, b]| [place(*a), place(*b)]));
        sheet.arrows.extend(
            m.arrows
                .iter()
                .map(|[t, f]| arrow(place(*t), place(*f), ARROW)),
        );
        if let Some((at, text)) = &m.text {
            sheet.texts.push(SheetText {
                at: place(*at),
                text: text.clone(),
                size: TEXT,
                width: text_width(text, TEXT),
            });
        }
    }
    sheet
}

/// A pen width as DXF's line weight code (hundredths of a mm).
pub(crate) fn weight_hundredths(mm: f64) -> u16 {
    // DXF takes only its own list of line weights; ours are all on it.
    PenWeight::ALL
        .iter()
        .map(|w| w.mm())
        .chain([HIDDEN, PLAIN, CUT])
        .min_by(|a, b| (a - mm).abs().total_cmp(&(b - mm).abs()))
        .map_or(25, |w| (w * 100.0).round() as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::DrawnLine;

    fn square(side: f64) -> Drawing {
        let c = [
            DVec2::new(0.0, 0.0),
            DVec2::new(side, 0.0),
            DVec2::new(side, side),
            DVec2::new(0.0, side),
        ];
        Drawing {
            lines: (0..4)
                .map(|i| DrawnLine {
                    a: c[i],
                    b: c[(i + 1) % 4],
                    kind: LineKind::Edge,
                    visible: true,
                    layer: "Base".into(),
                    style: None,
                })
                .collect(),
            ..Drawing::default()
        }
    }

    #[test]
    fn marks_keep_their_paper_size() {
        let mut d = square(50.0);
        d.marks.push(crate::export::Mark {
            lines: vec![[DVec2::new(0.0, 60.0), DVec2::new(50.0, 60.0)]],
            arrows: vec![[DVec2::new(0.0, 60.0), DVec2::new(50.0, 60.0)]],
            text: Some((DVec2::new(25.0, 60.0), "50".into())),
        });
        for n in [1.0, 10.0] {
            let s = sheet(
                &d,
                &ExportOptions {
                    scale: n,
                    ..ExportOptions::default()
                },
            );
            let [a, b] = s.mark_lines[0];
            assert!((a.distance(b) - 50.0 / n).abs() < 1e-9);
            let [tip, c1, c2] = s.arrows[0];
            assert!((tip.distance((c1 + c2) * 0.5) - ARROW).abs() < 1e-9);
            assert!((c1.distance(c2) - 1.0).abs() < 1e-9);
            assert!((s.texts[0].size - 3.5).abs() < 1e-12);
            // "50" in Helvetica: two digits of 556 thousandths.
            assert!((s.texts[0].width - 2.0 * 0.556 * 3.5).abs() < 1e-9);
        }
    }

    #[test]
    fn a_50_mm_square_at_full_size_is_50_mm_on_a4() {
        let s = sheet(&square(50.0), &ExportOptions::default());
        assert_eq!(s.size, DVec2::new(210.0, 297.0));
        assert!(s.fits);
        let l = &s.lines[0];
        assert!((l.a.distance(l.b) - 50.0).abs() < 1e-12);
        // Centred.
        assert!((l.a - DVec2::new(80.0, 123.5)).length() < 1e-12);
        assert!((l.weight - 0.25).abs() < 1e-12);
    }

    #[test]
    fn orientation_scale_and_fitting() {
        let mut wide = square(50.0);
        wide.lines[1].b.x = 400.0;
        let o = ExportOptions {
            scale: 2.0,
            ..ExportOptions::default()
        };
        let s = sheet(&wide, &o);
        // 400 × 50 mm at 1:2 is 200 × 25: Auto turns A4 on its side.
        assert_eq!(s.size, DVec2::new(297.0, 210.0));
        assert!(s.fits);
        let upright = ExportOptions {
            orientation: Orientation::Portrait,
            ..o
        };
        assert!(!sheet(&wide, &upright).fits);
        // Twice full size (1:0.5) of a 50 mm square is 100 mm.
        let big = sheet(
            &square(50.0),
            &ExportOptions {
                scale: 0.5,
                ..ExportOptions::default()
            },
        );
        let l = &big.lines[0];
        assert!((l.a.distance(l.b) - 100.0).abs() < 1e-12);
        // Fit to Drawing: the drawing and its margin.
        let fit = sheet(
            &square(50.0),
            &ExportOptions {
                paper: Paper::Fit,
                ..ExportOptions::default()
            },
        );
        assert_eq!(fit.size, DVec2::new(70.0, 70.0));
    }

    #[test]
    fn hidden_lines_are_left_out_or_dashed() {
        let mut d = square(10.0);
        d.lines[0].visible = false;
        d.lines[1].kind = LineKind::Cut;
        assert_eq!(sheet(&d, &ExportOptions::default()).lines.len(), 3);
        let dashed = sheet(
            &d,
            &ExportOptions {
                hidden_lines: HiddenLines::Dashed,
                ..ExportOptions::default()
            },
        );
        assert_eq!(dashed.lines.len(), 4);
        assert_eq!(dashed.lines[0].pattern, LinePattern::Dashed);
        assert!((dashed.lines[0].weight - 0.18).abs() < 1e-12);
        assert!((dashed.lines[1].weight - 0.5).abs() < 1e-12);
    }

    #[test]
    fn dxf_weights_are_its_own() {
        assert_eq!(weight_hundredths(0.35), 35);
        assert_eq!(weight_hundredths(0.18), 18);
        assert_eq!(weight_hundredths(1.0), 100);
    }
}
