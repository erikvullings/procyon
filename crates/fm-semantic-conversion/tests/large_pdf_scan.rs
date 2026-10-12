//! Synthetic-only probe for one-page PDF visual extraction before worker integration.

use fm_semantic_conversion::{
    BaselineConverter, ConversionBudgets, ConversionContext, ConversionOutcome, DocumentConverter,
    DocumentMetadata, MAX_VISUAL_BYTES, MAX_VISUAL_PIXELS, PdfPageWindow, SourceContent,
    VisualProvenance,
};

#[test]
#[ignore = "requires a generated synthetic 354-page scanned PDF"]
fn every_synthetic_scan_page_has_bounded_embedded_image_evidence() {
    let path = std::env::var_os("PROCYON_SYNTHETIC_SCAN_PDF")
        .expect("set PROCYON_SYNTHETIC_SCAN_PDF to the generated scanned fixture");
    let source = std::fs::read(path).expect("read synthetic fixture");
    assert!((174_000_000..=200 * 1024 * 1024).contains(&source.len()));
    let metadata = DocumentMetadata::unknown().with_media_type("application/pdf");
    let budgets = ConversionBudgets {
        max_source_bytes: 200 * 1024 * 1024,
        ..ConversionBudgets::default()
    };
    let without_images = BaselineConverter::new()
        .convert(
            SourceContent::Bytes(&source),
            &metadata,
            &ConversionContext::new()
                .with_budgets(budgets.clone())
                .with_pdf_page_window(PdfPageWindow::new(1, 1).unwrap()),
        )
        .expect("image-disabled page conversion");
    assert!(matches!(
        without_images,
        ConversionOutcome::NoTextLayer { .. }
    ));
    let mut fingerprints = std::collections::HashSet::new();
    for page in 1..=354 {
        let window = PdfPageWindow::new(page, page).unwrap();
        let outcome = BaselineConverter::new()
            .convert(
                SourceContent::Bytes(&source),
                &metadata,
                &ConversionContext::new()
                    .with_budgets(budgets.clone())
                    .with_pdf_page_window(window)
                    .with_visual_evidence(true),
            )
            .expect("page conversion");
        let ConversionOutcome::Converted(document) = outcome else {
            panic!("page {page} could not be extracted: {outcome:?}");
        };
        assert!(
            document.is_partial(),
            "page {page} cannot be published alone"
        );
        assert!(
            document.units().is_empty(),
            "scanned page has no text layer"
        );
        assert_eq!(document.visuals().len(), 1, "page {page}");
        assert!(document.visual_omissions().is_empty(), "page {page}");
        let image = &document.visuals()[0];
        assert_eq!(
            image.provenance,
            VisualProvenance::PdfImage {
                page_number: page,
                image_index: 0,
            }
        );
        assert!(image.data.len() <= MAX_VISUAL_BYTES, "page {page}");
        assert!(u64::from(image.width) * u64::from(image.height) <= MAX_VISUAL_PIXELS);
        assert!(
            fingerprints.insert(image.id.clone()),
            "duplicate page {page}"
        );
    }
    assert_eq!(fingerprints.len(), 354);
}
