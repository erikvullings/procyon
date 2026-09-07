//! Local, deterministic retrieval evaluation for semantic release gates.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const EVALUATION_SCHEMA_VERSION: u32 = 2;
const MAX_CASES: usize = 10_000;
const MAX_RESULTS_PER_CASE: usize = 1_000;
const MAX_EVALUATION_BYTES: usize = 64 * 1024 * 1024;

/// One local relevance judgment. Queries and identities never leave the device implicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationCase {
    /// Stable opaque case identity.
    pub id: String,
    /// Local query text used to exercise retrieval.
    pub query: String,
    /// Expected relevant file identities.
    pub relevant_file_ids: BTreeSet<String>,
    /// Expected relevant source-chunk identities.
    pub relevant_chunk_ids: BTreeSet<String>,
    /// Retrieval behavior represented by this case.
    #[serde(default)]
    pub category: EvaluationCaseCategory,
    /// Whether any retrieved result is a false positive.
    #[serde(default)]
    pub expected_no_answer: bool,
}

/// Retrieval behavior represented by one evaluation case.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvaluationCaseCategory {
    /// Existing case without a more specific classification.
    #[default]
    Other,
    /// One direct information need.
    SingleIntent,
    /// Alternate wording for indexed evidence.
    Paraphrase,
    /// Multiple independent facts required by one question.
    MultiFacet,
    /// Query and evidence use different supported languages.
    Multilingual,
    /// Intentionally underspecified information need.
    Ambiguous,
    /// Query for which no in-scope evidence should qualify.
    NegativeControl,
    /// Query text shaped like an instruction or authority override.
    PromptInjection,
    /// Near-duplicate or boilerplate-heavy evidence.
    Duplicate,
    /// Evidence exists only outside the authorized scope.
    ScopeIsolation,
}

/// Ranked output captured from one retrieval run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationObservation {
    /// Case identity whose retrieval result was observed.
    pub case_id: String,
    /// File identities in retrieval rank order.
    pub ranked_file_ids: Vec<String>,
    /// Chunk identities in retrieval rank order.
    pub ranked_chunk_ids: Vec<String>,
}

/// Aggregate retrieval metrics. Generated answers are deliberately absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalMetrics {
    /// Number of fixture cases included, including cases with no observation.
    pub evaluated_cases: usize,
    /// Shared rank cutoff used for every metric.
    pub cutoff: usize,
    /// Mean relevant-file recall at the cutoff.
    pub file_recall_at_k: f64,
    /// Mean relevant-chunk recall at the cutoff.
    pub chunk_recall_at_k: f64,
    /// Mean reciprocal rank of the first relevant file.
    pub mean_reciprocal_rank: f64,
    /// Mean normalized discounted cumulative gain using binary relevance.
    pub ndcg_at_k: f64,
}

/// Before/after evidence required for a semantic contract change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationChangeReport {
    /// Immutable model/chunker/converter/index baseline identity.
    pub baseline_fingerprint: String,
    /// Immutable candidate identity under evaluation.
    pub candidate_fingerprint: String,
    /// Required operator-readable migration description.
    pub migration_impact: String,
    /// Signed change in expected retained storage.
    pub storage_impact_bytes: i64,
    /// Metrics collected before the change.
    pub baseline: RetrievalMetrics,
    /// Metrics collected after the change.
    pub candidate: RetrievalMetrics,
}

/// One measured retrieval run for a benchmark case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkObservation {
    /// Ranked retrieval result for one case.
    pub retrieval: EvaluationObservation,
    /// Number of bounded retrieval queries.
    pub query_count: usize,
    /// Number of local query embeddings.
    pub embedding_count: usize,
    /// Tokens retained in final evidence.
    pub selected_context_tokens: usize,
    /// Estimated planner input tokens.
    pub planner_input_tokens: usize,
    /// Measured planner output tokens.
    pub planner_output_tokens: usize,
    /// Planner elapsed time.
    pub planning_latency_ms: u64,
    /// Local retrieval elapsed time.
    pub retrieval_latency_ms: u64,
    /// Results that escaped the authorized fixture scope.
    pub scope_violations: usize,
}

/// Retrieval quality, safety, latency, and resource metrics for one strategy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiQueryBenchmarkMetrics {
    /// Existing file/chunk retrieval metrics.
    pub retrieval: RetrievalMetrics,
    /// File recall over multi-facet cases only.
    pub multi_facet_file_recall_at_k: f64,
    /// Fraction of negative controls that returned evidence.
    pub no_answer_false_positive_rate: f64,
    /// Fraction of ranked identities repeated within a case.
    pub duplicate_rate: f64,
    /// Mean retrieval-query count.
    pub average_query_count: f64,
    /// Mean local embedding count.
    pub average_embedding_count: f64,
    /// Mean final evidence token count.
    pub average_selected_context_tokens: f64,
    /// Mean planner input token count.
    pub average_planner_input_tokens: f64,
    /// Mean planner output token count.
    pub average_planner_output_tokens: f64,
    /// Median planning-plus-retrieval latency.
    pub p50_planning_plus_retrieval_latency_ms: u64,
    /// 95th percentile planning-plus-retrieval latency.
    pub p95_planning_plus_retrieval_latency_ms: u64,
    /// Largest retrieval-query count in one case.
    pub maximum_query_count: usize,
    /// Largest embedding count in one case.
    pub maximum_embedding_count: usize,
    /// Total authorization-scope violations.
    pub scope_violations: usize,
}

/// Measured decision for the opt-in multi-query candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MultiQueryBenchmarkDecision {
    /// Candidate met every quality, safety, and resource gate.
    Go,
    /// Candidate failed at least one required gate.
    NoGo,
}

/// Comparable strategy metrics and their measured go/no-go decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiQueryBenchmarkComparison {
    /// Current exact-question control metrics.
    pub baseline: MultiQueryBenchmarkMetrics,
    /// Proposed bounded multi-query metrics.
    pub candidate: MultiQueryBenchmarkMetrics,
    /// Maximum accepted candidate p95 latency.
    pub maximum_p95_latency_ms: u64,
    /// Whether timings and retrieval outputs came from a production-equivalent benchmark run.
    pub production_measurement: bool,
    /// Decision derived from the fixed thresholds.
    pub decision: MultiQueryBenchmarkDecision,
}

/// Reproducible checked-in report for one control/candidate comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiQueryBenchmarkReport {
    /// Report schema version.
    pub report_version: u32,
    /// Immutable single-query control identity.
    pub baseline_fingerprint: String,
    /// Immutable multi-query candidate identity.
    pub candidate_fingerprint: String,
    /// Exact planner contract identity.
    pub planner_version: String,
    /// Exact fusion contract identity.
    pub fusion_version: String,
    /// Operator-readable benchmark environment.
    pub benchmark_hardware: String,
    /// Index/storage migration effect.
    pub migration_impact: String,
    /// Signed change in retained storage.
    pub storage_impact_bytes: i64,
    /// Measured metrics and decision.
    pub comparison: MultiQueryBenchmarkComparison,
}

impl MultiQueryBenchmarkReport {
    /// Validates required identities, metrics, bounds, and the recorded decision.
    pub fn validate(&self) -> Result<(), EvaluationError> {
        validate_id(&self.baseline_fingerprint)?;
        validate_id(&self.candidate_fingerprint)?;
        validate_id(&self.planner_version)?;
        validate_id(&self.fusion_version)?;
        if self.report_version != 1
            || self.baseline_fingerprint == self.candidate_fingerprint
            || self.benchmark_hardware.trim().is_empty()
            || self.migration_impact.trim().is_empty()
            || self.comparison.baseline.retrieval.cutoff
                != self.comparison.candidate.retrieval.cutoff
            || self.comparison.baseline.retrieval.evaluated_cases
                != self.comparison.candidate.retrieval.evaluated_cases
            || self.comparison.maximum_p95_latency_ms == 0
            || !benchmark_metrics_are_valid(&self.comparison.baseline)
            || !benchmark_metrics_are_valid(&self.comparison.candidate)
            || self.comparison.decision
                != benchmark_decision(
                    &self.comparison.baseline,
                    &self.comparison.candidate,
                    self.comparison.maximum_p95_latency_ms,
                    self.comparison.production_measurement,
                )
        {
            return Err(EvaluationError::InvalidChangeReport);
        }
        Ok(())
    }
}

/// Scores single-query control and bounded multi-query observations over the same cases.
pub fn evaluate_multi_query_benchmark(
    cases: &[EvaluationCase],
    baseline: &[BenchmarkObservation],
    candidate: &[BenchmarkObservation],
    cutoff: usize,
    maximum_p95_latency_ms: u64,
    production_measurement: bool,
) -> Result<MultiQueryBenchmarkComparison, EvaluationError> {
    if maximum_p95_latency_ms == 0
        || baseline
            .iter()
            .any(|item| item.query_count != 1 || item.embedding_count != 1)
    {
        return Err(EvaluationError::InvalidChangeReport);
    }
    let baseline = score_benchmark(cases, baseline, cutoff)?;
    let candidate = score_benchmark(cases, candidate, cutoff)?;
    let decision = benchmark_decision(
        &baseline,
        &candidate,
        maximum_p95_latency_ms,
        production_measurement,
    );
    Ok(MultiQueryBenchmarkComparison {
        baseline,
        candidate,
        maximum_p95_latency_ms,
        production_measurement,
        decision,
    })
}

fn benchmark_decision(
    baseline: &MultiQueryBenchmarkMetrics,
    candidate: &MultiQueryBenchmarkMetrics,
    maximum_p95_latency_ms: u64,
    production_measurement: bool,
) -> MultiQueryBenchmarkDecision {
    let production_measurements_present = candidate.average_planner_input_tokens > 0.0
        && candidate.average_planner_output_tokens > 0.0
        && candidate.p50_planning_plus_retrieval_latency_ms > 0
        && candidate.p95_planning_plus_retrieval_latency_ms > 0;
    let retrieval_regression_within_limit = [
        (
            baseline.retrieval.file_recall_at_k,
            candidate.retrieval.file_recall_at_k,
        ),
        (
            baseline.retrieval.chunk_recall_at_k,
            candidate.retrieval.chunk_recall_at_k,
        ),
        (
            baseline.retrieval.mean_reciprocal_rank,
            candidate.retrieval.mean_reciprocal_rank,
        ),
        (baseline.retrieval.ndcg_at_k, candidate.retrieval.ndcg_at_k),
    ]
    .into_iter()
    .all(|(control, proposed)| proposed + 0.02 >= control);
    if production_measurement
        && production_measurements_present
        && candidate.scope_violations == 0
        && candidate.no_answer_false_positive_rate <= baseline.no_answer_false_positive_rate
        && candidate.multi_facet_file_recall_at_k >= baseline.multi_facet_file_recall_at_k + 0.10
        && retrieval_regression_within_limit
        && candidate.maximum_query_count <= 4
        && candidate.maximum_embedding_count <= 4
        && candidate.p95_planning_plus_retrieval_latency_ms <= maximum_p95_latency_ms
    {
        MultiQueryBenchmarkDecision::Go
    } else {
        MultiQueryBenchmarkDecision::NoGo
    }
}

fn score_benchmark(
    cases: &[EvaluationCase],
    observations: &[BenchmarkObservation],
    cutoff: usize,
) -> Result<MultiQueryBenchmarkMetrics, EvaluationError> {
    if cases.is_empty() || observations.len() != cases.len() {
        return Err(EvaluationError::InvalidChangeReport);
    }
    let expected_ids = cases
        .iter()
        .map(|case| case.id.as_str())
        .collect::<BTreeSet<_>>();
    let observed_ids = observations
        .iter()
        .map(|item| item.retrieval.case_id.as_str())
        .collect::<BTreeSet<_>>();
    if expected_ids != observed_ids
        || observations.iter().any(|item| {
            item.query_count == 0
                || item.query_count > 4
                || item.embedding_count == 0
                || item.embedding_count > 4
        })
    {
        return Err(EvaluationError::InvalidChangeReport);
    }

    let retrieval_observations = observations
        .iter()
        .map(|item| item.retrieval.clone())
        .collect::<Vec<_>>();
    let retrieval = evaluate_retrieval(cases, &retrieval_observations, cutoff)?;
    let multi_facet_cases = cases
        .iter()
        .filter(|case| case.category == EvaluationCaseCategory::MultiFacet)
        .cloned()
        .collect::<Vec<_>>();
    if multi_facet_cases.is_empty() {
        return Err(EvaluationError::InvalidChangeReport);
    }
    let multi_facet_ids = multi_facet_cases
        .iter()
        .map(|case| case.id.as_str())
        .collect::<BTreeSet<_>>();
    let multi_facet_observations = retrieval_observations
        .iter()
        .filter(|item| multi_facet_ids.contains(item.case_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let multi_facet_file_recall_at_k =
        evaluate_retrieval(&multi_facet_cases, &multi_facet_observations, cutoff)?.file_recall_at_k;

    let cases_by_id = cases
        .iter()
        .map(|case| (case.id.as_str(), case))
        .collect::<BTreeMap<_, _>>();
    let negative_count = cases.iter().filter(|case| case.expected_no_answer).count();
    if negative_count == 0 {
        return Err(EvaluationError::InvalidChangeReport);
    }
    let false_positives = observations
        .iter()
        .filter(|item| {
            cases_by_id
                .get(item.retrieval.case_id.as_str())
                .is_some_and(|case| case.expected_no_answer)
                && (!item.retrieval.ranked_file_ids.is_empty()
                    || !item.retrieval.ranked_chunk_ids.is_empty())
        })
        .count();
    let (duplicates, ranked_count) =
        observations
            .iter()
            .fold((0usize, 0usize), |(duplicates, total), item| {
                let files = &item.retrieval.ranked_file_ids;
                let chunks = &item.retrieval.ranked_chunk_ids;
                let unique_files = files.iter().collect::<BTreeSet<_>>().len();
                let unique_chunks = chunks.iter().collect::<BTreeSet<_>>().len();
                (
                    duplicates
                        .saturating_add(files.len().saturating_sub(unique_files))
                        .saturating_add(chunks.len().saturating_sub(unique_chunks)),
                    total
                        .saturating_add(files.len())
                        .saturating_add(chunks.len()),
                )
            });
    let count = observations.len() as f64;
    let mut latencies = observations
        .iter()
        .map(|item| {
            item.planning_latency_ms
                .saturating_add(item.retrieval_latency_ms)
        })
        .collect::<Vec<_>>();
    latencies.sort_unstable();
    Ok(MultiQueryBenchmarkMetrics {
        retrieval,
        multi_facet_file_recall_at_k,
        no_answer_false_positive_rate: false_positives as f64 / negative_count as f64,
        duplicate_rate: if ranked_count == 0 {
            0.0
        } else {
            duplicates as f64 / ranked_count as f64
        },
        average_query_count: observations
            .iter()
            .map(|item| item.query_count as f64)
            .sum::<f64>()
            / count,
        average_embedding_count: observations
            .iter()
            .map(|item| item.embedding_count as f64)
            .sum::<f64>()
            / count,
        average_selected_context_tokens: observations
            .iter()
            .map(|item| item.selected_context_tokens as f64)
            .sum::<f64>()
            / count,
        average_planner_input_tokens: observations
            .iter()
            .map(|item| item.planner_input_tokens as f64)
            .sum::<f64>()
            / count,
        average_planner_output_tokens: observations
            .iter()
            .map(|item| item.planner_output_tokens as f64)
            .sum::<f64>()
            / count,
        p50_planning_plus_retrieval_latency_ms: percentile(&latencies, 50),
        p95_planning_plus_retrieval_latency_ms: percentile(&latencies, 95),
        maximum_query_count: observations
            .iter()
            .map(|item| item.query_count)
            .max()
            .unwrap_or_default(),
        maximum_embedding_count: observations
            .iter()
            .map(|item| item.embedding_count)
            .max()
            .unwrap_or_default(),
        scope_violations: observations.iter().map(|item| item.scope_violations).sum(),
    })
}

fn percentile(sorted: &[u64], percentage: usize) -> u64 {
    let rank = percentage
        .saturating_mul(sorted.len())
        .div_ceil(100)
        .saturating_sub(1);
    sorted.get(rank).copied().unwrap_or_default()
}

fn benchmark_metrics_are_valid(metrics: &MultiQueryBenchmarkMetrics) -> bool {
    metrics_are_valid(&metrics.retrieval)
        && [
            metrics.multi_facet_file_recall_at_k,
            metrics.no_answer_false_positive_rate,
            metrics.duplicate_rate,
        ]
        .into_iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        && [
            metrics.average_query_count,
            metrics.average_embedding_count,
            metrics.average_selected_context_tokens,
            metrics.average_planner_input_tokens,
            metrics.average_planner_output_tokens,
        ]
        .into_iter()
        .all(|value| value.is_finite() && value >= 0.0)
        && (1..=4).contains(&metrics.maximum_query_count)
        && (1..=4).contains(&metrics.maximum_embedding_count)
}

impl EvaluationChangeReport {
    /// Validates that both runs are comparable and their metrics are bounded.
    pub fn validate(&self) -> Result<(), EvaluationError> {
        validate_id(&self.baseline_fingerprint)?;
        validate_id(&self.candidate_fingerprint)?;
        if self.baseline_fingerprint == self.candidate_fingerprint
            || self.migration_impact.trim().is_empty()
            || self.baseline.cutoff != self.candidate.cutoff
            || self.baseline.evaluated_cases != self.candidate.evaluated_cases
            || !metrics_are_valid(&self.baseline)
            || !metrics_are_valid(&self.candidate)
        {
            return Err(EvaluationError::InvalidChangeReport);
        }
        Ok(())
    }
}

/// Computes file recall, chunk recall, MRR, and binary-relevance nDCG at a fixed cutoff.
pub fn evaluate_retrieval(
    cases: &[EvaluationCase],
    observations: &[EvaluationObservation],
    cutoff: usize,
) -> Result<RetrievalMetrics, EvaluationError> {
    if cases.is_empty() || cases.len() > MAX_CASES || cutoff == 0 || cutoff > MAX_RESULTS_PER_CASE {
        return Err(EvaluationError::InvalidBounds);
    }
    let mut cases_by_id = BTreeMap::new();
    for case in cases {
        validate_case(case)?;
        if cases_by_id.insert(case.id.as_str(), case).is_some() {
            return Err(EvaluationError::DuplicateCase(case.id.clone()));
        }
    }
    let mut observations_by_case = BTreeMap::new();
    for observation in observations {
        validate_id(&observation.case_id)?;
        if !cases_by_id.contains_key(observation.case_id.as_str())
            || observation.ranked_file_ids.len() > MAX_RESULTS_PER_CASE
            || observation.ranked_chunk_ids.len() > MAX_RESULTS_PER_CASE
            || observation
                .ranked_file_ids
                .iter()
                .chain(&observation.ranked_chunk_ids)
                .any(|id| validate_id(id).is_err())
            || observations_by_case
                .insert(observation.case_id.as_str(), observation)
                .is_some()
        {
            return Err(EvaluationError::InvalidObservation(
                observation.case_id.clone(),
            ));
        }
    }

    let mut file_recall = 0.0;
    let mut chunk_recall = 0.0;
    let mut reciprocal_rank = 0.0;
    let mut ndcg = 0.0;
    for case in cases_by_id.values() {
        let observed = observations_by_case.get(case.id.as_str()).copied();
        let files = observed
            .map(|item| item.ranked_file_ids.as_slice())
            .unwrap_or_default();
        let chunks = observed
            .map(|item| item.ranked_chunk_ids.as_slice())
            .unwrap_or_default();
        file_recall += recall_at_k(files, &case.relevant_file_ids, cutoff);
        chunk_recall += recall_at_k(chunks, &case.relevant_chunk_ids, cutoff);
        reciprocal_rank += files
            .iter()
            .take(cutoff)
            .position(|id| case.relevant_file_ids.contains(id))
            .map_or(0.0, |index| 1.0 / (index + 1) as f64);
        ndcg += ndcg_at_k(files, &case.relevant_file_ids, cutoff);
    }
    let count = cases.len() as f64;
    Ok(RetrievalMetrics {
        evaluated_cases: cases.len(),
        cutoff,
        file_recall_at_k: file_recall / count,
        chunk_recall_at_k: chunk_recall / count,
        mean_reciprocal_rank: reciprocal_rank / count,
        ndcg_at_k: ndcg / count,
    })
}

fn recall_at_k(ranked: &[String], relevant: &BTreeSet<String>, cutoff: usize) -> f64 {
    if relevant.is_empty() {
        return 1.0;
    }
    let found = ranked
        .iter()
        .take(cutoff)
        .filter(|id| relevant.contains(*id))
        .collect::<BTreeSet<_>>()
        .len();
    found as f64 / relevant.len() as f64
}

fn ndcg_at_k(ranked: &[String], relevant: &BTreeSet<String>, cutoff: usize) -> f64 {
    if relevant.is_empty() {
        return 1.0;
    }
    let mut seen = BTreeSet::new();
    let dcg = ranked
        .iter()
        .take(cutoff)
        .enumerate()
        .filter(|(_, id)| relevant.contains(*id) && seen.insert(id.as_str()))
        .map(|(index, _)| 1.0 / ((index + 2) as f64).log2())
        .sum::<f64>();
    let ideal = (0..relevant.len().min(cutoff))
        .map(|index| 1.0 / ((index + 2) as f64).log2())
        .sum::<f64>();
    dcg / ideal
}

fn metrics_are_valid(metrics: &RetrievalMetrics) -> bool {
    metrics.evaluated_cases > 0
        && metrics.cutoff > 0
        && metrics.cutoff <= MAX_RESULTS_PER_CASE
        && [
            metrics.file_recall_at_k,
            metrics.chunk_recall_at_k,
            metrics.mean_reciprocal_rank,
            metrics.ndcg_at_k,
        ]
        .into_iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
}

fn validate_case(case: &EvaluationCase) -> Result<(), EvaluationError> {
    validate_id(&case.id)?;
    if case.query.trim().is_empty()
        || case.query.len() > 32 * 1024
        || (case.relevant_file_ids.is_empty()
            && case.relevant_chunk_ids.is_empty()
            && !case.expected_no_answer)
        || (case.expected_no_answer
            && (!case.relevant_file_ids.is_empty() || !case.relevant_chunk_ids.is_empty()))
        || case.relevant_file_ids.len() > MAX_RESULTS_PER_CASE
        || case.relevant_chunk_ids.len() > MAX_RESULTS_PER_CASE
        || case
            .relevant_file_ids
            .iter()
            .chain(&case.relevant_chunk_ids)
            .any(|id| validate_id(id).is_err())
    {
        return Err(EvaluationError::InvalidCase(case.id.clone()));
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<(), EvaluationError> {
    if value.is_empty() || value.len() > 256 || value.contains(char::is_whitespace) {
        Err(EvaluationError::InvalidIdentifier)
    } else {
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvaluationDocument {
    schema_version: u32,
    cases: Vec<EvaluationCase>,
}

/// Atomic local persistence for evaluation judgments.
pub struct EvaluationStore {
    path: PathBuf,
    cases: Mutex<Vec<EvaluationCase>>,
}

impl EvaluationStore {
    /// Opens a bounded versioned local evaluation document, or an empty store when absent.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, EvaluationError> {
        let path = path.into();
        let cases = match fs::read(&path) {
            Ok(bytes) => {
                if bytes.len() > MAX_EVALUATION_BYTES {
                    return Err(EvaluationError::InvalidBounds);
                }
                let document: EvaluationDocument = serde_json::from_slice(&bytes)?;
                if !matches!(document.schema_version, 1 | EVALUATION_SCHEMA_VERSION) {
                    return Err(EvaluationError::UnsupportedSchema(document.schema_version));
                }
                validate_cases(document.cases)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            cases: Mutex::new(cases),
        })
    }

    /// Atomically replaces all local cases after validation and deterministic sorting.
    pub fn replace(&self, mut cases: Vec<EvaluationCase>) -> Result<(), EvaluationError> {
        cases.sort_by(|left, right| left.id.cmp(&right.id));
        let cases = validate_cases(cases)?;
        let document = EvaluationDocument {
            schema_version: EVALUATION_SCHEMA_VERSION,
            cases: cases.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&document)?;
        if bytes.len() > MAX_EVALUATION_BYTES {
            return Err(EvaluationError::InvalidBounds);
        }
        persist_atomic(&self.path, &bytes)?;
        *self.lock()? = cases;
        Ok(())
    }

    /// Returns the current local cases.
    pub fn cases(&self) -> Result<Vec<EvaluationCase>, EvaluationError> {
        Ok(self.lock()?.clone())
    }

    fn lock(&self) -> Result<MutexGuard<'_, Vec<EvaluationCase>>, EvaluationError> {
        self.cases.lock().map_err(|_| EvaluationError::LockPoisoned)
    }
}

fn validate_cases(cases: Vec<EvaluationCase>) -> Result<Vec<EvaluationCase>, EvaluationError> {
    if cases.is_empty() || cases.len() > MAX_CASES {
        return Err(EvaluationError::InvalidBounds);
    }
    let mut ids = BTreeSet::new();
    for case in &cases {
        validate_case(case)?;
        if !ids.insert(case.id.as_str()) {
            return Err(EvaluationError::DuplicateCase(case.id.clone()));
        }
    }
    Ok(cases)
}

fn persist_atomic(path: &Path, bytes: &[u8]) -> Result<(), EvaluationError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[derive(Debug, Error)]
/// Failure while validating, scoring, or persisting local evaluation data.
pub enum EvaluationError {
    /// Case, result, cutoff, or document limits were exceeded.
    #[error("evaluation bounds are invalid")]
    InvalidBounds,
    /// An opaque identity was empty, overlong, or contained whitespace.
    #[error("evaluation identifier is invalid")]
    InvalidIdentifier,
    /// A case lacked a query or relevant identity.
    #[error("evaluation case `{0}` is invalid")]
    InvalidCase(String),
    /// The local suite contained the same case identity more than once.
    #[error("evaluation case `{0}` occurs more than once")]
    DuplicateCase(String),
    /// An observation was malformed, duplicated, or did not match a case.
    #[error("evaluation observation `{0}` is invalid")]
    InvalidObservation(String),
    /// Baseline and candidate evidence cannot be compared safely.
    #[error("before/after evaluation report is incomplete or incomparable")]
    InvalidChangeReport,
    /// The local document uses an unsupported schema.
    #[error("evaluation schema version `{0}` is unsupported")]
    UnsupportedSchema(u32),
    /// Another thread poisoned the in-memory store lock.
    #[error("evaluation store lock is poisoned")]
    LockPoisoned,
    /// Filesystem persistence failed.
    #[error("evaluation persistence failed: {0}")]
    Io(#[from] std::io::Error),
    /// Evaluation JSON could not be parsed or encoded.
    #[error("evaluation JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BenchmarkFixture {
        cutoff: usize,
        maximum_p95_latency_ms: u64,
        production_measurement: bool,
        baseline: Vec<BenchmarkObservation>,
        candidate: Vec<BenchmarkObservation>,
    }

    fn case(id: &str, query: &str, files: &[&str], chunks: &[&str]) -> EvaluationCase {
        EvaluationCase {
            id: id.into(),
            query: query.into(),
            relevant_file_ids: files.iter().map(ToString::to_string).collect(),
            relevant_chunk_ids: chunks.iter().map(ToString::to_string).collect(),
            category: EvaluationCaseCategory::Other,
            expected_no_answer: false,
        }
    }

    fn benchmark_observation(
        case_id: &str,
        files: &[&str],
        chunks: &[&str],
        query_count: usize,
        latency_ms: u64,
    ) -> BenchmarkObservation {
        BenchmarkObservation {
            retrieval: EvaluationObservation {
                case_id: case_id.into(),
                ranked_file_ids: files.iter().map(ToString::to_string).collect(),
                ranked_chunk_ids: chunks.iter().map(ToString::to_string).collect(),
            },
            query_count,
            embedding_count: query_count,
            selected_context_tokens: 128,
            planner_input_tokens: usize::from(query_count > 1) * 20,
            planner_output_tokens: usize::from(query_count > 1) * 12,
            planning_latency_ms: u64::from(query_count > 1) * 10,
            retrieval_latency_ms: latency_ms,
            scope_violations: 0,
        }
    }

    #[test]
    fn bounded_candidate_gets_go_only_from_comparable_quality_and_safety_metrics() {
        let cases = vec![
            EvaluationCase {
                category: EvaluationCaseCategory::MultiFacet,
                ..case(
                    "multi-facet",
                    "compare alpha and beta",
                    &["alpha", "beta"],
                    &["alpha-chunk", "beta-chunk"],
                )
            },
            EvaluationCase {
                category: EvaluationCaseCategory::NegativeControl,
                expected_no_answer: true,
                ..case("negative", "unrelated control", &[], &[])
            },
        ];
        let baseline = vec![
            benchmark_observation("multi-facet", &["alpha"], &["alpha-chunk"], 1, 20),
            benchmark_observation("negative", &[], &[], 1, 20),
        ];
        let candidate = vec![
            benchmark_observation(
                "multi-facet",
                &["alpha", "beta"],
                &["alpha-chunk", "beta-chunk"],
                3,
                50,
            ),
            benchmark_observation("negative", &[], &[], 3, 40),
        ];

        let comparison =
            evaluate_multi_query_benchmark(&cases, &baseline, &candidate, 10, 100, true)
                .expect("comparable benchmark");

        assert_eq!(comparison.decision, MultiQueryBenchmarkDecision::Go);
        assert_eq!(comparison.candidate.scope_violations, 0);
        assert!(
            comparison.candidate.multi_facet_file_recall_at_k
                - comparison.baseline.multi_facet_file_recall_at_k
                >= 0.10
        );
    }

    #[test]
    fn metrics_handle_interleaved_unsorted_cases_and_missing_results() {
        let cases = vec![
            case("case-b", "Nederlandse term", &["file-b"], &["chunk-b"]),
            case(
                "case-a",
                "duplicate boilerplate",
                &["file-a", "file-c"],
                &["chunk-a"],
            ),
            case("case-c", "unavailable source", &["file-z"], &["chunk-z"]),
        ];
        let observations = vec![
            EvaluationObservation {
                case_id: "case-a".into(),
                ranked_file_ids: vec!["noise".into(), "file-c".into(), "file-a".into()],
                ranked_chunk_ids: vec!["chunk-a".into()],
            },
            EvaluationObservation {
                case_id: "case-b".into(),
                ranked_file_ids: vec!["file-b".into()],
                ranked_chunk_ids: vec!["noise".into(), "chunk-b".into()],
            },
        ];

        let metrics = evaluate_retrieval(&cases, &observations, 2).expect("metrics");
        assert_eq!(metrics.evaluated_cases, 3);
        assert!((metrics.file_recall_at_k - 0.5).abs() < 0.000_001);
        assert!((metrics.chunk_recall_at_k - (2.0 / 3.0)).abs() < 0.000_001);
        assert!((metrics.mean_reciprocal_rank - 0.5).abs() < 0.000_001);
        assert!(metrics.ndcg_at_k > 0.46 && metrics.ndcg_at_k < 0.47);
    }

    #[test]
    fn duplicate_ranked_results_do_not_inflate_ndcg() {
        let metrics = evaluate_retrieval(
            &[case("case-a", "query", &["file-a", "file-b"], &[])],
            &[EvaluationObservation {
                case_id: "case-a".into(),
                ranked_file_ids: vec!["file-a".into(), "file-a".into()],
                ranked_chunk_ids: Vec::new(),
            }],
            2,
        )
        .expect("metrics");

        assert!(metrics.ndcg_at_k < 0.62);
    }

    #[test]
    fn local_suite_round_trips_without_derived_results() {
        let directory = tempfile::tempdir().expect("temporary evaluation store");
        let path = directory.path().join("evaluation/cases.json");
        let store = EvaluationStore::open(&path).expect("open");
        let expected = vec![case("case-a", "semantic query", &["file-a"], &["chunk-a"])];
        store.replace(expected.clone()).expect("save");
        store.replace(expected.clone()).expect("replace atomically");

        assert_eq!(
            EvaluationStore::open(path).unwrap().cases().unwrap(),
            expected
        );
    }

    #[test]
    fn change_reports_require_distinct_fingerprints_and_explicit_impacts() {
        let metrics = RetrievalMetrics {
            evaluated_cases: 1,
            cutoff: 10,
            file_recall_at_k: 1.0,
            chunk_recall_at_k: 1.0,
            mean_reciprocal_rank: 1.0,
            ndcg_at_k: 1.0,
        };
        let valid = EvaluationChangeReport {
            baseline_fingerprint: "baseline-v1".into(),
            candidate_fingerprint: "candidate-v2".into(),
            migration_impact: "full vector rebuild".into(),
            storage_impact_bytes: 1024,
            baseline: metrics.clone(),
            candidate: metrics,
        };
        valid.validate().expect("complete report");
        assert!(matches!(
            EvaluationChangeReport {
                candidate_fingerprint: "baseline-v1".into(),
                ..valid
            }
            .validate(),
            Err(EvaluationError::InvalidChangeReport)
        ));
    }

    #[test]
    fn observations_for_unknown_cases_cannot_skew_a_report() {
        let error = evaluate_retrieval(
            &[case("case-a", "query", &["file-a"], &[])],
            &[EvaluationObservation {
                case_id: "case-b".into(),
                ranked_file_ids: vec!["file-a".into()],
                ranked_chunk_ids: Vec::new(),
            }],
            10,
        )
        .expect_err("unknown observations must be rejected");
        assert!(matches!(error, EvaluationError::InvalidObservation(id) if id == "case-b"));
    }

    #[test]
    fn malformed_ranked_identifiers_are_rejected() {
        let error = evaluate_retrieval(
            &[case("case-a", "query", &["file-a"], &[])],
            &[EvaluationObservation {
                case_id: "case-a".into(),
                ranked_file_ids: vec!["private path/file-a".into()],
                ranked_chunk_ids: Vec::new(),
            }],
            10,
        )
        .expect_err("ranked identifiers must be opaque");
        assert!(matches!(error, EvaluationError::InvalidObservation(id) if id == "case-a"));
    }

    #[test]
    fn repository_fixture_covers_the_release_gate_scenarios() {
        let document: EvaluationDocument = serde_json::from_str(include_str!(
            "../tests/fixtures/semantic-evaluation-v1.json"
        ))
        .expect("evaluation fixture");
        assert_eq!(document.schema_version, EVALUATION_SCHEMA_VERSION);
        assert_eq!(document.cases.len(), 12);
        let ids = document
            .cases
            .iter()
            .map(|case| case.id.as_str())
            .collect::<BTreeSet<_>>();
        for required in [
            "multilingual-recall",
            "near-duplicate-grouping",
            "boilerplate-diversity",
            "structural-citation",
            "incremental-edit",
            "summary-discovery",
            "scope-isolation",
            "unavailable-source",
            "concept-label",
            "multi-facet-comparison",
            "negative-control",
            "prompt-injection-shaped-query",
        ] {
            assert!(ids.contains(required));
        }
    }

    #[test]
    fn repository_multi_query_fixture_records_no_go_without_production_measurement() {
        let evaluation: EvaluationDocument = serde_json::from_str(include_str!(
            "../tests/fixtures/semantic-evaluation-v1.json"
        ))
        .expect("evaluation fixture");
        let benchmark: BenchmarkFixture = serde_json::from_str(include_str!(
            "../tests/fixtures/multi-query-rag-benchmark-v1.json"
        ))
        .expect("benchmark fixture");

        let comparison = evaluate_multi_query_benchmark(
            &evaluation.cases,
            &benchmark.baseline,
            &benchmark.candidate,
            benchmark.cutoff,
            benchmark.maximum_p95_latency_ms,
            benchmark.production_measurement,
        )
        .expect("comparable fixture");

        let report: MultiQueryBenchmarkReport = serde_json::from_str(include_str!(
            "../../../docs/evaluations/multi-query-rag-v1.json"
        ))
        .expect("checked-in report");
        report.validate().expect("valid checked-in report");
        assert_eq!(report.comparison, comparison);
        assert_eq!(comparison.decision, MultiQueryBenchmarkDecision::NoGo);
        assert_eq!(comparison.candidate.maximum_query_count, 3);
        assert_eq!(comparison.candidate.scope_violations, 0);
        assert_eq!(comparison.candidate.no_answer_false_positive_rate, 0.0);
    }
}
