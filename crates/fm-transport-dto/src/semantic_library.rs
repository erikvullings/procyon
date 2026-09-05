//! Stable HTTP/Tauri wire types for semantic-library enrolment (task 0179).
//!
//! These DTOs intentionally duplicate the application vocabulary. Core policy
//! and catalog types never become an accidental transport ABI.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::LocationDto;

/// Principal controlling semantic-library policy in this host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticLibraryAuthorityDto {
    /// No implementation is configured.
    Unavailable,
    /// The local desktop user manages one device-local library.
    DesktopManaged,
    /// A server administrator provisioned a private read-only library.
    AdministratorProvisioned,
    /// Deterministic in-process mock behavior.
    DeterministicMock,
}

/// Explicit operation supported by the active semantic-library capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum SemanticLibraryOperationDto {
    ViewStatus,
    ViewFolderStatus,
    PreviewEnrolment,
    Enrol,
    PlanExclusion,
    ConfirmExclusion,
    ResumeCleanup,
    Pause,
    Resume,
    UpdateEligibilityOverrides,
}

/// Semantic-library authority and supported operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticLibraryCapabilitiesDto {
    /// Active authority.
    pub authority: SemanticLibraryAuthorityDto,
    /// Explicit supported operations.
    pub operations: Vec<SemanticLibraryOperationDto>,
}

/// Effective consent at one active folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticFolderConsentDto {
    /// The folder itself is enrolled.
    IncludedHere,
    /// A recursively enrolled ancestor grants consent.
    InheritedFromParent,
    /// An explicit exclusion revokes consent.
    Excluded,
    /// No root grants consent.
    NotIncluded,
}

/// Safe active-folder consent status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticFolderStatusDto {
    /// Effective consent.
    pub consent: SemanticFolderConsentDto,
    /// Root granting or formerly granting consent.
    pub root_id: Option<String>,
    /// Most-specific exclusion, when excluded.
    pub exclusion_id: Option<String>,
    /// Whether the active workspace is attached to the granting root.
    pub workspace_referenced: bool,
    /// Whether source links can currently open.
    pub source_available: bool,
    /// Sanitized source-unavailable reason.
    pub unavailable_reason: Option<String>,
}

/// Whether a folder preview is complete, partial, or unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticEstimateCompletenessDto {
    /// Bounded enumeration completed.
    Estimated,
    /// Some entries could not be inspected or enumeration was bounded.
    Partial,
    /// No trustworthy estimate is available.
    Unavailable,
}

/// Curated reason why content is skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum SemanticEligibilityReasonDto {
    Hidden,
    System,
    ApplicationOrPackageBundle,
    DependencyDirectory,
    BuildDirectory,
    CacheDirectory,
    GitIgnored,
    UnsupportedMime,
    Oversized,
    OverBudget,
    SymlinkOutsideRoot,
    ExplicitlyExcluded,
}

/// One skip reason and its estimate/observation count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticEligibilityReasonCountDto {
    /// Stable reason.
    pub reason: SemanticEligibilityReasonDto,
    /// Number of entries.
    pub count: u64,
}

/// Hard budget apparently exceeded by an estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum SemanticBudgetKindDto {
    Documents,
    SourceBytes,
    ExtractedBytes,
    VectorBytes,
}

/// Bounded estimate shown before consent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticEnrolmentEstimateDto {
    /// Estimate completeness.
    pub completeness: SemanticEstimateCompletenessDto,
    /// Estimated eligible file count.
    pub estimated_files: Option<u64>,
    /// Estimated source bytes.
    pub estimated_source_bytes: Option<u64>,
    /// Estimated normalized excerpt bytes.
    pub estimated_extracted_bytes: Option<u64>,
    /// Estimated vector storage.
    pub estimated_vector_bytes: Option<u64>,
    /// Estimated additional local storage, including a missing model.
    pub estimated_additional_local_bytes: Option<u64>,
    /// Estimated missing model download bytes.
    pub missing_model_download_bytes: Option<u64>,
    /// Unsupported/skipped entries by fixed reason.
    pub skipped_reason_counts: Vec<SemanticEligibilityReasonCountDto>,
    /// Hard budgets the estimate appears to exceed.
    pub exceeded_budgets: Vec<SemanticBudgetKindDto>,
    /// Sanitized reason no estimate is available.
    pub unavailable_reason: Option<String>,
}

/// Confirmation-gated enrolment disclosure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticEnrolmentPreviewDto {
    /// Opaque confirmation identity.
    pub confirmation_id: String,
    /// Optimistic policy revision.
    pub policy_revision: u64,
    /// Reviewed provider-neutral location.
    pub location: LocationDto,
    /// Whether descendants will inherit consent.
    pub recursive: bool,
    /// Bounded estimate; values are never represented as exact promises.
    pub estimate: SemanticEnrolmentEstimateDto,
    /// Explicit local normalized-excerpt retention disclosure.
    pub normalized_excerpts_retained_locally: bool,
}

/// Named semantic resource profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticLibraryResourceProfileKindDto {
    /// Minimise local resource use.
    Compact,
    /// Balance resources and coverage.
    Balanced,
    /// Prioritise quality within hard ceilings.
    Quality,
}

/// Enforced semantic resource ceilings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticLibraryResourceBudgetsDto {
    /// Maximum documents.
    pub max_documents: u64,
    /// Maximum source bytes for one document.
    pub max_source_bytes_per_document: u64,
    /// Maximum cumulative source bytes.
    pub max_total_source_bytes: u64,
    /// Maximum normalized excerpt bytes.
    pub max_total_extracted_bytes: u64,
    /// Maximum vector bytes.
    pub max_total_vector_bytes: u64,
}

/// Selected resource profile and hard ceilings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticLibraryResourceProfileDto {
    /// Named profile.
    pub kind: SemanticLibraryResourceProfileKindDto,
    /// Enforced ceilings.
    pub budgets: SemanticLibraryResourceBudgetsDto,
}

/// Exact embedding identity for a library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticLibraryModelIdentityDto {
    /// Model identifier.
    pub model_id: String,
    /// Immutable upstream revision.
    pub revision: String,
    /// Embedding dimensions.
    pub dimensions: u32,
    /// Embedding-space identity.
    pub embedding_space: String,
}

/// Stable library and exact model identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticLibraryIdentityDto {
    /// Stable library UUID.
    pub library_id: String,
    /// Exact model identity.
    pub model: SemanticLibraryModelIdentityDto,
}

/// Current source-root reachability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SemanticRootAvailabilityDto {
    /// Source can be opened.
    Available,
    /// Indexed evidence remains usable but source links cannot open.
    TemporarilyUnavailable {
        /// Sanitized provider diagnostic.
        reason: String,
    },
}

/// Fixed eligibility override action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticEligibilityOverrideDto {
    /// Admit content skipped only for this overridable reason.
    Include,
    /// Explicitly retain the exclusion.
    Exclude,
}

/// One root eligibility override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticEligibilityOverrideStatusDto {
    /// Fixed curated reason.
    pub reason: SemanticEligibilityReasonDto,
    /// Include or exclude.
    pub action: SemanticEligibilityOverrideDto,
}

/// Destructive cleanup lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticCleanupStateDto {
    /// Consent is revoked but no cleanup plan is attached yet.
    Pending,
    /// Categories remain incomplete.
    Running,
    /// A category failed and cleanup is resumable.
    Failed,
    /// Every category completed.
    Complete,
}

/// Mandatory deletion category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum SemanticDeletionCategoryDto {
    Occurrences,
    ExtractedContent,
    Summaries,
    Labels,
    OrphanVectors,
    ConversationEvidencePins,
}

/// Progress for one mandatory cleanup category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticDeletionCategoryStatusDto {
    /// Stable category.
    pub category: SemanticDeletionCategoryDto,
    /// Authoritatively derived total.
    pub total_items: u64,
    /// Durably completed items.
    pub completed_items: u64,
    /// Whether this category completed.
    pub complete: bool,
    /// Sanitized last failure.
    pub last_error: Option<String>,
}

/// Complete cleanup status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticCleanupStatusDto {
    /// Opaque durable plan identity.
    pub plan_id: Option<String>,
    /// Overall state.
    pub status: SemanticCleanupStateDto,
    /// Every mandatory category.
    pub categories: Vec<SemanticDeletionCategoryStatusDto>,
}

/// One explicit descendant/root exclusion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticExclusionStatusDto {
    /// Stable exclusion identity.
    pub id: String,
    /// Provider-neutral scope.
    pub location: LocationDto,
    /// Destructive cleanup state.
    pub cleanup: SemanticCleanupStatusDto,
}

/// Safe status for one globally enrolled root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticRootStatusDto {
    /// Stable path-independent root identity.
    pub id: String,
    /// Provider-neutral location already visible to the user.
    pub location: LocationDto,
    /// Whether descendants inherit consent.
    pub recursive: bool,
    /// Whether a provider supplied stable identity.
    pub stable_identity_verified: bool,
    /// Workspaces attached to this global consent.
    pub workspace_references: Vec<Uuid>,
    /// Fixed reason-based eligibility overrides.
    pub eligibility_overrides: Vec<SemanticEligibilityOverrideStatusDto>,
    /// Persisted vocabulary identifiers.
    pub attached_vocabulary_ids: Vec<String>,
    /// Last available skip-reason counts.
    pub eligibility_reason_counts: Vec<SemanticEligibilityReasonCountDto>,
    /// Current source reachability.
    pub availability: SemanticRootAvailabilityDto,
    /// Last complete catalog reconciliation generation.
    pub reconciliation_generation: u64,
    /// Last indexed generation.
    pub indexed_generation: u64,
    /// Explicit exclusions.
    pub exclusions: Vec<SemanticExclusionStatusDto>,
}

/// Complete safe policy/catalog/state projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticLibraryStatusDto {
    /// Whether a library is configured.
    pub available: bool,
    /// Optimistic mutation revision.
    pub revision: u64,
    /// Whether ingestion is paused.
    pub paused: bool,
    /// Stable library/model identity.
    pub library: Option<SemanticLibraryIdentityDto>,
    /// Selected resource policy.
    pub resource_profile: Option<SemanticLibraryResourceProfileDto>,
    /// Reconciliation cadence.
    pub reconciliation_interval_seconds: Option<u64>,
    /// Globally enrolled roots.
    pub roots: Vec<SemanticRootStatusDto>,
    /// Explicit retention disclosure.
    pub normalized_excerpts_retained_locally: bool,
}

/// Requests consent status for the active workspace folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetSemanticFolderStatusRequestDto {
    /// Active workspace.
    pub workspace_id: Uuid,
    /// Active folder.
    pub location: LocationDto,
}

/// Requests a pre-consent enrolment disclosure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewSemanticEnrolmentRequestDto {
    /// Active workspace.
    pub workspace_id: Uuid,
    /// Active folder.
    pub location: LocationDto,
    /// Whether descendants inherit consent.
    pub recursive: bool,
}

/// Confirms one live enrolment preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmSemanticEnrolmentRequestDto {
    /// Opaque preview confirmation.
    pub confirmation_id: String,
    /// Optimistic policy revision.
    pub policy_revision: u64,
    /// Still-active workspace.
    pub workspace_id: Uuid,
    /// Still-active folder; must match the preview.
    pub location: LocationDto,
}

/// Creates an authoritative exclusion plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanSemanticExclusionRequestDto {
    /// Optimistic policy revision.
    pub policy_revision: u64,
    /// Active workspace.
    pub workspace_id: Uuid,
    /// Root or descendant to exclude.
    pub location: LocationDto,
}

/// Authoritative destructive plan awaiting confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticExclusionPlanDto {
    /// Opaque confirmation identity.
    pub confirmation_id: String,
    /// Revision the plan was derived from.
    pub policy_revision: u64,
    /// Stable owning root.
    pub root_id: String,
    /// Provider-neutral excluded scope.
    pub location: LocationDto,
    /// Every mandatory category and authoritative count.
    pub categories: Vec<SemanticDeletionCategoryStatusDto>,
}

/// Confirms one live exclusion plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmSemanticExclusionRequestDto {
    /// Opaque confirmation identity.
    pub confirmation_id: String,
    /// Optimistic policy revision.
    pub policy_revision: u64,
    /// Still-active workspace.
    pub workspace_id: Uuid,
    /// Still-active folder; must match the plan.
    pub location: LocationDto,
}

/// Optimistically guarded pause/resume request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SemanticLibraryRevisionRequestDto {
    /// Current policy revision.
    pub policy_revision: u64,
}

/// Resumes an incomplete cleanup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResumeSemanticCleanupRequestDto {
    /// Opaque durable cleanup plan.
    pub plan_id: String,
    /// Current policy revision.
    pub policy_revision: u64,
}

/// Replaces safe reason-based eligibility overrides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSemanticEligibilityOverridesRequestDto {
    /// Stable root identity.
    pub root_id: String,
    /// Attached active workspace.
    pub workspace_id: Uuid,
    /// Current policy revision.
    pub policy_revision: u64,
    /// Complete replacement override set.
    pub overrides: Vec<SemanticEligibilityOverrideStatusDto>,
}

/// Stable semantic-library error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum SemanticLibraryErrorCodeDto {
    Unavailable,
    AuthorityDenied,
    AccessDenied,
    IncompatibleLibraryIdentity,
    InvalidRequest,
    WorkspaceRequired,
    NotEnrolled,
    AlreadyExcluded,
    UnsafeEligibilityOverride,
    StaleRevision,
    StaleConfirmation,
    NotFound,
    Persistence,
    Cleanup,
    StateUnavailable,
    RevisionOverflow,
}

/// Structured semantic-library error details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SemanticLibraryErrorDetailsDto {
    /// Rejected host operation.
    AuthorityDenied {
        /// Active authority.
        authority: SemanticLibraryAuthorityDto,
        /// Rejected operation.
        operation: SemanticLibraryOperationDto,
    },
    /// Optimistic revision mismatch.
    StaleRevision {
        /// Submitted revision.
        expected: u64,
        /// Current revision.
        actual: u64,
    },
}

/// Shared HTTP/Tauri semantic-library failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticLibraryErrorDto {
    /// Stable machine-readable code.
    pub code: SemanticLibraryErrorCodeDto,
    /// Safe human-readable message.
    pub message: String,
    /// Request correlation identity.
    pub request_id: Uuid,
    /// Structured details when useful.
    pub details: Option<SemanticLibraryErrorDetailsDto>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_use_camel_case_and_reject_caller_supplied_deletion_counts() {
        let request: PreviewSemanticEnrolmentRequestDto =
            serde_json::from_value(serde_json::json!({
                "workspaceId": Uuid::nil(),
                "location": {"providerId": "local", "uri": "file:///docs"},
                "recursive": true
            }))
            .expect("camelCase request");
        assert!(request.recursive);

        let bypass =
            serde_json::from_value::<ConfirmSemanticExclusionRequestDto>(serde_json::json!({
                "confirmationId": "opaque",
                "policyRevision": 1,
                "workspaceId": Uuid::nil(),
                "location": {"providerId": "local", "uri": "file:///docs"},
                "categories": [{"category": "occurrences", "totalItems": 0}]
            }));
        assert!(bypass.is_err());
    }
}
