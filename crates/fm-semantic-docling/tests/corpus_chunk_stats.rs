//! Reports chunk-count and size statistics for candidate chunker options over a PDF folder.
//! Ignored by default: it needs real PDFs.
//!
//! ```text
//! PROCYON_DUMP_DIR=~/Downloads/corpus \
//!   cargo test --release -p fm-semantic-docling --test corpus_chunk_stats -- --ignored --nocapture
//! ```

use std::fs;
use std::path::PathBuf;

use fm_semantic_conversion::{
    Chunker, ChunkerOptions, ConversionBudgets, ConversionContext, DocumentConverter,
    DocumentMetadata, SourceContent,
};
use fm_semantic_docling::converter_with_baseline_fallback;

#[derive(Default)]
struct Stats {
    units: usize,
    chunks: usize,
    split_chunks: usize,
    under_256: usize,
    sizes: Vec<u32>,
}

fn chunker(target: u32, max: u32, overlap: u32) -> Chunker {
    Chunker::new(ChunkerOptions {
        target_tokens: target,
        max_tokens: max,
        overlap_tokens: overlap,
        max_excerpt_chars: 400,
    })
    .expect("valid options")
}

#[test]
#[ignore = "needs PROCYON_DUMP_DIR (a folder of PDFs)"]
fn chunk_statistics() {
    let directory = PathBuf::from(std::env::var("PROCYON_DUMP_DIR").expect("PROCYON_DUMP_DIR"));
    let candidates = [
        ("400/512", chunker(400, 512, 48)),
        ("781/1000", chunker(781, 1000, 93)),
    ];
    let mut totals: Vec<Stats> = candidates.iter().map(|_| Stats::default()).collect();
    let mut files: Vec<PathBuf> = fs::read_dir(&directory)
        .expect("readable directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "pdf"))
        .collect();
    files.sort();
    let converter = converter_with_baseline_fallback();
    for path in files {
        let bytes = fs::read(&path).expect("read PDF");
        let context = ConversionContext::new().with_budgets(ConversionBudgets {
            timeout: std::time::Duration::from_secs(1_800),
            ..ConversionBudgets::default()
        });
        let outcome = converter
            .convert(
                SourceContent::Bytes(&bytes),
                &DocumentMetadata::unknown().with_extension("pdf"),
                &context,
            )
            .expect("conversion");
        let Some(document) = outcome.document() else {
            eprintln!("{}: no document", path.display());
            continue;
        };
        for ((_, chunker), total) in candidates.iter().zip(&mut totals) {
            let chunks = chunker.chunk(document);
            total.units += document.units().len();
            total.chunks += chunks.len();
            total.split_chunks += chunks.iter().filter(|chunk| chunk.part.is_some()).count();
            for chunk in &chunks {
                total.under_256 += usize::from(chunk.estimated_tokens < 256);
                total.sizes.push(chunk.estimated_tokens);
            }
        }
    }
    for ((label, _), total) in candidates.iter().zip(&mut totals) {
        total.sizes.sort_unstable();
        let at = |q: usize| {
            total
                .sizes
                .get(total.sizes.len() * q / 100)
                .copied()
                .unwrap_or(0)
        };
        println!(
            "{label}: units={} chunks={} split-part-chunks={} (<256 est: {}) p10={} p50={} p90={} max={}",
            total.units,
            total.chunks,
            total.split_chunks,
            total.under_256,
            at(10),
            at(50),
            at(90),
            total.sizes.last().copied().unwrap_or(0)
        );
    }
}
