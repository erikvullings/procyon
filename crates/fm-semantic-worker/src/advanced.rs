//! Optional acceleration, reranking, and hybrid-retrieval contracts.
//!
//! These types are deliberately separate from dense semantic retrieval. No
//! caller enters an advanced path without selecting and configuring it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use thiserror::Error;
use tokio_util::sync::CancellationToken;

const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_RERANK_TEXT_BYTES: usize = 32 * 1024;
const MAX_ADVANCED_CANDIDATES: usize = 1_000;
const MINIMUM_RERANK_NDCG_GAIN: f64 = 0.02;

/// Local embedding implementation used by either CPU or accelerated execution.
pub trait AccelerationBackend: Send + Sync {
    /// Embeds an already bounded batch without network access.
    fn embed(
        &self,
        inputs: &[String],
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, String>;
}

/// Policy proving an accelerated backend shares the active vector space.
#[derive(Debug, Clone, PartialEq)]
pub struct AccelerationPolicy {
    maximum_absolute_difference: f32,
    migration: Option<EmbeddingSpaceMigration>,
}

impl AccelerationPolicy {
    /// Requires every accelerated coordinate to match CPU within `tolerance`.
    pub fn parity_required(tolerance: f32) -> Self {
        Self {
            maximum_absolute_difference: tolerance,
            migration: None,
        }
    }

    /// Declares that accelerated output requires a distinct rebuilt index.
    pub fn migration_declared(
        baseline_fingerprint: impl Into<String>,
        candidate_fingerprint: impl Into<String>,
    ) -> Result<Self, AdvancedSemanticError> {
        let migration = EmbeddingSpaceMigration {
            baseline_fingerprint: baseline_fingerprint.into(),
            candidate_fingerprint: candidate_fingerprint.into(),
        };
        if !is_safe_identifier(&migration.baseline_fingerprint)
            || !is_safe_identifier(&migration.candidate_fingerprint)
            || migration.baseline_fingerprint == migration.candidate_fingerprint
        {
            return Err(AdvancedSemanticError::InvalidPolicy);
        }
        Ok(Self {
            maximum_absolute_difference: 0.0,
            migration: Some(migration),
        })
    }

    fn validate(&self) -> Result<(), AdvancedSemanticError> {
        if !self.maximum_absolute_difference.is_finite()
            || self.maximum_absolute_difference < 0.0
            || self.maximum_absolute_difference > 0.1
        {
            return Err(AdvancedSemanticError::InvalidPolicy);
        }
        Ok(())
    }
}

/// Explicit model-space migration required before accelerated vectors publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingSpaceMigration {
    /// Existing CPU baseline fingerprint.
    pub baseline_fingerprint: String,
    /// New accelerated model-space fingerprint.
    pub candidate_fingerprint: String,
}

/// One embedding result and whether driver/runtime failure selected CPU.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceleratedEmbeddingResult {
    /// Validated vectors.
    pub vectors: Vec<Vec<f32>>,
    /// Whether the optional backend failed and CPU produced this result.
    pub used_cpu_fallback: bool,
    /// Distinct model space requiring a rebuilt, separately activated index.
    pub model_space_migration: Option<EmbeddingSpaceMigration>,
}

/// Optional accelerated embedding with one-time parity proof and safe CPU fallback.
pub struct AcceleratedEmbedding {
    cpu: Arc<dyn AccelerationBackend>,
    accelerated: Option<Arc<dyn AccelerationBackend>>,
    policy: AccelerationPolicy,
    parity_verified: AtomicBool,
}

impl AcceleratedEmbedding {
    /// Creates a CPU-first capability. An absent accelerator is valid.
    pub fn new(
        cpu: Arc<dyn AccelerationBackend>,
        accelerated: Option<Arc<dyn AccelerationBackend>>,
        policy: AccelerationPolicy,
    ) -> Result<Self, AdvancedSemanticError> {
        policy.validate()?;
        Ok(Self {
            cpu,
            accelerated,
            policy,
            parity_verified: AtomicBool::new(false),
        })
    }

    /// Embeds locally, falling back on runtime failure and rejecting silent drift.
    pub fn embed(
        &self,
        inputs: &[String],
        cancellation: &CancellationToken,
    ) -> Result<AcceleratedEmbeddingResult, AdvancedSemanticError> {
        if cancellation.is_cancelled() {
            return Err(AdvancedSemanticError::Cancelled);
        }
        let Some(accelerated) = &self.accelerated else {
            return self.cpu_result(inputs, cancellation, false);
        };
        let accelerated_vectors = match accelerated.embed(inputs, cancellation) {
            Ok(vectors) => validate_vectors(vectors, inputs.len())?,
            Err(_) => return self.cpu_result(inputs, cancellation, true),
        };
        if self.policy.migration.is_none() && !self.parity_verified.load(Ordering::Acquire) {
            let cpu_vectors = validate_vectors(
                self.cpu
                    .embed(inputs, cancellation)
                    .map_err(AdvancedSemanticError::Embedding)?,
                inputs.len(),
            )?;
            ensure_vector_parity(
                &cpu_vectors,
                &accelerated_vectors,
                self.policy.maximum_absolute_difference,
            )?;
            self.parity_verified.store(true, Ordering::Release);
        }
        if cancellation.is_cancelled() {
            return Err(AdvancedSemanticError::Cancelled);
        }
        Ok(AcceleratedEmbeddingResult {
            vectors: accelerated_vectors,
            used_cpu_fallback: false,
            model_space_migration: self.policy.migration.clone(),
        })
    }

    fn cpu_result(
        &self,
        inputs: &[String],
        cancellation: &CancellationToken,
        used_cpu_fallback: bool,
    ) -> Result<AcceleratedEmbeddingResult, AdvancedSemanticError> {
        let vectors = validate_vectors(
            self.cpu
                .embed(inputs, cancellation)
                .map_err(AdvancedSemanticError::Embedding)?,
            inputs.len(),
        )?;
        if cancellation.is_cancelled() {
            return Err(AdvancedSemanticError::Cancelled);
        }
        Ok(AcceleratedEmbeddingResult {
            vectors,
            used_cpu_fallback,
            model_space_migration: None,
        })
    }
}

fn validate_vectors(
    vectors: Vec<Vec<f32>>,
    expected: usize,
) -> Result<Vec<Vec<f32>>, AdvancedSemanticError> {
    let dimensions = vectors.first().map(Vec::len).unwrap_or_default();
    if vectors.len() != expected
        || dimensions == 0
        || vectors.iter().any(|vector| {
            vector.len() != dimensions || vector.iter().any(|value| !value.is_finite())
        })
    {
        return Err(AdvancedSemanticError::MalformedEmbedding);
    }
    Ok(vectors)
}

fn ensure_vector_parity(
    cpu: &[Vec<f32>],
    accelerated: &[Vec<f32>],
    tolerance: f32,
) -> Result<(), AdvancedSemanticError> {
    if cpu.len() != accelerated.len()
        || cpu.iter().zip(accelerated).any(|(left, right)| {
            left.len() != right.len()
                || left
                    .iter()
                    .zip(right)
                    .any(|(a, b)| (a - b).abs() > tolerance)
        })
    {
        return Err(AdvancedSemanticError::AccelerationParity { tolerance });
    }
    Ok(())
}

/// One dense candidate passed to a local reranker.
#[derive(Debug, Clone, PartialEq)]
pub struct RerankCandidate {
    /// Stable candidate identity.
    pub id: String,
    /// Dense retrieval score retained for diagnostics and fallback.
    pub dense_score: f32,
    /// Bounded local text presented to the reranker.
    pub text: String,
}

impl RerankCandidate {
    /// Creates a validated candidate without paths or provider metadata.
    pub fn new(
        id: impl Into<String>,
        dense_score: f32,
        text: impl Into<String>,
    ) -> Result<Self, AdvancedSemanticError> {
        let candidate = Self {
            id: id.into(),
            dense_score,
            text: text.into(),
        };
        if !is_safe_identifier(&candidate.id)
            || !candidate.dense_score.is_finite()
            || candidate.text.is_empty()
            || candidate.text.len() > MAX_RERANK_TEXT_BYTES
        {
            return Err(AdvancedSemanticError::MalformedCandidate);
        }
        Ok(candidate)
    }
}

/// Optional local reranker backend.
pub trait RerankerBackend: Send + Sync {
    /// Exact immutable model-space identity.
    fn model_fingerprint(&self) -> &str;

    /// Returns every candidate identity exactly once in preferred order with a finite score.
    fn rerank(
        &self,
        candidates: &[RerankCandidate],
        cancellation: &CancellationToken,
    ) -> Result<Vec<(String, f32)>, String>;
}

/// Evaluation and resource gate applied before a reranker is usable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RerankerPolicy {
    maximum_candidates: usize,
    minimum_ndcg_gain: f64,
    baseline_ndcg: f64,
    candidate_ndcg: f64,
}

impl RerankerPolicy {
    /// Creates a gate from a comparable task-0188 baseline and candidate report.
    pub fn new(
        maximum_candidates: usize,
        minimum_ndcg_gain: f64,
        baseline_ndcg: f64,
        candidate_ndcg: f64,
    ) -> Result<Self, AdvancedSemanticError> {
        let policy = Self {
            maximum_candidates,
            minimum_ndcg_gain,
            baseline_ndcg,
            candidate_ndcg,
        };
        if maximum_candidates == 0
            || maximum_candidates > MAX_ADVANCED_CANDIDATES
            || ![minimum_ndcg_gain, baseline_ndcg, candidate_ndcg]
                .into_iter()
                .all(f64::is_finite)
            || minimum_ndcg_gain < MINIMUM_RERANK_NDCG_GAIN
            || !(0.0..=1.0).contains(&baseline_ndcg)
            || !(0.0..=1.0).contains(&candidate_ndcg)
            || candidate_ndcg - baseline_ndcg < minimum_ndcg_gain
        {
            return Err(AdvancedSemanticError::QualityGate);
        }
        Ok(policy)
    }
}

/// Measured local cost of one reranking request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RerankCost {
    /// Exact reranker model identity.
    pub model_fingerprint: String,
    /// Candidate count processed.
    pub candidate_count: usize,
    /// Total bounded input characters.
    pub input_characters: usize,
    /// Wall-clock latency.
    pub latency_micros: u128,
}

/// Validated reranked output.
#[derive(Debug, Clone, PartialEq)]
pub struct RerankResult {
    /// Candidates in reranker order with reranker scores.
    pub candidates: Vec<RerankedCandidate>,
    /// Measured local cost.
    pub cost: RerankCost,
}

/// One reranked identity.
#[derive(Debug, Clone, PartialEq)]
pub struct RerankedCandidate {
    /// Stable candidate identity.
    pub id: String,
    /// Finite reranker score.
    pub score: f32,
}

/// Runs a quality-gated reranker over a bounded candidate set.
pub fn rerank_candidates(
    backend: &dyn RerankerBackend,
    policy: &RerankerPolicy,
    candidates: &[RerankCandidate],
    cancellation: &CancellationToken,
) -> Result<RerankResult, AdvancedSemanticError> {
    if cancellation.is_cancelled() {
        return Err(AdvancedSemanticError::Cancelled);
    }
    if candidates.is_empty() || candidates.len() > policy.maximum_candidates {
        return Err(AdvancedSemanticError::CandidateLimit {
            maximum: policy.maximum_candidates,
            actual: candidates.len(),
        });
    }
    if !is_safe_identifier(backend.model_fingerprint()) {
        return Err(AdvancedSemanticError::MalformedCandidate);
    }
    let expected = candidates
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect::<BTreeSet<_>>();
    if expected.len() != candidates.len() {
        return Err(AdvancedSemanticError::MalformedCandidate);
    }
    let started = Instant::now();
    let ranked = backend
        .rerank(candidates, cancellation)
        .map_err(AdvancedSemanticError::Reranker)?;
    if cancellation.is_cancelled() {
        return Err(AdvancedSemanticError::Cancelled);
    }
    let actual = ranked
        .iter()
        .map(|(id, _)| id.as_str())
        .collect::<BTreeSet<_>>();
    if actual != expected
        || actual.len() != ranked.len()
        || ranked.iter().any(|(_, score)| !score.is_finite())
    {
        return Err(AdvancedSemanticError::MalformedRerankerOutput);
    }
    Ok(RerankResult {
        candidates: ranked
            .into_iter()
            .map(|(id, score)| RerankedCandidate { id, score })
            .collect(),
        cost: RerankCost {
            model_fingerprint: backend.model_fingerprint().to_owned(),
            candidate_count: candidates.len(),
            input_characters: candidates
                .iter()
                .map(|candidate| candidate.text.chars().count())
                .sum(),
            latency_micros: started.elapsed().as_micros(),
        },
    })
}

/// Tenant and library authority attached to every hybrid candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedRetrieval {
    tenant_id: String,
    library_id: String,
}

impl ScopedRetrieval {
    /// Creates an opaque retrieval scope.
    pub fn new(
        tenant_id: impl Into<String>,
        library_id: impl Into<String>,
    ) -> Result<Self, AdvancedSemanticError> {
        let scope = Self {
            tenant_id: tenant_id.into(),
            library_id: library_id.into(),
        };
        if !is_safe_identifier(&scope.tenant_id) || !is_safe_identifier(&scope.library_id) {
            return Err(AdvancedSemanticError::ScopeMismatch);
        }
        Ok(scope)
    }
}

/// One independently retrieved dense or lexical candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct HybridCandidate {
    scope: ScopedRetrieval,
    id: String,
    source_score: f32,
    filter_match: bool,
}

impl HybridCandidate {
    /// Creates a scoped candidate after source-specific retrieval and filtering.
    pub fn new(
        scope: &ScopedRetrieval,
        id: impl Into<String>,
        source_score: f32,
        filter_match: bool,
    ) -> Result<Self, AdvancedSemanticError> {
        let candidate = Self {
            scope: scope.clone(),
            id: id.into(),
            source_score,
            filter_match,
        };
        if !is_safe_identifier(&candidate.id) || !source_score.is_finite() {
            return Err(AdvancedSemanticError::MalformedCandidate);
        }
        Ok(candidate)
    }
}

/// Explicit weighted reciprocal-rank-fusion policy for Hybrid mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HybridFusionPolicy {
    dense_weight: f64,
    lexical_weight: f64,
    rank_constant: u32,
    maximum_candidates: usize,
}

impl HybridFusionPolicy {
    /// Creates a bounded deterministic fusion policy.
    pub fn new(
        dense_weight: f64,
        lexical_weight: f64,
        rank_constant: u32,
        maximum_candidates: usize,
    ) -> Result<Self, AdvancedSemanticError> {
        if !dense_weight.is_finite()
            || !lexical_weight.is_finite()
            || dense_weight < 0.0
            || lexical_weight < 0.0
            || dense_weight + lexical_weight == 0.0
            || rank_constant == 0
            || maximum_candidates == 0
            || maximum_candidates > MAX_ADVANCED_CANDIDATES
        {
            return Err(AdvancedSemanticError::InvalidPolicy);
        }
        Ok(Self {
            dense_weight,
            lexical_weight,
            rank_constant,
            maximum_candidates,
        })
    }
}

/// One fused Hybrid-mode result.
#[derive(Debug, Clone, PartialEq)]
pub struct HybridResult {
    /// Stable candidate identity.
    pub id: String,
    /// Weighted reciprocal-rank-fusion score.
    pub score: f64,
}

/// Explicitly named Hybrid-mode output.
#[derive(Debug, Clone, PartialEq)]
pub struct HybridSearchResult {
    /// Stable mode name that cannot be confused with dense Semantic mode.
    pub mode: &'static str,
    /// Deterministically fused candidates.
    pub results: Vec<HybridResult>,
}

/// Fuses separately filtered dense and lexical rankings with weighted RRF.
pub fn fuse_hybrid(
    scope: &ScopedRetrieval,
    dense: &[HybridCandidate],
    lexical: &[HybridCandidate],
    limit: usize,
    policy: HybridFusionPolicy,
) -> Result<HybridSearchResult, AdvancedSemanticError> {
    if limit == 0
        || limit > policy.maximum_candidates
        || dense.len() > policy.maximum_candidates
        || lexical.len() > policy.maximum_candidates
    {
        return Err(AdvancedSemanticError::CandidateLimit {
            maximum: policy.maximum_candidates,
            actual: dense.len().max(lexical.len()).max(limit),
        });
    }
    if dense
        .iter()
        .chain(lexical)
        .any(|candidate| candidate.scope != *scope)
    {
        return Err(AdvancedSemanticError::ScopeMismatch);
    }
    let mut scores = BTreeMap::<String, f64>::new();
    add_rrf_scores(
        &mut scores,
        dense,
        policy.dense_weight,
        policy.rank_constant,
    );
    add_rrf_scores(
        &mut scores,
        lexical,
        policy.lexical_weight,
        policy.rank_constant,
    );
    let mut results = scores
        .into_iter()
        .map(|(id, score)| HybridResult { id, score })
        .collect::<Vec<_>>();
    results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.id.cmp(&right.id))
    });
    results.truncate(limit);
    Ok(HybridSearchResult {
        mode: "hybrid",
        results,
    })
}

fn add_rrf_scores(
    scores: &mut BTreeMap<String, f64>,
    candidates: &[HybridCandidate],
    weight: f64,
    rank_constant: u32,
) {
    let mut seen = BTreeSet::new();
    let mut accepted_rank = 0_u32;
    for candidate in candidates {
        if candidate.filter_match && seen.insert(candidate.id.as_str()) {
            accepted_rank += 1;
            *scores.entry(candidate.id.clone()).or_default() +=
                weight / (f64::from(rank_constant) + f64::from(accepted_rank));
        }
    }
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-._:".contains(character))
}

/// Failure from an optional advanced semantic capability.
#[derive(Debug, Error, PartialEq)]
pub enum AdvancedSemanticError {
    /// The explicit policy contains unsafe or meaningless bounds.
    #[error("advanced semantic policy is invalid")]
    InvalidPolicy,
    /// The request was cancelled.
    #[error("advanced semantic operation was cancelled")]
    Cancelled,
    /// A local embedding backend failed.
    #[error("local embedding backend failed: {0}")]
    Embedding(String),
    /// A backend returned invalid dimensions or non-finite values.
    #[error("embedding backend returned malformed vectors")]
    MalformedEmbedding,
    /// Accelerated vectors differ from the CPU baseline.
    #[error("accelerated embedding differs from CPU by more than {tolerance}")]
    AccelerationParity {
        /// Maximum accepted absolute coordinate difference.
        tolerance: f32,
    },
    /// Candidate count exceeded its explicit bound.
    #[error("advanced candidate count {actual} exceeds maximum {maximum}")]
    CandidateLimit {
        /// Configured maximum.
        maximum: usize,
        /// Submitted count or limit.
        actual: usize,
    },
    /// A candidate identity, score, or text was malformed.
    #[error("advanced candidate is malformed")]
    MalformedCandidate,
    /// The task-0188 quality gate was not met.
    #[error("reranker did not demonstrate the required evaluation gain")]
    QualityGate,
    /// The local reranker backend failed.
    #[error("local reranker failed: {0}")]
    Reranker(String),
    /// The reranker dropped, duplicated, invented, or mis-scored a candidate.
    #[error("local reranker returned malformed output")]
    MalformedRerankerOutput,
    /// A candidate belonged to another tenant or library.
    #[error("hybrid candidate does not belong to the authorized scope")]
    ScopeMismatch,
}
