//! Re-encodes a source raster as a smaller JPEG when it exceeds the per-attachment byte budget.

use std::io::Cursor;

use image::{
    ExtendedColorType, ImageEncoder, ImageReader, Limits, codecs::jpeg::JpegEncoder,
    imageops::FilterType,
};

use crate::model::{MAX_VISUAL_BYTES, MAX_VISUAL_PIXELS};

/// Quality of the re-encoded JPEG; visual search does not need archival fidelity.
const JPEG_QUALITY: u8 = 85;
/// Linear scales in percent, tried largest first until the encoding fits.
const SCALE_PERCENTS: [u64; 6] = [75, 50, 35, 25, 15, 10];
/// Bound on decoder allocations for one source image.
const MAX_DECODE_ALLOCATION: u64 = 256 * 1024 * 1024;

/// Returns a JPEG of `source` no larger than `MAX_VISUAL_BYTES`, or the reason it cannot be made so.
pub(crate) fn shrink_visual(source: &[u8]) -> Result<Vec<u8>, &'static str> {
    let (width, height) = bounded_reader(source)?
        .into_dimensions()
        .map_err(|_| "visual could not be read to shrink it")?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_VISUAL_PIXELS {
        return Err("visual has too many pixels to shrink");
    }
    let decoded = bounded_reader(source)?
        .decode()
        .map_err(|_| "visual could not be decoded to shrink it")?
        .to_rgb8();
    for percent in SCALE_PERCENTS {
        let target_width = scaled(decoded.width(), percent);
        let target_height = scaled(decoded.height(), percent);
        let resized =
            image::imageops::resize(&decoded, target_width, target_height, FilterType::Triangle);
        let mut encoded = Vec::new();
        JpegEncoder::new_with_quality(&mut encoded, JPEG_QUALITY)
            .write_image(
                resized.as_raw(),
                target_width,
                target_height,
                ExtendedColorType::Rgb8,
            )
            .map_err(|_| "visual could not be re-encoded to shrink it")?;
        if encoded.len() <= MAX_VISUAL_BYTES {
            return Ok(encoded);
        }
    }
    Err("visual could not be shrunk to the attachment size")
}

fn bounded_reader(source: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, &'static str> {
    let mut reader = ImageReader::new(Cursor::new(source))
        .with_guessed_format()
        .map_err(|_| "visual could not be read to shrink it")?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_ALLOCATION);
    reader.limits(limits);
    Ok(reader)
}

/// `dimension` scaled by `percent`, never below one pixel.
fn scaled(dimension: u32, percent: u64) -> u32 {
    u32::try_from(u64::from(dimension) * percent / 100)
        .unwrap_or(u32::MAX)
        .max(1)
}

#[cfg(test)]
mod tests {
    use image::{ImageBuffer, Rgb};

    use super::*;

    #[test]
    fn oversized_noise_is_reencoded_within_the_attachment_budget() {
        // Pseudo-random pixels defeat JPEG compression, so the source exceeds the budget.
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let image = ImageBuffer::from_fn(2_000, 1_500, |_, _| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            Rgb([seed as u8, (seed >> 8) as u8, (seed >> 16) as u8])
        });
        let mut source = Vec::new();
        JpegEncoder::new_with_quality(&mut source, 100)
            .write_image(image.as_raw(), 2_000, 1_500, ExtendedColorType::Rgb8)
            .expect("source encodes");
        assert!(source.len() > MAX_VISUAL_BYTES, "fixture must be oversized");

        let shrunk = shrink_visual(&source).expect("shrinks");

        assert!(shrunk.len() <= MAX_VISUAL_BYTES);
        let (width, height) = image::ImageReader::new(Cursor::new(&shrunk))
            .with_guessed_format()
            .expect("format")
            .into_dimensions()
            .expect("dimensions");
        assert!(width < 2_000 && height < 1_500);
    }

    #[test]
    fn scaling_never_reaches_zero() {
        assert_eq!(scaled(1, 10), 1);
        assert_eq!(scaled(100, 75), 75);
    }
}
