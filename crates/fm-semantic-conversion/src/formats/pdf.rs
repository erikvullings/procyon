//! Text-layer PDF conversion.
//!
//! `lopdf` reads the document structure and the per-page content streams; page
//! content is decompressed under an explicit limit so a small compressed
//! stream cannot inflate without bound. Pages are visited in ascending page
//! number - `Document::get_pages` returns a `BTreeMap`, and the page numbers
//! are collected and sorted explicitly rather than relying on object order.
//!
//! There is no rasterization and no OCR here: a PDF whose pages carry no
//! extractable text is reported as
//! [`ConversionOutcome::NoTextLayer`](crate::ConversionOutcome::NoTextLayer),
//! which is the signal an OCR pack (task 0189) would act on. An encrypted
//! document is reported as encrypted rather than parsed as garbage.

use std::collections::HashSet;
use std::io::{self, Write};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use lopdf::{Dictionary, Document, LoadOptions, Object, content::Content};

use crate::MAX_PDF_STRUCTURAL_STREAM_BYTES;
use crate::budget::Stop;
use crate::builder::{DocumentBuilder, UnitDraft, VisualDraft};
use crate::model::{
    MAX_VISUAL_ATTACHMENTS, MAX_VISUAL_BYTES, Provenance, TopLevelBoundary, UnitKind,
    VisualProvenance,
};
use crate::pdf_toc::{clean_pdf_toc_text, repair_spaced_elsevier_doi};

/// Maximum decompressed content bytes per page.
const MAX_PAGE_CONTENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_PDF_FLATE_RASTER_BYTES: usize = 4 * 1024 * 1024;

/// Why a PDF could not be converted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PdfError {
    /// The file is not a readable PDF.
    Malformed(String),
    /// The document is encrypted.
    Encrypted(String),
    /// The document has pages but no extractable text layer.
    NoTextLayer(String),
    /// A hard budget stopped the work, or the caller cancelled it.
    Stopped(Stop),
}

impl From<Stop> for PdfError {
    fn from(stop: Stop) -> Self {
        Self::Stopped(stop)
    }
}

/// Splits one page's extracted text into blocks separated by blank lines,
/// falling back to the whole page when it has no blank lines.
fn page_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut current_navigation = false;
    for line in text.lines() {
        let trimmed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.is_empty() {
            if !current.trim().is_empty() {
                blocks.push(std::mem::take(&mut current).trim().to_owned());
            }
            current.clear();
            continue;
        }
        let navigation = clean_pdf_toc_text(&trimmed).1;
        if !current.is_empty() && current_navigation != navigation {
            blocks.push(std::mem::take(&mut current));
        }
        current_navigation = navigation;
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(&trimmed);
    }
    if !current.trim().is_empty() {
        blocks.push(current.trim().to_owned());
    }
    blocks
}

/// Converts a PDF's text layer into per-page blocks.
pub(crate) fn convert(builder: &mut DocumentBuilder<'_, '_>, bytes: &[u8]) -> Result<(), PdfError> {
    let document = Document::load_mem_with_options(
        bytes,
        LoadOptions::with_max_decompressed_size(MAX_PDF_STRUCTURAL_STREAM_BYTES),
    )
    .map_err(|error| PdfError::Malformed(format!("the PDF is not readable: {error}")))?;
    if document.is_encrypted() {
        return Err(PdfError::Encrypted(
            "the PDF is encrypted and cannot be read without a password".to_owned(),
        ));
    }
    let pages = crate::checked_pdf_pages(&document).map_err(PdfError::Malformed)?;
    let mut page_numbers: Vec<u32> = pages.keys().copied().collect();
    page_numbers.sort_unstable();
    if page_numbers.is_empty() {
        return Err(PdfError::Malformed("the PDF contains no pages".to_owned()));
    }
    builder
        .tracker()
        .charge_items(page_numbers.len() as u64)
        .map_err(PdfError::from)?;

    let mut extracted_any = false;
    let mut uncovered_pages = 0_u32;
    let mut first_uncovered_page = None;
    for page_number in page_numbers {
        builder.checkpoint().map_err(PdfError::from)?;
        if builder.is_saturated() {
            break;
        }
        let text = match document.extract_text_with_limit(&[page_number], MAX_PAGE_CONTENT_BYTES) {
            Ok(text) => text,
            Err(error) => {
                return Err(PdfError::Malformed(format!(
                    "page {page_number} could not be read: {error}"
                )));
            }
        };
        for (block_index, block) in page_blocks(&text).into_iter().enumerate() {
            builder.checkpoint().map_err(PdfError::from)?;
            if builder.is_saturated() {
                break;
            }
            let (text, navigation) = clean_pdf_toc_text(&repair_spaced_elsevier_doi(&block));
            let pushed = builder
                .push(UnitDraft {
                    kind: if navigation {
                        UnitKind::Navigation
                    } else {
                        UnitKind::Paragraph
                    },
                    section_path: Vec::new(),
                    text,
                    provenance: Provenance::PdfBlock {
                        page_number,
                        block_index: block_index as u32,
                    },
                    boundary: TopLevelBoundary::Page(page_number),
                    source_offset: None,
                })
                .map_err(PdfError::from)?;
            extracted_any |= pushed;
        }
        let Some(page_id) = pages.get(&page_number).copied() else {
            continue;
        };
        let retained_image = if builder.visuals_enabled() {
            extract_page_images(builder, &document, page_id, page_number)?
        } else {
            false
        };
        if text.trim().is_empty() && builder.visuals_enabled() && !retained_image {
            uncovered_pages += 1;
            first_uncovered_page.get_or_insert(page_number);
            builder.omit_visual(
                Some(VisualProvenance::PdfPage { page_number }),
                format!(
                    "page {page_number} has no extractable text; page rendering is unsupported"
                ),
            );
        }
    }
    if !extracted_any && !builder.has_visuals() {
        return Err(PdfError::NoTextLayer(if builder.visuals_enabled() {
            format!(
                "the PDF has no extractable text or reusable image rasters; {uncovered_pages} pages omitted (first: page {}); page rendering is unsupported and an OCR pack is required",
                first_uncovered_page.unwrap_or(1)
            )
        } else {
            "the PDF pages contain no extractable text layer; an OCR pack is required".to_owned()
        }));
    }
    Ok(())
}

fn extract_page_images(
    builder: &mut DocumentBuilder<'_, '_>,
    document: &Document,
    page_id: lopdf::ObjectId,
    page_number: u32,
) -> Result<bool, PdfError> {
    let mut retained = false;
    let xobjects = match page_xobjects(document, page_id) {
        Ok(xobjects) => xobjects,
        Err(error) => {
            builder.omit_visual(
                Some(VisualProvenance::PdfPage { page_number }),
                format!("PDF page images could not be inspected: {error}"),
            );
            return Ok(false);
        }
    };
    let Some(xobjects) = xobjects else {
        return Ok(false);
    };
    let content = document
        .get_page_content_with_limit(page_id, MAX_PAGE_CONTENT_BYTES)
        .and_then(|bytes| Content::decode(&bytes))
        .map_err(|error| {
            PdfError::Malformed(format!(
                "page {page_number} images could not be located: {error}"
            ))
        })?;
    let mut drawn = HashSet::new();
    for operation in &content.operations {
        if operation.operator == "Do" {
            let name = operation
                .operands
                .first()
                .ok_or_else(|| {
                    PdfError::Malformed(format!(
                        "page {page_number} has an image drawing operator without a resource"
                    ))
                })?
                .as_name()
                .map_err(|error| {
                    PdfError::Malformed(format!(
                        "page {page_number} has an invalid image drawing resource: {error}"
                    ))
                })?;
            drawn.insert(name.to_vec());
        }
    }
    for (image_index, (name, image_ref)) in xobjects.iter().enumerate() {
        if !drawn.contains(name) {
            continue;
        }
        builder.checkpoint().map_err(PdfError::from)?;
        let provenance = VisualProvenance::PdfImage {
            page_number,
            image_index: image_index as u32,
        };
        let image = (|| -> lopdf::Result<&lopdf::Stream> {
            let id = image_ref.as_reference()?;
            document.get_object(id)?.as_stream()
        })();
        let image = match image {
            Ok(image) => image,
            Err(error) => {
                builder.omit_visual(
                    Some(provenance),
                    format!("PDF image could not be inspected: {error}"),
                );
                continue;
            }
        };
        match image.dict.get(b"Subtype").and_then(Object::as_name) {
            Ok(name) if name == b"Image" => {}
            Ok(_) => continue,
            Err(error) => {
                builder.omit_visual(
                    Some(provenance),
                    format!("PDF XObject subtype could not be inspected: {error}"),
                );
                continue;
            }
        }
        let filter = match image.dict.get(b"Filter") {
            Ok(Object::Name(filter)) => Some(filter.as_slice()),
            Ok(Object::Array(filters)) if filters.len() == 1 => filters[0].as_name().ok(),
            _ => None,
        };
        let (media_type, data) = match filter {
            Some(b"DCTDecode") if image.content.len() <= MAX_VISUAL_BYTES => {
                ("image/jpeg", image.content.clone())
            }
            Some(b"DCTDecode") => {
                builder.omit_visual(
                    Some(provenance),
                    format!("PDF image exceeds {MAX_VISUAL_BYTES} retained bytes"),
                );
                continue;
            }
            Some(b"FlateDecode") => match png_from_flate_image(image) {
                Ok(png) => ("image/png", png),
                Err(reason) => {
                    builder.omit_visual(Some(provenance), reason);
                    continue;
                }
            },
            _ => {
                builder.omit_visual(
                    Some(provenance),
                    "PDF image encoding is not a reusable JPEG or simple Flate raster",
                );
                continue;
            }
        };
        if builder.visual_count_exhausted() {
            return Err(PdfError::Malformed(format!(
                "page {page_number} image {} exceeds the {MAX_VISUAL_ATTACHMENTS}-attachment PDF limit; no partial generation published",
                image_index + 1
            )));
        }
        if data.len() > builder.remaining_visual_bytes() {
            return Err(PdfError::Malformed(format!(
                "page {page_number} image {} exceeds the remaining PDF visual byte budget; no partial generation published",
                image_index + 1
            )));
        }
        retained |= builder
            .push_visual(VisualDraft {
                media_type,
                data,
                provenance,
                caption: None,
            })
            .map_err(PdfError::from)?;
    }
    Ok(retained)
}

struct BoundedPngWriter(Vec<u8>);

impl Write for BoundedPngWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_VISUAL_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other(
                "encoded PDF image exceeds visual byte limit",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn png_from_flate_image(image: &lopdf::Stream) -> Result<Vec<u8>, &'static str> {
    if [
        b"DecodeParms".as_slice(),
        b"Decode",
        b"Mask",
        b"SMask",
        b"ImageMask",
    ]
    .iter()
    .any(|key| image.dict.has(key))
    {
        return Err("PDF Flate image has unsupported predictor, palette, mask or decode mapping");
    }
    let channels = match image.dict.get(b"ColorSpace").and_then(Object::as_name) {
        Ok(b"DeviceRGB") => 3_usize,
        Ok(b"DeviceGray") => 1,
        _ => return Err("PDF Flate image color space is unsupported"),
    };
    if !matches!(
        image.dict.get(b"BitsPerComponent").and_then(Object::as_i64),
        Ok(8)
    ) {
        return Err("PDF Flate image must have 8 bits per component");
    }
    let size = (|| {
        let width = u32::try_from(image.dict.get(b"Width").ok()?.as_i64().ok()?).ok()?;
        let height = u32::try_from(image.dict.get(b"Height").ok()?.as_i64().ok()?).ok()?;
        let bytes = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(channels)?;
        (width > 0 && height > 0 && bytes <= MAX_PDF_FLATE_RASTER_BYTES)
            .then_some((width, height, bytes))
    })()
    .ok_or("PDF Flate image dimensions or decoded bytes exceed the raster limit")?;
    let (width, height, expected_bytes) = size;
    let pixels = image
        .decompressed_content_with_limit(expected_bytes)
        .map_err(|_| "PDF Flate image could not be decompressed within its raster limit")?;
    if pixels.len() != expected_bytes {
        return Err("PDF Flate image decoded byte length does not match its dimensions");
    }
    let mut encoded = BoundedPngWriter(Vec::new());
    let color = if channels == 3 {
        ExtendedColorType::Rgb8
    } else {
        ExtendedColorType::L8
    };
    PngEncoder::new(&mut encoded)
        .write_image(&pixels, width, height, color)
        .map_err(|_| "PDF Flate image could not be encoded within the visual byte limit")?;
    Ok(encoded.0)
}

fn page_xobjects(
    document: &Document,
    page_id: lopdf::ObjectId,
) -> lopdf::Result<Option<&Dictionary>> {
    let mut current = page_id;
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(current) {
            return Err(lopdf::Error::ReferenceCycle(current));
        }
        let page = document.get_dictionary(current)?;
        match document.get_dict_in_dict(page, b"Resources") {
            Ok(resources) => {
                return match document.get_dict_in_dict(resources, b"XObject") {
                    Ok(xobjects) => Ok(Some(xobjects)),
                    Err(lopdf::Error::DictKey(_)) => Ok(None),
                    Err(error) => Err(error),
                };
            }
            Err(lopdf::Error::DictKey(_)) => {}
            Err(error) => return Err(error),
        }
        current = match page.get(b"Parent") {
            Ok(parent) => parent.as_reference()?,
            Err(lopdf::Error::DictKey(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use lopdf::{Object, Stream, dictionary};

    use super::*;
    use crate::budget::{BudgetTracker, ConversionBudgets, ManualClock};
    use crate::cancellation::Cancellation;
    use crate::model::{ComponentVersion, ConvertedDocument, FormatKind, VisualProvenance};

    /// Generates a small single-font PDF whose pages carry the given text
    /// lines, so no binary fixture is checked in.
    pub(crate) fn text_pdf(pages: &[&[&str]]) -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let mut page_ids = Vec::new();
        for lines in pages {
            let mut content = String::from("BT /F1 12 Tf 72 720 Td 14 TL\n");
            for line in *lines {
                content.push_str(&format!("({line}) Tj T*\n"));
            }
            content.push_str("ET\n");
            let content_id = document.add_object(Stream::new(dictionary! {}, content.into_bytes()));
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            });
            page_ids.push(page_id);
        }
        let kids: Vec<Object> = page_ids.iter().map(|id| Object::Reference(*id)).collect();
        let count = kids.len() as i64;
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => count,
                "Resources" => resources_id,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save pdf");
        bytes
    }

    /// Generates a PDF whose single page has no text-drawing operators.
    pub(crate) fn image_only_pdf() -> Vec<u8> {
        text_pdf(&[&[]])
    }

    #[test]
    fn baseline_pdf_removes_only_navigation_leaders() {
        let pdf = text_pdf(&[&[
            "........................ Aggression",
            "Alcohol/drug abuse .......... 12",
            "",
            "Ordinary prose... retains its ellipsis.",
        ]]);
        let document = convert_pdf(&pdf).expect("convert PDF");
        assert_eq!(document.units()[0].kind, UnitKind::Navigation);
        assert!(!document.units()[0].text.contains("...."));
        assert!(
            document
                .units()
                .iter()
                .any(|unit| unit.kind == UnitKind::Paragraph
                    && unit.text.contains("prose... retains"))
        );
    }

    #[test]
    fn baseline_pdf_repairs_spaced_elsevier_doi_without_joining_prose() {
        let pdf = text_pdf(&[&[
            "Child Abuse & Neglect. https://doi.org/10.1016/j. chiabu.2011.01.002.",
            "Another sentence: j. chiabu.2011.01.002 is not a DOI.",
        ]]);
        let document = convert_pdf(&pdf).expect("convert PDF");
        let text = &document.units()[0].text;
        assert!(text.contains("https://doi.org/10.1016/j.chiabu.2011.01.002."));
        assert!(text.contains("j. chiabu.2011.01.002 is not a DOI."));
    }

    fn convert_pdf(bytes: &[u8]) -> Result<ConvertedDocument, PdfError> {
        convert_pdf_with_visuals(bytes, false)
    }

    fn convert_pdf_with_visuals(
        bytes: &[u8],
        visuals: bool,
    ) -> Result<ConvertedDocument, PdfError> {
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Pdf,
            &mut tracker,
        );
        builder.set_visuals_enabled(visuals);
        convert(&mut builder, bytes)?;
        Ok(builder.finish())
    }

    #[test]
    fn pages_are_converted_in_ascending_order_with_page_provenance() {
        let bytes = text_pdf(&[&["Page one text"], &["Page two text"]]);
        let document = convert_pdf(&bytes).expect("conversion");
        let pages: Vec<u32> = document
            .units()
            .iter()
            .map(|unit| match unit.provenance {
                Provenance::PdfBlock { page_number, .. } => page_number,
                ref other => panic!("unexpected provenance {other:?}"),
            })
            .collect();
        assert_eq!(pages, [1, 2]);
        assert!(document.units()[0].text.contains("Page one text"));
        assert_eq!(document.units()[1].boundary, TopLevelBoundary::Page(2));
    }

    #[test]
    fn a_pdf_without_a_text_layer_is_reported_as_such() {
        let bytes = image_only_pdf();
        assert!(matches!(convert_pdf(&bytes), Err(PdfError::NoTextLayer(_))));
    }

    #[test]
    fn oversized_decompressed_page_is_not_misreported_as_needing_ocr() {
        let bytes = text_pdf(&[&[]]);
        let mut document = Document::load_mem(&bytes).expect("load fixture");
        let page_id = document.get_pages()[&1];
        let content_id = document.get_page_contents(page_id)[0];
        let stream = document
            .get_object_mut(content_id)
            .expect("page content")
            .as_stream_mut()
            .expect("content stream");
        stream.content = vec![b' '; MAX_PAGE_CONTENT_BYTES + 1];
        stream.compress().expect("compress content");
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save fixture");

        assert!(
            matches!(convert_pdf(&bytes), Err(PdfError::Malformed(detail)) if detail.contains("page 1")),
            "an unreadable page must not offer OCR guidance"
        );
    }

    #[test]
    fn oversized_structural_object_stream_cannot_hide_a_page() {
        let bytes = text_pdf(&[&["Visible text"]]);
        let mut document = Document::load_mem(&bytes).expect("load fixture");
        let page_id = document.get_pages()[&1];
        document
            .get_dictionary_mut(page_id)
            .expect("page dictionary")
            .set(
                "LargeStructuralValue",
                Object::string_literal(vec![b' '; MAX_PAGE_CONTENT_BYTES + 1]),
            );
        let mut bytes = Vec::new();
        document
            .save_modern(&mut bytes)
            .expect("save compressed PDF");
        assert!(
            bytes.len() < 1024 * 1024,
            "fixture must be a compression bomb"
        );
        assert!(
            matches!(convert_pdf(&bytes), Err(PdfError::Malformed(_))),
            "losing compressed page objects must fail, not publish incomplete evidence"
        );
    }

    #[test]
    fn lost_compressed_page_cannot_publish_the_other_page() {
        let bytes = text_pdf(&[&["first page"], &["second page"]]);
        let mut document = Document::load_mem(&bytes).expect("load fixture");
        let first = document.get_pages()[&1];
        document.get_dictionary_mut(first).expect("first page").set(
            "LargeStructuralValue",
            Object::string_literal(vec![b' '; MAX_PDF_STRUCTURAL_STREAM_BYTES + 1]),
        );
        let options = lopdf::SaveOptions::builder()
            .use_object_streams(true)
            .use_xref_streams(true)
            .max_objects_per_stream(1)
            .build();
        let mut bytes = Vec::new();
        document
            .save_with_options(&mut bytes, options)
            .expect("save split compressed PDF");
        let loaded = Document::load_mem_with_options(
            &bytes,
            LoadOptions::with_max_decompressed_size(MAX_PDF_STRUCTURAL_STREAM_BYTES),
        )
        .expect("load bounded PDF");
        assert_eq!(
            loaded.get_pages().len(),
            1,
            "fixture must lose only one page"
        );
        assert!(
            matches!(convert_pdf(&bytes), Err(PdfError::Malformed(detail)) if detail.contains("declared 2 pages, loaded 1")),
            "a remaining readable page cannot mask missing page structure"
        );
    }

    #[test]
    fn unreadable_page_does_not_publish_other_pages_as_complete() {
        let bytes = text_pdf(&[&["good first page"], &["good second page"]]);
        let mut document = Document::load_mem(&bytes).expect("load fixture");
        let content_id = document.get_page_contents(document.get_pages()[&1])[0];
        let stream = document
            .get_object_mut(content_id)
            .expect("page content")
            .as_stream_mut()
            .expect("content stream");
        stream.content = vec![b' '; MAX_PAGE_CONTENT_BYTES + 1];
        stream.compress().expect("compress content");
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save fixture");

        assert!(
            matches!(convert_pdf(&bytes), Err(PdfError::Malformed(detail)) if detail.contains("page 1")),
            "the good second page cannot mask an unreadable first page"
        );
    }

    #[test]
    fn a_truncated_pdf_is_malformed() {
        let bytes = b"%PDF-1.5\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n".to_vec();
        assert!(matches!(convert_pdf(&bytes), Err(PdfError::Malformed(_))));
    }

    #[test]
    fn page_text_is_split_into_blocks_on_blank_lines() {
        let blocks = page_blocks("first line\nsecond line\n\n\nthird block\n");
        assert_eq!(blocks, ["first line\nsecond line", "third block"]);
    }

    #[test]
    fn inherited_pdf_jpeg_images_are_retained_with_page_provenance() {
        let bytes = text_pdf(&[&["Visible text"]]);
        let mut document = Document::load_mem(&bytes).expect("load fixture");
        let page_id = document.get_pages()[&1];
        let content_id = document.get_page_contents(page_id)[0];
        let content = document
            .get_object_mut(content_id)
            .expect("page content")
            .as_stream_mut()
            .expect("content stream");
        let mut drawing = content.content.clone();
        drawing.extend_from_slice(b"q /Im1 Do Q\n");
        content.set_content(drawing);
        let jpeg = vec![
            0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0, 1, 0, 2, 3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0,
            0xff, 0xd9,
        ];
        let image_id = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2,
                "Height" => 1,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            jpeg,
        ));
        let resources_id = document.get_page_resources(page_id).unwrap().1[0];
        document
            .get_dictionary_mut(resources_id)
            .expect("inherited resources")
            .set("XObject", dictionary! { "Im1" => image_id });
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save fixture");

        let converted = convert_pdf_with_visuals(&bytes, true).expect("conversion");

        assert_eq!(converted.visuals().len(), 1);
        assert_eq!(
            converted.visuals()[0].provenance,
            VisualProvenance::PdfImage {
                page_number: 1,
                image_index: 0
            }
        );
    }

    #[test]
    fn unused_page_image_resource_is_not_searchable_evidence() {
        let bytes = text_pdf(&[&["Visible text"]]);
        let mut document = Document::load_mem(&bytes).expect("load fixture");
        let page_id = document.get_pages()[&1];
        let image_id = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2,
                "Height" => 1,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            vec![
                0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0, 1, 0, 2, 3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11,
                0, 0xff, 0xd9,
            ],
        ));
        let resources_id = document.get_page_resources(page_id).unwrap().1[0];
        document
            .get_dictionary_mut(resources_id)
            .expect("inherited resources")
            .set("XObject", dictionary! { "Im1" => image_id });
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save fixture");

        let converted = convert_pdf_with_visuals(&bytes, true).expect("conversion");
        assert!(converted.visuals().is_empty());
        assert!(converted.visual_omissions().is_empty());
    }

    #[test]
    fn drawn_flate_rgb_scan_is_retained_as_page_cited_png_only_with_visuals() {
        let bytes = text_pdf(&[&["Searchable first page"], &[]]);
        let mut document = Document::load_mem(&bytes).expect("load fixture");
        let page_id = document.get_pages()[&2];
        let content_id = document.get_page_contents(page_id)[0];
        document
            .get_object_mut(content_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(b"q /Im1 Do Q\n".to_vec());
        let pixels = [255, 0, 0, 0, 255, 0].repeat(1_000);
        let mut image = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2,
                "Height" => 1000,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
            },
            pixels.clone(),
        );
        image.compress().expect("compress pixels");
        assert!(image.dict.has(b"Filter"));
        let image_id = document.add_object(image);
        let resources_id = document.get_page_resources(page_id).unwrap().1[0];
        document
            .get_dictionary_mut(resources_id)
            .unwrap()
            .set("XObject", dictionary! { "Im1" => image_id });
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save scan");

        assert!(
            convert_pdf(&bytes)
                .expect("text-only conversion")
                .visuals()
                .is_empty()
        );
        let converted = convert_pdf_with_visuals(&bytes, true).expect("consented conversion");
        assert_eq!(
            converted.visuals().len(),
            1,
            "{:?}",
            converted.visual_omissions()
        );
        assert_eq!(converted.visuals()[0].media_type.as_str(), "image/png");
        assert_eq!(
            converted.visuals()[0].provenance,
            VisualProvenance::PdfImage {
                page_number: 2,
                image_index: 0
            }
        );
        assert_eq!(
            image::load_from_memory(&converted.visuals()[0].data)
                .expect("valid PNG")
                .to_rgb8()
                .into_raw(),
            pixels
        );

        let mut scan_only = Document::load_mem(&bytes).expect("load scan");
        scan_only
            .get_object_mut(scan_only.get_pages()[&1])
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .remove(b"Contents");
        let mut scan_bytes = Vec::new();
        scan_only
            .save_to(&mut scan_bytes)
            .expect("save image-only PDF");
        assert!(matches!(
            convert_pdf(&scan_bytes),
            Err(PdfError::NoTextLayer(_))
        ));
        let scan = convert_pdf_with_visuals(&scan_bytes, true).expect("consented image-only scan");
        assert_eq!(scan.visuals().len(), 1);
        assert_eq!(
            scan.visuals()[0].provenance,
            converted.visuals()[0].provenance
        );
    }

    #[test]
    fn flate_image_with_predictor_is_omitted_instead_of_misdecoded() {
        let bytes = text_pdf(&[&["Visible text"]]);
        let mut document = Document::load_mem(&bytes).expect("load fixture");
        let page_id = document.get_pages()[&1];
        let content_id = document.get_page_contents(page_id)[0];
        document
            .get_object_mut(content_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(b"q /Im1 Do Q\nBT /F1 12 Tf (Visible text) Tj ET\n".to_vec());
        let mut image = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2,
                "Height" => 1000,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "DecodeParms" => dictionary! { "Predictor" => 12 },
            },
            [255, 0, 0, 0, 255, 0].repeat(1_000),
        );
        image.compress().expect("compress pixels");
        let image_id = document.add_object(image);
        let resources_id = document.get_page_resources(page_id).unwrap().1[0];
        document
            .get_dictionary_mut(resources_id)
            .unwrap()
            .set("XObject", dictionary! { "Im1" => image_id });
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save fixture");

        let converted = convert_pdf_with_visuals(&bytes, true).expect("convert text");
        assert!(converted.visuals().is_empty());
        assert!(converted.visual_omissions().iter().any(|omission| {
            omission.detail.contains("predictor")
                && omission.provenance
                    == Some(VisualProvenance::PdfImage {
                        page_number: 1,
                        image_index: 0,
                    })
        }));

        let mut document = Document::load_mem(&bytes).expect("load compressed fixture");
        let resources_id = document
            .get_page_resources(document.get_pages()[&1])
            .unwrap()
            .1[0];
        let image_id = document
            .get_dict_in_dict(document.get_dictionary(resources_id).unwrap(), b"XObject")
            .unwrap()
            .get(b"Im1")
            .unwrap()
            .as_reference()
            .unwrap();
        let image = document
            .get_object_mut(image_id)
            .unwrap()
            .as_stream_mut()
            .unwrap();
        image.dict.remove(b"DecodeParms");
        image.dict.set("Height", 1);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save bounded fixture");
        let converted = convert_pdf_with_visuals(&bytes, true).expect("convert text");
        assert!(converted.visuals().is_empty());
        assert!(
            converted
                .visual_omissions()
                .iter()
                .any(|omission| omission.detail.contains("raster limit"))
        );
    }

    #[test]
    fn scan_pages_past_attachment_capacity_fail_without_partial_generation() {
        let page_count = crate::model::MAX_VISUAL_ATTACHMENTS + 1;
        let empty_page: &[&str] = &[];
        let pages = vec![empty_page; page_count];
        let bytes = text_pdf(&pages);
        let mut document = Document::load_mem(&bytes).expect("load fixture");
        let jpeg = vec![
            0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0, 1, 0, 2, 3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0,
            0xff, 0xd9,
        ];
        let image_id = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2,
                "Height" => 1,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            jpeg,
        ));
        for page_id in document.get_pages().values().copied().collect::<Vec<_>>() {
            let content_id = document.get_page_contents(page_id)[0];
            document
                .get_object_mut(content_id)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_plain_content(b"q /Im1 Do Q\n".to_vec());
        }
        let resources_id = document
            .get_page_resources(document.get_pages()[&1])
            .unwrap()
            .1[0];
        document
            .get_dictionary_mut(resources_id)
            .unwrap()
            .set("XObject", dictionary! { "Im1" => image_id });
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save fixture");

        assert!(
            matches!(convert_pdf_with_visuals(&bytes, true), Err(PdfError::Malformed(detail))
                if detail.contains(&format!("page {page_count} image 1"))
                    && detail.contains("no partial generation published"))
        );

        let mut document = Document::load_mem(&bytes).expect("load fixture");
        let last_page = document.get_pages()[&(page_count as u32)];
        let unsupported = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 2,
                "Height" => 1, "Filter" => "JPXDecode",
            },
            vec![1, 2, 3],
        ));
        document
            .get_dictionary_mut(last_page)
            .expect("last page")
            .set(
                "Resources",
                dictionary! {
                    "XObject" => dictionary! { "Im1" => unsupported }
                },
            );
        let mut bytes = Vec::new();
        document
            .save_to(&mut bytes)
            .expect("save unsupported fixture");
        let converted = convert_pdf_with_visuals(&bytes, true).expect("unsupported omission");
        assert_eq!(converted.visuals().len(), MAX_VISUAL_ATTACHMENTS);
        assert!(converted.visual_omissions().iter().any(|omission| {
            omission.provenance
                == Some(VisualProvenance::PdfImage {
                    page_number: page_count as u32,
                    image_index: 0,
                })
                && omission.detail.contains("encoding")
        }));
    }

    #[test]
    fn image_only_page_omission_has_structural_page_provenance() {
        let bytes = text_pdf(&[&["Searchable first page"], &[]]);
        let converted = convert_pdf_with_visuals(&bytes, true).expect("conversion");
        let page_omissions = converted
            .visual_omissions()
            .iter()
            .filter(|omission| {
                matches!(omission.provenance, Some(VisualProvenance::PdfPage { .. }))
            })
            .collect::<Vec<_>>();
        assert_eq!(page_omissions.len(), 1);
        assert_eq!(converted.visual_omissions().len(), 1);
        assert_eq!(
            page_omissions[0].provenance,
            Some(VisualProvenance::PdfPage { page_number: 2 })
        );
    }
}
