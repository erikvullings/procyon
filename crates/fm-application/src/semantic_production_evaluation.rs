//! Fail-closed evidence for exact-production semantic release evaluation.

use std::collections::{BTreeMap, BTreeSet};

use fm_semantic_components::{ProductionPipelineIdentity, production_pipeline_identity};
use fm_semantic_worker::rag_retrieval::RagRetrievalPolicy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::semantic_evaluation::{
    EvaluationCase, EvaluationObservation, RetrievalMetrics, evaluate_retrieval,
};

/// Version of the checked-in production corpus.
pub const PRODUCTION_CORPUS_SCHEMA_VERSION: u32 = 2;
/// Version of the release evidence report.
pub const PRODUCTION_REPORT_VERSION: u32 = 1;
/// Fixed retrieval cutoff required by task 0198.
pub const PRODUCTION_RETRIEVAL_CUTOFF: usize = 10;

const MINIMUM_FILE_RECALL: f64 = 0.90;
const MINIMUM_CHUNK_RECALL: f64 = 0.80;
const MINIMUM_MRR: f64 = 0.80;
const MINIMUM_NDCG: f64 = 0.80;
const SUPPORTED_TARGETS: [&str; 4] = [
    "linux-aarch64",
    "linux-x86_64",
    "macos-aarch64",
    "windows-x86_64",
];
const REQUIRED_MANUAL_CRITERIA: [&str; 6] = [
    "accessibility",
    "failure-modes",
    "generated-answer-grounding",
    "installed-lifecycle",
    "privacy",
    "release-owner-approval",
];
const REQUIRED_COMPONENTS: [&str; 3] = [
    "procyon.semantic.model.multilingual-e5-small",
    "procyon.semantic.worker",
    "procyon.semantic.zvec-runtime",
];

/// One repository-owned input document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionCorpusDocument {
    /// Stable logical file identity used by the task-0188 labels.
    pub id: String,
    /// Stable source identity used for citation opening.
    pub source_id: String,
    /// Whether this document belongs to the query's authorized tenant.
    pub scope: CorpusScope,
    /// Exact media type sent to the packaged worker.
    pub media_type: String,
    /// Deterministic source generator.
    pub source: CorpusSource,
    /// Previous source generation ingested before the current bytes, when applicable.
    #[serde(default)]
    pub previous_source: Option<CorpusSource>,
    /// Logical labels for every expected converted chunk.
    pub chunks: Vec<CorpusChunk>,
}

/// Authorization boundary represented by a corpus document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CorpusScope {
    /// Visible to the evaluation query.
    Authorized,
    /// Indexed under a different tenant and forbidden in results.
    Excluded,
}

/// Repository-owned source bytes produced for ingestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CorpusSource {
    /// UTF-8 source sent exactly as recorded.
    Text {
        /// Complete repository-owned source text.
        content: String,
    },
    /// Minimal deterministic PresentationML generated from these slides.
    Presentation {
        /// One visible text shape per slide.
        slides: Vec<String>,
    },
}

/// Logical label for one chunk produced by the production converter and chunker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusChunk {
    /// Opaque task-0188 chunk identity.
    pub id: String,
    /// Zero-based source position emitted by the production chunker.
    pub source_position: u32,
    /// Expected converter provenance family.
    pub provenance_kind: String,
}

/// The task-0188 cases plus the exact generated source corpus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionEvaluationCorpus {
    /// Versioned corpus schema.
    pub schema_version: u32,
    /// Stable corpus identity.
    pub corpus_id: String,
    /// Existing task-0188 relevance judgments.
    pub cases: Vec<EvaluationCase>,
    /// Production behavior each case is intended to exercise.
    pub scenario_requirements: BTreeMap<String, ProductionScenario>,
    /// Repository-owned source inputs.
    pub documents: Vec<ProductionCorpusDocument>,
}

/// Production behavior represented by one task-0188 case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProductionScenario {
    /// Ordinary conversion, indexing, scoped retrieval, and citation packing.
    Standard,
    /// A previous generation is replaced before retrieval.
    IncrementalEdit,
    /// The source is unavailable while retained evidence is queried.
    UnavailableSource,
    /// Retrieval depends on a generated summary record.
    GeneratedSummary,
    /// Retrieval depends on a published concept label.
    ConceptLabel,
}

impl ProductionEvaluationCorpus {
    /// Parses and validates the complete checked-in corpus.
    ///
    /// # Errors
    ///
    /// Returns a typed error for malformed JSON or incomplete labels.
    pub fn parse(json: &str) -> Result<Self, ProductionEvaluationError> {
        let corpus: Self = serde_json::from_str(json)?;
        corpus.validate()?;
        Ok(corpus)
    }

    /// Revalidates corpus uniqueness, scope, and label coverage.
    ///
    /// # Errors
    ///
    /// Returns a typed error when a case can refer to absent or excluded evidence.
    pub fn validate(&self) -> Result<(), ProductionEvaluationError> {
        if self.schema_version != PRODUCTION_CORPUS_SCHEMA_VERSION {
            return Err(ProductionEvaluationError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.corpus_id.trim().is_empty() || self.documents.is_empty() {
            return Err(ProductionEvaluationError::InvalidCorpus(
                "corpus identity and documents are required".into(),
            ));
        }
        evaluate_retrieval(&self.cases, &[], PRODUCTION_RETRIEVAL_CUTOFF)?;

        let mut document_ids = BTreeSet::new();
        let mut source_ids = BTreeSet::new();
        let mut chunk_ids = BTreeSet::new();
        let mut authorized_documents = BTreeSet::new();
        let mut authorized_chunks = BTreeSet::new();
        let mut has_excluded = false;
        let mut has_presentation = false;
        for document in &self.documents {
            if !valid_id(&document.id)
                || !valid_id(&document.source_id)
                || document.media_type.trim().is_empty()
                || document.chunks.is_empty()
                || !document_ids.insert(document.id.clone())
                || !source_ids.insert(document.source_id.clone())
            {
                return Err(ProductionEvaluationError::InvalidCorpus(
                    "document identities, media types, and chunks must be unique and complete"
                        .into(),
                ));
            }
            has_excluded |= document.scope == CorpusScope::Excluded;
            has_presentation |= matches!(document.source, CorpusSource::Presentation { .. });
            match &document.source {
                CorpusSource::Text { content } if content.trim().is_empty() => {
                    return Err(ProductionEvaluationError::InvalidCorpus(
                        "text sources cannot be empty".into(),
                    ));
                }
                CorpusSource::Presentation { slides } if slides.len() < 2 => {
                    return Err(ProductionEvaluationError::InvalidCorpus(
                        "presentation sources need multiple slides".into(),
                    ));
                }
                _ => {}
            }
            let mut positions = BTreeSet::new();
            for chunk in &document.chunks {
                if !valid_id(&chunk.id)
                    || chunk.provenance_kind.trim().is_empty()
                    || !positions.insert(chunk.source_position)
                    || !chunk_ids.insert(chunk.id.clone())
                {
                    return Err(ProductionEvaluationError::InvalidCorpus(
                        "chunk identities and source positions must be unique".into(),
                    ));
                }
                if document.scope == CorpusScope::Authorized {
                    authorized_chunks.insert(chunk.id.clone());
                }
            }
            if document.scope == CorpusScope::Authorized {
                authorized_documents.insert(document.id.clone());
            }
        }
        if !has_excluded || !has_presentation {
            return Err(ProductionEvaluationError::InvalidCorpus(
                "the corpus must cover scope exclusion and structural presentation citations"
                    .into(),
            ));
        }
        if !self.cases.iter().any(|case| case.expected_no_answer) {
            return Err(ProductionEvaluationError::InvalidCorpus(
                "the corpus needs a negative control".into(),
            ));
        }
        if self.scenario_requirements.len() != self.cases.len()
            || self
                .cases
                .iter()
                .any(|case| !self.scenario_requirements.contains_key(&case.id))
        {
            return Err(ProductionEvaluationError::InvalidCorpus(
                "every case needs one explicit production scenario".into(),
            ));
        }
        for case in &self.cases {
            if self.scenario_requirements[&case.id] == ProductionScenario::IncrementalEdit
                && !case.relevant_file_ids.iter().any(|file_id| {
                    self.document(file_id)
                        .is_some_and(|document| document.previous_source.is_some())
                })
            {
                return Err(ProductionEvaluationError::InvalidCorpus(format!(
                    "incremental-edit case `{}` has no previous source generation",
                    case.id
                )));
            }
        }
        for case in &self.cases {
            if !case
                .relevant_file_ids
                .iter()
                .all(|id| authorized_documents.contains(id))
                || !case
                    .relevant_chunk_ids
                    .iter()
                    .all(|id| authorized_chunks.contains(id))
            {
                return Err(ProductionEvaluationError::InvalidCorpus(format!(
                    "case `{}` references absent or excluded evidence",
                    case.id
                )));
            }
        }
        Ok(())
    }

    /// Returns a deterministic digest of parsed corpus content, independent of whitespace.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("validated corpus serializes");
        prefixed_sha256(b"semantic-production-corpus/1", &bytes)
    }

    /// Finds one source document by its logical identity.
    #[must_use]
    pub fn document(&self, id: &str) -> Option<&ProductionCorpusDocument> {
        self.documents.iter().find(|document| document.id == id)
    }

    /// Maps one production source position to its opaque task-0188 chunk label.
    #[must_use]
    pub fn chunk_at(&self, document_id: &str, source_position: u32) -> Option<&CorpusChunk> {
        self.document(document_id)?
            .chunks
            .iter()
            .find(|chunk| chunk.source_position == source_position)
    }
}

/// Exact production Ask thresholds and caps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalPolicyIdentity {
    /// Absolute cosine floor.
    pub absolute_floor: f32,
    /// Maximum distance below the strongest candidate.
    pub relative_window: f32,
    /// Maximum source documents.
    pub maximum_documents: usize,
    /// Maximum primary chunks per document.
    pub maximum_chunks_per_document: usize,
    /// Maximum complete context tokens.
    pub context_token_budget: usize,
    /// Adjacent structural context radius.
    pub adjacent_chunk_radius: u32,
}

impl RetrievalPolicyIdentity {
    /// Returns the worker-owned production Ask policy.
    #[must_use]
    pub fn production() -> Self {
        let policy = RagRetrievalPolicy::default_ask();
        Self {
            absolute_floor: policy.minimum_score,
            relative_window: policy.maximum_score_drop,
            maximum_documents: policy.maximum_documents,
            maximum_chunks_per_document: policy.maximum_chunks_per_document,
            context_token_budget: policy.context_token_budget,
            adjacent_chunk_radius: policy.adjacent_chunk_radius,
        }
    }
}

/// One content-addressed catalog artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionArtifactIdentity {
    /// Logical component identity.
    pub component_id: String,
    /// Content-addressed artifact identity.
    pub artifact_id: String,
    /// Package version.
    pub version: String,
    /// Lowercase SHA-256.
    pub sha256: String,
    /// Exact catalog byte length.
    pub byte_length: u64,
}

/// Immutable package and pipeline identity for one target measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionCandidateIdentity {
    /// Complete Procyon commit SHA recorded by the worker provenance.
    pub procyon_revision: String,
    /// Supported target label.
    pub target: String,
    /// Content-derived catalog revision.
    pub catalog_revision: String,
    /// SHA-256 of `catalog-input.json`.
    pub catalog_input_sha256: String,
    /// Exact converter/chunker/model/tokenizer/protocol/index contract.
    pub pipeline: ProductionPipelineIdentity,
    /// Exact Ask thresholds and caps.
    pub retrieval_policy: RetrievalPolicyIdentity,
    /// Every packaged worker/runtime/model artifact.
    pub artifacts: Vec<ProductionArtifactIdentity>,
    /// Digest of the retained Zvec qualification record.
    pub zvec_qualification_sha256: String,
    /// Digest of the retained ONNX Runtime qualification record, when applicable.
    pub onnx_qualification_sha256: Option<String>,
}

/// One selected chunk with no query or source text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedChunkEvidence {
    /// Opaque task-0188 chunk identity.
    pub chunk_id: String,
    /// Opaque logical file identity.
    pub file_id: String,
    /// Exact dense cosine similarity.
    pub score: f64,
    /// Token count used by production Ask context packing.
    pub token_count: usize,
    /// Opaque source occurrence identity.
    pub source_id: String,
    /// Converter provenance family.
    pub provenance_kind: String,
    /// Whether the source was unavailable.
    pub unavailable: bool,
    /// Whether newer source bytes were known.
    pub stale: bool,
    /// Whether this was generated rather than extracted evidence.
    pub generated: bool,
}

/// One deterministic citation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CitationObservation {
    /// Stable citation label.
    pub label: String,
    /// Opaque cited file identity.
    pub file_id: String,
    /// Opaque cited chunk identities.
    pub chunk_ids: Vec<String>,
}

/// Per-case evidence emitted by the packaged worker run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionCaseObservation {
    /// Opaque task-0188 case identity.
    pub case_id: String,
    /// Policy-qualified file ranking.
    pub ranked_file_ids: Vec<String>,
    /// Policy-qualified structural chunk ranking.
    pub ranked_chunks: Vec<RankedChunkEvidence>,
    /// Every raw in-scope chunk returned before Ask threshold and cap packing.
    pub candidate_chunks: Vec<RankedChunkEvidence>,
    /// Strongest raw in-scope chunk score, when any candidate existed.
    pub strongest_score: Option<f64>,
    /// Effective absolute-plus-relative score floor.
    pub effective_minimum_score: f64,
    /// Deterministic citations selected from the ranked offline evidence.
    pub offline_citations: Vec<CitationObservation>,
    /// Citations emitted by a real configured answer provider, or `null` when not run.
    pub grounded_answer_citations: Option<Vec<CitationObservation>>,
    /// Whether the case's declared lifecycle/specialized behavior was exercised.
    pub production_scenario_exercised: bool,
}

/// Aggregate retrieval and citation metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionEvaluationMetrics {
    /// Existing task-0188 retrieval metrics.
    pub retrieval: RetrievalMetrics,
    /// Fraction of negative controls that retained evidence.
    pub negative_control_false_positive_rate: f64,
    /// Fraction of deterministic offline citation references that were relevant.
    pub offline_citation_correctness: f64,
    /// Recall of expected relevant chunks in deterministic offline citations.
    pub offline_citation_recall: f64,
    /// Correctness of real generated-answer citation references, if generation ran.
    pub grounded_answer_citation_correctness: Option<f64>,
    /// Recall of expected citations from real generated answers, if generation ran.
    pub grounded_answer_citation_recall: Option<f64>,
}

/// One exact packaged target measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionTargetMeasurement {
    /// Immutable candidate identity.
    pub identity: ProductionCandidateIdentity,
    /// Whether strict production trust and a clean source build were verified.
    pub production_package: bool,
    /// Per-case opaque evidence.
    pub observations: Vec<ProductionCaseObservation>,
    /// Metrics recomputed from the observations.
    pub metrics: ProductionEvaluationMetrics,
}

/// Explicit unchanged-threshold and migration decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdDecision {
    /// Previous absolute floor.
    pub previous_absolute_floor: f32,
    /// Candidate absolute floor.
    pub candidate_absolute_floor: f32,
    /// Previous relative window.
    pub previous_relative_window: f32,
    /// Candidate relative window.
    pub candidate_relative_window: f32,
    /// Signed retained-storage change.
    pub storage_impact_bytes: i64,
    /// Operator-readable migration impact.
    pub migration_impact: String,
}

impl Default for ThresholdDecision {
    fn default() -> Self {
        Self {
            previous_absolute_floor: 0.84,
            candidate_absolute_floor: 0.84,
            previous_relative_window: 0.02,
            candidate_relative_window: 0.02,
            storage_impact_bytes: 0,
            migration_impact: "No threshold change and no storage or migration impact attributable to the retained 0.84/0.02 policy.".into(),
        }
    }
}

/// Migration evidence for the intentionally changed embedding input space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingPreprocessingMigration {
    /// Previous embedding-input policy.
    pub baseline: String,
    /// Candidate embedding-input policy.
    pub candidate: String,
    /// Whether existing derived vectors must be rebuilt.
    pub index_rebuild_required: bool,
    /// Steady-state storage change after replacement of the old index.
    pub steady_state_storage_impact_bytes: i64,
    /// Operator-readable migration behavior.
    pub migration_impact: String,
    /// Opaque private before/after evidence identity, when reviewed.
    pub baseline_comparison_evidence: Option<String>,
}

impl Default for EmbeddingPreprocessingMigration {
    fn default() -> Self {
        Self {
            baseline: "preserve-case/1".into(),
            candidate: "unicode-default-case-fold/1".into(),
            index_rebuild_required: true,
            steady_state_storage_impact_bytes: 0,
            migration_impact: "Existing embedding indexes are discarded and rebuilt from preserved source content; original display text and steady-state storage policy are unchanged.".into(),
            baseline_comparison_evidence: None,
        }
    }
}

/// Status of one task-0198 manual gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ManualCriterionStatus {
    /// Evidence has not been recorded.
    Pending,
    /// Evidence was recorded and approved.
    Passed,
}

/// One manual release criterion that automated retrieval cannot establish.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualCriterion {
    /// Stable criterion identity.
    pub id: String,
    /// Current evidence status.
    pub status: ManualCriterionStatus,
    /// Opaque retained-evidence reference; required for a pass.
    pub evidence: Option<String>,
}

/// Release decision derived from exact evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProductionEvaluationDecision {
    /// Every automated and manual gate passed.
    Go,
    /// Evidence is missing, untrusted, incomplete, or below the recorded gates.
    NoGo,
}

/// Aggregate, operator-readable semantic release report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionEvaluationReport {
    /// Report schema version.
    pub report_version: u32,
    /// Checked-in corpus identity.
    pub corpus_id: String,
    /// Digest of parsed cases and generated source inputs.
    pub corpus_fingerprint: String,
    /// Fingerprint of every release-critical implementation source.
    pub release_candidate_fingerprint: String,
    /// Honest description of how the evidence was collected.
    pub measurement_basis: String,
    /// Explicit limitations that apply to the report.
    pub limitations: Vec<String>,
    /// True only when all supported targets supplied strict production measurements.
    pub production_measurement: bool,
    /// Exact target measurements in stable order.
    pub measurements: Vec<ProductionTargetMeasurement>,
    /// Explicit threshold and storage/migration decision.
    pub threshold_decision: ThresholdDecision,
    /// Explicit embedding-space migration and before/after evidence.
    pub embedding_preprocessing_migration: EmbeddingPreprocessingMigration,
    /// Manual criteria outside this automated slice.
    pub manual_criteria: Vec<ManualCriterion>,
    /// Reasons derived from the recorded evidence.
    pub blocking_reasons: Vec<String>,
    /// Decision derived from the recorded evidence.
    pub decision: ProductionEvaluationDecision,
}

impl ProductionEvaluationReport {
    /// Parses a report.
    ///
    /// # Errors
    ///
    /// Returns a typed error for malformed JSON.
    pub fn parse(json: &str) -> Result<Self, ProductionEvaluationError> {
        Ok(serde_json::from_str(json)?)
    }

    /// Builds an honestly blocked report from the supplied target measurements.
    #[must_use]
    pub fn from_measurements(
        corpus: &ProductionEvaluationCorpus,
        measurement_basis: impl Into<String>,
        limitations: Vec<String>,
        measurements: Vec<ProductionTargetMeasurement>,
        manual_criteria: Vec<ManualCriterion>,
    ) -> Self {
        Self::from_measurements_with_release_evidence(
            corpus,
            measurement_basis,
            limitations,
            measurements,
            EmbeddingPreprocessingMigration::default(),
            manual_criteria,
        )
    }

    /// Builds a report while carrying forward separately reviewed migration and manual evidence.
    #[must_use]
    pub fn from_measurements_with_release_evidence(
        corpus: &ProductionEvaluationCorpus,
        measurement_basis: impl Into<String>,
        limitations: Vec<String>,
        mut measurements: Vec<ProductionTargetMeasurement>,
        embedding_preprocessing_migration: EmbeddingPreprocessingMigration,
        manual_criteria: Vec<ManualCriterion>,
    ) -> Self {
        measurements.sort_by(|left, right| left.identity.target.cmp(&right.identity.target));
        let production_measurement = has_all_production_targets(&measurements);
        let threshold_decision = ThresholdDecision::default();
        let blocking_reasons = blocking_reasons(
            &measurements,
            production_measurement,
            &threshold_decision,
            &embedding_preprocessing_migration,
            &manual_criteria,
        );
        let decision = if blocking_reasons.is_empty() {
            ProductionEvaluationDecision::Go
        } else {
            ProductionEvaluationDecision::NoGo
        };
        Self {
            report_version: PRODUCTION_REPORT_VERSION,
            corpus_id: corpus.corpus_id.clone(),
            corpus_fingerprint: corpus.fingerprint(),
            release_candidate_fingerprint: release_candidate_fingerprint(),
            measurement_basis: measurement_basis.into(),
            limitations,
            production_measurement,
            measurements,
            threshold_decision,
            embedding_preprocessing_migration,
            manual_criteria,
            blocking_reasons,
            decision,
        }
    }

    /// Returns all still-pending task-0198 criteria.
    #[must_use]
    pub fn pending_manual_criteria() -> Vec<ManualCriterion> {
        REQUIRED_MANUAL_CRITERIA
            .iter()
            .map(|id| ManualCriterion {
                id: (*id).into(),
                status: ManualCriterionStatus::Pending,
                evidence: None,
            })
            .collect()
    }

    /// Serializes stable checked-in or private evidence JSON.
    ///
    /// # Errors
    ///
    /// Returns a typed error when serialization fails.
    pub fn to_json(&self) -> Result<String, ProductionEvaluationError> {
        let mut json = serde_json::to_string_pretty(self)?;
        json.push('\n');
        Ok(json)
    }

    /// Recomputes metrics, fingerprints, blockers, and the release decision.
    ///
    /// # Errors
    ///
    /// Returns a typed error for stale, tampered, incomplete, or unsafe evidence.
    pub fn validate(
        &self,
        corpus: &ProductionEvaluationCorpus,
    ) -> Result<(), ProductionEvaluationError> {
        corpus.validate()?;
        if self.report_version != PRODUCTION_REPORT_VERSION {
            return Err(ProductionEvaluationError::UnsupportedSchema(
                self.report_version,
            ));
        }
        if self.corpus_id != corpus.corpus_id
            || self.corpus_fingerprint != corpus.fingerprint()
            || self.release_candidate_fingerprint != release_candidate_fingerprint()
        {
            return Err(ProductionEvaluationError::InvalidReport(
                "the report is stale or does not match the current corpus and implementation"
                    .into(),
            ));
        }
        if self.measurement_basis.trim().is_empty() {
            return Err(ProductionEvaluationError::InvalidReport(
                "a measurement basis is required".into(),
            ));
        }
        if !self.production_measurement && self.limitations.is_empty() {
            return Err(ProductionEvaluationError::InvalidReport(
                "a non-production or incomplete report must state limitations".into(),
            ));
        }
        validate_threshold_decision(&self.threshold_decision)?;
        validate_embedding_migration(&self.embedding_preprocessing_migration)?;
        validate_manual_criteria(&self.manual_criteria)?;
        let mut targets = BTreeSet::new();
        let mut revisions = BTreeSet::new();
        let mut model_artifacts = BTreeSet::new();
        for measurement in &self.measurements {
            if !targets.insert(measurement.identity.target.as_str()) {
                return Err(ProductionEvaluationError::InvalidReport(
                    "a target measurement occurs more than once".into(),
                ));
            }
            revisions.insert(measurement.identity.procyon_revision.as_str());
            let model = measurement
                .identity
                .artifacts
                .iter()
                .find(|artifact| {
                    artifact.component_id == "procyon.semantic.model.multilingual-e5-small"
                })
                .ok_or(ProductionEvaluationError::InvalidIdentity)?;
            model_artifacts.insert((
                model.artifact_id.as_str(),
                model.sha256.as_str(),
                model.byte_length,
            ));
            measurement.validate(corpus)?;
        }
        if revisions.len() > 1 || model_artifacts.len() > 1 {
            return Err(ProductionEvaluationError::InvalidReport(
                "all target measurements must use one Procyon revision and model artifact".into(),
            ));
        }
        let production_measurement = has_all_production_targets(&self.measurements);
        if production_measurement != self.production_measurement {
            return Err(ProductionEvaluationError::InvalidReport(
                "production measurement status does not follow from target evidence".into(),
            ));
        }
        let expected = blocking_reasons(
            &self.measurements,
            production_measurement,
            &self.threshold_decision,
            &self.embedding_preprocessing_migration,
            &self.manual_criteria,
        );
        if expected != self.blocking_reasons {
            return Err(ProductionEvaluationError::InvalidReport(
                "blocking reasons do not follow from the recorded evidence".into(),
            ));
        }
        let decision = if expected.is_empty() {
            ProductionEvaluationDecision::Go
        } else {
            ProductionEvaluationDecision::NoGo
        };
        if decision != self.decision {
            return Err(ProductionEvaluationError::InvalidReport(
                "the decision does not follow from the recorded evidence".into(),
            ));
        }
        Ok(())
    }
}

impl ProductionTargetMeasurement {
    /// Scores one exact target observation set.
    ///
    /// # Errors
    ///
    /// Returns a typed error for missing, duplicate, malformed, or leaking evidence.
    pub fn new(
        corpus: &ProductionEvaluationCorpus,
        identity: ProductionCandidateIdentity,
        production_package: bool,
        observations: Vec<ProductionCaseObservation>,
    ) -> Result<Self, ProductionEvaluationError> {
        validate_candidate_identity(&identity)?;
        let metrics = score_observations(corpus, &observations)?;
        Ok(Self {
            identity,
            production_package,
            observations,
            metrics,
        })
    }

    fn validate(
        &self,
        corpus: &ProductionEvaluationCorpus,
    ) -> Result<(), ProductionEvaluationError> {
        validate_candidate_identity(&self.identity)?;
        let expected = score_observations(corpus, &self.observations)?;
        if expected != self.metrics {
            return Err(ProductionEvaluationError::InvalidReport(format!(
                "metrics for `{}` do not follow from per-case evidence",
                self.identity.target
            )));
        }
        Ok(())
    }
}

fn score_observations(
    corpus: &ProductionEvaluationCorpus,
    observations: &[ProductionCaseObservation],
) -> Result<ProductionEvaluationMetrics, ProductionEvaluationError> {
    if observations.len() != corpus.cases.len() {
        return Err(ProductionEvaluationError::InvalidObservation(
            "every corpus case needs exactly one observation".into(),
        ));
    }
    let cases = corpus
        .cases
        .iter()
        .map(|case| (case.id.as_str(), case))
        .collect::<BTreeMap<_, _>>();
    let mut seen_cases = BTreeSet::new();
    let mut retrieval = Vec::with_capacity(observations.len());
    let mut negative_controls = 0_usize;
    let mut negative_false_positives = 0_usize;
    let mut offline_correct = 0_usize;
    let mut offline_total = 0_usize;
    let mut offline_recalled = 0_usize;
    let mut offline_expected = 0_usize;
    let mut grounded_correct = 0_usize;
    let mut grounded_total = 0_usize;
    let mut grounded_recalled = 0_usize;
    let mut grounded_expected = 0_usize;
    let mut all_grounded = true;

    for observation in observations {
        let Some(case) = cases.get(observation.case_id.as_str()).copied() else {
            return Err(ProductionEvaluationError::InvalidObservation(format!(
                "unknown case `{}`",
                observation.case_id
            )));
        };
        if !seen_cases.insert(observation.case_id.as_str()) {
            return Err(ProductionEvaluationError::InvalidObservation(format!(
                "duplicate case `{}`",
                observation.case_id
            )));
        }
        validate_case_observation(corpus, case, observation)?;
        let ranked_chunk_ids = observation
            .ranked_chunks
            .iter()
            .map(|chunk| chunk.chunk_id.clone())
            .collect::<Vec<_>>();
        retrieval.push(EvaluationObservation {
            case_id: observation.case_id.clone(),
            ranked_file_ids: observation.ranked_file_ids.clone(),
            ranked_chunk_ids,
        });
        if case.expected_no_answer {
            negative_controls += 1;
            if !observation.ranked_file_ids.is_empty() || !observation.ranked_chunks.is_empty() {
                negative_false_positives += 1;
            }
        }
        score_citations(
            case,
            &observation.offline_citations,
            &mut offline_correct,
            &mut offline_total,
            &mut offline_recalled,
            &mut offline_expected,
        );
        match &observation.grounded_answer_citations {
            Some(citations) => score_citations(
                case,
                citations,
                &mut grounded_correct,
                &mut grounded_total,
                &mut grounded_recalled,
                &mut grounded_expected,
            ),
            None => all_grounded = false,
        }
    }
    if seen_cases.len() != cases.len() || negative_controls == 0 {
        return Err(ProductionEvaluationError::InvalidObservation(
            "case coverage or negative controls are incomplete".into(),
        ));
    }
    let ratio = |numerator: usize, denominator: usize| {
        if denominator == 0 {
            1.0
        } else {
            numerator as f64 / denominator as f64
        }
    };
    Ok(ProductionEvaluationMetrics {
        retrieval: evaluate_retrieval(&corpus.cases, &retrieval, PRODUCTION_RETRIEVAL_CUTOFF)?,
        negative_control_false_positive_rate: ratio(negative_false_positives, negative_controls),
        offline_citation_correctness: ratio(offline_correct, offline_total),
        offline_citation_recall: ratio(offline_recalled, offline_expected),
        grounded_answer_citation_correctness: all_grounded
            .then(|| ratio(grounded_correct, grounded_total)),
        grounded_answer_citation_recall: all_grounded
            .then(|| ratio(grounded_recalled, grounded_expected)),
    })
}

fn validate_case_observation(
    corpus: &ProductionEvaluationCorpus,
    case: &EvaluationCase,
    observation: &ProductionCaseObservation,
) -> Result<(), ProductionEvaluationError> {
    let policy = RagRetrievalPolicy::default_ask();
    if observation.ranked_file_ids.len() > policy.maximum_documents
        || observation.ranked_chunks.len()
            > policy
                .maximum_documents
                .saturating_mul(policy.maximum_chunks_per_document)
        || observation.candidate_chunks.len() > 512
        || !observation.effective_minimum_score.is_finite()
        || observation.effective_minimum_score < f64::from(policy.minimum_score)
        || observation
            .strongest_score
            .is_some_and(|score| !score.is_finite() || !(-1.0..=1.0).contains(&score))
    {
        return Err(ProductionEvaluationError::InvalidObservation(
            observation.case_id.clone(),
        ));
    }
    let mut files = BTreeSet::new();
    for file_id in &observation.ranked_file_ids {
        if !files.insert(file_id.as_str())
            || corpus
                .document(file_id)
                .is_none_or(|document| document.scope != CorpusScope::Authorized)
        {
            return Err(ProductionEvaluationError::ScopeLeakage(file_id.clone()));
        }
    }
    let mut candidate_chunks = BTreeMap::new();
    let mut previous_score = f64::INFINITY;
    for chunk in &observation.candidate_chunks {
        validate_ranked_chunk(corpus, chunk, &observation.case_id)?;
        if chunk.score > previous_score
            || candidate_chunks
                .insert(chunk.chunk_id.as_str(), chunk)
                .is_some()
        {
            return Err(ProductionEvaluationError::InvalidObservation(
                "candidate chunks must be unique and score ordered".into(),
            ));
        }
        previous_score = chunk.score;
    }
    let eligible_scores = observation
        .candidate_chunks
        .iter()
        .filter(|chunk| !chunk.generated)
        .map(|chunk| chunk.score as f32)
        .collect::<Vec<_>>();
    let strongest = eligible_scores.iter().copied().max_by(f32::total_cmp);
    let effective = f64::from(policy.effective_minimum_score(eligible_scores));
    if !optional_score_matches(observation.strongest_score, strongest.map(f64::from))
        || (observation.effective_minimum_score - effective).abs() > 1e-6
    {
        return Err(ProductionEvaluationError::InvalidObservation(
            "strongest and effective scores do not follow from candidate evidence".into(),
        ));
    }

    let mut chunks = BTreeSet::new();
    let mut chunks_per_file = BTreeMap::<&str, usize>::new();
    let mut selected_files = Vec::new();
    let mut selected_file_set = BTreeSet::new();
    let mut selected_tokens = 0_usize;
    for chunk in &observation.ranked_chunks {
        validate_ranked_chunk(corpus, chunk, &observation.case_id)?;
        if candidate_chunks.get(chunk.chunk_id.as_str()).copied() != Some(chunk)
            || chunk.score + f64::EPSILON < observation.effective_minimum_score
            || !observation.ranked_file_ids.contains(&chunk.file_id)
            || !chunks.insert(chunk.chunk_id.as_str())
        {
            return Err(ProductionEvaluationError::InvalidObservation(
                observation.case_id.clone(),
            ));
        }
        let count = chunks_per_file.entry(&chunk.file_id).or_default();
        *count += 1;
        if *count > policy.maximum_chunks_per_document {
            return Err(ProductionEvaluationError::InvalidObservation(
                "per-document Ask chunk cap was exceeded".into(),
            ));
        }
        if selected_file_set.insert(chunk.file_id.as_str()) {
            selected_files.push(chunk.file_id.as_str());
        }
        selected_tokens = selected_tokens.saturating_add(chunk.token_count);
    }
    if selected_files
        != observation
            .ranked_file_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
        || selected_tokens > policy.context_token_budget
    {
        return Err(ProductionEvaluationError::InvalidObservation(
            "file ranking or Ask token packing is inconsistent".into(),
        ));
    }
    if observation.ranked_chunks.is_empty() != observation.ranked_file_ids.is_empty() {
        return Err(ProductionEvaluationError::InvalidObservation(
            "ranked files and chunks must be present together".into(),
        ));
    }
    validate_citations(corpus, observation, &observation.offline_citations)?;
    if let Some(citations) = &observation.grounded_answer_citations {
        validate_citations(corpus, observation, citations)?;
    }
    if case.expected_no_answer && !observation.offline_citations.is_empty() {
        return Err(ProductionEvaluationError::InvalidObservation(
            "negative controls cannot record offline citations".into(),
        ));
    }
    Ok(())
}

fn optional_score_matches(recorded: Option<f64>, expected: Option<f64>) -> bool {
    match (recorded, expected) {
        (Some(recorded), Some(expected)) => (recorded - expected).abs() <= 1e-6,
        (None, None) => true,
        _ => false,
    }
}

fn validate_ranked_chunk(
    corpus: &ProductionEvaluationCorpus,
    chunk: &RankedChunkEvidence,
    case_id: &str,
) -> Result<(), ProductionEvaluationError> {
    let Some(document) = corpus.document(&chunk.file_id) else {
        return Err(ProductionEvaluationError::ScopeLeakage(
            chunk.file_id.clone(),
        ));
    };
    let Some(expected) = document
        .chunks
        .iter()
        .find(|expected| expected.id == chunk.chunk_id)
    else {
        return Err(ProductionEvaluationError::ScopeLeakage(
            chunk.chunk_id.clone(),
        ));
    };
    if document.scope != CorpusScope::Authorized
        || expected.provenance_kind != chunk.provenance_kind
        || document.source_id != chunk.source_id
        || !chunk.score.is_finite()
        || !(-1.0..=1.0).contains(&chunk.score)
        || chunk.token_count == 0
        || chunk.unavailable
        || chunk.stale
    {
        return Err(ProductionEvaluationError::InvalidObservation(
            case_id.into(),
        ));
    }
    Ok(())
}

fn validate_citations(
    corpus: &ProductionEvaluationCorpus,
    observation: &ProductionCaseObservation,
    citations: &[CitationObservation],
) -> Result<(), ProductionEvaluationError> {
    let ranked = observation
        .ranked_chunks
        .iter()
        .map(|chunk| (chunk.chunk_id.as_str(), chunk.file_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut labels = BTreeSet::new();
    let mut cited_chunks = BTreeSet::new();
    for citation in citations {
        if !valid_id(&citation.label)
            || !labels.insert(citation.label.as_str())
            || citation.chunk_ids.is_empty()
            || corpus
                .document(&citation.file_id)
                .is_none_or(|document| document.scope != CorpusScope::Authorized)
        {
            return Err(ProductionEvaluationError::InvalidCitation(
                observation.case_id.clone(),
            ));
        }
        for chunk_id in &citation.chunk_ids {
            if ranked.get(chunk_id.as_str()).copied() != Some(citation.file_id.as_str())
                || !cited_chunks.insert(chunk_id.as_str())
            {
                return Err(ProductionEvaluationError::InvalidCitation(
                    observation.case_id.clone(),
                ));
            }
        }
    }
    Ok(())
}

fn score_citations(
    case: &EvaluationCase,
    citations: &[CitationObservation],
    correct: &mut usize,
    total: &mut usize,
    recalled: &mut usize,
    expected: &mut usize,
) {
    let cited = citations
        .iter()
        .flat_map(|citation| citation.chunk_ids.iter())
        .collect::<BTreeSet<_>>();
    *correct += cited
        .iter()
        .filter(|chunk| case.relevant_chunk_ids.contains(**chunk))
        .count();
    *total += cited.len();
    *recalled += case
        .relevant_chunk_ids
        .iter()
        .filter(|chunk| cited.contains(chunk))
        .count();
    *expected += case.relevant_chunk_ids.len();
}

fn validate_candidate_identity(
    identity: &ProductionCandidateIdentity,
) -> Result<(), ProductionEvaluationError> {
    if !is_sha256(&identity.catalog_input_sha256)
        || !is_sha256(&identity.zvec_qualification_sha256)
        || identity
            .onnx_qualification_sha256
            .as_deref()
            .is_some_and(|digest| !is_sha256(digest))
        || identity.procyon_revision.len() != 40
        || !identity
            .procyon_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || !SUPPORTED_TARGETS.contains(&identity.target.as_str())
        || identity.catalog_revision.trim().is_empty()
        || identity.pipeline != production_pipeline_identity()
        || identity.retrieval_policy != RetrievalPolicyIdentity::production()
    {
        return Err(ProductionEvaluationError::InvalidIdentity);
    }
    let expected = REQUIRED_COMPONENTS
        .into_iter()
        .chain((identity.target == "linux-x86_64").then_some("procyon.semantic.onnx-runtime"))
        .collect::<BTreeSet<_>>();
    let actual = identity
        .artifacts
        .iter()
        .map(|artifact| artifact.component_id.as_str())
        .collect::<BTreeSet<_>>();
    if actual != expected
        || identity.artifacts.len() != expected.len()
        || identity.artifacts.iter().any(|artifact| {
            !valid_id(&artifact.component_id)
                || !valid_id(&artifact.artifact_id)
                || artifact.version.trim().is_empty()
                || !is_sha256(&artifact.sha256)
                || artifact.byte_length == 0
        })
        || (identity.target == "linux-x86_64") != identity.onnx_qualification_sha256.is_some()
    {
        return Err(ProductionEvaluationError::InvalidIdentity);
    }
    Ok(())
}

fn validate_threshold_decision(
    decision: &ThresholdDecision,
) -> Result<(), ProductionEvaluationError> {
    let expected = ThresholdDecision::default();
    if decision != &expected {
        return Err(ProductionEvaluationError::InvalidReport(
            "the measured candidate must retain the 0.84/0.02 policy with zero migration impact"
                .into(),
        ));
    }
    Ok(())
}

fn validate_embedding_migration(
    migration: &EmbeddingPreprocessingMigration,
) -> Result<(), ProductionEvaluationError> {
    let expected = EmbeddingPreprocessingMigration::default();
    if migration.baseline != expected.baseline
        || migration.candidate != expected.candidate
        || !migration.index_rebuild_required
        || migration.steady_state_storage_impact_bytes != 0
        || migration.migration_impact != expected.migration_impact
        || migration
            .baseline_comparison_evidence
            .as_deref()
            .is_some_and(|evidence| !valid_id(evidence))
    {
        return Err(ProductionEvaluationError::InvalidReport(
            "embedding preprocessing migration evidence is incomplete or inconsistent".into(),
        ));
    }
    Ok(())
}

fn validate_manual_criteria(criteria: &[ManualCriterion]) -> Result<(), ProductionEvaluationError> {
    let expected = REQUIRED_MANUAL_CRITERIA
        .into_iter()
        .collect::<BTreeSet<_>>();
    let actual = criteria
        .iter()
        .map(|criterion| criterion.id.as_str())
        .collect::<BTreeSet<_>>();
    if actual != expected || actual.len() != criteria.len() {
        return Err(ProductionEvaluationError::InvalidReport(
            "the complete task-0198 manual criteria set is required".into(),
        ));
    }
    if criteria.iter().any(|criterion| {
        criterion.status == ManualCriterionStatus::Passed
            && criterion
                .evidence
                .as_deref()
                .is_none_or(|evidence| !valid_id(evidence))
    }) {
        return Err(ProductionEvaluationError::InvalidReport(
            "passed manual criteria require an opaque evidence identity".into(),
        ));
    }
    Ok(())
}

fn has_all_production_targets(measurements: &[ProductionTargetMeasurement]) -> bool {
    measurements.len() == SUPPORTED_TARGETS.len()
        && measurements
            .iter()
            .all(|measurement| measurement.production_package)
        && measurements
            .iter()
            .map(|measurement| measurement.identity.target.as_str())
            .collect::<BTreeSet<_>>()
            == SUPPORTED_TARGETS.into_iter().collect::<BTreeSet<_>>()
}

fn blocking_reasons(
    measurements: &[ProductionTargetMeasurement],
    production_measurement: bool,
    threshold: &ThresholdDecision,
    migration: &EmbeddingPreprocessingMigration,
    manual_criteria: &[ManualCriterion],
) -> Vec<String> {
    let mut reasons = Vec::new();
    if !production_measurement {
        reasons.push(
            "Exact strict-production measurements are required for macOS arm64, Windows x86-64, Linux x86-64, and Linux arm64.".into(),
        );
    }
    if threshold != &ThresholdDecision::default() {
        reasons.push(
            "The 0.84 absolute floor and 0.02 relative window changed without comparable evidence."
                .into(),
        );
    }
    if migration.baseline_comparison_evidence.is_none() {
        reasons.push(
            "Reviewed before/after production evidence for Unicode case-folded embeddings is missing."
                .into(),
        );
    }
    for measurement in measurements {
        let metrics = &measurement.metrics;
        for observation in &measurement.observations {
            if !observation.production_scenario_exercised {
                reasons.push(format!(
                    "{} did not exercise the declared production scenario for case `{}`.",
                    measurement.identity.target, observation.case_id
                ));
            }
        }
        if metrics.retrieval.file_recall_at_k < MINIMUM_FILE_RECALL
            || metrics.retrieval.chunk_recall_at_k < MINIMUM_CHUNK_RECALL
            || metrics.retrieval.mean_reciprocal_rank < MINIMUM_MRR
            || metrics.retrieval.ndcg_at_k < MINIMUM_NDCG
        {
            reasons.push(format!(
                "{} retrieval did not meet the recorded recall, MRR, and nDCG quality floors.",
                measurement.identity.target
            ));
        }
        if metrics.negative_control_false_positive_rate != 0.0 {
            reasons.push(format!(
                "{} retained evidence for a negative control.",
                measurement.identity.target
            ));
        }
        if metrics.offline_citation_correctness != 1.0 || metrics.offline_citation_recall != 1.0 {
            reasons.push(format!(
                "{} deterministic citation evidence was incomplete or irrelevant.",
                measurement.identity.target
            ));
        }
        if matches!(
            (
                metrics.grounded_answer_citation_correctness,
                metrics.grounded_answer_citation_recall,
            ),
            (Some(correctness), Some(recall)) if correctness != 1.0 || recall != 1.0
        ) || metrics.grounded_answer_citation_correctness.is_some()
            != metrics.grounded_answer_citation_recall.is_some()
        {
            reasons.push(format!(
                "{} grounded generated-answer citation evidence was incomplete or incorrect.",
                measurement.identity.target
            ));
        }
    }
    for criterion in manual_criteria {
        if criterion.status != ManualCriterionStatus::Passed {
            reasons.push(format!(
                "Task-0198 manual criterion `{}` remains pending.",
                criterion.id
            ));
        }
    }
    reasons
}

/// Fingerprints every source that can change conversion, chunking, embedding,
/// storage, retrieval, citation selection, package identity, or report scoring.
#[must_use]
pub fn release_candidate_fingerprint() -> String {
    const SOURCES: &[(&str, &[u8])] = &[
        (
            "application/semantic_evaluation.rs",
            include_bytes!("semantic_evaluation.rs"),
        ),
        (
            "application/semantic_production_evaluation.rs",
            include_bytes!("semantic_production_evaluation.rs"),
        ),
        (
            "application/evaluate_semantic_production.rs",
            include_bytes!("../examples/evaluate_semantic_production.rs"),
        ),
        (
            "components/catalog.rs",
            include_bytes!("../../fm-semantic-components/src/catalog.rs"),
        ),
        (
            "components/production.rs",
            include_bytes!("../../fm-semantic-components/src/production.rs"),
        ),
        (
            "components/release_bundle.rs",
            include_bytes!("../../fm-semantic-components/src/release_bundle.rs"),
        ),
        (
            "conversion/chunk.rs",
            include_bytes!("../../fm-semantic-conversion/src/chunk.rs"),
        ),
        (
            "conversion/converter.rs",
            include_bytes!("../../fm-semantic-conversion/src/converter.rs"),
        ),
        (
            "docling/lib.rs",
            include_bytes!("../../fm-semantic-docling/src/lib.rs"),
        ),
        (
            "protocol/worker.proto",
            include_bytes!("../../fm-semantic-protocol/proto/semantic/v1/worker.proto"),
        ),
        (
            "worker/developer_bundle.rs",
            include_bytes!("../../fm-semantic-worker/src/developer_bundle.rs"),
        ),
        (
            "worker/embedding.rs",
            include_bytes!("../../fm-semantic-worker/src/embedding.rs"),
        ),
        (
            "worker/ingestion.rs",
            include_bytes!("../../fm-semantic-worker/src/ingestion.rs"),
        ),
        (
            "worker/rag_retrieval.rs",
            include_bytes!("../../fm-semantic-worker/src/rag_retrieval.rs"),
        ),
        (
            "worker/semantic_search.rs",
            include_bytes!("../../fm-semantic-worker/src/semantic_search.rs"),
        ),
        (
            "worker/semantic_storage.rs",
            include_bytes!("../../fm-semantic-worker/src/semantic_storage.rs"),
        ),
        (
            "worker/zvec_storage.rs",
            include_bytes!("../../fm-semantic-worker/src/zvec_storage.rs"),
        ),
        (
            "scripts/smoke-semantic-production-bundle.mjs",
            include_bytes!("../../../scripts/smoke-semantic-production-bundle.mjs"),
        ),
        ("Cargo.lock", include_bytes!("../../../Cargo.lock")),
    ];
    let mut hasher = Sha256::new();
    hasher.update(b"semantic-production-candidate/1");
    for (name, source) in SOURCES {
        hasher.update(name.len().to_le_bytes());
        hasher.update(name.as_bytes());
        hasher.update(source.len().to_le_bytes());
        hasher.update(source);
    }
    format!("sha256:{}", hexadecimal(&hasher.finalize()))
}

fn prefixed_sha256(prefix: &[u8], bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    hasher.update(bytes.len().to_le_bytes());
    hasher.update(bytes);
    format!("sha256:{}", hexadecimal(&hasher.finalize()))
}

fn hexadecimal(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

/// Production evaluation validation failure.
#[derive(Debug, Error)]
pub enum ProductionEvaluationError {
    /// The document/report uses an unsupported schema.
    #[error("unsupported production evaluation schema `{0}`")]
    UnsupportedSchema(u32),
    /// The corpus is incomplete or internally inconsistent.
    #[error("invalid production evaluation corpus: {0}")]
    InvalidCorpus(String),
    /// Per-case evidence is missing, duplicated, or malformed.
    #[error("invalid production evaluation observation: {0}")]
    InvalidObservation(String),
    /// A result escaped the authorized corpus scope.
    #[error("production evaluation scope leakage: {0}")]
    ScopeLeakage(String),
    /// A citation is malformed or does not reference ranked authorized evidence.
    #[error("invalid production evaluation citation for case `{0}`")]
    InvalidCitation(String),
    /// Package, runtime, model, or pipeline identity is incomplete or non-production.
    #[error("invalid production candidate identity")]
    InvalidIdentity,
    /// A report is stale, tampered, or internally inconsistent.
    #[error("invalid production evaluation report: {0}")]
    InvalidReport(String),
    /// Existing retrieval metric validation failed.
    #[error(transparent)]
    Retrieval(#[from] crate::semantic_evaluation::EvaluationError),
    /// JSON parsing or serialization failed.
    #[error("production evaluation JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus() -> ProductionEvaluationCorpus {
        ProductionEvaluationCorpus {
            schema_version: PRODUCTION_CORPUS_SCHEMA_VERSION,
            corpus_id: "corpus-v1".into(),
            cases: vec![
                EvaluationCase {
                    id: "positive".into(),
                    query: "query".into(),
                    relevant_file_ids: BTreeSet::from(["file-a".into()]),
                    relevant_chunk_ids: BTreeSet::from(["chunk-a".into()]),
                    category: Default::default(),
                    expected_no_answer: false,
                },
                EvaluationCase {
                    id: "negative".into(),
                    query: "negative".into(),
                    relevant_file_ids: BTreeSet::new(),
                    relevant_chunk_ids: BTreeSet::new(),
                    category: Default::default(),
                    expected_no_answer: true,
                },
            ],
            scenario_requirements: BTreeMap::from([
                ("negative".into(), ProductionScenario::Standard),
                ("positive".into(), ProductionScenario::Standard),
            ]),
            documents: vec![
                ProductionCorpusDocument {
                    id: "file-a".into(),
                    source_id: "source-a".into(),
                    scope: CorpusScope::Authorized,
                    media_type: "text/markdown".into(),
                    source: CorpusSource::Text {
                        content: "# Query\n\nRelevant answer.".into(),
                    },
                    previous_source: None,
                    chunks: vec![CorpusChunk {
                        id: "chunk-a".into(),
                        source_position: 0,
                        provenance_kind: "textLines".into(),
                    }],
                },
                ProductionCorpusDocument {
                    id: "excluded-file".into(),
                    source_id: "excluded-source".into(),
                    scope: CorpusScope::Excluded,
                    media_type:
                        "application/vnd.openxmlformats-officedocument.presentationml.presentation"
                            .into(),
                    source: CorpusSource::Presentation {
                        slides: vec!["One".into(), "Two".into()],
                    },
                    previous_source: None,
                    chunks: vec![
                        CorpusChunk {
                            id: "excluded-1".into(),
                            source_position: 0,
                            provenance_kind: "slide".into(),
                        },
                        CorpusChunk {
                            id: "excluded-2".into(),
                            source_position: 1,
                            provenance_kind: "slide".into(),
                        },
                    ],
                },
            ],
        }
    }

    fn identity(target: &str) -> ProductionCandidateIdentity {
        let mut artifacts = REQUIRED_COMPONENTS
            .iter()
            .map(|component| ProductionArtifactIdentity {
                component_id: (*component).into(),
                artifact_id: format!("{component}.artifact"),
                version: "1.0.0".into(),
                sha256: format!("sha256:{}", "1".repeat(64)),
                byte_length: 1,
            })
            .collect::<Vec<_>>();
        let onnx = (target == "linux-x86_64").then(|| {
            artifacts.push(ProductionArtifactIdentity {
                component_id: "procyon.semantic.onnx-runtime".into(),
                artifact_id: "procyon.semantic.onnx-runtime.artifact".into(),
                version: "1.28.0".into(),
                sha256: format!("sha256:{}", "2".repeat(64)),
                byte_length: 1,
            });
            format!("sha256:{}", "3".repeat(64))
        });
        ProductionCandidateIdentity {
            procyon_revision: "a".repeat(40),
            target: target.into(),
            catalog_revision: "catalog-v1".into(),
            catalog_input_sha256: format!("sha256:{}", "4".repeat(64)),
            pipeline: production_pipeline_identity(),
            retrieval_policy: RetrievalPolicyIdentity::production(),
            artifacts,
            zvec_qualification_sha256: format!("sha256:{}", "5".repeat(64)),
            onnx_qualification_sha256: onnx,
        }
    }

    fn observations() -> Vec<ProductionCaseObservation> {
        vec![
            ProductionCaseObservation {
                case_id: "positive".into(),
                ranked_file_ids: vec!["file-a".into()],
                ranked_chunks: vec![RankedChunkEvidence {
                    chunk_id: "chunk-a".into(),
                    file_id: "file-a".into(),
                    score: 0.9,
                    token_count: 2,
                    source_id: "source-a".into(),
                    provenance_kind: "textLines".into(),
                    unavailable: false,
                    stale: false,
                    generated: false,
                }],
                candidate_chunks: vec![RankedChunkEvidence {
                    chunk_id: "chunk-a".into(),
                    file_id: "file-a".into(),
                    score: 0.9,
                    token_count: 2,
                    source_id: "source-a".into(),
                    provenance_kind: "textLines".into(),
                    unavailable: false,
                    stale: false,
                    generated: false,
                }],
                strongest_score: Some(0.9),
                effective_minimum_score: 0.88,
                offline_citations: vec![CitationObservation {
                    label: "C1".into(),
                    file_id: "file-a".into(),
                    chunk_ids: vec!["chunk-a".into()],
                }],
                grounded_answer_citations: None,
                production_scenario_exercised: true,
            },
            ProductionCaseObservation {
                case_id: "negative".into(),
                ranked_file_ids: Vec::new(),
                ranked_chunks: Vec::new(),
                candidate_chunks: vec![RankedChunkEvidence {
                    chunk_id: "chunk-a".into(),
                    file_id: "file-a".into(),
                    score: 0.7,
                    token_count: 2,
                    source_id: "source-a".into(),
                    provenance_kind: "textLines".into(),
                    unavailable: false,
                    stale: false,
                    generated: false,
                }],
                strongest_score: Some(0.7),
                effective_minimum_score: 0.84,
                offline_citations: Vec::new(),
                grounded_answer_citations: None,
                production_scenario_exercised: true,
            },
        ]
    }

    #[test]
    fn exact_case_coverage_and_metrics_are_recomputed() {
        let corpus = corpus();
        let measurement = ProductionTargetMeasurement::new(
            &corpus,
            identity("macos-aarch64"),
            true,
            observations(),
        )
        .expect("measurement");
        assert_eq!(measurement.metrics.retrieval.file_recall_at_k, 1.0);
        assert_eq!(
            measurement.metrics.negative_control_false_positive_rate,
            0.0
        );
        assert_eq!(measurement.metrics.offline_citation_correctness, 1.0);
        assert_eq!(
            measurement.metrics.grounded_answer_citation_correctness,
            None
        );
    }

    #[test]
    fn missing_duplicate_and_unknown_cases_are_rejected() {
        let corpus = corpus();
        let mut missing = observations();
        missing.pop();
        assert!(matches!(
            ProductionTargetMeasurement::new(&corpus, identity("macos-aarch64"), true, missing),
            Err(ProductionEvaluationError::InvalidObservation(_))
        ));
        let mut duplicate = observations();
        duplicate[1].case_id = "positive".into();
        assert!(matches!(
            ProductionTargetMeasurement::new(&corpus, identity("macos-aarch64"), true, duplicate),
            Err(ProductionEvaluationError::InvalidObservation(_))
        ));
    }

    #[test]
    fn scope_leakage_and_malformed_citations_are_rejected() {
        let corpus = corpus();
        let mut leaked = observations();
        leaked[0].ranked_file_ids = vec!["excluded-file".into()];
        leaked[0].ranked_chunks[0].file_id = "excluded-file".into();
        leaked[0].ranked_chunks[0].chunk_id = "excluded-1".into();
        assert!(matches!(
            ProductionTargetMeasurement::new(&corpus, identity("macos-aarch64"), true, leaked),
            Err(ProductionEvaluationError::InvalidObservation(_)
                | ProductionEvaluationError::ScopeLeakage(_))
        ));

        let mut malformed = observations();
        malformed[0].offline_citations[0].chunk_ids = vec!["excluded-1".into()];
        assert!(matches!(
            ProductionTargetMeasurement::new(&corpus, identity("macos-aarch64"), true, malformed),
            Err(ProductionEvaluationError::InvalidCitation(_))
        ));

        let mut extra_file = observations();
        extra_file[0].ranked_file_ids.push("excluded-file".into());
        assert!(matches!(
            ProductionTargetMeasurement::new(&corpus, identity("macos-aarch64"), true, extra_file),
            Err(ProductionEvaluationError::ScopeLeakage(_))
        ));

        let mut wrong_source = observations();
        wrong_source[0].ranked_chunks[0].source_id = "other-source".into();
        wrong_source[0].candidate_chunks[0].source_id = "other-source".into();
        assert!(matches!(
            ProductionTargetMeasurement::new(
                &corpus,
                identity("macos-aarch64"),
                true,
                wrong_source
            ),
            Err(ProductionEvaluationError::InvalidObservation(_))
        ));

        let mut below_threshold = observations();
        below_threshold[0].ranked_chunks[0].score = 0.80;
        below_threshold[0].candidate_chunks[0].score = 0.80;
        below_threshold[0].strongest_score = Some(0.80);
        below_threshold[0].effective_minimum_score = 0.84;
        assert!(matches!(
            ProductionTargetMeasurement::new(
                &corpus,
                identity("macos-aarch64"),
                true,
                below_threshold
            ),
            Err(ProductionEvaluationError::InvalidObservation(_))
        ));
    }

    #[test]
    fn package_identity_and_report_tampering_are_rejected() {
        let corpus = corpus();
        let measurement = ProductionTargetMeasurement::new(
            &corpus,
            identity("macos-aarch64"),
            true,
            observations(),
        )
        .expect("measurement");
        let mut report = ProductionEvaluationReport::from_measurements(
            &corpus,
            "exact packaged worker",
            vec!["manual criteria remain".into()],
            vec![measurement],
            ProductionEvaluationReport::pending_manual_criteria(),
        );
        report.validate(&corpus).expect("valid no-go report");

        report.decision = ProductionEvaluationDecision::Go;
        assert!(matches!(
            report.validate(&corpus),
            Err(ProductionEvaluationError::InvalidReport(_))
        ));
        report.decision = ProductionEvaluationDecision::NoGo;
        report.release_candidate_fingerprint = "sha256:stale".into();
        assert!(matches!(
            report.validate(&corpus),
            Err(ProductionEvaluationError::InvalidReport(_))
        ));

        let mut wrong_identity = identity("macos-aarch64");
        wrong_identity.pipeline = ProductionPipelineIdentity::new(
            1,
            2,
            "baseline/2",
            "structural/3",
            "unicode-default-case-fold/1",
            production_pipeline_identity().tokenizer().clone(),
            production_pipeline_identity().model().clone(),
        )
        .expect("alternate identity");
        assert!(matches!(
            ProductionTargetMeasurement::new(&corpus, wrong_identity, true, observations()),
            Err(ProductionEvaluationError::InvalidIdentity)
        ));

        let first = ProductionTargetMeasurement::new(
            &corpus,
            identity("macos-aarch64"),
            true,
            observations(),
        )
        .expect("first target");
        let mut second_identity = identity("windows-x86_64");
        second_identity.procyon_revision = "b".repeat(40);
        let second =
            ProductionTargetMeasurement::new(&corpus, second_identity, true, observations())
                .expect("second target");
        let mixed = ProductionEvaluationReport::from_measurements(
            &corpus,
            "mixed revisions",
            vec!["invalid comparison".into()],
            vec![first, second],
            ProductionEvaluationReport::pending_manual_criteria(),
        );
        assert!(matches!(
            mixed.validate(&corpus),
            Err(ProductionEvaluationError::InvalidReport(_))
        ));

        let mut incomplete_scenario_observations = observations();
        incomplete_scenario_observations[0].production_scenario_exercised = false;
        let incomplete_scenario = ProductionTargetMeasurement::new(
            &corpus,
            identity("macos-aarch64"),
            true,
            incomplete_scenario_observations,
        )
        .expect("incomplete scenario measurement");
        let report = ProductionEvaluationReport::from_measurements(
            &corpus,
            "scenario evidence",
            vec!["one scenario remains incomplete".into()],
            vec![incomplete_scenario],
            ProductionEvaluationReport::pending_manual_criteria(),
        );
        assert!(
            report
                .blocking_reasons
                .iter()
                .any(|reason| reason.contains("declared production scenario"))
        );
    }

    #[test]
    fn corpus_fingerprint_changes_with_source_or_labels() {
        let original = corpus();
        let mut changed = original.clone();
        let CorpusSource::Text { content } = &mut changed.documents[0].source else {
            panic!("text source");
        };
        content.push_str(" changed");
        assert_ne!(original.fingerprint(), changed.fingerprint());
    }

    #[test]
    fn negative_control_false_positives_are_counted() {
        let corpus = corpus();
        let mut evidence = observations();
        evidence[1].ranked_file_ids = vec!["file-a".into()];
        evidence[1].ranked_chunks = evidence[0].ranked_chunks.clone();
        evidence[1].candidate_chunks = evidence[0].candidate_chunks.clone();
        evidence[1].strongest_score = Some(0.9);
        evidence[1].effective_minimum_score = 0.88;
        let measurement =
            ProductionTargetMeasurement::new(&corpus, identity("macos-aarch64"), true, evidence)
                .expect("measured false positive");
        assert_eq!(
            measurement.metrics.negative_control_false_positive_rate,
            1.0
        );
    }

    #[test]
    fn repository_corpus_and_no_go_template_are_current() {
        let corpus = ProductionEvaluationCorpus::parse(include_str!(
            "../tests/fixtures/semantic-evaluation-v1.json"
        ))
        .expect("task-0188 production corpus");
        assert_eq!(corpus.cases.len(), 12);
        assert_eq!(
            corpus.fingerprint(),
            "sha256:6e0c201a6393bdfad56999ecfa86d6b66387c8aa41c8809055bf14429755f3ca"
        );
        let report = ProductionEvaluationReport::parse(include_str!(
            "../../../docs/evaluations/semantic-production-v1.json"
        ))
        .expect("checked-in semantic template");
        report
            .validate(&corpus)
            .expect("current fail-closed template");
        assert_eq!(report.decision, ProductionEvaluationDecision::NoGo);
        assert!(!report.production_measurement);
    }
}
