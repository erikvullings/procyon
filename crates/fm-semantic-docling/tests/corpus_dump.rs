//! Dumps converted PDF units as JSON lines for the gold-standard scoring harness
//! (`scripts/eval/pdf-gold`). Ignored by default: it needs a folder of real PDFs.
//!
//! ```text
//! PROCYON_DUMP_DIR=~/Downloads/corpus PROCYON_DUMP_OUT=/tmp/dump \
//!   cargo test --release -p fm-semantic-docling --test corpus_dump -- --ignored
//! ```
//!
//! With the `ml` feature (and the verified model pack on the loader path) the `ml` dump runs too;
//! see `scripts/eval/pdf-gold/README.md`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use fm_semantic_conversion::{
    ConversionBudgets, ConversionContext, DocumentConverter, DocumentMetadata, Provenance,
    SourceContent, StructuralUnit,
};
use fm_semantic_docling::converter_with_baseline_fallback;

fn pdfs(directory: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(directory)
        .expect("PROCYON_DUMP_DIR must be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "pdf"))
        .collect();
    files.sort();
    files
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
            control if control < ' ' => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn record(unit: &StructuralUnit) -> String {
    let (page, block) = match unit.provenance {
        Provenance::PdfBlock {
            page_number,
            block_index,
        } => (page_number, block_index),
        _ => (0, 0),
    };
    format!(
        "{{\"page\":{page},\"block\":{block},\"kind\":{},\"text\":{}}}\n",
        json_string(&format!("{:?}", unit.kind)),
        json_string(&unit.text)
    )
}

fn budgets() -> ConversionBudgets {
    ConversionBudgets {
        timeout: std::time::Duration::from_secs(1_800),
        ..ConversionBudgets::default()
    }
}

fn write_dump(method: &str, stem: &str, units: &[StructuralUnit]) {
    let out = PathBuf::from(std::env::var("PROCYON_DUMP_OUT").expect("PROCYON_DUMP_OUT"));
    let directory = out.join(method);
    fs::create_dir_all(&directory).expect("create dump directory");
    let body: String = units.iter().map(record).collect();
    fs::write(directory.join(format!("{stem}.jsonl")), body).expect("write dump");
}

#[test]
#[ignore = "needs PROCYON_DUMP_DIR (a folder of PDFs) and PROCYON_DUMP_OUT"]
fn dump_text_layer() {
    let directory = PathBuf::from(std::env::var("PROCYON_DUMP_DIR").expect("PROCYON_DUMP_DIR"));
    let converter = converter_with_baseline_fallback();
    for path in pdfs(&directory) {
        let bytes = fs::read(&path).expect("read PDF");
        let context = ConversionContext::new().with_budgets(budgets());
        let outcome = converter
            .convert(
                SourceContent::Bytes(&bytes),
                &DocumentMetadata::unknown().with_extension("pdf"),
                &context,
            )
            .expect("conversion");
        let stem = path.file_stem().expect("file stem").to_string_lossy();
        match outcome.document() {
            Some(document) => write_dump("text-layer", &stem, document.units()),
            None => eprintln!("{stem}: no document (no text layer?)"),
        }
    }
}

#[cfg(feature = "ml")]
#[test]
#[ignore = "needs the verified Docling ML pack, PROCYON_DUMP_DIR and PROCYON_DUMP_OUT"]
fn dump_ml() {
    use fm_semantic_conversion::{AdvancedConverterBackend, ConversionOutcome};
    use fm_semantic_docling::{DoclingPdfBackend, OcrLanguage};

    let directory = PathBuf::from(std::env::var("PROCYON_DUMP_DIR").expect("PROCYON_DUMP_DIR"));
    let backend = DoclingPdfBackend::try_ml(OcrLanguage::English).expect("ML pack");
    for path in pdfs(&directory) {
        let bytes = fs::read(&path).expect("read PDF");
        let context = ConversionContext::new().with_budgets(budgets());
        let converted = AdvancedConverterBackend::convert_bytes(
            &backend,
            &bytes,
            &DocumentMetadata::unknown().with_extension("pdf"),
            &context,
        )
        .expect("conversion");
        let stem = path.file_stem().expect("file stem").to_string_lossy();
        match converted.outcome {
            ConversionOutcome::Converted(document) => write_dump("ml", &stem, document.units()),
            other => eprintln!("{stem}: {other:?}"),
        }
    }
}

/// Writes the per-page layout confidence as JSON lines to `<out>/confidence/<pdf stem>.jsonl`.
#[test]
#[ignore = "needs PROCYON_DUMP_DIR (a folder of PDFs) and PROCYON_DUMP_OUT"]
fn dump_confidence() {
    let directory = PathBuf::from(std::env::var("PROCYON_DUMP_DIR").expect("PROCYON_DUMP_DIR"));
    let out = PathBuf::from(std::env::var("PROCYON_DUMP_OUT").expect("PROCYON_DUMP_OUT"))
        .join("confidence");
    fs::create_dir_all(&out).expect("create output directory");
    for path in pdfs(&directory) {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("utf-8 file name");
        let bytes = fs::read(&path).expect("read PDF");
        let mut lines = String::new();
        for (index, page) in fm_semantic_docling::page_confidence(&bytes)
            .iter()
            .enumerate()
        {
            writeln!(
                lines,
                "{{\"page\":{},\"score\":{:.4},\"rows\":{},\"unresolved\":{},\"cuts\":{},\"weak\":{}}}",
                index + 1,
                page.score,
                page.rows,
                page.unresolved_rows,
                page.cuts,
                page.weak_cuts
            )
            .expect("write to string");
        }
        fs::write(out.join(format!("{stem}.jsonl")), lines).expect("write confidence");
    }
}
