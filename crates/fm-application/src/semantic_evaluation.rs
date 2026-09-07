//! Local, deterministic retrieval evaluation for semantic release gates.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const EVALUATION_SCHEMA_VERSION: u32 = 1;
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
}

/// Ranked output captured from one retrieval run.
#[derive(Debug, Clone, PartialEq, Eq)]
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
        || (case.relevant_file_ids.is_empty() && case.relevant_chunk_ids.is_empty())
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
                if document.schema_version != EVALUATION_SCHEMA_VERSION {
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

    fn case(id: &str, query: &str, files: &[&str], chunks: &[&str]) -> EvaluationCase {
        EvaluationCase {
            id: id.into(),
            query: query.into(),
            relevant_file_ids: files.iter().map(ToString::to_string).collect(),
            relevant_chunk_ids: chunks.iter().map(ToString::to_string).collect(),
        }
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
        assert_eq!(document.cases.len(), 9);
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
        ] {
            assert!(ids.contains(required));
        }
    }
}
