//! Public-Interface tests for the optional Docling PDF Adapter.

use fm_semantic_conversion::{
    BaselineConverter, CancellationFlag, ConversionBudgets, ConversionContext, ConversionOutcome,
    DocumentConverter, DocumentMetadata, OptionalConverter, Provenance, SourceContent,
    TopLevelBoundary,
};
use fm_semantic_docling::{
    DOCLING_PDF_CONVERTER_VERSION, OCRMYPDF_CONVERTER_VERSION, OcrMyPdfAvailability,
    OcrMyPdfConfiguration, OcrMyPdfConverter, converter_with_baseline_fallback,
};
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
    converter_with_baseline_fallback()
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
fn image_only_pdf_is_excluded_with_actionable_cross_platform_ocr_guidance() {
    let bytes = text_pdf(&[&[]]);

    let outcome = converter()
        .convert(
            SourceContent::Bytes(&bytes),
            &DocumentMetadata::unknown().with_extension("pdf"),
            &ConversionContext::new(),
        )
        .expect("typed outcome");

    let ConversionOutcome::NoTextLayer { detail } = outcome else {
        panic!("expected an OCR-required outcome");
    };
    assert!(detail.contains("OCRmyPDF"));
    assert!(detail.contains("Homebrew"));
    assert!(detail.contains("Linux"));
    assert!(detail.contains("WSL"));
}

#[test]
fn page_number_only_pdf_is_not_reintroduced_by_the_baseline_fallback() {
    let bytes = text_pdf(&[&["1"]]);

    let outcome = converter()
        .convert(
            SourceContent::Bytes(&bytes),
            &DocumentMetadata::unknown().with_extension("pdf"),
            &ConversionContext::new(),
        )
        .expect("typed outcome");

    assert!(matches!(outcome, ConversionOutcome::NoTextLayer { .. }));
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

#[cfg(unix)]
mod ocrmypdf {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::*;

    fn fake_executable(root: &Path, body: &str, fixture: Option<&[u8]>) -> PathBuf {
        if let Some(fixture) = fixture {
            fs::write(root.join("fixture.pdf"), fixture).expect("write OCR fixture");
        }
        let executable = root.join("fake-ocrmypdf");
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\nwhile [ \"$#\" -gt 2 ]; do shift; done\ninput=\"$1\"\noutput=\"$2\"\n{body}\n"
            ),
        )
        .expect("write fake OCRmyPDF");
        let mut permissions = fs::metadata(&executable)
            .expect("fake metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&executable, permissions).expect("make fake executable");
        executable
    }

    fn ocr_converter(configuration: OcrMyPdfConfiguration) -> OcrMyPdfConverter {
        OcrMyPdfConverter::new(Arc::new(converter_with_baseline_fallback()), configuration)
    }

    fn no_text_pdf() -> Vec<u8> {
        text_pdf(&[&[]])
    }

    fn convert(
        converter: &dyn DocumentConverter,
        bytes: &[u8],
        context: &ConversionContext,
    ) -> ConversionOutcome {
        converter
            .convert(
                SourceContent::Bytes(bytes),
                &DocumentMetadata::unknown().with_media_type("application/pdf"),
                context,
            )
            .expect("typed conversion")
    }

    fn assert_temporary_root_is_empty(root: &Path) {
        assert_eq!(
            fs::read_dir(root).expect("read temporary root").count(),
            0,
            "OCR input and output must be removed"
        );
    }

    #[test]
    fn detects_an_installed_executable_and_reports_absence() {
        let directory = tempfile::tempdir().expect("tempdir");
        let executable = fake_executable(directory.path(), "exit 0", None);
        assert_eq!(
            OcrMyPdfAvailability::detect(Some(&executable)),
            OcrMyPdfAvailability::Available { executable }
        );

        let OcrMyPdfAvailability::Unavailable { guidance } =
            OcrMyPdfAvailability::detect(Some(&directory.path().join("missing")))
        else {
            panic!("missing executable should be unavailable");
        };
        assert!(guidance.contains("Homebrew"));
        assert!(guidance.contains("Linux"));
        assert!(guidance.contains("WSL"));
    }

    #[test]
    fn successful_ocr_is_reconverted_by_docling_and_discloses_provenance() {
        let directory = tempfile::tempdir().expect("tempdir");
        let temporary_root = tempfile::tempdir().expect("OCR temporary root");
        let searchable = text_pdf(&[&[
            "Recognized evidence from OCR contains enough meaningful prose.",
            "The second sentence confirms that deterministic conversion sees a real text layer.",
        ]]);
        let executable = fake_executable(
            directory.path(),
            "cp \"$(dirname \"$0\")/fixture.pdf\" \"$output\"",
            Some(&searchable),
        );
        let converter = ocr_converter(
            OcrMyPdfConfiguration::new(executable).with_temporary_root(temporary_root.path()),
        );

        let outcome = convert(&converter, &no_text_pdf(), &ConversionContext::new());
        let document = outcome.document().expect("OCR-converted document");
        assert_eq!(document.converter(), OCRMYPDF_CONVERTER_VERSION);
        assert!(document.units()[0].text.contains("Recognized evidence"));
        assert!(document.warnings().iter().any(|warning| matches!(
            warning,
            fm_semantic_conversion::ConversionWarning::OcrAssessment { .. }
        )));
        assert_temporary_root_is_empty(temporary_root.path());
    }

    #[test]
    fn ordinary_searchable_pdf_never_launches_ocr() {
        let directory = tempfile::tempdir().expect("tempdir");
        let marker = directory.path().join("launched");
        let executable = fake_executable(
            directory.path(),
            "touch \"$(dirname \"$0\")/launched\"; exit 9",
            None,
        );
        let converter = ocr_converter(OcrMyPdfConfiguration::new(executable));

        let outcome = convert(
            &converter,
            &text_pdf(&[&[
                "This existing searchable layer contains enough meaningful prose.",
                "OCR must not run when deterministic Docling can already read this document.",
            ]]),
            &ConversionContext::new(),
        );
        assert!(outcome.document().is_some());
        assert!(!marker.exists());
    }

    #[test]
    fn absent_and_failed_executables_keep_actionable_guidance_and_cleanup() {
        let directory = tempfile::tempdir().expect("tempdir");
        let temporary_root = tempfile::tempdir().expect("OCR temporary root");
        let absent = ocr_converter(
            OcrMyPdfConfiguration::new(directory.path().join("missing"))
                .with_temporary_root(temporary_root.path()),
        );
        let ConversionOutcome::NoTextLayer { detail } =
            convert(&absent, &no_text_pdf(), &ConversionContext::new())
        else {
            panic!("absent executable should preserve OCR-required outcome");
        };
        assert!(detail.contains("could not be started"));
        assert!(detail.contains("OCRmyPDF"));
        assert_temporary_root_is_empty(temporary_root.path());

        let failed_executable = fake_executable(directory.path(), "exit 7", None);
        let failed = ocr_converter(
            OcrMyPdfConfiguration::new(failed_executable)
                .with_temporary_root(temporary_root.path()),
        );
        let ConversionOutcome::NoTextLayer { detail } =
            convert(&failed, &no_text_pdf(), &ConversionContext::new())
        else {
            panic!("failed executable should preserve OCR-required outcome");
        };
        assert!(detail.contains("exited unsuccessfully"));
        assert_temporary_root_is_empty(temporary_root.path());
    }

    #[test]
    fn timeout_terminates_the_child_and_cleans_temporary_files() {
        let directory = tempfile::tempdir().expect("tempdir");
        let temporary_root = tempfile::tempdir().expect("OCR temporary root");
        let executable = fake_executable(directory.path(), "sleep 5", None);
        let converter = ocr_converter(
            OcrMyPdfConfiguration::new(executable)
                .with_timeout(Duration::from_millis(75))
                .with_temporary_root(temporary_root.path()),
        );
        let started = Instant::now();

        let outcome = convert(&converter, &no_text_pdf(), &ConversionContext::new());
        let ConversionOutcome::NoTextLayer { detail } = outcome else {
            panic!("timeout should remain an actionable OCR-required outcome");
        };
        assert!(detail.contains("local deadline"));
        assert!(detail.contains("OCRmyPDF"));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_temporary_root_is_empty(temporary_root.path());
    }

    #[test]
    fn cancellation_terminates_the_child_and_cleans_temporary_files() {
        let directory = tempfile::tempdir().expect("tempdir");
        let temporary_root = tempfile::tempdir().expect("OCR temporary root");
        let executable = fake_executable(directory.path(), "sleep 5", None);
        let converter = ocr_converter(
            OcrMyPdfConfiguration::new(executable).with_temporary_root(temporary_root.path()),
        );
        let flag = CancellationFlag::new();
        let cancellation = flag.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(75));
            cancellation.cancel();
        });
        let context = ConversionContext::new().with_cancellation(flag.handle());

        let outcome = convert(&converter, &no_text_pdf(), &context);
        canceller.join().expect("canceller");
        assert!(matches!(outcome, ConversionOutcome::Cancelled));
        assert_temporary_root_is_empty(temporary_root.path());
    }

    #[test]
    fn oversized_ocr_output_is_rejected_before_it_is_read() {
        let directory = tempfile::tempdir().expect("tempdir");
        let temporary_root = tempfile::tempdir().expect("OCR temporary root");
        let searchable = text_pdf(&[&[
            "Recognized output is deliberately over the test byte limit.",
            "This additional sentence ensures the fixture has a valid searchable text layer.",
        ]]);
        let executable = fake_executable(
            directory.path(),
            "cp \"$(dirname \"$0\")/fixture.pdf\" \"$output\"",
            Some(&searchable),
        );
        let converter = ocr_converter(
            OcrMyPdfConfiguration::new(executable).with_temporary_root(temporary_root.path()),
        );
        let source = no_text_pdf();
        let context = ConversionContext::new().with_budgets(ConversionBudgets {
            max_source_bytes: source.len() as u64,
            ..ConversionBudgets::default()
        });

        let outcome = convert(&converter, &source, &context);
        let ConversionOutcome::NoTextLayer { detail } = outcome else {
            panic!("oversized OCR output should remain an actionable skip");
        };
        assert!(detail.contains("exceeded"));
        assert!(detail.contains("OCRmyPDF"));
        assert_temporary_root_is_empty(temporary_root.path());
    }

    #[test]
    fn timeout_terminates_ocr_descendants() {
        let directory = tempfile::tempdir().expect("tempdir");
        let temporary_root = tempfile::tempdir().expect("OCR temporary root");
        let executable = fake_executable(
            directory.path(),
            "(sleep 1; touch \"${0%/*}/descendant-survived\") &\n\
             sleep 5",
            None,
        );
        let converter = ocr_converter(
            OcrMyPdfConfiguration::new(executable)
                .with_timeout(Duration::from_millis(250))
                .with_temporary_root(temporary_root.path()),
        );

        let outcome = convert(&converter, &no_text_pdf(), &ConversionContext::new());
        assert!(matches!(outcome, ConversionOutcome::NoTextLayer { .. }));
        std::thread::sleep(Duration::from_millis(1_100));
        assert!(!directory.path().join("descendant-survived").exists());
        assert_temporary_root_is_empty(temporary_root.path());
    }

    #[test]
    fn ocr_child_receives_only_the_bounded_environment() {
        let directory = tempfile::tempdir().expect("tempdir");
        let executable = fake_executable(
            directory.path(),
            "set > \"${0%/*}/environment\"; exit 7",
            None,
        );
        let converter = ocr_converter(OcrMyPdfConfiguration::new(executable));

        let outcome = convert(&converter, &no_text_pdf(), &ConversionContext::new());
        assert!(matches!(outcome, ConversionOutcome::NoTextLayer { .. }));
        let environment =
            fs::read_to_string(directory.path().join("environment")).expect("captured environment");
        assert!(!environment.contains("HOME="));
        assert!(environment.contains("TMPDIR="));
    }

    #[test]
    #[ignore = "requires PROCYON_OCR_SMOKE_PDF and a local OCRmyPDF installation"]
    fn real_ocrmypdf_smoke_test_preserves_the_source_and_produces_text() {
        let source_path = std::env::var_os("PROCYON_OCR_SMOKE_PDF").expect("PROCYON_OCR_SMOKE_PDF");
        let source = fs::read(&source_path).expect("read smoke PDF");
        let OcrMyPdfAvailability::Available { executable } = OcrMyPdfAvailability::detect(None)
        else {
            panic!("OCRmyPDF is not installed");
        };
        let converter = ocr_converter(OcrMyPdfConfiguration::new(executable));
        let context = ConversionContext::new().with_budgets(ConversionBudgets {
            timeout: Duration::from_secs(4 * 60),
            ..ConversionBudgets::default()
        });

        let outcome = convert(&converter, &source, &context);
        assert!(
            outcome.document().is_some(),
            "OCR did not produce searchable text: {outcome:?}"
        );
        assert_eq!(
            fs::read(source_path).expect("re-read smoke PDF"),
            source,
            "OCR must not modify the original source"
        );
    }
}
