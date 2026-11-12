//! # smith-io
//!
//! SketchSmith — file input and output | v1.0.0 | Thursday 12 November 2026
//!
//! The native `.glb` format with the `SKETCHSMITH_model` extension,
//! and later all importers (glTF, OBJ, STL, DAE, SVG, and `.skp` through
//! `smith-skp`) and all exporters (OBJ, STL, 3MF, glTF, DXF, STEP, PDF, SVG,
//! PNG).
//!
//! Built so far:
//! * [`native`] — encode and decode the native file ([`write_glb`],
//!   [`read_glb`]);
//! * [`save`] — write a file atomically: to a temporary file in the same
//!   folder, flushed to disk, then renamed over the target, so a crash or a
//!   full disk never leaves a half-written model in place of the old one;
//! * [`open`] — read a file from disk;
//! * [`textures`] — import a PNG or JPEG as a material's image
//!   ([`load_image`]) and decode one for display ([`decode_for_display`]);
//! * [`export`] — model exports: OBJ with its MTL file and textures, STL,
//!   3MF, plain glTF, STEP and 3D DXF; hidden-line drawings of a view with their
//!   dimensions and text, as PDF, SVG and 2D DXF; PNG pictures;
//! * [`import`] — model imports: OBJ with its MTL file and images, STL,
//!   DAE (COLLADA) with its groups, components and materials, SVG
//!   drawings as lines, curves and filled faces, and other programs' glTF
//!   (`.gltf` and `.glb`) with its nodes, shared meshes and materials.
//!
//! Dependency rule: never depends on `smith-render`; PNG view export renders through
//! a trait supplied by the binary.

mod error;
pub mod export;
mod glb;
pub mod import;
pub mod native;
pub mod textures;
mod xml;

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub use error::IoError;
pub use native::{NativeFile, Opened, SavedAxes, SavedView, read_glb, write_glb, write_parts};
pub use textures::{DisplayImage, MAX_DISPLAY_SIDE, decode_for_display, load_image};

/// The native file extension, without the dot.
pub const NATIVE_EXTENSION: &str = "glb";

/// Saves `file` to `path` as a native `.glb`, atomically.
///
/// # Errors
/// [`IoError::Os`] when the folder cannot be written; [`IoError::TooLarge`].
pub fn save(path: &Path, file: &NativeFile) -> Result<(), IoError> {
    let bytes = write_glb(file)?;
    write_atomic(path, &bytes, 0)
}

/// Saves a document, view, axes and mode (`present`: Present mode) to
/// `path` as a native `.glb`, atomically (as [`save`], without building a
/// [`NativeFile`]). No backup is kept.
///
/// # Errors
/// As [`save`].
pub fn save_parts(
    path: &Path,
    document: &smith_model::Document,
    view: Option<&SavedView>,
    axes: Option<&SavedAxes>,
    present: bool,
) -> Result<(), IoError> {
    save_parts_with_backups(path, document, view, axes, present, 0)
}

/// As [`save_parts`], keeping up to `backups` earlier versions of `path`
/// beside it, as Blender does with its Save Versions setting: the file being
/// replaced becomes `name.glb1`, an existing `name.glb1` becomes `name.glb2`,
/// and so on; the oldest beyond `backups` is deleted. 0 keeps none.
///
/// # Errors
/// As [`save`].
pub fn save_parts_with_backups(
    path: &Path,
    document: &smith_model::Document,
    view: Option<&SavedView>,
    axes: Option<&SavedAxes>,
    present: bool,
    backups: u32,
) -> Result<(), IoError> {
    let bytes = write_parts(document, view, axes, present)?;
    write_atomic(path, &bytes, backups)
}

/// The path of backup `n` of `path`: the file name with `n` appended
/// (`bracket.glb` → `bracket.glb1`).
pub fn backup_path(path: &Path, n: u32) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(n.to_string());
    PathBuf::from(name)
}

/// Opens a native `.glb` file.
///
/// # Errors
/// [`IoError::Os`], [`IoError::NotGlb`], [`IoError::Foreign`] or
/// [`IoError::Damaged`].
pub fn open(path: &Path) -> Result<Opened, IoError> {
    let bytes = fs::read(path)?;
    read_glb(&bytes)
}

/// Writes `bytes` to a temporary file beside `path`, flushes it to disk, then
/// renames it into place (and flushes the folder, so the rename itself
/// survives a power cut). With `backups` above 0 the existing file is first
/// moved to backup 1 (see [`save_parts_with_backups`]). On failure the
/// temporary file is removed and any existing file at `path` is put back.
fn write_atomic(path: &Path, bytes: &[u8], backups: u32) -> Result<(), IoError> {
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model.glb".into());
    let temp = dir.join(format!(".{name}.saving-{}", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        let backed_up = backups > 0 && path.exists();
        if backed_up {
            rotate_backups(path, backups)?;
        }
        if let Err(e) = fs::rename(&temp, path) {
            if backed_up {
                // Put the original back where it was.
                let _ = fs::rename(backup_path(path, 1), path);
            }
            return Err(e);
        }
        if let Ok(d) = fs::File::open(dir) {
            // Not every file system supports syncing a folder; ignore that.
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    Ok(result?)
}

/// Shifts the backups of `path` up by one (`.glb1` → `.glb2` …, dropping the
/// one beyond `backups`), then moves `path` itself to backup 1.
fn rotate_backups(path: &Path, backups: u32) -> std::io::Result<()> {
    for n in (1..backups).rev() {
        let from = backup_path(path, n);
        if from.exists() {
            fs::rename(&from, backup_path(path, n + 1))?;
        }
    }
    fs::rename(path, backup_path(path, 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(p: &Path) -> Vec<u8> {
        fs::read(p).unwrap()
    }

    #[test]
    fn backups_rotate_like_blender() {
        let dir = std::env::temp_dir().join(format!("smith-io-backups-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bracket.glb");
        for bytes in [b"a", b"b", b"c", b"d"] {
            write_atomic(&path, bytes, 2).unwrap();
        }
        assert_eq!(read(&path), b"d");
        assert_eq!(read(&dir.join("bracket.glb1")), b"c");
        assert_eq!(read(&dir.join("bracket.glb2")), b"b");
        assert!(!dir.join("bracket.glb3").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_backups_when_off() {
        let dir = std::env::temp_dir().join(format!("smith-io-nobackup-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ring.glb");
        write_atomic(&path, b"a", 0).unwrap();
        write_atomic(&path, b"b", 0).unwrap();
        assert_eq!(read(&path), b"b");
        assert!(!dir.join("ring.glb1").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
