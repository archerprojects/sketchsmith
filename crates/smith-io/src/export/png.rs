//! SketchSmith — PNG pictures | v1.0.0 | Thursday 12 November 2026
//!
//! A picture of the canvas's view as PNG: the size chosen, the background
//! as shown or transparent. The pixels come from the application, which
//! renders them (this crate never renders); here they are only encoded.

use std::path::Path;

use image::ImageEncoder as _;
use image::codecs::png::PngEncoder;
use serde::{Deserialize, Serialize};

use super::write_file;
use crate::error::IoError;

/// The largest side a picture may have, pixels.
pub const MAX_SIDE: u32 = 8192;

/// The size a picture is made at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PictureSize {
    /// The canvas's own size.
    #[default]
    Canvas,
    /// Twice the canvas's size.
    Double,
    /// 1920 × 1080.
    Hd,
    /// 3840 × 2160.
    Uhd,
    /// Width and height as typed.
    Custom,
}

impl PictureSize {
    /// The sizes, in list order.
    pub const ALL: [PictureSize; 5] = [
        Self::Canvas,
        Self::Double,
        Self::Hd,
        Self::Uhd,
        Self::Custom,
    ];

    /// The size's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Canvas => "Canvas",
            Self::Double => "2× Canvas",
            Self::Hd => "1920 × 1080",
            Self::Uhd => "3840 × 2160",
            Self::Custom => "Custom",
        }
    }

    /// The picture's width and height for a canvas `canvas` pixels in size
    /// and a typed `custom` size, each side 1 to [`MAX_SIDE`].
    pub fn pixels(self, canvas: [u32; 2], custom: [u32; 2]) -> [u32; 2] {
        let [w, h] = match self {
            Self::Canvas => canvas,
            Self::Double => [canvas[0] * 2, canvas[1] * 2],
            Self::Hd => [1920, 1080],
            Self::Uhd => [3840, 2160],
            Self::Custom => custom,
        };
        [w.clamp(1, MAX_SIDE), h.clamp(1, MAX_SIDE)]
    }
}

/// A picture's background.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Background {
    /// As the canvas shows it.
    #[default]
    Shown,
    /// None: only the model and its marks.
    Transparent,
}

impl Background {
    /// The choices, in list order.
    pub const ALL: [Background; 2] = [Self::Shown, Self::Transparent];

    /// The choice's name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Shown => "As Shown",
            Self::Transparent => "Transparent",
        }
    }
}

/// Writes `rgba` (`width` × `height`, sRGB, rows top first, straight alpha)
/// as the PNG `path`.
///
/// # Errors
/// [`IoError::Os`] when the file cannot be written; [`IoError::Image`] when
/// the pixels do not match the size.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), IoError> {
    write_file(path, &png_bytes(width, height, rgba)?)
}

/// The PNG file for the pixels.
pub(crate) fn png_bytes(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, IoError> {
    // The encoder panics on a wrong length, so check it first.
    if rgba.len() as u64 != u64::from(width) * u64::from(height) * 4 {
        return Err(IoError::Image(format!(
            "the picture's pixels do not fill {width} × {height}"
        )));
    }
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| IoError::Image(e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageDecoder as _;

    #[test]
    fn sizes_follow_the_choice() {
        let canvas = [1200, 800];
        assert_eq!(PictureSize::Canvas.pixels(canvas, [0, 0]), [1200, 800]);
        assert_eq!(PictureSize::Double.pixels(canvas, [0, 0]), [2400, 1600]);
        assert_eq!(PictureSize::Uhd.pixels(canvas, [0, 0]), [3840, 2160]);
        assert_eq!(PictureSize::Custom.pixels(canvas, [10_000, 0]), [8192, 1]);
    }

    #[test]
    fn pixels_round_trip_with_their_alpha() {
        let px: Vec<u8> = vec![255, 0, 0, 255, 0, 0, 0, 0, 10, 20, 30, 128, 1, 2, 3, 4];
        let png = png_bytes(2, 2, &px).unwrap();
        let d = image::codecs::png::PngDecoder::new(std::io::Cursor::new(png)).unwrap();
        assert_eq!(d.dimensions(), (2, 2));
        let mut back = vec![0; d.total_bytes() as usize];
        d.read_image(&mut back).unwrap();
        assert_eq!(back, px);
        assert!(png_bytes(3, 2, &px).is_err());
    }
}
