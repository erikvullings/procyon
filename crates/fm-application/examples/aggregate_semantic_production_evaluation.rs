//! Aggregates private per-target production semantic evidence.

use std::env;
use std::fs;
use std::path::PathBuf;

use fm_application::semantic_production_evaluation::{
    ProductionEvaluationCorpus, ProductionEvaluationReport,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("expected aggregate report output path")?;
    let approved_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("expected reviewed report path")?;
    let paths = arguments.map(PathBuf::from).collect::<Vec<_>>();
    if paths.is_empty() {
        return Err("expected at least one per-target report".into());
    }
    let corpus = ProductionEvaluationCorpus::parse(include_str!(
        "../tests/fixtures/semantic-evaluation-v1.json"
    ))?;
    let approved = ProductionEvaluationReport::parse(&fs::read_to_string(&approved_path)?)?;
    let mut measurements = Vec::new();
    for path in paths {
        let report = ProductionEvaluationReport::parse(&fs::read_to_string(&path)?)?;
        report.validate(&corpus)?;
        if report.measurements.len() != 1 || !report.measurements[0].production_package {
            return Err(format!(
                "per-target report {} must contain one strict production measurement",
                path.display()
            )
            .into());
        }
        measurements.extend(report.measurements);
    }
    let report = ProductionEvaluationReport::from_measurements_for_policy(
        &corpus,
        approved.evidence_policy,
        "Private supported-target matrix over exact content-addressed production workers, packaged native runtimes, pinned multilingual model, production converter/chunker, native Zvec indexes, and Ask retrieval policy.",
        approved.limitations,
        measurements,
        approved.embedding_preprocessing_migration,
        approved.manual_criteria,
    );
    report.validate(&corpus)?;
    if !report.production_measurement {
        return Err("aggregate report did not cover every supported production target".into());
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, report.to_json()?)?;
    println!("{}", output.display());
    Ok(())
}
