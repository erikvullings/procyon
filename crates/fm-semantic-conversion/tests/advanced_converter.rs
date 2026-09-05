//! Optional advanced-converter contract tests.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use fm_semantic_conversion::{
    AdvancedCapability, AdvancedConversion, AdvancedConverterAdapter, AdvancedConverterBackend,
    BaselineConverter, CancellationFlag, ComponentVersion, ConversionBudgets, ConversionContext,
    ConversionError, ConversionOutcome, ConvertedDocument, DocumentConverter, DocumentMetadata,
    FormatKind, ManualClock, OptionalConverter, Provenance, ProvenancePrecision, SourceContent,
    SourceMap, StructuralUnit, TopLevelBoundary, UnitKind,
};

struct OcrBackend {
    malformed: bool,
    clock: Option<Arc<ManualClock>>,
}

impl AdvancedConverterBackend for OcrBackend {
    fn version(&self) -> ComponentVersion {
        ComponentVersion::new("fixture-ocr", 1)
    }

    fn capabilities(&self) -> &[AdvancedCapability] {
        &[AdvancedCapability::Ocr, AdvancedCapability::ComplexLayout]
    }

    fn convert_bytes(
        &self,
        _bytes: &[u8],
        _metadata: &DocumentMetadata,
        _context: &ConversionContext,
    ) -> Result<AdvancedConversion, ConversionError> {
        if let Some(clock) = &self.clock {
            clock.advance(Duration::from_secs(2));
        }
        let units = (!self.malformed)
            .then(|| StructuralUnit {
                order: 0,
                kind: UnitKind::Paragraph,
                format: FormatKind::Pdf,
                section_path: vec!["Scanned page".into()],
                text: "Recognized local text".into(),
                provenance: Provenance::PdfBlock {
                    page_number: 1,
                    block_index: 0,
                },
                boundary: TopLevelBoundary::Page(1),
                source_map: SourceMap::default(),
                truncated: false,
            })
            .into_iter()
            .collect();
        Ok(AdvancedConversion {
            outcome: ConversionOutcome::Converted(ConvertedDocument::new(
                self.version(),
                FormatKind::Pdf,
                units,
                Vec::new(),
                Vec::new(),
            )),
            provenance_precision: ProvenancePrecision::Exact,
        })
    }
}

#[test]
fn optional_converter_handles_baseline_gap_without_paths_or_network() {
    let converter = OptionalConverter::new(
        Arc::new(BaselineConverter::new()),
        Some(Arc::new(AdvancedConverterAdapter::new(Arc::new(
            OcrBackend {
                malformed: false,
                clock: None,
            },
        )))),
    );
    let metadata = DocumentMetadata::unknown().with_media_type("image/tiff");
    let outcome = converter
        .convert(
            SourceContent::Bytes(b"II*\0scanned fixture"),
            &metadata,
            &ConversionContext::new(),
        )
        .expect("conversion");
    let document = outcome.document().expect("advanced document");
    assert_eq!(
        document.converter(),
        ComponentVersion::new("fixture-ocr", 1)
    );
    assert_eq!(document.units()[0].text, "Recognized local text");
}

#[test]
fn removing_advanced_pack_keeps_baseline_documents_readable() {
    let converter = OptionalConverter::new(Arc::new(BaselineConverter::new()), None);
    let baseline = converter
        .convert(
            SourceContent::Bytes(b"plain text remains readable"),
            &DocumentMetadata::unknown().with_media_type("text/plain"),
            &ConversionContext::new(),
        )
        .expect("baseline conversion");
    assert!(matches!(baseline, ConversionOutcome::Converted(_)));

    let unsupported = converter
        .convert(
            SourceContent::Bytes(b"II*\0scanned fixture"),
            &DocumentMetadata::unknown().with_media_type("image/tiff"),
            &ConversionContext::new(),
        )
        .expect("unsupported outcome");
    assert!(matches!(unsupported, ConversionOutcome::Unsupported { .. }));
}

#[test]
fn malformed_or_over_budget_advanced_output_is_rejected() {
    let advanced = AdvancedConverterAdapter::new(Arc::new(OcrBackend {
        malformed: true,
        clock: None,
    }));
    let malformed = advanced
        .convert_with_report(
            SourceContent::Bytes(b"scan"),
            &DocumentMetadata::unknown().with_media_type("image/tiff"),
            &ConversionContext::new(),
        )
        .expect("typed outcome");
    assert!(matches!(
        malformed.outcome,
        ConversionOutcome::Malformed { .. }
    ));

    let context = ConversionContext::new().with_budgets(ConversionBudgets {
        max_source_bytes: 3,
        ..ConversionBudgets::default()
    });
    let over_budget = advanced
        .convert_with_report(
            SourceContent::Bytes(b"scan"),
            &DocumentMetadata::unknown(),
            &context,
        )
        .expect("typed outcome");
    assert!(matches!(
        over_budget.outcome,
        ConversionOutcome::OverBudget { .. }
    ));
}

#[test]
fn advanced_converter_bounds_readers_and_observes_cancellation_and_timeout() {
    let advanced = AdvancedConverterAdapter::new(Arc::new(OcrBackend {
        malformed: false,
        clock: None,
    }));
    let mut reader = Cursor::new(vec![0_u8; 5]);
    let bounded = ConversionContext::new().with_budgets(ConversionBudgets {
        max_source_bytes: 4,
        ..ConversionBudgets::default()
    });
    assert!(matches!(
        advanced
            .convert_with_report(
                SourceContent::Reader(&mut reader),
                &DocumentMetadata::unknown(),
                &bounded,
            )
            .expect("bounded reader"),
        AdvancedConversion {
            outcome: ConversionOutcome::OverBudget { .. },
            ..
        }
    ));

    let cancellation = CancellationFlag::new();
    cancellation.cancel();
    let cancelled = ConversionContext::new().with_cancellation(cancellation.handle());
    assert!(matches!(
        advanced
            .convert_with_report(
                SourceContent::Bytes(b"scan"),
                &DocumentMetadata::unknown(),
                &cancelled,
            )
            .expect("cancelled conversion"),
        AdvancedConversion {
            outcome: ConversionOutcome::Cancelled,
            ..
        }
    ));

    let clock = Arc::new(ManualClock::new());
    let slow = AdvancedConverterAdapter::new(Arc::new(OcrBackend {
        malformed: false,
        clock: Some(Arc::clone(&clock)),
    }));
    let timeout = ConversionContext::new()
        .with_clock(clock)
        .with_budgets(ConversionBudgets {
            timeout: Duration::from_secs(1),
            ..ConversionBudgets::default()
        });
    assert!(matches!(
        slow.convert_with_report(
            SourceContent::Bytes(b"scan"),
            &DocumentMetadata::unknown(),
            &timeout,
        )
        .expect("timed conversion"),
        AdvancedConversion {
            outcome: ConversionOutcome::OverBudget { .. },
            ..
        }
    ));
}
