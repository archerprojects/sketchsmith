//! SketchSmith — units, typed values and formatting | v1.0.0 | Thursday 12 November 2026
//!
//! The typed-value grammar, shared by typing over the model and
//! every Operation Inputs field.
//!
//! **Lengths** accept a unit on any term (`mm`, `cm`, `m`, `in` or `"`, `ft` or
//! `'`) and simple arithmetic: `+ − * /` and parentheses. A unit written after a
//! product or quotient applies to the whole term, so `3/8"` is three-eighths of
//! an inch and `2*6.35mm` is 12.7 mm. A term with a unit followed directly by a
//! number continues as a sum, so `2'6"` is 2 ft + 6 in. A number with no unit
//! uses the document's display unit.
//!
//! **Value forms** (whole input): `a,b` (two lengths), `a,b,c` (3D delta),
//! `[x,y,z]` (absolute point), `<x,y,z>` (relative point), `5x` or `*5`
//! (external array), `/5` (internal array), `24s` (segments), `5r` (radius).
//!
//! **Formatting** shows a length in the display unit at the display precision,
//! prefixed with `~` when the shown value is rounded. Values are
//! always stored at full `f64` precision; rounding is display-only.

use std::fmt;

use smith_geom::DVec3;

/// Millimetres per inch.
pub const MM_PER_INCH: f64 = 25.4;
/// Millimetres per foot (written out, so `1'` is exactly 304.8 in `f64`).
pub const MM_PER_FOOT: f64 = 304.8;

/// A display/default length unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    /// Millimetres (default).
    #[default]
    Millimetre,
    /// Centimetres.
    Centimetre,
    /// Metres.
    Metre,
    /// Decimal inches.
    DecimalInch,
    /// Fractional inches (1/2ⁿ).
    FractionalInch,
    /// Feet and fractional inches.
    FeetInches,
}

impl LengthUnit {
    /// Millimetres per unit, for input interpretation.
    pub fn mm_per_unit(self) -> f64 {
        match self {
            Self::Millimetre => 1.0,
            Self::Centimetre => 10.0,
            Self::Metre => 1000.0,
            Self::DecimalInch | Self::FractionalInch | Self::FeetInches => MM_PER_INCH,
        }
    }

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Millimetre => "Millimetres",
            Self::Centimetre => "Centimetres",
            Self::Metre => "Metres",
            Self::DecimalInch => "Decimal inches",
            Self::FractionalInch => "Fractional inches",
            Self::FeetInches => "Feet and inches",
        }
    }

    /// Every unit, in menu order.
    pub const ALL: [LengthUnit; 6] = [
        Self::Millimetre,
        Self::Centimetre,
        Self::Metre,
        Self::DecimalInch,
        Self::FractionalInch,
        Self::FeetInches,
    ];
}

/// Display settings for lengths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UnitSettings {
    /// Display and default input unit.
    pub unit: LengthUnit,
    /// Decimal places for decimal units.
    pub decimals: u8,
    /// Fraction denominator exponent for fractional units (4 → 1/16").
    pub fraction_bits: u8,
}

impl Default for UnitSettings {
    fn default() -> Self {
        Self {
            unit: LengthUnit::Millimetre,
            decimals: 2,
            fraction_bits: 4,
        }
    }
}

/// A problem with typed input, with the position it was found at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputError {
    /// What went wrong, in plain words.
    pub message: String,
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for InputError {}

fn err<T>(message: impl Into<String>) -> Result<T, InputError> {
    Err(InputError {
        message: message.into(),
    })
}

// ----- tokenizer ---------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok {
    Num(f64),
    Unit(f64), // mm per unit
    Plus,
    Minus,
    Star,
    Slash,
    Open,
    Close,
}

fn tokenize(text: &str) -> Result<Vec<Tok>, InputError> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' => i += 1,
            '+' => {
                out.push(Tok::Plus);
                i += 1;
            }
            '-' | '−' => {
                out.push(Tok::Minus);
                i += 1;
            }
            '*' | '×' => {
                out.push(Tok::Star);
                i += 1;
            }
            '/' => {
                out.push(Tok::Slash);
                i += 1;
            }
            '(' => {
                out.push(Tok::Open);
                i += 1;
            }
            ')' => {
                out.push(Tok::Close);
                i += 1;
            }
            '"' => {
                out.push(Tok::Unit(MM_PER_INCH));
                i += 1;
            }
            '\'' => {
                out.push(Tok::Unit(MM_PER_FOOT));
                i += 1;
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                let v: f64 = s.parse().map_err(|_| InputError {
                    message: format!("`{s}` is not a number"),
                })?;
                out.push(Tok::Num(v));
            }
            c if c.is_ascii_alphabetic() => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let s: String = chars[start..i]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase();
                let f = match s.as_str() {
                    "mm" => 1.0,
                    "cm" => 10.0,
                    "m" => 1000.0,
                    "in" | "inch" | "inches" => MM_PER_INCH,
                    "ft" | "feet" | "foot" => MM_PER_FOOT,
                    _ => return err(format!("unknown unit `{s}`")),
                };
                out.push(Tok::Unit(f));
            }
            _ => return err(format!("unexpected `{c}`")),
        }
    }
    Ok(out)
}

// ----- expression parser ---------------------------------------------------------

/// A partially evaluated value: `raw` in `unit` (or unitless).
#[derive(Debug, Clone, Copy)]
struct Val {
    raw: f64,
    /// `Some(mm per unit)` once a unit is known for this value.
    unit: Option<f64>,
}

impl Val {
    fn mm(self, default: f64) -> f64 {
        self.raw * self.unit.unwrap_or(default)
    }
}

struct Parser<'a> {
    toks: &'a [Tok],
    pos: usize,
    default: f64,
}

impl Parser<'_> {
    fn peek(&self) -> Option<Tok> {
        self.toks.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.peek();
        self.pos += 1;
        t
    }

    /// expr := term (('+' | '−') term | term-after-unit)*
    ///
    /// Terms with units are summed in mm; unitless terms are summed in raw
    /// numbers, so a purely unitless expression stays exact until the default
    /// unit is applied once at the end.
    fn expr(&mut self) -> Result<Val, InputError> {
        let mut mm_total = 0.0;
        let mut raw_total = 0.0;
        let mut any_unit = false;
        let mut add = |v: Val, sign: f64, any_unit: &mut bool| match v.unit {
            Some(u) => {
                *any_unit = true;
                mm_total += sign * v.raw * u;
            }
            None => raw_total += sign * v.raw,
        };
        let first = self.term()?;
        let mut last_had_unit = first.unit.is_some();
        add(first, 1.0, &mut any_unit);
        loop {
            match self.peek() {
                Some(Tok::Plus) | Some(Tok::Minus) => {
                    let sign = if self.next() == Some(Tok::Plus) {
                        1.0
                    } else {
                        -1.0
                    };
                    let t = self.term()?;
                    last_had_unit = t.unit.is_some();
                    add(t, sign, &mut any_unit);
                }
                // `2'6"`: a number right after a unit-terminated term continues the sum.
                Some(Tok::Num(_)) | Some(Tok::Open) if last_had_unit => {
                    let t = self.term()?;
                    last_had_unit = t.unit.is_some();
                    add(t, 1.0, &mut any_unit);
                }
                _ => break,
            }
        }
        // With any unit present, report in mm (unitless parts take the default
        // unit); otherwise stay unitless so an outer unit (`(1+2)mm`) can apply.
        Ok(if any_unit {
            Val {
                raw: mm_total + raw_total * self.default,
                unit: Some(1.0),
            }
        } else {
            Val {
                raw: raw_total,
                unit: None,
            }
        })
    }

    /// term := factor (('*' | '/') factor)* with at most one unit anywhere in it.
    fn term(&mut self) -> Result<Val, InputError> {
        let mut v = self.factor()?;
        loop {
            match self.peek() {
                Some(Tok::Unit(f)) => {
                    self.next();
                    if v.unit.is_some() {
                        return err("two units in one term");
                    }
                    v.unit = Some(f);
                    // A unit ends the term unless more multiplication follows.
                    if !matches!(self.peek(), Some(Tok::Star) | Some(Tok::Slash)) {
                        return Ok(v);
                    }
                }
                Some(Tok::Star) | Some(Tok::Slash) => {
                    let mul = self.next() == Some(Tok::Star);
                    let rhs = self.factor()?;
                    if rhs.unit.is_some() && v.unit.is_some() {
                        return err("cannot multiply two lengths");
                    }
                    if !mul && rhs.raw == 0.0 {
                        return err("division by zero");
                    }
                    v = Val {
                        raw: if mul {
                            v.raw * rhs.raw
                        } else {
                            v.raw / rhs.raw
                        },
                        unit: v.unit.or(rhs.unit),
                    };
                }
                _ => return Ok(v),
            }
        }
    }

    /// factor := number | '(' expr ')' | '−' factor
    fn factor(&mut self) -> Result<Val, InputError> {
        match self.next() {
            Some(Tok::Num(n)) => {
                // Mixed number: `6 1/2` means 6 + 1/2.
                if let (Some(Tok::Num(b)), Some(Tok::Slash), Some(Tok::Num(c))) = (
                    self.toks.get(self.pos).copied(),
                    self.toks.get(self.pos + 1).copied(),
                    self.toks.get(self.pos + 2).copied(),
                ) && c != 0.0
                {
                    self.pos += 3;
                    return Ok(Val {
                        raw: n + b / c,
                        unit: None,
                    });
                }
                Ok(Val { raw: n, unit: None })
            }
            Some(Tok::Minus) => {
                let v = self.factor()?;
                Ok(Val { raw: -v.raw, ..v })
            }
            Some(Tok::Open) => {
                let v = self.expr()?;
                if self.next() != Some(Tok::Close) {
                    return err("missing `)`");
                }
                Ok(v)
            }
            Some(_) => err("expected a number"),
            None => err("value is incomplete"),
        }
    }
}

/// Parses one length expression to millimetres. Bare numbers use `default`.
///
/// # Errors
/// A plain-language [`InputError`] for malformed input.
pub fn parse_length(text: &str, default: LengthUnit) -> Result<f64, InputError> {
    let toks = tokenize(text)?;
    if toks.is_empty() {
        return err("nothing typed");
    }
    let mut p = Parser {
        toks: &toks,
        pos: 0,
        default: default.mm_per_unit(),
    };
    let v = p.expr()?;
    if p.pos < toks.len() {
        return err("unexpected text after the value");
    }
    let mm = v.mm(p.default);
    if mm.is_finite() {
        Ok(mm)
    } else {
        err("value is out of range")
    }
}

/// Parses a positive whole count (allows arithmetic, e.g. `12/3`).
fn parse_count(text: &str) -> Result<u32, InputError> {
    let toks = tokenize(text)?;
    let mut p = Parser {
        toks: &toks,
        pos: 0,
        default: 1.0,
    };
    let v = p.expr()?;
    if p.pos < toks.len() || v.unit.is_some() {
        return err("expected a whole number");
    }
    let n = v.raw.round();
    if (v.raw - n).abs() > 1e-9 || !(1.0..=1e6).contains(&n) {
        return err("expected a whole number of at least 1");
    }
    Ok(n as u32)
}

/// A complete measurement-box entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TypedValue {
    /// One length, mm.
    Length(f64),
    /// Two lengths (`a,b`), mm.
    Pair(f64, f64),
    /// A 3D delta (`a,b,c`), mm.
    Delta(DVec3),
    /// An absolute point (`[x,y,z]`), mm.
    Absolute(DVec3),
    /// A relative point (`<x,y,z>`), mm.
    Relative(DVec3),
    /// External array: `5x` or `*5`.
    Copies(u32),
    /// Internal array: `/5`.
    Divide(u32),
    /// Segment count: `24s`.
    Segments(u32),
    /// Radius: `5r`, mm.
    Radius(f64),
}

/// Parses a whole measurement-box entry.
///
/// # Errors
/// A plain-language [`InputError`].
pub fn parse_typed(text: &str, default: LengthUnit) -> Result<TypedValue, InputError> {
    let t = text.trim();
    if t.is_empty() {
        return err("nothing typed");
    }
    let lower = t.to_ascii_lowercase();
    if let Some(rest) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return Ok(TypedValue::Absolute(parse_vec(rest, default)?));
    }
    if let Some(rest) = t.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
        return Ok(TypedValue::Relative(parse_vec(rest, default)?));
    }
    if let Some(rest) = t.strip_prefix('*') {
        return Ok(TypedValue::Copies(parse_count(rest)?));
    }
    if let Some(rest) = t.strip_prefix('/') {
        return Ok(TypedValue::Divide(parse_count(rest)?));
    }
    if let Some(rest) = lower.strip_suffix('x') {
        return Ok(TypedValue::Copies(parse_count(rest)?));
    }
    if let Some(rest) = lower.strip_suffix('s') {
        return Ok(TypedValue::Segments(parse_count(rest)?));
    }
    if let Some(rest) = lower.strip_suffix('r') {
        return Ok(TypedValue::Radius(parse_length(rest, default)?));
    }
    let parts: Vec<&str> = t.split(',').collect();
    match parts.as_slice() {
        [a] => Ok(TypedValue::Length(parse_length(a, default)?)),
        [a, b] => Ok(TypedValue::Pair(
            parse_length(a, default)?,
            parse_length(b, default)?,
        )),
        [a, b, c] => Ok(TypedValue::Delta(DVec3::new(
            parse_length(a, default)?,
            parse_length(b, default)?,
            parse_length(c, default)?,
        ))),
        _ => err("too many values; use up to three separated by commas"),
    }
}

fn parse_vec(text: &str, default: LengthUnit) -> Result<DVec3, InputError> {
    let parts: Vec<&str> = text.split(',').collect();
    let [x, y, z] = parts.as_slice() else {
        return err("a point needs three values: x,y,z");
    };
    Ok(DVec3::new(
        parse_length(x, default)?,
        parse_length(y, default)?,
        parse_length(z, default)?,
    ))
}

// ----- formatting ----------------------------------------------------------------

/// Formats a length (mm) for display: unit, precision, and a leading `~` when
/// the shown value is rounded.
pub fn format_length(mm: f64, s: &UnitSettings) -> String {
    let (text, exact) = match s.unit {
        LengthUnit::Millimetre => decimal(mm, 1.0, s.decimals, "mm"),
        LengthUnit::Centimetre => decimal(mm, 10.0, s.decimals, "cm"),
        LengthUnit::Metre => decimal(mm, 1000.0, s.decimals, "m"),
        LengthUnit::DecimalInch => decimal(mm, MM_PER_INCH, s.decimals, "\""),
        LengthUnit::FractionalInch => fractional(mm / MM_PER_INCH, s.fraction_bits, None),
        LengthUnit::FeetInches => {
            let inches = mm / MM_PER_INCH;
            let sign = if inches < 0.0 { "-" } else { "" };
            let feet = (inches.abs() / 12.0).floor();
            let (rest, exact) = fractional(inches.abs() - feet * 12.0, s.fraction_bits, Some(feet));
            (format!("{sign}{rest}"), exact)
        }
    };
    if exact { text } else { format!("~ {text}") }
}

/// Formats a stored length (mm) at full precision for an editable field
///: the value in the display unit, with no rounding marker and
/// trailing zeros trimmed, so the field reads back to the same number. Decimal
/// units have no suffix (the parser reads a bare number in the display unit);
/// fractional and feet-and-inch units show decimal inches with `"`.
pub fn format_exact(mm: f64, s: &UnitSettings) -> String {
    let (value, suffix) = match s.unit {
        LengthUnit::Millimetre => (mm, ""),
        LengthUnit::Centimetre => (mm / 10.0, ""),
        LengthUnit::Metre => (mm / 1000.0, ""),
        LengthUnit::DecimalInch => (mm / MM_PER_INCH, ""),
        LengthUnit::FractionalInch | LengthUnit::FeetInches => (mm / MM_PER_INCH, "\""),
    };
    let mut text = format!("{value:.10}");
    if text.contains('.') {
        text = text.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    if text == "-0" {
        text = "0".to_owned();
    }
    format!("{text}{suffix}")
}

/// Formats an area (mm²) in the display unit squared.
pub fn format_area(mm2: f64, s: &UnitSettings) -> String {
    let (div, suffix) = match s.unit {
        LengthUnit::Millimetre => (1.0, "mm²"),
        LengthUnit::Centimetre => (100.0, "cm²"),
        LengthUnit::Metre => (1e6, "m²"),
        _ => (MM_PER_INCH * MM_PER_INCH, "in²"),
    };
    let v = mm2 / div;
    let shown = format!("{v:.prec$}", prec = usize::from(s.decimals));
    let exact = (shown.parse::<f64>().unwrap_or(f64::NAN) - v).abs() <= v.abs() * 1e-12;
    let text = format!("{shown} {suffix}");
    if exact { text } else { format!("~ {text}") }
}

/// A volume in mm³ shown in the display unit cubed (marked `~` when
/// rounded).
pub fn format_volume(mm3: f64, s: &UnitSettings) -> String {
    let (div, suffix) = match s.unit {
        LengthUnit::Millimetre => (1.0, "mm³"),
        LengthUnit::Centimetre => (1000.0, "cm³"),
        LengthUnit::Metre => (1e9, "m³"),
        _ => (MM_PER_INCH * MM_PER_INCH * MM_PER_INCH, "in³"),
    };
    let v = mm3 / div;
    let shown = format!("{v:.prec$}", prec = usize::from(s.decimals));
    let exact = (shown.parse::<f64>().unwrap_or(f64::NAN) - v).abs() <= v.abs() * 1e-12;
    let text = format!("{shown} {suffix}");
    if exact { text } else { format!("~ {text}") }
}

fn decimal(mm: f64, per: f64, decimals: u8, suffix: &str) -> (String, bool) {
    let v = mm / per;
    let shown = format!("{v:.prec$}", prec = usize::from(decimals));
    let back = shown.parse::<f64>().unwrap_or(f64::NAN);
    let exact = (back - v).abs() <= v.abs().max(1.0) * 1e-12;
    let gap = if suffix == "\"" { "" } else { " " };
    (format!("{shown}{gap}{suffix}"), exact)
}

/// Inches as whole + fraction to 1/2^bits, optionally after a feet count.
fn fractional(inches: f64, bits: u8, feet: Option<f64>) -> (String, bool) {
    let denom = 1u64 << bits.min(8);
    let sign = if inches < 0.0 && feet.is_none() {
        "-"
    } else {
        ""
    };
    let total = (inches.abs() * denom as f64).round() as u64;
    let exact = ((total as f64) / denom as f64 - inches.abs()).abs() <= 1e-9;
    let (mut whole, mut num) = (total / denom, total % denom);
    let mut den = denom;
    while num > 0 && num % 2 == 0 {
        num /= 2;
        den /= 2;
    }
    let carry_feet = feet.is_some();
    let mut feet = feet.unwrap_or(0.0);
    if carry_feet && whole >= 12 {
        feet += 1.0;
        whole -= 12;
    }
    let inch_part = match (whole, num) {
        (w, 0) => format!("{w}\""),
        (0, n) => format!("{n}/{den}\""),
        (w, n) => format!("{w} {n}/{den}\""),
    };
    let text = if carry_feet && feet > 0.0 {
        format!("{sign}{feet}' {inch_part}")
    } else {
        format!("{sign}{inch_part}")
    };
    (text, exact)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MM: LengthUnit = LengthUnit::Millimetre;

    fn len(s: &str) -> f64 {
        parse_length(s, MM).unwrap()
    }

    /// Equal to within the last few bits of an `f64` at this magnitude.
    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= b.abs().max(1.0) * 1e-12
    }

    #[test]
    fn plain_and_units() {
        assert_eq!(len("12.5"), 12.5);
        assert_eq!(len("12.5mm"), 12.5);
        assert_eq!(len("2cm"), 20.0);
        assert_eq!(len("1.5 m"), 1500.0);
        assert_eq!(len("1in"), 25.4);
        assert_eq!(len("1\""), 25.4);
        assert_eq!(len("1'"), 304.8);
        assert_eq!(parse_length("1", LengthUnit::DecimalInch).unwrap(), 25.4);
    }

    #[test]
    fn arithmetic_from_the_spec() {
        // Acceptance examples.
        assert_eq!(len("40/3"), 40.0 / 3.0);
        assert!((len("3/8\" + 1mm") - 10.525).abs() < 1e-12);
        assert_eq!(len("(25-1.5)/2"), 11.75);
        assert_eq!(len("2*6.35"), 12.7);
        assert_eq!(len("2*6.35mm"), 12.7);
        assert_eq!(len("(1+2)mm"), 3.0);
        assert_eq!(len("40mm/2"), 20.0);
        assert_eq!(len("-3"), -3.0);
    }

    #[test]
    fn fractions_and_feet_inches() {
        assert!(close(len("3/8\""), 0.375 * 25.4));
        assert!(close(len("2'6\""), 30.0 * 25.4));
        assert!(close(len("2' 6\""), 30.0 * 25.4));
        assert!(close(len("1' 6 1/2\""), 18.5 * 25.4));
        assert!(close(len("6 1/2\""), 6.5 * 25.4));
    }

    #[test]
    fn errors_are_clear() {
        for bad in ["", "12furlongs", "3mm*2mm", "(2+3", "4/0", "1..2", "5 ?"] {
            let e = parse_length(bad, MM).unwrap_err();
            assert!(!e.message.is_empty(), "{bad}");
        }
    }

    #[test]
    fn value_forms() {
        assert_eq!(
            parse_typed("20,30", MM).unwrap(),
            TypedValue::Pair(20.0, 30.0)
        );
        assert_eq!(
            parse_typed("1,2,3", MM).unwrap(),
            TypedValue::Delta(DVec3::new(1.0, 2.0, 3.0))
        );
        assert_eq!(
            parse_typed("[1,2,3cm]", MM).unwrap(),
            TypedValue::Absolute(DVec3::new(1.0, 2.0, 30.0))
        );
        assert_eq!(
            parse_typed("<0,0,5>", MM).unwrap(),
            TypedValue::Relative(DVec3::new(0.0, 0.0, 5.0))
        );
        assert_eq!(parse_typed("5x", MM).unwrap(), TypedValue::Copies(5));
        assert_eq!(parse_typed("*5", MM).unwrap(), TypedValue::Copies(5));
        assert_eq!(parse_typed("/4", MM).unwrap(), TypedValue::Divide(4));
        assert_eq!(parse_typed("24s", MM).unwrap(), TypedValue::Segments(24));
        assert_eq!(parse_typed("0.5r", MM).unwrap(), TypedValue::Radius(0.5));
        assert!(parse_typed("2.5x", MM).is_err());
        assert!(parse_typed("[1,2]", MM).is_err());
        assert!(parse_typed("1,2,3,4", MM).is_err());
    }

    #[test]
    fn formatting_and_rounding_marker() {
        let s = UnitSettings::default();
        assert_eq!(format_length(12.5, &s), "12.50 mm");
        assert_eq!(format_length(12.3456, &s), "~ 12.35 mm");
        let cm = UnitSettings {
            unit: LengthUnit::Centimetre,
            ..s
        };
        assert_eq!(format_length(25.0, &cm), "2.50 cm");
        let fr = UnitSettings {
            unit: LengthUnit::FractionalInch,
            ..s
        };
        assert_eq!(format_length(0.375 * 25.4, &fr), "3/8\"");
        assert_eq!(format_length(1.5 * 25.4, &fr), "1 1/2\"");
        assert!(format_length(1.0, &fr).starts_with("~ "));
        let ft = UnitSettings {
            unit: LengthUnit::FeetInches,
            ..s
        };
        assert_eq!(format_length(30.0 * 25.4, &ft), "2' 6\"");
        assert_eq!(format_length(18.5 * 25.4, &ft), "1' 6 1/2\"");
        assert_eq!(format_area(200.0, &s), "200.00 mm²");
    }

    #[test]
    fn parse_format_round_trip_in_every_unit() {
        // Shown values parse back to within half of the last displayed digit.
        for unit in LengthUnit::ALL {
            let s = UnitSettings {
                unit,
                decimals: 4,
                fraction_bits: 6,
            };
            let v = 25.4 * 1.25;
            let shown = format_length(v, &s);
            let back = parse_length(shown.trim_start_matches("~ "), unit).unwrap();
            let tolerance = unit.mm_per_unit() * 0.5e-4 * 1.001; // half the last shown digit
            assert!((back - v).abs() <= tolerance, "{unit:?}: {shown} -> {back}");
        }
    }

    #[test]
    fn exact_format_reads_back_unchanged() {
        let mm = UnitSettings::default();
        assert_eq!(format_exact(12.3456, &mm), "12.3456");
        assert_eq!(format_exact(600.0, &mm), "600");
        assert_eq!(format_exact(40.0 / 3.0, &mm), "13.3333333333");
        let inch = UnitSettings {
            unit: LengthUnit::FractionalInch,
            ..UnitSettings::default()
        };
        assert_eq!(format_exact(25.4 * 0.375, &inch), "0.375\"");
        let back = parse_length(&format_exact(12.3456, &mm), LengthUnit::Millimetre).unwrap();
        assert_eq!(back, 12.3456);
    }
}
