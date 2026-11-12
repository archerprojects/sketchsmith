//! SketchSmith — SKP byte cursor | v1.0.0 | Thursday 12 November 2026
//!
//! Reading the archive's bytes. Every read is checked against the end of the
//! data: running off it is a [`Fault::Bounds`], never a panic. Like the
//! original reader, a raw run of bytes taken past the end comes back short
//! (and the cursor still moves on), so the next checked read fails.

use std::fmt;

/// Why a read failed. The walk tells them apart: an entity list stops at a
/// [`Fault::Parse`] where the file's own count runs past its records, while
/// running off the data or a broken class name ends the walk.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Fault {
    /// The bytes are not what the format puts there; `at` is the offset
    /// read from, when known.
    Parse { message: String, at: Option<usize> },
    /// A read ran past the end of the data.
    Bounds,
    /// A class name is not ASCII.
    Text,
}

impl Fault {
    /// A parse fault at the cursor.
    pub(crate) fn at(r: &R<'_>, message: impl Into<String>) -> Self {
        Fault::Parse {
            message: format!("{} {}", message.into(), r.ctx()),
            at: Some(r.pos),
        }
    }

    /// A parse fault with no place in the file.
    pub(crate) fn plain(message: impl Into<String>) -> Self {
        Fault::Parse {
            message: message.into(),
            at: None,
        }
    }

    /// How far into the file it got (`-1` when unknown).
    pub(crate) fn reach(&self) -> i64 {
        match self {
            Fault::Parse { at: Some(at), .. } => i64::try_from(*at).unwrap_or(i64::MAX),
            _ => -1,
        }
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Parse { message, .. } => f.write_str(message),
            Fault::Bounds => f.write_str("read past the end of the file"),
            Fault::Text => f.write_str("class name is not ASCII"),
        }
    }
}

pub(crate) type Res<T> = Result<T, Fault>;

/// The string record marker.
pub(crate) const STR_MARKER: &[u8] = b"\xff\xfe\xff";

/// `data[a..b]`, clipped to the data.
pub(crate) fn clip(data: &[u8], a: usize, b: usize) -> &[u8] {
    let end = b.min(data.len());
    if a >= end { &[] } else { &data[a..end] }
}

/// The first offset of `needle` in `data[start..end]` (clipped).
pub(crate) fn find(data: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    let hay = clip(data, start, end);
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len())
        .position(|w| w == needle)
        .map(|i| start + i)
}

/// A little-endian u16 at `at`.
pub(crate) fn u16_at(data: &[u8], at: usize) -> Res<u16> {
    let b = data.get(at..at.checked_add(2).ok_or(Fault::Bounds)?);
    b.map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(Fault::Bounds)
}

/// A little-endian u32 at `at`.
pub(crate) fn u32_at(data: &[u8], at: usize) -> Res<u32> {
    let b = data.get(at..at.checked_add(4).ok_or(Fault::Bounds)?);
    b.map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(Fault::Bounds)
}

/// A little-endian f64 at `at`.
pub(crate) fn f64_at(data: &[u8], at: usize) -> Res<f64> {
    let b = data
        .get(at..at.checked_add(8).ok_or(Fault::Bounds)?)
        .ok_or(Fault::Bounds)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(b);
    Ok(f64::from_le_bytes(a))
}

/// A byte cursor.
#[derive(Debug, Clone)]
pub(crate) struct R<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) pos: usize,
}

impl<'a> R<'a> {
    pub(crate) fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    pub(crate) fn u8(&mut self) -> Res<u8> {
        let v = *self.data.get(self.pos).ok_or(Fault::Bounds)?;
        self.pos += 1;
        Ok(v)
    }

    pub(crate) fn u16(&mut self) -> Res<u16> {
        let v = u16_at(self.data, self.pos)?;
        self.pos += 2;
        Ok(v)
    }

    pub(crate) fn u32(&mut self) -> Res<u32> {
        let v = u32_at(self.data, self.pos)?;
        self.pos += 4;
        Ok(v)
    }

    pub(crate) fn f64(&mut self) -> Res<f64> {
        let v = f64_at(self.data, self.pos)?;
        self.pos += 8;
        Ok(v)
    }

    /// `N` doubles.
    pub(crate) fn f64s<const N: usize>(&mut self) -> Res<[f64; N]> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f64()?;
        }
        Ok(out)
    }

    /// `n` bytes (short at the end of the data); the cursor moves `n`.
    pub(crate) fn raw(&mut self, n: usize) -> &'a [u8] {
        let v = clip(self.data, self.pos, self.pos.saturating_add(n));
        self.pos = self.pos.saturating_add(n);
        v
    }

    /// The next `n` bytes (short at the end), without moving.
    pub(crate) fn peek(&self, n: usize) -> &'a [u8] {
        clip(self.data, self.pos, self.pos.saturating_add(n))
    }

    pub(crate) fn peek_u16(&self) -> Res<u16> {
        u16_at(self.data, self.pos)
    }

    /// A UTF-16 string record.
    pub(crate) fn utf16(&mut self) -> Res<String> {
        if self.peek(3) != STR_MARKER {
            return Err(Fault::at(self, "expected a string record"));
        }
        self.pos += 3;
        let mut n = usize::from(self.u8()?);
        if n == 0xFF {
            n = usize::from(self.u16()?);
            if n == 0xFFFF {
                n = self.u32()? as usize;
            }
        }
        let bytes = self.raw(n.saturating_mul(2));
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        Ok(String::from_utf16_lossy(&units))
    }

    /// Where the cursor is, with the bytes around it, for a fault message.
    pub(crate) fn ctx(&self) -> String {
        let hex = |b: &[u8]| {
            b.iter()
                .map(|x| format!("{x:02x}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let p = self.pos;
        format!(
            "@{p:#x}: …{} | {}…",
            hex(clip(self.data, p.saturating_sub(16), p)),
            hex(clip(self.data, p, p.saturating_add(32)))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_stop_at_the_end_without_panicking() {
        let data = [1u8, 0, 2];
        let mut r = R::new(&data, 0);
        assert_eq!(r.u16(), Ok(1));
        assert_eq!(r.u16(), Err(Fault::Bounds));
        assert_eq!(r.raw(5), &[2]);
        assert_eq!(r.u8(), Err(Fault::Bounds));
        assert_eq!(find(&data, &[2], 0, 10), Some(2));
        assert!(clip(&data, 5, 9).is_empty());
    }

    #[test]
    fn strings_read_short_and_long_lengths() {
        let mut d = STR_MARKER.to_vec();
        d.push(2);
        d.extend_from_slice(&[b'h', 0, b'i', 0]);
        let mut r = R::new(&d, 0);
        assert_eq!(r.utf16().unwrap(), "hi");
        let mut r = R::new(b"xyz", 0);
        assert!(matches!(r.utf16(), Err(Fault::Parse { .. })));
    }
}
