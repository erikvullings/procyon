//! Bounded hybrid knowledge retrieval and source-oriented evidence.
//!
//! Native full-text and dense-vector retrieval are peers. Their independent
//! rankings are fused with deterministic reciprocal-rank fusion; raw BM25 and
//! cosine scores are never added, normalized, or otherwise combined. Ranking
//! precedes per-file diversification, the complete-chunk token budget, and
//! bounded adjacent context expansion, so context can never change a rank.
//!
//! This capability is a search result model. It never contacts an LLM and is
//! independent of answer generation.
//!
//! Fusion is implemented here rather than through Zvec's physical `MultiQuery`
//! batching, so logical planning, ranking, and evidence remain independently
//! testable and identical for every backend and route combination.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use fm_semantic_conversion::ChunkProvenance;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::embedding::EmbeddingError;
use crate::ingestion::EmbeddingProvider;
use crate::semantic_search::{SearchCoverage, SemanticCandidateIndex};
use crate::semantic_storage::{
    CatalogReader, QueryEvidence, QueryFilters, SemanticCatalog, StorageError,
};

/// Rank constant used by Procyon's deterministic reciprocal-rank fusion.
pub const DEFAULT_RANK_CONSTANT: u32 = 60;
/// Maximum planned source queries accepted by one retrieval.
pub const MAX_SOURCE_QUERIES: usize = 8;
const MAX_QUERY_BYTES: usize = 8 * 1024;
const MAX_CANDIDATE_LIMIT: usize = 512;
const MAX_RESULT_LIMIT: usize = 200;
const MAX_RESULTS_PER_FILE: usize = 32;
const MAX_CONTEXT_TOKENS: usize = 32_768;
const MAX_ADJACENT_RADIUS: u32 = 4;
const AUTHORIZATION_BATCH_SIZE: usize = 1_024;
/// Maximum exactly-scoped slices accepted by one retrieval.
pub const MAX_RETRIEVAL_SCOPES: usize = 8;
/// Largest candidate page requested while filling a restricted scope's budget.
///
/// Matches the derived index's own top-k ceiling, so an overfetch can never be
/// rejected by the index it is issued against.
const MAX_RESTRICTED_CANDIDATE_PAGE: usize = 1_000;
/// How many times the candidate limit may be overfetched to fill that budget.
const MAX_RESTRICTED_OVERFETCH_FACTOR: usize = 8;
/// Maximum exact authorized source identities accepted by one retrieval.
pub const MAX_ALLOWED_SOURCES: usize = 4_096;
/// Maximum duplicate source identities retained for one evidence row.
pub const MAX_DUPLICATE_SOURCES: usize = 32;

/// Read-only native full-text boundary; the peer of [`SemanticCandidateIndex`].
///
/// Only rank order crosses this boundary. Lexical relevance scores stay inside
/// the index so that no caller can arithmetically combine them with cosine
/// similarity.
pub trait FullTextCandidateIndex: Send + Sync {
    /// Retrieves bounded lexical candidates in descending relevance order.
    ///
    /// # Errors
    ///
    /// Returns a sanitized index failure.
    fn query_full_text(
        &self,
        text: &str,
        limit: usize,
        filters: &QueryFilters,
    ) -> Result<Vec<String>, String>;
}

/// Physical retrieval route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeRoute {
    /// Native full-text and dense vectors fused by rank.
    Hybrid,
    /// Native full-text only; requires no query embedding.
    FullText,
    /// Dense vectors only.
    Semantic,
}

/// Why one source query was issued, preserved for every result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeRetrievalReason {
    /// The raw subject exactly as the user entered it.
    Subject,
    /// A broad orientation expansion.
    Overview,
    /// A definition expansion.
    Definition,
    /// A procedure or how-to expansion.
    Procedure,
    /// An examples expansion.
    Examples,
    /// A supporting-evidence expansion.
    Evidence,
    /// An arguments expansion.
    Arguments,
    /// A comparison expansion.
    Comparison,
    /// A limitations expansion.
    Limitations,
    /// A references expansion.
    References,
    /// An explicit user-supplied related term.
    Related,
}

/// One planned source query and the reason it exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeQuery {
    /// Query text; never rewritten by this capability.
    pub text: String,
    /// Retrieval reason preserved through fusion into the trace.
    pub reason: KnowledgeRetrievalReason,
}

/// Independently reported retrieval capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeCapabilities {
    /// A native full-text index is present.
    pub full_text: bool,
    /// A compatible embedding runtime and vector index are present.
    pub query_embeddings: bool,
}

/// Sanitized reason an explicitly requested route was not used verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteFallbackReason {
    /// No embedding runtime or vector index is configured.
    QueryEmbeddingsUnavailable,
    /// Query embedding failed before retrieval.
    QueryEmbeddingFailed,
    /// The dense route failed during retrieval.
    SemanticQueryFailed,
    /// No native full-text index is available.
    FullTextIndexUnavailable,
    /// The lexical route failed during retrieval.
    FullTextQueryFailed,
}

/// Requested and actually applied routes with an explicit fallback reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteOutcome {
    /// Route the caller asked for.
    pub requested: KnowledgeRoute,
    /// Route actually executed.
    pub applied: KnowledgeRoute,
    /// Present whenever `applied` differs from `requested`.
    pub fallback_reason: Option<RouteFallbackReason>,
}

/// Bounded resource, diversity, and context limits for one retrieval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnowledgeRetrievalPolicy {
    /// Candidates requested per route per source query.
    pub candidate_limit: usize,
    /// Maximum ranked primary results.
    pub result_limit: usize,
    /// Maximum ranked primary results from one file.
    pub maximum_results_per_file: usize,
    /// Maximum complete-chunk tokens across primary and adjacent evidence.
    pub context_token_budget: usize,
    /// Structural chunks expanded on each side of a primary result.
    pub adjacent_chunk_radius: u32,
    /// Restricts adjacent context to the primary result's structural section.
    pub section_bounded_context: bool,
    /// Reciprocal-rank-fusion rank constant.
    pub rank_constant: u32,
    /// Whether a bounded privacy-safe trace is produced.
    pub include_trace: bool,
}

impl KnowledgeRetrievalPolicy {
    /// Returns conservative defaults for interactive knowledge search.
    #[must_use]
    pub const fn default_search() -> Self {
        Self {
            candidate_limit: 64,
            result_limit: 20,
            maximum_results_per_file: 3,
            context_token_budget: 8_192,
            adjacent_chunk_radius: 1,
            section_bounded_context: true,
            rank_constant: DEFAULT_RANK_CONSTANT,
            include_trace: false,
        }
    }

    const fn valid(self) -> bool {
        self.candidate_limit >= 1
            && self.candidate_limit <= MAX_CANDIDATE_LIMIT
            && self.result_limit >= 1
            && self.result_limit <= MAX_RESULT_LIMIT
            && self.maximum_results_per_file >= 1
            && self.maximum_results_per_file <= MAX_RESULTS_PER_FILE
            && self.context_token_budget >= 1
            && self.context_token_budget <= MAX_CONTEXT_TOKENS
            && self.adjacent_chunk_radius <= MAX_ADJACENT_RADIUS
            && self.rank_constant != 0
    }
}

/// Exact host-authorized source identities for one retrieval.
///
/// The host owns consent and enumerates the occurrences a caller may currently
/// see. Applying that set inside the worker — before the result, per-file, and
/// token budgets are spent — keeps a high-ranked out-of-scope candidate from
/// crowding out an authorized one. An empty set means the request filters
/// already describe the authorized scope exactly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnowledgeSourceRestriction {
    /// Opaque host source identities this retrieval may return.
    pub allowed_source_ids: BTreeSet<String>,
}

impl KnowledgeSourceRestriction {
    /// Whether one opaque source identity may be returned.
    #[must_use]
    pub fn permits(&self, source_id: &str) -> bool {
        self.allowed_source_ids.is_empty() || self.allowed_source_ids.contains(source_id)
    }

    fn bounded(&self) -> bool {
        self.allowed_source_ids.len() <= MAX_ALLOWED_SOURCES
    }
}

/// One bounded knowledge retrieval request.
///
/// A scope that the index filters cannot express exactly is carried as several
/// exactly-scoped slices rather than as one broadened filter. Every slice is
/// retrieved as candidates, all slices are fused into a single global ranking,
/// and the result, per-file, token, and adjacency budgets are then spent once
/// over that ranking — never per slice.
#[derive(Debug, Clone)]
pub struct KnowledgeRetrievalRequest {
    /// Planned source queries; the logical plan is owned by the caller.
    pub queries: Vec<KnowledgeQuery>,
    /// Requested physical route.
    pub route: KnowledgeRoute,
    /// Exactly-scoped slices retrieved and ranked as one logical scope.
    pub scopes: Vec<KnowledgeRetrievalScope>,
    /// Current host source hashes keyed by opaque source identity.
    pub current_hashes: HashMap<String, String>,
    /// Requested-scope coverage supplied by the authoritative host catalog.
    pub coverage: SearchCoverage,
    /// Bounded retrieval limits applied once across every scope.
    pub policy: KnowledgeRetrievalPolicy,
}

/// One exactly-scoped slice of a single authorized retrieval scope.
#[derive(Debug, Clone, Default)]
pub struct KnowledgeRetrievalScope {
    /// Authoritative tenant/library/root/workspace filters.
    pub filters: QueryFilters,
    /// Exact authorized source identities applied while candidates are fetched.
    pub source_restriction: KnowledgeSourceRestriction,
}

/// One ranked candidate list produced by a single scope, route, and query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedCandidateList {
    /// Index of the exactly-scoped slice that produced this list.
    pub scope_index: usize,
    /// Index of the source query that produced this list.
    pub query_index: usize,
    /// Route that produced this list.
    pub route: KnowledgeRoute,
    /// Candidate record identities in descending relevance order.
    pub record_ids: Vec<String>,
}

/// One route/query rank that contributed to a fused candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankContribution {
    /// Index of the contributing source query.
    pub query_index: usize,
    /// Contributing route.
    pub route: KnowledgeRoute,
    /// One-based rank within that route's result list.
    pub rank: usize,
}

/// One deterministically fused candidate before authorization.
#[derive(Debug, Clone, PartialEq)]
pub struct FusedCandidate {
    /// Stable derived record identity.
    pub record_id: String,
    /// Every rank that contributed, in execution order.
    pub contributions: Vec<RankContribution>,
    /// Reciprocal-rank-fusion score; never a mixed raw relevance score.
    pub score: f64,
}

/// One authorized source-oriented evidence row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEvidence {
    /// Stable derived record identity.
    pub record_id: String,
    /// Source occurrence identity.
    pub occurrence_id: String,
    /// Opaque host source identity.
    pub source_id: String,
    /// Other authorized occurrences carrying this exact chunk.
    pub duplicate_source_ids: Vec<String>,
    /// Content identity shared by every occurrence of the file.
    pub document_id: String,
    /// Owning semantic library.
    pub library_id: String,
    /// `chunk`, `summary`, or a future versioned kind.
    pub chunk_kind: String,
    /// Bounded display excerpt.
    pub excerpt: String,
    /// Complete structurally bounded chunk content.
    pub content: String,
    /// Token count charged against the context budget.
    pub token_count: usize,
    /// Structural heading hierarchy included in the embedding input.
    pub section_path: Vec<String>,
    /// Strongest real converter provenance.
    pub provenance: ChunkProvenance,
    /// Indexed IANA media type.
    pub media_type: Option<String>,
    /// Indexed source modification time.
    pub modified_at_ms: Option<i64>,
    /// Published source hash.
    pub indexed_content_hash: String,
    /// Published generation.
    pub generation: u64,
    /// Document order.
    pub source_position: u32,
    /// Generated rather than extracted evidence.
    pub generated: bool,
    /// Source is currently unavailable.
    pub unavailable: bool,
    /// Current known bytes differ from the indexed generation.
    pub stale: bool,
    /// Added after ranking as adjacent structural context.
    pub adjacent: bool,
    /// One-based final rank; adjacent rows inherit their primary's rank.
    pub final_rank: usize,
}

/// One traced source query.
///
/// Candidate counts describe the candidates this retrieval actually ranked,
/// summed over every scope. For a scope described by an exact source set they
/// are therefore already authorized counts, never the index's pre-authorization
/// top-k.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TracedQuery {
    /// Query text supplied by the caller.
    pub text: String,
    /// Reason the query was planned.
    pub reason: KnowledgeRetrievalReason,
    /// Dense candidates ranked for this query.
    pub semantic_candidates: usize,
    /// Lexical candidates ranked for this query.
    pub full_text_candidates: usize,
}

/// One traced evidence row. Content, excerpts, headings, and filesystem paths
/// are deliberately absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TracedEvidence {
    /// Stable derived record identity.
    pub record_id: String,
    /// Source occurrence identity.
    pub occurrence_id: String,
    /// Opaque host source identity.
    pub source_id: String,
    /// Content identity of the file.
    pub document_id: String,
    /// Ranks that produced this row.
    pub contributions: Vec<RankContribution>,
    /// Reciprocal-rank-fusion score.
    pub fused_score: f64,
    /// One-based final rank.
    pub final_rank: usize,
    /// Structural provenance inside the source.
    pub provenance: ChunkProvenance,
    /// Added after ranking as adjacent context.
    pub adjacent: bool,
    /// Generated rather than extracted evidence.
    pub generated: bool,
    /// Source is currently unavailable.
    pub unavailable: bool,
    /// Current known bytes differ from the indexed generation.
    pub stale: bool,
}

/// Bounded privacy-safe explanation of one retrieval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalTrace {
    /// Requested/applied routes and any fallback reason.
    pub route: RouteOutcome,
    /// Capabilities observed when the route was selected.
    pub capabilities: KnowledgeCapabilities,
    /// Rank constant used by fusion.
    pub rank_constant: u32,
    /// Source queries in execution order.
    pub queries: Vec<TracedQuery>,
    /// Final evidence rows in emitted order.
    pub entries: Vec<TracedEvidence>,
}

/// Deterministic bounded retrieval output.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeRetrieval {
    /// Requested/applied routes and any explicit fallback.
    pub route: RouteOutcome,
    /// Capabilities observed for this retrieval.
    pub capabilities: KnowledgeCapabilities,
    /// Ranked primary evidence with post-rank adjacent context.
    pub evidence: Vec<KnowledgeEvidence>,
    /// Complete-chunk tokens retained.
    pub token_count: usize,
    /// Honest requested-scope coverage.
    pub coverage: SearchCoverage,
    /// Optional bounded trace.
    pub trace: Option<RetrievalTrace>,
}

/// Typed knowledge retrieval failure.
#[derive(Debug, thiserror::Error)]
pub enum KnowledgeRetrievalError {
    /// No query, a blank query, or an oversized query was supplied.
    #[error("knowledge query must be non-empty and bounded")]
    EmptyQuery,
    /// More source queries than the bounded planner contract allows.
    #[error("{actual} source queries exceed the maximum of {maximum}")]
    TooManyQueries {
        /// Configured maximum.
        maximum: usize,
        /// Submitted count.
        actual: usize,
    },
    /// One or more bounded limits are invalid.
    #[error("knowledge retrieval policy is invalid")]
    InvalidPolicy,
    /// The exact authorized source set exceeded its bounded contract.
    #[error("{actual} authorized source identities exceed the maximum of {maximum}")]
    UnboundedSourceRestriction {
        /// Configured maximum.
        maximum: usize,
        /// Submitted count.
        actual: usize,
    },
    /// No scope, or more exactly-scoped slices than the contract allows.
    #[error("{actual} retrieval scopes exceed the maximum of {maximum}")]
    InvalidScopes {
        /// Configured maximum.
        maximum: usize,
        /// Submitted count.
        actual: usize,
    },
    /// The retrieval was cancelled.
    #[error("knowledge retrieval was cancelled")]
    Cancelled,
    /// A dense route was requested without a usable embedding capability.
    #[error("semantic retrieval is unavailable")]
    SemanticUnavailable,
    /// A lexical route was requested without a native full-text index.
    #[error("full-text retrieval is unavailable")]
    FullTextUnavailable,
    /// Neither route is available.
    #[error("no knowledge retrieval route is available")]
    RetrievalUnavailable,
    /// Local query embedding failed.
    #[error(transparent)]
    Embedding(#[from] EmbeddingError),
    /// The embedding backend omitted a query vector.
    #[error("knowledge query embedding is missing")]
    MissingQueryVector,
    /// Dense candidate retrieval failed.
    #[error("semantic candidate retrieval failed: {0}")]
    Index(String),
    /// Lexical candidate retrieval failed.
    #[error("full-text candidate retrieval failed: {0}")]
    FullTextIndex(String),
    /// Authoritative catalog reauthorization failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// Persisted evidence provenance was malformed.
    #[error("knowledge evidence provenance is invalid")]
    InvalidProvenance,
}

/// Local hybrid retrieval over one catalog, embedding runtime, and indexes.
pub struct KnowledgeRetrievalService {
    catalog: SemanticCatalog,
    embedder: Option<Arc<dyn EmbeddingProvider>>,
    vector_index: Option<Arc<dyn SemanticCandidateIndex>>,
    full_text_index: Option<Arc<dyn FullTextCandidateIndex>>,
}

impl KnowledgeRetrievalService {
    /// Composes the retrieval capability from independently optional parts.
    #[must_use]
    pub fn new(
        catalog: SemanticCatalog,
        embedder: Option<Arc<dyn EmbeddingProvider>>,
        vector_index: Option<Arc<dyn SemanticCandidateIndex>>,
        full_text_index: Option<Arc<dyn FullTextCandidateIndex>>,
    ) -> Self {
        Self {
            catalog,
            embedder,
            vector_index,
            full_text_index,
        }
    }

    /// Reports full-text and query-embedding availability independently.
    #[must_use]
    pub fn capabilities(&self) -> KnowledgeCapabilities {
        KnowledgeCapabilities {
            full_text: self.full_text_index.is_some(),
            query_embeddings: self.embedder.is_some() && self.vector_index.is_some(),
        }
    }

    /// Retrieves fused, authorized, source-oriented evidence.
    ///
    /// Every exactly-scoped slice contributes candidates, the slices are fused
    /// into one global ranking, and the bounded budgets are spent once over
    /// that ranking, so a multi-slice scope returns what a single unpartitioned
    /// retrieval of the same authorized scope would have returned.
    ///
    /// # Errors
    ///
    /// Returns typed validation, capability, cancellation, embedding, index,
    /// catalog, or provenance failures.
    pub fn retrieve(
        &self,
        request: KnowledgeRetrievalRequest,
        cancellation: &CancellationToken,
    ) -> Result<KnowledgeRetrieval, KnowledgeRetrievalError> {
        validate_request(&request)?;
        if cancellation.is_cancelled() {
            return Err(KnowledgeRetrievalError::Cancelled);
        }
        let capabilities = self.capabilities();
        let mut plan = RoutePlan::select(request.route, capabilities)?;
        // One read lease spans candidate authorization and materialization, so
        // a superseded record cannot be reclaimed between the two.
        let reader = self.catalog.begin_read()?;
        let lists = self.execute_routes(&reader, &request, &mut plan, cancellation)?;
        if cancellation.is_cancelled() {
            return Err(KnowledgeRetrievalError::Cancelled);
        }
        let fused = fuse_ranked_candidates(&lists, request.policy.rank_constant);
        let evidence = self.materialize(&reader, &fused, &lists, &request, cancellation)?;
        let token_count = evidence
            .iter()
            .map(|item| item.token_count)
            .fold(0, usize::saturating_add);
        let route = plan.outcome(request.route);
        let trace = request.policy.include_trace.then(|| RetrievalTrace {
            route,
            capabilities,
            rank_constant: request.policy.rank_constant,
            queries: trace_queries(&request.queries, &lists),
            entries: trace_entries(&evidence, &fused),
        });
        Ok(KnowledgeRetrieval {
            route,
            capabilities,
            evidence,
            token_count,
            coverage: request.coverage,
            trace,
        })
    }

    fn execute_routes(
        &self,
        reader: &CatalogReader,
        request: &KnowledgeRetrievalRequest,
        plan: &mut RoutePlan,
        cancellation: &CancellationToken,
    ) -> Result<Vec<RankedCandidateList>, KnowledgeRetrievalError> {
        let mut semantic = Vec::new();
        if plan.semantic {
            match self.query_semantic(reader, request, cancellation) {
                Ok(lists) => semantic = lists,
                Err(KnowledgeRetrievalError::Cancelled) => {
                    return Err(KnowledgeRetrievalError::Cancelled);
                }
                Err(error) => {
                    if !plan.full_text {
                        return Err(error);
                    }
                    plan.semantic = false;
                    plan.fallback_reason = Some(semantic_failure_reason(&error));
                }
            }
        }
        let mut full_text = Vec::new();
        if plan.full_text {
            match self.query_full_text(reader, request, cancellation) {
                Ok(lists) => full_text = lists,
                Err(KnowledgeRetrievalError::Cancelled) => {
                    return Err(KnowledgeRetrievalError::Cancelled);
                }
                Err(error) => {
                    if !plan.semantic {
                        return Err(error);
                    }
                    plan.full_text = false;
                    plan.fallback_reason = Some(RouteFallbackReason::FullTextQueryFailed);
                    full_text.clear();
                }
            }
        }
        if !plan.semantic {
            semantic.clear();
        }
        let mut lists = Vec::with_capacity(semantic.len() + full_text.len());
        for index in 0..request.queries.len() {
            lists.extend(
                semantic
                    .iter()
                    .chain(full_text.iter())
                    .filter(|list| list.query_index == index)
                    .cloned(),
            );
        }
        Ok(lists)
    }

    fn query_semantic(
        &self,
        reader: &CatalogReader,
        request: &KnowledgeRetrievalRequest,
        cancellation: &CancellationToken,
    ) -> Result<Vec<RankedCandidateList>, KnowledgeRetrievalError> {
        let (Some(embedder), Some(index)) = (&self.embedder, &self.vector_index) else {
            return Err(KnowledgeRetrievalError::SemanticUnavailable);
        };
        let texts = request
            .queries
            .iter()
            .map(|query| query.text.clone())
            .collect::<Vec<_>>();
        let vectors = match embedder.embed(&texts, cancellation) {
            Ok(vectors) => vectors,
            Err(EmbeddingError::Cancelled) => return Err(KnowledgeRetrievalError::Cancelled),
            Err(error) => return Err(KnowledgeRetrievalError::Embedding(error)),
        };
        if vectors.len() != texts.len() {
            return Err(KnowledgeRetrievalError::MissingQueryVector);
        }
        let mut lists = Vec::with_capacity(vectors.len() * request.scopes.len());
        for (scope_index, scope) in request.scopes.iter().enumerate() {
            for (query_index, vector) in vectors.iter().enumerate() {
                if cancellation.is_cancelled() {
                    return Err(KnowledgeRetrievalError::Cancelled);
                }
                let record_ids = fill_candidates(reader, scope, request.policy, |limit| {
                    index
                        .query(vector, limit, &scope.filters)
                        .map(|candidates| {
                            candidates
                                .into_iter()
                                .map(|candidate| candidate.record_id)
                                .collect()
                        })
                        .map_err(KnowledgeRetrievalError::Index)
                })?;
                lists.push(RankedCandidateList {
                    scope_index,
                    query_index,
                    route: KnowledgeRoute::Semantic,
                    record_ids,
                });
            }
        }
        Ok(lists)
    }

    fn query_full_text(
        &self,
        reader: &CatalogReader,
        request: &KnowledgeRetrievalRequest,
        cancellation: &CancellationToken,
    ) -> Result<Vec<RankedCandidateList>, KnowledgeRetrievalError> {
        let Some(index) = &self.full_text_index else {
            return Err(KnowledgeRetrievalError::FullTextUnavailable);
        };
        let mut lists = Vec::with_capacity(request.queries.len() * request.scopes.len());
        for (scope_index, scope) in request.scopes.iter().enumerate() {
            for (query_index, query) in request.queries.iter().enumerate() {
                if cancellation.is_cancelled() {
                    return Err(KnowledgeRetrievalError::Cancelled);
                }
                let record_ids = fill_candidates(reader, scope, request.policy, |limit| {
                    index
                        .query_full_text(&query.text, limit, &scope.filters)
                        .map_err(KnowledgeRetrievalError::FullTextIndex)
                })?;
                lists.push(RankedCandidateList {
                    scope_index,
                    query_index,
                    route: KnowledgeRoute::FullText,
                    record_ids,
                });
            }
        }
        Ok(lists)
    }

    /// Materializes one globally ranked candidate list into bounded evidence.
    ///
    /// Authorization is resolved per originating scope, because each scope owns
    /// its own filters and exact source set. The result, per-file, token, and
    /// adjacency budgets are then spent exactly once, walking the global
    /// ranking, so no scope can spend another scope's budget.
    fn materialize(
        &self,
        reader: &CatalogReader,
        fused: &[FusedCandidate],
        lists: &[RankedCandidateList],
        request: &KnowledgeRetrievalRequest,
        cancellation: &CancellationToken,
    ) -> Result<Vec<KnowledgeEvidence>, KnowledgeRetrievalError> {
        let authorized = self.authorize_candidates(reader, fused, lists, request, cancellation)?;
        let mut primaries = Vec::<(usize, QueryEvidence)>::new();
        let mut duplicates = HashMap::<String, BTreeSet<String>>::new();
        let mut logical_chunks = HashMap::<ChunkIdentity, String>::new();
        let mut results_per_file = HashMap::<String, usize>::new();
        let mut token_count = 0usize;
        for candidate in fused {
            if cancellation.is_cancelled() {
                return Err(KnowledgeRetrievalError::Cancelled);
            }
            let Some((scope_index, evidence)) = authorized.get(&candidate.record_id) else {
                continue;
            };
            let identity = ChunkIdentity::of(evidence);
            if let Some(existing) = logical_chunks.get(&identity) {
                let duplicates = duplicates.entry(existing.clone()).or_default();
                if duplicates.len() < MAX_DUPLICATE_SOURCES {
                    duplicates.insert(evidence.source_id.clone());
                }
                continue;
            }
            if primaries.len() >= request.policy.result_limit {
                continue;
            }
            let used = results_per_file
                .get(&evidence.document_id)
                .copied()
                .unwrap_or_default();
            if used >= request.policy.maximum_results_per_file {
                continue;
            }
            let Some(next_tokens) = token_count.checked_add(evidence.token_count) else {
                continue;
            };
            if next_tokens > request.policy.context_token_budget {
                continue;
            }
            token_count = next_tokens;
            logical_chunks.insert(identity, evidence.record_id.clone());
            results_per_file.insert(evidence.document_id.clone(), used + 1);
            primaries.push((*scope_index, evidence.clone()));
        }

        let mut evidence = Vec::new();
        for (index, (scope_index, primary)) in primaries.iter().enumerate() {
            if cancellation.is_cancelled() {
                return Err(KnowledgeRetrievalError::Cancelled);
            }
            let scope =
                request
                    .scopes
                    .get(*scope_index)
                    .ok_or(KnowledgeRetrievalError::InvalidScopes {
                        maximum: MAX_RETRIEVAL_SCOPES,
                        actual: request.scopes.len(),
                    })?;
            let final_rank = index + 1;
            let adjacent = if request.policy.adjacent_chunk_radius == 0 || primary.generated {
                Vec::new()
            } else {
                reader.adjacent_source_chunks(
                    primary,
                    request.policy.adjacent_chunk_radius,
                    &scope.filters,
                )?
            };
            if cancellation.is_cancelled() {
                return Err(KnowledgeRetrievalError::Cancelled);
            }
            evidence.push(build_evidence(
                primary.clone(),
                duplicates.get(&primary.record_id),
                false,
                final_rank,
                request,
            )?);
            for context in adjacent {
                if context.record_id == primary.record_id
                    || !scope.source_restriction.permits(&context.source_id)
                    || (request.policy.section_bounded_context
                        && context.section_path != primary.section_path)
                    || logical_chunks.contains_key(&ChunkIdentity::of(&context))
                {
                    continue;
                }
                let Some(next_tokens) = token_count.checked_add(context.token_count) else {
                    continue;
                };
                if next_tokens > request.policy.context_token_budget {
                    continue;
                }
                token_count = next_tokens;
                logical_chunks.insert(ChunkIdentity::of(&context), context.record_id.clone());
                evidence.push(build_evidence(context, None, true, final_rank, request)?);
            }
        }
        if cancellation.is_cancelled() {
            return Err(KnowledgeRetrievalError::Cancelled);
        }
        Ok(evidence)
    }

    /// Resolves every fused candidate against the scope that produced it.
    ///
    /// A candidate returned by more than one scope is authorized once, by the
    /// lowest scope index, so overlapping scopes cannot produce two evidence
    /// rows for the same record.
    fn authorize_candidates(
        &self,
        reader: &CatalogReader,
        fused: &[FusedCandidate],
        lists: &[RankedCandidateList],
        request: &KnowledgeRetrievalRequest,
        cancellation: &CancellationToken,
    ) -> Result<HashMap<String, (usize, QueryEvidence)>, KnowledgeRetrievalError> {
        let owners = scope_owners(lists);
        let mut authorized = HashMap::<String, (usize, QueryEvidence)>::new();
        for (scope_index, scope) in request.scopes.iter().enumerate() {
            let candidate_ids = fused
                .iter()
                .filter(|candidate| {
                    owners.get(&candidate.record_id) == Some(&scope_index)
                        && !authorized.contains_key(&candidate.record_id)
                })
                .map(|candidate| candidate.record_id.clone())
                .collect::<Vec<_>>();
            for batch in candidate_ids.chunks(AUTHORIZATION_BATCH_SIZE) {
                if cancellation.is_cancelled() {
                    return Err(KnowledgeRetrievalError::Cancelled);
                }
                for evidence in reader.filter_visible_candidates(batch, &scope.filters)? {
                    // The exact authorized source set is applied before any
                    // budget is spent, so an out-of-scope candidate that
                    // outranks an authorized one cannot consume a result slot,
                    // a per-file slot, or the token budget.
                    if !scope.source_restriction.permits(&evidence.source_id) {
                        continue;
                    }
                    authorized.insert(evidence.record_id.clone(), (scope_index, evidence));
                }
            }
        }
        Ok(authorized)
    }
}

#[derive(Debug, Clone, Copy)]
struct RoutePlan {
    semantic: bool,
    full_text: bool,
    fallback_reason: Option<RouteFallbackReason>,
}

impl RoutePlan {
    fn select(
        requested: KnowledgeRoute,
        capabilities: KnowledgeCapabilities,
    ) -> Result<Self, KnowledgeRetrievalError> {
        match requested {
            KnowledgeRoute::Semantic if !capabilities.query_embeddings => {
                Err(KnowledgeRetrievalError::SemanticUnavailable)
            }
            KnowledgeRoute::Semantic => Ok(Self {
                semantic: true,
                full_text: false,
                fallback_reason: None,
            }),
            KnowledgeRoute::FullText if !capabilities.full_text => {
                Err(KnowledgeRetrievalError::FullTextUnavailable)
            }
            KnowledgeRoute::FullText => Ok(Self {
                semantic: false,
                full_text: true,
                fallback_reason: None,
            }),
            KnowledgeRoute::Hybrid => match (capabilities.query_embeddings, capabilities.full_text)
            {
                (false, false) => Err(KnowledgeRetrievalError::RetrievalUnavailable),
                (true, true) => Ok(Self {
                    semantic: true,
                    full_text: true,
                    fallback_reason: None,
                }),
                (false, true) => Ok(Self {
                    semantic: false,
                    full_text: true,
                    fallback_reason: Some(RouteFallbackReason::QueryEmbeddingsUnavailable),
                }),
                (true, false) => Ok(Self {
                    semantic: true,
                    full_text: false,
                    fallback_reason: Some(RouteFallbackReason::FullTextIndexUnavailable),
                }),
            },
        }
    }

    fn outcome(self, requested: KnowledgeRoute) -> RouteOutcome {
        let applied = match (self.semantic, self.full_text) {
            (true, true) => KnowledgeRoute::Hybrid,
            (true, false) => KnowledgeRoute::Semantic,
            _ => KnowledgeRoute::FullText,
        };
        RouteOutcome {
            requested,
            applied,
            fallback_reason: (applied != requested)
                .then_some(self.fallback_reason)
                .flatten(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ChunkIdentity {
    document_id: String,
    record_kind: String,
    generation: u64,
    source_position: u32,
}

impl ChunkIdentity {
    fn of(evidence: &QueryEvidence) -> Self {
        Self {
            document_id: evidence.document_id.clone(),
            record_kind: evidence.record_kind.clone(),
            generation: evidence.generation,
            source_position: evidence.source_position,
        }
    }
}

/// Fuses independent scope, route, and query rankings with deterministic
/// reciprocal-rank fusion.
///
/// Only rank positions are combined. A candidate contributes at most one rank
/// per source query and route — its best one — no matter how many lists or
/// scopes returned it, so neither a repeated identity inside one list nor an
/// overlap between two scopes can inflate its own fused score.
#[must_use]
pub fn fuse_ranked_candidates(
    lists: &[RankedCandidateList],
    rank_constant: u32,
) -> Vec<FusedCandidate> {
    let mut fused = BTreeMap::<String, FusedCandidate>::new();
    for list in lists {
        let mut seen = BTreeSet::new();
        let mut rank = 0usize;
        for record_id in &list.record_ids {
            if !seen.insert(record_id.as_str()) {
                continue;
            }
            rank += 1;
            let entry = fused
                .entry(record_id.clone())
                .or_insert_with(|| FusedCandidate {
                    record_id: record_id.clone(),
                    contributions: Vec::new(),
                    score: 0.0,
                });
            if let Some(existing) = entry.contributions.iter_mut().find(|contribution| {
                contribution.query_index == list.query_index && contribution.route == list.route
            }) {
                existing.rank = existing.rank.min(rank);
                continue;
            }
            entry.contributions.push(RankContribution {
                query_index: list.query_index,
                route: list.route,
                rank,
            });
        }
    }
    let mut ranked = fused.into_values().collect::<Vec<_>>();
    for candidate in &mut ranked {
        candidate.score = candidate
            .contributions
            .iter()
            .map(|contribution| 1.0 / (f64::from(rank_constant) + contribution.rank as f64))
            .sum();
    }
    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.record_id.cmp(&right.record_id))
    });
    ranked
}

/// Fills one scope's candidate budget with candidates it may actually return.
///
/// An unrestricted scope is already exact, so its top-k is used verbatim. A
/// scope described by an exact source set is not expressible as an index
/// filter, so the index is overfetched in deterministic doubling pages and each
/// page is reduced to its authorized candidates until the budget is filled, the
/// index is exhausted, or the bounded overfetch ceiling is reached. Relevance
/// order is preserved throughout.
fn fill_candidates<Fetch>(
    reader: &CatalogReader,
    scope: &KnowledgeRetrievalScope,
    policy: KnowledgeRetrievalPolicy,
    mut fetch: Fetch,
) -> Result<Vec<String>, KnowledgeRetrievalError>
where
    Fetch: FnMut(usize) -> Result<Vec<String>, KnowledgeRetrievalError>,
{
    let budget = policy.candidate_limit;
    if scope.source_restriction.allowed_source_ids.is_empty() {
        return fetch(budget);
    }
    let ceiling = budget
        .saturating_mul(MAX_RESTRICTED_OVERFETCH_FACTOR)
        .min(MAX_RESTRICTED_CANDIDATE_PAGE)
        .max(budget);
    let mut page = budget;
    loop {
        let candidates = fetch(page)?;
        let exhausted = candidates.len() < page;
        let mut authorized = Vec::with_capacity(budget.min(candidates.len()));
        for batch in candidates.chunks(AUTHORIZATION_BATCH_SIZE) {
            for (record_id, source_id) in reader.visible_candidate_sources(batch, &scope.filters)? {
                if scope.source_restriction.permits(&source_id) {
                    authorized.push(record_id);
                }
            }
        }
        if authorized.len() >= budget {
            authorized.truncate(budget);
            return Ok(authorized);
        }
        if exhausted || page >= ceiling {
            return Ok(authorized);
        }
        page = page.saturating_mul(2).min(ceiling);
    }
}

/// Maps every candidate identity to the lowest scope index that produced it.
fn scope_owners(lists: &[RankedCandidateList]) -> HashMap<String, usize> {
    let mut owners = HashMap::<String, usize>::new();
    for list in lists {
        for record_id in &list.record_ids {
            owners
                .entry(record_id.clone())
                .and_modify(|owner| *owner = (*owner).min(list.scope_index))
                .or_insert(list.scope_index);
        }
    }
    owners
}

const fn semantic_failure_reason(error: &KnowledgeRetrievalError) -> RouteFallbackReason {
    match error {
        KnowledgeRetrievalError::Index(_) => RouteFallbackReason::SemanticQueryFailed,
        _ => RouteFallbackReason::QueryEmbeddingFailed,
    }
}

fn validate_request(request: &KnowledgeRetrievalRequest) -> Result<(), KnowledgeRetrievalError> {
    if !request.policy.valid() {
        return Err(KnowledgeRetrievalError::InvalidPolicy);
    }
    if request.queries.len() > MAX_SOURCE_QUERIES {
        return Err(KnowledgeRetrievalError::TooManyQueries {
            maximum: MAX_SOURCE_QUERIES,
            actual: request.queries.len(),
        });
    }
    if request.queries.is_empty()
        || request
            .queries
            .iter()
            .any(|query| query.text.trim().is_empty() || query.text.len() > MAX_QUERY_BYTES)
    {
        return Err(KnowledgeRetrievalError::EmptyQuery);
    }
    if request.scopes.is_empty() || request.scopes.len() > MAX_RETRIEVAL_SCOPES {
        return Err(KnowledgeRetrievalError::InvalidScopes {
            maximum: MAX_RETRIEVAL_SCOPES,
            actual: request.scopes.len(),
        });
    }
    for scope in &request.scopes {
        if !scope.source_restriction.bounded() {
            return Err(KnowledgeRetrievalError::UnboundedSourceRestriction {
                maximum: MAX_ALLOWED_SOURCES,
                actual: scope.source_restriction.allowed_source_ids.len(),
            });
        }
    }
    Ok(())
}

fn build_evidence(
    evidence: QueryEvidence,
    duplicates: Option<&BTreeSet<String>>,
    adjacent: bool,
    final_rank: usize,
    request: &KnowledgeRetrievalRequest,
) -> Result<KnowledgeEvidence, KnowledgeRetrievalError> {
    let provenance = serde_json::from_str(&evidence.provenance)
        .map_err(|_| KnowledgeRetrievalError::InvalidProvenance)?;
    let stale = request
        .current_hashes
        .get(&evidence.source_id)
        .is_some_and(|hash| hash != &evidence.content_hash);
    Ok(KnowledgeEvidence {
        record_id: evidence.record_id,
        occurrence_id: evidence.occurrence_id,
        source_id: evidence.source_id,
        duplicate_source_ids: duplicates
            .map(|sources| sources.iter().cloned().collect())
            .unwrap_or_default(),
        document_id: evidence.document_id,
        library_id: evidence.library_id,
        chunk_kind: evidence.record_kind,
        excerpt: evidence.excerpt,
        content: evidence.content,
        token_count: evidence.token_count,
        section_path: evidence.section_path,
        provenance,
        media_type: Some(evidence.media_type),
        modified_at_ms: (evidence.modified_at_ms != 0).then_some(evidence.modified_at_ms),
        indexed_content_hash: evidence.content_hash,
        generation: evidence.generation,
        source_position: evidence.source_position,
        generated: evidence.generated,
        unavailable: !evidence.available,
        stale,
        adjacent,
        final_rank,
    })
}

fn trace_queries(queries: &[KnowledgeQuery], lists: &[RankedCandidateList]) -> Vec<TracedQuery> {
    queries
        .iter()
        .enumerate()
        .map(|(index, query)| {
            let count = |route: KnowledgeRoute| {
                lists
                    .iter()
                    .filter(|list| list.query_index == index && list.route == route)
                    .map(|list| list.record_ids.len())
                    .sum()
            };
            TracedQuery {
                text: query.text.clone(),
                reason: query.reason,
                semantic_candidates: count(KnowledgeRoute::Semantic),
                full_text_candidates: count(KnowledgeRoute::FullText),
            }
        })
        .collect()
}

fn trace_entries(evidence: &[KnowledgeEvidence], fused: &[FusedCandidate]) -> Vec<TracedEvidence> {
    let fused = fused
        .iter()
        .map(|candidate| (candidate.record_id.as_str(), candidate))
        .collect::<HashMap<_, _>>();
    evidence
        .iter()
        .map(|item| {
            let candidate = fused.get(item.record_id.as_str());
            TracedEvidence {
                record_id: item.record_id.clone(),
                occurrence_id: item.occurrence_id.clone(),
                source_id: item.source_id.clone(),
                document_id: item.document_id.clone(),
                contributions: candidate
                    .map(|candidate| candidate.contributions.clone())
                    .unwrap_or_default(),
                fused_score: candidate.map_or(0.0, |candidate| candidate.score),
                final_rank: item.final_rank,
                provenance: item.provenance.clone(),
                adjacent: item.adjacent,
                generated: item.generated,
                unavailable: item.unavailable,
                stale: item.stale,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::embedding::{
        EmbeddingCacheKey, EmbeddingError, EmbeddingModelIdentity, VectorNormalization,
    };
    use crate::ingestion::EmbeddingProvider;
    use crate::semantic_search::{ScoredRecord, SemanticCandidateIndex};
    use crate::semantic_storage::{
        DistanceMetric, LibraryIndexManifest, Occurrence, QueryFilters, SemanticCatalog,
        StagedGeneration, StagedRecord,
    };

    struct FakeEmbedder {
        identity: EmbeddingModelIdentity,
        failure: Option<EmbeddingError>,
        calls: Mutex<usize>,
    }

    impl FakeEmbedder {
        fn working() -> Arc<Self> {
            Arc::new(Self {
                identity: identity(),
                failure: None,
                calls: Mutex::new(0),
            })
        }

        fn failing(failure: EmbeddingError) -> Arc<Self> {
            Arc::new(Self {
                identity: identity(),
                failure: Some(failure),
                calls: Mutex::new(0),
            })
        }
    }

    impl EmbeddingProvider for FakeEmbedder {
        fn identity(&self) -> &EmbeddingModelIdentity {
            &self.identity
        }

        fn embed(
            &self,
            inputs: &[String],
            cancellation: &CancellationToken,
        ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            *self.calls.lock().unwrap() += 1;
            if cancellation.is_cancelled() {
                return Err(EmbeddingError::Cancelled);
            }
            match &self.failure {
                Some(EmbeddingError::Cancelled) => Err(EmbeddingError::Cancelled),
                Some(_) => Err(EmbeddingError::ModelIdentityMismatch),
                None => Ok(inputs.iter().map(|_| vec![0.0, 0.6, 0.8]).collect()),
            }
        }
    }

    #[derive(Default)]
    struct FakeVectorIndex {
        results: Mutex<Vec<Vec<ScoredRecord>>>,
        failure: Option<String>,
    }

    impl FakeVectorIndex {
        fn returning(results: Vec<Vec<(&str, f32)>>) -> Arc<Self> {
            Arc::new(Self {
                results: Mutex::new(
                    results
                        .into_iter()
                        .map(|batch| {
                            batch
                                .into_iter()
                                .map(|(record_id, score)| ScoredRecord {
                                    record_id: record_id.into(),
                                    score,
                                })
                                .collect()
                        })
                        .collect(),
                ),
                failure: None,
            })
        }
    }

    impl SemanticCandidateIndex for FakeVectorIndex {
        fn query(
            &self,
            _vector: &[f32],
            _limit: usize,
            _filters: &QueryFilters,
        ) -> Result<Vec<ScoredRecord>, String> {
            if let Some(failure) = &self.failure {
                return Err(failure.clone());
            }
            let mut results = self.results.lock().unwrap();
            if results.is_empty() {
                return Ok(Vec::new());
            }
            Ok(results.remove(0))
        }
    }

    #[derive(Default)]
    struct FakeFullTextIndex {
        results: Mutex<Vec<Vec<String>>>,
    }

    impl FakeFullTextIndex {
        fn returning(results: Vec<Vec<&str>>) -> Arc<Self> {
            Self::returning_owned(
                results
                    .into_iter()
                    .map(|batch| batch.into_iter().map(str::to_owned).collect())
                    .collect(),
            )
        }

        fn returning_owned(results: Vec<Vec<String>>) -> Arc<Self> {
            Arc::new(Self {
                results: Mutex::new(results),
            })
        }
    }

    impl FullTextCandidateIndex for FakeFullTextIndex {
        fn query_full_text(
            &self,
            _text: &str,
            _limit: usize,
            _filters: &QueryFilters,
        ) -> Result<Vec<String>, String> {
            let mut results = self.results.lock().unwrap();
            if results.is_empty() {
                return Ok(Vec::new());
            }
            Ok(results.remove(0))
        }
    }

    fn identity() -> EmbeddingModelIdentity {
        EmbeddingModelIdentity {
            model_id: "embed-model".into(),
            model_revision: "embed-revision".into(),
            tokenizer: "tokenizer-revision".into(),
            dimensions: 3,
            max_input_tokens: 8_192,
        }
    }

    fn manifest() -> LibraryIndexManifest {
        LibraryIndexManifest {
            zvec_schema_version: 2,
            dimensions: 3,
            distance_metric: DistanceMetric::Cosine,
            model_revision: "embed-revision".into(),
            tokenizer: "tokenizer-revision".into(),
            embedding_preprocessing: "unicode-default-case-fold/1".into(),
            normalization: VectorNormalization::L2,
            chunker_version: "structural/2".into(),
            converter_version: "text/1".into(),
        }
    }

    fn occurrence(
        occurrence_id: &str,
        source_id: &str,
        root_id: &str,
        workspace_id: &str,
        available: bool,
    ) -> Occurrence {
        Occurrence {
            occurrence_id: occurrence_id.into(),
            source_id: source_id.into(),
            root_id: root_id.into(),
            workspace_id: Some(workspace_id.into()),
            media_type: "text/plain".into(),
            modified_at_ms: 1_000,
            available,
            provenance: "source".into(),
        }
    }

    fn record(record_id: &str, occurrence_id: &str, position: u32) -> StagedRecord {
        section_record(record_id, occurrence_id, position, "Section")
    }

    fn section_record(
        record_id: &str,
        occurrence_id: &str,
        position: u32,
        section: &str,
    ) -> StagedRecord {
        let content = format!("complete text for {record_id}");
        StagedRecord {
            record_id: record_id.into(),
            occurrence_id: occurrence_id.into(),
            cache_key: EmbeddingCacheKey::calculate(
                &content,
                &identity(),
                &identity().tokenizer,
                "structural/2",
            ),
            vector: vec![0.0, 0.6, 0.8],
            record_kind: "chunk".into(),
            excerpt: format!("excerpt {record_id}"),
            content,
            token_count: 10,
            section_path: vec![section.into()],
            structural_role: "body".into(),
            provenance: serde_json::to_string(&ChunkProvenance::Exact(
                fm_semantic_conversion::Provenance::TextLines {
                    start_line: position + 1,
                    end_line: position + 2,
                },
            ))
            .expect("serialize provenance"),
            source_position: position,
            generated: false,
            concept_id: None,
        }
    }

    fn publish(
        catalog: &SemanticCatalog,
        tenant_id: &str,
        document_id: &str,
        occurrences: Vec<Occurrence>,
        records: Vec<StagedRecord>,
    ) {
        catalog
            .stage_generation(&StagedGeneration {
                tenant_id: tenant_id.into(),
                library_id: "library-a".into(),
                document_id: document_id.into(),
                content_hash: format!("hash-{document_id}"),
                generation: 1,
                occurrences,
                records,
            })
            .expect("stage");
        catalog
            .publish_generation(tenant_id, "library-a", document_id, 1)
            .expect("publish");
    }

    fn catalog_path() -> std::path::PathBuf {
        tempdir()
            .expect("temporary directory")
            .keep()
            .join("catalog.sqlite")
    }

    fn fixture(path: &std::path::Path) -> SemanticCatalog {
        let catalog = SemanticCatalog::open(path).expect("catalog");
        catalog
            .register_library("tenant-a", "library-a", &manifest())
            .expect("register tenant-a");
        catalog
            .register_library("tenant-b", "library-a", &manifest())
            .expect("register tenant-b");
        publish(
            &catalog,
            "tenant-a",
            "document-a",
            vec![
                occurrence("occurrence-a", "source-a", "root-a", "workspace-a", true),
                occurrence(
                    "occurrence-a-copy",
                    "source-a-copy",
                    "root-a",
                    "workspace-a",
                    true,
                ),
            ],
            vec![
                record("a-0", "occurrence-a", 0),
                record("a-1", "occurrence-a", 1),
                record("a-2", "occurrence-a", 2),
                record("a-0-copy", "occurrence-a-copy", 0),
            ],
        );
        publish(
            &catalog,
            "tenant-a",
            "document-b",
            vec![occurrence(
                "occurrence-b",
                "source-b",
                "root-b",
                "workspace-b",
                true,
            )],
            vec![
                record("b-0", "occurrence-b", 0),
                record("b-1", "occurrence-b", 1),
            ],
        );
        publish(
            &catalog,
            "tenant-a",
            "document-c",
            vec![occurrence(
                "occurrence-c",
                "source-c",
                "root-a",
                "workspace-a",
                false,
            )],
            vec![record("c-0", "occurrence-c", 0)],
        );
        publish(
            &catalog,
            "tenant-a",
            "document-e",
            vec![occurrence(
                "occurrence-e",
                "source-e",
                "root-a",
                "workspace-a",
                true,
            )],
            vec![
                section_record("e-0", "occurrence-e", 0, "Alpha"),
                section_record("e-1", "occurrence-e", 1, "Alpha"),
                section_record("e-2", "occurrence-e", 2, "Beta"),
            ],
        );
        publish(
            &catalog,
            "tenant-b",
            "document-d",
            vec![occurrence(
                "occurrence-d",
                "source-d",
                "root-d",
                "workspace-a",
                true,
            )],
            vec![record("d-0", "occurrence-d", 0)],
        );
        catalog
    }

    /// A catalog whose authorized scope spans two enrolled roots, including one
    /// file that occurs under both of them.
    fn multi_root_fixture(path: &std::path::Path) -> SemanticCatalog {
        let catalog = SemanticCatalog::open(path).expect("catalog");
        catalog
            .register_library("tenant-a", "library-a", &manifest())
            .expect("register tenant-a");
        // One file present under both roots, at the same structural position in
        // both, so the two occurrences are one logical chunk.
        publish(
            &catalog,
            "tenant-a",
            "document-x",
            vec![
                occurrence(
                    "occurrence-x-a",
                    "source-x-a",
                    "root-a",
                    "workspace-a",
                    true,
                ),
                occurrence(
                    "occurrence-x-b",
                    "source-x-b",
                    "root-b",
                    "workspace-a",
                    true,
                ),
            ],
            vec![
                record("x-0", "occurrence-x-a", 0),
                record("x-0b", "occurrence-x-b", 0),
            ],
        );
        // One file present under both roots whose indexed chunks do not
        // overlap, so its per-file budget can only be honored globally.
        publish(
            &catalog,
            "tenant-a",
            "document-m",
            vec![
                occurrence(
                    "occurrence-m-a",
                    "source-m-a",
                    "root-a",
                    "workspace-a",
                    true,
                ),
                occurrence(
                    "occurrence-m-b",
                    "source-m-b",
                    "root-b",
                    "workspace-a",
                    true,
                ),
            ],
            vec![
                record("m-a-0", "occurrence-m-a", 0),
                record("m-a-1", "occurrence-m-a", 1),
                record("m-b-2", "occurrence-m-b", 2),
                record("m-b-3", "occurrence-m-b", 3),
            ],
        );
        publish(
            &catalog,
            "tenant-a",
            "document-d1",
            vec![occurrence(
                "occurrence-d1",
                "source-d1",
                "root-a",
                "workspace-a",
                true,
            )],
            vec![
                record("d1-0", "occurrence-d1", 0),
                record("d1-1", "occurrence-d1", 1),
            ],
        );
        publish(
            &catalog,
            "tenant-a",
            "document-d2",
            vec![occurrence(
                "occurrence-d2",
                "source-d2",
                "root-b",
                "workspace-a",
                true,
            )],
            vec![
                record("d2-0", "occurrence-d2", 0),
                record("d2-1", "occurrence-d2", 1),
            ],
        );
        catalog
    }

    /// Full-text index answering from a fixed per-root corpus that honors the
    /// requested top-k, so overfetching is observable.
    struct RootCorpusFullTextIndex {
        corpora: BTreeMap<String, Vec<String>>,
        limits: Mutex<Vec<usize>>,
    }

    impl RootCorpusFullTextIndex {
        fn new(corpora: &[(&str, &[&str])]) -> Arc<Self> {
            Arc::new(Self {
                corpora: corpora
                    .iter()
                    .map(|(root, records)| {
                        (
                            (*root).to_owned(),
                            records.iter().map(|record| (*record).to_owned()).collect(),
                        )
                    })
                    .collect(),
                limits: Mutex::new(Vec::new()),
            })
        }

        fn limits(&self) -> Vec<usize> {
            self.limits.lock().unwrap().clone()
        }
    }

    impl FullTextCandidateIndex for RootCorpusFullTextIndex {
        fn query_full_text(
            &self,
            _text: &str,
            limit: usize,
            filters: &QueryFilters,
        ) -> Result<Vec<String>, String> {
            self.limits.lock().unwrap().push(limit);
            let root = filters.root_id.clone().unwrap_or_default();
            Ok(self
                .corpora
                .get(&root)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .take(limit)
                .collect())
        }
    }

    /// Dense index answering from a fixed per-root corpus that honors top-k.
    struct RootCorpusVectorIndex {
        corpora: BTreeMap<String, Vec<String>>,
        limits: Mutex<Vec<usize>>,
    }

    impl RootCorpusVectorIndex {
        fn new(corpora: &[(&str, &[&str])]) -> Arc<Self> {
            Arc::new(Self {
                corpora: corpora
                    .iter()
                    .map(|(root, records)| {
                        (
                            (*root).to_owned(),
                            records.iter().map(|record| (*record).to_owned()).collect(),
                        )
                    })
                    .collect(),
                limits: Mutex::new(Vec::new()),
            })
        }

        fn limits(&self) -> Vec<usize> {
            self.limits.lock().unwrap().clone()
        }
    }

    impl SemanticCandidateIndex for RootCorpusVectorIndex {
        fn query(
            &self,
            _vector: &[f32],
            limit: usize,
            filters: &QueryFilters,
        ) -> Result<Vec<ScoredRecord>, String> {
            self.limits.lock().unwrap().push(limit);
            let root = filters.root_id.clone().unwrap_or_default();
            Ok(self
                .corpora
                .get(&root)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .take(limit)
                .enumerate()
                .map(|(index, record_id)| ScoredRecord {
                    record_id,
                    score: 1.0 - index as f32 / 100.0,
                })
                .collect())
        }
    }
    fn filters() -> QueryFilters {
        QueryFilters {
            tenant_id: "tenant-a".into(),
            library_id: Some("library-a".into()),
            ..QueryFilters::default()
        }
    }

    fn policy() -> KnowledgeRetrievalPolicy {
        KnowledgeRetrievalPolicy {
            adjacent_chunk_radius: 0,
            include_trace: false,
            ..KnowledgeRetrievalPolicy::default_search()
        }
    }

    fn request(
        route: KnowledgeRoute,
        policy: KnowledgeRetrievalPolicy,
    ) -> KnowledgeRetrievalRequest {
        scoped_request(route, policy, vec![scope(filters(), &[])])
    }

    fn scoped_request(
        route: KnowledgeRoute,
        policy: KnowledgeRetrievalPolicy,
        scopes: Vec<KnowledgeRetrievalScope>,
    ) -> KnowledgeRetrievalRequest {
        KnowledgeRetrievalRequest {
            queries: vec![KnowledgeQuery {
                text: "structured knowledge".into(),
                reason: KnowledgeRetrievalReason::Subject,
            }],
            route,
            scopes,
            current_hashes: HashMap::new(),
            coverage: crate::semantic_search::SearchCoverage::default(),
            policy,
        }
    }

    fn scope(filters: QueryFilters, allowed: &[&str]) -> KnowledgeRetrievalScope {
        KnowledgeRetrievalScope {
            filters,
            source_restriction: KnowledgeSourceRestriction {
                allowed_source_ids: allowed.iter().map(|source| (*source).to_owned()).collect(),
            },
        }
    }

    fn root_filters(root_id: &str) -> QueryFilters {
        QueryFilters {
            root_id: Some(root_id.into()),
            ..filters()
        }
    }

    fn record_ids(retrieval: &KnowledgeRetrieval) -> Vec<&str> {
        retrieval
            .evidence
            .iter()
            .map(|evidence| evidence.record_id.as_str())
            .collect()
    }

    #[test]
    fn hybrid_fuses_ranks_deterministically_without_mixing_score_domains() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            Some(FakeEmbedder::working()),
            Some(FakeVectorIndex::returning(vec![vec![
                ("a-0", 0.99),
                ("b-0", 0.51),
            ]])),
            Some(FakeFullTextIndex::returning(vec![vec!["b-0", "a-1"]])),
        );

        let retrieval = service
            .retrieve(
                request(KnowledgeRoute::Hybrid, policy()),
                &CancellationToken::new(),
            )
            .expect("hybrid retrieval");

        assert_eq!(retrieval.route.applied, KnowledgeRoute::Hybrid);
        assert_eq!(retrieval.route.fallback_reason, None);
        assert_eq!(record_ids(&retrieval), ["b-0", "a-0", "a-1"]);
        assert_eq!(retrieval.evidence[0].final_rank, 1);
        assert_eq!(retrieval.evidence[2].final_rank, 3);
    }

    #[test]
    fn fusion_ignores_candidate_input_order_and_raw_scores() {
        let ranked = fuse_ranked_candidates(
            &[
                RankedCandidateList {
                    scope_index: 0,
                    query_index: 0,
                    route: KnowledgeRoute::Semantic,
                    record_ids: vec!["a-0".into(), "b-0".into(), "a-0".into()],
                },
                RankedCandidateList {
                    scope_index: 0,
                    query_index: 0,
                    route: KnowledgeRoute::FullText,
                    record_ids: vec!["b-0".into(), "c-0".into()],
                },
            ],
            DEFAULT_RANK_CONSTANT,
        );

        assert_eq!(
            ranked
                .iter()
                .map(|candidate| candidate.record_id.as_str())
                .collect::<Vec<_>>(),
            ["b-0", "a-0", "c-0"]
        );
        assert_eq!(ranked[0].contributions.len(), 2);
        assert_eq!(ranked[0].contributions[0].rank, 2);
        assert_eq!(ranked[0].contributions[1].rank, 1);
    }

    /// The exact authorized source set must be applied before the result,
    /// per-file, and token budgets: an out-of-scope candidate that outranks an
    /// authorized one must not consume the scope's only result slot.
    #[test]
    fn out_of_scope_candidates_never_crowd_out_authorized_ones() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![
                vec!["b-0", "b-1", "a-0"],
                vec!["b-0", "b-1", "a-0"],
            ])),
        );
        let narrow = KnowledgeRetrievalPolicy {
            result_limit: 1,
            ..policy()
        };

        let unrestricted = service
            .retrieve(
                request(KnowledgeRoute::FullText, narrow),
                &CancellationToken::new(),
            )
            .expect("unrestricted retrieval");
        assert_eq!(record_ids(&unrestricted), ["b-0"]);

        let restricted = scoped_request(
            KnowledgeRoute::FullText,
            narrow,
            vec![scope(filters(), &["source-a"])],
        );
        let restricted = service
            .retrieve(restricted, &CancellationToken::new())
            .expect("restricted retrieval");

        assert_eq!(record_ids(&restricted), ["a-0"]);
        assert_eq!(restricted.evidence[0].source_id, "source-a");
    }

    /// A scope described by an exact source set must still return a full
    /// candidate budget of candidates it may actually show. Checking the
    /// restriction only after the index's top-k lets unauthorized records
    /// consume the budget and silently starves the scope.
    #[test]
    fn a_restricted_full_text_scope_fills_its_candidate_budget_with_authorized_hits() {
        let path = catalog_path();
        let index = RootCorpusFullTextIndex::new(&[(
            "root-a",
            &["b-0", "b-1", "e-0", "e-1", "a-0", "a-1"],
        )]);
        let service =
            KnowledgeRetrievalService::new(fixture(&path), None, None, Some(index.clone()));
        let narrow = KnowledgeRetrievalPolicy {
            candidate_limit: 2,
            ..policy()
        };

        let retrieval = service
            .retrieve(
                scoped_request(
                    KnowledgeRoute::FullText,
                    narrow,
                    vec![scope(root_filters("root-a"), &["source-a"])],
                ),
                &CancellationToken::new(),
            )
            .expect("restricted retrieval");

        assert_eq!(record_ids(&retrieval), ["a-0", "a-1"]);
        // The budget is filled by deterministic doubling, never by asking the
        // index for an unbounded page.
        assert_eq!(index.limits(), vec![2, 4, 8]);
    }

    #[test]
    fn a_restricted_semantic_scope_fills_its_candidate_budget_with_authorized_hits() {
        let path = catalog_path();
        let index =
            RootCorpusVectorIndex::new(&[("root-a", &["b-0", "b-1", "e-0", "e-1", "a-0", "a-1"])]);
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            Some(FakeEmbedder::working()),
            Some(index.clone()),
            None,
        );
        let narrow = KnowledgeRetrievalPolicy {
            candidate_limit: 2,
            ..policy()
        };

        let retrieval = service
            .retrieve(
                scoped_request(
                    KnowledgeRoute::Semantic,
                    narrow,
                    vec![scope(root_filters("root-a"), &["source-a"])],
                ),
                &CancellationToken::new(),
            )
            .expect("restricted retrieval");

        assert_eq!(record_ids(&retrieval), ["a-0", "a-1"]);
        assert_eq!(index.limits(), vec![2, 4, 8]);
    }

    /// Overfetching is bounded: an authorized budget that cannot be filled
    /// returns what the scope really has instead of paging without end.
    #[test]
    fn filling_a_restricted_budget_stops_at_the_bounded_overfetch_ceiling() {
        let path = catalog_path();
        let index = RootCorpusFullTextIndex::new(&[(
            "root-a",
            &["b-0", "b-1", "e-0", "e-1", "e-2", "a-0"],
        )]);
        let service =
            KnowledgeRetrievalService::new(fixture(&path), None, None, Some(index.clone()));
        let narrow = KnowledgeRetrievalPolicy {
            candidate_limit: 4,
            ..policy()
        };

        let retrieval = service
            .retrieve(
                scoped_request(
                    KnowledgeRoute::FullText,
                    narrow,
                    vec![scope(root_filters("root-a"), &["source-a"])],
                ),
                &CancellationToken::new(),
            )
            .expect("restricted retrieval");

        assert_eq!(record_ids(&retrieval), ["a-0"]);
        assert!(index.limits().len() <= 4, "{:?}", index.limits());
    }

    /// An unrestricted scope is already exact, so nothing is overfetched.
    #[test]
    fn an_unrestricted_scope_asks_the_index_for_its_candidate_limit_once() {
        let path = catalog_path();
        let index = RootCorpusFullTextIndex::new(&[("root-a", &["a-0", "a-1", "a-2"])]);
        let service =
            KnowledgeRetrievalService::new(fixture(&path), None, None, Some(index.clone()));

        service
            .retrieve(
                scoped_request(
                    KnowledgeRoute::FullText,
                    KnowledgeRetrievalPolicy {
                        candidate_limit: 3,
                        ..policy()
                    },
                    vec![scope(root_filters("root-a"), &[])],
                ),
                &CancellationToken::new(),
            )
            .expect("unrestricted retrieval");

        assert_eq!(index.limits(), vec![3]);
    }

    fn multi_root_scopes() -> Vec<KnowledgeRetrievalScope> {
        vec![
            scope(root_filters("root-a"), &[]),
            scope(root_filters("root-b"), &[]),
        ]
    }

    /// The result limit belongs to the search, not to each partition of the
    /// scope: two roots must not return two full result sets.
    #[test]
    fn a_multi_root_scope_spends_one_global_result_limit() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            multi_root_fixture(&path),
            None,
            None,
            Some(RootCorpusFullTextIndex::new(&[
                ("root-a", &["d1-0", "d1-1"]),
                ("root-b", &["d2-0", "d2-1"]),
            ])),
        );

        let retrieval = service
            .retrieve(
                scoped_request(
                    KnowledgeRoute::FullText,
                    KnowledgeRetrievalPolicy {
                        result_limit: 2,
                        ..policy()
                    },
                    multi_root_scopes(),
                ),
                &CancellationToken::new(),
            )
            .expect("multi-root retrieval");

        assert_eq!(record_ids(&retrieval), ["d1-0", "d2-0"]);
        assert_eq!(retrieval.evidence[0].final_rank, 1);
        assert_eq!(retrieval.evidence[1].final_rank, 2);
    }

    /// So does the complete-chunk token budget.
    #[test]
    fn a_multi_root_scope_spends_one_global_token_budget() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            multi_root_fixture(&path),
            None,
            None,
            Some(RootCorpusFullTextIndex::new(&[
                ("root-a", &["d1-0", "d1-1"]),
                ("root-b", &["d2-0", "d2-1"]),
            ])),
        );

        let retrieval = service
            .retrieve(
                scoped_request(
                    KnowledgeRoute::FullText,
                    KnowledgeRetrievalPolicy {
                        context_token_budget: 25,
                        ..policy()
                    },
                    multi_root_scopes(),
                ),
                &CancellationToken::new(),
            )
            .expect("multi-root retrieval");

        assert_eq!(record_ids(&retrieval), ["d1-0", "d2-0"]);
        assert_eq!(retrieval.token_count, 20);
    }

    /// One file indexed under two roots must not be allowed twice as many
    /// results as a file indexed under one.
    #[test]
    fn a_multi_root_scope_spends_one_global_per_file_limit() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            multi_root_fixture(&path),
            None,
            None,
            Some(RootCorpusFullTextIndex::new(&[
                ("root-a", &["m-a-0", "m-a-1"]),
                ("root-b", &["m-b-2", "m-b-3"]),
            ])),
        );

        let retrieval = service
            .retrieve(
                scoped_request(
                    KnowledgeRoute::FullText,
                    KnowledgeRetrievalPolicy {
                        maximum_results_per_file: 2,
                        ..policy()
                    },
                    multi_root_scopes(),
                ),
                &CancellationToken::new(),
            )
            .expect("multi-root retrieval");

        assert_eq!(record_ids(&retrieval), ["m-a-0", "m-b-2"]);
    }

    /// The same logical chunk reached through two roots is one result carrying
    /// both authorized occurrences, never two competing rows.
    #[test]
    fn a_chunk_reachable_through_two_roots_collapses_to_one_ranked_result() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            multi_root_fixture(&path),
            None,
            None,
            Some(RootCorpusFullTextIndex::new(&[
                ("root-a", &["x-0"]),
                ("root-b", &["x-0b"]),
            ])),
        );

        let retrieval = service
            .retrieve(
                scoped_request(KnowledgeRoute::FullText, policy(), multi_root_scopes()),
                &CancellationToken::new(),
            )
            .expect("multi-root retrieval");

        assert_eq!(record_ids(&retrieval), ["x-0"]);
        assert_eq!(retrieval.evidence[0].source_id, "source-x-a");
        assert_eq!(
            retrieval.evidence[0].duplicate_source_ids,
            vec!["source-x-b".to_owned()]
        );
        assert_eq!(retrieval.token_count, 10);
    }

    /// Adjacent context is expanded once, after the global ranking, using the
    /// scope that authorized the primary it belongs to.
    #[test]
    fn adjacent_context_across_roots_stays_bound_to_its_own_scope() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            multi_root_fixture(&path),
            None,
            None,
            Some(RootCorpusFullTextIndex::new(&[
                ("root-a", &["m-a-0"]),
                ("root-b", &["m-b-3"]),
            ])),
        );

        let retrieval = service
            .retrieve(
                scoped_request(
                    KnowledgeRoute::FullText,
                    KnowledgeRetrievalPolicy {
                        adjacent_chunk_radius: 1,
                        section_bounded_context: false,
                        ..policy()
                    },
                    multi_root_scopes(),
                ),
                &CancellationToken::new(),
            )
            .expect("multi-root retrieval");

        assert_eq!(record_ids(&retrieval), ["m-a-0", "m-a-1", "m-b-3", "m-b-2"]);
        assert!(!retrieval.evidence[0].adjacent);
        assert!(retrieval.evidence[1].adjacent);
        assert_eq!(retrieval.evidence[1].final_rank, 1);
        assert!(retrieval.evidence[3].adjacent);
        assert_eq!(retrieval.evidence[3].final_rank, 2);
    }

    /// A retrieval must name at least one scope and no more than the bounded
    /// maximum, so an unscoped tenant-wide search cannot be requested.
    #[test]
    fn an_absent_or_oversized_scope_set_is_rejected_before_retrieval() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec!["a-0"]])),
        );

        assert!(matches!(
            service.retrieve(
                scoped_request(KnowledgeRoute::FullText, policy(), Vec::new()),
                &CancellationToken::new(),
            ),
            Err(KnowledgeRetrievalError::InvalidScopes { .. })
        ));
        assert!(matches!(
            service.retrieve(
                scoped_request(
                    KnowledgeRoute::FullText,
                    policy(),
                    (0..=MAX_RETRIEVAL_SCOPES)
                        .map(|index| scope(root_filters(&format!("root-{index}")), &[]))
                        .collect(),
                ),
                &CancellationToken::new(),
            ),
            Err(KnowledgeRetrievalError::InvalidScopes { .. })
        ));
    }

    /// Fusion combines ranks, never partition-local scores: a candidate that
    /// two scopes return is credited once per source query and route.
    #[test]
    fn fusion_credits_a_candidate_once_per_query_and_route_across_scopes() {
        let shared = fuse_ranked_candidates(
            &[
                RankedCandidateList {
                    scope_index: 0,
                    query_index: 0,
                    route: KnowledgeRoute::FullText,
                    record_ids: vec!["a-0".into(), "b-0".into()],
                },
                RankedCandidateList {
                    scope_index: 1,
                    query_index: 0,
                    route: KnowledgeRoute::FullText,
                    record_ids: vec!["a-0".into()],
                },
            ],
            DEFAULT_RANK_CONSTANT,
        );

        let single = fuse_ranked_candidates(
            &[RankedCandidateList {
                scope_index: 0,
                query_index: 0,
                route: KnowledgeRoute::FullText,
                record_ids: vec!["a-0".into(), "b-0".into()],
            }],
            DEFAULT_RANK_CONSTANT,
        );

        assert_eq!(shared[0].record_id, "a-0");
        assert_eq!(shared[0].contributions.len(), 1);
        assert_eq!(shared[0].score, single[0].score);
        assert_eq!(shared[1].score, single[1].score);
    }

    #[test]
    fn an_unbounded_authorized_source_set_is_rejected_before_retrieval() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec!["a-0"]])),
        );
        let mut unbounded = request(KnowledgeRoute::FullText, policy());
        unbounded.scopes[0].source_restriction = KnowledgeSourceRestriction {
            allowed_source_ids: (0..=MAX_ALLOWED_SOURCES)
                .map(|index| format!("source-{index}"))
                .collect(),
        };

        let error = service
            .retrieve(unbounded, &CancellationToken::new())
            .expect_err("an unbounded restriction must be rejected");

        assert!(matches!(
            error,
            KnowledgeRetrievalError::UnboundedSourceRestriction { .. }
        ));
    }

    #[test]
    fn full_text_route_needs_no_query_embedding() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec!["a-1", "b-0"]])),
        );

        assert_eq!(
            service.capabilities(),
            KnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            }
        );
        let retrieval = service
            .retrieve(
                request(KnowledgeRoute::FullText, policy()),
                &CancellationToken::new(),
            )
            .expect("full-text retrieval");

        assert_eq!(retrieval.route.applied, KnowledgeRoute::FullText);
        assert_eq!(record_ids(&retrieval), ["a-1", "b-0"]);
    }

    #[test]
    fn hybrid_falls_back_to_full_text_when_query_embeddings_are_unavailable() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec!["a-1"]])),
        );

        let retrieval = service
            .retrieve(
                request(KnowledgeRoute::Hybrid, policy()),
                &CancellationToken::new(),
            )
            .expect("fallback retrieval");

        assert_eq!(retrieval.route.requested, KnowledgeRoute::Hybrid);
        assert_eq!(retrieval.route.applied, KnowledgeRoute::FullText);
        assert_eq!(
            retrieval.route.fallback_reason,
            Some(RouteFallbackReason::QueryEmbeddingsUnavailable)
        );
        assert_eq!(record_ids(&retrieval), ["a-1"]);
    }

    #[test]
    fn hybrid_falls_back_to_full_text_when_query_embedding_fails() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            Some(FakeEmbedder::failing(EmbeddingError::ModelIdentityMismatch)),
            Some(FakeVectorIndex::returning(vec![vec![("a-0", 0.99)]])),
            Some(FakeFullTextIndex::returning(vec![vec!["b-0"]])),
        );

        let retrieval = service
            .retrieve(
                request(KnowledgeRoute::Hybrid, policy()),
                &CancellationToken::new(),
            )
            .expect("fallback retrieval");

        assert_eq!(retrieval.route.applied, KnowledgeRoute::FullText);
        assert_eq!(
            retrieval.route.fallback_reason,
            Some(RouteFallbackReason::QueryEmbeddingFailed)
        );
        assert_eq!(record_ids(&retrieval), ["b-0"]);
    }

    #[test]
    fn hybrid_falls_back_to_semantic_when_the_full_text_index_is_absent() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            Some(FakeEmbedder::working()),
            Some(FakeVectorIndex::returning(vec![vec![("a-0", 0.99)]])),
            None,
        );

        let retrieval = service
            .retrieve(
                request(KnowledgeRoute::Hybrid, policy()),
                &CancellationToken::new(),
            )
            .expect("fallback retrieval");

        assert_eq!(retrieval.route.applied, KnowledgeRoute::Semantic);
        assert_eq!(
            retrieval.route.fallback_reason,
            Some(RouteFallbackReason::FullTextIndexUnavailable)
        );
    }

    #[test]
    fn unavailable_routes_are_typed_instead_of_silently_empty() {
        let path = catalog_path();
        let semantic_only = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec!["a-0"]])),
        );
        assert!(matches!(
            semantic_only.retrieve(
                request(KnowledgeRoute::Semantic, policy()),
                &CancellationToken::new()
            ),
            Err(KnowledgeRetrievalError::SemanticUnavailable)
        ));

        let full_text_only = KnowledgeRetrievalService::new(
            fixture(&path),
            Some(FakeEmbedder::working()),
            Some(FakeVectorIndex::returning(vec![vec![("a-0", 0.9)]])),
            None,
        );
        assert!(matches!(
            full_text_only.retrieve(
                request(KnowledgeRoute::FullText, policy()),
                &CancellationToken::new()
            ),
            Err(KnowledgeRetrievalError::FullTextUnavailable)
        ));

        let neither = KnowledgeRetrievalService::new(fixture(&path), None, None, None);
        assert!(matches!(
            neither.retrieve(
                request(KnowledgeRoute::Hybrid, policy()),
                &CancellationToken::new()
            ),
            Err(KnowledgeRetrievalError::RetrievalUnavailable)
        ));
    }

    #[test]
    fn results_respect_per_file_diversity_and_final_limits() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec![
                "a-0", "a-1", "a-2", "b-0", "b-1",
            ]])),
        );

        let retrieval = service
            .retrieve(
                request(
                    KnowledgeRoute::FullText,
                    KnowledgeRetrievalPolicy {
                        maximum_results_per_file: 2,
                        result_limit: 3,
                        ..policy()
                    },
                ),
                &CancellationToken::new(),
            )
            .expect("diverse retrieval");

        assert_eq!(record_ids(&retrieval), ["a-0", "a-1", "b-0"]);
    }

    #[test]
    fn complete_chunk_token_budget_bounds_primary_and_adjacent_context() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec![
                "a-0", "b-0", "b-1",
            ]])),
        );

        let retrieval = service
            .retrieve(
                request(
                    KnowledgeRoute::FullText,
                    KnowledgeRetrievalPolicy {
                        context_token_budget: 25,
                        ..policy()
                    },
                ),
                &CancellationToken::new(),
            )
            .expect("bounded retrieval");

        assert_eq!(record_ids(&retrieval), ["a-0", "b-0"]);
        assert_eq!(retrieval.token_count, 20);
    }

    #[test]
    fn adjacent_context_is_expanded_only_after_ranking() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec!["a-1", "b-0"]])),
        );

        let retrieval = service
            .retrieve(
                request(
                    KnowledgeRoute::FullText,
                    KnowledgeRetrievalPolicy {
                        adjacent_chunk_radius: 1,
                        ..policy()
                    },
                ),
                &CancellationToken::new(),
            )
            .expect("expanded retrieval");

        assert_eq!(record_ids(&retrieval), ["a-1", "a-0", "a-2", "b-0", "b-1"]);
        assert_eq!(
            retrieval
                .evidence
                .iter()
                .map(|evidence| (evidence.adjacent, evidence.final_rank))
                .collect::<Vec<_>>(),
            [(false, 1), (true, 1), (true, 1), (false, 2), (true, 2)]
        );
    }

    #[test]
    fn duplicate_occurrences_of_one_chunk_collapse_to_a_stable_identity() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec![
                "a-0", "b-0", "a-0-copy",
            ]])),
        );

        let retrieval = service
            .retrieve(
                request(
                    KnowledgeRoute::FullText,
                    KnowledgeRetrievalPolicy {
                        result_limit: 1,
                        ..policy()
                    },
                ),
                &CancellationToken::new(),
            )
            .expect("deduplicated retrieval");

        assert_eq!(record_ids(&retrieval), ["a-0"]);
        assert_eq!(
            retrieval.evidence[0].duplicate_source_ids,
            ["source-a-copy"]
        );
    }

    #[test]
    fn authorization_continues_in_batches_until_a_visible_result_is_found() {
        let path = catalog_path();
        let mut candidates = (0..AUTHORIZATION_BATCH_SIZE)
            .map(|index| format!("unauthorized-{index}"))
            .collect::<Vec<_>>();
        candidates.push("a-0".into());
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning_owned(vec![candidates])),
        );

        let retrieval = service
            .retrieve(
                request(KnowledgeRoute::FullText, policy()),
                &CancellationToken::new(),
            )
            .expect("batched authorization");

        assert_eq!(record_ids(&retrieval), ["a-0"]);
    }

    #[test]
    fn semantic_route_returns_vector_only_results_without_a_full_text_index() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            Some(FakeEmbedder::working()),
            Some(FakeVectorIndex::returning(vec![vec![
                ("b-0", 0.93),
                ("a-0", 0.91),
            ]])),
            Some(FakeFullTextIndex::returning(vec![vec!["a-1"]])),
        );

        let retrieval = service
            .retrieve(
                request(KnowledgeRoute::Semantic, policy()),
                &CancellationToken::new(),
            )
            .expect("semantic retrieval");

        assert_eq!(retrieval.route.applied, KnowledgeRoute::Semantic);
        assert_eq!(retrieval.route.fallback_reason, None);
        assert_eq!(record_ids(&retrieval), ["b-0", "a-0"]);
    }

    #[test]
    fn hybrid_fusion_is_stable_across_route_execution_order() {
        let lists = [
            RankedCandidateList {
                scope_index: 0,
                query_index: 0,
                route: KnowledgeRoute::Semantic,
                record_ids: vec!["a-0".into(), "b-0".into()],
            },
            RankedCandidateList {
                scope_index: 0,
                query_index: 1,
                route: KnowledgeRoute::FullText,
                record_ids: vec!["b-0".into(), "c-0".into()],
            },
        ];
        let forward = fuse_ranked_candidates(&lists, DEFAULT_RANK_CONSTANT);
        let reversed =
            fuse_ranked_candidates(&[lists[1].clone(), lists[0].clone()], DEFAULT_RANK_CONSTANT);

        assert_eq!(
            forward
                .iter()
                .map(|candidate| (candidate.record_id.as_str(), candidate.score))
                .collect::<Vec<_>>(),
            reversed
                .iter()
                .map(|candidate| (candidate.record_id.as_str(), candidate.score))
                .collect::<Vec<_>>()
        );
        assert_eq!(forward[0].record_id, "b-0");
    }

    #[test]
    fn adjacent_context_stays_inside_the_primary_section_when_requested() {
        let path = catalog_path();
        let bounded = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec!["e-1"]])),
        )
        .retrieve(
            request(
                KnowledgeRoute::FullText,
                KnowledgeRetrievalPolicy {
                    adjacent_chunk_radius: 1,
                    section_bounded_context: true,
                    ..policy()
                },
            ),
            &CancellationToken::new(),
        )
        .expect("section bounded retrieval");
        assert_eq!(record_ids(&bounded), ["e-1", "e-0"]);

        let unbounded = KnowledgeRetrievalService::new(
            SemanticCatalog::open(&path).expect("reopen catalog"),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec!["e-1"]])),
        )
        .retrieve(
            request(
                KnowledgeRoute::FullText,
                KnowledgeRetrievalPolicy {
                    adjacent_chunk_radius: 1,
                    section_bounded_context: false,
                    ..policy()
                },
            ),
            &CancellationToken::new(),
        )
        .expect("unbounded retrieval");
        assert_eq!(record_ids(&unbounded), ["e-1", "e-0", "e-2"]);
    }

    #[test]
    fn scope_filters_isolate_tenants_roots_and_workspaces() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![
                vec!["d-0", "a-0"],
                vec!["a-0", "b-0"],
                vec!["a-0", "b-0"],
            ])),
        );

        let tenant_scoped = service
            .retrieve(
                request(KnowledgeRoute::FullText, policy()),
                &CancellationToken::new(),
            )
            .expect("tenant scoped retrieval");
        assert_eq!(record_ids(&tenant_scoped), ["a-0"]);

        let mut root_scoped = request(KnowledgeRoute::FullText, policy());
        root_scoped.scopes[0].filters.root_id = Some("root-b".into());
        let root_scoped = service
            .retrieve(root_scoped, &CancellationToken::new())
            .expect("root scoped retrieval");
        assert_eq!(record_ids(&root_scoped), ["b-0"]);

        let mut workspace_scoped = request(KnowledgeRoute::FullText, policy());
        workspace_scoped.scopes[0].filters.workspace_id = Some("workspace-b".into());
        let workspace_scoped = service
            .retrieve(workspace_scoped, &CancellationToken::new())
            .expect("workspace scoped retrieval");
        assert_eq!(record_ids(&workspace_scoped), ["b-0"]);
    }

    #[test]
    fn stale_and_unavailable_sources_are_reported_rather_than_hidden() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![
                vec!["a-0", "c-0"],
                vec!["c-0"],
            ])),
        );

        let mut stale = request(KnowledgeRoute::FullText, policy());
        stale
            .current_hashes
            .insert("source-a".into(), "hash-changed".into());
        let stale = service
            .retrieve(stale, &CancellationToken::new())
            .expect("stale retrieval");
        assert_eq!(record_ids(&stale), ["a-0"]);
        assert!(stale.evidence[0].stale);
        assert!(!stale.evidence[0].unavailable);

        let mut unavailable = request(KnowledgeRoute::FullText, policy());
        unavailable.scopes[0].filters.include_unavailable = true;
        let unavailable = service
            .retrieve(unavailable, &CancellationToken::new())
            .expect("unavailable retrieval");
        assert_eq!(record_ids(&unavailable), ["c-0"]);
        assert!(unavailable.evidence[0].unavailable);
    }

    #[test]
    fn trace_explains_routing_and_ranks_without_exposing_content() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            Some(FakeEmbedder::working()),
            Some(FakeVectorIndex::returning(vec![vec![("a-0", 0.99)]])),
            Some(FakeFullTextIndex::returning(vec![vec!["b-0", "a-0"]])),
        );

        let retrieval = service
            .retrieve(
                request(
                    KnowledgeRoute::Hybrid,
                    KnowledgeRetrievalPolicy {
                        include_trace: true,
                        ..policy()
                    },
                ),
                &CancellationToken::new(),
            )
            .expect("traced retrieval");

        let trace = retrieval.trace.as_ref().expect("trace");
        assert_eq!(trace.route.applied, KnowledgeRoute::Hybrid);
        assert_eq!(trace.rank_constant, DEFAULT_RANK_CONSTANT);
        assert_eq!(trace.queries[0].text, "structured knowledge");
        assert_eq!(trace.queries[0].reason, KnowledgeRetrievalReason::Subject);
        assert_eq!(trace.entries[0].record_id, "a-0");
        assert_eq!(trace.entries[0].final_rank, 1);
        assert_eq!(
            trace.entries[0]
                .contributions
                .iter()
                .map(|contribution| (contribution.route, contribution.rank))
                .collect::<Vec<_>>(),
            [(KnowledgeRoute::Semantic, 1), (KnowledgeRoute::FullText, 2)]
        );
        assert!(matches!(
            trace.entries[0].provenance,
            fm_semantic_conversion::ChunkProvenance::Exact(_)
        ));

        let serialized = serde_json::to_string(trace).expect("serialize trace");
        assert!(!serialized.contains("complete text"));
        assert!(!serialized.contains("excerpt "));
        assert!(!serialized.contains("Section"));
    }

    #[test]
    fn cancellation_is_typed_before_and_after_candidate_retrieval() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec!["a-0"], vec!["a-0"]])),
        );

        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(matches!(
            service.retrieve(request(KnowledgeRoute::FullText, policy()), &cancelled),
            Err(KnowledgeRetrievalError::Cancelled)
        ));

        let embedding_cancelled = KnowledgeRetrievalService::new(
            fixture(&path),
            Some(FakeEmbedder::failing(EmbeddingError::Cancelled)),
            Some(FakeVectorIndex::returning(vec![vec![("a-0", 0.9)]])),
            Some(FakeFullTextIndex::returning(vec![vec!["a-0"]])),
        );
        assert!(matches!(
            embedding_cancelled.retrieve(
                request(KnowledgeRoute::Hybrid, policy()),
                &CancellationToken::new()
            ),
            Err(KnowledgeRetrievalError::Cancelled)
        ));

        let materialization_cancelled = CancellationToken::new();
        materialization_cancelled.cancel();
        let lists = [RankedCandidateList {
            scope_index: 0,
            query_index: 0,
            route: KnowledgeRoute::FullText,
            record_ids: vec!["a-0".into()],
        }];
        assert!(matches!(
            service.materialize(
                &service.catalog.begin_read().expect("read lease"),
                &fuse_ranked_candidates(&lists, DEFAULT_RANK_CONSTANT),
                &lists,
                &request(
                    KnowledgeRoute::FullText,
                    KnowledgeRetrievalPolicy {
                        adjacent_chunk_radius: 1,
                        ..policy()
                    },
                ),
                &materialization_cancelled,
            ),
            Err(KnowledgeRetrievalError::Cancelled)
        ));
    }

    #[test]
    fn retrieval_is_identical_after_a_worker_restart() {
        let path = catalog_path();
        let first = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec!["a-1", "b-0"]])),
        )
        .retrieve(
            request(
                KnowledgeRoute::FullText,
                KnowledgeRetrievalPolicy {
                    adjacent_chunk_radius: 1,
                    ..policy()
                },
            ),
            &CancellationToken::new(),
        )
        .expect("first retrieval");

        let restarted = KnowledgeRetrievalService::new(
            SemanticCatalog::open(&path).expect("reopen catalog"),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec!["b-0", "a-1"]])),
        )
        .retrieve(
            request(
                KnowledgeRoute::FullText,
                KnowledgeRetrievalPolicy {
                    adjacent_chunk_radius: 1,
                    ..policy()
                },
            ),
            &CancellationToken::new(),
        )
        .expect("restarted retrieval");

        assert_eq!(record_ids(&first), ["a-1", "a-0", "a-2", "b-0", "b-1"]);
        assert_eq!(record_ids(&restarted), ["b-0", "b-1", "a-1", "a-0", "a-2"]);
        assert_eq!(first.token_count, restarted.token_count);
    }

    #[cfg(feature = "zvec")]
    #[test]
    fn hybrid_retrieval_fuses_the_real_native_full_text_and_vector_indexes() {
        use crate::semantic_storage::VectorIndexKind;
        use crate::zvec_storage::{ZvecRecord, ZvecStorage};

        let path = catalog_path();
        let catalog = fixture(&path);
        let index_directory = tempdir().expect("temporary directory");
        let index = Arc::new(
            ZvecStorage::create(
                &index_directory.path().join("zvec"),
                3,
                VectorIndexKind::Flat,
            )
            .expect("create collection"),
        );
        let derived = |record_id: &str, content: &str, embedding: Vec<f32>| ZvecRecord {
            record_id: record_id.into(),
            tenant_id: "tenant-a".into(),
            library_id: "library-a".into(),
            root_id: "root-a".into(),
            workspace_id: Some("workspace-a".into()),
            media_type: "text/plain".into(),
            modified_at_ms: 1_000,
            concept_id: None,
            generation: 1,
            content: content.into(),
            embedding,
        };
        index
            .insert(&[
                derived("a-0", "alpha overview", vec![0.0, 0.7, 0.7]),
                derived("a-1", "beta specialist identifier", vec![0.0, 0.6, 0.8]),
                derived("b-0", "gamma unrelated", vec![1.0, 0.0, 0.0]),
                derived("b-1", "delta phosphorescence", vec![0.0, 0.0, 1.0]),
            ])
            .expect("insert derived records");
        index.flush().expect("flush");

        let service = KnowledgeRetrievalService::new(
            catalog,
            Some(FakeEmbedder::working()),
            Some(index.clone()),
            Some(index),
        );
        let mut request = request(
            KnowledgeRoute::Hybrid,
            KnowledgeRetrievalPolicy {
                include_trace: true,
                ..policy()
            },
        );
        request.queries[0].text = "phosphorescence".into();

        let retrieval = service
            .retrieve(request, &CancellationToken::new())
            .expect("native hybrid retrieval");

        assert_eq!(retrieval.route.applied, KnowledgeRoute::Hybrid);
        assert_eq!(record_ids(&retrieval), ["b-1", "a-1", "a-0", "b-0"]);
        let trace = retrieval.trace.as_ref().expect("trace");
        assert_eq!(trace.queries[0].full_text_candidates, 1);
        assert_eq!(trace.queries[0].semantic_candidates, 4);
        assert_eq!(
            trace.entries[0]
                .contributions
                .iter()
                .map(|contribution| (contribution.route, contribution.rank))
                .collect::<Vec<_>>(),
            [(KnowledgeRoute::Semantic, 3), (KnowledgeRoute::FullText, 1)]
        );
    }

    #[test]
    fn invalid_policies_and_queries_are_rejected() {
        let path = catalog_path();
        let service = KnowledgeRetrievalService::new(
            fixture(&path),
            None,
            None,
            Some(FakeFullTextIndex::returning(vec![vec!["a-0"]])),
        );

        let mut blank = request(KnowledgeRoute::FullText, policy());
        blank.queries[0].text = "   ".into();
        assert!(matches!(
            service.retrieve(blank, &CancellationToken::new()),
            Err(KnowledgeRetrievalError::EmptyQuery)
        ));

        let mut empty = request(KnowledgeRoute::FullText, policy());
        empty.queries.clear();
        assert!(matches!(
            service.retrieve(empty, &CancellationToken::new()),
            Err(KnowledgeRetrievalError::EmptyQuery)
        ));

        let mut too_many = request(KnowledgeRoute::FullText, policy());
        too_many.queries = (0..MAX_SOURCE_QUERIES + 1)
            .map(|index| KnowledgeQuery {
                text: format!("query {index}"),
                reason: KnowledgeRetrievalReason::Related,
            })
            .collect();
        assert!(matches!(
            service.retrieve(too_many, &CancellationToken::new()),
            Err(KnowledgeRetrievalError::TooManyQueries { .. })
        ));

        assert!(matches!(
            service.retrieve(
                request(
                    KnowledgeRoute::FullText,
                    KnowledgeRetrievalPolicy {
                        result_limit: 0,
                        ..policy()
                    }
                ),
                &CancellationToken::new()
            ),
            Err(KnowledgeRetrievalError::InvalidPolicy)
        ));
    }
}
