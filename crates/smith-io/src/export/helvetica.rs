//! SketchSmith — Helvetica widths | v1.0.0 | Thursday 12 November 2026
//!
//! The advance widths of PDF's built-in Helvetica for the characters of
//! WinAnsiEncoding, so drawing text can be centred without embedding a
//! font. Taken from Adobe's Core 14 AFM file `Helvetica.afm` (glyphs looked
//! up by name for each WinAnsi code); its notice, kept as the terms ask:
//!
//! > Copyright (c) 1985, 1987, 1989, 1990, 1997 Adobe Systems Incorporated.
//! > All Rights Reserved. Helvetica is a trademark of Linotype-Hell AG
//! > and/or its subsidiaries.
//!
//! The terms are in `THIRD_PARTY_NOTICES.md`.

/// Widths in thousandths of the font size, for codes 32 to 255.
const WIDTHS: [u16; 224] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
    0, 556, 0, 222, 556, 333, 1000, 556, 556, 333, 1000, 667, 333, 1000, 0, 611, 0, 0, 222, 222,
    333, 333, 350, 556, 1000, 333, 1000, 500, 333, 944, 0, 500, 667, 278, 333, 556, 556, 556, 556,
    260, 556, 333, 737, 370, 556, 584, 333, 737, 333, 400, 584, 333, 333, 333, 556, 537, 278, 333,
    333, 365, 556, 834, 834, 834, 611, 667, 667, 667, 667, 667, 667, 1000, 722, 667, 667, 667, 667,
    278, 278, 278, 278, 722, 722, 778, 778, 778, 778, 778, 584, 778, 722, 722, 722, 722, 667, 667,
    611, 556, 556, 556, 556, 556, 556, 889, 500, 556, 556, 556, 556, 278, 278, 278, 278, 556, 556,
    556, 556, 556, 556, 556, 584, 611, 556, 556, 556, 556, 500, 556, 500,
];

/// The WinAnsiEncoding code of `c`, if Helvetica has it.
pub(crate) fn code(c: char) -> Option<u8> {
    let u = u32::from(c);
    let code = match u {
        0x20..=0x7E | 0xA0..=0xFF => u as u8,
        _ => match c {
            '€' => 0x80,
            '‚' => 0x82,
            'ƒ' => 0x83,
            '„' => 0x84,
            '…' => 0x85,
            '†' => 0x86,
            '‡' => 0x87,
            'ˆ' => 0x88,
            '‰' => 0x89,
            'Š' => 0x8A,
            '‹' => 0x8B,
            'Œ' => 0x8C,
            'Ž' => 0x8E,
            '\u{2018}' => 0x91,
            '\u{2019}' => 0x92,
            '\u{201C}' => 0x93,
            '\u{201D}' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            '˜' => 0x98,
            '™' => 0x99,
            'š' => 0x9A,
            '›' => 0x9B,
            'œ' => 0x9C,
            'ž' => 0x9E,
            'Ÿ' => 0x9F,
            _ => return None,
        },
    };
    (WIDTHS[usize::from(code) - 32] > 0).then_some(code)
}

/// The width of WinAnsi `code`, in thousandths of the font size.
pub(crate) fn width(code: u8) -> u16 {
    code.checked_sub(32)
        .and_then(|i| WIDTHS.get(usize::from(i)))
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_match_the_metrics() {
        assert_eq!(width(code(' ').unwrap()), 278);
        assert_eq!(width(code('0').unwrap()), 556);
        assert_eq!(width(code('M').unwrap()), 833);
        assert_eq!(width(code('Ø').unwrap()), 778);
        assert_eq!(width(code('°').unwrap()), 400);
        assert_eq!(code('€'), Some(0x80));
        assert_eq!(code('中'), None);
    }
}
