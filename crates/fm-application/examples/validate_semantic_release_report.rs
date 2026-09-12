//! Validates that checked-in semantic evidence supports a production release.

use std::{env, fs, process};

use fm_application::semantic_production_evaluation::{
    ProductionEvaluationCorpus, ProductionEvaluationDecision, ProductionEvaluationReport,
};

fn validate() -> Result<(), String> {
    let path = env::args()
        .nth(1)
        .ok_or_else(|| "expected the semantic evaluation report path".to_owned())?;
    let json = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read semantic evaluation report {path}: {error}"))?;
    let report = ProductionEvaluationReport::parse(&json)
        .map_err(|error| format!("failed to parse semantic evaluation report {path}: {error}"))?;
    let corpus = ProductionEvaluationCorpus::parse(include_str!(
        "../tests/fixtures/semantic-evaluation-v1.json"
    ))
    .map_err(|error| format!("failed to parse task-0188 corpus: {error}"))?;
    report
        .validate(&corpus)
        .map_err(|error| format!("invalid semantic evaluation report {path}: {error}"))?;
    if report.decision != ProductionEvaluationDecision::Go
        || !report.production_measurement
        || !report.blocking_reasons.is_empty()
    {
        return Err(format!(
            "semantic evaluation report {path} does not record a measured, unblocked go"
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
