//! Transport-neutral Structured Knowledge Search requests and projections.
//!
//! Search is complete without any LLM: capabilities, roots, parsing, planning,
//! and execution never require a generation profile, and answer generation is
//! reported as an independent capability that may be absent.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{LlmEndpointLocalityDto, LocationDto};

/// Independently reported knowledge capabilities.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeCapabilitiesDto {
    /// Native full-text retrieval is available.
    pub full_text: bool,
    /// Compatible query embeddings and vector retrieval are available.
    pub semantic: bool,
    /// Optional evidence-grounded answer generation is available.
    pub answer_generation: bool,
}

/// User-selected information shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeNeedDto {
    /// Broad orientation material.
    Overview,
    /// Definitions and terminology.
    Definition,
    /// Procedures and ordered guidance.
    Procedure,
    /// Concrete examples.
    Examples,
    /// Supporting evidence.
    Evidence,
    /// Competing arguments or tradeoffs.
    Arguments,
    /// Comparative material.
    Comparison,
    /// Limitations, risks, and caveats.
    Limitations,
    /// References and source-oriented material.
    References,
}

/// Requested physical retrieval mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeRetrievalModeDto {
    /// Fuse native full-text and dense retrieval when both are available.
    #[default]
    Hybrid,
    /// Use native full-text retrieval only.
    FullText,
    /// Use dense semantic retrieval only.
    Semantic,
}

/// Physical route actually used by one retrieval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeRouteDto {
    /// Native full-text and dense vectors fused by rank.
    Hybrid,
    /// Native full-text only.
    FullText,
    /// Dense vectors only.
    Semantic,
}

/// Sanitized reason the requested route was not used verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeRouteFallbackReasonDto {
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

/// Requested and applied routes with an explicit fallback reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRouteOutcomeDto {
    /// Route the caller asked for.
    pub requested: KnowledgeRouteDto,
    /// Route actually executed.
    pub applied: KnowledgeRouteDto,
    /// Present whenever `applied` differs from `requested`.
    pub fallback_reason: Option<KnowledgeRouteFallbackReasonDto>,
}

/// Optional typed answer goal; never used as retrieval text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeActionDto {
    /// Explain the selected subject.
    Explain,
    /// Build general understanding with examples.
    Learn,
    /// Apply the subject as a procedure.
    Apply,
    /// Evaluate evidence, tradeoffs, and limitations.
    Evaluate,
    /// Compare the selected subjects.
    Compare,
    /// Produce a source-oriented answer.
    Cite,
}

/// Requested answer detail; answer-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeAnswerDepthDto {
    /// Short answer.
    Brief,
    /// Normal answer depth.
    Standard,
    /// Expanded answer.
    Detailed,
}

/// Requested answer presentation; answer-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeOutputFormatDto {
    /// Prose response.
    Narrative,
    /// Bulleted response.
    Bullets,
    /// Ordered steps.
    Steps,
    /// Tabular response.
    Table,
}

/// DSL-expressible scope selector inside the authorized library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeScopeSelectorKindDto {
    /// The complete authorized library.
    WholeLibrary,
    /// One authorized indexed root.
    Root,
    /// One authorized workspace.
    Workspace,
}

/// One DSL-expressible scope selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeScopeSelectorDto {
    /// Selector kind.
    pub kind: KnowledgeScopeSelectorKindDto,
    /// Opaque root or workspace identity; absent for the whole library.
    pub id: Option<String>,
}

/// Visible authorized search scope kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeScopeKindDto {
    /// Every occurrence authorized through the workspace.
    #[default]
    EntireLibrary,
    /// Named enrolled indexed roots.
    EnrolledRoots,
    /// The current indexed folder and its descendants.
    CurrentFolder,
    /// Exact occurrences represented by a semantic result set.
    SemanticResults,
}

/// Visible authorized search scope resolved by the application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeScopeDto {
    /// Scope kind.
    pub kind: KnowledgeScopeKindDto,
    /// Workspace through which access is authorized.
    pub workspace_id: Uuid,
    /// Enrolled root IDs for the enrolled-root scope.
    #[serde(default)]
    pub enrolled_root_ids: Vec<String>,
    /// Folder location for the current-folder scope.
    #[serde(default)]
    pub folder: Option<LocationDto>,
    /// Opaque source IDs from a host-produced semantic result set.
    #[serde(default)]
    pub semantic_source_ids: Vec<String>,
}

/// One indexed root a knowledge search may be scoped to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRootDto {
    /// Stable path-independent root identity.
    pub root_id: String,
    /// Provider-neutral location already visible in the file manager.
    pub location: LocationDto,
    /// User-visible root label.
    pub label: String,
    /// Whether descendants inherit consent.
    pub recursive: bool,
    /// Whether the indexed source is currently reachable.
    pub available: bool,
    /// Last generation committed to runtime state.
    pub indexed_generation: u64,
}

/// Requests the indexed roots authorized through one workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListKnowledgeRootsRequestDto {
    /// Workspace through which roots are authorized.
    pub workspace_id: Uuid,
}

/// Typed composer state mirroring the retrieval/answer split exactly.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeQueryDraftDto {
    /// Ordered retrieval subjects.
    #[serde(default)]
    pub about: Vec<String>,
    /// Explicit information needs.
    #[serde(default)]
    pub needs: Vec<KnowledgeNeedDto>,
    /// Explicit lower-priority related terms.
    #[serde(default)]
    pub related: Vec<String>,
    /// DSL-expressible scope selectors.
    #[serde(default)]
    pub scopes: Vec<KnowledgeScopeSelectorDto>,
    /// Answer-only typed goal; never used as retrieval text.
    #[serde(default)]
    pub action: Option<KnowledgeActionDto>,
    /// Answer-only application context; never used for retrieval.
    #[serde(default)]
    pub context: Option<String>,
    /// Answer-only constraints; never used for retrieval.
    #[serde(default)]
    pub constraints: Vec<String>,
    /// Answer-only output format.
    #[serde(default)]
    pub format: Option<KnowledgeOutputFormatDto>,
    /// Answer-only answer depth.
    #[serde(default)]
    pub depth: Option<KnowledgeAnswerDepthDto>,
}

/// Parser diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeDiagnosticSeverityDto {
    /// The affected value or field was dropped.
    Error,
    /// The value was accepted but is worth surfacing.
    Warning,
}

/// Machine-matchable parser diagnostic classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeDiagnosticCodeDto {
    /// A field key matched no canonical field or alias.
    UnknownField,
    /// A value-only field was assigned more than once.
    DuplicateField,
    /// A value was empty after trimming.
    EmptyValue,
    /// A need value was not recognized.
    InvalidNeedValue,
    /// A scope value did not match the documented grammar.
    InvalidScopeValue,
    /// An action value was not recognized.
    InvalidActionValue,
    /// An output-format value was not recognized.
    InvalidFormatValue,
    /// A repeatable field exceeded its bounded entry count.
    TooManyValues,
    /// A value exceeded its canonical byte bound.
    ValueTooLong,
    /// A quoted value has no closing quote.
    UnterminatedQuote,
    /// An answer-depth value was not recognized.
    InvalidDepthValue,
}

/// One actionable parser diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeDiagnosticDto {
    /// Diagnostic severity.
    pub severity: KnowledgeDiagnosticSeverityDto,
    /// Machine-matchable diagnostic code.
    pub code: KnowledgeDiagnosticCodeDto,
    /// Human-readable explanation.
    pub message: String,
    /// Suggested correction, when a close match was found.
    pub suggestion: Option<String>,
    /// Inclusive start byte offset into the parsed text.
    pub start: u32,
    /// Exclusive end byte offset into the parsed text.
    pub end: u32,
}

/// How the parser arrived at the draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeParseConfidenceDto {
    /// Every field came from an explicit DSL assignment.
    Explicit,
    /// One deterministic natural-language template matched.
    Deterministic,
    /// Several templates matched; alternatives are recorded.
    Ambiguous,
}

/// One recorded alternative interpretation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeParseAmbiguityDto {
    /// Human-readable explanation of the alternative reading.
    pub description: String,
    /// Alternative information need, when retrieval shape is ambiguous.
    pub alternative_need: Option<KnowledgeNeedDto>,
    /// Alternative answer action, when answer intent is ambiguous.
    pub alternative_action: Option<KnowledgeActionDto>,
}

/// Requests deterministic interpretation of DSL or natural-language text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParseKnowledgeQueryRequestDto {
    /// Raw composer or DSL text; never sent to an LLM.
    pub text: String,
}

/// Deterministic interpretation shown before execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeQueryInterpretationDto {
    /// Typed composer state.
    pub draft: KnowledgeQueryDraftDto,
    /// How the draft was derived.
    pub confidence: KnowledgeParseConfidenceDto,
    /// Alternative interpretations recorded instead of discarded.
    pub ambiguities: Vec<KnowledgeParseAmbiguityDto>,
    /// Actionable diagnostics.
    pub diagnostics: Vec<KnowledgeDiagnosticDto>,
    /// Canonical multiline DSL equivalent of the draft.
    pub dsl_multiline: String,
    /// Canonical compact DSL equivalent of the draft.
    pub dsl_compact: String,
    /// Answer-only fields present in the draft and excluded from retrieval.
    pub excluded_from_retrieval: Vec<KnowledgeExcludedFieldDto>,
}

/// One draft field explicitly not used for retrieval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeExcludedFieldDto {
    /// Canonical DSL field name (`do`, `to`, `constraint`, `format`, `depth`).
    pub field: String,
    /// Value carried by the field.
    pub value: String,
}

/// Bounded retrieval and evidence options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSearchOptionsDto {
    /// Maximum logical searches emitted by the planner.
    pub maximum_searches: u32,
    /// Candidate count requested per physical route and logical search.
    pub candidate_limit: u32,
    /// Maximum primary results in the evidence set.
    pub result_limit: u32,
    /// Maximum primary results selected from one document.
    pub maximum_results_per_file: u32,
    /// Complete-chunk token budget for primary and adjacent evidence.
    pub context_token_budget: u32,
    /// Neighboring structural chunks included on each side.
    pub adjacent_chunk_radius: u32,
    /// Whether adjacent chunks must share the primary section.
    pub section_bounded_context: bool,
    /// Whether to return the privacy-safe planning and retrieval trace.
    pub include_trace: bool,
}

/// Stable planner priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeSearchPriorityDto {
    /// Raw subject.
    Primary,
    /// Need-derived expansion.
    Secondary,
    /// Explicit related term.
    Related,
}

/// Auditable reason kind for emitting one logical search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeSearchReasonKindDto {
    /// Raw user subject.
    Subject,
    /// Explicit need expansion.
    Need,
    /// Need defaulted from a typed answer action.
    ActionDefault,
    /// Explicit related term.
    RelatedTerm,
}

/// One retained reason a logical search exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSearchReasonDto {
    /// Reason kind.
    pub kind: KnowledgeSearchReasonKindDto,
    /// Position of the expanded subject, when the reason has one.
    pub subject_index: Option<u32>,
    /// Information need used by the template, when the reason has one.
    pub need: Option<KnowledgeNeedDto>,
    /// Answer action that selected a default need, when applicable.
    pub action: Option<KnowledgeActionDto>,
    /// Position of the related term, when applicable.
    pub related_term_index: Option<u32>,
}

/// One deterministic source search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgePlannedSearchDto {
    /// Exact query text sent to retrieval.
    pub text: String,
    /// Stable planning priority.
    pub priority: KnowledgeSearchPriorityDto,
    /// All reasons retained after equivalent-query deduplication.
    pub reasons: Vec<KnowledgeSearchReasonDto>,
}

/// Deterministic inspectable plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSearchPlanDto {
    /// Planner identity.
    pub version: String,
    /// Original ordered subjects.
    pub subjects: Vec<String>,
    /// Requested retrieval mode.
    pub mode: KnowledgeRetrievalModeDto,
    /// Validated resource and evidence options.
    pub options: KnowledgeSearchOptionsDto,
    /// Stable bounded source searches.
    pub searches: Vec<KnowledgePlannedSearchDto>,
    /// Expansions omitted because of query-size or search-count bounds.
    pub omitted_searches: u32,
    /// Resolved authorized scope this plan executes against.
    ///
    /// This is the effective scope after root selectors, DSL scope selectors,
    /// and host authorization were applied, not the requested scope.
    pub scope: KnowledgeScopeDto,
    /// User-visible scope label.
    pub scope_label: String,
    /// Number of authorized indexed sources inside the scope.
    pub authorized_sources: u64,
    /// Whether retrieval ranks exactly this scope rather than a superset.
    pub scope_is_exact: bool,
    /// Answer-only fields present in the request and excluded from retrieval.
    pub excluded_from_retrieval: Vec<KnowledgeExcludedFieldDto>,
}

/// Requests a deterministic plan without executing retrieval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanKnowledgeSearchRequestDto {
    /// Typed composer state.
    pub draft: KnowledgeQueryDraftDto,
    /// Visible authorized scope.
    pub scope: KnowledgeScopeDto,
    /// Requested retrieval mode.
    #[serde(default)]
    pub mode: KnowledgeRetrievalModeDto,
    /// Optional bounded option overrides.
    #[serde(default)]
    pub options: Option<KnowledgeSearchOptionsDto>,
}

/// Requests execution of a deterministic plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteKnowledgeSearchRequestDto {
    /// Caller-owned identity used for cancellation and correlation.
    pub request_id: Uuid,
    /// Typed composer state.
    pub draft: KnowledgeQueryDraftDto,
    /// Visible authorized scope.
    pub scope: KnowledgeScopeDto,
    /// Requested retrieval mode.
    #[serde(default)]
    pub mode: KnowledgeRetrievalModeDto,
    /// Optional bounded option overrides.
    #[serde(default)]
    pub options: Option<KnowledgeSearchOptionsDto>,
}

/// Requests cancellation of one running knowledge search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelKnowledgeSearchRequestDto {
    /// Identity supplied when the search was started.
    pub request_id: Uuid,
}

/// Honest requested-scope coverage.
///
/// Every field is either measured against authoritative host state or reported
/// as unknown. Nothing here is derived from an assumption that indexing has
/// kept up with consent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeCoverageDto {
    /// Authorized sources in the resolved scope.
    pub eligible: u64,
    /// Sources with a complete published generation.
    ///
    /// `null` when publication state is worker-owned and the host cannot read
    /// it, which is reported rather than inferred from the eligible count.
    pub indexed: Option<u64>,
    /// Authorized sources with a comparable current content fingerprint.
    pub fingerprinted: u64,
    /// Authorized sources that currently cannot be opened.
    pub unavailable: u64,
    /// Returned rows indexed from content that has since changed.
    pub stale_evidence: u64,
    /// Returned rows whose source is currently unavailable.
    pub unavailable_evidence: u64,
    /// Returned rows whose freshness the host cannot determine.
    pub unknown_freshness_evidence: u64,
    /// Whether retrieval ranked exactly the authorized scope.
    pub scope_is_exact: bool,
    /// Whether the requested scope is only partially represented.
    pub partial: bool,
}

/// One route/query rank that contributed to a fused result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRankContributionDto {
    /// Index of the contributing planned search.
    pub search_index: u32,
    /// Contributing route.
    pub route: KnowledgeRouteDto,
    /// One-based rank within that route's result list.
    pub rank: u32,
}

/// One authorized source-oriented evidence row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEvidenceDto {
    /// Stable derived record identity.
    pub record_id: String,
    /// Source occurrence identity used for exact source navigation.
    pub source_id: String,
    /// Other authorized occurrences carrying this exact chunk.
    pub duplicate_source_ids: Vec<String>,
    /// Content identity shared by every occurrence of the file.
    pub document_id: String,
    /// User-visible document title.
    pub title: String,
    /// `chunk`, `summary`, or a future versioned kind.
    pub chunk_kind: String,
    /// Bounded display excerpt.
    pub excerpt: String,
    /// Complete structurally bounded chunk content.
    pub content: String,
    /// Token count charged against the context budget.
    pub token_count: u64,
    /// Structural heading hierarchy.
    pub section_path: Vec<String>,
    /// Serialized structural provenance.
    pub provenance: String,
    /// Indexed IANA media type.
    pub media_type: Option<String>,
    /// Indexed source modification time.
    pub modified_at_ms: Option<i64>,
    /// Document order.
    pub source_position: u32,
    /// Generated rather than extracted evidence.
    pub generated: bool,
    /// Source is currently unavailable, as of a fresh host snapshot.
    pub unavailable: bool,
    /// Whether current source bytes differ from the indexed generation.
    ///
    /// `null` when the host has no comparable fingerprint for the source, so
    /// freshness is honestly unknown rather than assumed current.
    pub stale: Option<bool>,
    /// Added after ranking as adjacent structural context.
    pub adjacent: bool,
    /// One-based final rank; adjacent rows inherit their primary's rank.
    pub final_rank: u32,
    /// Deterministic reciprocal-rank-fusion score.
    pub fused_score: f64,
    /// Planned searches that retrieved this row.
    pub matched_search_indexes: Vec<u32>,
    /// Reasons retained from every matched planned search.
    pub reasons: Vec<KnowledgeSearchReasonDto>,
    /// Route/query ranks that produced this row.
    pub rank_contributions: Vec<KnowledgeRankContributionDto>,
}

/// One traced planned search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeTracedQueryDto {
    /// Query text supplied by the planner.
    pub text: String,
    /// Dense candidates returned for this query.
    pub semantic_candidates: u32,
    /// Lexical candidates returned for this query.
    pub full_text_candidates: u32,
}

/// Bounded privacy-safe retrieval explanation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSearchTraceDto {
    /// Rank constant used by fusion.
    pub rank_constant: u32,
    /// Planned searches in execution order.
    pub queries: Vec<KnowledgeTracedQueryDto>,
}

/// Complete search-only result; no answer is generated or required.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSearchResultDto {
    /// Identity supplied by the caller.
    pub request_id: Uuid,
    /// Deterministic plan that produced this result.
    pub plan: KnowledgeSearchPlanDto,
    /// Requested and applied routes with any explicit fallback.
    pub route: KnowledgeRouteOutcomeDto,
    /// Capabilities observed for this search.
    pub capabilities: KnowledgeCapabilitiesDto,
    /// Honest requested-scope coverage.
    pub coverage: KnowledgeCoverageDto,
    /// Ranked primary evidence with post-rank adjacent context.
    pub evidence: Vec<KnowledgeEvidenceDto>,
    /// Complete-chunk tokens retained.
    pub token_count: u64,
    /// Fingerprint of this exact evidence set for optional later answering.
    pub evidence_fingerprint: String,
    /// Evidence rows dropped by application authorization after retrieval.
    pub withheld_unauthorized: u64,
    /// Optional bounded trace.
    pub trace: Option<KnowledgeSearchTraceDto>,
}

/// Resolves one opaque evidence source against current authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveKnowledgeSourceRequestDto {
    /// Workspace through which the source is opened.
    pub workspace_id: Uuid,
    /// Opaque source identity returned with evidence.
    pub source_id: String,
}

/// Current local navigation target for one evidence source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSourceLocationDto {
    /// Current entry identity.
    pub entry_id: Uuid,
    /// Current provider-neutral location.
    pub location: LocationDto,
    /// Whether the original source can currently open.
    pub available: bool,
}

/// Requests one optional answer from an already inspected evidence set.
///
/// Nothing in this request re-enters retrieval: the evidence set is addressed
/// by the fingerprint an earlier successful search returned, and the answer-only
/// fields shape presentation only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateKnowledgeAnswerRequestDto {
    /// Caller-owned identity used for cancellation and correlation.
    pub request_id: Uuid,
    /// Workspace through which the evidence set was authorized.
    pub workspace_id: Uuid,
    /// Fingerprint of the exact evidence set returned by the search.
    pub evidence_fingerprint: String,
    /// Explicitly selected saved generation profile.
    pub profile_id: Uuid,
    /// Explicit opt-in to distinguishable model-only knowledge.
    #[serde(default)]
    pub allow_model_knowledge: bool,
    /// Answer-only typed goal; never used as retrieval text.
    #[serde(default)]
    pub action: Option<KnowledgeActionDto>,
    /// Answer-only application context; never used for retrieval.
    #[serde(default)]
    pub context: Option<String>,
    /// Answer-only constraints; never used for retrieval.
    #[serde(default)]
    pub constraints: Vec<String>,
    /// Answer-only requested depth.
    #[serde(default)]
    pub depth: Option<KnowledgeAnswerDepthDto>,
    /// Answer-only requested presentation.
    #[serde(default)]
    pub output: Option<KnowledgeOutputFormatDto>,
}

/// Requests cancellation of one running knowledge answer generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelKnowledgeAnswerRequestDto {
    /// Identity supplied when generation was started.
    pub request_id: Uuid,
}

/// One citation resolved locally against the displayed evidence set.
///
/// Identities are the ones the search already displayed, so a citation opens
/// exactly the inspected source through the existing knowledge source
/// authority rather than through anything the model produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeAnswerCitationDto {
    /// Opaque label copied by the model.
    pub label: String,
    /// Stable derived record identity from the displayed evidence.
    pub record_id: String,
    /// Source occurrence identity used for exact source navigation.
    pub source_id: String,
    /// Serialized structural provenance of the cited evidence.
    pub provenance: String,
    /// Structural heading hierarchy of the cited evidence.
    pub section_path: Vec<String>,
    /// One-based final rank the evidence had in the displayed set.
    pub final_rank: u32,
    /// Source is currently unavailable, as of a fresh authorization snapshot.
    pub unavailable: bool,
    /// Whether current source bytes differ from the indexed generation.
    ///
    /// `null` when the host has no comparable fingerprint for the source.
    pub stale: Option<bool>,
    /// Generated rather than extracted evidence.
    pub generated: bool,
}

/// One completed optional answer over an already inspected evidence set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeAnswerDto {
    /// Identity supplied by the caller.
    pub request_id: Uuid,
    /// Fingerprint of the evidence set the answer was generated from.
    pub evidence_fingerprint: String,
    /// Profile that produced the answer.
    pub profile_id: Uuid,
    /// User-visible profile name.
    pub profile_name: String,
    /// Local or cloud endpoint classification.
    pub locality: LlmEndpointLocalityDto,
    /// Answer text, potentially including opaque citation labels.
    pub text: String,
    /// Citations actually referenced by the answer.
    pub citations: Vec<KnowledgeAnswerCitationDto>,
    /// Whether general model knowledge was permitted and labelled.
    pub model_knowledge_allowed: bool,
    /// Whether the retained evidence could not support a grounded answer.
    pub insufficient: bool,
    /// Evidence rows withheld by a fresh authorization snapshot.
    ///
    /// Only the count is reported; removed evidence is never disclosed.
    pub withheld_unauthorized: u64,
    /// Retained rows indexed from content that has since changed.
    pub stale_evidence: u64,
    /// Retained rows whose source is currently unavailable.
    pub unavailable_evidence: u64,
}
