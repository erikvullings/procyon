//! Validates that a checked-in knowledge retrieval report supports release.

use std::{env, fs, process};

use fm_application::knowledge_evaluation::{EvaluationDecision, EvaluationReport, RetrievalCorpus};

fn validate() -> Result<(), String> {
    let path = env::args()
        .nth(1)
        .ok_or_else(|| "expected the evaluation report path".to_owned())?;
    let json = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read evaluation report {path}: {error}"))?;
    let report = EvaluationReport::parse(&json)
        .map_err(|error| format!("failed to parse evaluation report {path}: {error}"))?;
    report
        .validate()
        .map_err(|error| format!("invalid evaluation report {path}: {error}"))?;
    let corpus_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/knowledge-retrieval-corpus-v1.json");
    let corpus_json = fs::read_to_string(&corpus_path).map_err(|error| {
        format!(
            "failed to read evaluation corpus {}: {error}",
            corpus_path.display()
        )
    })?;
    let corpus = RetrievalCorpus::parse(&corpus_json)
        .map_err(|error| format!("failed to parse evaluation corpus: {error}"))?;
    if report.corpus_id != corpus.corpus_id || report.corpus_fingerprint != corpus.fingerprint() {
        return Err("the evaluation report does not match the current corpus".to_owned());
    }
    if report.decision != EvaluationDecision::Go
        || !report.production_measurement
        || !report.blocking_reasons.is_empty()
    {
        return Err(format!(
            "evaluation report {path} does not record a measured, unblocked go"
        ));
    }
    Ok(())
}

fn main() {
    if let Err(error) = validate() {
        eprintln!("{error}");
        process::exit(1);
    }
}
