//! SketchSmith — texture images | v1.0.0 | Thursday 12 November 2026
//!
//! **Import** ([`load_image`]): a material's image is a PNG or JPEG file,
//! kept byte for byte (no re-encoding, no quality lost); only its size is
//! read here. Any other format is refused with a message: convert it
//! outside the app.
//!
//! **Display** ([`decode_for_display`]): the image decoded to 8-bit RGBA
//! (16-bit, grey and palette PNGs included), scaled down so its long side is
//! at most [`MAX_DISPLAY_SIDE`] pixels, with every mipmap level built here on
//! the CPU so distant repeats do not shimmer. The file always keeps the
//! original.

use std::path::Path;

use smith_model::{ImageFormat, TextureImage};

use crate::error::IoError;

/// The longest side an image is shown at; larger images are scaled down for
/// display only. 4096 pixels across a 50 mm repeat is about 80 per mm.
pub const MAX_DISPLAY_SIDE: u32 = 4096;

/// An image ready for the GPU: RGBA8 (sRGB) mip levels, largest first.
#[derive(Debug, Clone)]
pub struct DisplayImage {
    /// Each level's width, height and pixels (4 bytes each, rows top first).
    pub levels: Vec<(u32, u32, Vec<u8>)>,
}

/// The format of image `bytes` from their signature.
fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else {
        None
    }
}

fn codec(format: ImageFormat) -> image::ImageFormat {
    match format {
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
    }
}

/// Reads `bytes` (named `name`) as a texture image: PNG or JPEG only.
///
/// # Errors
/// [`IoError::Image`] for another format or an image that cannot be read.
pub fn image_from_bytes(name: &str, bytes: Vec<u8>) -> Result<TextureImage, IoError> {
    let format = sniff(&bytes).ok_or_else(|| {
        IoError::Image("Only PNG and JPEG images can be loaded. Convert it to one first.".into())
    })?;
    let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), codec(format));
    let (w, h) = reader
        .into_dimensions()
        .map_err(|e| IoError::Image(format!("The image could not be read ({e}).")))?;
    if w == 0 || h == 0 {
        return Err(IoError::Image("The image is empty.".into()));
    }
    Ok(TextureImage {
        name: name.into(),
        format,
        bytes,
        pixels: [w, h],
    })
}

/// Loads the PNG or JPEG at `path` as a texture image.
///
/// # Errors
/// [`IoError::Os`] when the file cannot be read; [`IoError::Image`] as
/// [`image_from_bytes`].
pub fn load_image(path: &Path) -> Result<TextureImage, IoError> {
    let bytes = std::fs::read(path)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    image_from_bytes(&name, bytes)
}

/// Decodes `image` for display (see the module notes).
///
/// # Errors
/// [`IoError::Image`] when it cannot be decoded.
pub fn decode_for_display(image: &TextureImage) -> Result<DisplayImage, IoError> {
    let decoded = image::load_from_memory_with_format(&image.bytes, codec(image.format))
        .map_err(|e| IoError::Image(format!("The image could not be decoded ({e}).")))?;
    let mut rgba = decoded.to_rgba8();
    let (w, h) = rgba.dimensions();
    let long = w.max(h);
    if long > MAX_DISPLAY_SIDE {
        let scale = f64::from(MAX_DISPLAY_SIDE) / f64::from(long);
        let nw = ((f64::from(w) * scale).round() as u32).max(1);
        let nh = ((f64::from(h) * scale).round() as u32).max(1);
        rgba = image::imageops::resize(&rgba, nw, nh, image::imageops::FilterType::Triangle);
    }
    let mut levels = Vec::new();
    loop {
        let (w, h) = rgba.dimensions();
        levels.push((w, h, rgba.as_raw().clone()));
        if w == 1 && h == 1 {
            break;
        }
        rgba = image::imageops::resize(
            &rgba,
            (w / 2).max(1),
            (h / 2).max(1),
            image::imageops::FilterType::Triangle,
        );
    }
    Ok(DisplayImage { levels })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG of `w` × `h` pixels.
    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png)
            .expect("encode");
        out.into_inner()
    }

    #[test]
    fn a_png_keeps_its_bytes_and_reads_its_size() {
        let bytes = png(6, 3);
        let t = image_from_bytes("a.png", bytes.clone()).expect("png");
        assert_eq!(t.pixels, [6, 3]);
        assert_eq!(t.format, ImageFormat::Png);
        assert_eq!(t.bytes, bytes);
    }

    #[test]
    fn other_formats_are_refused() {
        assert!(matches!(
            image_from_bytes("a.gif", b"GIF89a....".to_vec()),
            Err(IoError::Image(_))
        ));
        assert!(matches!(
            image_from_bytes("a.png", b"\x89PNG\r\n\x1a\nbroken".to_vec()),
            Err(IoError::Image(_))
        ));
    }

    #[test]
    fn display_images_are_capped_and_mipmapped_down_to_one_pixel() {
        let t = image_from_bytes("big.png", png(MAX_DISPLAY_SIDE * 2, 8)).expect("png");
        let d = decode_for_display(&t).expect("decode");
        let (w, h, px) = &d.levels[0];
        assert_eq!((*w, *h), (MAX_DISPLAY_SIDE, 4));
        assert_eq!(px.len(), (w * h * 4) as usize);
        assert_eq!(d.levels.last().map(|l| (l.0, l.1)), Some((1, 1)));
        assert_eq!(d.levels.len(), 13);
    }
}
