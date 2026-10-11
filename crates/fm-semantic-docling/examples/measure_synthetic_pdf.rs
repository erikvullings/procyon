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
    if !matches!(variant, "scanned" | "text" | "malformed" | "bomb" | "mixed") {
        return Err(
            "only the generated scanned|text|malformed|bomb|mixed fixtures may be measured",
        );
    }
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/large-pdf-budget-{variant}.pdf")))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let variant = std::env::args()
        .nth(1)
        .ok_or("specify scanned|text|malformed|bomb|mixed")?;
    let path = fixture_path(&variant)?;
    let source_bytes = path.metadata()?.len();
    if source_bytes > FIXTURE_SOURCE_LIMIT {
        return Err("the synthetic PDF exceeds the fixture-only 200 MiB limit".into());
    }
    let bytes = std::fs::read(&path)?;
    let context = ConversionContext::new()
        .with_budgets(ConversionBudgets {
            max_source_bytes: FIXTURE_SOURCE_LIMIT,
            timeout: Duration::from_secs(90),
            ..ConversionBudgets::default()
        })
        .with_visual_evidence(variant == "mixed");
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
            if variant == "mixed"
                && (document.visuals().len() != 2 || document.visual_omissions().len() != 1)
            {
                return Err(
                    "mixed fixture must retain two JPEGs and omit its non-JPEG page".into(),
                );
            }
            let text_pages = document
                .units()
                .iter()
                .filter_map(|unit| match unit.provenance {
                    Provenance::PdfBlock { page_number, .. } => Some(page_number),
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            println!(
                "source_bytes={source_bytes} units={} text_pages={} visuals={} visual_omissions={} omissions={}",
                document.units().len(),
                text_pages.len(),
                document.visuals().len(),
                document.visual_omissions().len(),
                document.omissions().len()
            );
            for omission in document.visual_omissions() {
                println!("visual_omission={omission:?}");
            }
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
        assert!(
            fixture_path("mixed")
                .expect("synthetic mixed fixture")
                .ends_with("target/large-pdf-budget-mixed.pdf")
        );
    }
}
