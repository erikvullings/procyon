//! Search-only Structured Knowledge execution.
//!
//! This capability composes the canonical [`crate::knowledge`] planner with the
//! worker's `knowledge_retrieval` capability. It never contacts an LLM: search
//! is complete and useful when answer generation is absent, and answer
//! generation is reported as an independent capability.
//!
//! Authorization stays here rather than in the worker. Scopes are resolved by
//! the host into an authorized source set before retrieval, and every returned
//! evidence row is re-checked against that set afterwards, so a worker index
//! that is stale relative to host consent can never widen visible results.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fm_semantic_worker::knowledge_retrieval::{
    KnowledgeCapabilities as WorkerKnowledgeCapabilities, KnowledgeEvidence, KnowledgeQuery,
    KnowledgeRetrieval, KnowledgeRetrievalError, KnowledgeRetrievalPolicy,
    KnowledgeRetrievalRequest, KnowledgeRetrievalScope, KnowledgeRetrievalService, KnowledgeRoute,
    KnowledgeSourceRestriction, MAX_SOURCE_QUERIES, RankContribution, RouteOutcome,
};
use fm_semantic_worker::semantic_search::SearchCoverage;
use fm_semantic_worker::semantic_storage::QueryFilters;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::knowledge::{
    KnowledgeCapabilities, KnowledgeSearchPlan, KnowledgeSearchReason, RetrievalMode,
};

/// Maximum pre-cancellations retained for searches that have not started.
const MAX_PRE_CANCELLATIONS: usize = 256;
/// How long an unmatched pre-cancellation is retained before it expires.
const PRE_CANCELLATION_TTL: Duration = Duration::from_secs(300);
/// Maximum exactly-scoped retrievals merged into one knowledge search.
pub const MAX_SCOPE_PARTITIONS: usize = 8;

/// Actionable knowledge-search failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KnowledgeSearchError {
    /// No knowledge retrieval capability is configured or reachable.
    #[error("knowledge retrieval is unavailable")]
    Unavailable,
    /// The requested route cannot be served and no fallback exists.
    #[error("{0}")]
    RouteUnavailable(String),
    /// The request violated a bounded contract.
    #[error("invalid knowledge search request: {0}")]
    InvalidRequest(String),
    /// The search was cancelled by the caller.
    #[error("knowledge search was cancelled")]
    Cancelled,
    /// Another search is already running under the same request identifier.
    #[error("a knowledge search with this request identifier is already running")]
    DuplicateRequest,
    /// Current authorization could no longer be resolved for this search.
    #[error("knowledge authorization could not be re-resolved: {0}")]
    AuthorizationUnavailable(String),
    /// Retrieval failed for a sanitized reason.
    #[error("knowledge retrieval failed: {0}")]
    RetrievalFailed(String),
}

fn map_retrieval_error(error: &KnowledgeRetrievalError) -> KnowledgeSearchError {
    match error {
        KnowledgeRetrievalError::Cancelled => KnowledgeSearchError::Cancelled,
        KnowledgeRetrievalError::EmptyQuery
        | KnowledgeRetrievalError::TooManyQueries { .. }
        | KnowledgeRetrievalError::InvalidPolicy => {
            KnowledgeSearchError::InvalidRequest(error.to_string())
        }
        KnowledgeRetrievalError::SemanticUnavailable
        | KnowledgeRetrievalError::FullTextUnavailable
        | KnowledgeRetrievalError::RetrievalUnavailable => {
            KnowledgeSearchError::RouteUnavailable(error.to_string())
        }
        _ => KnowledgeSearchError::RetrievalFailed(error.to_string()),
    }
}

/// Worker-side retrieval needed by host-owned knowledge search.
#[async_trait]
pub trait KnowledgeRetrievalCapability: Send + Sync {
    /// Reports full-text and query-embedding availability independently.
    async fn capabilities(&self) -> WorkerKnowledgeCapabilities;

    /// Retrieves fused, worker-authorized, source-oriented evidence.
    ///
    /// # Errors
    ///
    /// Returns typed capability, cancellation, or retrieval failures.
    async fn retrieve(
        &self,
        request: KnowledgeRetrievalRequest,
        cancellation: &CancellationToken,
    ) -> Result<KnowledgeRetrieval, KnowledgeSearchError>;
}

/// Inert default used until a host injects a real capability.
pub struct UnavailableKnowledgeRetrievalCapability;

#[async_trait]
impl KnowledgeRetrievalCapability for UnavailableKnowledgeRetrievalCapability {
    async fn capabilities(&self) -> WorkerKnowledgeCapabilities {
        WorkerKnowledgeCapabilities {
            full_text: false,
            query_embeddings: false,
        }
    }

    async fn retrieve(
        &self,
        _request: KnowledgeRetrievalRequest,
        _cancellation: &CancellationToken,
    ) -> Result<KnowledgeRetrieval, KnowledgeSearchError> {
        Err(KnowledgeSearchError::Unavailable)
    }
}

#[async_trait]
impl KnowledgeRetrievalCapability for KnowledgeRetrievalService {
    async fn capabilities(&self) -> WorkerKnowledgeCapabilities {
        Self::capabilities(self)
    }

    async fn retrieve(
        &self,
        request: KnowledgeRetrievalRequest,
        cancellation: &CancellationToken,
    ) -> Result<KnowledgeRetrieval, KnowledgeSearchError> {
        Self::retrieve(self, request, cancellation).map_err(|error| map_retrieval_error(&error))
    }
}

/// Knowledge retrieval over the same host-provided semantic capability used by
/// indexing and search, so browser and desktop hosts behave identically.
pub(crate) struct SemanticKnowledgeRetrievalCapability {
    semantic: crate::semantic::SemanticService,
}

impl SemanticKnowledgeRetrievalCapability {
    pub(crate) const fn new(semantic: crate::semantic::SemanticService) -> Self {
        Self { semantic }
    }
}

#[async_trait]
impl KnowledgeRetrievalCapability for SemanticKnowledgeRetrievalCapability {
    async fn capabilities(&self) -> WorkerKnowledgeCapabilities {
        self.semantic
            .knowledge_capabilities()
            .await
            .unwrap_or(WorkerKnowledgeCapabilities {
                full_text: false,
                query_embeddings: false,
            })
    }

    async fn retrieve(
        &self,
        request: KnowledgeRetrievalRequest,
        cancellation: &CancellationToken,
    ) -> Result<KnowledgeRetrieval, KnowledgeSearchError> {
        if cancellation.is_cancelled() {
            return Err(KnowledgeSearchError::Cancelled);
        }
        let request_id = crate::semantic::SemanticOperationId::new(Uuid::new_v4().to_string());
        let search = self.semantic.knowledge_search(request_id.clone(), request);
        tokio::select! {
            result = search => result.map_err(|error| map_semantic_error(&error)),
            () = cancellation.cancelled() => {
                let _ = self.semantic.cancel(request_id).await;
                Err(KnowledgeSearchError::Cancelled)
            }
        }
    }
}

fn map_semantic_error(error: &crate::semantic::SemanticError) -> KnowledgeSearchError {
    use crate::semantic::SemanticError;
    match error {
        SemanticError::Cancelled => KnowledgeSearchError::Cancelled,
        SemanticError::Unavailable
        | SemanticError::AuthenticationRejected
        | SemanticError::AuthenticationConfiguration
        | SemanticError::ClientUpdateRequired(_)
        | SemanticError::WorkerUpdateRequired(_) => KnowledgeSearchError::Unavailable,
        SemanticError::InvalidRequest(message) => {
            KnowledgeSearchError::InvalidRequest(message.clone())
        }
        _ => KnowledgeSearchError::RetrievalFailed(error.to_string()),
    }
}

/// One current snapshot of what a caller may see.
///
/// Authorization is a moving target: consent can be revoked, a root can be
/// excluded, and a source can become unavailable while a retrieval is running.
/// A snapshot therefore describes exactly one instant, and a search takes a
/// fresh one immediately before it projects evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnowledgeAuthorizationSnapshot {
    /// Exact occurrences the caller may currently see.
    pub allowed_source_ids: BTreeSet<String>,
    /// Authorized occurrences that currently cannot be opened.
    pub unavailable_source_ids: BTreeSet<String>,
    /// Current host content fingerprints keyed by opaque source identity.
    pub fingerprints: HashMap<String, String>,
}

impl KnowledgeAuthorizationSnapshot {
    /// Whether one opaque source is visible in this snapshot.
    #[must_use]
    pub fn permits(&self, source_id: &str) -> bool {
        self.allowed_source_ids.contains(source_id)
    }

    /// Whether the host currently reports this source as unavailable.
    #[must_use]
    pub fn unavailable(&self, source_id: &str) -> bool {
        self.unavailable_source_ids.contains(source_id)
    }

    /// Whether indexed evidence still matches current source truth.
    ///
    /// Returns `None` when the host has no comparable fingerprint, which is
    /// reported as unknown rather than guessed either way.
    #[must_use]
    pub fn stale(&self, source_id: &str, indexed_content_hash: &str) -> Option<bool> {
        let current = self.fingerprints.get(source_id)?;
        let indexed = indexed_content_hash.trim();
        if indexed.is_empty() {
            return None;
        }
        Some(normalize_fingerprint(current) != normalize_fingerprint(indexed))
    }
}

/// Compares digests written with or without their algorithm prefix.
fn normalize_fingerprint(value: &str) -> &str {
    value.strip_prefix("sha256:").unwrap_or(value)
}

/// Re-resolves current authorization for one running knowledge search.
///
/// This is deliberately narrow: the coordinator never learns about workspaces,
/// consent policy, or the file manager service. It only asks the host "what may
/// this caller see right now?" immediately before projecting evidence.
#[async_trait]
pub trait KnowledgeAuthorizationRefresh: Send + Sync {
    /// Re-resolves the authoritative scope for this exact search.
    ///
    /// # Errors
    ///
    /// Returns a typed failure when authorization can no longer be resolved,
    /// which must fail the search rather than fall back to a stale snapshot.
    async fn refresh(&self) -> Result<KnowledgeAuthorizationSnapshot, KnowledgeSearchError>;
}

/// Refresh capability that repeats the snapshot taken before retrieval.
///
/// Useful for tests and for hosts whose authorization cannot change during one
/// search; production hosts inject a real re-resolution instead.
pub struct StaticKnowledgeAuthorizationRefresh(pub KnowledgeAuthorizationSnapshot);

#[async_trait]
impl KnowledgeAuthorizationRefresh for StaticKnowledgeAuthorizationRefresh {
    async fn refresh(&self) -> Result<KnowledgeAuthorizationSnapshot, KnowledgeSearchError> {
        Ok(self.0.clone())
    }
}

/// One exactly-scoped retrieval issued to the worker.
///
/// A broad tenant is never truncated before the caller's real scope is applied:
/// filters describe the scope exactly where they can, and the bounded exact
/// source set describes it where they cannot.
#[derive(Debug, Clone)]
pub struct KnowledgeRetrievalPartition {
    /// Authoritative tenant/library/root/workspace filters.
    pub filters: QueryFilters,
    /// Exact authorized sources applied by the worker before its budgets.
    pub restriction: KnowledgeSourceRestriction,
}

/// Host-resolved authorization for one knowledge search.
#[derive(Debug, Clone)]
pub struct AuthorizedKnowledgeSearch {
    /// Deterministic plan produced by the canonical planner.
    pub plan: KnowledgeSearchPlan,
    /// Exactly-scoped retrievals whose results are merged before the final cut.
    pub partitions: Vec<KnowledgeRetrievalPartition>,
    /// Whether the issued partitions describe the authorized scope exactly.
    ///
    /// `false` means the worker searched a superset that the host narrowed
    /// afterwards, which is reported rather than hidden.
    pub scope_is_exact: bool,
    /// Authorization as it stood when the search was admitted.
    pub snapshot: KnowledgeAuthorizationSnapshot,
    /// Authorized sources counted by the host catalog.
    pub eligible: u64,
}

/// Ranking metadata retained for one evidence record.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KnowledgeEvidenceRanking {
    /// Route/query ranks that produced the record.
    pub contributions: Vec<RankContribution>,
    /// Deterministic reciprocal-rank-fusion score computed by the worker.
    pub fused_score: f64,
}

/// Honest coverage of one knowledge search.
///
/// The host catalog knows which sources are authorized and which are currently
/// unavailable. It does not know whether the worker has published a complete
/// generation for each of them, so publication is reported as unknown instead
/// of being derived from the eligible count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KnowledgeSearchCoverage {
    /// Authorized sources in the resolved scope.
    pub eligible: u64,
    /// Sources with a complete published generation, when the host knows it.
    pub indexed: Option<u64>,
    /// Authorized sources with a comparable current content fingerprint.
    pub fingerprinted: u64,
    /// Authorized sources that currently cannot be opened.
    pub unavailable: u64,
    /// Returned evidence rows indexed from content that has since changed.
    pub stale_evidence: u64,
    /// Returned evidence rows whose source is currently unavailable.
    pub unavailable_evidence: u64,
    /// Returned evidence rows whose freshness the host cannot determine.
    pub unknown_freshness_evidence: u64,
    /// Whether the worker searched a superset of the authorized scope.
    pub scope_is_exact: bool,
}

impl KnowledgeSearchCoverage {
    /// Whether the requested scope is *known* to be partially represented.
    ///
    /// Unknown publication state is reported through [`Self::indexed`] rather
    /// than folded in here, so this flag stays a statement about measured
    /// gaps instead of being permanently true.
    #[must_use]
    pub const fn partial(self) -> bool {
        !self.scope_is_exact
            || self.unavailable != 0
            || self.stale_evidence != 0
            || match self.indexed {
                Some(indexed) => indexed < self.eligible,
                None => false,
            }
    }
}

/// Deterministic search-only outcome.
#[derive(Debug, Clone)]
pub struct KnowledgeSearchOutcome {
    /// Requested and applied routes with any explicit fallback.
    pub route: RouteOutcome,
    /// Retrieval capabilities observed for this search.
    pub capabilities: WorkerKnowledgeCapabilities,
    /// Authorized evidence in emitted order.
    pub evidence: Vec<KnowledgeEvidence>,
    /// Worker ranking metadata keyed by evidence record identity.
    pub rankings: HashMap<String, KnowledgeEvidenceRanking>,
    /// Host-observed freshness keyed by evidence record identity.
    ///
    /// `None` means the host has no comparable fingerprint for that source.
    pub freshness: HashMap<String, Option<bool>>,
    /// Complete-chunk tokens retained after authorization.
    pub token_count: usize,
    /// Honest requested-scope coverage.
    pub coverage: KnowledgeSearchCoverage,
    /// Evidence rows withheld by host authorization after retrieval.
    pub withheld_unauthorized: u64,
    /// Fingerprint of this exact evidence set for optional later answering.
    pub evidence_fingerprint: String,
    /// Optional bounded privacy-safe trace.
    pub trace: Option<fm_semantic_worker::knowledge_retrieval::RetrievalTrace>,
}

/// Cancellation ownership of one in-flight or pre-cancelled request.
struct ActiveSearch {
    generation: u64,
    cancellation: CancellationToken,
}

/// Registry of running requests and bounded pre-cancellation tombstones.
///
/// Shared by search execution and by the optional answer capability in
/// [`crate::knowledge_answer`], so both honor the same duplicate-identifier,
/// pre-cancellation, and bounded-retention semantics.
#[derive(Default)]
pub(crate) struct CancellationRegistry {
    active: HashMap<Uuid, ActiveSearch>,
    /// Cancellations that arrived before their search registered, in arrival
    /// order so the oldest is evicted first.
    tombstones: VecDeque<(Uuid, Instant)>,
    next_generation: u64,
}

impl CancellationRegistry {
    /// Claims one request identifier, returning its cancellation ownership.
    ///
    /// A duplicate identifier is rejected rather than silently taking over the
    /// running search's cancellation.
    pub(crate) fn begin(
        &mut self,
        request_id: Uuid,
        cancellation: &CancellationToken,
    ) -> Option<u64> {
        self.expire_tombstones();
        if self.active.contains_key(&request_id) {
            return None;
        }
        if self.take_tombstone(request_id) {
            cancellation.cancel();
        }
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        self.active.insert(
            request_id,
            ActiveSearch {
                generation,
                cancellation: cancellation.clone(),
            },
        );
        Some(generation)
    }

    /// Releases one registration, ignoring a newer claim on the same id.
    pub(crate) fn finish(&mut self, request_id: Uuid, generation: u64) {
        if self
            .active
            .get(&request_id)
            .is_some_and(|active| active.generation == generation)
        {
            self.active.remove(&request_id);
        }
    }

    /// Cancels a running request, or records a bounded pre-cancellation.
    pub(crate) fn cancel(&mut self, request_id: Uuid) -> bool {
        self.expire_tombstones();
        if let Some(active) = self.active.get(&request_id) {
            active.cancellation.cancel();
            return true;
        }
        if !self.tombstones.iter().any(|(id, _)| *id == request_id) {
            self.tombstones.push_back((request_id, Instant::now()));
        }
        while self.tombstones.len() > MAX_PRE_CANCELLATIONS {
            self.tombstones.pop_front();
        }
        false
    }

    fn take_tombstone(&mut self, request_id: Uuid) -> bool {
        let Some(index) = self.tombstones.iter().position(|(id, _)| *id == request_id) else {
            return false;
        };
        self.tombstones.remove(index);
        true
    }

    /// Drops pre-cancellations whose search never arrived.
    fn expire_tombstones(&mut self) {
        let now = Instant::now();
        while self
            .tombstones
            .front()
            .is_some_and(|(_, recorded)| now.duration_since(*recorded) >= PRE_CANCELLATION_TTL)
        {
            self.tombstones.pop_front();
        }
    }
}

/// Releases a registration even when the executing future is dropped.
pub(crate) struct RegistrationGuard<'registry> {
    registry: &'registry Mutex<CancellationRegistry>,
    request_id: Uuid,
    generation: u64,
}

impl<'registry> RegistrationGuard<'registry> {
    /// Claims one request identifier for the lifetime of the returned guard.
    ///
    /// Returns `None` when the identifier is already running, which callers
    /// reject rather than taking over the running request's cancellation.
    pub(crate) fn claim(
        registry: &'registry Mutex<CancellationRegistry>,
        request_id: Uuid,
        cancellation: &CancellationToken,
    ) -> Option<Self> {
        let generation = registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .begin(request_id, cancellation)?;
        Some(Self {
            registry,
            request_id,
            generation,
        })
    }
}

impl Drop for RegistrationGuard<'_> {
    fn drop(&mut self) {
        self.registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finish(self.request_id, self.generation);
    }
}

/// Search-only knowledge capability shared by every host.
pub struct KnowledgeSearchCoordinator {
    capability: Arc<dyn KnowledgeRetrievalCapability>,
    cancellations: Mutex<CancellationRegistry>,
}

impl KnowledgeSearchCoordinator {
    /// Composes the coordinator over one retrieval capability.
    #[must_use]
    pub fn new(capability: Arc<dyn KnowledgeRetrievalCapability>) -> Self {
        Self {
            capability,
            cancellations: Mutex::new(CancellationRegistry::default()),
        }
    }

    /// Reports retrieval and answer capabilities independently.
    ///
    /// `answer_generation` is host-owned: retrieval never depends on it.
    pub async fn capabilities(&self, answer_generation: bool) -> KnowledgeCapabilities {
        KnowledgeCapabilities::from_worker(self.capability.capabilities().await, answer_generation)
    }

    /// Requests cancellation of one in-flight or not-yet-started search.
    ///
    /// Returns whether a running search was cancelled. A search that has not
    /// started yet is recorded, bounded in both count and age, so it is
    /// cancelled as soon as it registers without retaining identifiers for
    /// searches that never arrive.
    pub fn cancel(&self, request_id: Uuid) -> bool {
        self.registry().cancel(request_id)
    }

    /// Executes one authorized plan without contacting an LLM.
    ///
    /// Authorization is re-resolved through `refresh` after retrieval and
    /// before any evidence is projected, so consent revoked while the worker
    /// was running cannot disclose content.
    ///
    /// # Errors
    ///
    /// Returns typed capability, duplicate-identifier, cancellation, or
    /// retrieval failures.
    pub async fn execute(
        &self,
        request_id: Uuid,
        authorized: AuthorizedKnowledgeSearch,
        refresh: &dyn KnowledgeAuthorizationRefresh,
        cancellation: &CancellationToken,
    ) -> Result<KnowledgeSearchOutcome, KnowledgeSearchError> {
        let cancellation = cancellation.clone();
        let Some(_guard) = RegistrationGuard::claim(&self.cancellations, request_id, &cancellation)
        else {
            return Err(KnowledgeSearchError::DuplicateRequest);
        };
        self.execute_registered(&authorized, refresh, &cancellation)
            .await
    }

    fn registry(&self) -> std::sync::MutexGuard<'_, CancellationRegistry> {
        self.cancellations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    async fn execute_registered(
        &self,
        authorized: &AuthorizedKnowledgeSearch,
        refresh: &dyn KnowledgeAuthorizationRefresh,
        cancellation: &CancellationToken,
    ) -> Result<KnowledgeSearchOutcome, KnowledgeSearchError> {
        if cancellation.is_cancelled() {
            return Err(KnowledgeSearchError::Cancelled);
        }
        if authorized.plan.searches.is_empty() {
            return Err(KnowledgeSearchError::InvalidRequest(
                "knowledge plan contains no searches".into(),
            ));
        }
        if authorized.plan.searches.len() > MAX_SOURCE_QUERIES {
            return Err(KnowledgeSearchError::InvalidRequest(
                "knowledge plan exceeds the bounded source-query maximum".into(),
            ));
        }
        if authorized.snapshot.allowed_source_ids.is_empty() {
            return Err(KnowledgeSearchError::InvalidRequest(
                "knowledge scope contains no authorized indexed sources".into(),
            ));
        }
        if authorized.partitions.is_empty() {
            return Err(KnowledgeSearchError::InvalidRequest(
                "knowledge scope resolved to no searchable partition".into(),
            ));
        }
        if authorized.partitions.len() > MAX_SCOPE_PARTITIONS {
            return Err(KnowledgeSearchError::InvalidRequest(
                "knowledge scope exceeds the bounded partition maximum".into(),
            ));
        }
        // Every partition travels in one retrieval. The worker fuses their
        // candidates into a single global ranking and spends the result,
        // per-file, token, and adjacency budgets once, so a multi-root scope
        // returns what an unpartitioned search of the same scope would.
        let retrieval = self
            .capability
            .retrieve(request_for(authorized), cancellation)
            .await?;
        if cancellation.is_cancelled() {
            return Err(KnowledgeSearchError::Cancelled);
        }
        // Authorization captured before the worker call describes the past. A
        // fresh snapshot is resolved here, and evidence is intersected with
        // both, so a revocation during retrieval can only ever remove rows.
        let current = refresh.refresh().await?;
        if cancellation.is_cancelled() {
            return Err(KnowledgeSearchError::Cancelled);
        }
        Ok(authorize_retrieval(retrieval, authorized, &current))
    }
}

fn request_for(authorized: &AuthorizedKnowledgeSearch) -> KnowledgeRetrievalRequest {
    KnowledgeRetrievalRequest {
        queries: authorized
            .plan
            .searches
            .iter()
            .map(|search| KnowledgeQuery {
                text: search.text.clone(),
                reason: search.primary_retrieval_reason(),
            })
            .collect(),
        route: route_for(authorized.plan.mode),
        scopes: authorized
            .partitions
            .iter()
            .map(|partition| KnowledgeRetrievalScope {
                filters: partition.filters.clone(),
                source_restriction: partition.restriction.clone(),
            })
            .collect(),
        // Freshness is decided by the host against a snapshot taken after
        // retrieval, so no fingerprint map is sent to the worker.
        current_hashes: HashMap::new(),
        coverage: SearchCoverage {
            eligible: authorized.eligible,
            ..SearchCoverage::default()
        },
        // The trace is always retained internally: it is the only privacy-safe
        // link between an evidence row and the planned search (and therefore
        // the knowledge need) that produced it. Hosts choose separately whether
        // to project it into a response.
        policy: policy_for(authorized),
    }
}

const fn route_for(mode: RetrievalMode) -> KnowledgeRoute {
    match mode {
        RetrievalMode::Hybrid => KnowledgeRoute::Hybrid,
        RetrievalMode::FullText => KnowledgeRoute::FullText,
        RetrievalMode::Semantic => KnowledgeRoute::Semantic,
    }
}

fn policy_for(authorized: &AuthorizedKnowledgeSearch) -> KnowledgeRetrievalPolicy {
    let options = authorized.plan.options;
    KnowledgeRetrievalPolicy {
        candidate_limit: options.candidate_limit,
        result_limit: options.result_limit,
        maximum_results_per_file: options.maximum_results_per_file,
        context_token_budget: options.context_token_budget,
        adjacent_chunk_radius: options.adjacent_chunk_radius,
        section_bounded_context: options.section_bounded_context,
        include_trace: true,
        ..KnowledgeRetrievalPolicy::default_search()
    }
}

/// Applies current host authorization to worker evidence and renumbers ranks.
fn authorize_retrieval(
    retrieval: KnowledgeRetrieval,
    authorized: &AuthorizedKnowledgeSearch,
    current: &KnowledgeAuthorizationSnapshot,
) -> KnowledgeSearchOutcome {
    let rankings = retrieval.trace.as_ref().map_or_else(HashMap::new, |trace| {
        trace
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.record_id.clone(),
                    KnowledgeEvidenceRanking {
                        contributions: entry.contributions.clone(),
                        fused_score: entry.fused_score,
                    },
                )
            })
            .collect::<HashMap<_, _>>()
    });
    let mut evidence = Vec::with_capacity(retrieval.evidence.len());
    let mut freshness = HashMap::new();
    let mut withheld = 0u64;
    let mut stale_evidence = 0u64;
    let mut unavailable_evidence = 0u64;
    let mut unknown_freshness = 0u64;
    let mut dropped_ranks = HashSet::new();
    let mut rank_map = HashMap::new();
    let mut next_rank = 0usize;
    for mut row in retrieval.evidence {
        // A row must be authorized both by the snapshot the search was admitted
        // with and by the snapshot taken just now: retrieval can never widen
        // what the caller was allowed to see, and revocation takes effect
        // immediately.
        let visible =
            authorized.snapshot.permits(&row.source_id) && current.permits(&row.source_id);
        if !visible {
            if !row.adjacent {
                dropped_ranks.insert(row.final_rank);
            }
            withheld = withheld.saturating_add(1);
            continue;
        }
        if row.adjacent && dropped_ranks.contains(&row.final_rank) {
            withheld = withheld.saturating_add(1);
            continue;
        }
        row.duplicate_source_ids.retain(|source_id| {
            authorized.snapshot.permits(source_id) && current.permits(source_id)
        });
        let stale = current.stale(&row.source_id, &row.indexed_content_hash);
        row.stale = stale.unwrap_or(row.stale);
        row.unavailable = current.unavailable(&row.source_id);
        if !row.adjacent {
            match stale {
                Some(true) => stale_evidence = stale_evidence.saturating_add(1),
                Some(false) => {}
                None => unknown_freshness = unknown_freshness.saturating_add(1),
            }
            if row.unavailable {
                unavailable_evidence = unavailable_evidence.saturating_add(1);
            }
        }
        freshness.insert(row.record_id.clone(), stale);
        let rank = if row.adjacent {
            rank_map.get(&row.final_rank).copied().unwrap_or(next_rank)
        } else {
            next_rank += 1;
            rank_map.insert(row.final_rank, next_rank);
            next_rank
        };
        row.final_rank = rank;
        evidence.push(row);
    }
    let token_count = evidence
        .iter()
        .map(|row| row.token_count)
        .fold(0usize, usize::saturating_add);
    let evidence_fingerprint = evidence_fingerprint(&authorized.plan, &evidence);
    let eligible = u64::try_from(current.allowed_source_ids.len()).unwrap_or(u64::MAX);
    let coverage = KnowledgeSearchCoverage {
        eligible,
        // The host catalog records observed occurrences; whether the worker has
        // published a complete generation for each of them is worker state the
        // host cannot read here, so it stays unknown instead of being inferred.
        indexed: None,
        fingerprinted: u64::try_from(
            current
                .allowed_source_ids
                .iter()
                .filter(|source_id| current.fingerprints.contains_key(*source_id))
                .count(),
        )
        .unwrap_or(u64::MAX),
        unavailable: u64::try_from(
            current
                .allowed_source_ids
                .iter()
                .filter(|source_id| current.unavailable(source_id))
                .count(),
        )
        .unwrap_or(u64::MAX),
        stale_evidence,
        unavailable_evidence,
        unknown_freshness_evidence: unknown_freshness,
        scope_is_exact: authorized.scope_is_exact,
    };
    KnowledgeSearchOutcome {
        route: retrieval.route,
        capabilities: retrieval.capabilities,
        rankings,
        freshness,
        evidence,
        token_count,
        coverage,
        withheld_unauthorized: withheld,
        evidence_fingerprint,
        trace: retrieval.trace,
    }
}

fn evidence_fingerprint(plan: &KnowledgeSearchPlan, evidence: &[KnowledgeEvidence]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plan.version.as_bytes());
    hasher.update([0x1f]);
    for search in &plan.searches {
        hasher.update(search.text.as_bytes());
        hasher.update([0x1e]);
    }
    hasher.update([0x1d]);
    for row in evidence {
        hasher.update(row.record_id.as_bytes());
        hasher.update([0x1f]);
        hasher.update(row.source_id.as_bytes());
        hasher.update([0x1e]);
    }
    let bytes = hasher.finalize();
    let mut fingerprint = String::with_capacity(7 + bytes.len() * 2);
    fingerprint.push_str("sha256:");
    for byte in bytes {
        write!(fingerprint, "{byte:02x}").expect("writing to String cannot fail");
    }
    fingerprint
}

/// Maps every reason retained by the planned searches that produced one row.
#[must_use]
pub fn reasons_for_contributions(
    plan: &KnowledgeSearchPlan,
    contributions: &[RankContribution],
) -> Vec<KnowledgeSearchReason> {
    let mut reasons = Vec::new();
    for contribution in contributions {
        let Some(search) = plan.searches.get(contribution.query_index) else {
            continue;
        };
        for reason in &search.reasons {
            if !reasons.contains(reason) {
                reasons.push(*reason);
            }
        }
    }
    reasons
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use fm_semantic_conversion::ChunkProvenance;
    use fm_semantic_conversion::Provenance;
    use fm_semantic_worker::knowledge_retrieval::{
        KnowledgeRetrievalReason, RetrievalTrace, TracedEvidence,
    };

    use super::*;
    use crate::knowledge::{
        KnowledgeNeed, KnowledgePlanner, KnowledgeScope, KnowledgeScopeSelector,
        KnowledgeSearchOptions, KnowledgeSearchRequest, KnowledgeSubject,
    };

    struct FixedCapability {
        capabilities: WorkerKnowledgeCapabilities,
        retrieval: Mutex<Option<KnowledgeRetrieval>>,
        requests: Mutex<Vec<KnowledgeRetrievalRequest>>,
        block_until: Option<CancellationToken>,
    }

    impl FixedCapability {
        fn new(capabilities: WorkerKnowledgeCapabilities, retrieval: KnowledgeRetrieval) -> Self {
            Self {
                capabilities,
                retrieval: Mutex::new(Some(retrieval)),
                requests: Mutex::new(Vec::new()),
                block_until: None,
            }
        }

        fn blocking(mut self, released: CancellationToken) -> Self {
            self.block_until = Some(released);
            self
        }
    }

    #[async_trait]
    impl KnowledgeRetrievalCapability for FixedCapability {
        async fn capabilities(&self) -> WorkerKnowledgeCapabilities {
            self.capabilities
        }

        async fn retrieve(
            &self,
            request: KnowledgeRetrievalRequest,
            cancellation: &CancellationToken,
        ) -> Result<KnowledgeRetrieval, KnowledgeSearchError> {
            self.requests.lock().unwrap().push(request);
            if let Some(released) = &self.block_until {
                tokio::select! {
                    () = released.cancelled() => {}
                    () = cancellation.cancelled() => {
                        return Err(KnowledgeSearchError::Cancelled);
                    }
                }
            }
            if cancellation.is_cancelled() {
                return Err(KnowledgeSearchError::Cancelled);
            }
            self.retrieval
                .lock()
                .unwrap()
                .clone()
                .ok_or(KnowledgeSearchError::Unavailable)
        }
    }

    /// Authorization that narrows between retrieval and projection.
    struct NarrowingRefresh {
        snapshots: Mutex<Vec<KnowledgeAuthorizationSnapshot>>,
        calls: AtomicUsize,
    }

    impl NarrowingRefresh {
        fn new(snapshots: Vec<KnowledgeAuthorizationSnapshot>) -> Self {
            Self {
                snapshots: Mutex::new(snapshots),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl KnowledgeAuthorizationRefresh for NarrowingRefresh {
        async fn refresh(&self) -> Result<KnowledgeAuthorizationSnapshot, KnowledgeSearchError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let mut snapshots = self.snapshots.lock().unwrap();
            if snapshots.len() > 1 {
                Ok(snapshots.remove(0))
            } else {
                Ok(snapshots.first().cloned().unwrap_or_default())
            }
        }
    }

    /// Authorization that can no longer be resolved at all.
    struct FailingRefresh;

    #[async_trait]
    impl KnowledgeAuthorizationRefresh for FailingRefresh {
        async fn refresh(&self) -> Result<KnowledgeAuthorizationSnapshot, KnowledgeSearchError> {
            Err(KnowledgeSearchError::AuthorizationUnavailable(
                "library is unavailable".into(),
            ))
        }
    }

    fn evidence(
        record_id: &str,
        source_id: &str,
        adjacent: bool,
        rank: usize,
    ) -> KnowledgeEvidence {
        KnowledgeEvidence {
            record_id: record_id.to_owned(),
            occurrence_id: format!("{source_id}#occ"),
            source_id: source_id.to_owned(),
            duplicate_source_ids: Vec::new(),
            document_id: format!("{source_id}-doc"),
            library_id: "library".to_owned(),
            chunk_kind: "chunk".to_owned(),
            excerpt: "excerpt".to_owned(),
            content: "content".to_owned(),
            token_count: 4,
            section_path: vec!["Section".to_owned()],
            provenance: ChunkProvenance::Exact(Provenance::TextLines {
                start_line: 1,
                end_line: 2,
            }),
            media_type: Some("text/plain".to_owned()),
            modified_at_ms: Some(10),
            indexed_content_hash: "sha256:indexed".to_owned(),
            generation: 1,
            source_position: 0,
            generated: false,
            unavailable: false,
            stale: false,
            adjacent,
            final_rank: rank,
        }
    }

    fn traced(record_id: &str, query_index: usize) -> TracedEvidence {
        TracedEvidence {
            record_id: record_id.to_owned(),
            occurrence_id: format!("{record_id}#occ"),
            source_id: record_id.to_owned(),
            document_id: format!("{record_id}-doc"),
            contributions: vec![RankContribution {
                query_index,
                route: KnowledgeRoute::FullText,
                rank: 1,
            }],
            fused_score: 0.5,
            final_rank: 1,
            provenance: ChunkProvenance::Exact(Provenance::TextLines {
                start_line: 1,
                end_line: 2,
            }),
            adjacent: false,
            generated: false,
            unavailable: false,
            stale: false,
        }
    }

    fn plan(needs: Vec<KnowledgeNeed>) -> KnowledgeSearchPlan {
        KnowledgePlanner::plan(
            &KnowledgeSearchRequest {
                subjects: vec![KnowledgeSubject {
                    text: "wind turbines".to_owned(),
                }],
                needs,
                related_terms: Vec::new(),
                scopes: vec![KnowledgeScope {
                    tenant_id: "tenant".to_owned(),
                    library_id: "library".to_owned(),
                    selector: KnowledgeScopeSelector::WholeLibrary,
                }],
                mode: RetrievalMode::FullText,
                options: KnowledgeSearchOptions::default(),
            },
            None,
        )
        .expect("plan")
    }

    fn snapshot(allowed: &[&str]) -> KnowledgeAuthorizationSnapshot {
        KnowledgeAuthorizationSnapshot {
            allowed_source_ids: allowed.iter().map(|value| (*value).to_owned()).collect(),
            unavailable_source_ids: BTreeSet::new(),
            fingerprints: allowed
                .iter()
                .map(|value| ((*value).to_owned(), "sha256:indexed".to_owned()))
                .collect(),
        }
    }

    fn authorized(plan: KnowledgeSearchPlan, allowed: &[&str]) -> AuthorizedKnowledgeSearch {
        AuthorizedKnowledgeSearch {
            plan,
            partitions: vec![KnowledgeRetrievalPartition {
                filters: QueryFilters {
                    tenant_id: "tenant".to_owned(),
                    library_id: Some("library".to_owned()),
                    include_unavailable: true,
                    ..QueryFilters::default()
                },
                restriction: KnowledgeSourceRestriction {
                    allowed_source_ids: allowed.iter().map(|value| (*value).to_owned()).collect(),
                },
            }],
            scope_is_exact: true,
            eligible: allowed.len() as u64,
            snapshot: snapshot(allowed),
        }
    }

    fn retrieval(
        evidence: Vec<KnowledgeEvidence>,
        trace: Option<RetrievalTrace>,
    ) -> KnowledgeRetrieval {
        KnowledgeRetrieval {
            route: RouteOutcome {
                requested: KnowledgeRoute::Hybrid,
                applied: KnowledgeRoute::FullText,
                fallback_reason: Some(
                    fm_semantic_worker::knowledge_retrieval::RouteFallbackReason::QueryEmbeddingsUnavailable,
                ),
            },
            capabilities: WorkerKnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            },
            token_count: evidence.iter().map(|row| row.token_count).sum(),
            evidence,
            coverage: SearchCoverage::default(),
            trace,
        }
    }

    async fn execute(
        coordinator: &KnowledgeSearchCoordinator,
        authorized: AuthorizedKnowledgeSearch,
    ) -> Result<KnowledgeSearchOutcome, KnowledgeSearchError> {
        let refresh = StaticKnowledgeAuthorizationRefresh(authorized.snapshot.clone());
        coordinator
            .execute(
                Uuid::new_v4(),
                authorized,
                &refresh,
                &CancellationToken::new(),
            )
            .await
    }

    #[tokio::test]
    async fn search_executes_full_text_only_without_any_answer_capability() {
        let plan = plan(vec![KnowledgeNeed::Definition]);
        let capability = Arc::new(FixedCapability::new(
            WorkerKnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            },
            retrieval(
                vec![evidence("r1", "source-a", false, 1)],
                Some(RetrievalTrace {
                    route: RouteOutcome {
                        requested: KnowledgeRoute::Hybrid,
                        applied: KnowledgeRoute::FullText,
                        fallback_reason: None,
                    },
                    capabilities: WorkerKnowledgeCapabilities {
                        full_text: true,
                        query_embeddings: false,
                    },
                    rank_constant: 60,
                    queries: Vec::new(),
                    entries: vec![traced("r1", 1)],
                }),
            ),
        ));
        let coordinator = KnowledgeSearchCoordinator::new(capability.clone());

        let capabilities = coordinator.capabilities(false).await;
        assert!(capabilities.full_text);
        assert!(!capabilities.semantic);
        assert!(!capabilities.answer_generation);
        assert!(capabilities.supports(RetrievalMode::Hybrid));

        let outcome = execute(&coordinator, authorized(plan.clone(), &["source-a"]))
            .await
            .expect("full-text-only search must succeed without an LLM");

        assert_eq!(outcome.evidence.len(), 1);
        assert_eq!(outcome.route.applied, KnowledgeRoute::FullText);
        assert!(outcome.route.fallback_reason.is_some());
        assert!(!outcome.evidence_fingerprint.is_empty());
        assert_eq!(outcome.coverage.eligible, 1);
        assert_eq!(outcome.coverage.indexed, None);
        assert_eq!(outcome.coverage.fingerprinted, 1);
        assert!(!outcome.evidence[0].stale);
        let reasons = reasons_for_contributions(
            &plan,
            &outcome
                .rankings
                .get("r1")
                .expect("trace links evidence to a planned search")
                .contributions,
        );
        assert!(matches!(
            reasons.first(),
            Some(KnowledgeSearchReason::Need {
                need: KnowledgeNeed::Definition,
                ..
            })
        ));
        let issued = capability.requests.lock().unwrap();
        assert_eq!(issued.len(), 1);
        assert!(issued[0].policy.include_trace);
        assert_eq!(
            issued[0].queries[0].reason,
            KnowledgeRetrievalReason::Subject
        );
        // The exact authorized scope travels with the request, so the worker
        // applies it before it spends its result budget.
        assert_eq!(issued[0].scopes.len(), 1);
        assert!(
            issued[0].scopes[0]
                .source_restriction
                .allowed_source_ids
                .contains("source-a")
        );
    }

    #[tokio::test]
    async fn evidence_outside_the_authorized_scope_is_withheld_with_its_adjacent_context() {
        let coordinator = KnowledgeSearchCoordinator::new(Arc::new(FixedCapability::new(
            WorkerKnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            },
            retrieval(
                vec![
                    evidence("r1", "source-a", false, 1),
                    evidence("r2", "source-a", true, 1),
                    evidence("r3", "denied-source", false, 2),
                    evidence("r4", "denied-source", true, 2),
                ],
                None,
            ),
        )));

        let outcome = execute(&coordinator, authorized(plan(Vec::new()), &["source-a"]))
            .await
            .expect("authorized subset must still return");

        assert_eq!(outcome.withheld_unauthorized, 2);
        assert!(
            outcome
                .evidence
                .iter()
                .all(|row| row.source_id == "source-a")
        );
        assert_eq!(outcome.evidence[0].final_rank, 1);
        assert_eq!(outcome.evidence[1].final_rank, 1);
    }

    /// Authorization captured before retrieval describes the past. Consent
    /// revoked while the worker was running must remove evidence, and the
    /// removal must be reported rather than silently hidden.
    #[tokio::test]
    async fn authorization_revoked_during_retrieval_withholds_its_evidence() {
        let coordinator = KnowledgeSearchCoordinator::new(Arc::new(FixedCapability::new(
            WorkerKnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            },
            retrieval(
                vec![
                    evidence("r1", "source-a", false, 1),
                    evidence("r2", "source-b", false, 2),
                ],
                None,
            ),
        )));
        // The search is admitted while both sources are authorized; by the time
        // evidence is projected, only the first one still is.
        let refresh = NarrowingRefresh::new(vec![snapshot(&["source-a"])]);

        let outcome = coordinator
            .execute(
                Uuid::new_v4(),
                authorized(plan(Vec::new()), &["source-a", "source-b"]),
                &refresh,
                &CancellationToken::new(),
            )
            .await
            .expect("a narrowed scope still returns its authorized rows");

        assert_eq!(refresh.calls.load(Ordering::Relaxed), 1);
        assert_eq!(outcome.evidence.len(), 1);
        assert_eq!(outcome.evidence[0].source_id, "source-a");
        assert_eq!(outcome.withheld_unauthorized, 1);
        assert_eq!(outcome.coverage.eligible, 1);
    }

    #[tokio::test]
    async fn a_search_whose_authorization_cannot_be_re_resolved_returns_no_evidence() {
        let coordinator = KnowledgeSearchCoordinator::new(Arc::new(FixedCapability::new(
            WorkerKnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            },
            retrieval(vec![evidence("r1", "source-a", false, 1)], None),
        )));

        let error = coordinator
            .execute(
                Uuid::new_v4(),
                authorized(plan(Vec::new()), &["source-a"]),
                &FailingRefresh,
                &CancellationToken::new(),
            )
            .await
            .expect_err("unresolvable authorization must fail the search");

        assert!(matches!(
            error,
            KnowledgeSearchError::AuthorizationUnavailable(_)
        ));
    }

    /// Availability and freshness are host truth, taken fresh after retrieval,
    /// not whatever the worker index believed when it was written.
    #[tokio::test]
    async fn evidence_state_reflects_the_current_host_snapshot() {
        let coordinator = KnowledgeSearchCoordinator::new(Arc::new(FixedCapability::new(
            WorkerKnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            },
            retrieval(
                vec![
                    evidence("r1", "source-a", false, 1),
                    evidence("r2", "source-b", false, 2),
                    evidence("r3", "source-c", false, 3),
                ],
                None,
            ),
        )));
        let mut current = snapshot(&["source-a", "source-b", "source-c"]);
        // The host re-read source-a and it changed, source-b is offline, and
        // source-c has no comparable fingerprint at all.
        current
            .fingerprints
            .insert("source-a".to_owned(), "sha256:changed".to_owned());
        current.fingerprints.remove("source-c");
        current.unavailable_source_ids.insert("source-b".to_owned());
        let refresh = NarrowingRefresh::new(vec![current]);

        let outcome = coordinator
            .execute(
                Uuid::new_v4(),
                authorized(plan(Vec::new()), &["source-a", "source-b", "source-c"]),
                &refresh,
                &CancellationToken::new(),
            )
            .await
            .expect("search must report current state");

        assert!(outcome.evidence[0].stale);
        assert_eq!(outcome.freshness.get("r1"), Some(&Some(true)));
        assert!(outcome.evidence[1].unavailable);
        assert_eq!(outcome.freshness.get("r3"), Some(&None));
        assert_eq!(outcome.coverage.stale_evidence, 1);
        assert_eq!(outcome.coverage.unavailable_evidence, 1);
        assert_eq!(outcome.coverage.unknown_freshness_evidence, 1);
        assert_eq!(outcome.coverage.fingerprinted, 2);
        assert_eq!(outcome.coverage.unavailable, 1);
        // Publication is worker-owned state: it is reported as unknown rather
        // than claimed to equal the eligible count.
        assert_eq!(outcome.coverage.indexed, None);
        assert!(outcome.coverage.partial());
    }

    #[tokio::test]
    async fn a_scope_without_authorized_sources_never_reaches_the_worker() {
        let capability = Arc::new(FixedCapability::new(
            WorkerKnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            },
            retrieval(vec![evidence("r1", "source-a", false, 1)], None),
        ));
        let coordinator = KnowledgeSearchCoordinator::new(capability.clone());

        let error = execute(&coordinator, authorized(plan(Vec::new()), &[]))
            .await
            .expect_err("an empty authorized scope must be rejected");

        assert!(matches!(error, KnowledgeSearchError::InvalidRequest(_)));
        assert!(capability.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancellation_before_execution_stops_the_search() {
        let coordinator = KnowledgeSearchCoordinator::new(Arc::new(FixedCapability::new(
            WorkerKnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            },
            retrieval(vec![evidence("r1", "source-a", false, 1)], None),
        )));
        let request_id = Uuid::new_v4();
        let authorized = authorized(plan(Vec::new()), &["source-a"]);
        let refresh = StaticKnowledgeAuthorizationRefresh(authorized.snapshot.clone());

        assert!(!coordinator.cancel(request_id));
        let error = coordinator
            .execute(request_id, authorized, &refresh, &CancellationToken::new())
            .await
            .expect_err("a search cancelled before it started must not run");

        assert_eq!(error, KnowledgeSearchError::Cancelled);
    }

    /// One request identifier owns exactly one running search. A second search
    /// under the same identifier is rejected instead of quietly taking over the
    /// first one's cancellation.
    #[tokio::test]
    async fn a_duplicate_request_identifier_is_rejected_while_the_first_search_runs() {
        let released = CancellationToken::new();
        let coordinator = Arc::new(KnowledgeSearchCoordinator::new(Arc::new(
            FixedCapability::new(
                WorkerKnowledgeCapabilities {
                    full_text: true,
                    query_embeddings: false,
                },
                retrieval(vec![evidence("r1", "source-a", false, 1)], None),
            )
            .blocking(released.clone()),
        )));
        let request_id = Uuid::new_v4();
        let first_authorized = authorized(plan(Vec::new()), &["source-a"]);
        let first_snapshot = first_authorized.snapshot.clone();
        let running = {
            let coordinator = Arc::clone(&coordinator);
            tokio::spawn(async move {
                let refresh = StaticKnowledgeAuthorizationRefresh(first_snapshot);
                coordinator
                    .execute(
                        request_id,
                        first_authorized,
                        &refresh,
                        &CancellationToken::new(),
                    )
                    .await
            })
        };
        tokio::task::yield_now().await;
        while !coordinator.cancel(request_id) {
            tokio::task::yield_now().await;
        }

        let second = coordinator
            .execute(
                request_id,
                authorized(plan(Vec::new()), &["source-a"]),
                &StaticKnowledgeAuthorizationRefresh(snapshot(&["source-a"])),
                &CancellationToken::new(),
            )
            .await;
        assert_eq!(second.err(), Some(KnowledgeSearchError::DuplicateRequest));

        released.cancel();
        assert_eq!(
            running.await.expect("search task").err(),
            Some(KnowledgeSearchError::Cancelled)
        );
        // Once the first search finishes, the identifier is free again.
        assert!(
            execute(&coordinator, authorized(plan(Vec::new()), &["source-a"]))
                .await
                .is_ok()
        );
    }

    /// A dropped future must release its registration: nothing keeps a
    /// cancellation token alive for a search that no longer exists.
    #[tokio::test]
    async fn dropping_a_search_future_releases_its_request_identifier() {
        let released = CancellationToken::new();
        let coordinator = KnowledgeSearchCoordinator::new(Arc::new(
            FixedCapability::new(
                WorkerKnowledgeCapabilities {
                    full_text: true,
                    query_embeddings: false,
                },
                retrieval(vec![evidence("r1", "source-a", false, 1)], None),
            )
            .blocking(released.clone()),
        ));
        let request_id = Uuid::new_v4();
        let abandoned = authorized(plan(Vec::new()), &["source-a"]);
        let refresh = StaticKnowledgeAuthorizationRefresh(abandoned.snapshot.clone());
        let abandoned_cancellation = CancellationToken::new();
        {
            let mut search = Box::pin(coordinator.execute(
                request_id,
                abandoned,
                &refresh,
                &abandoned_cancellation,
            ));
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(50), &mut search)
                    .await
                    .is_err()
            );
            assert!(coordinator.cancel(request_id));
        }

        assert!(!coordinator.registry().active.contains_key(&request_id));
        released.cancel();
        // The identifier is reusable, and the abandoned search left no state
        // that could cancel the next one.
        let outcome = coordinator
            .execute(
                request_id,
                authorized(plan(Vec::new()), &["source-a"]),
                &StaticKnowledgeAuthorizationRefresh(snapshot(&["source-a"])),
                &CancellationToken::new(),
            )
            .await
            .expect("a released identifier must be reusable");
        assert_eq!(outcome.evidence.len(), 1);
    }

    /// Pre-cancellations are bounded in count and age, so a client that cancels
    /// searches it never starts cannot grow the registry without limit.
    #[tokio::test]
    async fn pre_cancellations_are_bounded_and_expire() {
        let coordinator = KnowledgeSearchCoordinator::new(Arc::new(FixedCapability::new(
            WorkerKnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            },
            retrieval(vec![evidence("r1", "source-a", false, 1)], None),
        )));

        for _ in 0..(MAX_PRE_CANCELLATIONS * 2) {
            assert!(!coordinator.cancel(Uuid::new_v4()));
        }
        assert_eq!(
            coordinator.registry().tombstones.len(),
            MAX_PRE_CANCELLATIONS
        );

        // An expired pre-cancellation must not cancel a later, unrelated search
        // that happens to reuse its identifier.
        let request_id = Uuid::new_v4();
        assert!(!coordinator.cancel(request_id));
        {
            let mut registry = coordinator.registry();
            let expired = Instant::now() - (PRE_CANCELLATION_TTL + Duration::from_secs(1));
            for entry in &mut registry.tombstones {
                entry.1 = expired;
            }
        }
        let outcome = execute(&coordinator, authorized(plan(Vec::new()), &["source-a"]))
            .await
            .expect("an expired pre-cancellation must not stop a new search");
        assert_eq!(outcome.evidence.len(), 1);
        assert!(coordinator.registry().tombstones.is_empty());
    }

    #[tokio::test]
    async fn an_unconfigured_capability_reports_no_routes_and_no_answers() {
        let coordinator =
            KnowledgeSearchCoordinator::new(Arc::new(UnavailableKnowledgeRetrievalCapability));

        let capabilities = coordinator.capabilities(false).await;
        assert!(!capabilities.full_text);
        assert!(!capabilities.semantic);
        assert!(!capabilities.supports(RetrievalMode::Hybrid));

        let error = execute(&coordinator, authorized(plan(Vec::new()), &["source-a"]))
            .await
            .expect_err("an unconfigured capability cannot retrieve");
        assert_eq!(error, KnowledgeSearchError::Unavailable);
    }

    /// Multi-root scopes are one logical search, not several. Every partition
    /// travels in a single retrieval so the worker can fuse their candidates
    /// into one global ranking and spend the result, per-file, token, and
    /// adjacency budgets exactly once.
    #[tokio::test]
    async fn every_partition_is_issued_as_one_globally_ranked_retrieval() {
        struct ScopeRecordingCapability {
            requests: Mutex<Vec<KnowledgeRetrievalRequest>>,
        }

        #[async_trait]
        impl KnowledgeRetrievalCapability for ScopeRecordingCapability {
            async fn capabilities(&self) -> WorkerKnowledgeCapabilities {
                WorkerKnowledgeCapabilities {
                    full_text: true,
                    query_embeddings: false,
                }
            }

            async fn retrieve(
                &self,
                request: KnowledgeRetrievalRequest,
                _cancellation: &CancellationToken,
            ) -> Result<KnowledgeRetrieval, KnowledgeSearchError> {
                self.requests.lock().unwrap().push(request);
                let mut trace = RetrievalTrace {
                    route: RouteOutcome {
                        requested: KnowledgeRoute::FullText,
                        applied: KnowledgeRoute::FullText,
                        fallback_reason: None,
                    },
                    capabilities: WorkerKnowledgeCapabilities {
                        full_text: true,
                        query_embeddings: false,
                    },
                    rank_constant: 60,
                    queries: Vec::new(),
                    entries: vec![traced("r2", 0), traced("r1", 1)],
                };
                trace.entries[0].fused_score = 0.9;
                trace.entries[0].source_id = "source-b".to_owned();
                trace.entries[1].fused_score = 0.2;
                trace.entries[1].source_id = "source-a".to_owned();
                Ok(retrieval(
                    vec![
                        evidence("r2", "source-b", false, 1),
                        evidence("r1", "source-a", false, 2),
                    ],
                    Some(trace),
                ))
            }
        }

        let capability = Arc::new(ScopeRecordingCapability {
            requests: Mutex::new(Vec::new()),
        });
        let coordinator = KnowledgeSearchCoordinator::new(capability.clone());
        let mut authorized = authorized(plan(Vec::new()), &["source-a", "source-b"]);
        authorized.partitions = ["root-a", "root-b"]
            .into_iter()
            .map(|root_id| KnowledgeRetrievalPartition {
                filters: QueryFilters {
                    tenant_id: "tenant".to_owned(),
                    library_id: Some("library".to_owned()),
                    root_id: Some(root_id.to_owned()),
                    include_unavailable: true,
                    ..QueryFilters::default()
                },
                restriction: KnowledgeSourceRestriction::default(),
            })
            .collect();
        let snapshot = authorized.snapshot.clone();

        let outcome = coordinator
            .execute(
                Uuid::new_v4(),
                authorized,
                &StaticKnowledgeAuthorizationRefresh(snapshot),
                &CancellationToken::new(),
            )
            .await
            .expect("partitioned search");

        let issued = capability.requests.lock().unwrap();
        assert_eq!(
            issued.len(),
            1,
            "partitions must not be retrieved as independent searches"
        );
        assert_eq!(issued[0].scopes.len(), 2);
        assert_eq!(
            issued[0].scopes[0].filters.root_id.as_deref(),
            Some("root-a")
        );
        assert_eq!(
            issued[0].scopes[1].filters.root_id.as_deref(),
            Some("root-b")
        );
        assert_eq!(outcome.evidence.len(), 2);
        assert_eq!(outcome.evidence[0].source_id, "source-b");
        assert_eq!(outcome.evidence[0].final_rank, 1);
        assert_eq!(outcome.evidence[1].final_rank, 2);
    }
}
