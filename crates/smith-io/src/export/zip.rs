//! SketchSmith — zip archive writer | v1.0.0 | Thursday 12 November 2026
//!
//! The little of the zip format a 3MF package needs: a handful of entries,
//! each deflated, then the central directory. Plain zip only (no ZIP64), as
//! the 3MF core specification asks of files that fit: an archive past 4 GiB
//! is refused. Entry times are 1 January 1980, the format's earliest.

use std::io::Write as _;

use flate2::Compression;
use flate2::write::DeflateEncoder;

use crate::error::IoError;

/// The zip "made by" and "needed" version: 2.0, the first with deflate.
const VERSION: u16 = 20;
/// Compression method 8: deflate.
const DEFLATE: u16 = 8;
/// 1 January 1980 in MS-DOS date form.
const DOS_DATE: u16 = (1 << 5) | 1;

/// Builds a zip archive in memory.
#[derive(Default)]
pub(crate) struct ZipWriter {
    out: Vec<u8>,
    central: Vec<u8>,
    entries: u16,
}

impl ZipWriter {
    /// Adds `bytes` as the entry `name` (a `/`-separated path, no leading
    /// `/`).
    ///
    /// # Errors
    /// [`IoError::TooLarge`] past 4 GiB or 65 535 entries.
    pub(crate) fn add(&mut self, name: &str, bytes: &[u8]) -> Result<(), IoError> {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes)?;
        let packed = encoder.finish()?;
        let crc = crc32fast::hash(bytes);
        let big = |n: usize| u32::try_from(n).map_err(|_| IoError::TooLarge);
        let (packed_len, len, offset) =
            (big(packed.len())?, big(bytes.len())?, big(self.out.len())?);
        let name_len = u16::try_from(name.len()).map_err(|_| IoError::TooLarge)?;
        self.entries = self.entries.checked_add(1).ok_or(IoError::TooLarge)?;

        // Local file header.
        let o = &mut self.out;
        o.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        o.extend_from_slice(&VERSION.to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes()); // flags
        o.extend_from_slice(&DEFLATE.to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes()); // time
        o.extend_from_slice(&DOS_DATE.to_le_bytes());
        o.extend_from_slice(&crc.to_le_bytes());
        o.extend_from_slice(&packed_len.to_le_bytes());
        o.extend_from_slice(&len.to_le_bytes());
        o.extend_from_slice(&name_len.to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes()); // extra length
        o.extend_from_slice(name.as_bytes());
        o.extend_from_slice(&packed);

        // Its central directory record.
        let c = &mut self.central;
        c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        c.extend_from_slice(&VERSION.to_le_bytes()); // made by
        c.extend_from_slice(&VERSION.to_le_bytes()); // needed
        c.extend_from_slice(&0u16.to_le_bytes()); // flags
        c.extend_from_slice(&DEFLATE.to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes()); // time
        c.extend_from_slice(&DOS_DATE.to_le_bytes());
        c.extend_from_slice(&crc.to_le_bytes());
        c.extend_from_slice(&packed_len.to_le_bytes());
        c.extend_from_slice(&len.to_le_bytes());
        c.extend_from_slice(&name_len.to_le_bytes());
        c.extend_from_slice(&[0; 8]); // extra, comment, disk lengths; internal attributes
        c.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        c.extend_from_slice(&offset.to_le_bytes());
        c.extend_from_slice(name.as_bytes());
        Ok(())
    }

    /// The finished archive.
    ///
    /// # Errors
    /// [`IoError::TooLarge`] past 4 GiB.
    pub(crate) fn finish(mut self) -> Result<Vec<u8>, IoError> {
        let big = |n: usize| u32::try_from(n).map_err(|_| IoError::TooLarge);
        let (start, size) = (big(self.out.len())?, big(self.central.len())?);
        big(self.out.len() + self.central.len() + 22)?;
        self.out.extend_from_slice(&self.central);
        let o = &mut self.out;
        o.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        o.extend_from_slice(&[0; 4]); // this disk, the directory's disk
        o.extend_from_slice(&self.entries.to_le_bytes());
        o.extend_from_slice(&self.entries.to_le_bytes());
        o.extend_from_slice(&size.to_le_bytes());
        o.extend_from_slice(&start.to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes()); // comment length
        Ok(self.out)
    }
}

#[cfg(test)]
pub(crate) use test_read::read_back;

/// Reading archives back, for tests.
#[cfg(test)]
mod test_read {
    use std::io::Read as _;

    use super::DEFLATE;

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }

    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    /// Reads the archive back through its central directory: each entry's
    /// name and inflated bytes, checked against its CRC and sizes.
    pub(crate) fn read_back(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
        let end = zip.len() - 22;
        assert_eq!(u32_at(zip, end), 0x0605_4b50);
        let count = usize::from(u16_at(zip, end + 10));
        let mut at = u32_at(zip, end + 16) as usize;
        let mut out = Vec::new();
        for _ in 0..count {
            assert_eq!(u32_at(zip, at), 0x0201_4b50);
            let crc = u32_at(zip, at + 16);
            let packed = u32_at(zip, at + 20) as usize;
            let len = u32_at(zip, at + 24) as usize;
            let name_len = usize::from(u16_at(zip, at + 28));
            let local = u32_at(zip, at + 42) as usize;
            let name = String::from_utf8(zip[at + 46..at + 46 + name_len].to_vec()).unwrap();
            assert_eq!(u32_at(zip, local), 0x0403_4b50);
            assert_eq!(u16_at(zip, local + 8), DEFLATE);
            let data = local + 30 + usize::from(u16_at(zip, local + 26));
            let mut bytes = Vec::new();
            flate2::read::DeflateDecoder::new(&zip[data..data + packed])
                .read_to_end(&mut bytes)
                .unwrap();
            assert_eq!(bytes.len(), len);
            assert_eq!(crc32fast::hash(&bytes), crc);
            out.push((name, bytes));
            at += 46 + name_len;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_read_back_whole() {
        let mut z = ZipWriter::default();
        let big: Vec<u8> = (0..50_000u32)
            .flat_map(|i| (i % 251).to_le_bytes())
            .collect();
        z.add("[Content_Types].xml", b"<Types/>").unwrap();
        z.add("3D/3dmodel.model", &big).unwrap();
        z.add("empty", b"").unwrap();
        let zip = z.finish().unwrap();
        let back = read_back(&zip);
        assert_eq!(back.len(), 3);
        assert_eq!(
            back[0],
            ("[Content_Types].xml".into(), b"<Types/>".to_vec())
        );
        assert_eq!(back[1].1, big);
        assert_eq!(back[2], ("empty".into(), Vec::new()));
    }
}
