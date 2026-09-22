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

use lopdf::Document;

use crate::budget::Stop;
use crate::builder::{DocumentBuilder, UnitDraft, VisualDraft};
use crate::model::{Omission, Provenance, TopLevelBoundary, UnitKind, VisualProvenance};

/// Maximum decompressed content bytes per page.
const MAX_PAGE_CONTENT_BYTES: usize = 16 * 1024 * 1024;

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
    for line in text.lines() {
        let trimmed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.is_empty() {
            if !current.trim().is_empty() {
                blocks.push(std::mem::take(&mut current).trim().to_owned());
            }
            current.clear();
            continue;
        }
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
    let document = Document::load_mem(bytes)
        .map_err(|error| PdfError::Malformed(format!("the PDF is not readable: {error}")))?;
    if document.is_encrypted() {
        return Err(PdfError::Encrypted(
            "the PDF is encrypted and cannot be read without a password".to_owned(),
        ));
    }
    let pages = document.get_pages();
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
    for page_number in page_numbers {
        builder.checkpoint().map_err(PdfError::from)?;
        if builder.is_saturated() {
            break;
        }
        let text = match document.extract_text_with_limit(&[page_number], MAX_PAGE_CONTENT_BYTES) {
            Ok(text) => text,
            Err(error) => {
                builder.omit(Omission::UnreadablePart {
                    detail: format!("page {page_number} could not be read: {error}"),
                });
                continue;
            }
        };
        for (block_index, block) in page_blocks(&text).into_iter().enumerate() {
            builder.checkpoint().map_err(PdfError::from)?;
            if builder.is_saturated() {
                break;
            }
            let pushed = builder
                .push(UnitDraft {
                    kind: UnitKind::Paragraph,
                    section_path: Vec::new(),
                    text: block,
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
        if builder.visuals_enabled() {
            extract_page_images(builder, &document, page_id, page_number)?;
        }
    }
    if !extracted_any && !builder.has_visuals() {
        return Err(PdfError::NoTextLayer(
            "the PDF pages contain no extractable text layer; an OCR pack is required".to_owned(),
        ));
    }
    Ok(())
}

fn extract_page_images(
    builder: &mut DocumentBuilder<'_, '_>,
    document: &Document,
    page_id: lopdf::ObjectId,
    page_number: u32,
) -> Result<(), PdfError> {
    match document.get_page_images(page_id) {
        Ok(images) => {
            for (image_index, image) in images.into_iter().enumerate() {
                let provenance = VisualProvenance::PdfImage {
                    page_number,
                    image_index: image_index as u32,
                };
                if image.filters.as_deref() != Some(&["DCTDecode".to_owned()]) {
                    builder.omit_visual(
                        Some(provenance),
                        "PDF image encoding is not a reusable JPEG raster",
                    );
                    continue;
                }
                builder
                    .push_visual(VisualDraft {
                        media_type: "image/jpeg",
                        data: image.content.to_vec(),
                        provenance,
                        caption: None,
                    })
                    .map_err(PdfError::from)?;
            }
        }
        Err(error) => builder.omit_visual(
            Some(VisualProvenance::PdfImage {
                page_number,
                image_index: 0,
            }),
            format!("PDF page images could not be inspected: {error}"),
        ),
    }
    Ok(())
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
    fn reusable_pdf_jpeg_images_are_retained_with_page_provenance() {
        let bytes = text_pdf(&[&["Visible text"]]);
        let mut document = Document::load_mem(&bytes).expect("load fixture");
        let page_id = document.get_pages()[&1];
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
        document
            .add_xobject(page_id, b"Im1", image_id)
            .expect("attach image");
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
}
