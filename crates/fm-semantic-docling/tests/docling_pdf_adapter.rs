//! Public-Interface tests for the optional Docling PDF Adapter.

use std::sync::Arc;

use fm_semantic_conversion::{
    AdvancedConverterAdapter, BaselineConverter, ConversionBudgets, ConversionContext,
    ConversionOutcome, DocumentConverter, DocumentMetadata, OptionalConverter, Provenance,
    SourceContent, TopLevelBoundary,
};
use fm_semantic_docling::{DOCLING_PDF_CONVERTER_VERSION, DoclingPdfBackend};
use lopdf::{Document, Object, Stream, dictionary};

fn text_pdf(pages: &[&[&str]]) -> Vec<u8> {
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
    document.save_to(&mut bytes).expect("save fixture PDF");
    bytes
}

fn positioned_pdf(content: &str) -> Vec<u8> {
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
    let content_id = document.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
            "Resources" => resources_id,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("save positioned PDF");
    bytes
}

fn converter() -> OptionalConverter {
    OptionalConverter::prefer_advanced(
        Arc::new(BaselineConverter::new()),
        Some(Arc::new(AdvancedConverterAdapter::new(Arc::new(
            DoclingPdfBackend::new(),
        )))),
    )
}

#[test]
fn preferred_adapter_converts_a_text_layer_pdf_through_document_converter() {
    let bytes = text_pdf(&[&["First page evidence"], &["Second page evidence"]]);
    let outcome = converter()
        .convert(
            SourceContent::Bytes(&bytes),
            &DocumentMetadata::unknown().with_media_type("application/pdf"),
            &ConversionContext::new(),
        )
        .expect("conversion");
    let document = outcome.document().expect("converted PDF");

    assert_eq!(document.converter(), DOCLING_PDF_CONVERTER_VERSION);
    assert_eq!(document.units().len(), 2);
    assert!(document.units()[0].text.contains("First page evidence"));
    assert_eq!(
        document.units()[1].provenance,
        Provenance::PdfBlock {
            page_number: 2,
            block_index: 0,
        }
    );
    assert_eq!(document.units()[1].boundary, TopLevelBoundary::Page(2));
}

#[test]
fn page_limit_is_explicit_partial_output() {
    let bytes = text_pdf(&[
        &["Enough text to establish a real first-page text layer."],
        &["Enough text to establish a real second-page text layer."],
    ]);
    let context = ConversionContext::new().with_budgets(ConversionBudgets {
        max_items: 1,
        ..ConversionBudgets::default()
    });
    let outcome = converter()
        .convert(
            SourceContent::Bytes(&bytes),
            &DocumentMetadata::unknown().with_extension("pdf"),
            &context,
        )
        .expect("conversion");
    let document = outcome.document().expect("partial PDF");

    assert!(document.is_partial());
    assert_eq!(document.units().len(), 1);
    assert_eq!(
        document.omissions(),
        [fm_semantic_conversion::Omission::ItemsDropped {
            item: "page",
            converted: 1,
            total: 2,
        }]
    );
}

#[test]
fn malformed_input_is_typed_and_non_pdf_content_uses_the_baseline() {
    let malformed = converter()
        .convert(
            SourceContent::Bytes(b"%PDF-not-a-document"),
            &DocumentMetadata::unknown().with_extension("pdf"),
            &ConversionContext::new(),
        )
        .expect("typed outcome");
    assert!(matches!(malformed, ConversionOutcome::Malformed { .. }));

    let baseline = converter()
        .convert(
            SourceContent::Bytes(b"plain text remains available"),
            &DocumentMetadata::unknown().with_media_type("text/plain"),
            &ConversionContext::new(),
        )
        .expect("baseline fallback");
    assert_eq!(
        baseline.document().expect("baseline document").converter(),
        fm_semantic_conversion::BASELINE_CONVERTER_VERSION
    );
}

#[test]
fn deterministic_docling_orders_positioned_columns_instead_of_operator_order() {
    let bytes = positioned_pdf(
        "BT /F1 12 Tf\n\
         1 0 0 1 330 720 Tm (Right column starts after the left column.) Tj\n\
         1 0 0 1 72 720 Tm (Left column starts first in reading order.) Tj\n\
         1 0 0 1 330 690 Tm (Right column continues after left finishes.) Tj\n\
         1 0 0 1 72 690 Tm (Left column continues before the right column.) Tj\n\
         ET\n",
    );
    let baseline = BaselineConverter::new()
        .convert(
            SourceContent::Bytes(&bytes),
            &DocumentMetadata::unknown().with_extension("pdf"),
            &ConversionContext::new(),
        )
        .expect("baseline conversion")
        .document()
        .expect("baseline document")
        .units()
        .iter()
        .map(|unit| unit.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let advanced_outcome = converter()
        .convert(
            SourceContent::Bytes(&bytes),
            &DocumentMetadata::unknown().with_extension("pdf"),
            &ConversionContext::new(),
        )
        .expect("Docling conversion");
    let advanced = advanced_outcome
        .document()
        .expect("Docling document")
        .units()
        .iter()
        .map(|unit| unit.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(
        baseline
            .find("Right column starts")
            .expect("right baseline")
            < baseline.find("Left column starts").expect("left baseline")
    );
    assert!(
        advanced.find("Left column starts").expect("left Docling")
            < advanced.find("Right column starts").expect("right Docling")
    );
}
