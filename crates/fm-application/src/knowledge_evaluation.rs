//! Repository-owned release-gate evaluation for Structured Knowledge retrieval.
//!
//! This module answers one question: does structured retrieval find better
//! *sources* than sending the user's whole question to a vector index? It does
//! that by running a repository-owned multilingual corpus through the real
//! [`crate::knowledge::KnowledgePlanner`], the real
//! [`crate::knowledge_search::KnowledgeSearchCoordinator`], and the real worker
//! [`KnowledgeRetrievalService`], so planning, fusion, authorization, and
//! evidence materialization are the production ones.
//!
//! Two things are deliberately *not* production here, and every report says so:
//! the embedding model is a deterministic lexical surrogate rather than a real
//! multilingual model, and the candidate indexes the checked-in report was
//! produced with are in-process rather than the native Zvec collection. Numbers
//! produced under those conditions describe the fixture, not a shipped system,
//! which is why a measured go can never be derived from them.
//!
//! [`evaluate_knowledge_retrieval_with`] accepts caller-supplied indexes, and
//! the crate's `zvec` feature runs the same corpus over the real native Zvec
//! dense and full-text collection. That closes the index half of the gap; the
//! embedding half stays open until a production model is measured. Which index
//! answered is part of every pipeline fingerprint, so an in-process run and a
//! native run over one corpus can never be confused for each other.
//!
//! Generated-answer fluency is intentionally absent: this is a source-discovery
//! evaluation, and a fluent answer over the wrong sources is a worse outcome
//! than no answer at all.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use fm_semantic_conversion::{ChunkProvenance, Provenance};
use fm_semantic_worker::embedding::{
    EMBEDDING_PREPROCESSING_VERSION, EmbeddingCacheKey, EmbeddingError, EmbeddingModelIdentity,
    VectorNormalization,
};
use fm_semantic_worker::ingestion::EmbeddingProvider;
use fm_semantic_worker::knowledge_retrieval::{
    DEFAULT_RANK_CONSTANT, FullTextCandidateIndex, KnowledgeRetrievalService,
    KnowledgeSourceRestriction,
};
use fm_semantic_worker::semantic_search::{ScoredRecord, SemanticCandidateIndex};
use fm_semantic_worker::semantic_storage::{
    DistanceMetric, LibraryIndexManifest, Occurrence, QueryFilters, SemanticCatalog,
    StagedGeneration, StagedRecord,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::knowledge::{
    KnowledgeNeed, KnowledgePlanner, KnowledgeScope, KnowledgeScopeSelector,
    KnowledgeSearchOptions, KnowledgeSearchRequest, KnowledgeSubject, RetrievalMode,
};
use crate::knowledge_release::KnowledgeReleaseAccess;
use crate::knowledge_search::{
    AuthorizedKnowledgeSearch, KnowledgeAuthorizationSnapshot, KnowledgeRetrievalCapability,
    KnowledgeRetrievalPartition, KnowledgeSearchCoordinator, KnowledgeSearchError,
    StaticKnowledgeAuthorizationRefresh,
};

/// Supported corpus schema version.
const CORPUS_SCHEMA_VERSION: u32 = 1;
/// Supported report schema version.
const REPORT_VERSION: u32 = 1;
/// Dimensions of the deterministic surrogate embedding.
const SURROGATE_DIMENSIONS: usize = 128;
/// Structural chunking contract recorded in the library manifest.
const CHUNKER_VERSION: &str = "structural/2";
/// Conversion contract recorded in the library manifest.
const CONVERTER_VERSION: &str = "text/1";
/// Shallow rank cutoff reported for every strategy.
const SHALLOW_CUTOFF: usize = 5;
/// Deep rank cutoff reported for every strategy, and the cutoff used for MRR.
const DEEP_CUTOFF: usize = 10;
/// Fixed media type recorded for every fixture occurrence.
const FIXTURE_MEDIA_TYPE: &str = "text/plain";
/// Fixed modification time recorded for every fixture occurrence.
const FIXTURE_MODIFIED_AT_MS: i64 = 1_000;
/// Byte written after every logically distinct group in a fingerprint.
const GROUP_SEPARATOR: u8 = 0x1d;

/// One structurally bounded chunk of a corpus document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorpusChunk {
    /// Stable opaque chunk identity, also used as the derived record identity.
    pub chunk_id: String,
    /// Structural heading hierarchy.
    pub section_path: Vec<String>,
    /// Complete chunk text in the document's language.
    pub text: String,
}

/// One authorized occurrence of a corpus document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorpusOccurrence {
    /// Stable occurrence identity.
    pub occurrence_id: String,
    /// Opaque host source identity.
    pub source_id: String,
    /// Enrolled root owning the occurrence.
    pub root_id: String,
    /// Workspace owning the occurrence.
    pub workspace_id: String,
    /// Whether the source can currently be opened.
    pub available: bool,
}

/// Publication history exercised by one corpus document.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CorpusLifecycle {
    /// One published generation.
    #[default]
    Published,
    /// A superseded first generation replaced by the current one.
    Updated,
    /// A published document whose occurrences were later deleted.
    Deleted,
}

/// One corpus document with its occurrences and chunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorpusDocument {
    /// Content identity shared by every occurrence.
    pub document_id: String,
    /// BCP-47-style language tag of the document body.
    pub language: String,
    /// Retrieval behavior this document represents.
    pub role: String,
    /// Publication history exercised by this document.
    #[serde(default)]
    pub lifecycle: CorpusLifecycle,
    /// Authorized occurrences of the document.
    pub occurrences: Vec<CorpusOccurrence>,
    /// Chunks of the superseded first generation.
    #[serde(default)]
    pub previous_chunks: Vec<CorpusChunk>,
    /// Chunks of the current generation.
    pub chunks: Vec<CorpusChunk>,
}

/// One local relevance judgment expressed against the corpus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalCase {
    /// Stable opaque case identity.
    pub id: String,
    /// Retrieval behavior represented by this case.
    pub category: String,
    /// Language of the question.
    pub language: String,
    /// Complete natural-language question, as a user would type it.
    pub question: String,
    /// Structured subject entered instead of the whole question.
    pub subject: String,
    /// Structured information needs.
    #[serde(default)]
    pub needs: Vec<KnowledgeNeed>,
    /// Structured lower-priority expansions.
    #[serde(default)]
    pub related_terms: Vec<String>,
    /// Sources a correct retrieval must surface.
    #[serde(default)]
    pub relevant_source_ids: BTreeSet<String>,
    /// Chunks a correct retrieval must surface.
    #[serde(default)]
    pub relevant_chunk_ids: BTreeSet<String>,
    /// Authorized sources that only match the question's application context.
    #[serde(default)]
    pub context_only_source_ids: BTreeSet<String>,
    /// Chunks that must never be returned, such as superseded or deleted ones.
    #[serde(default)]
    pub forbidden_chunk_ids: BTreeSet<String>,
    /// Whether no in-scope evidence qualifies.
    #[serde(default)]
    pub expected_no_evidence: bool,
}

impl RetrievalCase {
    /// Whether this case contributes to the recall and reciprocal-rank means.
    #[must_use]
    pub const fn scored(&self) -> bool {
        !self.expected_no_evidence
    }
}

/// A repository-owned multilingual retrieval corpus and its judgments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalCorpus {
    /// Corpus schema version.
    pub schema_version: u32,
    /// Stable corpus identity retained in every fingerprint.
    pub corpus_id: String,
    /// Tenant owning the evaluation library.
    pub tenant_id: String,
    /// Evaluation library identity.
    pub library_id: String,
    /// Roots every case is authorized to search.
    pub authorized_root_ids: Vec<String>,
    /// Declared cross-language token groups honored by the surrogate embedder.
    ///
    /// These groups are the *only* multilingual behavior in this evaluation.
    /// They stand in for what a real multilingual model would align, and no
    /// conclusion about a production model can be drawn from them.
    #[serde(default)]
    pub cross_language_terms: Vec<Vec<String>>,
    /// Corpus documents, including deliberately out-of-scope ones.
    pub documents: Vec<CorpusDocument>,
    /// Local relevance judgments.
    pub cases: Vec<RetrievalCase>,
}

impl RetrievalCorpus {
    /// Parses a repository corpus document.
    ///
    /// # Errors
    ///
    /// Returns [`KnowledgeEvaluationError::Json`] for malformed input and
    /// [`KnowledgeEvaluationError::UnsupportedSchema`] for an unknown version.
    pub fn parse(json: &str) -> Result<Self, KnowledgeEvaluationError> {
        let corpus: Self = serde_json::from_str(json)?;
        if corpus.schema_version != CORPUS_SCHEMA_VERSION {
            return Err(KnowledgeEvaluationError::UnsupportedSchema(
                corpus.schema_version,
            ));
        }
        Ok(corpus)
    }

    /// Validates that the corpus covers every behavior the release gate needs.
    ///
    /// # Errors
    ///
    /// Returns [`KnowledgeEvaluationError::IncompleteCorpus`] naming the first
    /// missing or contradictory requirement.
    pub fn validate(&self) -> Result<(), KnowledgeEvaluationError> {
        self.validate_shape()?;
        self.validate_judgments()?;
        Ok(())
    }

    fn validate_shape(&self) -> Result<(), KnowledgeEvaluationError> {
        if self.corpus_id.trim().is_empty()
            || self.tenant_id.trim().is_empty()
            || self.library_id.trim().is_empty()
        {
            return Err(KnowledgeEvaluationError::IncompleteCorpus(
                "corpus, tenant, and library identities are required".into(),
            ));
        }
        if self.authorized_root_ids.is_empty() {
            return Err(KnowledgeEvaluationError::IncompleteCorpus(
                "at least one authorized root is required".into(),
            ));
        }
        if self.documents.is_empty() || self.cases.is_empty() {
            return Err(KnowledgeEvaluationError::IncompleteCorpus(
                "documents and cases are required".into(),
            ));
        }
        let isolated = self
            .documents
            .iter()
            .flat_map(|document| &document.occurrences)
            .any(|occurrence| !self.authorized_root_ids.contains(&occurrence.root_id));
        if !isolated {
            return Err(KnowledgeEvaluationError::IncompleteCorpus(
                "an unauthorized root is required to prove scope isolation".into(),
            ));
        }
        Ok(())
    }

    fn validate_judgments(&self) -> Result<(), KnowledgeEvaluationError> {
        let sources = self.source_ids();
        let chunks = self.chunk_ids();
        for case in &self.cases {
            if case.subject.trim().is_empty() || case.question.trim().is_empty() {
                return Err(KnowledgeEvaluationError::IncompleteCorpus(format!(
                    "case `{}` needs both a question and a subject",
                    case.id
                )));
            }
            if case.expected_no_evidence
                && (!case.relevant_source_ids.is_empty() || !case.relevant_chunk_ids.is_empty())
            {
                return Err(KnowledgeEvaluationError::IncompleteCorpus(format!(
                    "case `{}` cannot both expect and forbid evidence",
                    case.id
                )));
            }
            for source_id in case
                .relevant_source_ids
                .iter()
                .chain(&case.context_only_source_ids)
            {
                if !sources.contains(source_id.as_str()) {
                    return Err(KnowledgeEvaluationError::IncompleteCorpus(format!(
                        "case `{}` names unknown source `{source_id}`",
                        case.id
                    )));
                }
            }
            for chunk_id in case
                .relevant_chunk_ids
                .iter()
                .chain(&case.forbidden_chunk_ids)
            {
                if !chunks.contains(chunk_id.as_str()) {
                    return Err(KnowledgeEvaluationError::IncompleteCorpus(format!(
                        "case `{}` names unknown chunk `{chunk_id}`",
                        case.id
                    )));
                }
            }
        }
        if !self.cases.iter().any(|case| case.expected_no_evidence) {
            return Err(KnowledgeEvaluationError::IncompleteCorpus(
                "at least one negative control is required".into(),
            ));
        }
        if !self
            .cases
            .iter()
            .any(|case| !case.forbidden_chunk_ids.is_empty())
        {
            return Err(KnowledgeEvaluationError::IncompleteCorpus(
                "update and deletion regressions require forbidden chunks".into(),
            ));
        }
        if self.match_sorting_case().is_none() {
            return Err(KnowledgeEvaluationError::IncompleteCorpus(
                "a `matchSorting` case with a context-only source is required".into(),
            ));
        }
        Ok(())
    }

    /// The explicit match-sorting regression case, when the corpus declares one.
    #[must_use]
    pub fn match_sorting_case(&self) -> Option<&RetrievalCase> {
        self.cases.iter().find(|case| {
            case.category == "matchSorting"
                && !case.relevant_source_ids.is_empty()
                && !case.context_only_source_ids.is_empty()
        })
    }

    /// Exact deterministic identity of the corpus content and its judgments.
    ///
    /// Every list is hashed under its own marker and cardinality, so no value
    /// can move between two lists — a chunk between generations, a term between
    /// judgment sets — without changing the digest.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.schema_version.to_le_bytes());
        hash_group(
            &mut hasher,
            "identity",
            [
                self.corpus_id.as_str(),
                self.tenant_id.as_str(),
                self.library_id.as_str(),
            ],
        );
        hash_group(
            &mut hasher,
            "authorizedRootIds",
            self.authorized_root_ids.iter().map(String::as_str),
        );
        hash_count(
            &mut hasher,
            "crossLanguageTerms",
            self.cross_language_terms.len(),
        );
        for group in &self.cross_language_terms {
            hash_group(
                &mut hasher,
                "crossLanguageGroup",
                group.iter().map(String::as_str),
            );
        }
        hash_count(&mut hasher, "documents", self.documents.len());
        for document in &self.documents {
            hash_document(&mut hasher, document);
        }
        hash_count(&mut hasher, "cases", self.cases.len());
        for case in &self.cases {
            hash_case(&mut hasher, case);
        }
        hexadecimal(hasher.finalize().as_slice())
    }

    fn source_ids(&self) -> BTreeSet<&str> {
        self.documents
            .iter()
            .flat_map(|document| &document.occurrences)
            .map(|occurrence| occurrence.source_id.as_str())
            .collect()
    }

    fn chunk_ids(&self) -> BTreeSet<&str> {
        self.documents
            .iter()
            .flat_map(|document| document.chunks.iter().chain(&document.previous_chunks))
            .map(|chunk| chunk.chunk_id.as_str())
            .collect()
    }

    /// Sources the authorized roots expose, in deterministic order.
    ///
    /// Deleted sources stay in this set deliberately. Withholding them here
    /// would make the deletion regression prove only that the evaluation's own
    /// allow-list omitted them; keeping them authorized makes catalog deletion
    /// the single thing that can remove them from retrieval.
    #[must_use]
    pub fn authorized_sources(&self) -> BTreeSet<String> {
        self.documents
            .iter()
            .flat_map(|document| &document.occurrences)
            .filter(|occurrence| self.authorized_root_ids.contains(&occurrence.root_id))
            .map(|occurrence| occurrence.source_id.clone())
            .collect()
    }

    /// Sources whose occurrences the corpus deletes after publication.
    #[must_use]
    pub fn deleted_sources(&self) -> BTreeSet<String> {
        self.documents
            .iter()
            .filter(|document| document.lifecycle == CorpusLifecycle::Deleted)
            .flat_map(|document| &document.occurrences)
            .map(|occurrence| occurrence.source_id.clone())
            .collect()
    }
}

fn hash_document(hasher: &mut Sha256, document: &CorpusDocument) {
    hash_group(
        hasher,
        "document",
        [
            document.document_id.as_str(),
            document.language.as_str(),
            document.role.as_str(),
        ],
    );
    hasher.update([
        u8::try_from(document.lifecycle as usize).unwrap_or_default(),
        GROUP_SEPARATOR,
    ]);
    hash_count(hasher, "occurrences", document.occurrences.len());
    for occurrence in &document.occurrences {
        hash_group(
            hasher,
            "occurrence",
            [
                occurrence.occurrence_id.as_str(),
                occurrence.source_id.as_str(),
                occurrence.root_id.as_str(),
                occurrence.workspace_id.as_str(),
            ],
        );
        hasher.update([u8::from(occurrence.available), GROUP_SEPARATOR]);
    }
    hash_chunks(hasher, "previousChunks", &document.previous_chunks);
    hash_chunks(hasher, "chunks", &document.chunks);
    hasher.update([GROUP_SEPARATOR]);
}

/// Hashes one generation's chunks under its own marker and cardinality, so
/// moving a chunk between the superseded and current generations — which is
/// exactly what the update regression turns on — changes the digest.
fn hash_chunks(hasher: &mut Sha256, marker: &str, chunks: &[CorpusChunk]) {
    hash_count(hasher, marker, chunks.len());
    for chunk in chunks {
        hash_field(hasher, &chunk.chunk_id);
        hash_group(
            hasher,
            "sectionPath",
            chunk.section_path.iter().map(String::as_str),
        );
        hash_field(hasher, &chunk.text);
    }
    hasher.update([GROUP_SEPARATOR]);
}

fn hash_case(hasher: &mut Sha256, case: &RetrievalCase) {
    hash_group(
        hasher,
        "case",
        [
            case.id.as_str(),
            case.category.as_str(),
            case.language.as_str(),
            case.question.as_str(),
            case.subject.as_str(),
        ],
    );
    hash_count(hasher, "needs", case.needs.len());
    for need in &case.needs {
        hash_field(hasher, &format!("{need:?}"));
    }
    hasher.update([GROUP_SEPARATOR]);
    hash_group(
        hasher,
        "relatedTerms",
        case.related_terms.iter().map(String::as_str),
    );
    hash_group(
        hasher,
        "relevantSourceIds",
        case.relevant_source_ids.iter().map(String::as_str),
    );
    hash_group(
        hasher,
        "relevantChunkIds",
        case.relevant_chunk_ids.iter().map(String::as_str),
    );
    hash_group(
        hasher,
        "contextOnlySourceIds",
        case.context_only_source_ids.iter().map(String::as_str),
    );
    hash_group(
        hasher,
        "forbiddenChunkIds",
        case.forbidden_chunk_ids.iter().map(String::as_str),
    );
    hasher.update([u8::from(case.expected_no_evidence), GROUP_SEPARATOR]);
}

/// Hashes a named, length-prefixed group of values.
fn hash_group<'a, Values>(hasher: &mut Sha256, marker: &str, values: Values)
where
    Values: IntoIterator<Item = &'a str>,
    Values::IntoIter: ExactSizeIterator,
{
    let values = values.into_iter();
    hash_count(hasher, marker, values.len());
    for value in values {
        hash_field(hasher, value);
    }
    hasher.update([GROUP_SEPARATOR]);
}

/// Hashes one group marker and its cardinality.
fn hash_count(hasher: &mut Sha256, marker: &str, count: usize) {
    hash_field(hasher, marker);
    hasher.update((count as u64).to_le_bytes());
}

fn hash_field(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn hexadecimal(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(7 + bytes.len() * 2);
    value.push_str("sha256:");
    for byte in bytes {
        let _ = write!(value, "{byte:02x}");
    }
    value
}

/// One of exactly five compared source-discovery strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeRetrievalStrategy {
    /// Today's control: the user's whole question embedded once.
    WholeQuestionVector,
    /// The structured subject alone, embedded once.
    SubjectVector,
    /// Structured subject and need expansions, fused across dense queries.
    StructuredSubjectNeedVector,
    /// Structured subject and need expansions over native full text only.
    StructuredFullText,
    /// Structured subject and need expansions fused across both routes.
    StructuredHybrid,
}

impl KnowledgeRetrievalStrategy {
    /// Every compared strategy in stable report order.
    pub const ALL: [Self; 5] = [
        Self::WholeQuestionVector,
        Self::SubjectVector,
        Self::StructuredSubjectNeedVector,
        Self::StructuredFullText,
        Self::StructuredHybrid,
    ];

    /// Stable identity used in fingerprints and reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WholeQuestionVector => "wholeQuestionVector",
            Self::SubjectVector => "subjectVector",
            Self::StructuredSubjectNeedVector => "structuredSubjectNeedVector",
            Self::StructuredFullText => "structuredFullText",
            Self::StructuredHybrid => "structuredHybrid",
        }
    }

    /// Physical retrieval mode requested from the coordinator.
    #[must_use]
    pub const fn mode(self) -> RetrievalMode {
        match self {
            Self::WholeQuestionVector | Self::SubjectVector | Self::StructuredSubjectNeedVector => {
                RetrievalMode::Semantic
            }
            Self::StructuredFullText => RetrievalMode::FullText,
            Self::StructuredHybrid => RetrievalMode::Hybrid,
        }
    }

    /// Whether the structured subject and needs are used instead of the question.
    #[must_use]
    pub const fn structured(self) -> bool {
        !matches!(self, Self::WholeQuestionVector)
    }

    /// Whether need expansions are planned in addition to the subject.
    #[must_use]
    pub const fn expands_needs(self) -> bool {
        matches!(
            self,
            Self::StructuredSubjectNeedVector | Self::StructuredFullText | Self::StructuredHybrid
        )
    }

    /// Operator-readable index migration this strategy would require.
    #[must_use]
    pub const fn migration_impact(self) -> &'static str {
        match self {
            Self::WholeQuestionVector | Self::SubjectVector | Self::StructuredSubjectNeedVector => {
                "No migration: reuses the published vector index and embedding space unchanged."
            }
            Self::StructuredFullText | Self::StructuredHybrid => {
                "Requires the native full-text index introduced with library manifest \
                 zvecSchemaVersion 2; every published generation must be re-indexed before the \
                 route becomes available."
            }
        }
    }
}

/// Deterministic retrieval-quality and cost metrics for one strategy.
///
/// Every field is reproducible from the corpus: no wall-clock timing and no
/// generated-answer measurement participates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyMetrics {
    /// Cases executed, including negative controls.
    pub evaluated_cases: usize,
    /// Cases contributing to recall and reciprocal rank.
    pub scored_cases: usize,
    /// Sources every case is authorized to search.
    ///
    /// This counts what authorization permits, not what the catalog can still
    /// resolve: a source whose occurrences were deleted after publication stays
    /// authorized so that catalog deletion, rather than the evaluation's own
    /// allow-list, is what keeps it out of every result.
    pub authorized_sources: usize,
    /// Mean relevant-source recall within the first five ranked sources.
    pub file_recall_at_5: f64,
    /// Mean relevant-source recall within the first ten ranked sources.
    pub file_recall_at_10: f64,
    /// Mean relevant-chunk recall within the first five ranked chunks.
    pub chunk_recall_at_5: f64,
    /// Mean relevant-chunk recall within the first ten ranked chunks.
    pub chunk_recall_at_10: f64,
    /// Mean reciprocal rank of the first relevant source within ten ranks.
    pub mean_reciprocal_rank: f64,
    /// Distinct relevant sources retrieved within ten ranks, summed over cases.
    pub relevant_unique_files: usize,
    /// Distinct relevant sources the corpus expects, summed over cases.
    pub expected_unique_files: usize,
    /// Retrieved sources within the top five that only matched application context.
    pub context_driven_irrelevant_hits: usize,
    /// Returned chunks that were superseded or deleted.
    pub stale_or_deleted_hits: usize,
    /// Returned sources outside the authorized scope.
    pub scope_violations: usize,
    /// Negative controls that returned any evidence.
    pub negative_control_false_positives: usize,
    /// Fraction of negative controls that returned evidence.
    pub negative_control_false_positive_rate: f64,
    /// Logical searches planned across every case.
    pub planned_searches: usize,
    /// Physical index queries issued across every case.
    pub index_queries: usize,
    /// Query embeddings computed across every case.
    pub query_embeddings: usize,
    /// Mean logical searches per case.
    pub average_planned_searches: f64,
    /// Mean query embeddings per case.
    pub average_query_embeddings: f64,
    /// Derived index bytes this strategy must retain for the corpus.
    pub retained_index_bytes: u64,
    /// Signed retained-byte change against the whole-question control.
    pub storage_impact_bytes: i64,
    /// Operator-readable index migration required by this strategy.
    pub migration_impact: String,
}

/// Wall-clock observation, which depends on the machine that produced it.
///
/// Each observation is the sum of two measured phases, planning the request and
/// executing that plan through the coordinator, so it covers everything a user
/// waits for after the request arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyObservation {
    /// Median planning-plus-retrieval time in microseconds.
    pub p50_micros: u64,
    /// 95th percentile planning-plus-retrieval time in microseconds.
    pub p95_micros: u64,
    /// Slowest planning-plus-retrieval time in microseconds.
    pub maximum_micros: u64,
    /// Median time spent planning, which the totals above include.
    pub planning_p50_micros: u64,
    /// Median time spent retrieving, which the totals above include.
    pub retrieval_p50_micros: u64,
}

/// Deterministic metrics, latency, and exact pipeline identity for a strategy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyEvaluation {
    /// Compared strategy.
    pub strategy: KnowledgeRetrievalStrategy,
    /// Exact deterministic identity of corpus, planner, route, and policy.
    pub pipeline_fingerprint: String,
    /// Reproducible retrieval-quality and cost metrics.
    pub metrics: StrategyMetrics,
    /// Machine-dependent timing recorded alongside the deterministic metrics.
    pub latency: LatencyObservation,
}

/// One strategy's behavior on the match-sorting regression case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSortingObservation {
    /// Strategy that produced the ranking.
    pub strategy: KnowledgeRetrievalStrategy,
    /// One-based ranks of the expected procedure and example sources.
    pub expected_ranks: Vec<usize>,
    /// One-based rank of the context-only sorting document, when returned.
    pub context_only_rank: Option<usize>,
}

/// Proof that structured retrieval sorts real matches above context-only text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSortingRegression {
    /// Corpus case exercising the regression.
    pub case_id: String,
    /// Procedure and example sources that must rank strongly.
    pub expected_source_ids: Vec<String>,
    /// Sources that only mention sorting as application context.
    pub context_only_source_ids: Vec<String>,
    /// Per-strategy ranking behavior.
    pub observations: Vec<MatchSortingObservation>,
    /// Whether structured hybrid retrieval ranked every expected source first.
    pub passed: bool,
}

/// Comparable evaluation of every strategy over one corpus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRetrievalComparison {
    /// Corpus identity.
    pub corpus_id: String,
    /// Exact corpus fingerprint.
    pub corpus_fingerprint: String,
    /// Exact planner contract identity.
    pub planner_version: String,
    /// Exact fusion contract identity.
    pub fusion_version: String,
    /// Fingerprint of release-critical retrieval and index source code.
    pub release_candidate_fingerprint: String,
    /// Every compared strategy in stable order.
    pub strategies: Vec<StrategyEvaluation>,
    /// Match-sorting regression evidence.
    pub match_sorting_regression: MatchSortingRegression,
}

impl KnowledgeRetrievalComparison {
    /// Returns one strategy's evaluation.
    #[must_use]
    pub fn strategy(&self, strategy: KnowledgeRetrievalStrategy) -> Option<&StrategyEvaluation> {
        self.strategies
            .iter()
            .find(|item| item.strategy == strategy)
    }
}

/// Measured release decision for making Structured Knowledge Search visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvaluationDecision {
    /// Every gate passed on production-equivalent measurements.
    Go,
    /// At least one gate failed, or the measurements are not production ones.
    NoGo,
}

/// Reproducible checked-in release-gate report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationReport {
    /// Report schema version.
    pub report_version: u32,
    /// Corpus identity.
    pub corpus_id: String,
    /// Exact corpus fingerprint.
    pub corpus_fingerprint: String,
    /// Exact planner contract identity.
    pub planner_version: String,
    /// Exact fusion contract identity.
    pub fusion_version: String,
    /// Fingerprint of release-critical retrieval and index source code.
    pub release_candidate_fingerprint: String,
    /// Honest description of what produced these numbers.
    pub measurement_basis: String,
    /// Explicit limitations a reader must apply to every number in this report.
    pub limitations: Vec<String>,
    /// Whether retrieval ran on production components and hardware.
    pub production_measurement: bool,
    /// Maximum accepted 95th percentile latency for a go.
    pub maximum_p95_micros: u64,
    /// Every compared strategy in stable order.
    pub strategies: Vec<StrategyEvaluation>,
    /// Match-sorting regression evidence.
    pub match_sorting_regression: MatchSortingRegression,
    /// Deterministically derived reasons a go is not available.
    pub blocking_reasons: Vec<String>,
    /// Decision derived from the recorded evidence.
    pub decision: EvaluationDecision,
}

impl EvaluationReport {
    /// Parses a checked-in report.
    ///
    /// # Errors
    ///
    /// Returns [`KnowledgeEvaluationError::Json`] for malformed input.
    pub fn parse(json: &str) -> Result<Self, KnowledgeEvaluationError> {
        Ok(serde_json::from_str(json)?)
    }

    /// Builds a report from a comparison and its measurement conditions.
    #[must_use]
    pub fn from_comparison(
        comparison: &KnowledgeRetrievalComparison,
        measurement_basis: impl Into<String>,
        limitations: Vec<String>,
        production_measurement: bool,
        maximum_p95_micros: u64,
    ) -> Self {
        let blocking_reasons = blocking_reasons(
            &comparison.strategies,
            &comparison.match_sorting_regression,
            production_measurement,
            maximum_p95_micros,
        );
        let decision = if blocking_reasons.is_empty() {
            EvaluationDecision::Go
        } else {
            EvaluationDecision::NoGo
        };
        Self {
            report_version: REPORT_VERSION,
            corpus_id: comparison.corpus_id.clone(),
            corpus_fingerprint: comparison.corpus_fingerprint.clone(),
            planner_version: comparison.planner_version.clone(),
            fusion_version: comparison.fusion_version.clone(),
            release_candidate_fingerprint: comparison.release_candidate_fingerprint.clone(),
            measurement_basis: measurement_basis.into(),
            limitations,
            production_measurement,
            maximum_p95_micros,
            strategies: comparison.strategies.clone(),
            match_sorting_regression: comparison.match_sorting_regression.clone(),
            blocking_reasons,
            decision,
        }
    }

    /// Serializes the report exactly as it is checked in.
    ///
    /// # Errors
    ///
    /// Returns [`KnowledgeEvaluationError::Json`] when encoding fails.
    pub fn to_checked_in_json(&self) -> Result<String, KnowledgeEvaluationError> {
        let mut json = serde_json::to_string_pretty(self)?;
        json.push('\n');
        Ok(json)
    }

    /// Recomputes the decision from the recorded evidence and rejects drift.
    ///
    /// A report cannot claim a go that its own metrics do not support, and it
    /// cannot claim a go at all without production measurements.
    ///
    /// # Errors
    ///
    /// Returns [`KnowledgeEvaluationError::InvalidReport`] describing the first
    /// inconsistency.
    pub fn validate(&self) -> Result<(), KnowledgeEvaluationError> {
        if self.report_version != REPORT_VERSION {
            return Err(KnowledgeEvaluationError::UnsupportedSchema(
                self.report_version,
            ));
        }
        if self.measurement_basis.trim().is_empty() || self.maximum_p95_micros == 0 {
            return Err(KnowledgeEvaluationError::InvalidReport(
                "a report needs a measurement basis and a latency ceiling".into(),
            ));
        }
        if self.release_candidate_fingerprint != release_candidate_fingerprint() {
            return Err(KnowledgeEvaluationError::InvalidReport(
                "the report was not measured against the current retrieval implementation".into(),
            ));
        }
        if !self.production_measurement && self.limitations.is_empty() {
            return Err(KnowledgeEvaluationError::InvalidReport(
                "a non-production report must state its limitations explicitly".into(),
            ));
        }
        self.validate_strategies()?;
        let regression_passed = recorded_sorting_regression_passed(&self.match_sorting_regression);
        if regression_passed != self.match_sorting_regression.passed {
            return Err(KnowledgeEvaluationError::InvalidReport(
                "the match-sorting decision does not follow from its recorded ranks".into(),
            ));
        }
        let expected = blocking_reasons(
            &self.strategies,
            &self.match_sorting_regression,
            self.production_measurement,
            self.maximum_p95_micros,
        );
        if expected != self.blocking_reasons {
            return Err(KnowledgeEvaluationError::InvalidReport(
                "recorded blocking reasons do not follow from the recorded metrics".into(),
            ));
        }
        let decision = if expected.is_empty() {
            EvaluationDecision::Go
        } else {
            EvaluationDecision::NoGo
        };
        if decision != self.decision {
            return Err(KnowledgeEvaluationError::InvalidReport(
                "recorded decision does not follow from the recorded metrics".into(),
            ));
        }
        Ok(())
    }

    fn validate_strategies(&self) -> Result<(), KnowledgeEvaluationError> {
        if self
            .strategies
            .iter()
            .map(|item| item.strategy)
            .collect::<Vec<_>>()
            != KnowledgeRetrievalStrategy::ALL
        {
            return Err(KnowledgeEvaluationError::InvalidReport(
                "exactly the five compared strategies are required, in order".into(),
            ));
        }
        let fingerprints = self
            .strategies
            .iter()
            .map(|item| item.pipeline_fingerprint.as_str())
            .collect::<BTreeSet<_>>();
        if fingerprints.len() != self.strategies.len() {
            return Err(KnowledgeEvaluationError::InvalidReport(
                "each strategy needs its own exact pipeline fingerprint".into(),
            ));
        }
        for evaluation in &self.strategies {
            if !metrics_are_valid(&evaluation.metrics)
                || evaluation.latency.p50_micros > evaluation.latency.p95_micros
                || evaluation.latency.p95_micros > evaluation.latency.maximum_micros
                || evaluation.latency.planning_p50_micros > evaluation.latency.maximum_micros
                || evaluation.latency.retrieval_p50_micros > evaluation.latency.maximum_micros
            {
                return Err(KnowledgeEvaluationError::InvalidReport(format!(
                    "strategy `{}` recorded impossible metrics",
                    evaluation.strategy.as_str()
                )));
            }
            if evaluation.metrics.authorized_sources
                != self.strategies[0].metrics.authorized_sources
                || evaluation.metrics.evaluated_cases != self.strategies[0].metrics.evaluated_cases
            {
                return Err(KnowledgeEvaluationError::InvalidReport(
                    "strategies must be compared on identical authorized content".into(),
                ));
            }
        }
        Ok(())
    }
}

fn metrics_are_valid(metrics: &StrategyMetrics) -> bool {
    metrics.evaluated_cases > 0
        && metrics.scored_cases > 0
        && metrics.scored_cases <= metrics.evaluated_cases
        && metrics.authorized_sources > 0
        && metrics.relevant_unique_files <= metrics.expected_unique_files
        && !metrics.migration_impact.trim().is_empty()
        && [
            metrics.file_recall_at_5,
            metrics.file_recall_at_10,
            metrics.chunk_recall_at_5,
            metrics.chunk_recall_at_10,
            metrics.mean_reciprocal_rank,
            metrics.negative_control_false_positive_rate,
        ]
        .into_iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        && metrics.file_recall_at_10 + f64::EPSILON >= metrics.file_recall_at_5
        && metrics.chunk_recall_at_10 + f64::EPSILON >= metrics.chunk_recall_at_5
}

/// Derives, in stable order, every reason the corpus evidence blocks a go.
fn blocking_reasons(
    strategies: &[StrategyEvaluation],
    regression: &MatchSortingRegression,
    production_measurement: bool,
    maximum_p95_micros: u64,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if !production_measurement {
        reasons.push(
            "Retrieval ran against a deterministic repository fixture with a surrogate embedding \
             model and in-process indexes; no production or cross-platform measurement exists."
                .into(),
        );
    }
    let (Some(control), Some(candidate)) = (
        strategies
            .iter()
            .find(|item| item.strategy == KnowledgeRetrievalStrategy::WholeQuestionVector),
        strategies
            .iter()
            .find(|item| item.strategy == KnowledgeRetrievalStrategy::StructuredHybrid),
    ) else {
        reasons.push(
            "The whole-question control and structured hybrid candidate are required.".into(),
        );
        return reasons;
    };
    if !regression.passed {
        reasons.push(
            "The match-sorting regression failed: procedure and example sources did not rank \
             above the context-only sorting document."
                .into(),
        );
    }
    if candidate.metrics.scope_violations != 0 || candidate.metrics.stale_or_deleted_hits != 0 {
        reasons.push(
            "Structured hybrid retrieval returned out-of-scope, superseded, or deleted evidence."
                .into(),
        );
    }
    if candidate.metrics.file_recall_at_10 < control.metrics.file_recall_at_10 + 0.10 {
        reasons.push(
            "Structured hybrid retrieval did not improve source recall at ten by the required \
             0.10 over whole-question vector search."
                .into(),
        );
    }
    if candidate.metrics.mean_reciprocal_rank < control.metrics.mean_reciprocal_rank + 0.05 {
        reasons.push(
            "Structured hybrid retrieval did not improve mean reciprocal rank by the required \
             0.05 over whole-question vector search."
                .into(),
        );
    }
    if candidate.metrics.context_driven_irrelevant_hits
        > control.metrics.context_driven_irrelevant_hits
    {
        reasons.push(
            "Structured hybrid retrieval returned more context-driven irrelevant sources than \
             whole-question vector search."
                .into(),
        );
    }
    if candidate.metrics.negative_control_false_positive_rate
        > control.metrics.negative_control_false_positive_rate
    {
        reasons.push(
            "Structured hybrid retrieval answered more negative controls than whole-question \
             vector search."
                .into(),
        );
    }
    if candidate.latency.p95_micros > maximum_p95_micros {
        reasons.push(
            "Structured hybrid retrieval exceeded the accepted 95th percentile latency.".into(),
        );
    }
    reasons
}

/// One published record, exposed so an external index can be filled with
/// exactly the content the catalog published.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationIndexRecord {
    /// Derived record identity.
    pub record_id: String,
    /// Tenant filter field.
    pub tenant_id: String,
    /// Library filter field.
    pub library_id: String,
    /// Root filter field.
    pub root_id: String,
    /// Workspace filter field.
    pub workspace_id: String,
    /// Indexed media type.
    pub media_type: String,
    /// Source modification time.
    pub modified_at_ms: i64,
    /// Published generation.
    pub generation: u64,
    /// Complete chunk text used by lexical retrieval.
    pub content: String,
    /// Normalized surrogate embedding used by dense retrieval.
    pub vector: Vec<f32>,
}

/// Dense and lexical candidate indexes one evaluation run retrieves through.
pub struct EvaluationIndexes {
    /// Stable identity of the candidate index backend behind both routes.
    ///
    /// Retrieval quality depends on which index answered, so this identity is
    /// hashed into every pipeline fingerprint: an in-process surrogate run and
    /// a native Zvec run over the same corpus produce distinct fingerprints and
    /// can never be mistaken for one another.
    pub backend_identity: String,
    /// Dense candidate index.
    pub semantic: Arc<dyn SemanticCandidateIndex>,
    /// Lexical candidate index.
    pub full_text: Arc<dyn FullTextCandidateIndex>,
}

/// Backend identity of the deterministic in-process candidate indexes.
pub const SURROGATE_INDEX_IDENTITY: &str = "in-process-surrogate/1";

/// Release visibility the evaluation harness runs under.
///
/// The evaluation is what *produces* a release decision, so it must never
/// depend on one. A release-profile test build resolves
/// [`KnowledgeReleaseAccess::from_build`] to `Unqualified`, which would make
/// the coordinator refuse every case before it reached the worker.
const EVALUATION_RELEASE_ACCESS: KnowledgeReleaseAccess = KnowledgeReleaseAccess::Developer;

/// Builds the coordinator every evaluated case is executed through.
fn evaluation_coordinator(
    capability: Arc<dyn KnowledgeRetrievalCapability>,
) -> KnowledgeSearchCoordinator {
    KnowledgeSearchCoordinator::with_release_access(capability, EVALUATION_RELEASE_ACCESS)
}

/// Runs the corpus through the production planner, coordinator, and retrieval
/// service once per strategy and scores the results.
///
/// Retrieval uses the deterministic surrogate indexes. Use
/// [`evaluate_knowledge_retrieval_with`] to run the same corpus over a real
/// native index instead.
///
/// # Errors
///
/// Returns corpus, catalog, planning, or retrieval failures.
pub async fn evaluate_knowledge_retrieval(
    corpus: &RetrievalCorpus,
) -> Result<KnowledgeRetrievalComparison, KnowledgeEvaluationError> {
    evaluate_knowledge_retrieval_with(corpus, |statistics, records| {
        let index = Arc::new(SurrogateIndex::new(statistics, records));
        EvaluationIndexes {
            backend_identity: SURROGATE_INDEX_IDENTITY.into(),
            semantic: index.clone(),
            full_text: index,
        }
    })
    .await
}

/// Runs the corpus with caller-supplied candidate indexes.
///
/// The builder receives the corpus lexical statistics and every published
/// record, including superseded, deleted, and unauthorized ones, so that the
/// catalog — not the index — remains what removes them. It must declare a
/// stable [`EvaluationIndexes::backend_identity`], which every pipeline
/// fingerprint retains.
///
/// # Errors
///
/// Returns corpus, catalog, planning, or retrieval failures, and
/// [`KnowledgeEvaluationError::MissingIndexBackendIdentity`] when the builder
/// declares no backend identity.
pub async fn evaluate_knowledge_retrieval_with<Build>(
    corpus: &RetrievalCorpus,
    build: Build,
) -> Result<KnowledgeRetrievalComparison, KnowledgeEvaluationError>
where
    Build: FnOnce(Arc<LexicalStatistics>, &[EvaluationIndexRecord]) -> EvaluationIndexes,
{
    corpus.validate()?;
    let directory = tempfile::tempdir()?;
    let harness = CorpusHarness::build(corpus, &directory.path().join("catalog.sqlite"), build)?;
    let coordinator = evaluation_coordinator(Arc::new(KnowledgeRetrievalService::new(
        harness.catalog.clone(),
        Some(harness.embedder.clone()),
        Some(Arc::new(CountingSemanticIndex {
            inner: harness.indexes.semantic.clone(),
            queries: harness.queries.clone(),
        })),
        Some(Arc::new(CountingFullTextIndex {
            inner: harness.indexes.full_text.clone(),
            queries: harness.queries.clone(),
        })),
    )));

    let mut strategies = Vec::with_capacity(KnowledgeRetrievalStrategy::ALL.len());
    let mut sorting_observations = Vec::new();
    let match_sorting_case = corpus
        .match_sorting_case()
        .ok_or_else(|| {
            KnowledgeEvaluationError::IncompleteCorpus("a match-sorting case is required".into())
        })?
        .clone();
    for strategy in KnowledgeRetrievalStrategy::ALL {
        let mut observations = Vec::with_capacity(corpus.cases.len());
        for case in &corpus.cases {
            observations.push(harness.run(&coordinator, case, strategy).await?);
        }
        if let Some(observation) = observations
            .iter()
            .find(|item| item.case_id == match_sorting_case.id)
        {
            sorting_observations.push(sorting_observation(
                strategy,
                observation,
                &match_sorting_case,
            ));
        }
        strategies.push(harness.score(corpus, strategy, &observations));
    }
    let control_bytes = strategies
        .first()
        .map_or(0, |item| item.metrics.retained_index_bytes);
    for evaluation in &mut strategies {
        evaluation.metrics.storage_impact_bytes =
            i64::try_from(evaluation.metrics.retained_index_bytes).unwrap_or(i64::MAX)
                - i64::try_from(control_bytes).unwrap_or(i64::MAX);
    }
    let regression = MatchSortingRegression {
        case_id: match_sorting_case.id.clone(),
        expected_source_ids: match_sorting_case
            .relevant_source_ids
            .iter()
            .cloned()
            .collect(),
        context_only_source_ids: match_sorting_case
            .context_only_source_ids
            .iter()
            .cloned()
            .collect(),
        passed: sorting_regression_passed(&sorting_observations, &match_sorting_case),
        observations: sorting_observations,
    };
    Ok(KnowledgeRetrievalComparison {
        corpus_id: corpus.corpus_id.clone(),
        corpus_fingerprint: corpus.fingerprint(),
        planner_version: KnowledgePlanner::VERSION.into(),
        fusion_version: format!("reciprocal-rank-fusion/{DEFAULT_RANK_CONSTANT}"),
        release_candidate_fingerprint: release_candidate_fingerprint(),
        strategies,
        match_sorting_regression: regression,
    })
}

/// Fingerprints every source file that can alter the evaluated retrieval path.
#[must_use]
pub fn release_candidate_fingerprint() -> String {
    const SOURCES: &[(&str, &[u8])] = &[
        ("knowledge.rs", include_bytes!("knowledge.rs")),
        ("knowledge_dsl.rs", include_bytes!("knowledge_dsl.rs")),
        (
            "knowledge_evaluation.rs",
            include_bytes!("knowledge_evaluation.rs"),
        ),
        (
            "knowledge_mapping.rs",
            include_bytes!("knowledge_mapping.rs"),
        ),
        (
            "knowledge_release.rs",
            include_bytes!("knowledge_release.rs"),
        ),
        ("knowledge_search.rs", include_bytes!("knowledge_search.rs")),
        (
            "knowledge_service.rs",
            include_bytes!("knowledge_service.rs"),
        ),
        (
            "worker/embedding.rs",
            include_bytes!("../../fm-semantic-worker/src/embedding.rs"),
        ),
        (
            "worker/knowledge_retrieval.rs",
            include_bytes!("../../fm-semantic-worker/src/knowledge_retrieval.rs"),
        ),
        (
            "worker/zvec_storage.rs",
            include_bytes!("../../fm-semantic-worker/src/zvec_storage.rs"),
        ),
        ("Cargo.lock", include_bytes!("../../../Cargo.lock")),
    ];
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "knowledge-release-candidate/1");
    for (name, source) in SOURCES {
        hash_field(&mut hasher, name);
        hasher.update(source.len().to_le_bytes());
        hasher.update(source);
        hasher.update([GROUP_SEPARATOR]);
    }
    hexadecimal(hasher.finalize().as_slice())
}

fn sorting_observation(
    strategy: KnowledgeRetrievalStrategy,
    observation: &CaseObservation,
    case: &RetrievalCase,
) -> MatchSortingObservation {
    let rank_of = |source_id: &String| {
        observation
            .ranked_source_ids
            .iter()
            .position(|candidate| candidate == source_id)
            .map(|index| index + 1)
    };
    MatchSortingObservation {
        strategy,
        expected_ranks: case
            .relevant_source_ids
            .iter()
            .filter_map(rank_of)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        context_only_rank: case
            .context_only_source_ids
            .iter()
            .filter_map(rank_of)
            .min(),
    }
}

/// The regression holds when structured hybrid retrieval ranks every expected
/// source, and ranks all of them above any context-only sorting document.
fn sorting_regression_passed(
    observations: &[MatchSortingObservation],
    case: &RetrievalCase,
) -> bool {
    match_sorting_ranks_pass(
        observations,
        case.relevant_source_ids.len(),
        !case.relevant_source_ids.is_empty() && !case.context_only_source_ids.is_empty(),
    )
}

fn recorded_sorting_regression_passed(regression: &MatchSortingRegression) -> bool {
    let expected_sources = regression
        .expected_source_ids
        .iter()
        .collect::<BTreeSet<_>>();
    let context_sources = regression
        .context_only_source_ids
        .iter()
        .collect::<BTreeSet<_>>();
    let sources_are_valid = !expected_sources.is_empty()
        && expected_sources.len() == regression.expected_source_ids.len()
        && !context_sources.is_empty()
        && context_sources.len() == regression.context_only_source_ids.len()
        && expected_sources.is_disjoint(&context_sources);
    match_sorting_ranks_pass(
        &regression.observations,
        expected_sources.len(),
        sources_are_valid,
    )
}

fn match_sorting_ranks_pass(
    observations: &[MatchSortingObservation],
    expected_source_count: usize,
    sources_are_valid: bool,
) -> bool {
    if !sources_are_valid {
        return false;
    }
    observations
        .iter()
        .find(|item| item.strategy == KnowledgeRetrievalStrategy::StructuredHybrid)
        .is_some_and(|item| {
            let unique_ranks = item.expected_ranks.iter().copied().collect::<BTreeSet<_>>();
            item.expected_ranks.len() == expected_source_count
                && unique_ranks.len() == expected_source_count
                && item
                    .expected_ranks
                    .iter()
                    .all(|rank| (1..=SHALLOW_CUTOFF).contains(rank))
                && item
                    .context_only_rank
                    .is_none_or(|context| item.expected_ranks.iter().all(|rank| *rank < context))
        })
}

/// One case executed under one strategy.
#[derive(Debug, Clone)]
struct CaseObservation {
    case_id: String,
    ranked_source_ids: Vec<String>,
    ranked_chunk_ids: Vec<String>,
    returned_chunk_ids: BTreeSet<String>,
    planned_searches: usize,
    index_queries: usize,
    query_embeddings: usize,
    /// Time spent turning the request into a plan.
    planning_micros: u64,
    /// Time spent executing that plan through the coordinator.
    retrieval_micros: u64,
    /// Reported latency, which is exactly both phases.
    latency_micros: u64,
}

/// Published corpus state plus the deterministic indexes retrieval runs over.
struct CorpusHarness {
    catalog: SemanticCatalog,
    embedder: Arc<SurrogateEmbedder>,
    indexes: EvaluationIndexes,
    queries: Arc<AtomicUsize>,
    tenant_id: String,
    library_id: String,
    authorized_sources: BTreeSet<String>,
    vector_bytes: u64,
    full_text_bytes: u64,
}

impl CorpusHarness {
    fn build<Build>(
        corpus: &RetrievalCorpus,
        catalog_path: &std::path::Path,
        build: Build,
    ) -> Result<Self, KnowledgeEvaluationError>
    where
        Build: FnOnce(Arc<LexicalStatistics>, &[EvaluationIndexRecord]) -> EvaluationIndexes,
    {
        let statistics = Arc::new(LexicalStatistics::new(
            &corpus.cross_language_terms,
            &corpus
                .documents
                .iter()
                .flat_map(|document| document.chunks.iter().chain(&document.previous_chunks))
                .map(|chunk| chunk.text.clone())
                .collect::<Vec<_>>(),
        ));
        let embedder = Arc::new(SurrogateEmbedder::new(statistics.clone()));
        let catalog = SemanticCatalog::open(catalog_path)?;
        catalog.register_library(&corpus.tenant_id, &corpus.library_id, &manifest())?;
        let mut records = Vec::new();
        let mut vector_bytes = 0u64;
        let mut full_text_bytes = 0u64;
        for document in &corpus.documents {
            for (generation, chunks) in &publication_plan(document) {
                stage_and_publish(&catalog, corpus, document, *generation, chunks, &embedder)?;
                for chunk in *chunks {
                    for (copy, occurrence) in document.occurrences.iter().enumerate() {
                        let vector = embedder.vector(&chunk.text);
                        vector_bytes = vector_bytes.saturating_add(
                            vector
                                .len()
                                .saturating_mul(std::mem::size_of::<f32>())
                                .try_into()
                                .unwrap_or(u64::MAX),
                        );
                        full_text_bytes = full_text_bytes
                            .saturating_add(chunk.text.len().try_into().unwrap_or(u64::MAX));
                        records.push(EvaluationIndexRecord {
                            record_id: record_id_for(&chunk.chunk_id, copy),
                            tenant_id: corpus.tenant_id.clone(),
                            library_id: corpus.library_id.clone(),
                            root_id: occurrence.root_id.clone(),
                            workspace_id: occurrence.workspace_id.clone(),
                            media_type: FIXTURE_MEDIA_TYPE.into(),
                            modified_at_ms: FIXTURE_MODIFIED_AT_MS,
                            generation: *generation,
                            content: chunk.text.clone(),
                            vector,
                        });
                    }
                }
            }
            if document.lifecycle == CorpusLifecycle::Deleted {
                for occurrence in &document.occurrences {
                    catalog.delete_occurrence(&occurrence.occurrence_id)?;
                }
            }
        }
        Ok(Self {
            catalog,
            embedder,
            indexes: {
                let indexes = build(statistics, &records);
                if indexes.backend_identity.trim().is_empty() {
                    return Err(KnowledgeEvaluationError::MissingIndexBackendIdentity);
                }
                indexes
            },
            queries: Arc::new(AtomicUsize::new(0)),
            tenant_id: corpus.tenant_id.clone(),
            library_id: corpus.library_id.clone(),
            authorized_sources: corpus.authorized_sources(),
            vector_bytes,
            full_text_bytes,
        })
    }

    async fn run(
        &self,
        coordinator: &KnowledgeSearchCoordinator,
        case: &RetrievalCase,
        strategy: KnowledgeRetrievalStrategy,
    ) -> Result<CaseObservation, KnowledgeEvaluationError> {
        // Reported latency is planning plus retrieval, measured as two explicit
        // phases and summed. Everything the host resolved before the request
        // arrived — the request itself and the authorization snapshot — is
        // prepared outside both phases.
        let request = self.request(case, strategy);
        let request_id = Uuid::new_v4();
        let partitions = self.partitions();
        let snapshot = self.snapshot();
        let eligible = self.authorized_sources.len().try_into().unwrap_or(u64::MAX);
        let refresh = StaticKnowledgeAuthorizationRefresh(self.snapshot());
        let queries_before = self.queries.load(Ordering::Relaxed);
        let embeddings_before = self.embedder.embeddings();
        let planning_started = Instant::now();
        let plan = KnowledgePlanner::plan(&request, None)?;
        let planning_micros = elapsed_micros(planning_started);
        let planned_searches = plan.searches.len();
        let authorized = AuthorizedKnowledgeSearch {
            plan,
            partitions,
            scope_is_exact: true,
            snapshot,
            eligible,
        };
        let retrieval_started = Instant::now();
        let outcome = coordinator
            .execute(request_id, authorized, &refresh, &CancellationToken::new())
            .await?;
        let retrieval_micros = elapsed_micros(retrieval_started);
        let mut ranked_source_ids = Vec::new();
        let mut ranked_chunk_ids = Vec::new();
        let mut returned_chunk_ids = BTreeSet::new();
        let mut primaries = outcome
            .evidence
            .iter()
            .filter(|row| !row.adjacent)
            .collect::<Vec<_>>();
        primaries.sort_by_key(|row| row.final_rank);
        for row in primaries {
            // Every authorized occurrence of one chunk is disclosed together,
            // so a duplicate copy counts as found rather than as a miss.
            for source_id in std::iter::once(&row.source_id).chain(&row.duplicate_source_ids) {
                if !ranked_source_ids.contains(source_id) {
                    ranked_source_ids.push(source_id.clone());
                }
            }
            ranked_chunk_ids.push(row.record_id.clone());
        }
        for row in &outcome.evidence {
            returned_chunk_ids.insert(row.record_id.clone());
        }
        Ok(CaseObservation {
            case_id: case.id.clone(),
            ranked_source_ids,
            ranked_chunk_ids,
            returned_chunk_ids,
            planned_searches,
            index_queries: self.queries.load(Ordering::Relaxed) - queries_before,
            query_embeddings: self.embedder.embeddings() - embeddings_before,
            planning_micros,
            retrieval_micros,
            latency_micros: planning_micros.saturating_add(retrieval_micros),
        })
    }

    fn request(
        &self,
        case: &RetrievalCase,
        strategy: KnowledgeRetrievalStrategy,
    ) -> KnowledgeSearchRequest {
        KnowledgeSearchRequest {
            subjects: vec![KnowledgeSubject {
                text: if strategy.structured() {
                    case.subject.clone()
                } else {
                    case.question.clone()
                },
            }],
            needs: if strategy.expands_needs() {
                case.needs.clone()
            } else {
                Vec::new()
            },
            related_terms: if strategy.expands_needs() {
                case.related_terms.clone()
            } else {
                Vec::new()
            },
            scopes: vec![KnowledgeScope {
                tenant_id: self.tenant_id.clone(),
                library_id: self.library_id.clone(),
                selector: KnowledgeScopeSelector::WholeLibrary,
            }],
            mode: strategy.mode(),
            options: evaluation_options(),
        }
    }

    /// Mirrors the production `restricted_partitions` path: one bounded
    /// partition whose exact authorized source set describes the scope, so the
    /// worker applies authorization before spending any budget and the whole
    /// authorized corpus is ranked once rather than per root.
    fn partitions(&self) -> Vec<KnowledgeRetrievalPartition> {
        vec![KnowledgeRetrievalPartition {
            filters: QueryFilters {
                tenant_id: self.tenant_id.clone(),
                library_id: Some(self.library_id.clone()),
                include_unavailable: true,
                ..QueryFilters::default()
            },
            restriction: KnowledgeSourceRestriction {
                allowed_source_ids: self.authorized_sources.clone(),
            },
        }]
    }

    fn snapshot(&self) -> KnowledgeAuthorizationSnapshot {
        KnowledgeAuthorizationSnapshot {
            allowed_source_ids: self.authorized_sources.clone(),
            unavailable_source_ids: BTreeSet::new(),
            fingerprints: self
                .authorized_sources
                .iter()
                .map(|source_id| (source_id.clone(), content_hash(source_id)))
                .collect(),
        }
    }

    fn score(
        &self,
        corpus: &RetrievalCorpus,
        strategy: KnowledgeRetrievalStrategy,
        observations: &[CaseObservation],
    ) -> StrategyEvaluation {
        let mut totals = ScoreTotals::default();
        let by_id = observations
            .iter()
            .map(|item| (item.case_id.as_str(), item))
            .collect::<BTreeMap<_, _>>();
        for case in &corpus.cases {
            let Some(observation) = by_id.get(case.id.as_str()) else {
                continue;
            };
            totals.accumulate(case, observation, &self.authorized_sources);
        }
        let mut latencies = observations
            .iter()
            .map(|item| item.latency_micros)
            .collect::<Vec<_>>();
        latencies.sort_unstable();
        let mut planning = observations
            .iter()
            .map(|item| item.planning_micros)
            .collect::<Vec<_>>();
        planning.sort_unstable();
        let mut retrieval = observations
            .iter()
            .map(|item| item.retrieval_micros)
            .collect::<Vec<_>>();
        retrieval.sort_unstable();
        let retained_index_bytes = match strategy.mode() {
            RetrievalMode::Semantic => self.vector_bytes,
            RetrievalMode::FullText => self.full_text_bytes,
            RetrievalMode::Hybrid => self.vector_bytes.saturating_add(self.full_text_bytes),
        };
        StrategyEvaluation {
            strategy,
            pipeline_fingerprint: self.pipeline_fingerprint(corpus, strategy),
            metrics: totals.finish(
                observations,
                self.authorized_sources.len(),
                retained_index_bytes,
                strategy,
            ),
            latency: LatencyObservation {
                p50_micros: percentile(&latencies, 50),
                p95_micros: percentile(&latencies, 95),
                maximum_micros: latencies.last().copied().unwrap_or_default(),
                planning_p50_micros: percentile(&planning, 50),
                retrieval_p50_micros: percentile(&retrieval, 50),
            },
        }
    }

    fn pipeline_fingerprint(
        &self,
        corpus: &RetrievalCorpus,
        strategy: KnowledgeRetrievalStrategy,
    ) -> String {
        let identity = self.embedder.identity();
        let mut hasher = Sha256::new();
        for part in [
            corpus.fingerprint().as_str(),
            KnowledgePlanner::VERSION,
            strategy.as_str(),
            match strategy.mode() {
                RetrievalMode::Hybrid => "hybrid",
                RetrievalMode::FullText => "fullText",
                RetrievalMode::Semantic => "semantic",
            },
            identity.model_id.as_str(),
            identity.model_revision.as_str(),
            identity.tokenizer.as_str(),
            self.indexes.backend_identity.as_str(),
            EMBEDDING_PREPROCESSING_VERSION,
            CHUNKER_VERSION,
            CONVERTER_VERSION,
        ] {
            hash_field(&mut hasher, part);
        }
        hasher.update(DEFAULT_RANK_CONSTANT.to_le_bytes());
        hasher.update((identity.dimensions as u64).to_le_bytes());
        let options = evaluation_options();
        for bound in [
            options.maximum_searches,
            options.candidate_limit,
            options.result_limit,
            options.maximum_results_per_file,
            options.context_token_budget,
            options.adjacent_chunk_radius as usize,
        ] {
            hasher.update((bound as u64).to_le_bytes());
        }
        hasher.update([u8::from(options.section_bounded_context)]);
        hexadecimal(hasher.finalize().as_slice())
    }
}

/// Running totals used to derive one strategy's metrics.
#[derive(Debug, Default)]
struct ScoreTotals {
    scored_cases: usize,
    file_recall_at_5: f64,
    file_recall_at_10: f64,
    chunk_recall_at_5: f64,
    chunk_recall_at_10: f64,
    reciprocal_rank: f64,
    relevant_unique_files: usize,
    expected_unique_files: usize,
    context_driven_irrelevant_hits: usize,
    stale_or_deleted_hits: usize,
    scope_violations: usize,
    negative_controls: usize,
    negative_control_false_positives: usize,
}

impl ScoreTotals {
    fn accumulate(
        &mut self,
        case: &RetrievalCase,
        observation: &CaseObservation,
        authorized: &BTreeSet<String>,
    ) {
        self.scope_violations += observation
            .ranked_source_ids
            .iter()
            .filter(|source_id| !authorized.contains(*source_id))
            .count();
        self.stale_or_deleted_hits += observation
            .returned_chunk_ids
            .iter()
            .filter(|chunk_id| case.forbidden_chunk_ids.contains(*chunk_id))
            .count();
        self.context_driven_irrelevant_hits += observation
            .ranked_source_ids
            .iter()
            .take(SHALLOW_CUTOFF)
            .filter(|source_id| case.context_only_source_ids.contains(*source_id))
            .count();
        if !case.scored() {
            self.negative_controls += 1;
            self.negative_control_false_positives +=
                usize::from(!observation.ranked_source_ids.is_empty());
            return;
        }
        self.scored_cases += 1;
        self.expected_unique_files += case.relevant_source_ids.len();
        self.relevant_unique_files += found(
            &observation.ranked_source_ids,
            &case.relevant_source_ids,
            DEEP_CUTOFF,
        );
        self.file_recall_at_5 += recall(
            &observation.ranked_source_ids,
            &case.relevant_source_ids,
            SHALLOW_CUTOFF,
        );
        self.file_recall_at_10 += recall(
            &observation.ranked_source_ids,
            &case.relevant_source_ids,
            DEEP_CUTOFF,
        );
        self.chunk_recall_at_5 += recall(
            &observation.ranked_chunk_ids,
            &case.relevant_chunk_ids,
            SHALLOW_CUTOFF,
        );
        self.chunk_recall_at_10 += recall(
            &observation.ranked_chunk_ids,
            &case.relevant_chunk_ids,
            DEEP_CUTOFF,
        );
        self.reciprocal_rank += observation
            .ranked_source_ids
            .iter()
            .take(DEEP_CUTOFF)
            .position(|source_id| case.relevant_source_ids.contains(source_id))
            .map_or(0.0, |index| 1.0 / (index + 1) as f64);
    }

    fn finish(
        self,
        observations: &[CaseObservation],
        authorized_sources: usize,
        retained_index_bytes: u64,
        strategy: KnowledgeRetrievalStrategy,
    ) -> StrategyMetrics {
        let scored = self.scored_cases.max(1) as f64;
        let cases = observations.len().max(1) as f64;
        StrategyMetrics {
            evaluated_cases: observations.len(),
            scored_cases: self.scored_cases,
            authorized_sources,
            file_recall_at_5: self.file_recall_at_5 / scored,
            file_recall_at_10: self.file_recall_at_10 / scored,
            chunk_recall_at_5: self.chunk_recall_at_5 / scored,
            chunk_recall_at_10: self.chunk_recall_at_10 / scored,
            mean_reciprocal_rank: self.reciprocal_rank / scored,
            relevant_unique_files: self.relevant_unique_files,
            expected_unique_files: self.expected_unique_files,
            context_driven_irrelevant_hits: self.context_driven_irrelevant_hits,
            stale_or_deleted_hits: self.stale_or_deleted_hits,
            scope_violations: self.scope_violations,
            negative_control_false_positives: self.negative_control_false_positives,
            negative_control_false_positive_rate: if self.negative_controls == 0 {
                0.0
            } else {
                self.negative_control_false_positives as f64 / self.negative_controls as f64
            },
            planned_searches: observations.iter().map(|item| item.planned_searches).sum(),
            index_queries: observations.iter().map(|item| item.index_queries).sum(),
            query_embeddings: observations.iter().map(|item| item.query_embeddings).sum(),
            average_planned_searches: observations
                .iter()
                .map(|item| item.planned_searches as f64)
                .sum::<f64>()
                / cases,
            average_query_embeddings: observations
                .iter()
                .map(|item| item.query_embeddings as f64)
                .sum::<f64>()
                / cases,
            retained_index_bytes,
            storage_impact_bytes: 0,
            migration_impact: strategy.migration_impact().into(),
        }
    }
}

fn found(ranked: &[String], relevant: &BTreeSet<String>, cutoff: usize) -> usize {
    ranked
        .iter()
        .take(cutoff)
        .filter(|id| relevant.contains(*id))
        .collect::<BTreeSet<_>>()
        .len()
}

fn recall(ranked: &[String], relevant: &BTreeSet<String>, cutoff: usize) -> f64 {
    if relevant.is_empty() {
        return 1.0;
    }
    found(ranked, relevant, cutoff) as f64 / relevant.len() as f64
}

fn percentile(sorted: &[u64], percentage: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = percentage
        .saturating_mul(sorted.len())
        .div_ceil(100)
        .saturating_sub(1);
    sorted.get(rank).copied().unwrap_or_default()
}

/// Whole microseconds elapsed since `started`.
fn elapsed_micros(started: Instant) -> u64 {
    started.elapsed().as_micros().try_into().unwrap_or(u64::MAX)
}

/// Bounded retrieval options every strategy shares.
///
/// A result limit of twenty with at most two results per file leaves room for
/// ten distinct sources, which is what makes recall at ten meaningful, and the
/// adjacent radius keeps production's structural context expansion in play.
/// Both the executed request and the pipeline fingerprint read these bounds
/// from here, so a changed bound always changes the fingerprint.
fn evaluation_options() -> KnowledgeSearchOptions {
    KnowledgeSearchOptions {
        candidate_limit: 64,
        result_limit: 20,
        maximum_results_per_file: 2,
        adjacent_chunk_radius: 1,
        ..KnowledgeSearchOptions::default()
    }
}

fn manifest() -> LibraryIndexManifest {
    LibraryIndexManifest {
        zvec_schema_version: 2,
        dimensions: SURROGATE_DIMENSIONS,
        distance_metric: DistanceMetric::Cosine,
        model_revision: SURROGATE_REVISION.into(),
        tokenizer: SURROGATE_TOKENIZER.into(),
        embedding_preprocessing: EMBEDDING_PREPROCESSING_VERSION.into(),
        converter_version: CONVERTER_VERSION.into(),
        chunker_version: CHUNKER_VERSION.into(),
        normalization: VectorNormalization::L2,
    }
}

fn content_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, value);
    hexadecimal(hasher.finalize().as_slice())
}

/// Generations published for one document, oldest first.
fn publication_plan(document: &CorpusDocument) -> Vec<(u64, &Vec<CorpusChunk>)> {
    if document.lifecycle == CorpusLifecycle::Updated && !document.previous_chunks.is_empty() {
        vec![(1, &document.previous_chunks), (2, &document.chunks)]
    } else {
        vec![(1, &document.chunks)]
    }
}

fn stage_and_publish(
    catalog: &SemanticCatalog,
    corpus: &RetrievalCorpus,
    document: &CorpusDocument,
    generation: u64,
    chunks: &[CorpusChunk],
    embedder: &SurrogateEmbedder,
) -> Result<(), KnowledgeEvaluationError> {
    let occurrences = document
        .occurrences
        .iter()
        .map(|occurrence| Occurrence {
            occurrence_id: occurrence.occurrence_id.clone(),
            source_id: occurrence.source_id.clone(),
            root_id: occurrence.root_id.clone(),
            workspace_id: Some(occurrence.workspace_id.clone()),
            media_type: FIXTURE_MEDIA_TYPE.into(),
            modified_at_ms: FIXTURE_MODIFIED_AT_MS,
            available: occurrence.available,
            provenance: "source".into(),
        })
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    for (position, chunk) in chunks.iter().enumerate() {
        for (copy, occurrence) in document.occurrences.iter().enumerate() {
            records.push(staged_record(chunk, occurrence, position, copy, embedder));
        }
    }
    catalog.stage_generation(&StagedGeneration {
        tenant_id: corpus.tenant_id.clone(),
        library_id: corpus.library_id.clone(),
        document_id: document.document_id.clone(),
        content_hash: content_hash(&document.document_id),
        generation,
        occurrences,
        records,
    })?;
    catalog.publish_generation(
        &corpus.tenant_id,
        &corpus.library_id,
        &document.document_id,
        generation,
    )?;
    Ok(())
}

/// Each occurrence of a chunk needs its own derived record identity; the worker
/// collapses them back into one result carrying every authorized copy.
fn record_id_for(chunk_id: &str, copy: usize) -> String {
    if copy == 0 {
        chunk_id.to_owned()
    } else {
        format!("{chunk_id}#copy{copy}")
    }
}

fn staged_record(
    chunk: &CorpusChunk,
    occurrence: &CorpusOccurrence,
    position: usize,
    copy: usize,
    embedder: &SurrogateEmbedder,
) -> StagedRecord {
    let start_line = u32::try_from(position).unwrap_or(0) * 10 + 1;
    StagedRecord {
        record_id: record_id_for(&chunk.chunk_id, copy),
        occurrence_id: occurrence.occurrence_id.clone(),
        cache_key: EmbeddingCacheKey::calculate(
            &chunk.text,
            embedder.identity(),
            &embedder.identity().tokenizer,
            CHUNKER_VERSION,
        ),
        vector: embedder.vector(&chunk.text),
        record_kind: "chunk".into(),
        excerpt: chunk.text.chars().take(160).collect(),
        content: chunk.text.clone(),
        token_count: u32::try_from(chunk.text.split_whitespace().count()).unwrap_or(u32::MAX),
        section_path: chunk.section_path.clone(),
        structural_role: "body".into(),
        provenance: serde_json::to_string(&ChunkProvenance::Exact(Provenance::TextLines {
            start_line,
            end_line: start_line + 9,
        }))
        .unwrap_or_else(|_| "null".into()),
        source_position: u32::try_from(position).unwrap_or(u32::MAX),
        generated: false,
        concept_id: None,
    }
}

/// Surrogate model revision recorded in the manifest and every fingerprint.
const SURROGATE_REVISION: &str = "deterministic-lexical-surrogate/1";
/// Surrogate tokenizer identity recorded in the manifest.
const SURROGATE_TOKENIZER: &str = "lowercase-unicode-word/1";

/// Corpus-derived lexical statistics shared by the surrogate embedder and the
/// surrogate candidate indexes.
///
/// Both a real embedding model and a real lexical index discount common words
/// and reward rare ones. A plain term-frequency surrogate does neither, and it
/// would rank a page that repeats one common word above a page that actually
/// covers the subject. Inverse document frequency is therefore computed once
/// from the corpus and used by both routes, so the surrogate fails for the same
/// reasons a real pipeline would rather than for arithmetic reasons of its own.
pub struct LexicalStatistics {
    canonical_terms: BTreeMap<String, String>,
    document_frequency: BTreeMap<String, usize>,
    documents: usize,
}

impl LexicalStatistics {
    /// Builds statistics from declared cross-language groups and corpus text.
    #[must_use]
    pub fn new(groups: &[Vec<String>], texts: &[String]) -> Self {
        let mut canonical_terms = BTreeMap::new();
        for group in groups {
            let Some(canonical) = group.first() else {
                continue;
            };
            for term in group {
                canonical_terms.insert(term.to_lowercase(), canonical.to_lowercase());
            }
        }
        let mut statistics = Self {
            canonical_terms,
            document_frequency: BTreeMap::new(),
            documents: texts.len(),
        };
        for text in texts {
            for term in statistics.terms(text).into_iter().collect::<BTreeSet<_>>() {
                *statistics.document_frequency.entry(term).or_default() += 1;
            }
        }
        statistics
    }

    /// Canonical tokens of one text, in order, with cross-language folding.
    fn terms(&self, text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|character: char| !character.is_alphanumeric())
            .filter(|token| token.chars().count() > 1)
            .map(|token| {
                self.canonical_terms
                    .get(token)
                    .cloned()
                    .unwrap_or_else(|| token.to_owned())
            })
            .collect()
    }

    /// Smoothed inverse document frequency; unseen terms are treated as rare.
    fn inverse_document_frequency(&self, term: &str) -> f32 {
        let frequency = self.document_frequency.get(term).copied().unwrap_or(0);
        (((self.documents + 1) as f32) / ((frequency + 1) as f32)).ln() + 1.0
    }

    /// Deterministic normalized term-frequency/inverse-document-frequency vector.
    #[must_use]
    pub fn vector(&self, text: &str) -> Vec<f32> {
        let mut counts = BTreeMap::<String, f32>::new();
        for term in self.terms(text) {
            *counts.entry(term).or_default() += 1.0;
        }
        let mut vector = vec![0.0f32; SURROGATE_DIMENSIONS];
        for (term, count) in counts {
            vector[bucket_of(&term)] += (1.0 + count.ln()) * self.inverse_document_frequency(&term);
        }
        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in &mut vector {
                *value /= norm;
            }
        }
        vector
    }
}

/// A deterministic lexical stand-in for a multilingual embedding model.
///
/// It reproduces exactly across machines and runs, and it is not a semantic
/// model: it aligns two languages only where the corpus declares that they
/// should align, and it has no notion of meaning beyond token statistics.
pub struct SurrogateEmbedder {
    identity: EmbeddingModelIdentity,
    statistics: Arc<LexicalStatistics>,
    embeddings: AtomicUsize,
}

impl SurrogateEmbedder {
    /// Builds the surrogate over shared corpus statistics.
    #[must_use]
    pub fn new(statistics: Arc<LexicalStatistics>) -> Self {
        Self {
            identity: EmbeddingModelIdentity {
                model_id: "surrogate-lexical".into(),
                model_revision: SURROGATE_REVISION.into(),
                tokenizer: SURROGATE_TOKENIZER.into(),
                dimensions: SURROGATE_DIMENSIONS,
                max_input_tokens: 8_192,
            },
            statistics,
            embeddings: AtomicUsize::new(0),
        }
    }

    /// Number of inputs embedded so far.
    #[must_use]
    pub fn embeddings(&self) -> usize {
        self.embeddings.load(Ordering::Relaxed)
    }

    /// Deterministic normalized vector for one text.
    #[must_use]
    pub fn vector(&self, text: &str) -> Vec<f32> {
        self.statistics.vector(text)
    }
}

impl EmbeddingProvider for SurrogateEmbedder {
    fn identity(&self) -> &EmbeddingModelIdentity {
        &self.identity
    }

    fn embed(
        &self,
        inputs: &[String],
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if cancellation.is_cancelled() {
            return Err(EmbeddingError::Cancelled);
        }
        self.embeddings.fetch_add(inputs.len(), Ordering::Relaxed);
        Ok(inputs.iter().map(|input| self.vector(input)).collect())
    }
}

fn bucket_of(term: &str) -> usize {
    let digest = Sha256::digest(term.as_bytes());
    let mut value = 0usize;
    for byte in digest.iter().take(8) {
        value = (value << 8) | *byte as usize;
    }
    value % SURROGATE_DIMENSIONS
}

/// Counts every dense candidate query issued by the retrieval service.
struct CountingSemanticIndex {
    inner: Arc<dyn SemanticCandidateIndex>,
    queries: Arc<AtomicUsize>,
}

impl SemanticCandidateIndex for CountingSemanticIndex {
    fn query(
        &self,
        vector: &[f32],
        limit: usize,
        filters: &QueryFilters,
    ) -> Result<Vec<ScoredRecord>, String> {
        self.queries.fetch_add(1, Ordering::Relaxed);
        self.inner.query(vector, limit, filters)
    }
}

/// Counts every lexical candidate query issued by the retrieval service.
struct CountingFullTextIndex {
    inner: Arc<dyn FullTextCandidateIndex>,
    queries: Arc<AtomicUsize>,
}

impl FullTextCandidateIndex for CountingFullTextIndex {
    fn query_full_text(
        &self,
        text: &str,
        limit: usize,
        filters: &QueryFilters,
    ) -> Result<Vec<String>, String> {
        self.queries.fetch_add(1, Ordering::Relaxed);
        self.inner.query_full_text(text, limit, filters)
    }
}

fn matches(record: &EvaluationIndexRecord, filters: &QueryFilters) -> bool {
    filters.tenant_id == record.tenant_id
        && filters
            .library_id
            .as_ref()
            .is_none_or(|value| *value == record.library_id)
        && filters
            .root_id
            .as_ref()
            .is_none_or(|value| *value == record.root_id)
        && filters
            .workspace_id
            .as_ref()
            .is_none_or(|value| *value == record.workspace_id)
}

/// Deterministic in-process stand-in for the native dense and lexical indexes.
///
/// It deliberately keeps superseded and deleted records, and records outside the
/// authorized roots, so that the catalog — not the index — is what removes them.
pub struct SurrogateIndex {
    statistics: Arc<LexicalStatistics>,
    records: Vec<EvaluationIndexRecord>,
}

impl SurrogateIndex {
    /// Builds an index over shared corpus statistics and published records.
    #[must_use]
    pub fn new(statistics: Arc<LexicalStatistics>, records: &[EvaluationIndexRecord]) -> Self {
        Self {
            statistics,
            records: records.to_vec(),
        }
    }

    fn ranked<Score>(&self, limit: usize, filters: &QueryFilters, score: Score) -> Vec<String>
    where
        Score: Fn(&EvaluationIndexRecord) -> Option<f32>,
    {
        let mut scored = self
            .records
            .iter()
            .filter(|record| matches(record, filters))
            .filter_map(|record| score(record).map(|value| (record.record_id.clone(), value)))
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.0.cmp(&right.0))
        });
        scored.dedup_by(|left, right| left.0 == right.0);
        scored.truncate(limit);
        scored.into_iter().map(|(record_id, _)| record_id).collect()
    }
}

impl SemanticCandidateIndex for SurrogateIndex {
    fn query(
        &self,
        vector: &[f32],
        limit: usize,
        filters: &QueryFilters,
    ) -> Result<Vec<ScoredRecord>, String> {
        let similarity = |record: &EvaluationIndexRecord| {
            record
                .vector
                .iter()
                .zip(vector)
                .map(|(left, right)| left * right)
                .sum::<f32>()
        };
        let scores = self
            .records
            .iter()
            .map(|record| (record.record_id.as_str(), similarity(record)))
            .collect::<BTreeMap<_, _>>();
        Ok(self
            .ranked(limit, filters, |record| Some(similarity(record)))
            .into_iter()
            .map(|record_id| ScoredRecord {
                score: scores.get(record_id.as_str()).copied().unwrap_or_default(),
                record_id,
            })
            .collect())
    }
}

impl FullTextCandidateIndex for SurrogateIndex {
    fn query_full_text(
        &self,
        text: &str,
        limit: usize,
        filters: &QueryFilters,
    ) -> Result<Vec<String>, String> {
        let terms = self
            .statistics
            .terms(text)
            .into_iter()
            .collect::<BTreeSet<_>>();
        Ok(self.ranked(limit, filters, |record| {
            let mut counts = BTreeMap::<String, f32>::new();
            for term in self.statistics.terms(&record.content) {
                if terms.contains(&term) {
                    *counts.entry(term).or_default() += 1.0;
                }
            }
            if counts.is_empty() {
                return None;
            }
            Some(
                counts
                    .iter()
                    .map(|(term, count)| {
                        (1.0 + count.ln()) * self.statistics.inverse_document_frequency(term)
                    })
                    .sum(),
            )
        }))
    }
}

/// Failure while loading, running, or validating a knowledge evaluation.
#[derive(Debug, thiserror::Error)]
pub enum KnowledgeEvaluationError {
    /// The corpus does not cover a required release-gate behavior.
    #[error("knowledge evaluation corpus is incomplete: {0}")]
    IncompleteCorpus(String),
    /// A checked-in report contradicts its own recorded evidence.
    #[error("knowledge evaluation report is invalid: {0}")]
    InvalidReport(String),
    /// The caller-supplied candidate indexes declared no backend identity.
    #[error("knowledge evaluation candidate indexes need a stable backend identity")]
    MissingIndexBackendIdentity,
    /// The corpus or report uses an unsupported schema version.
    #[error("knowledge evaluation schema version `{0}` is unsupported")]
    UnsupportedSchema(u32),
    /// A canonical knowledge request could not be planned.
    #[error("knowledge evaluation request is invalid: {0}")]
    Request(#[from] crate::knowledge::KnowledgeRequestError),
    /// Retrieval failed while executing a case.
    #[error("knowledge evaluation retrieval failed: {0}")]
    Retrieval(#[from] KnowledgeSearchError),
    /// The evaluation catalog could not be built.
    #[error("knowledge evaluation catalog failed: {0}")]
    Storage(#[from] fm_semantic_worker::semantic_storage::StorageError),
    /// Corpus or report JSON could not be parsed or encoded.
    #[error("knowledge evaluation JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    /// The temporary evaluation catalog could not be created.
    #[error("knowledge evaluation filesystem failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_search::UnavailableKnowledgeRetrievalCapability;

    const CORPUS: &str = include_str!("../tests/fixtures/knowledge-retrieval-corpus-v1.json");
    const REPORT: &str = include_str!("../../../docs/evaluations/knowledge-retrieval-v1.json");
    /// Backend identity recorded by the native Zvec run.
    #[cfg(feature = "zvec")]
    const NATIVE_ZVEC_INDEX_IDENTITY: &str = "native-zvec-flat/1";

    fn corpus() -> RetrievalCorpus {
        RetrievalCorpus::parse(CORPUS).expect("repository corpus")
    }

    fn report() -> EvaluationReport {
        EvaluationReport::parse(REPORT).expect("checked-in report")
    }

    #[cfg(feature = "zvec")]
    fn strategy_fingerprint(
        report: &EvaluationReport,
        strategy: KnowledgeRetrievalStrategy,
    ) -> String {
        report
            .strategies
            .iter()
            .find(|item| item.strategy == strategy)
            .expect("recorded strategy")
            .pipeline_fingerprint
            .clone()
    }

    /// Builds the deterministic in-process indexes under a chosen identity.
    fn surrogate_indexes(
        identity: &str,
    ) -> impl FnOnce(Arc<LexicalStatistics>, &[EvaluationIndexRecord]) -> EvaluationIndexes + '_
    {
        move |statistics, records| {
            let index = Arc::new(SurrogateIndex::new(statistics, records));
            EvaluationIndexes {
                backend_identity: identity.to_owned(),
                semantic: index.clone(),
                full_text: index,
            }
        }
    }

    /// Runs one corpus case through the same harness the report is built from.
    async fn observe(
        corpus: &RetrievalCorpus,
        case: &RetrievalCase,
        strategy: KnowledgeRetrievalStrategy,
    ) -> (BTreeSet<String>, CaseObservation) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let harness = CorpusHarness::build(
            corpus,
            &directory.path().join("catalog.sqlite"),
            surrogate_indexes(SURROGATE_INDEX_IDENTITY),
        )
        .expect("harness");
        let coordinator = evaluation_coordinator(Arc::new(KnowledgeRetrievalService::new(
            harness.catalog.clone(),
            Some(harness.embedder.clone()),
            Some(harness.indexes.semantic.clone()),
            Some(harness.indexes.full_text.clone()),
        )));
        let observation = harness
            .run(&coordinator, case, strategy)
            .await
            .expect("observation");
        (harness.authorized_sources.clone(), observation)
    }

    fn case_named<'a>(corpus: &'a RetrievalCorpus, category: &str) -> &'a RetrievalCase {
        corpus
            .cases
            .iter()
            .find(|case| case.category == category)
            .unwrap_or_else(|| panic!("corpus needs a `{category}` case"))
    }

    #[test]
    fn the_corpus_fingerprint_covers_documents_and_judgments() {
        let corpus = corpus();
        let mut edited = corpus.clone();
        edited.documents[0].chunks[0].text.push_str(" appended");
        let mut rejudged = corpus.clone();
        rejudged.cases[0]
            .relevant_source_ids
            .insert("source-glossary".into());

        assert_ne!(corpus.fingerprint(), edited.fingerprint());
        assert_ne!(corpus.fingerprint(), rejudged.fingerprint());
        assert_eq!(corpus.fingerprint(), corpus.clone().fingerprint());
    }

    /// A two-generation, one-judgment corpus used to prove that the fingerprint
    /// separates groups rather than hashing one flat stream of values.
    fn separable_corpus() -> RetrievalCorpus {
        let chunk = |chunk_id: &str| CorpusChunk {
            chunk_id: chunk_id.into(),
            section_path: vec!["Section".into()],
            text: format!("body of {chunk_id}"),
        };
        RetrievalCorpus {
            schema_version: CORPUS_SCHEMA_VERSION,
            corpus_id: "corpus".into(),
            tenant_id: "tenant".into(),
            library_id: "library".into(),
            authorized_root_ids: vec!["root".into()],
            cross_language_terms: Vec::new(),
            documents: vec![CorpusDocument {
                document_id: "document".into(),
                language: "en".into(),
                role: "procedure".into(),
                lifecycle: CorpusLifecycle::Updated,
                occurrences: vec![CorpusOccurrence {
                    occurrence_id: "occurrence".into(),
                    source_id: "source".into(),
                    root_id: "root".into(),
                    workspace_id: "workspace".into(),
                    available: true,
                }],
                previous_chunks: vec![chunk("chunk-1")],
                chunks: vec![chunk("chunk-2")],
            }],
            cases: vec![RetrievalCase {
                id: "case".into(),
                category: "update".into(),
                language: "en".into(),
                question: "question".into(),
                subject: "subject".into(),
                needs: Vec::new(),
                related_terms: vec!["shared".into()],
                relevant_source_ids: BTreeSet::new(),
                relevant_chunk_ids: BTreeSet::new(),
                context_only_source_ids: BTreeSet::new(),
                forbidden_chunk_ids: BTreeSet::new(),
                expected_no_evidence: false,
            }],
        }
    }

    /// Publishing a chunk as current instead of superseded is the difference
    /// between evidence a case must return and evidence it must never return,
    /// so the two generations may not collapse into one hashed sequence.
    #[test]
    fn a_chunk_moved_between_generations_changes_the_corpus_fingerprint() {
        let corpus = separable_corpus();
        let mut moved = corpus.clone();
        let superseded = moved.documents[0].previous_chunks.remove(0);
        moved.documents[0].chunks.insert(0, superseded);

        let flattened = |corpus: &RetrievalCorpus| {
            corpus.documents[0]
                .previous_chunks
                .iter()
                .chain(&corpus.documents[0].chunks)
                .map(|chunk| chunk.chunk_id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            flattened(&corpus),
            flattened(&moved),
            "the two corpora must differ only in which generation owns a chunk"
        );
        assert_ne!(corpus.fingerprint(), moved.fingerprint());
    }

    /// The same term means the opposite thing in different judgment groups: a
    /// forbidden chunk and a relevant chunk cannot hash to the same corpus.
    #[test]
    fn one_judgment_value_hashes_differently_in_every_case_group() {
        let corpus = separable_corpus();
        let mut fingerprints = BTreeSet::new();
        for group in 0..5 {
            let mut variant = corpus.clone();
            let case = &mut variant.cases[0];
            case.related_terms.clear();
            match group {
                0 => case.related_terms.push("shared".into()),
                1 => {
                    case.relevant_source_ids.insert("shared".into());
                }
                2 => {
                    case.relevant_chunk_ids.insert("shared".into());
                }
                3 => {
                    case.context_only_source_ids.insert("shared".into());
                }
                _ => {
                    case.forbidden_chunk_ids.insert("shared".into());
                }
            }
            fingerprints.insert(variant.fingerprint());
        }

        assert_eq!(
            fingerprints.len(),
            5,
            "every judgment group must contribute its own digest"
        );
    }

    #[test]
    fn regrouping_cross_language_terms_changes_the_corpus_fingerprint() {
        let mut merged = separable_corpus();
        merged.cross_language_terms = vec![vec!["nacelle".into(), "gondel".into()]];
        let mut split = merged.clone();
        split.cross_language_terms = vec![vec!["nacelle".into()], vec!["gondel".into()]];

        assert_ne!(merged.fingerprint(), split.fingerprint());
    }

    #[test]
    fn a_corpus_without_an_unauthorized_root_cannot_prove_scope_isolation() {
        let mut corpus = corpus();
        let roots = corpus
            .documents
            .iter()
            .flat_map(|document| &document.occurrences)
            .map(|occurrence| occurrence.root_id.clone())
            .collect::<BTreeSet<_>>();
        corpus.authorized_root_ids = roots.into_iter().collect();

        assert!(matches!(
            corpus.validate(),
            Err(KnowledgeEvaluationError::IncompleteCorpus(_))
        ));
    }

    #[test]
    fn a_case_naming_an_unknown_source_is_rejected() {
        let mut corpus = corpus();
        corpus.cases[0]
            .relevant_source_ids
            .insert("source-does-not-exist".into());

        assert!(matches!(
            corpus.validate(),
            Err(KnowledgeEvaluationError::IncompleteCorpus(message))
                if message.contains("source-does-not-exist")
        ));
    }

    #[test]
    fn recall_counts_distinct_relevant_sources_within_the_cutoff() {
        let relevant = ["a", "b"].into_iter().map(ToOwned::to_owned).collect();
        let ranked = ["noise", "a", "a", "b"]
            .into_iter()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();

        assert!((recall(&ranked, &relevant, 2) - 0.5).abs() < f64::EPSILON);
        assert!((recall(&ranked, &relevant, 4) - 1.0).abs() < f64::EPSILON);
        assert!((recall(&ranked, &BTreeSet::new(), 4) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_regression_fails_when_a_context_only_source_outranks_a_real_match() {
        let case = RetrievalCase {
            id: "case".into(),
            category: "matchSorting".into(),
            language: "en".into(),
            question: "question".into(),
            subject: "subject".into(),
            needs: Vec::new(),
            related_terms: Vec::new(),
            relevant_source_ids: ["procedure", "example"]
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
            relevant_chunk_ids: BTreeSet::new(),
            context_only_source_ids: ["context"].into_iter().map(ToOwned::to_owned).collect(),
            forbidden_chunk_ids: BTreeSet::new(),
            expected_no_evidence: false,
        };
        let passing = MatchSortingObservation {
            strategy: KnowledgeRetrievalStrategy::StructuredHybrid,
            expected_ranks: vec![1, 2],
            context_only_rank: Some(3),
        };

        assert!(sorting_regression_passed(
            std::slice::from_ref(&passing),
            &case
        ));
        assert!(!sorting_regression_passed(
            &[MatchSortingObservation {
                context_only_rank: Some(2),
                expected_ranks: vec![1, 3],
                ..passing.clone()
            }],
            &case
        ));
        assert!(!sorting_regression_passed(
            &[MatchSortingObservation {
                expected_ranks: vec![1],
                ..passing
            }],
            &case
        ));
    }

    #[test]
    fn a_report_cannot_claim_a_go_its_own_metrics_do_not_support() {
        let mut forged = report();
        forged.decision = EvaluationDecision::Go;
        assert!(matches!(
            forged.validate(),
            Err(KnowledgeEvaluationError::InvalidReport(_))
        ));

        let mut silenced = report();
        silenced.blocking_reasons.clear();
        assert!(matches!(
            silenced.validate(),
            Err(KnowledgeEvaluationError::InvalidReport(_))
        ));

        let mut undeclared = report();
        undeclared.limitations.clear();
        assert!(matches!(
            undeclared.validate(),
            Err(KnowledgeEvaluationError::InvalidReport(_))
        ));

        let mut false_regression = report();
        false_regression
            .match_sorting_regression
            .observations
            .iter_mut()
            .find(|item| item.strategy == KnowledgeRetrievalStrategy::StructuredHybrid)
            .expect("structured hybrid observation")
            .context_only_rank = Some(1);
        assert!(matches!(
            false_regression.validate(),
            Err(KnowledgeEvaluationError::InvalidReport(_))
        ));

        let mut stale = report();
        stale.release_candidate_fingerprint = "stale-candidate".into();
        assert!(matches!(
            stale.validate(),
            Err(KnowledgeEvaluationError::InvalidReport(_))
        ));
    }

    #[test]
    fn a_report_whose_strategies_saw_different_content_is_rejected() {
        let mut skewed = report();
        skewed.strategies[1].metrics.authorized_sources += 1;

        assert!(matches!(
            skewed.validate(),
            Err(KnowledgeEvaluationError::InvalidReport(_))
        ));
    }

    #[test]
    fn a_production_measurement_still_needs_every_quality_gate() {
        let comparison = KnowledgeRetrievalComparison {
            corpus_id: report().corpus_id,
            corpus_fingerprint: report().corpus_fingerprint,
            planner_version: report().planner_version,
            fusion_version: report().fusion_version,
            release_candidate_fingerprint: report().release_candidate_fingerprint,
            strategies: report().strategies,
            match_sorting_regression: report().match_sorting_regression,
        };

        let promoted = EvaluationReport::from_comparison(
            &comparison,
            "hypothetical production run",
            Vec::new(),
            true,
            u64::MAX,
        );

        promoted.validate().expect("self-consistent report");
        assert_eq!(promoted.decision, EvaluationDecision::NoGo);
        assert!(
            promoted
                .blocking_reasons
                .iter()
                .all(|reason| !reason.contains("no production")),
            "only measured quality gates may remain: {:?}",
            promoted.blocking_reasons
        );
    }

    #[test]
    fn a_production_measurement_that_passes_every_quality_gate_can_record_a_go() {
        let existing = report();
        let mut strategies = existing.strategies;
        let control = strategies
            .iter_mut()
            .find(|item| item.strategy == KnowledgeRetrievalStrategy::WholeQuestionVector)
            .expect("control");
        control.metrics.file_recall_at_5 = 0.70;
        control.metrics.file_recall_at_10 = 0.80;
        control.metrics.mean_reciprocal_rank = 0.50;
        let comparison = KnowledgeRetrievalComparison {
            corpus_id: existing.corpus_id,
            corpus_fingerprint: existing.corpus_fingerprint,
            planner_version: existing.planner_version,
            fusion_version: existing.fusion_version,
            release_candidate_fingerprint: existing.release_candidate_fingerprint,
            strategies,
            match_sorting_regression: existing.match_sorting_regression,
        };

        let qualified = EvaluationReport::from_comparison(
            &comparison,
            "hypothetical production run",
            Vec::new(),
            true,
            u64::MAX,
        );

        qualified.validate().expect("self-consistent report");
        assert_eq!(qualified.decision, EvaluationDecision::Go);
        assert!(qualified.blocking_reasons.is_empty());
    }

    #[test]
    fn the_checked_in_report_records_the_measured_no_go() {
        let report = report();

        report.validate().expect("valid checked-in report");
        assert_eq!(report.decision, EvaluationDecision::NoGo);
        assert!(!report.production_measurement);
        assert!(report.match_sorting_regression.passed);
        assert_eq!(report.strategies.len(), 5);
    }

    /// The evaluation produces the release decision, so it must not require
    /// one. A release-profile test build resolves the compiled qualification to
    /// `Unqualified`, which would make the coordinator refuse every case before
    /// it reached the worker and leave the report unreproducible.
    #[tokio::test]
    async fn the_harness_coordinator_never_inherits_the_build_qualification() {
        let coordinator = evaluation_coordinator(Arc::new(UnavailableKnowledgeRetrievalCapability));

        assert_eq!(
            KnowledgeReleaseAccess::resolve(false, None),
            KnowledgeReleaseAccess::Unqualified,
            "a release build without a measured go decision fails closed"
        );
        assert_eq!(coordinator.release_access(), EVALUATION_RELEASE_ACCESS);
        assert!(
            coordinator.release_access().is_available(),
            "the harness must stay usable in a release-profile test build"
        );
    }

    /// Executing one real case proves the whole path, and fails with
    /// `Unavailable` under `cargo test --release` if the harness ever goes back
    /// to resolving release access from the build.
    #[tokio::test]
    async fn the_harness_executes_a_case_in_any_build_profile() {
        let corpus = corpus();
        let (_, observation) = observe(
            &corpus,
            case_named(&corpus, "procedure"),
            KnowledgeRetrievalStrategy::StructuredHybrid,
        )
        .await;

        assert!(!observation.ranked_source_ids.is_empty());
    }

    /// Retrieval quality depends on which index answered, so an in-process run
    /// and a native run over one corpus must never share a fingerprint.
    #[test]
    fn the_candidate_index_backend_identity_is_part_of_every_pipeline_fingerprint() {
        let corpus = corpus();
        let fingerprints = |identity: &str| {
            let directory = tempfile::tempdir().expect("temporary directory");
            let harness = CorpusHarness::build(
                &corpus,
                &directory.path().join("catalog.sqlite"),
                surrogate_indexes(identity),
            )
            .expect("harness");
            KnowledgeRetrievalStrategy::ALL
                .into_iter()
                .map(|strategy| harness.pipeline_fingerprint(&corpus, strategy))
                .collect::<BTreeSet<_>>()
        };

        let surrogate = fingerprints(SURROGATE_INDEX_IDENTITY);
        let native = fingerprints("native-zvec-flat/1");

        assert_eq!(surrogate.len(), KnowledgeRetrievalStrategy::ALL.len());
        assert_eq!(native.len(), KnowledgeRetrievalStrategy::ALL.len());
        assert!(
            surrogate.is_disjoint(&native),
            "a different candidate index backend must produce different fingerprints"
        );
        assert_eq!(surrogate, fingerprints(SURROGATE_INDEX_IDENTITY));
    }

    #[tokio::test]
    async fn candidate_indexes_without_a_backend_identity_are_rejected() {
        let error = evaluate_knowledge_retrieval_with(&corpus(), surrogate_indexes("   "))
            .await
            .expect_err("an unidentified backend cannot be fingerprinted");

        assert!(matches!(
            error,
            KnowledgeEvaluationError::MissingIndexBackendIdentity
        ));
    }

    /// The deletion regression must test catalog deletion, not the evaluation's
    /// own allow-list: the deleted source stays authorized, and retrieval still
    /// never returns it because the catalog no longer resolves its records.
    #[tokio::test]
    async fn a_deleted_source_stays_authorized_and_is_removed_by_catalog_deletion() {
        let corpus = corpus();
        let deleted_sources = corpus.deleted_sources();
        let deleted_chunks = corpus
            .documents
            .iter()
            .filter(|document| document.lifecycle == CorpusLifecycle::Deleted)
            .flat_map(|document| &document.chunks)
            .map(|chunk| chunk.chunk_id.clone())
            .collect::<BTreeSet<_>>();

        assert!(!deleted_sources.is_empty(), "a deletion case is required");
        assert!(
            deleted_sources.is_subset(&corpus.authorized_sources()),
            "a deleted source must still be authorized, or deletion proves nothing"
        );

        for strategy in KnowledgeRetrievalStrategy::ALL {
            let (authorized, observation) =
                observe(&corpus, case_named(&corpus, "deletion"), strategy).await;

            assert!(deleted_sources.is_subset(&authorized));
            for source_id in &deleted_sources {
                assert!(
                    !observation.ranked_source_ids.contains(source_id),
                    "{strategy:?} returned deleted source `{source_id}`"
                );
            }
            for chunk_id in &deleted_chunks {
                assert!(
                    !observation.returned_chunk_ids.contains(chunk_id),
                    "{strategy:?} returned deleted chunk `{chunk_id}`"
                );
            }
        }
    }

    /// Reported latency is planning plus retrieval, so dropping the planning
    /// phase from the total has to fail here.
    #[tokio::test]
    async fn reported_latency_covers_planning_and_retrieval() {
        let corpus = corpus();
        let mut planning_total = 0u64;
        for strategy in KnowledgeRetrievalStrategy::ALL {
            for category in ["procedure", "multilingual", "matchSorting"] {
                let (_, observation) =
                    observe(&corpus, case_named(&corpus, category), strategy).await;

                assert_eq!(
                    observation.latency_micros,
                    observation
                        .planning_micros
                        .saturating_add(observation.retrieval_micros),
                    "{strategy:?}/{category} must report both phases"
                );
                planning_total = planning_total.saturating_add(observation.planning_micros);
            }
        }

        assert!(
            planning_total > 0,
            "planning must contribute measurable time to the reported latency"
        );
    }

    #[tokio::test]
    async fn the_surrogate_run_reproduces_the_checked_in_deterministic_metrics() {
        let comparison = evaluate_knowledge_retrieval(&corpus())
            .await
            .expect("comparison");
        let report = report();

        assert_eq!(report.corpus_fingerprint, comparison.corpus_fingerprint);
        for expected in &comparison.strategies {
            let recorded = report
                .strategies
                .iter()
                .find(|item| item.strategy == expected.strategy)
                .expect("recorded strategy");
            assert_eq!(recorded.pipeline_fingerprint, expected.pipeline_fingerprint);
            assert_eq!(recorded.metrics, expected.metrics);
        }
        assert_eq!(
            report.match_sorting_regression,
            comparison.match_sorting_regression
        );
    }

    /// Runs the same corpus over the real native Zvec dense and full-text
    /// indexes, so the structured routes are exercised against the shipped
    /// index rather than the in-process stand-in.
    #[cfg(feature = "zvec")]
    #[tokio::test]
    async fn the_corpus_runs_over_the_real_native_zvec_indexes() {
        use fm_semantic_worker::semantic_storage::VectorIndexKind;
        use fm_semantic_worker::zvec_storage::{ZvecRecord, ZvecStorage};

        let directory = tempfile::tempdir().expect("temporary directory");
        let comparison = evaluate_knowledge_retrieval_with(&corpus(), |_, records| {
            let index = Arc::new(
                ZvecStorage::create(
                    &directory.path().join("zvec"),
                    SURROGATE_DIMENSIONS,
                    VectorIndexKind::Flat,
                )
                .expect("create collection"),
            );
            index
                .insert(
                    &records
                        .iter()
                        .map(|record| ZvecRecord {
                            record_id: record.record_id.clone(),
                            tenant_id: record.tenant_id.clone(),
                            library_id: record.library_id.clone(),
                            root_id: record.root_id.clone(),
                            workspace_id: Some(record.workspace_id.clone()),
                            media_type: record.media_type.clone(),
                            modified_at_ms: record.modified_at_ms,
                            concept_id: None,
                            generation: record.generation,
                            content: record.content.clone(),
                            embedding: record.vector.clone(),
                        })
                        .collect::<Vec<_>>(),
                )
                .expect("insert derived records");
            index.flush().expect("flush");
            EvaluationIndexes {
                backend_identity: NATIVE_ZVEC_INDEX_IDENTITY.into(),
                semantic: index.clone(),
                full_text: index,
            }
        })
        .await
        .expect("native comparison");

        let hybrid = comparison
            .strategy(KnowledgeRetrievalStrategy::StructuredHybrid)
            .expect("hybrid strategy");
        let control = comparison
            .strategy(KnowledgeRetrievalStrategy::WholeQuestionVector)
            .expect("control strategy");
        assert_eq!(hybrid.metrics.scope_violations, 0);
        assert_eq!(hybrid.metrics.stale_or_deleted_hits, 0);
        assert!(hybrid.metrics.mean_reciprocal_rank >= control.metrics.mean_reciprocal_rank);
        assert!(
            hybrid.metrics.context_driven_irrelevant_hits
                <= control.metrics.context_driven_irrelevant_hits
        );
        assert!(comparison.match_sorting_regression.passed);
        for evaluation in &comparison.strategies {
            assert_ne!(
                evaluation.pipeline_fingerprint,
                strategy_fingerprint(&report(), evaluation.strategy),
                "a native Zvec run must not share the in-process run's fingerprint"
            );
        }
    }
}
