//! Validates that checked-in semantic evidence supports a production release.

use std::{env, fs, process};

use fm_application::semantic_production_evaluation::{
    ProductionEvaluationCorpus, ProductionEvaluationDecision, ProductionEvaluationReport,
    ProductionEvidencePolicy,
};

fn validate() -> Result<(), String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 3 || arguments[0] != "--policy" {
        return Err(
            "expected --policy <production-stable|experimental-alpha> <report-path>".to_owned(),
        );
    }
    let evidence_policy = match arguments[1].as_str() {
        "production-stable" => ProductionEvidencePolicy::ProductionStable,
        "experimental-alpha" => ProductionEvidencePolicy::ExperimentalAlpha,
        policy => return Err(format!("unsupported semantic evidence policy `{policy}`")),
    };
    let path = &arguments[2];
    let json = fs::read_to_string(path)
        .map_err(|error| format!("failed to read semantic evaluation report {path}: {error}"))?;
    let report = ProductionEvaluationReport::parse(&json)
        .map_err(|error| format!("failed to parse semantic evaluation report {path}: {error}"))?;
    let corpus = ProductionEvaluationCorpus::parse(include_str!(
        "../tests/fixtures/semantic-evaluation-v1.json"
    ))
    .map_err(|error| format!("failed to parse task-0188 corpus: {error}"))?;
    report
        .validate_for_policy(&corpus, evidence_policy)
        .map_err(|error| format!("invalid semantic evaluation report {path}: {error}"))?;
    if report.decision != ProductionEvaluationDecision::Go
        || !report.production_measurement
        || !report.blocking_reasons.is_empty()
    {
        return Err(format!(
            "semantic evaluation report {path} does not record a measured, unblocked {evidence_policy:?} go"
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
