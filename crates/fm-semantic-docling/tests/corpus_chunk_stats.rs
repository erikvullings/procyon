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

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Writes the embedding input of every chunk, per candidate maximum, as JSON lines
/// (`<out>/<max>/<pdf stem>.jsonl`) for `scripts/eval/retrieval`.
#[test]
#[ignore = "needs PROCYON_DUMP_DIR (a folder of PDFs) and PROCYON_CHUNK_OUT"]
fn dump_chunks() {
    let directory = PathBuf::from(std::env::var("PROCYON_DUMP_DIR").expect("PROCYON_DUMP_DIR"));
    let out = PathBuf::from(std::env::var("PROCYON_CHUNK_OUT").expect("PROCYON_CHUNK_OUT"));
    let maxima: Vec<u32> = std::env::var("PROCYON_CHUNK_MAXIMA")
        .unwrap_or_else(|_| "256,384,512,768,1000".to_owned())
        .split(',')
        .map(|value| value.trim().parse().expect("numeric maximum"))
        .collect();
    let mut files: Vec<PathBuf> = fs::read_dir(&directory)
        .expect("readable directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "pdf"))
        .collect();
    files.sort();
    let converter = converter_with_baseline_fallback();
    for &maximum in &maxima {
        fs::create_dir_all(out.join(maximum.to_string())).expect("create output directory");
    }
    for path in files {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("utf-8 file name")
            .to_owned();
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
            continue;
        };
        for &maximum in &maxima {
            let chunker = chunker(maximum * 25 / 32, maximum, maximum * 3 / 32);
            let mut lines = String::new();
            for chunk in chunker.chunk(document) {
                lines.push_str(&format!(
                    "{{\"doc\":{},\"index\":{},\"tokens\":{},\"navigation\":{},\"text\":{}}}\n",
                    json_string(&stem),
                    chunk.index,
                    chunk.estimated_tokens,
                    chunk.is_navigation,
                    json_string(&chunk.embedding_input),
                ));
            }
            fs::write(
                out.join(maximum.to_string()).join(format!("{stem}.jsonl")),
                lines,
            )
            .expect("write chunks");
        }
    }
}
