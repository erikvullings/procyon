//! First-page thumbnail extraction for PDF documents (task 0134 follow-up).
//!
//! **Deliberately partial**: this is not a PDF renderer. No small pure-Rust
//! PDF rasterizer exists - real page rendering (fonts, vector paths, colour
//! spaces) is what PDFium/MuPDF exist for, and both require a ~15-20MB
//! non-Rust native library, the same category of dependency this task
//! avoids for video (no `ffmpeg`). Instead, this extracts the largest
//! embedded raster image on page 1 via `lopdf` (pure Rust, no heavy
//! transitive deps with `default-features = false`) and thumbnails that
//! directly. This covers the common case of a scanned/photographed page;
//! an ordinary text/vector document has no embedded page-sized image and
//! reports [`ThumbnailError::UnsupportedFormat`], falling back to the
//! generic icon like every other unsupported format here.

use std::io::{Cursor, Read};

use lopdf::Document;
use lopdf::xobject::PdfImage;

use crate::thumbnail::{GeneratedThumbnail, MAX_SOURCE_BYTES, ThumbnailError, ThumbnailSize};

/// Extensions this module will attempt to open (without the leading dot,
/// case-insensitive).
pub const SUPPORTED_PDF_EXTENSIONS: &[&str] = &["pdf"];

/// Whether `extension` is a document [`generate_pdf_thumbnail`] will attempt
/// to open.
pub fn is_supported_pdf_extension(extension: &str) -> bool {
    let lower = extension.to_ascii_lowercase();
    SUPPORTED_PDF_EXTENSIONS.contains(&lower.as_str())
}

/// Decodes a `PdfImage`'s raw stream bytes into a raster image.
///
/// `PdfImage::content` is the stream exactly as stored (still filtered) -
/// `lopdf::Document::get_page_images` does not decode it, since the right
/// decoding step depends on the filter chain. Handles the two common cases:
/// `DCTDecode` (the stream bytes already *are* a complete JPEG file - decode
/// directly), and `FlateDecode` producing raw 8-bit `DeviceRGB`/`DeviceGray`
/// samples (zlib-inflate, then reconstruct). Anything else (JPEG2000,
/// CCITT fax, JBIG2, indexed/palette colour, non-8-bit samples) is reported
/// unsupported rather than guessed at.
fn decode_pdf_image(image: &PdfImage<'_>) -> Result<image::DynamicImage, ThumbnailError> {
    let filters = image.filters.as_deref().unwrap_or(&[]);
    if filters.iter().any(|filter| filter == "DCTDecode") {
        return Ok(image::load_from_memory(image.content)?);
    }

    let only_flate_or_uncompressed = filters.iter().all(|filter| filter == "FlateDecode");
    if !only_flate_or_uncompressed {
        // JPXDecode, CCITTFaxDecode, JBIG2Decode, or an unrecognized filter.
        return Err(ThumbnailError::UnsupportedFormat);
    }
    if image.bits_per_component != Some(8) {
        return Err(ThumbnailError::UnsupportedFormat);
    }
    let width = u32::try_from(image.width).map_err(|_| ThumbnailError::UnsupportedFormat)?;
    let height = u32::try_from(image.height).map_err(|_| ThumbnailError::UnsupportedFormat)?;

    let mut inflated = Vec::new();
    let samples: &[u8] = if filters.iter().any(|filter| filter == "FlateDecode") {
        let mut decoder = flate2::read::ZlibDecoder::new(image.content);
        decoder
            .read_to_end(&mut inflated)
            .map_err(|_| ThumbnailError::UnsupportedFormat)?;
        &inflated
    } else {
        image.content
    };

    match image.color_space.as_deref() {
        Some("DeviceRGB") => {
            let expected = (width as usize) * (height as usize) * 3;
            let buffer = samples
                .get(..expected)
                .ok_or(ThumbnailError::UnsupportedFormat)?;
            image::RgbImage::from_raw(width, height, buffer.to_vec())
                .map(image::DynamicImage::ImageRgb8)
                .ok_or(ThumbnailError::UnsupportedFormat)
        }
        Some("DeviceGray") => {
            let expected = (width as usize) * (height as usize);
            let buffer = samples
                .get(..expected)
                .ok_or(ThumbnailError::UnsupportedFormat)?;
            image::GrayImage::from_raw(width, height, buffer.to_vec())
                .map(image::DynamicImage::ImageLuma8)
                .ok_or(ThumbnailError::UnsupportedFormat)
        }
        // DeviceCMYK, Indexed and ICC-based colour spaces are not reconstructed.
        _ => Err(ThumbnailError::UnsupportedFormat),
    }
}

/// Thumbnails the largest embedded raster image on a PDF's first page. See
/// the module docs for exactly what this does and does not support.
pub fn generate_pdf_thumbnail(
    bytes: &[u8],
    size: ThumbnailSize,
) -> Result<GeneratedThumbnail, ThumbnailError> {
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(ThumbnailError::SourceTooLarge {
            size: bytes.len() as u64,
            limit: MAX_SOURCE_BYTES,
        });
    }

    let document = Document::load_mem(bytes).map_err(|_| ThumbnailError::UnsupportedFormat)?;
    let first_page_id = document
        .get_pages()
        .values()
        .next()
        .copied()
        .ok_or(ThumbnailError::UnsupportedFormat)?;
    let images = document
        .get_page_images(first_page_id)
        .map_err(|_| ThumbnailError::UnsupportedFormat)?;
    let largest = images
        .iter()
        .max_by_key(|image| image.width.saturating_mul(image.height))
        .ok_or(ThumbnailError::UnsupportedFormat)?;

    let decoded = decode_pdf_image(largest)?;
    let thumbnail = decoded.thumbnail(size.max_dimension(), size.max_dimension());
    let mut out = Vec::new();
    thumbnail
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .map_err(ThumbnailError::Encode)?;
    Ok(GeneratedThumbnail {
        bytes: out,
        content_type: "image/jpeg",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, GenericImage, Rgba};
    use lopdf::{Dictionary, Object, Stream, dictionary};

    /// Builds a minimal-but-real single-page PDF with one embedded image
    /// XObject, using `lopdf`'s own writer - end-to-end through the exact
    /// parse/extract path production code uses.
    fn build_fixture_pdf(image_dict_extra: Dictionary, image_bytes: Vec<u8>) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();

        let mut image_dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
        };
        for (key, value) in image_dict_extra.iter() {
            image_dict.set(key.clone(), value.clone());
        }
        let image_id = doc.add_object(Object::Stream(Stream::new(image_dict, image_bytes)));

        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { "Im1" => image_id },
        });
        let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut buffer = Vec::new();
        doc.save_to(&mut buffer).expect("save fixture pdf");
        buffer
    }

    fn jpeg_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut image = DynamicImage::new_rgba8(width, height);
        for x in 0..width {
            for y in 0..height {
                image.put_pixel(x, y, Rgba([(x % 255) as u8, (y % 255) as u8, 128, 255]));
            }
        }
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .expect("encode fixture jpeg");
        bytes
    }

    #[test]
    fn generates_a_thumbnail_from_a_dct_decode_embedded_jpeg() {
        let jpeg = jpeg_bytes(120, 60);
        let extra = dictionary! {
            "Width" => 120,
            "Height" => 60,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
        };
        let bytes = build_fixture_pdf(extra, jpeg);

        let thumbnail =
            generate_pdf_thumbnail(&bytes, ThumbnailSize::Small).expect("generate thumbnail");
        assert_eq!(thumbnail.content_type, "image/jpeg");
        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode result");
        assert!(decoded.width() <= ThumbnailSize::Small.max_dimension());
        // 2:1 aspect ratio preserved.
        assert_eq!(decoded.width(), decoded.height() * 2);
    }

    #[test]
    fn generates_a_thumbnail_from_a_flate_decode_raw_rgb_image() {
        let width = 40_u32;
        let height = 20_u32;
        let mut raw = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                raw.extend_from_slice(&[(x % 255) as u8, (y % 255) as u8, 64]);
            }
        }
        let mut compressed = Vec::new();
        {
            let mut encoder =
                flate2::write::ZlibEncoder::new(&mut compressed, flate2::Compression::fast());
            std::io::Write::write_all(&mut encoder, &raw).expect("compress fixture");
        }
        let extra = dictionary! {
            "Width" => i64::from(width),
            "Height" => i64::from(height),
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "FlateDecode",
        };
        let bytes = build_fixture_pdf(extra, compressed);

        let thumbnail =
            generate_pdf_thumbnail(&bytes, ThumbnailSize::Medium).expect("generate thumbnail");
        let decoded = image::load_from_memory(&thumbnail.bytes).expect("decode result");
        assert_eq!(decoded.width(), decoded.height() * 2);
    }

    #[test]
    fn rejects_a_pdf_page_with_no_embedded_image_as_unsupported() {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("save fixture pdf");

        let error = generate_pdf_thumbnail(&bytes, ThumbnailSize::Small).unwrap_err();
        assert!(matches!(error, ThumbnailError::UnsupportedFormat));
    }

    #[test]
    fn rejects_a_jpx_decode_image_as_unsupported_rather_than_misdecoding_it() {
        let extra = dictionary! {
            "Width" => 10,
            "Height" => 10,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "JPXDecode",
        };
        let bytes = build_fixture_pdf(extra, vec![0_u8; 32]);

        let error = generate_pdf_thumbnail(&bytes, ThumbnailSize::Small).unwrap_err();
        assert!(matches!(error, ThumbnailError::UnsupportedFormat));
    }

    #[test]
    fn rejects_a_non_pdf_file_as_unsupported() {
        let error = generate_pdf_thumbnail(b"not a pdf file", ThumbnailSize::Small).unwrap_err();
        assert!(matches!(error, ThumbnailError::UnsupportedFormat));
    }

    #[test]
    fn rejects_a_source_file_over_the_byte_budget_before_parsing() {
        let bytes = vec![0_u8; (MAX_SOURCE_BYTES + 1) as usize];
        let error = generate_pdf_thumbnail(&bytes, ThumbnailSize::Small).unwrap_err();
        assert!(matches!(error, ThumbnailError::SourceTooLarge { .. }));
    }

    #[test]
    fn recognizes_supported_pdf_extensions_case_insensitively() {
        assert!(is_supported_pdf_extension("pdf"));
        assert!(is_supported_pdf_extension("PDF"));
        assert!(!is_supported_pdf_extension("epub"));
    }
}
