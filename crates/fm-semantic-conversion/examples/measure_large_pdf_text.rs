//! Measures conversion of a generated PDF fixture without changing production limits.
//!
//! Generate the fixture with scripts/generate-semantic-pdf-fixtures.mjs in a
//! temporary directory. Pass only that synthetic text fixture to this probe.

use std::env;
use std::fs;
use std::time::{Duration, Instant};

use fm_semantic_conversion::{
    BaselineConverter, ConversionBudgets, ConversionContext, ConversionOutcome, DocumentConverter,
    DocumentMetadata, Provenance, SourceContent,
};
use sha2::{Digest, Sha256};

const SYNTHETIC_TEXT_DIGEST: &str =
    "b1e36f4b3e4664d00e09475372f9e90858aaef0adfab3762aaa0ea98cc4d2285";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os()
        .nth(1)
        .ok_or("pass a synthetic PDF fixture path")?;
    let name = std::path::Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid fixture name")?;
    if name != "synthetic-354-page-text.pdf" {
        return Err("only the generated 354-page text fixture is accepted".into());
    }
    let source = fs::read(&path)?;
    let source_bytes = source.len();
    if !(175 * 1024 * 1024..=200 * 1024 * 1024).contains(&source_bytes) {
        return Err("synthetic fixture is outside the expected 175-200 MiB range".into());
    }
    let digest = Sha256::digest(&source)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if digest != SYNTHETIC_TEXT_DIGEST {
        return Err("input bytes do not match the generated synthetic fixture".into());
    }
    let budgets = ConversionBudgets {
        max_source_bytes: 200 * 1024 * 1024,
        timeout: Duration::from_secs(120),
        ..ConversionBudgets::default()
    };
    let started = Instant::now();
    let outcome = BaselineConverter::new().convert(
        SourceContent::Bytes(&source),
        &DocumentMetadata::unknown().with_media_type("application/pdf"),
        &ConversionContext::new().with_budgets(budgets),
    )?;
    let ConversionOutcome::Converted(document) = outcome else {
        return Err(format!("synthetic PDF conversion failed: {outcome:?}").into());
    };
    if document.is_partial() {
        return Err(format!(
            "synthetic PDF conversion was partial: {:?}",
            document.omissions()
        )
        .into());
    }
    let pages = document
        .units()
        .iter()
        .filter_map(|unit| match unit.provenance {
            Provenance::PdfBlock { page_number, .. } => Some(page_number),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    if pages.len() != 354 || pages.first() != Some(&1) || pages.last() != Some(&354) {
        return Err(format!("expected text on pages 1-354; found {pages:?}").into());
    }
    println!(
        "source_bytes={source_bytes} pages={} text_units={} elapsed_ms={}",
        pages.len(),
        document.units().len(),
        started.elapsed().as_millis()
    );
    Ok(())
}
