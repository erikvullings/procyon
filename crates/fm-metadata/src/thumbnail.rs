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
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ThumbnailError::UnsupportedFormat)?;
    if reader.format().is_none() {
        return Err(ThumbnailError::UnsupportedFormat);
    }
    let image = reader.decode()?;
    let thumbnail = image.thumbnail(size.max_dimension(), size.max_dimension());

    let mut out = Vec::new();
    thumbnail
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg)
        .map_err(ThumbnailError::Encode)?;
    Ok(GeneratedThumbnail {
        bytes: out,
        content_type: "image/jpeg",
    })
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
