//! Optional, path-free PDF page rasterization inside the supervised child.

use std::io;
use std::path::Path;

use fm_semantic_conversion::{
    MAX_VISUAL_BYTES, MAX_VISUAL_PIXELS, MediaType, VisualAttachment, VisualProvenance,
};
use image::ImageEncoder;
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};

const MAX_RENDER_SIDE: i32 = 768;
pub(crate) const RENDER_IDENTITY: &str = "pdfium/8086+pdfium-render/0.8.37+jpeg85/768";

pub(crate) fn render_page(
    source: &[u8],
    page_number: u32,
    expected_page_count: u32,
) -> io::Result<VisualAttachment> {
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .ok_or_else(|| io::Error::other("PDF renderer executable has no parent directory"))?;
    let library = Pdfium::pdfium_platform_library_name_at_path(directory);
    render_page_with_library(source, page_number, expected_page_count, &library)
}

fn render_page_with_library(
    source: &[u8],
    page_number: u32,
    expected_page_count: u32,
    library: &Path,
) -> io::Result<VisualAttachment> {
    let bindings = Pdfium::bind_to_library(library).map_err(io::Error::other)?;
    let pdfium = Pdfium::new(bindings);
    let document = pdfium
        .load_pdf_from_byte_slice(source, None)
        .map_err(io::Error::other)?;
    if u32::from(document.pages().len()) != expected_page_count {
        return Err(io::Error::other(
            "PDF render page count disagrees with text conversion",
        ));
    }
    let index = page_number
        .checked_sub(1)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| io::Error::other("PDF renderer page number is invalid"))?;
    let page = document.pages().get(index).map_err(io::Error::other)?;
    let (page_width, page_height) = (page.width().value, page.height().value);
    if !page_width.is_finite()
        || !page_height.is_finite()
        || !(1.0..=14_400.0).contains(&page_width)
        || !(1.0..=14_400.0).contains(&page_height)
    {
        return Err(io::Error::other(
            "PDF page geometry exceeds the rendering budget",
        ));
    }
    let config = PdfRenderConfig::new()
        .set_target_width(MAX_RENDER_SIDE)
        .set_maximum_width(MAX_RENDER_SIDE)
        .set_maximum_height(MAX_RENDER_SIDE);
    let image = page
        .render_with_config(&config)
        .map_err(io::Error::other)?
        .as_image()
        .into_rgb8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_VISUAL_PIXELS {
        return Err(io::Error::other(
            "PDF rendered page exceeds the pixel budget",
        ));
    }
    let mut data = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 85)
        .write_image(&image, width, height, image::ExtendedColorType::Rgb8)
        .map_err(io::Error::other)?;
    if data.is_empty() || data.len() > MAX_VISUAL_BYTES {
        return Err(io::Error::other(
            "PDF rendered page exceeds the byte budget",
        ));
    }
    Ok(VisualAttachment {
        id: blake3::hash(&data).to_hex().to_string(),
        media_type: MediaType::parse("image/jpeg"),
        data,
        width,
        height,
        provenance: VisualProvenance::PdfPageRender { page_number },
        caption: None,
    })
}
