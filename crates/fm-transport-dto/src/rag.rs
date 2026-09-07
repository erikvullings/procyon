//! Transport-neutral grounded Ask requests and projections.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{DocumentSummaryTargetDto, LlmEndpointLocalityDto, LocationDto};

/// User-visible retrieval scope kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum RagScopeKindDto {
    /// Every authorized occurrence in the indexed library.
    EntireLibrary,
    /// Exact selected files.
    SelectedFiles,
    /// Current folder and descendants.
    CurrentFolder,
    /// Exact occurrences represented by a semantic result set.
    SemanticResults,
    /// Named enrolled roots.
    EnrolledRoots,
}

/// Scope selection for one grounded Ask conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RagScopeDto {
    /// Scope kind.
    pub kind: RagScopeKindDto,
    /// Workspace through which access is authorized.
    pub workspace_id: Uuid,
    /// User-visible scope label.
    pub label: String,
    /// Exact file targets for selected-file scope.
    pub selected_files: Vec<DocumentSummaryTargetDto>,
    /// Folder location for current-folder scope.
    pub folder: Option<LocationDto>,
    /// Opaque source IDs from a host-produced semantic result set.
    pub semantic_source_ids: Vec<String>,
    /// Enrolled root IDs.
    pub enrolled_root_ids: Vec<String>,
}

/// Requests inspectable local evidence without generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRagRequestDto {
    /// Exact question used for local dense retrieval.
    pub question: String,
    /// Selected saved generation profile.
    pub profile_id: Uuid,
    /// Visible authorized evidence scope.
    pub scope: RagScopeDto,
}

/// Honest coverage for an Ask scope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RagCoverageDto {
    /// Eligible source count.
    pub eligible: u64,
    /// Ready indexed source count.
    pub indexed: u64,
    /// Stale source count.
    pub stale: u64,
    /// Pending source count.
    pub pending: u64,
    /// Excluded source count.
    pub excluded: u64,
    /// Failed source count.
    pub failed: u64,
    /// Unavailable retained source count.
    pub unavailable: u64,
}

/// One inspectable local evidence excerpt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RagEvidenceDto {
    /// Opaque citation label.
    pub label: String,
    /// Opaque source occurrence identity used only for local navigation.
    pub source_id: String,
    /// Complete bounded structural excerpt.
    pub excerpt: String,
    /// Optional title permitted by profile redaction policy.
    pub title: Option<String>,
    /// Structural heading hierarchy.
    pub section_path: Vec<String>,
    /// Serialized page/line/structural provenance.
    pub provenance: String,
    /// Dense similarity.
    pub score: f32,
    /// Generated compression evidence is visually distinguished.
    pub generated: bool,
    /// Source bytes changed after indexing.
    pub stale: bool,
    /// Original source can currently open.
    pub available: bool,
}

/// Inspectable retrieval preview shown before generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RagPreviewDto {
    /// Fingerprint required to confirm this evidence set.
    pub retrieval_fingerprint: String,
    /// Visible scope.
    pub scope: RagScopeDto,
    /// Selected profile.
    pub profile_id: Uuid,
    /// User-visible profile name.
    pub profile_name: String,
    /// Local or cloud classification.
    pub locality: LlmEndpointLocalityDto,
    /// Complete selected evidence token estimate.
    pub evidence_tokens: u64,
    /// Retrieved evidence.
    pub evidence: Vec<RagEvidenceDto>,
    /// Honest requested-scope coverage.
    pub coverage: RagCoverageDto,
    /// Whether evidence cannot support a grounded answer.
    pub insufficient: bool,
}

/// Confirms generation against an inspected evidence set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRagAnswerRequestDto {
    /// Exact question used during preview.
    pub question: String,
    /// Selected saved generation profile.
    pub profile_id: Uuid,
    /// Visible authorized evidence scope.
    pub scope: RagScopeDto,
    /// Fingerprint returned by preview.
    pub expected_retrieval_fingerprint: String,
    /// Explicit opt-in to distinguishable model-only knowledge.
    pub allow_model_knowledge: bool,
    /// Server-issued ephemeral conversation to continue.
    pub conversation_id: Option<Uuid>,
}

/// One locally resolved citation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RagCitationDto {
    /// Opaque citation label.
    pub label: String,
    /// Opaque source occurrence identity used only for local navigation.
    pub source_id: String,
    /// Best available provenance.
    pub provenance: String,
    /// Original source is currently unavailable.
    pub unavailable: bool,
    /// Source changed after indexing.
    pub stale: bool,
    /// Evidence was generated compression rather than primary source text.
    pub generated: bool,
}

/// Completed grounded answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RagAnswerDto {
    /// Answer text.
    pub text: String,
    /// Citations referenced by the answer.
    pub citations: Vec<RagCitationDto>,
    /// Whether general model knowledge was permitted.
    pub model_knowledge_allowed: bool,
}

/// Bounded retrieval/generation event returned in lifecycle order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RagAnswerEventDto {
    /// Retrieval completed.
    Retrieval {
        /// Inspectable evidence.
        preview: Box<RagPreviewDto>,
    },
    /// Generated token segment.
    Token {
        /// Text appended to the answer.
        text: String,
    },
    /// Generation completed.
    Done {
        /// Completed answer.
        answer: RagAnswerDto,
    },
}

/// Result of starting one Ask generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRagAnswerResponseDto {
    /// Ephemeral conversation identity used by explicit Save.
    pub conversation_id: Uuid,
    /// Retrieval and generation events in lifecycle order.
    pub events: Vec<RagAnswerEventDto>,
}

/// Requests explicit persistence of one generated conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveRagConversationRequestDto {
    /// Ephemeral conversation returned by generation.
    pub conversation_id: Uuid,
    /// Workspace used to render saved scope metadata.
    pub workspace_id: Uuid,
}

/// Requests saved conversations visible to one workspace tenant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListSavedRagConversationsRequestDto {
    /// Workspace used to render saved scope metadata.
    pub workspace_id: Uuid,
}

/// Requests deletion of one tenant-owned saved conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRagConversationRequestDto {
    /// Saved conversation identity.
    pub conversation_id: Uuid,
}

/// Resolves one opaque citation locally after current authorization checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveRagCitationRequestDto {
    /// Workspace through which the citation is opened.
    pub workspace_id: Uuid,
    /// Opaque source identity returned by generation.
    pub source_id: String,
}

/// Current local navigation target for a citation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRagCitationDto {
    /// Current entry identity.
    pub entry_id: Uuid,
    /// Current provider-neutral location.
    pub location: LocationDto,
    /// Whether the original source can currently open.
    pub available: bool,
}

/// Saved conversation summary without duplicated source chunks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SavedRagConversationDto {
    /// Stable local conversation identity.
    pub id: Uuid,
    /// Selected profile.
    pub profile_id: Uuid,
    /// Visible scope.
    pub scope: RagScopeDto,
    /// Whether model knowledge was allowed.
    pub model_knowledge_allowed: bool,
    /// Persisted question/answer turns.
    pub turns: Vec<SavedRagTurnDto>,
    /// Approximate serialized storage use.
    pub storage_bytes: u64,
}

/// One persisted turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SavedRagTurnDto {
    /// User question.
    pub question: String,
    /// Generated answer.
    pub answer: RagAnswerDto,
}
