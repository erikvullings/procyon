//! Evaluation-only converter probe for the deterministic PDF fixtures in target/.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use fm_semantic_conversion::{
    ConversionBudgets, ConversionContext, ConversionOutcome, DocumentConverter, DocumentMetadata,
    Provenance, SourceContent,
};
use fm_semantic_docling::converter_with_baseline_fallback;

const FIXTURE_SOURCE_LIMIT: u64 = 200 * 1024 * 1024;

fn fixture_path(variant: &str) -> Result<PathBuf, &'static str> {
    if !matches!(variant, "scanned" | "text" | "malformed" | "bomb") {
        return Err("only the generated scanned|text|malformed|bomb fixtures may be measured");
    }
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/large-pdf-budget-{variant}.pdf")))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let variant = std::env::args()
        .nth(1)
        .ok_or("specify scanned|text|malformed|bomb")?;
    let path = fixture_path(&variant)?;
    let source_bytes = path.metadata()?.len();
    if source_bytes > FIXTURE_SOURCE_LIMIT {
        return Err("the synthetic PDF exceeds the fixture-only 200 MiB limit".into());
    }
    let bytes = std::fs::read(&path)?;
    let context = ConversionContext::new().with_budgets(ConversionBudgets {
        max_source_bytes: FIXTURE_SOURCE_LIMIT,
        timeout: Duration::from_secs(90),
        ..ConversionBudgets::default()
    });
    let metadata = DocumentMetadata::unknown()
        .with_media_type("application/pdf")
        .with_byte_length(source_bytes);
    let outcome = converter_with_baseline_fallback().convert(
        SourceContent::Bytes(&bytes),
        &metadata,
        &context,
    )?;
    match outcome {
        ConversionOutcome::Converted(document) => {
            let text_pages = document
                .units()
                .iter()
                .filter_map(|unit| match unit.provenance {
                    Provenance::PdfBlock { page_number, .. } => Some(page_number),
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            println!(
                "source_bytes={source_bytes} units={} text_pages={} visuals={} omissions={}",
                document.units().len(),
                text_pages.len(),
                document.visuals().len(),
                document.omissions().len()
            );
        }
        other => println!("source_bytes={source_bytes} outcome={other:?}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_arbitrary_paths_and_variants() {
        assert!(fixture_path("../../private.pdf").is_err());
        assert!(fixture_path("edited").is_err());
        assert!(
            fixture_path("text")
                .expect("synthetic text fixture")
                .ends_with("target/large-pdf-budget-text.pdf")
        );
    }
}
