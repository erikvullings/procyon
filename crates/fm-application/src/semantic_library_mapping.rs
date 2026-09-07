//! Explicit semantic-library application/transport mappings.

use std::collections::BTreeMap;

use fm_domain::{Location, ProviderId, WorkspaceId};
use fm_semantic_library as core;
use fm_transport_dto::{
    ConfirmSemanticEnrolmentRequestDto, ConfirmSemanticExclusionRequestDto,
    GetSemanticFolderStatusRequestDto, PlanSemanticExclusionRequestDto,
    PreviewSemanticEnrolmentRequestDto, ResumeSemanticCleanupRequestDto, SemanticBudgetKindDto,
    SemanticCleanupStateDto, SemanticCleanupStatusDto, SemanticDeletionCategoryDto,
    SemanticDeletionCategoryStatusDto, SemanticEligibilityOverrideDto,
    SemanticEligibilityOverrideStatusDto, SemanticEligibilityReasonCountDto,
    SemanticEligibilityReasonDto, SemanticEnrolmentEstimateDto, SemanticEnrolmentPreviewDto,
    SemanticEstimateCompletenessDto, SemanticExclusionPlanDto, SemanticExclusionStatusDto,
    SemanticFolderConsentDto, SemanticFolderStatusDto, SemanticLibraryAuthorityDto,
    SemanticLibraryCapabilitiesDto, SemanticLibraryErrorCodeDto, SemanticLibraryErrorDetailsDto,
    SemanticLibraryErrorDto, SemanticLibraryIdentityDto, SemanticLibraryModelIdentityDto,
    SemanticLibraryOperationDto, SemanticLibraryResourceBudgetsDto,
    SemanticLibraryResourceProfileDto, SemanticLibraryResourceProfileKindDto,
    SemanticLibraryRevisionRequestDto, SemanticLibraryStatusDto, SemanticRootAvailabilityDto,
    SemanticRootStatusDto, UpdateSemanticEligibilityOverridesRequestDto,
};
use uuid::Uuid;

use crate::FileManagerService;
use crate::semantic_library::{
    SemanticAccessContext, SemanticBudgetKind, SemanticCleanupState, SemanticCleanupStatus,
    SemanticDeletionCategory, SemanticDeletionCategoryStatus, SemanticEligibilityOverrideStatus,
    SemanticEligibilityReasonCount, SemanticEnrolmentEstimateStatus, SemanticEnrolmentPreview,
    SemanticEstimateCompleteness, SemanticExclusionPlan, SemanticExclusionStatus,
    SemanticFolderConsent, SemanticFolderContext, SemanticFolderStatus, SemanticLibraryAuthority,
    SemanticLibraryCapabilities, SemanticLibraryError, SemanticLibraryIdentity,
    SemanticLibraryModelIdentity, SemanticLibraryOperation, SemanticLibraryStatus,
    SemanticResourceProfile, SemanticRootAvailability, SemanticRootStatus,
    parse_semantic_deletion_plan_id, parse_semantic_root_id,
};

/// Maps semantic-library authority to its wire discriminator.
#[must_use]
pub const fn semantic_library_authority_to_dto(
    authority: SemanticLibraryAuthority,
) -> SemanticLibraryAuthorityDto {
    match authority {
        SemanticLibraryAuthority::Unavailable => SemanticLibraryAuthorityDto::Unavailable,
        SemanticLibraryAuthority::DesktopManaged => SemanticLibraryAuthorityDto::DesktopManaged,
        SemanticLibraryAuthority::AdministratorProvisioned => {
            SemanticLibraryAuthorityDto::AdministratorProvisioned
        }
        SemanticLibraryAuthority::DeterministicMock => {
            SemanticLibraryAuthorityDto::DeterministicMock
        }
    }
}

/// Maps semantic-library capabilities to their wire representation.
#[must_use]
pub fn semantic_library_capabilities_to_dto(
    capabilities: SemanticLibraryCapabilities,
) -> SemanticLibraryCapabilitiesDto {
    SemanticLibraryCapabilitiesDto {
        authority: semantic_library_authority_to_dto(capabilities.authority()),
        operations: capabilities
            .operations()
            .iter()
            .copied()
            .map(semantic_library_operation_to_dto)
            .collect(),
    }
}

/// Maps semantic-library status to its safe wire projection.
#[must_use]
pub fn semantic_library_status_to_dto(status: SemanticLibraryStatus) -> SemanticLibraryStatusDto {
    SemanticLibraryStatusDto {
        available: status.available,
        revision: status.revision,
        paused: status.paused,
        library: status.library.map(semantic_library_identity_to_dto),
        resource_profile: status
            .resource_profile
            .map(semantic_resource_profile_to_dto),
        reconciliation_interval_seconds: status.reconciliation_interval_seconds,
        roots: status
            .roots
            .into_iter()
            .map(semantic_root_status_to_dto)
            .collect(),
        normalized_excerpts_retained_locally: status.normalized_excerpts_retained_locally,
    }
}

/// Maps an application failure to the shared HTTP/Tauri shape.
#[must_use]
pub fn semantic_library_error_to_dto(
    error: SemanticLibraryError,
    request_id: Uuid,
) -> SemanticLibraryErrorDto {
    let message = error.to_string();
    let (code, details) = match error {
        SemanticLibraryError::Unavailable => (SemanticLibraryErrorCodeDto::Unavailable, None),
        SemanticLibraryError::AuthorityDenied {
            authority,
            operation,
        } => (
            SemanticLibraryErrorCodeDto::AuthorityDenied,
            Some(SemanticLibraryErrorDetailsDto::AuthorityDenied {
                authority: semantic_library_authority_to_dto(authority),
                operation: semantic_library_operation_to_dto(operation),
            }),
        ),
        SemanticLibraryError::AccessDenied => (SemanticLibraryErrorCodeDto::AccessDenied, None),
        SemanticLibraryError::IncompatibleLibraryIdentity => (
            SemanticLibraryErrorCodeDto::IncompatibleLibraryIdentity,
            None,
        ),
        SemanticLibraryError::InvalidRequest => (SemanticLibraryErrorCodeDto::InvalidRequest, None),
        SemanticLibraryError::WorkspaceRequired => {
            (SemanticLibraryErrorCodeDto::WorkspaceRequired, None)
        }
        SemanticLibraryError::NotEnrolled => (SemanticLibraryErrorCodeDto::NotEnrolled, None),
        SemanticLibraryError::AlreadyExcluded => {
            (SemanticLibraryErrorCodeDto::AlreadyExcluded, None)
        }
        SemanticLibraryError::UnsafeEligibilityOverride => {
            (SemanticLibraryErrorCodeDto::UnsafeEligibilityOverride, None)
        }
        SemanticLibraryError::StaleRevision { expected, actual } => (
            SemanticLibraryErrorCodeDto::StaleRevision,
            Some(SemanticLibraryErrorDetailsDto::StaleRevision { expected, actual }),
        ),
        SemanticLibraryError::StaleConfirmation => {
            (SemanticLibraryErrorCodeDto::StaleConfirmation, None)
        }
        SemanticLibraryError::NotFound => (SemanticLibraryErrorCodeDto::NotFound, None),
        SemanticLibraryError::Persistence => (SemanticLibraryErrorCodeDto::Persistence, None),
        SemanticLibraryError::Cleanup => (SemanticLibraryErrorCodeDto::Cleanup, None),
        SemanticLibraryError::StateUnavailable => {
            (SemanticLibraryErrorCodeDto::StateUnavailable, None)
        }
        SemanticLibraryError::RevisionOverflow => {
            (SemanticLibraryErrorCodeDto::RevisionOverflow, None)
        }
    };
    SemanticLibraryErrorDto {
        code,
        message,
        request_id,
        details,
    }
}

impl FileManagerService {
    /// Returns semantic-library capabilities as a wire DTO.
    pub async fn semantic_library_capabilities_dto(
        &self,
        access: &SemanticAccessContext,
    ) -> SemanticLibraryCapabilitiesDto {
        semantic_library_capabilities_to_dto(self.semantic_library_capabilities(access).await)
    }

    /// Returns semantic-library status as a wire DTO.
    ///
    /// # Errors
    ///
    /// Returns an authorization, lock, or persistence failure.
    pub async fn semantic_library_status_dto(
        &self,
        access: &SemanticAccessContext,
    ) -> Result<SemanticLibraryStatusDto, SemanticLibraryError> {
        self.semantic_library_status(access)
            .await
            .map(semantic_library_status_to_dto)
    }

    /// Returns active-folder consent as a wire DTO.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, workspace, or persistence failure.
    pub async fn semantic_folder_status_dto(
        &self,
        access: &SemanticAccessContext,
        request: GetSemanticFolderStatusRequestDto,
    ) -> Result<SemanticFolderStatusDto, SemanticLibraryError> {
        let context = semantic_folder_context(request.workspace_id, request.location)?;
        self.semantic_library_folder_status(access, context)
            .await
            .map(semantic_folder_status_to_dto)
    }

    /// Creates an enrolment disclosure from a wire request.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, workspace, or persistence failure.
    pub async fn preview_semantic_enrolment(
        &self,
        access: &SemanticAccessContext,
        request: PreviewSemanticEnrolmentRequestDto,
    ) -> Result<SemanticEnrolmentPreviewDto, SemanticLibraryError> {
        self.ensure_semantic_library_operation(access, SemanticLibraryOperation::PreviewEnrolment)
            .await?;
        let context = semantic_folder_context(request.workspace_id, request.location)?;
        self.semantic_library_preview_enrolment(access, context, request.recursive)
            .await
            .map(semantic_enrolment_preview_to_dto)
    }

    /// Confirms one live enrolment disclosure.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, workspace, stale-revision,
    /// stale-confirmation, or persistence failure.
    pub async fn confirm_semantic_enrolment(
        &self,
        access: &SemanticAccessContext,
        request: ConfirmSemanticEnrolmentRequestDto,
    ) -> Result<SemanticLibraryStatusDto, SemanticLibraryError> {
        self.ensure_semantic_library_operation(access, SemanticLibraryOperation::Enrol)
            .await?;
        let context = semantic_folder_context(request.workspace_id, request.location)?;
        self.semantic_library_confirm_enrolment(
            access,
            &request.confirmation_id,
            request.policy_revision,
            context,
        )
        .await
        .map(semantic_library_status_to_dto)
    }

    /// Creates an authoritative exclusion plan.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, workspace, consent-state, or
    /// persistence failure.
    pub async fn plan_semantic_exclusion(
        &self,
        access: &SemanticAccessContext,
        request: PlanSemanticExclusionRequestDto,
    ) -> Result<SemanticExclusionPlanDto, SemanticLibraryError> {
        self.ensure_semantic_library_operation(access, SemanticLibraryOperation::PlanExclusion)
            .await?;
        let context = semantic_folder_context(request.workspace_id, request.location)?;
        self.semantic_library_plan_exclusion(access, context, request.policy_revision)
            .await
            .map(semantic_exclusion_plan_to_dto)
    }

    /// Confirms one live exclusion plan.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, workspace, stale-revision,
    /// stale-confirmation, or persistence failure.
    pub async fn confirm_semantic_exclusion(
        &self,
        access: &SemanticAccessContext,
        request: ConfirmSemanticExclusionRequestDto,
    ) -> Result<SemanticLibraryStatusDto, SemanticLibraryError> {
        self.ensure_semantic_library_operation(access, SemanticLibraryOperation::ConfirmExclusion)
            .await?;
        let context = semantic_folder_context(request.workspace_id, request.location)?;
        self.semantic_library_confirm_exclusion(
            access,
            &request.confirmation_id,
            request.policy_revision,
            context,
        )
        .await
        .map(semantic_library_status_to_dto)
    }

    /// Resumes a durable cleanup plan.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, not-found, stale-revision, or
    /// persistence failure.
    pub async fn resume_semantic_cleanup(
        &self,
        access: &SemanticAccessContext,
        request: ResumeSemanticCleanupRequestDto,
    ) -> Result<SemanticLibraryStatusDto, SemanticLibraryError> {
        self.semantic_library_resume_cleanup(
            access,
            parse_semantic_deletion_plan_id(&request.plan_id)?,
            request.policy_revision,
        )
        .await
        .map(semantic_library_status_to_dto)
    }

    /// Pauses ingestion without revoking consent.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, stale-revision, or persistence
    /// failure.
    pub async fn pause_semantic_library(
        &self,
        access: &SemanticAccessContext,
        request: SemanticLibraryRevisionRequestDto,
    ) -> Result<SemanticLibraryStatusDto, SemanticLibraryError> {
        self.semantic_library_pause(access, request.policy_revision)
            .await
            .map(semantic_library_status_to_dto)
    }

    /// Resumes ingestion.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, stale-revision, or persistence
    /// failure.
    pub async fn resume_semantic_library(
        &self,
        access: &SemanticAccessContext,
        request: SemanticLibraryRevisionRequestDto,
    ) -> Result<SemanticLibraryStatusDto, SemanticLibraryError> {
        self.semantic_library_resume(access, request.policy_revision)
            .await
            .map(semantic_library_status_to_dto)
    }

    /// Replaces fixed reason-based root overrides.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, workspace, unsafe-override,
    /// not-found, stale-revision, or persistence failure.
    pub async fn update_semantic_eligibility_overrides(
        &self,
        access: &SemanticAccessContext,
        request: UpdateSemanticEligibilityOverridesRequestDto,
    ) -> Result<SemanticLibraryStatusDto, SemanticLibraryError> {
        self.ensure_semantic_library_operation(
            access,
            SemanticLibraryOperation::UpdateEligibilityOverrides,
        )
        .await?;
        let mut overrides = BTreeMap::new();
        for override_status in request.overrides {
            let previous = overrides.insert(
                semantic_eligibility_reason_from_dto(override_status.reason),
                semantic_eligibility_override_from_dto(override_status.action),
            );
            if previous.is_some() {
                return Err(SemanticLibraryError::InvalidRequest);
            }
        }
        self.semantic_library_update_eligibility_overrides(
            access,
            parse_semantic_root_id(&request.root_id)?,
            WorkspaceId::from(request.workspace_id),
            request.policy_revision,
            overrides,
        )
        .await
        .map(semantic_library_status_to_dto)
    }
}

fn semantic_folder_context(
    workspace_id: Uuid,
    location: fm_transport_dto::LocationDto,
) -> Result<SemanticFolderContext, SemanticLibraryError> {
    let location = Location::try_new(ProviderId::new(location.provider_id), location.uri)
        .map_err(|_| SemanticLibraryError::InvalidRequest)?;
    Ok(SemanticFolderContext::new(
        WorkspaceId::from(workspace_id),
        location,
    ))
}

fn semantic_library_identity_to_dto(
    identity: SemanticLibraryIdentity,
) -> SemanticLibraryIdentityDto {
    SemanticLibraryIdentityDto {
        library_id: identity.library_id,
        model: semantic_library_model_identity_to_dto(identity.model),
    }
}

fn semantic_library_model_identity_to_dto(
    identity: SemanticLibraryModelIdentity,
) -> SemanticLibraryModelIdentityDto {
    SemanticLibraryModelIdentityDto {
        model_id: identity.model_id,
        revision: identity.revision,
        dimensions: identity.dimensions,
        embedding_space: identity.embedding_space,
    }
}

fn semantic_resource_profile_to_dto(
    profile: SemanticResourceProfile,
) -> SemanticLibraryResourceProfileDto {
    SemanticLibraryResourceProfileDto {
        kind: match profile.kind {
            core::ResourceProfileKind::Compact => SemanticLibraryResourceProfileKindDto::Compact,
            core::ResourceProfileKind::Balanced => SemanticLibraryResourceProfileKindDto::Balanced,
            core::ResourceProfileKind::Quality => SemanticLibraryResourceProfileKindDto::Quality,
        },
        budgets: SemanticLibraryResourceBudgetsDto {
            max_documents: profile.budgets.max_documents,
            max_source_bytes_per_document: profile.budgets.max_source_bytes_per_document,
            max_total_source_bytes: profile.budgets.max_total_source_bytes,
            max_total_extracted_bytes: profile.budgets.max_total_extracted_bytes,
            max_total_vector_bytes: profile.budgets.max_total_vector_bytes,
        },
    }
}

fn semantic_root_status_to_dto(root: SemanticRootStatus) -> SemanticRootStatusDto {
    SemanticRootStatusDto {
        id: root.id,
        location: root.location.into(),
        recursive: root.recursive,
        stable_identity_verified: root.stable_identity_verified,
        workspace_references: root.workspace_references,
        eligibility_overrides: root
            .eligibility_overrides
            .into_iter()
            .map(semantic_eligibility_override_status_to_dto)
            .collect(),
        attached_vocabulary_ids: root.attached_vocabulary_ids,
        eligibility_reason_counts: root
            .eligibility_reason_counts
            .into_iter()
            .map(semantic_eligibility_reason_count_to_dto)
            .collect(),
        ocr_required_files: root
            .ocr_required_files
            .into_iter()
            .map(Into::into)
            .collect(),
        availability: match root.availability {
            SemanticRootAvailability::Available => SemanticRootAvailabilityDto::Available,
            SemanticRootAvailability::TemporarilyUnavailable { reason } => {
                SemanticRootAvailabilityDto::TemporarilyUnavailable { reason }
            }
        },
        reconciliation_generation: root.reconciliation_generation,
        indexed_generation: root.indexed_generation,
        exclusions: root
            .exclusions
            .into_iter()
            .map(semantic_exclusion_status_to_dto)
            .collect(),
    }
}

fn semantic_eligibility_override_status_to_dto(
    status: SemanticEligibilityOverrideStatus,
) -> SemanticEligibilityOverrideStatusDto {
    SemanticEligibilityOverrideStatusDto {
        reason: semantic_eligibility_reason_to_dto(status.reason),
        action: match status.action {
            core::EligibilityOverride::Include => SemanticEligibilityOverrideDto::Include,
            core::EligibilityOverride::Exclude => SemanticEligibilityOverrideDto::Exclude,
        },
    }
}

fn semantic_exclusion_status_to_dto(
    exclusion: SemanticExclusionStatus,
) -> SemanticExclusionStatusDto {
    SemanticExclusionStatusDto {
        id: exclusion.id,
        location: exclusion.location.into(),
        cleanup: semantic_cleanup_status_to_dto(exclusion.cleanup),
    }
}

fn semantic_cleanup_status_to_dto(cleanup: SemanticCleanupStatus) -> SemanticCleanupStatusDto {
    SemanticCleanupStatusDto {
        plan_id: cleanup.plan_id,
        status: match cleanup.status {
            SemanticCleanupState::Pending => SemanticCleanupStateDto::Pending,
            SemanticCleanupState::Running => SemanticCleanupStateDto::Running,
            SemanticCleanupState::Failed => SemanticCleanupStateDto::Failed,
            SemanticCleanupState::Complete => SemanticCleanupStateDto::Complete,
        },
        categories: cleanup
            .categories
            .into_iter()
            .map(semantic_deletion_category_status_to_dto)
            .collect(),
    }
}

fn semantic_deletion_category_status_to_dto(
    category: SemanticDeletionCategoryStatus,
) -> SemanticDeletionCategoryStatusDto {
    SemanticDeletionCategoryStatusDto {
        category: semantic_deletion_category_to_dto(category.category),
        total_items: category.total_items,
        completed_items: category.completed_items,
        complete: category.complete,
        last_error: category.last_error,
    }
}

fn semantic_folder_status_to_dto(status: SemanticFolderStatus) -> SemanticFolderStatusDto {
    SemanticFolderStatusDto {
        consent: match status.consent {
            SemanticFolderConsent::IncludedHere => SemanticFolderConsentDto::IncludedHere,
            SemanticFolderConsent::InheritedFromParent => {
                SemanticFolderConsentDto::InheritedFromParent
            }
            SemanticFolderConsent::Excluded => SemanticFolderConsentDto::Excluded,
            SemanticFolderConsent::NotIncluded => SemanticFolderConsentDto::NotIncluded,
        },
        root_id: status.root_id,
        exclusion_id: status.exclusion_id,
        workspace_referenced: status.workspace_referenced,
        source_available: status.source_available,
        unavailable_reason: status.unavailable_reason,
    }
}

fn semantic_enrolment_preview_to_dto(
    preview: SemanticEnrolmentPreview,
) -> SemanticEnrolmentPreviewDto {
    SemanticEnrolmentPreviewDto {
        confirmation_id: preview.confirmation_id,
        policy_revision: preview.policy_revision,
        location: preview.location.into(),
        recursive: preview.recursive,
        estimate: semantic_estimate_to_dto(preview.estimate),
        normalized_excerpts_retained_locally: preview.normalized_excerpts_retained_locally,
    }
}

fn semantic_estimate_to_dto(
    estimate: SemanticEnrolmentEstimateStatus,
) -> SemanticEnrolmentEstimateDto {
    SemanticEnrolmentEstimateDto {
        completeness: match estimate.completeness {
            SemanticEstimateCompleteness::Estimated => SemanticEstimateCompletenessDto::Estimated,
            SemanticEstimateCompleteness::Partial => SemanticEstimateCompletenessDto::Partial,
            SemanticEstimateCompleteness::Unavailable => {
                SemanticEstimateCompletenessDto::Unavailable
            }
        },
        estimated_files: estimate.estimated_files,
        estimated_source_bytes: estimate.estimated_source_bytes,
        estimated_extracted_bytes: estimate.estimated_extracted_bytes,
        estimated_vector_bytes: estimate.estimated_vector_bytes,
        estimated_additional_local_bytes: estimate.estimated_additional_local_bytes,
        missing_model_download_bytes: estimate.missing_model_download_bytes,
        skipped_reason_counts: estimate
            .skipped_reason_counts
            .into_iter()
            .map(semantic_eligibility_reason_count_to_dto)
            .collect(),
        exceeded_budgets: estimate
            .exceeded_budgets
            .into_iter()
            .map(|kind| match kind {
                SemanticBudgetKind::Documents => SemanticBudgetKindDto::Documents,
                SemanticBudgetKind::SourceBytes => SemanticBudgetKindDto::SourceBytes,
                SemanticBudgetKind::ExtractedBytes => SemanticBudgetKindDto::ExtractedBytes,
                SemanticBudgetKind::VectorBytes => SemanticBudgetKindDto::VectorBytes,
            })
            .collect(),
        unavailable_reason: estimate.unavailable_reason,
    }
}

fn semantic_eligibility_reason_count_to_dto(
    count: SemanticEligibilityReasonCount,
) -> SemanticEligibilityReasonCountDto {
    SemanticEligibilityReasonCountDto {
        reason: semantic_eligibility_reason_to_dto(count.reason),
        count: count.count,
    }
}

fn semantic_exclusion_plan_to_dto(plan: SemanticExclusionPlan) -> SemanticExclusionPlanDto {
    SemanticExclusionPlanDto {
        confirmation_id: plan.confirmation_id,
        policy_revision: plan.policy_revision,
        root_id: plan.root_id,
        location: plan.location.into(),
        categories: plan
            .categories
            .into_iter()
            .map(semantic_deletion_category_status_to_dto)
            .collect(),
    }
}

const fn semantic_library_operation_to_dto(
    operation: SemanticLibraryOperation,
) -> SemanticLibraryOperationDto {
    match operation {
        SemanticLibraryOperation::ViewStatus => SemanticLibraryOperationDto::ViewStatus,
        SemanticLibraryOperation::ViewFolderStatus => SemanticLibraryOperationDto::ViewFolderStatus,
        SemanticLibraryOperation::PreviewEnrolment => SemanticLibraryOperationDto::PreviewEnrolment,
        SemanticLibraryOperation::Enrol => SemanticLibraryOperationDto::Enrol,
        SemanticLibraryOperation::PlanExclusion => SemanticLibraryOperationDto::PlanExclusion,
        SemanticLibraryOperation::ConfirmExclusion => SemanticLibraryOperationDto::ConfirmExclusion,
        SemanticLibraryOperation::ResumeCleanup => SemanticLibraryOperationDto::ResumeCleanup,
        SemanticLibraryOperation::Pause => SemanticLibraryOperationDto::Pause,
        SemanticLibraryOperation::Resume => SemanticLibraryOperationDto::Resume,
        SemanticLibraryOperation::UpdateEligibilityOverrides => {
            SemanticLibraryOperationDto::UpdateEligibilityOverrides
        }
    }
}

const fn semantic_deletion_category_to_dto(
    category: SemanticDeletionCategory,
) -> SemanticDeletionCategoryDto {
    match category {
        SemanticDeletionCategory::Occurrences => SemanticDeletionCategoryDto::Occurrences,
        SemanticDeletionCategory::ExtractedContent => SemanticDeletionCategoryDto::ExtractedContent,
        SemanticDeletionCategory::Summaries => SemanticDeletionCategoryDto::Summaries,
        SemanticDeletionCategory::Labels => SemanticDeletionCategoryDto::Labels,
        SemanticDeletionCategory::OrphanVectors => SemanticDeletionCategoryDto::OrphanVectors,
        SemanticDeletionCategory::ConversationEvidencePins => {
            SemanticDeletionCategoryDto::ConversationEvidencePins
        }
    }
}

const fn semantic_eligibility_reason_to_dto(
    reason: core::EligibilityReason,
) -> SemanticEligibilityReasonDto {
    match reason {
        core::EligibilityReason::Hidden => SemanticEligibilityReasonDto::Hidden,
        core::EligibilityReason::System => SemanticEligibilityReasonDto::System,
        core::EligibilityReason::ApplicationOrPackageBundle => {
            SemanticEligibilityReasonDto::ApplicationOrPackageBundle
        }
        core::EligibilityReason::DependencyDirectory => {
            SemanticEligibilityReasonDto::DependencyDirectory
        }
        core::EligibilityReason::BuildDirectory => SemanticEligibilityReasonDto::BuildDirectory,
        core::EligibilityReason::CacheDirectory => SemanticEligibilityReasonDto::CacheDirectory,
        core::EligibilityReason::GitIgnored => SemanticEligibilityReasonDto::GitIgnored,
        core::EligibilityReason::UnsupportedMime => SemanticEligibilityReasonDto::UnsupportedMime,
        core::EligibilityReason::Oversized => SemanticEligibilityReasonDto::Oversized,
        core::EligibilityReason::OverBudget => SemanticEligibilityReasonDto::OverBudget,
        core::EligibilityReason::SymlinkOutsideRoot => {
            SemanticEligibilityReasonDto::SymlinkOutsideRoot
        }
        core::EligibilityReason::ExplicitlyExcluded => {
            SemanticEligibilityReasonDto::ExplicitlyExcluded
        }
    }
}

const fn semantic_eligibility_reason_from_dto(
    reason: SemanticEligibilityReasonDto,
) -> core::EligibilityReason {
    match reason {
        SemanticEligibilityReasonDto::Hidden => core::EligibilityReason::Hidden,
        SemanticEligibilityReasonDto::System => core::EligibilityReason::System,
        SemanticEligibilityReasonDto::ApplicationOrPackageBundle => {
            core::EligibilityReason::ApplicationOrPackageBundle
        }
        SemanticEligibilityReasonDto::DependencyDirectory => {
            core::EligibilityReason::DependencyDirectory
        }
        SemanticEligibilityReasonDto::BuildDirectory => core::EligibilityReason::BuildDirectory,
        SemanticEligibilityReasonDto::CacheDirectory => core::EligibilityReason::CacheDirectory,
        SemanticEligibilityReasonDto::GitIgnored => core::EligibilityReason::GitIgnored,
        SemanticEligibilityReasonDto::UnsupportedMime => core::EligibilityReason::UnsupportedMime,
        SemanticEligibilityReasonDto::Oversized => core::EligibilityReason::Oversized,
        SemanticEligibilityReasonDto::OverBudget => core::EligibilityReason::OverBudget,
        SemanticEligibilityReasonDto::SymlinkOutsideRoot => {
            core::EligibilityReason::SymlinkOutsideRoot
        }
        SemanticEligibilityReasonDto::ExplicitlyExcluded => {
            core::EligibilityReason::ExplicitlyExcluded
        }
    }
}

const fn semantic_eligibility_override_from_dto(
    action: SemanticEligibilityOverrideDto,
) -> core::EligibilityOverride {
    match action {
        SemanticEligibilityOverrideDto::Include => core::EligibilityOverride::Include,
        SemanticEligibilityOverrideDto::Exclude => core::EligibilityOverride::Exclude,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_revision_error_keeps_machine_readable_revisions() {
        let dto = semantic_library_error_to_dto(
            SemanticLibraryError::StaleRevision {
                expected: 3,
                actual: 4,
            },
            Uuid::nil(),
        );
        assert_eq!(dto.code, SemanticLibraryErrorCodeDto::StaleRevision);
        assert_eq!(
            dto.details,
            Some(SemanticLibraryErrorDetailsDto::StaleRevision {
                expected: 3,
                actual: 4,
            })
        );
    }
}
