//! SketchSmith — SVG reading | v1.0.0 | Thursday 12 November 2026
//!
//! Reads an SVG drawing into lines and curves on the ground plane, drawn as
//! the Draw tools would, with the filled shapes that decide which closed
//! loops keep their faces.
//!
//! * **Size**: `width` and `height` in mm, cm, in, pt or pc give the real
//!   size through the `viewBox`; bare numbers and px are CSS pixels, 96 to
//!   the inch. Custom units read the drawing's own units as the Unit at
//!   Scale 1:N instead. The top left lands at the origin, y turned to run up.
//! * **Shapes**: `path` (`M L H V C S Q T A Z`, absolute and relative),
//!   `rect` (rounded corners as arcs), `circle`, `ellipse`, `line`,
//!   `polyline`, `polygon`; `<use>` of another element; transforms
//!   (`matrix`, `translate`, `scale`, `rotate`, `skewX`, `skewY`) composed
//!   down the tree.
//! * **Curves stay curves**: runs of cubic segments are Bézier curves
//!   (quadratics raised to cubic exactly), circular arcs and circles are
//!   arcs and circles while the transform keeps them round, and elliptical
//!   arcs and ellipses are Bézier curves; straight runs are lines. Segment
//!   counts are the Draw tools' defaults.
//! * **Fills**: a shape whose `fill` is not `none` fills by its
//!   `fill-rule` (non-zero or even-odd), from attributes or `style`, as its
//!   groups pass them down; `display:none` hides. Stroke widths are not
//!   imported: a path is its centre line.
//! * **Groups**: each `<g>` holding shapes is a group, named by its
//!   Inkscape layer label, else its id.
//! * Text and images are counted and skipped; definitions, clip paths,
//!   masks, patterns, gradients and styles are skipped.

use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, TAU};
use std::path::Path;

use glam::{DMat3, DVec2, DVec3};
use smith_geom::curve::{
    Arc, Bezier, Circle, Curve, CurveFrame, DEFAULT_ARC_SEGMENTS, DEFAULT_BEZIER_SEGMENTS,
    DEFAULT_CIRCLE_SEGMENTS,
};

use super::{Drawn, Fill, ImportGroup, ImportOptions, Imported};
use crate::error::IoError;
use crate::export::UnitPreset;
use crate::xml::{self, Element};

/// Reads the SVG `path`.
pub(super) fn read_svg(path: &Path, options: &ImportOptions) -> Result<Imported, IoError> {
    let bytes = std::fs::read(path)?;
    let root = xml::parse(&xml::text_of(&bytes))?;
    if root.name != "svg" {
        return Err(IoError::Damaged("this is not an SVG file".into()));
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Import".into());
    let mut ids = HashMap::new();
    for e in root.descendants() {
        if let Some(id) = e.attr("id") {
            ids.insert(id, e);
        }
    }
    let mut svg = Svg {
        ids,
        texts: 0,
        images: 0,
    };
    let mut top = ImportGroup {
        name: stem.clone(),
        ..ImportGroup::default()
    };
    svg.children(&root, page(&root, options), Style::default(), &mut top, 0);
    // The drawing's own shapes stay in a group named after the file; its
    // groups stand beside it.
    let mut groups = std::mem::take(&mut top.children);
    if !top.drawn.is_empty() {
        groups.insert(0, top);
    }
    Ok(Imported {
        name: stem,
        groups,
        texts: svg.texts,
        images: svg.images,
        ..Imported::default()
    })
}

/// The drawing's units onto model mm on the ground plane, y up: the
/// viewBox's top left at the origin.
fn page(root: &Element, options: &ImportOptions) -> DMat3 {
    let vb: Vec<f64> = root
        .attr("viewBox")
        .map(|v| {
            v.split(|c: char| c == ',' || c.is_whitespace())
                .filter_map(|t| t.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    let vb = (vb.len() == 4 && vb[2] > 0.0 && vb[3] > 0.0).then(|| [vb[0], vb[1], vb[2], vb[3]]);
    let px = 25.4 / 96.0;
    let (sx, sy) = if options.from_file || options.units != UnitPreset::Custom {
        match vb {
            Some([_, _, w, h]) => {
                let size = |attr: &str, user: f64| {
                    root.attr(attr).and_then(length_mm).unwrap_or(user * px)
                };
                (size("width", w) / w, size("height", h) / h)
            }
            None => (px, px),
        }
    } else {
        let s = options.mm_per_unit();
        (s, s)
    };
    let (x0, y0) = vb.map_or((0.0, 0.0), |v| (v[0], v[1]));
    DMat3::from_cols(
        DVec3::new(sx, 0.0, 0.0),
        DVec3::new(0.0, -sy, 0.0),
        DVec3::new(-sx * x0, sy * y0, 1.0),
    )
}

/// A length in mm: a number with an absolute unit, or with px or none
/// (CSS pixels, 96 to the inch). `None` for percentages and anything else.
fn length_mm(s: &str) -> Option<f64> {
    let s = s.trim();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let v: f64 = num.parse().ok()?;
    let per = match unit.trim() {
        "mm" => 1.0,
        "cm" => 10.0,
        "in" => 25.4,
        "pt" => 25.4 / 72.0,
        "pc" => 25.4 / 6.0,
        "" | "px" => 25.4 / 96.0,
        _ => return None,
    };
    Some(v * per)
}

/// The fill a shape inherits.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Style {
    filled: bool,
    even_odd: bool,
    hidden: bool,
}

impl Default for Style {
    fn default() -> Self {
        // SVG fills black, non-zero, unless told otherwise.
        Self {
            filled: true,
            even_odd: false,
            hidden: false,
        }
    }
}

impl Style {
    /// The style `e` sets over `self`: its attributes, then its `style`.
    fn under(self, e: &Element) -> Self {
        let mut s = self;
        let mut set = |k: &str, v: &str| match (k.trim(), v.trim()) {
            ("fill", "none") => s.filled = false,
            ("fill", v) if v != "inherit" => s.filled = true,
            ("fill-rule", "evenodd") => s.even_odd = true,
            ("fill-rule", "nonzero") => s.even_odd = false,
            ("display", "none") => s.hidden = true,
            _ => {}
        };
        for (k, v) in &e.attrs {
            set(k, v);
        }
        if let Some(style) = e.attr("style") {
            for decl in style.split(';') {
                if let Some((k, v)) = decl.split_once(':') {
                    set(k, v);
                }
            }
        }
        s
    }
}

/// One drawn piece of a sub-path, in the drawing's own units.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Seg {
    Line(DVec2),
    Cubic(DVec2, DVec2, DVec2),
    Arc {
        rx: f64,
        ry: f64,
        turn: f64,
        large: bool,
        sweep: bool,
        to: DVec2,
    },
}

/// A sub-path: where it starts, its pieces, and whether it closes.
#[derive(Debug, Clone, PartialEq, Default)]
struct Sub {
    start: DVec2,
    segs: Vec<Seg>,
    closed: bool,
}

/// Reading one drawing.
struct Svg<'a> {
    ids: HashMap<&'a str, &'a Element>,
    texts: usize,
    images: usize,
}

/// The deepest nesting (and `<use>` chain) followed.
const MAX_DEPTH: usize = 64;

impl<'a> Svg<'a> {
    /// Draws `e`'s children into `g`, through `m`, with `style`.
    fn children(
        &mut self,
        e: &'a Element,
        m: DMat3,
        style: Style,
        g: &mut ImportGroup,
        depth: usize,
    ) {
        for c in &e.children {
            self.element(c, m, style, g, depth + 1);
        }
    }

    /// Draws element `e` (and what it holds) into `g`.
    fn element(
        &mut self,
        e: &'a Element,
        m: DMat3,
        style: Style,
        g: &mut ImportGroup,
        depth: usize,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        let style = style.under(e);
        if style.hidden {
            return;
        }
        let m = m * transform(e.attr("transform").unwrap_or(""));
        let num = |k: &str| {
            e.attr(k)
                .and_then(|v| v.trim().trim_end_matches("px").parse::<f64>().ok())
        };
        match e.name.as_str() {
            "g" | "a" | "switch" => {
                let mut child = ImportGroup {
                    name: e
                        .attr("label")
                        .or_else(|| e.attr("id"))
                        .unwrap_or("Group")
                        .to_owned(),
                    ..ImportGroup::default()
                };
                self.children(e, m, style, &mut child, depth);
                if !child.drawn.is_empty() || !child.children.is_empty() {
                    g.children.push(child);
                }
            }
            "svg" => {
                let at = DVec2::new(num("x").unwrap_or(0.0), num("y").unwrap_or(0.0));
                self.children(e, m * DMat3::from_translation(at), style, g, depth);
            }
            "use" => {
                let Some(target) = e
                    .attr("href")
                    .and_then(|h| self.ids.get(h.trim_start_matches('#')).copied())
                else {
                    return;
                };
                let at = DVec2::new(num("x").unwrap_or(0.0), num("y").unwrap_or(0.0));
                self.element(target, m * DMat3::from_translation(at), style, g, depth + 1);
            }
            "path" => {
                let subs = parse_path(e.attr("d").unwrap_or(""));
                emit(&subs, m, style, g);
            }
            "rect" => {
                let (x, y) = (num("x").unwrap_or(0.0), num("y").unwrap_or(0.0));
                let (w, h) = (num("width").unwrap_or(0.0), num("height").unwrap_or(0.0));
                if w <= 0.0 || h <= 0.0 {
                    return;
                }
                let (mut rx, mut ry) = (num("rx"), num("ry"));
                if rx.is_none() {
                    rx = ry;
                }
                if ry.is_none() {
                    ry = rx;
                }
                let (rx, ry) = (
                    rx.unwrap_or(0.0).min(w / 2.0),
                    ry.unwrap_or(0.0).min(h / 2.0),
                );
                let arc = |to: DVec2| Seg::Arc {
                    rx,
                    ry,
                    turn: 0.0,
                    large: false,
                    sweep: true,
                    to,
                };
                let sub = if rx > 0.0 && ry > 0.0 {
                    Sub {
                        start: DVec2::new(x + rx, y),
                        segs: vec![
                            Seg::Line(DVec2::new(x + w - rx, y)),
                            arc(DVec2::new(x + w, y + ry)),
                            Seg::Line(DVec2::new(x + w, y + h - ry)),
                            arc(DVec2::new(x + w - rx, y + h)),
                            Seg::Line(DVec2::new(x + rx, y + h)),
                            arc(DVec2::new(x, y + h - ry)),
                            Seg::Line(DVec2::new(x, y + ry)),
                            arc(DVec2::new(x + rx, y)),
                        ],
                        closed: true,
                    }
                } else {
                    Sub {
                        start: DVec2::new(x, y),
                        segs: vec![
                            Seg::Line(DVec2::new(x + w, y)),
                            Seg::Line(DVec2::new(x + w, y + h)),
                            Seg::Line(DVec2::new(x, y + h)),
                        ],
                        closed: true,
                    }
                };
                emit(&[sub], m, style, g);
            }
            "circle" | "ellipse" => {
                let c = DVec2::new(num("cx").unwrap_or(0.0), num("cy").unwrap_or(0.0));
                let (rx, ry) = if e.name == "circle" {
                    let r = num("r").unwrap_or(0.0);
                    (r, r)
                } else {
                    (num("rx").unwrap_or(0.0), num("ry").unwrap_or(0.0))
                };
                if rx <= 0.0 || ry <= 0.0 {
                    return;
                }
                let round = (rx - ry).abs() <= 1e-9 * rx;
                if round && let Some(s) = similarity(m) {
                    // A circle the transform keeps round: a Circle.
                    let centre = apply(m, c);
                    if let (Ok(frame), r) = (CurveFrame::new(centre, DVec3::X, DVec3::Z), rx * s)
                        && let Ok(circle) = Circle::new(frame, r)
                    {
                        let curve = Curve::Circle(circle);
                        if style.filled {
                            let piece = Drawn::Curve(curve.clone(), DEFAULT_CIRCLE_SEGMENTS);
                            g.fills.push(Fill {
                                loops: vec![outline(&[piece])],
                                even_odd: style.even_odd,
                            });
                        }
                        g.drawn.push(Drawn::Curve(curve, DEFAULT_CIRCLE_SEGMENTS));
                        return;
                    }
                }
                // Else four elliptical quarters.
                let p = |t: f64| c + DVec2::new(rx * t.cos(), ry * t.sin());
                let arc = |to: DVec2| Seg::Arc {
                    rx,
                    ry,
                    turn: 0.0,
                    large: false,
                    sweep: true,
                    to,
                };
                let sub = Sub {
                    start: p(0.0),
                    segs: (1..=4).map(|k| arc(p(f64::from(k) * FRAC_PI_2))).collect(),
                    closed: true,
                };
                emit(&[sub], m, style, g);
            }
            "line" => {
                let a = DVec2::new(num("x1").unwrap_or(0.0), num("y1").unwrap_or(0.0));
                let b = DVec2::new(num("x2").unwrap_or(0.0), num("y2").unwrap_or(0.0));
                let sub = Sub {
                    start: a,
                    segs: vec![Seg::Line(b)],
                    closed: false,
                };
                emit(
                    &[sub],
                    m,
                    Style {
                        filled: false,
                        ..style
                    },
                    g,
                );
            }
            "polyline" | "polygon" => {
                let v: Vec<f64> = e
                    .attr("points")
                    .unwrap_or("")
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter_map(|t| t.parse().ok())
                    .collect();
                let pts: Vec<DVec2> = v
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| DVec2::new(p[0], p[1]))
                    .collect();
                let Some(first) = pts.first() else { return };
                let sub = Sub {
                    start: *first,
                    segs: pts[1..].iter().map(|p| Seg::Line(*p)).collect(),
                    closed: e.name == "polygon",
                };
                emit(&[sub], m, style, g);
            }
            "text" => self.texts += 1,
            "image" => self.images += 1,
            _ => {}
        }
    }
}

/// A point of the drawing through `m`, on the ground plane.
fn apply(m: DMat3, p: DVec2) -> DVec3 {
    m.transform_point2(p).extend(0.0)
}

/// The uniform scale of `m` when it keeps circles round (no stretch or
/// shear), else `None`.
fn similarity(m: DMat3) -> Option<f64> {
    let (a, b) = (m.x_axis.truncate(), m.y_axis.truncate());
    let (la, lb) = (a.length(), b.length());
    ((la - lb).abs() <= 1e-9 * la && a.dot(b).abs() <= 1e-9 * la * lb).then_some(la)
}

/// Draws `subs` into `g` through `m`; their outlines join `g`'s fills when
/// `style` fills.
fn emit(subs: &[Sub], m: DMat3, style: Style, g: &mut ImportGroup) {
    let mut loops: Vec<Vec<DVec2>> = Vec::new();
    for sub in subs {
        let mut pieces: Vec<Drawn> = Vec::new();
        let mut chain: Vec<DVec3> = Vec::new();
        let flush = |chain: &mut Vec<DVec3>, pieces: &mut Vec<Drawn>| {
            if chain.len() >= 4
                && let Ok(b) = Bezier::new(std::mem::take(chain))
            {
                pieces.push(Drawn::Curve(Curve::Bezier(b), DEFAULT_BEZIER_SEGMENTS));
            }
            chain.clear();
        };
        let mut at = sub.start;
        let mut segs = sub.segs.clone();
        if sub.closed && segs.last().is_none_or(|s| end_of(s) != sub.start) {
            segs.push(Seg::Line(sub.start));
        }
        for seg in segs {
            match seg {
                Seg::Line(to) => {
                    flush(&mut chain, &mut pieces);
                    if to != at {
                        pieces.push(Drawn::Line(apply(m, at), apply(m, to)));
                    }
                    at = to;
                }
                Seg::Cubic(c1, c2, to) => {
                    if chain.is_empty() {
                        chain.push(apply(m, at));
                    }
                    chain.extend([apply(m, c1), apply(m, c2), apply(m, to)]);
                    at = to;
                }
                Seg::Arc {
                    rx,
                    ry,
                    turn,
                    large,
                    sweep,
                    to,
                } => {
                    match arc_piece(at, rx, ry, turn, large, sweep, to, m) {
                        ArcPiece::Line => {
                            flush(&mut chain, &mut pieces);
                            if to != at {
                                pieces.push(Drawn::Line(apply(m, at), apply(m, to)));
                            }
                        }
                        ArcPiece::Round(curve) => {
                            flush(&mut chain, &mut pieces);
                            pieces.push(Drawn::Curve(curve, DEFAULT_ARC_SEGMENTS));
                        }
                        ArcPiece::Cubics(cubics) => {
                            if chain.is_empty() {
                                chain.push(apply(m, at));
                            }
                            for (c1, c2, p) in cubics {
                                chain.extend([apply(m, c1), apply(m, c2), apply(m, p)]);
                            }
                        }
                    }
                    at = to;
                }
            }
        }
        flush(&mut chain, &mut pieces);
        if style.filled {
            loops.push(outline(&pieces));
        }
        g.drawn.extend(pieces);
    }
    if style.filled && !loops.is_empty() {
        g.fills.push(Fill {
            loops,
            even_odd: style.even_odd,
        });
    }
}

/// The points of a sub-path's pieces, end to end, on the ground plane: as
/// the drawn edges will run, so faces fall cleanly inside or out.
fn outline(pieces: &[Drawn]) -> Vec<DVec2> {
    let mut out: Vec<DVec2> = Vec::new();
    let mut push = |p: DVec3| {
        let q = p.truncate();
        if out.last() != Some(&q) {
            out.push(q);
        }
    };
    for p in pieces {
        match p {
            Drawn::Line(a, b) => {
                push(*a);
                push(*b);
            }
            Drawn::Curve(c, n) => {
                if let Ok(t) = c.tessellate(*n) {
                    for q in t.points {
                        push(q);
                    }
                }
            }
        }
    }
    out
}

/// Where a segment ends.
fn end_of(s: &Seg) -> DVec2 {
    match *s {
        Seg::Line(p) | Seg::Cubic(_, _, p) | Seg::Arc { to: p, .. } => p,
    }
}

/// What an SVG arc becomes.
enum ArcPiece {
    /// A straight line (a radius of 0).
    Line,
    /// A circular arc, round under the transform.
    Round(Curve),
    /// Cubic spans (in the drawing's units): control, control, end.
    Cubics(Vec<(DVec2, DVec2, DVec2)>),
}

/// The SVG arc from `p1` to `p2` (the specification's endpoint form,
/// converted to its centre form, radii grown if too small) as a circular
/// arc when it is round under `m`, else as cubic spans of at most 90°.
#[allow(clippy::too_many_arguments)]
fn arc_piece(
    p1: DVec2,
    rx: f64,
    ry: f64,
    turn_deg: f64,
    large: bool,
    sweep: bool,
    p2: DVec2,
    m: DMat3,
) -> ArcPiece {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if p1 == p2 || rx < 1e-12 || ry < 1e-12 {
        return ArcPiece::Line;
    }
    let phi = turn_deg.to_radians();
    let (s, c) = phi.sin_cos();
    let rot = |v: DVec2| DVec2::new(c * v.x - s * v.y, s * v.x + c * v.y);
    let unrot = |v: DVec2| DVec2::new(c * v.x + s * v.y, -s * v.x + c * v.y);
    let h = unrot((p1 - p2) / 2.0);
    let lambda = (h.x / rx).powi(2) + (h.y / ry).powi(2);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * h.y * h.y - ry * ry * h.x * h.x;
    let den = rx * rx * h.y * h.y + ry * ry * h.x * h.x;
    let k = (num / den).max(0.0).sqrt() * if large == sweep { -1.0 } else { 1.0 };
    let cp = DVec2::new(k * rx * h.y / ry, -k * ry * h.x / rx);
    let centre = rot(cp) + (p1 + p2) / 2.0;
    let angle = |v: DVec2| v.y.atan2(v.x);
    let t1 = angle(DVec2::new((h.x - cp.x) / rx, (h.y - cp.y) / ry));
    let t2 = angle(DVec2::new((-h.x - cp.x) / rx, (-h.y - cp.y) / ry));
    let mut dt = t2 - t1;
    if sweep && dt < 0.0 {
        dt += TAU;
    } else if !sweep && dt > 0.0 {
        dt -= TAU;
    }
    if (rx - ry).abs() <= 1e-9 * rx
        && let Some(scale) = similarity(m)
    {
        // Round on the model too: an arc about the mapped centre, turning
        // the other way if the transform mirrors.
        let c3 = apply(m, centre);
        let a = apply(m, p1) - c3;
        let start = a.y.atan2(a.x);
        let det = m.x_axis.truncate().perp_dot(m.y_axis.truncate());
        let turn = dt * det.signum();
        let (from, to) = if turn > 0.0 {
            (start, start + turn)
        } else {
            (start + turn, start)
        };
        if let Ok(frame) = CurveFrame::new(c3, DVec3::X, DVec3::Z)
            && let Ok(arc) = Arc::new(frame, rx * scale, from, to)
        {
            return ArcPiece::Round(Curve::Arc(arc));
        }
    }
    // Cubic spans of at most a quarter turn each.
    let n = (dt.abs() / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = dt / n as f64;
    let alpha = 4.0 / 3.0 * (step / 4.0).tan();
    let at = |t: f64| centre + rot(DVec2::new(rx * t.cos(), ry * t.sin()));
    let tangent = |t: f64| rot(DVec2::new(-rx * t.sin(), ry * t.cos()));
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (a, b) = (t1 + step * i as f64, t1 + step * (i + 1) as f64);
        let end = if i + 1 == n { p2 } else { at(b) };
        out.push((at(a) + tangent(a) * alpha, at(b) - tangent(b) * alpha, end));
    }
    ArcPiece::Cubics(out)
}

/// A `transform` attribute as a matrix (its parts applied in order).
fn transform(s: &str) -> DMat3 {
    let mut m = DMat3::IDENTITY;
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let v: Vec<f64> = rest[open + 1..open + close]
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter_map(|t| t.parse().ok())
            .collect();
        rest = &rest[open + close + 1..];
        let part = match (name, v.as_slice()) {
            ("matrix", [a, b, c, d, e, f]) => DMat3::from_cols(
                DVec3::new(*a, *b, 0.0),
                DVec3::new(*c, *d, 0.0),
                DVec3::new(*e, *f, 1.0),
            ),
            ("translate", [x]) => DMat3::from_translation(DVec2::new(*x, 0.0)),
            ("translate", [x, y]) => DMat3::from_translation(DVec2::new(*x, *y)),
            ("scale", [s]) => DMat3::from_scale(DVec2::splat(*s)),
            ("scale", [x, y]) => DMat3::from_scale(DVec2::new(*x, *y)),
            ("rotate", [a]) => DMat3::from_angle(a.to_radians()),
            ("rotate", [a, x, y]) => {
                let at = DVec2::new(*x, *y);
                DMat3::from_translation(at)
                    * DMat3::from_angle(a.to_radians())
                    * DMat3::from_translation(-at)
            }
            ("skewX", [a]) => DMat3::from_cols(
                DVec3::X,
                DVec3::new(a.to_radians().tan(), 1.0, 0.0),
                DVec3::Z,
            ),
            ("skewY", [a]) => DMat3::from_cols(
                DVec3::new(1.0, a.to_radians().tan(), 0.0),
                DVec3::Y,
                DVec3::Z,
            ),
            _ => DMat3::IDENTITY,
        };
        m *= part;
    }
    m
}

/// Reads path data: numbers (signs, decimals and exponents run together as
/// SVG allows: `1.5.5` is 1.5 then .5) and single-digit arc flags.
struct PathData<'a> {
    s: &'a [u8],
    i: usize,
}

impl PathData<'_> {
    fn gap(&mut self) {
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',')
        {
            self.i += 1;
        }
    }

    fn number(&mut self) -> Option<f64> {
        self.gap();
        let start = self.i;
        let s = self.s;
        if self.i < s.len() && matches!(s[self.i], b'+' | b'-') {
            self.i += 1;
        }
        let mut dot = false;
        let mut digits = false;
        while self.i < s.len() {
            match s[self.i] {
                b'0'..=b'9' => digits = true,
                b'.' if !dot => dot = true,
                _ => break,
            }
            self.i += 1;
        }
        if digits && self.i < s.len() && matches!(s[self.i], b'e' | b'E') {
            let back = self.i;
            self.i += 1;
            if self.i < s.len() && matches!(s[self.i], b'+' | b'-') {
                self.i += 1;
            }
            let exp_start = self.i;
            while self.i < s.len() && s[self.i].is_ascii_digit() {
                self.i += 1;
            }
            if self.i == exp_start {
                self.i = back;
            }
        }
        if !digits {
            self.i = start;
            return None;
        }
        std::str::from_utf8(&s[start..self.i]).ok()?.parse().ok()
    }

    fn flag(&mut self) -> Option<bool> {
        self.gap();
        let f = match self.s.get(self.i)? {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.i += 1;
        Some(f)
    }

    fn point(&mut self) -> Option<DVec2> {
        Some(DVec2::new(self.number()?, self.number()?))
    }

    /// The next command letter, if one comes next.
    fn command(&mut self) -> Option<u8> {
        self.gap();
        let c = *self.s.get(self.i)?;
        c.is_ascii_alphabetic().then(|| {
            self.i += 1;
            c
        })
    }
}

/// Adds `seg` to the last sub-path.
fn push(subs: &mut [Sub], seg: Seg) {
    if let Some(s) = subs.last_mut() {
        s.segs.push(seg);
    }
}

/// The sub-paths of SVG path data `d`. Parsing stops at the first error, as
/// SVG renders a path up to it.
fn parse_path(d: &str) -> Vec<Sub> {
    let mut p = PathData {
        s: d.as_bytes(),
        i: 0,
    };
    let mut subs: Vec<Sub> = Vec::new();
    let mut at = DVec2::ZERO;
    let mut start = DVec2::ZERO;
    // The last cubic and quadratic control points, for S and T.
    let (mut last_c, mut last_q): (Option<DVec2>, Option<DVec2>) = (None, None);
    let mut cmd = 0u8;
    loop {
        if let Some(c) = p.command() {
            cmd = c;
        } else if cmd == 0 || p.i >= p.s.len() {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { at } else { DVec2::ZERO };
        let current = |subs: &mut Vec<Sub>, at: DVec2| {
            if subs.is_empty() || subs.last().is_some_and(|s| s.closed) {
                subs.push(Sub {
                    start: at,
                    ..Sub::default()
                });
            }
        };
        let (mut c_next, mut q_next) = (None, None);
        match cmd.to_ascii_uppercase() {
            b'M' => {
                let Some(q) = p.point() else { break };
                at = base + q;
                start = at;
                subs.push(Sub {
                    start: at,
                    ..Sub::default()
                });
                // Further pairs after M are lines.
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                let Some(q) = p.point() else { break };
                current(&mut subs, at);
                at = base + q;
                push(&mut subs, Seg::Line(at));
            }
            b'H' => {
                let Some(x) = p.number() else { break };
                current(&mut subs, at);
                at = DVec2::new(if rel { at.x + x } else { x }, at.y);
                push(&mut subs, Seg::Line(at));
            }
            b'V' => {
                let Some(y) = p.number() else { break };
                current(&mut subs, at);
                at = DVec2::new(at.x, if rel { at.y + y } else { y });
                push(&mut subs, Seg::Line(at));
            }
            b'C' => {
                let (Some(c1), Some(c2), Some(q)) = (p.point(), p.point(), p.point()) else {
                    break;
                };
                current(&mut subs, at);
                let (c1, c2, q) = (base + c1, base + c2, base + q);
                push(&mut subs, Seg::Cubic(c1, c2, q));
                c_next = Some(c2);
                at = q;
            }
            b'S' => {
                let (Some(c2), Some(q)) = (p.point(), p.point()) else {
                    break;
                };
                current(&mut subs, at);
                let c1 = last_c.map_or(at, |c| 2.0 * at - c);
                let (c2, q) = (base + c2, base + q);
                push(&mut subs, Seg::Cubic(c1, c2, q));
                c_next = Some(c2);
                at = q;
            }
            b'Q' | b'T' => {
                let ctrl = if cmd.eq_ignore_ascii_case(&b'Q') {
                    let Some(c) = p.point() else { break };
                    base + c
                } else {
                    last_q.map_or(at, |c| 2.0 * at - c)
                };
                let Some(q) = p.point() else { break };
                current(&mut subs, at);
                let q = base + q;
                // A quadratic raised to a cubic exactly.
                let c1 = at + (ctrl - at) * (2.0 / 3.0);
                let c2 = q + (ctrl - q) * (2.0 / 3.0);
                push(&mut subs, Seg::Cubic(c1, c2, q));
                q_next = Some(ctrl);
                at = q;
            }
            b'A' => {
                let (Some(rx), Some(ry), Some(turn), Some(large), Some(sweep), Some(q)) = (
                    p.number(),
                    p.number(),
                    p.number(),
                    p.flag(),
                    p.flag(),
                    p.point(),
                ) else {
                    break;
                };
                current(&mut subs, at);
                let to = base + q;
                push(
                    &mut subs,
                    Seg::Arc {
                        rx,
                        ry,
                        turn,
                        large,
                        sweep,
                        to,
                    },
                );
                at = to;
            }
            b'Z' => {
                if let Some(s) = subs.last_mut()
                    && !s.closed
                {
                    s.closed = true;
                }
                at = start;
                // Z takes no numbers: a number after it starts nothing.
                cmd = 0;
            }
            _ => break,
        }
        last_c = c_next;
        last_q = q_next;
    }
    subs.retain(|s| !s.segs.is_empty());
    subs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{ImportFormat, place};
    use glam::DMat4;
    use smith_model::Document;

    /// Imports SVG `text` into a new model: the model and its report.
    fn import(text: &str) -> (Document, crate::import::ImportReport) {
        let d = std::env::temp_dir().join(format!(
            "smith-io-svg-{}-{}",
            std::process::id(),
            text.len()
        ));
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("logo.svg");
        std::fs::write(&path, text).unwrap();
        let imported = read_svg(&path, &ImportOptions::for_format(ImportFormat::Svg)).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
        let mut doc = Document::new();
        let (_, report) = doc
            .transaction("Import", |dd| place(dd, None, DMat4::IDENTITY, &imported))
            .unwrap();
        (doc, report)
    }

    fn size(doc: &Document) -> DVec3 {
        let (lo, hi) = doc.model_bounds().unwrap();
        hi - lo
    }

    /// Every curve in the model, by kind.
    fn curve_kinds(doc: &Document) -> Vec<&'static str> {
        doc.definition_ids()
            .filter_map(|k| doc.definition_entities(k))
            .flat_map(|e| {
                e.curve_ids()
                    .filter_map(|c| e.curve(c))
                    .map(|c| match c.curve() {
                        Curve::Circle(_) => "circle",
                        Curve::Arc(_) => "arc",
                        Curve::Bezier(_) => "bezier",
                        _ => "other",
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn a_96_dpi_logo_comes_in_at_true_size_with_its_hole() {
        // One inch square with a half-inch square hole, by both fill rules
        // (the hole wound the other way for non-zero).
        for rule in ["evenodd", "nonzero"] {
            let (doc, report) = import(&format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96">
                <path fill-rule="{rule}" d="M0 0 H96 V96 H0 Z M24 24 V72 H72 V24 Z"/></svg>"#
            ));
            assert!((size(&doc) - DVec3::new(25.4, 25.4, 0.0)).length() < 1e-9);
            assert_eq!(report.counts.faces, 1, "{rule}");
        }
        // Wound the same way, non-zero fills the middle too.
        let (_, report) = import(
            r#"<svg width="96" height="96"><path d="M0 0 H96 V96 H0 Z M24 24 H72 V72 H24 Z"/></svg>"#,
        );
        assert_eq!(report.counts.faces, 2);
    }

    #[test]
    fn real_units_through_the_viewbox_and_our_own_export() {
        // 100 × 50 mm drawn in a 200 × 100 viewBox.
        let (doc, _) = import(
            r#"<svg width="100mm" height="50mm" viewBox="0 0 200 100"><rect width="200" height="100"/></svg>"#,
        );
        assert!((size(&doc) - DVec3::new(100.0, 50.0, 0.0)).length() < 1e-9);
        // An SVG this app exported: a 50 mm square line drawing on A4.
        let line = |a: DVec2, b: DVec2| crate::export::SheetLine {
            a,
            b,
            weight: 0.25,
            pattern: smith_model::LinePattern::Solid,
            layer: "Base".into(),
        };
        let c = [
            DVec2::new(80.0, 120.0),
            DVec2::new(130.0, 120.0),
            DVec2::new(130.0, 170.0),
            DVec2::new(80.0, 170.0),
        ];
        let sheet = crate::export::Sheet {
            size: DVec2::new(210.0, 297.0),
            lines: (0..4).map(|i| line(c[i], c[(i + 1) % 4])).collect(),
            mark_lines: Vec::new(),
            arrows: Vec::new(),
            texts: Vec::new(),
            fits: true,
        };
        let (doc, report) = import(&crate::export::svg_text_for_tests_inner("square", &sheet));
        assert!((size(&doc) - DVec3::new(50.0, 50.0, 0.0)).length() < 1e-6);
        // Our export draws lines without fill: edges, no faces.
        assert_eq!(report.counts.faces, 0);
    }

    #[test]
    fn curves_stay_curves() {
        let (doc, report) = import(
            r#"<svg width="100" height="100">
              <circle cx="20" cy="20" r="10"/>
              <path d="M40 40 C50 30 60 30 70 40 S90 50 90 40 Z"/>
              <path d="M10 70 A20 10 0 0 1 50 70 Z"/>
              <path d="M60 70 A10 10 0 0 1 80 70 Z"/>
              <rect x="0" y="90" width="20" height="10" rx="2" fill="none"/>
            </svg>"#,
        );
        let kinds = curve_kinds(&doc);
        assert!(kinds.contains(&"circle"));
        assert!(kinds.contains(&"bezier"));
        assert!(kinds.iter().filter(|k| **k == "arc").count() >= 5);
        // Circle 1; the cubic shape 2 (its closing line runs through its
        // own middle point, where the curve crosses over: two filled
        // regions, as any viewer shows); half ellipse 1; half disc 1. The
        // unfilled rounded rectangle has none.
        assert_eq!(report.counts.faces, 5);
    }

    #[test]
    fn transforms_groups_use_text_and_images() {
        let (doc, report) = import(
            r##"<svg xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:inkscape="i" width="96" height="96">
              <defs><rect id="tile" width="10" height="10"/></defs>
              <g inkscape:label="Badge" transform="translate(10,0) scale(2)" style="fill:#f00">
                <rect width="10" height="10"/>
                <use xlink:href="#tile" x="20"/>
              </g>
              <g display="none"><rect width="500" height="500"/></g>
              <text x="0" y="50">Hello</text>
              <image href="a.png" width="5" height="5"/>
            </svg>"##,
        );
        assert_eq!(report.counts.faces, 2);
        assert_eq!(report.texts, 1);
        assert_eq!(report.images, 1);
        assert_eq!(
            report.lines(),
            [
                "1 Text Skipped: Convert Text to Paths First",
                "1 Image Skipped"
            ]
        );
        // From x 10 to 10 + 2 × 30 = 70 px wide.
        assert!((size(&doc).x - 60.0 * 25.4 / 96.0).abs() < 1e-9);
        let named = doc
            .definition_ids()
            .any(|k| doc.definition(k).is_some_and(|d| d.name == "Badge"));
        assert!(named);
    }

    #[test]
    fn path_data_as_svg_allows() {
        // Numbers run together (-5.5.5.5 is −5.5, .5, .5), flags packed
        // (011 is flags 0 and 1, then 1).
        let subs = parse_path("M0,0L10-5.5.5.5zm1 1h2v2a1 1 0 011 1Q5 5 6 6t1 1");
        assert_eq!(subs.len(), 2);
        assert_eq!(
            subs[0].segs,
            [
                Seg::Line(DVec2::new(10.0, -5.5)),
                Seg::Line(DVec2::new(0.5, 0.5))
            ]
        );
        assert!(subs[0].closed);
        // The second starts at (1, 1), relative to where the first closed.
        assert_eq!(subs[1].start, DVec2::new(1.0, 1.0));
        assert_eq!(subs[1].segs.len(), 5);
        assert!(matches!(
            subs[1].segs[2],
            Seg::Arc { large: false, sweep: true, to, .. } if to == DVec2::new(4.0, 4.0)
        ));
        assert!(matches!(subs[1].segs[4], Seg::Cubic(_, _, to) if to == DVec2::new(7.0, 7.0)));
        assert_eq!(length_mm("25.4mm"), Some(25.4));
        assert_eq!(length_mm("1in"), Some(25.4));
        assert_eq!(length_mm("96"), Some(25.4));
        assert_eq!(length_mm("50%"), None);
    }
}
