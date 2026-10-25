//! Public-interface regressions for streamed JSON Lines conversion.

use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use fm_semantic_conversion::{
    BaselineConverter, ChunkProvenance, Chunker, ConversionBudgets, ConversionContext,
    ConversionOutcome, ConversionWarning, DocumentConverter, DocumentMetadata, FormatKind,
    Omission, OptionalConverter, Provenance, SkippedRecordReason, SourceContent,
};

fn metadata(extension: &str) -> DocumentMetadata {
    DocumentMetadata::unknown().with_extension(extension)
}

fn feed(records: usize) -> String {
    (1..=records)
        .map(|index| {
            format!(
                "{{\"title\":\"Story {index}\",\"text\":\"Body of story number {index} about floods and dams.\",\"channel\":\"News\",\"published\":\"2025-01-{:02}\"}}\n",
                (index % 28) + 1
            )
        })
        .collect()
}

/// A reader that serves a deterministic feed of unbounded length and counts
/// how much of it was consumed, so memory/IO boundedness is observable.
struct EndlessFeed {
    pending: Vec<u8>,
    next_record: u64,
    served: Arc<AtomicU64>,
}

impl Read for EndlessFeed {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.pending.is_empty() {
            self.next_record += 1;
            self.pending = format!(
                "{{\"text\":\"endless record {}\",\"image\":\"{}\"}}\n",
                self.next_record,
                "QUJD".repeat(5_000)
            )
            .into_bytes();
        }
        let count = buffer.len().min(self.pending.len());
        buffer[..count].copy_from_slice(&self.pending[..count]);
        self.pending.drain(..count);
        self.served.fetch_add(count as u64, Ordering::Relaxed);
        Ok(count)
    }
}

fn convert(
    content: SourceContent<'_>,
    extension: &str,
    context: &ConversionContext,
) -> ConversionOutcome {
    BaselineConverter::new()
        .convert(content, &metadata(extension), context)
        .expect("conversion")
}

#[test]
fn a_reader_larger_than_the_source_budget_is_still_converted() {
    let source = feed(200);
    let context = ConversionContext::new().with_budgets(ConversionBudgets {
        max_source_bytes: 1_024,
        ..ConversionBudgets::default()
    });
    assert!(source.len() > 1_024);

    let mut reader = io::Cursor::new(source.clone().into_bytes());
    let from_reader = convert(SourceContent::Reader(&mut reader), "jsonl", &context);
    let from_bytes = convert(SourceContent::Bytes(source.as_bytes()), "ndjson", &context);

    for outcome in [&from_reader, &from_bytes] {
        let document = outcome.document().expect("converted");
        assert_eq!(document.format(), FormatKind::Jsonl);
        assert_eq!(document.units().len(), 200);
        assert!(!document.is_partial());
    }
    // A non-JSONL source of the same size is still rejected by the budget.
    assert!(matches!(
        convert(SourceContent::Bytes(source.as_bytes()), "txt", &context),
        ConversionOutcome::OverBudget { .. }
    ));
}

#[test]
fn the_optional_converter_chain_streams_json_lines_past_the_source_budget() {
    let converter = OptionalConverter::prefer_advanced(Arc::new(BaselineConverter::new()), None);
    let source = feed(100);
    let context = ConversionContext::new().with_budgets(ConversionBudgets {
        max_source_bytes: 512,
        ..ConversionBudgets::default()
    });
    let mut reader = io::Cursor::new(source.into_bytes());
    let outcome = converter
        .convert(
            SourceContent::Reader(&mut reader),
            &metadata("jsonl"),
            &context,
        )
        .expect("conversion");
    assert_eq!(outcome.document().expect("converted").units().len(), 100);
}

#[test]
fn the_media_type_alone_selects_the_json_lines_converter() {
    let outcome = BaselineConverter::new()
        .convert(
            SourceContent::Bytes(b"{\"text\":\"hello\"}\n"),
            &DocumentMetadata::unknown().with_media_type("application/x-ndjson"),
            &ConversionContext::new(),
        )
        .expect("conversion");
    assert_eq!(
        outcome.document().expect("converted").format(),
        FormatKind::Jsonl
    );
}

#[test]
fn an_endless_stream_stops_at_the_output_budget_with_bounded_reads() {
    let served = Arc::new(AtomicU64::new(0));
    let mut reader = EndlessFeed {
        pending: Vec::new(),
        next_record: 0,
        served: Arc::clone(&served),
    };
    let context = ConversionContext::new().with_budgets(ConversionBudgets {
        max_units: 50,
        ..ConversionBudgets::default()
    });
    let outcome = convert(SourceContent::Reader(&mut reader), "jsonl", &context);
    let document = outcome.document().expect("converted");
    assert_eq!(document.units().len(), 50);
    assert!(document.is_partial());
    assert!(
        document
            .omissions()
            .contains(&Omission::UnitLimit { limit: 50 })
    );
    // The 20 KB base64 image of each record is never part of the output.
    assert!(
        document
            .units()
            .iter()
            .all(|unit| !unit.text.contains("QUJD"))
    );
    // 51 records of ~20 KB plus at most one read window; nowhere near "all".
    assert!(served.load(Ordering::Relaxed) < 3 * 1024 * 1024);
}

#[test]
fn chunks_cite_the_record_line_and_exclude_metadata_from_their_body() {
    let source = "{\"title\":\"Dam breach\",\"text\":\"Water rose overnight.\",\"channel\":\"News24\",\"published\":\"2025-03-01\"}\n\
                  {\"title\":\"Harvest\",\"text\":\"Wheat prices fell.\",\"channel\":\"Farm\",\"published\":\"2024-11-30\"}\n";
    let outcome = convert(
        SourceContent::Bytes(source.as_bytes()),
        "jsonl",
        &ConversionContext::new(),
    );
    let document = outcome.document().expect("converted");
    let chunks = Chunker::default().chunk(document);
    assert_eq!(chunks.len(), 2);
    assert_eq!(
        chunks[1].provenance,
        ChunkProvenance::Exact(Provenance::JsonlRecord {
            line: 2,
            byte_offset: source.lines().next().expect("first line").len() as u64 + 1
        })
    );
    assert_eq!(
        chunks[0].section_path,
        ["Dam breach", "News24 \u{b7} 2025-03-01"]
    );
    assert!(chunks[0].embedding_input.contains("Dam breach"));
    assert!(chunks[0].embedding_input.contains("Water rose overnight."));
    // The metadata appears exactly once: as the context prefix, not in the body.
    assert_eq!(chunks[0].embedding_input.matches("News24").count(), 1);
    assert_ne!(chunks[0].fingerprint, chunks[1].fingerprint);

    // Same content converted again, from a different "location", is identical.
    let again = convert(
        SourceContent::Bytes(source.as_bytes()),
        "ndjson",
        &ConversionContext::new(),
    );
    let again_chunks = Chunker::default().chunk(again.document().expect("converted"));
    assert_eq!(
        chunks
            .iter()
            .map(|chunk| &chunk.fingerprint)
            .collect::<Vec<_>>(),
        again_chunks
            .iter()
            .map(|chunk| &chunk.fingerprint)
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_file_of_only_malformed_lines_is_reported_not_silently_empty() {
    let outcome = convert(
        SourceContent::Bytes(b"nope\n[1]\n"),
        "jsonl",
        &ConversionContext::new(),
    );
    match outcome {
        ConversionOutcome::Malformed { detail } => {
            assert!(detail.contains("first at line 1"), "{detail}");
        }
        other => panic!("expected Malformed, got {other:?}"),
    }
}

#[test]
fn malformed_lines_between_good_ones_do_not_fail_the_document() {
    let source = "{\"text\":\"one\"}\n{broken\n\n{\"text\":\"two\"}";
    let outcome = convert(
        SourceContent::Bytes(source.as_bytes()),
        "jsonl",
        &ConversionContext::new(),
    );
    let document = outcome.document().expect("converted");
    assert_eq!(document.units().len(), 2);
    assert!(
        document
            .warnings()
            .contains(&ConversionWarning::SkippedRecord {
                line: 2,
                reason: SkippedRecordReason::InvalidJson
            })
    );
    assert_eq!(
        document.units()[1].provenance,
        Provenance::JsonlRecord {
            line: 4,
            byte_offset: 24
        }
    );
}

#[test]
fn empty_and_binary_sources_have_distinct_outcomes() {
    let empty = convert(
        SourceContent::Bytes(b""),
        "jsonl",
        &ConversionContext::new(),
    );
    assert!(matches!(empty, ConversionOutcome::Skipped { .. }));
    let binary = convert(
        SourceContent::Bytes(b"\x00\x01\x02\x03"),
        "jsonl",
        &ConversionContext::new(),
    );
    assert!(matches!(binary, ConversionOutcome::Unsupported { .. }));
}
