//! # smith-skp
//!
//! SketchSmith — SketchUp `.skp` reader | v1.0.0 | Thursday 12 November 2026
//!
//! A pure-Rust port of the OpenSKP parser (MIT, © 2024–2026 Ahsan Mehmood,
//! <https://github.com/iamahsanmehmood/openskp>). `.skp` files come in two
//! containers: 2013–2020 files are one MFC `CArchive` object stream, read
//! here; 2021 and later files are a VFF header around a ZIP archive, which
//! this reader recognises and refuses ([`SkpError::Newer`]).
//!
//! [`read`] hands back an [`SkpModel`]: layers, materials, component
//! definitions and the root's entities, in the file's own units (inches,
//! Z up). It never panics on a broken file: every read is checked, and the
//! walk runs on its own thread, so a fault anywhere comes back as
//! [`SkpError::Unreadable`].
//!
//! SketchSmith is not affiliated with or endorsed by Trimble Inc.
//!
//! ```text
//! MIT License
//!
//! Copyright (c) 2024-2026 Ahsan Mehmood
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.
//! ```

mod archive;
mod cursor;
mod model;
mod records;
mod walk;

pub use model::{
    ArcCurve, CoEdge, Definition, Dimension, Edge, Entities, Face, GuideLine, GuidePoint, Instance,
    Layer, Material, Point, SectionPlane, SkpModel, Text, Texture, Vertex,
};

/// The stack the walk runs on: objects can nest deeply in large files.
const WALK_STACK: usize = 256 * 1024 * 1024;

/// A `.skp` that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkpError {
    /// Not a `.skp` file.
    #[error("not an SKP file")]
    NotSkp,
    /// A SketchUp 2021 or later file.
    #[error("an SKP file from SketchUp 2021 or later")]
    Newer,
    /// A 2013–2020 file the reader could not get through (the detail says
    /// where).
    #[error("unreadable SKP file: {0}")]
    Unreadable(String),
}

/// Which container a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// 2013–2020: the MFC object stream.
    Classic,
    /// 2021 and later: VFF around a ZIP archive.
    Vff,
    /// Neither.
    NotSkp,
}

/// The VFF and classic header's magic.
const MAGIC: &[u8] = b"\xff\xfe\xff\x0e";
const ZIP_LOCAL_HEADER: &[u8] = b"PK\x03\x04";

/// Which container `data` is.
pub fn container(data: &[u8]) -> Container {
    if !data.starts_with(MAGIC) {
        return Container::NotSkp;
    }
    let head = |n: usize| &data[..n.min(data.len())];
    let has = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
    // A classic header names its version map near the start, with no ZIP
    // archive there; anything else carrying a ZIP archive is VFF.
    if !has(head(0x100), ZIP_LOCAL_HEADER) && has(head(0x200), b"CVersionMap") {
        return Container::Classic;
    }
    if has(data, ZIP_LOCAL_HEADER) {
        return Container::Vff;
    }
    Container::NotSkp
}

/// Reads a `.skp` file's bytes.
///
/// # Errors
/// [`SkpError::NotSkp`] when it is not one; [`SkpError::Newer`] for a 2021
/// or later file; [`SkpError::Unreadable`] when the walk fails.
pub fn read(data: &[u8]) -> Result<SkpModel, SkpError> {
    match container(data) {
        Container::NotSkp => return Err(SkpError::NotSkp),
        Container::Vff => return Err(SkpError::Newer),
        Container::Classic => {}
    }
    std::thread::scope(|scope| {
        let walker = std::thread::Builder::new()
            .name("skp-read".into())
            .stack_size(WALK_STACK)
            .spawn_scoped(scope, || {
                walk::walk(data)
                    .map(|w| model::build(data, w))
                    .map_err(|e| SkpError::Unreadable(e.to_string()))
            });
        match walker {
            Ok(handle) => handle
                .join()
                .unwrap_or_else(|_| Err(SkpError::Unreadable("the reader stopped".into()))),
            Err(e) => Err(SkpError::Unreadable(e.to_string())),
        }
    })
}
