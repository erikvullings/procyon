//! Release-gate evaluation of Structured Knowledge retrieval strategies.
//!
//! Every number here is produced by running the repository-owned corpus
//! through the production planner, coordinator, and worker retrieval service.
//! Nothing is transcribed by hand.

use std::collections::BTreeSet;

use fm_application::knowledge_evaluation::{
    EvaluationDecision, KnowledgeEvaluationError, KnowledgeRetrievalStrategy, RetrievalCorpus,
    StrategyEvaluation, evaluate_knowledge_retrieval,
};

const CORPUS: &str = include_str!("fixtures/knowledge-retrieval-corpus-v1.json");
const REPORT: &str = include_str!("../../../docs/evaluations/knowledge-retrieval-v1.json");

/// Honest description of what produced the checked-in numbers.
const MEASUREMENT_BASIS: &str = "Deterministic repository fixture: production planner, \
     coordinator, catalog, fusion, and evidence materialization, with a deterministic lexical \
     surrogate embedding model and the `in-process-surrogate/1` candidate index backend instead \
     of a production multilingual model and the native Zvec collection. Latency is planning plus \
     retrieval, single-run wall clock on one developer machine, not production or cross-platform \
     hardware.";
/// Latency ceiling a production measurement would have to respect.
const MAXIMUM_P95_MICROS: u64 = 250_000;

/// Limitations a reader must apply to every number in the checked-in report.
fn limitations() -> Vec<String> {
    [
        "The embedding model is a deterministic term-frequency/inverse-document-frequency \
         surrogate. Cross-language matching exists only where the corpus declares an alias, so \
         no conclusion about a production multilingual model can be drawn from the multilingual \
         cases.",
        "Candidate retrieval runs over the `in-process-surrogate/1` index backend rather than the \
         native Zvec collection, so lexical scoring, approximate-nearest-neighbour recall, and \
         native full-text migration behaviour are not measured. That backend identity is part of \
         every pipeline fingerprint here, so these numbers can never be read as native ones.",
        "Source recall at ten saturates on this eighteen-document corpus: every strategy reaches \
         1.0, so the recall-at-ten gate cannot distinguish the strategies here regardless of \
         retrieval quality. Ranking quality is visible in recall at five, mean reciprocal rank, \
         and context-driven irrelevant hits instead.",
        "Negative controls count any returned source as a false positive. Dense routes have no \
         relevance floor and therefore always answer, which is a property of the route rather \
         than of a specific strategy.",
        "Latency is a single-run wall clock on one developer machine under a debug build. It is \
         not a production, release-build, or cross-platform measurement.",
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect()
}

fn corpus() -> RetrievalCorpus {
    RetrievalCorpus::parse(CORPUS).expect("repository corpus")
}

fn strategy(
    evaluations: &[StrategyEvaluation],
    strategy: KnowledgeRetrievalStrategy,
) -> &StrategyEvaluation {
    evaluations
        .iter()
        .find(|item| item.strategy == strategy)
        .expect("evaluated strategy")
}

#[tokio::test]
async fn the_corpus_covers_every_required_retrieval_behavior() {
    let corpus = corpus();

    let categories = corpus
        .cases
        .iter()
        .map(|case| case.category.as_str())
        .collect::<BTreeSet<_>>();
    let languages = corpus
        .documents
        .iter()
        .map(|document| document.language.as_str())
        .collect::<BTreeSet<_>>();

    corpus.validate().expect("valid corpus");
    assert!(
        languages.len() >= 3,
        "a multilingual corpus needs several languages, got {languages:?}"
    );
    for required in [
        "specialistTerm",
        "multilingual",
        "procedure",
        "example",
        "limitation",
        "applicationContext",
        "duplicate",
        "update",
        "deletion",
        "scopeIsolation",
        "negativeControl",
        "matchSorting",
    ] {
        assert!(
            categories.contains(required),
            "corpus is missing the `{required}` behavior"
        );
    }
}

#[tokio::test]
async fn exactly_five_strategies_are_compared_on_identical_authorized_content() {
    let comparison = evaluate_knowledge_retrieval(&corpus())
        .await
        .expect("comparison");

    assert_eq!(comparison.strategies.len(), 5);
    assert_eq!(
        comparison
            .strategies
            .iter()
            .map(|item| item.strategy)
            .collect::<Vec<_>>(),
        KnowledgeRetrievalStrategy::ALL
    );
    let fingerprints = comparison
        .strategies
        .iter()
        .map(|item| item.pipeline_fingerprint.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        fingerprints.len(),
        5,
        "each strategy needs its own exact pipeline fingerprint"
    );
    for evaluation in &comparison.strategies {
        assert_eq!(
            evaluation.metrics.evaluated_cases,
            comparison.strategies[0].metrics.evaluated_cases
        );
        assert_eq!(
            evaluation.metrics.authorized_sources,
            comparison.strategies[0].metrics.authorized_sources,
            "every strategy must see identical authorized content"
        );
        assert_eq!(evaluation.metrics.scope_violations, 0);
    }
}

#[tokio::test]
async fn structured_retrieval_ranks_procedures_and_examples_above_context_only_sorting() {
    let comparison = evaluate_knowledge_retrieval(&corpus())
        .await
        .expect("comparison");

    let regression = &comparison.match_sorting_regression;
    assert!(!regression.expected_source_ids.is_empty());
    for observation in &regression.observations {
        if observation.strategy == KnowledgeRetrievalStrategy::StructuredHybrid {
            assert!(
                observation.expected_ranks.iter().all(|rank| *rank <= 5),
                "procedure and example sources must rank strongly, got {:?}",
                observation.expected_ranks
            );
            assert!(
                observation
                    .context_only_rank
                    .is_none_or(|rank| rank > observation.expected_ranks.len()),
                "the context-only sorting document must not outrank real matches"
            );
        }
    }
    assert!(regression.passed);
}

#[tokio::test]
async fn the_checked_in_report_is_reproduced_exactly_and_records_no_go() {
    let comparison = evaluate_knowledge_retrieval(&corpus())
        .await
        .expect("comparison");
    let report = fm_application::knowledge_evaluation::EvaluationReport::parse(REPORT)
        .expect("checked-in report");

    report.validate().expect("valid checked-in report");
    assert_eq!(report.corpus_fingerprint, comparison.corpus_fingerprint);
    for expected in &comparison.strategies {
        let recorded = strategy(&report.strategies, expected.strategy);
        assert_eq!(recorded.pipeline_fingerprint, expected.pipeline_fingerprint);
        assert_eq!(recorded.metrics, expected.metrics);
    }
    assert_eq!(
        report.match_sorting_regression,
        comparison.match_sorting_regression
    );
    assert_eq!(report.decision, EvaluationDecision::NoGo);
    assert!(
        !report.production_measurement,
        "no production or cross-platform measurement exists yet"
    );
    assert!(!report.blocking_reasons.is_empty());
}

/// The comparison is run on everything authorization permits, including a
/// source whose occurrences were deleted after publication. Catalog deletion,
/// not the evaluation's allow-list, is what keeps that source out of results.
#[tokio::test]
async fn a_deleted_source_stays_inside_the_authorized_content_every_strategy_saw() {
    let corpus = corpus();
    let comparison = evaluate_knowledge_retrieval(&corpus)
        .await
        .expect("comparison");

    let authorized = corpus.authorized_sources();
    let deleted = corpus.deleted_sources();
    assert!(!deleted.is_empty(), "a deletion case is required");
    assert!(
        deleted.is_subset(&authorized),
        "withholding deleted sources from the allow-list would fake the regression"
    );
    for evaluation in &comparison.strategies {
        assert_eq!(
            evaluation.metrics.authorized_sources,
            authorized.len(),
            "{:?} must be compared on everything authorization permits",
            evaluation.strategy
        );
        assert_eq!(
            evaluation.metrics.stale_or_deleted_hits, 0,
            "{:?} returned superseded or deleted evidence",
            evaluation.strategy
        );
    }
}

/// Reported latency is planning plus retrieval, so neither phase can exceed it.
#[tokio::test]
async fn reported_latency_accounts_for_planning_as_well_as_retrieval() {
    let comparison = evaluate_knowledge_retrieval(&corpus())
        .await
        .expect("comparison");

    for evaluation in &comparison.strategies {
        let latency = evaluation.latency;
        assert!(
            latency.planning_p50_micros <= latency.p50_micros
                && latency.retrieval_p50_micros <= latency.p50_micros,
            "{:?} reported a phase longer than the total it belongs to: {latency:?}",
            evaluation.strategy
        );
    }
}

#[tokio::test]
async fn retained_full_text_storage_counts_every_indexed_occurrence() {
    let corpus = corpus();
    let comparison = evaluate_knowledge_retrieval(&corpus)
        .await
        .expect("comparison");
    let bytes_without_duplicate_occurrences = corpus
        .documents
        .iter()
        .flat_map(|document| document.previous_chunks.iter().chain(&document.chunks))
        .map(|chunk| u64::try_from(chunk.text.len()).expect("fixture text length"))
        .sum::<u64>();
    let expected_bytes = corpus
        .documents
        .iter()
        .map(|document| {
            document
                .previous_chunks
                .iter()
                .chain(&document.chunks)
                .map(|chunk| u64::try_from(chunk.text.len()).expect("fixture text length"))
                .sum::<u64>()
                * u64::try_from(document.occurrences.len()).expect("occurrence count")
        })
        .sum::<u64>();
    let measured = strategy(
        &comparison.strategies,
        KnowledgeRetrievalStrategy::StructuredFullText,
    )
    .metrics
    .retained_index_bytes;

    assert_eq!(measured, expected_bytes);
    assert!(
        measured > bytes_without_duplicate_occurrences,
        "duplicate occurrences must increase retained index storage"
    );
}

#[tokio::test]
async fn the_report_records_no_generated_answer_fluency_metric() {
    let report = fm_application::knowledge_evaluation::EvaluationReport::parse(REPORT)
        .expect("checked-in report");

    let encoded = serde_json::to_string(&report).expect("re-encode");
    for forbidden in [
        "fluency",
        "answerQuality",
        "generatedAnswer",
        "bleu",
        "rouge",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "retrieval evaluation must not record `{forbidden}`"
        );
    }
}

#[tokio::test]
async fn a_corpus_without_negative_controls_is_rejected() {
    let mut corpus = corpus();
    corpus.cases.retain(|case| !case.expected_no_evidence);

    assert!(matches!(
        corpus.validate(),
        Err(KnowledgeEvaluationError::IncompleteCorpus(_))
    ));
}

/// Rewrites the checked-in report from a fresh run of this corpus.
///
/// Latency is machine dependent, so the report is regenerated deliberately
/// rather than on every test run.
#[tokio::test]
#[ignore = "regenerates docs/evaluations/knowledge-retrieval-v1.json on request"]
async fn regenerate_the_checked_in_report() {
    let comparison = evaluate_knowledge_retrieval(&corpus())
        .await
        .expect("comparison");
    let report = fm_application::knowledge_evaluation::EvaluationReport::from_comparison(
        &comparison,
        MEASUREMENT_BASIS,
        limitations(),
        false,
        MAXIMUM_P95_MICROS,
    );

    report.validate().expect("regenerated report is consistent");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/evaluations/knowledge-retrieval-v1.json");
    std::fs::write(path, report.to_checked_in_json().expect("encode")).expect("write report");
}
