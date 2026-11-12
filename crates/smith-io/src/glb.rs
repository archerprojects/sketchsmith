//! SketchSmith — GLB container | v1.0.0 | Thursday 12 November 2026
//!
//! The binary glTF 2.0 container (glTF 2.0 specification §4.4): a 12-byte
//! header (`glTF`, version 2, total length), a JSON chunk padded with spaces
//! to 4 bytes, and an optional BIN chunk padded with zeros to 4 bytes. All
//! integers are little-endian.

use crate::error::IoError;

const MAGIC: &[u8; 4] = b"glTF";
const VERSION: u32 = 2;
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;

/// Rounds `n` up to a multiple of 4.
pub(crate) fn pad4(n: usize) -> usize {
    n.div_ceil(4) * 4
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn len32(n: usize) -> Result<u32, IoError> {
    u32::try_from(n).map_err(|_| IoError::TooLarge)
}

/// Packs a JSON document and binary payload into a GLB file.
///
/// # Errors
/// [`IoError::TooLarge`] beyond the format's 4 GiB limit.
pub(crate) fn write(json: &[u8], bin: &[u8]) -> Result<Vec<u8>, IoError> {
    let json_len = pad4(json.len());
    let bin_len = pad4(bin.len());
    let total = 12 + 8 + json_len + if bin.is_empty() { 0 } else { 8 + bin_len };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(MAGIC);
    push_u32(&mut out, VERSION);
    push_u32(&mut out, len32(total)?);
    push_u32(&mut out, len32(json_len)?);
    push_u32(&mut out, CHUNK_JSON);
    out.extend_from_slice(json);
    out.resize(20 + json_len, b' ');
    if !bin.is_empty() {
        push_u32(&mut out, len32(bin_len)?);
        push_u32(&mut out, CHUNK_BIN);
        out.extend_from_slice(bin);
        out.resize(total, 0);
    }
    Ok(out)
}

/// Splits a GLB file into its JSON text and binary payload.
///
/// # Errors
/// [`IoError::NotGlb`] when the header is wrong, [`IoError::Damaged`] when a
/// chunk runs past the end of the file.
pub(crate) fn read(bytes: &[u8]) -> Result<(&[u8], &[u8]), IoError> {
    if bytes.get(0..4) != Some(MAGIC.as_slice()) {
        return Err(IoError::NotGlb);
    }
    let version = u32_at(bytes, 4).ok_or(IoError::NotGlb)?;
    if version != VERSION {
        return Err(IoError::Damaged(format!(
            "glTF container version {version}"
        )));
    }
    let total = u32_at(bytes, 8).ok_or(IoError::NotGlb)? as usize;
    if total > bytes.len() {
        return Err(IoError::Damaged(
            "file is shorter than its header says".into(),
        ));
    }
    let bytes = &bytes[..total];
    let mut at = 12;
    let mut json = None;
    let mut bin: &[u8] = &[];
    while at + 8 <= bytes.len() {
        let len = u32_at(bytes, at).unwrap_or(0) as usize;
        let kind = u32_at(bytes, at + 4).unwrap_or(0);
        let body = bytes
            .get(at + 8..at + 8 + len)
            .ok_or_else(|| IoError::Damaged("chunk runs past the end of the file".into()))?;
        match kind {
            CHUNK_JSON if json.is_none() => json = Some(body),
            CHUNK_BIN if bin.is_empty() => bin = body,
            _ => {} // Unknown chunks are skipped, as the glTF spec requires.
        }
        at += 8 + pad4(len);
    }
    let json = json.ok_or_else(|| IoError::Damaged("no JSON chunk".into()))?;
    Ok((json, bin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_round_trips_and_pads() {
        let file = write(b"{\"a\":1}", &[1, 2, 3, 4, 5]).expect("write");
        assert_eq!(file.len() % 4, 0);
        assert_eq!(&file[0..4], b"glTF");
        let (json, bin) = read(&file).expect("read");
        assert_eq!(json.trim_ascii_end(), b"{\"a\":1}");
        assert_eq!(&bin[..5], &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn bad_headers_are_refused() {
        assert!(matches!(read(b"nope"), Err(IoError::NotGlb)));
        let mut file = write(b"{}", &[]).expect("write");
        file.truncate(14);
        assert!(read(&file).is_err());
    }
}
