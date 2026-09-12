//! Prints the current fail-closed semantic release report template.

use fm_application::semantic_production_evaluation::{
    ProductionEvaluationCorpus, ProductionEvaluationReport,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let corpus = ProductionEvaluationCorpus::parse(include_str!(
        "../tests/fixtures/semantic-evaluation-v1.json"
    ))?;
    let report = ProductionEvaluationReport::from_measurements(
        &corpus,
        "No exact production measurements have been checked in. Private workflow evidence must be reviewed and reduced to opaque aggregate and per-case identities before this template can change.",
        vec![
            "No supported-target production measurements are recorded in this checked-in template.".into(),
            "Generated-answer, installed lifecycle, accessibility, privacy, and failure-mode criteria remain unmeasured.".into(),
        ],
        Vec::new(),
        ProductionEvaluationReport::pending_manual_criteria(),
    );
    report.validate(&corpus)?;
    print!("{}", report.to_json()?);
    Ok(())
}
