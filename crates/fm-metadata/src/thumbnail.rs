//! Pure-Rust, provider-agnostic thumbnail generation for downscaled image
//! previews (task 0134). Operates on already-read bytes so it works
//! identically for any [`fm_vfs::FileSystemProvider`] (local, archive,
//! remote), not just local files.

use std::io::Cursor;

use image::{ImageFormat, ImageReader};

/// Source file extensions this module can decode directly (case-insensitive,
/// without the leading dot). CBZ/CBR are handled one layer up, in
/// `fm-application`, by extracting a page through the existing archive
/// provider and feeding its bytes back into [`generate_image_thumbnail`].
pub const SUPPORTED_IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "ico"];

/// Largest source file this module will attempt to decode. Guards against a
/// single huge image stalling the thumbnail budget (task 0134 acceptance
/// criteria: "a configurable size limit... so thumbnailing a directory...
/// doesn't stall the UI").
pub const MAX_SOURCE_BYTES: u64 = 25 * 1024 * 1024;

/// The three grid/icon sizes the frontend can request (task 0134: "Icon size
/// is small, medium and large").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThumbnailSize {
    /// Icon-sized, for the directory table's icon column.
    Small,
    /// The default grid-view tile size.
    Medium,
    /// The largest grid-view tile size.
    Large,
}

impl ThumbnailSize {
    /// Largest edge, in pixels, a generated thumbnail may have.
    pub fn max_dimension(self) -> u32 {
        match self {
            ThumbnailSize::Small => 64,
            ThumbnailSize::Medium => 128,
            ThumbnailSize::Large => 256,
        }
    }

    /// Stable lowercase name, used both as the `size` query parameter and as
    /// part of the on-disk cache key.
    pub fn as_str(self) -> &'static str {
        match self {
            ThumbnailSize::Small => "small",
            ThumbnailSize::Medium => "medium",
            ThumbnailSize::Large => "large",
        }
    }

    /// Parses the `size` query parameter / cache-key component.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "small" => Some(ThumbnailSize::Small),
            "medium" => Some(ThumbnailSize::Medium),
            "large" => Some(ThumbnailSize::Large),
            _ => None,
        }
    }
}

/// A generated thumbnail, ready to be cached and served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedThumbnail {
    /// The encoded thumbnail image bytes.
    pub bytes: Vec<u8>,
    /// The MIME type of [`Self::bytes`], for the HTTP `Content-Type` header.
    pub content_type: &'static str,
}

/// Errors producing a thumbnail. Every variant is a "no thumbnail available"
/// outcome from the caller's point of view (falls back to the generic type
/// icon); none of them represent a broken filesystem/provider.
#[derive(Debug, thiserror::Error)]
pub enum ThumbnailError {
    /// The source file exceeds [`MAX_SOURCE_BYTES`].
    #[error("source file too large ({size} bytes > {limit} limit)")]
    SourceTooLarge {
        /// The source file's actual size, in bytes.
        size: u64,
        /// The configured limit, in bytes.
        limit: u64,
    },
    /// The bytes are not a recognized/decodable image format.
    #[error("unrecognized or unsupported image format")]
    UnsupportedFormat,
    /// The image crate failed to decode otherwise-recognized bytes.
    #[error("failed to decode image: {0}")]
    Decode(#[from] image::ImageError),
    /// The image crate failed to encode the downscaled thumbnail.
    #[error("failed to encode thumbnail: {0}")]
    Encode(image::ImageError),
}

/// Decodes `bytes` as an image, downscales it to fit within `size`'s bounding
/// box (preserving aspect ratio) and re-encodes it as JPEG.
///
/// `bytes` must already have been confirmed to be at most [`MAX_SOURCE_BYTES`]
/// by the caller (the check happens here too, defensively) — callers that
/// know the source size up front should skip reading it at all once it
/// exceeds the budget, which this function cannot do since it only sees
/// already-read bytes.
pub fn generate_image_thumbnail(
    bytes: &[u8],
    size: ThumbnailSize,
) -> Result<GeneratedThumbnail, ThumbnailError> {
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(ThumbnailError::SourceTooLarge {
            size: bytes.len() as u64,
            limit: MAX_SOURCE_BYTES,
        });
    }
    // A favicon-style `.ico` holds several sizes of one image. Decode only the one that fits the
    // requested thumbnail rather than whichever frame the decoder prefers.
    let single_frame = select_ico_frame(bytes, size.max_dimension());
    let bytes = single_frame.as_deref().unwrap_or(bytes);
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ThumbnailError::UnsupportedFormat)?;
    if reader.format().is_none() {
        return Err(ThumbnailError::UnsupportedFormat);
    }
    let image = reader.decode()?;
    let thumbnail = image.thumbnail(size.max_dimension(), size.max_dimension());

    // JPEG has no alpha channel and would paint every transparent pixel black, so an image that
    // really is transparent (a logo, an icon, a cut-out) is kept as PNG. Everything else, photos
    // included, stays the much smaller JPEG even when its file format could carry transparency.
    let (format, content_type) = if has_transparency(&thumbnail) {
        (ImageFormat::Png, "image/png")
    } else {
        (ImageFormat::Jpeg, "image/jpeg")
    };
    let mut out = Vec::new();
    thumbnail
        .write_to(&mut Cursor::new(&mut out), format)
        .map_err(ThumbnailError::Encode)?;
    Ok(GeneratedThumbnail {
        bytes: out,
        content_type,
    })
}

/// Whether any pixel of `image` is not fully opaque. Checked on the already downscaled thumbnail,
/// so the scan is at most a few tens of thousands of pixels.
fn has_transparency(image: &image::DynamicImage) -> bool {
    image.color().has_alpha() && image.to_rgba8().pixels().any(|pixel| pixel.0[3] < u8::MAX)
}

/// Whether `bytes` start an `.ico` file (`ICONDIR`: reserved 0, type 1).
#[must_use]
pub(crate) fn is_ico(bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(&[0, 0, 1, 0])
}

/// The MIME type of already-generated thumbnail bytes (PNG when the image is transparent,
/// otherwise JPEG), for callers
/// that only hold the cached bytes.
#[must_use]
pub fn thumbnail_content_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        "image/jpeg"
    }
}

/// Size of the `ICONDIR` header and of each `ICONDIRENTRY` in an `.ico` file.
const ICO_HEADER_LEN: usize = 6;
const ICO_ENTRY_LEN: usize = 16;

/// For a multi-image `.ico`, returns a single-image `.ico` holding only the frame that best fits a
/// `target`-pixel thumbnail: the smallest frame at least `target` wide and tall, or the largest
/// frame when none is that big. Returns `None` for anything that is not a well-formed `.ico` with
/// more than one frame, so the caller decodes the original bytes unchanged.
fn select_ico_frame(bytes: &[u8], target: u32) -> Option<Vec<u8>> {
    // ICONDIR: reserved = 0, type = 1 (icon), image count.
    if !is_ico(bytes) {
        return None;
    }
    let count = usize::from(u16::from_le_bytes([*bytes.get(4)?, *bytes.get(5)?]));
    if count < 2 {
        return None;
    }
    let mut best: Option<(u32, &[u8])> = None;
    for index in 0..count {
        let start = ICO_HEADER_LEN + index * ICO_ENTRY_LEN;
        let entry = bytes.get(start..start + ICO_ENTRY_LEN)?;
        // A stored width or height of 0 means 256.
        let dimension = |value: u8| if value == 0 { 256 } else { u32::from(value) };
        let edge = dimension(entry[0]).min(dimension(entry[1]));
        let better = match best {
            None => true,
            Some((current, _)) => {
                if edge >= target && current >= target {
                    edge < current
                } else if edge >= target {
                    true
                } else if current >= target {
                    false
                } else {
                    edge > current
                }
            }
        };
        if better {
            best = Some((edge, entry));
        }
    }
    let entry = best?.1;
    let length = usize::try_from(u32::from_le_bytes(entry[8..12].try_into().ok()?)).ok()?;
    let offset = usize::try_from(u32::from_le_bytes(entry[12..16].try_into().ok()?)).ok()?;
    let data = bytes.get(offset..offset.checked_add(length)?)?;

    // Re-wrap the chosen frame as a one-image `.ico`: header, its entry (data now at offset 22),
    // then the frame's bytes (PNG or headerless BMP, exactly as stored).
    let mut single = Vec::with_capacity(ICO_HEADER_LEN + ICO_ENTRY_LEN + data.len());
    single.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    single.extend_from_slice(&entry[..12]);
    single.extend_from_slice(
        &u32::try_from(ICO_HEADER_LEN + ICO_ENTRY_LEN)
            .ok()?
            .to_le_bytes(),
    );
    single.extend_from_slice(data);
    Some(single)
}

/// Whether `extension` (without the leading dot, any case) is a format
/// [`generate_image_thumbnail`] can decode directly.
pub fn is_supported_image_extension(extension: &str) -> bool {
    let lower = extension.to_ascii_lowercase();
    SUPPORTED_IMAGE_EXTENSIONS.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, GenericImage, Rgba};

    fn fixture_bytes(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
        let mut image = DynamicImage::new_rgba8(width, height);
        for x in 0..width {
            for y in 0..height {
                image.put_pixel(x, y, Rgba([(x % 255) as u8, (y % 255) as u8, 128, 255]));
            }
        }
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), format)
            .expect("encode fixture image");
        bytes
    }

    #[test]
    fn generates_a_thumbnail_from_png_within_bounds() {
        let bytes = fixture_bytes(ImageFormat::Png, 400, 200);
        let thumbnail =
            generate_image_thumbnail(&bytes, ThumbnailSize::Medium).expect("generate thumbnail");
        assert_eq!(thumbnail.content_type, "image/jpeg");
        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode thumbnail");
        assert!(decoded.width() <= ThumbnailSize::Medium.max_dimension());
        assert!(decoded.height() <= ThumbnailSize::Medium.max_dimension());
        // Aspect ratio (2:1) must be preserved, not stretched to a square.
        assert_eq!(decoded.width(), decoded.height() * 2);
    }

    #[test]
    fn generates_a_thumbnail_from_jpeg() {
        let bytes = fixture_bytes(ImageFormat::Jpeg, 100, 100);
        let thumbnail =
            generate_image_thumbnail(&bytes, ThumbnailSize::Small).expect("generate thumbnail");
        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode thumbnail");
        assert!(decoded.width() <= ThumbnailSize::Small.max_dimension());
    }

    /// A multi-frame `.ico` whose frames are solid colours keyed by size, so the frame a
    /// thumbnail came from can be read back from its pixels.
    fn multi_size_ico(sizes: &[(u32, [u8; 3])]) -> Vec<u8> {
        use image::codecs::ico::{IcoEncoder, IcoFrame};
        use image::{ExtendedColorType, RgbaImage};
        let frames: Vec<IcoFrame<'static>> = sizes
            .iter()
            .map(|(edge, [r, g, b])| {
                let pixels = RgbaImage::from_pixel(*edge, *edge, Rgba([*r, *g, *b, 255]));
                IcoFrame::as_png(pixels.as_raw(), *edge, *edge, ExtendedColorType::Rgba8)
                    .expect("encode ico frame")
            })
            .collect();
        let mut bytes = Vec::new();
        IcoEncoder::new(&mut bytes)
            .encode_images(&frames)
            .expect("encode ico");
        bytes
    }

    fn dominant_channel(thumbnail: &GeneratedThumbnail) -> usize {
        let pixel = image::load_from_memory(&thumbnail.bytes)
            .expect("decode thumbnail")
            .to_rgb8()
            .get_pixel(2, 2)
            .0;
        pixel
            .iter()
            .enumerate()
            .max_by_key(|(_, value)| **value)
            .map(|(index, _)| index)
            .expect("three channels")
    }

    #[test]
    fn an_ico_thumbnail_comes_from_the_frame_that_fits_the_requested_size() {
        // red 16, green 32, blue 64, then a yellow-ish 256 frame (red and green high).
        let ico = multi_size_ico(&[
            (16, [250, 0, 0]),
            (32, [0, 250, 0]),
            (64, [0, 0, 250]),
            (256, [250, 250, 0]),
        ]);

        // Small is 64 px: the 64 px frame (blue) is the exact fit, not the 256 px one.
        let small = generate_image_thumbnail(&ico, ThumbnailSize::Small).expect("small");
        assert_eq!(dominant_channel(&small), 2);
        // Medium is 128 px: nothing is exactly 128, so the smallest frame that is big enough (256).
        let medium = generate_image_thumbnail(&ico, ThumbnailSize::Medium).expect("medium");
        assert_ne!(dominant_channel(&medium), 2);
    }

    #[test]
    fn an_ico_with_only_small_frames_uses_the_largest() {
        let ico = multi_size_ico(&[(16, [250, 0, 0]), (32, [0, 250, 0])]);
        let large = generate_image_thumbnail(&ico, ThumbnailSize::Large).expect("large");
        assert_eq!(dominant_channel(&large), 1, "the 32 px green frame");
    }

    #[test]
    fn a_malformed_ico_directory_falls_back_to_the_original_bytes() {
        let mut ico = multi_size_ico(&[(16, [250, 0, 0]), (32, [0, 250, 0])]);
        // Claim far more frames than the file holds: selection must give up, not panic.
        ico[4] = 200;
        let _ = generate_image_thumbnail(&ico, ThumbnailSize::Small);
        assert!(select_ico_frame(&ico, 64).is_none());
    }

    #[test]
    fn generates_a_thumbnail_from_gif() {
        let bytes = fixture_bytes(ImageFormat::Gif, 80, 60);
        let thumbnail =
            generate_image_thumbnail(&bytes, ThumbnailSize::Large).expect("generate thumbnail");
        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode thumbnail");
        assert!(decoded.width() <= ThumbnailSize::Large.max_dimension());
    }

    #[test]
    fn generates_a_thumbnail_from_webp() {
        let bytes = fixture_bytes(ImageFormat::WebP, 120, 90);
        let thumbnail =
            generate_image_thumbnail(&bytes, ThumbnailSize::Small).expect("generate thumbnail");
        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode thumbnail");
        assert!(decoded.width() <= ThumbnailSize::Small.max_dimension());
        assert!(decoded.height() <= ThumbnailSize::Small.max_dimension());
    }

    #[test]
    fn generates_a_thumbnail_from_ico() {
        // Icon files are always thumbnailed regardless of size (unlike other images, which are
        // size-gated by the frontend) since an icon's own content already *is* its icon.
        let bytes = fixture_bytes(ImageFormat::Ico, 32, 32);
        let thumbnail =
            generate_image_thumbnail(&bytes, ThumbnailSize::Small).expect("generate thumbnail");
        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode thumbnail");
        assert!(decoded.width() <= ThumbnailSize::Small.max_dimension());
        assert!(decoded.height() <= ThumbnailSize::Small.max_dimension());
    }

    #[test]
    fn an_ico_thumbnail_is_a_png_that_keeps_its_transparency() {
        use image::codecs::ico::{IcoEncoder, IcoFrame};
        use image::{ExtendedColorType, RgbaImage};
        // Fully transparent pixels everywhere except a solid red block in the middle.
        let mut pixels = RgbaImage::from_pixel(32, 32, Rgba([0, 0, 0, 0]));
        for x in 12..20 {
            for y in 12..20 {
                pixels.put_pixel(x, y, Rgba([250, 0, 0, 255]));
            }
        }
        let frame = IcoFrame::as_png(pixels.as_raw(), 32, 32, ExtendedColorType::Rgba8)
            .expect("encode ico frame");
        let mut ico = Vec::new();
        IcoEncoder::new(&mut ico)
            .encode_images(&[frame])
            .expect("encode ico");

        let thumbnail = generate_image_thumbnail(&ico, ThumbnailSize::Small).expect("thumbnail");

        assert_eq!(thumbnail.content_type, "image/png");
        assert_eq!(thumbnail_content_type(&thumbnail.bytes), "image/png");
        let decoded = image::load_from_memory(&thumbnail.bytes)
            .expect("decode thumbnail")
            .to_rgba8();
        assert_eq!(
            decoded.get_pixel(0, 0).0[3],
            0,
            "transparent stays transparent"
        );
        // The opaque red block survives wherever scaling put it.
        assert!(
            decoded
                .pixels()
                .any(|pixel| pixel.0[3] > 200 && pixel.0[0] > 200),
            "an opaque red pixel remains"
        );
    }

    /// A transparent image: a solid red block in the middle of fully transparent pixels.
    fn transparent_bytes(format: ImageFormat) -> Vec<u8> {
        let mut image = DynamicImage::new_rgba8(64, 64);
        for x in 0..64 {
            for y in 0..64 {
                let inside = (20..44).contains(&x) && (20..44).contains(&y);
                image.put_pixel(
                    x,
                    y,
                    if inside {
                        Rgba([250, 0, 0, 255])
                    } else {
                        Rgba([0, 0, 0, 0])
                    },
                );
            }
        }
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), format)
            .expect("encode transparent fixture");
        bytes
    }

    fn assert_transparency_survives(thumbnail: &GeneratedThumbnail) {
        assert_eq!(thumbnail.content_type, "image/png");
        assert_eq!(thumbnail_content_type(&thumbnail.bytes), "image/png");
        let decoded = image::load_from_memory(&thumbnail.bytes)
            .expect("decode thumbnail")
            .to_rgba8();
        assert_eq!(
            decoded.get_pixel(0, 0).0[3],
            0,
            "transparent stays transparent"
        );
        assert!(
            decoded
                .pixels()
                .any(|pixel| pixel.0[3] > 200 && pixel.0[0] > 200),
            "the opaque red block remains"
        );
    }

    #[test]
    fn a_transparent_png_keeps_its_transparency() {
        let thumbnail =
            generate_image_thumbnail(&transparent_bytes(ImageFormat::Png), ThumbnailSize::Medium)
                .expect("thumbnail");
        assert_transparency_survives(&thumbnail);
    }

    #[test]
    fn a_transparent_webp_keeps_its_transparency() {
        let thumbnail =
            generate_image_thumbnail(&transparent_bytes(ImageFormat::WebP), ThumbnailSize::Medium)
                .expect("thumbnail");
        assert_transparency_survives(&thumbnail);
    }

    #[test]
    fn a_transparent_gif_keeps_its_transparency() {
        let thumbnail =
            generate_image_thumbnail(&transparent_bytes(ImageFormat::Gif), ThumbnailSize::Medium)
                .expect("thumbnail");
        assert_transparency_survives(&thumbnail);
    }

    #[test]
    fn an_opaque_image_with_an_alpha_channel_stays_jpeg() {
        // fixture_bytes writes RGBA pixels that are all fully opaque: nothing to preserve.
        let bytes = fixture_bytes(ImageFormat::Png, 80, 80);
        let thumbnail = generate_image_thumbnail(&bytes, ThumbnailSize::Medium).expect("thumbnail");
        assert_eq!(thumbnail.content_type, "image/jpeg");
    }

    #[test]
    fn photos_stay_jpeg() {
        let bytes = fixture_bytes(ImageFormat::Png, 100, 100);
        let thumbnail = generate_image_thumbnail(&bytes, ThumbnailSize::Small).expect("thumbnail");
        assert_eq!(thumbnail.content_type, "image/jpeg");
        assert_eq!(thumbnail_content_type(&thumbnail.bytes), "image/jpeg");
    }

    #[test]
    fn rejects_a_source_file_over_the_byte_budget_before_decoding() {
        // Filled with zeros, not a valid image at all - proves the size
        // check runs before any decode attempt (a decode error would also
        // be a plausible-looking failure otherwise).
        let bytes = vec![0_u8; (MAX_SOURCE_BYTES + 1) as usize];
        let error = generate_image_thumbnail(&bytes, ThumbnailSize::Small).unwrap_err();
        assert!(matches!(
            error,
            ThumbnailError::SourceTooLarge { size, limit }
                if size == bytes_len(&bytes) && limit == MAX_SOURCE_BYTES
        ));
    }

    fn bytes_len(bytes: &[u8]) -> u64 {
        bytes.len() as u64
    }

    #[test]
    fn rejects_garbage_bytes_as_unsupported_format() {
        let bytes = b"not an image".to_vec();
        let error = generate_image_thumbnail(&bytes, ThumbnailSize::Small).unwrap_err();
        assert!(matches!(error, ThumbnailError::UnsupportedFormat));
    }

    #[test]
    fn thumbnail_size_round_trips_through_as_str_and_parse() {
        for size in [
            ThumbnailSize::Small,
            ThumbnailSize::Medium,
            ThumbnailSize::Large,
        ] {
            assert_eq!(ThumbnailSize::parse(size.as_str()), Some(size));
        }
        assert_eq!(ThumbnailSize::parse("huge"), None);
    }

    #[test]
    fn recognizes_supported_image_extensions_case_insensitively() {
        for extension in [
            "jpg", "JPG", "jpeg", "png", "PNG", "gif", "webp", "ico", "ICO",
        ] {
            assert!(is_supported_image_extension(extension), "{extension}");
        }
        assert!(!is_supported_image_extension("cbz"));
        assert!(!is_supported_image_extension("mp4"));
    }
}
