//! Explicit mappings between semantic application types and transport DTOs.

use std::path::{Path, PathBuf};

use fm_transport_dto::{
    AcceptSemanticInstallationOfferRequestDto, CheckpointSemanticModelMigrationRequestDto,
    CompleteSemanticModelMigrationRequestDto, ConfirmSemanticIndexRemovalRequestDto,
    ConfirmSemanticModelMigrationRequestDto, CreateSemanticIndexRemovalPlanRequestDto,
    CreateSemanticInstallationOfferRequestDto, ImportSemanticLocalModelRequestDto,
    InstallSemanticWorkerPatchRequestDto, InstalledSemanticComponentDto,
    InstalledSemanticComponentStateDto, MoveSemanticDataRequestDto,
    PlanSemanticModelMigrationRequestDto, SemanticCategoryDiskUseDto,
    SemanticComponentAuthorityDto, SemanticComponentCapabilitiesDto,
    SemanticComponentDisclosureDto, SemanticComponentErrorCodeDto,
    SemanticComponentErrorDetailsDto, SemanticComponentErrorDto, SemanticComponentKindDto,
    SemanticComponentLifecycleDto, SemanticComponentStatusDto, SemanticDataCategoryDto,
    SemanticDataMoveReceiptDto, SemanticDiskUseDto, SemanticEmbeddingNormalizationDto,
    SemanticIndexRecordCountsDto, SemanticIndexRemovalPlanDto, SemanticIndexRemovalReceiptDto,
    SemanticIndexRetentionDecisionDto, SemanticInstallReceiptDto, SemanticInstallationOfferDto,
    SemanticLicenseDto, SemanticModelIdentityDto, SemanticModelImportFieldDto,
    SemanticModelMetadataDto, SemanticModelMigrationPlanDto, SemanticModelMigrationProgressDto,
    SemanticModelMigrationReasonDto, SemanticModelProfileDto, SemanticModelSelectionDto,
    SemanticProfileDto, SemanticReindexEstimateDto, SemanticRuntimeExecutableDownloadDto,
    SemanticUninstallReceiptDto, SemanticWorkerPatchResponseDto,
    UninstallSemanticComponentsRequestDto,
};
use uuid::Uuid;

use crate::FileManagerService;
use crate::semantic_components::{
    InstallationOfferId, InstalledSemanticComponent, InstalledSemanticComponentState,
    RuntimeExecutableDownload, SemanticCategoryDiskUse, SemanticComponentAuthority,
    SemanticComponentCapabilities, SemanticComponentDisclosure, SemanticComponentError,
    SemanticComponentKind, SemanticComponentLifecycle, SemanticComponentOperation,
    SemanticComponentStatus, SemanticDataCategory, SemanticDataMoveReceipt, SemanticDiskUse,
    SemanticEmbeddingNormalization, SemanticIndexRecordCounts, SemanticIndexRemovalConfirmation,
    SemanticIndexRemovalPlan, SemanticIndexRemovalPlanId, SemanticIndexRemovalReceipt,
    SemanticIndexRetentionDecision, SemanticInstallReceipt, SemanticInstallationConsent,
    SemanticInstallationOffer, SemanticLocalModelImportRequest, SemanticModelIdentity,
    SemanticModelImportField, SemanticModelMigrationCheckpoint, SemanticModelMigrationConfirmation,
    SemanticModelMigrationId, SemanticModelMigrationPlan, SemanticModelMigrationProgress,
    SemanticModelMigrationReason, SemanticModelProfile, SemanticModelSelection, SemanticProfile,
    SemanticReindexEstimate, SemanticUninstallReceipt, SemanticWorkerPatchRequest,
};

/// Maps semantic authority to its stable transport discriminator.
#[must_use]
pub const fn semantic_component_authority_to_dto(
    authority: SemanticComponentAuthority,
) -> SemanticComponentAuthorityDto {
    match authority {
        SemanticComponentAuthority::Unavailable => SemanticComponentAuthorityDto::Unavailable,
        SemanticComponentAuthority::DesktopManaged => SemanticComponentAuthorityDto::DesktopManaged,
        SemanticComponentAuthority::AdministratorProvisioned => {
            SemanticComponentAuthorityDto::AdministratorProvisioned
        }
        SemanticComponentAuthority::DeterministicMock => {
            SemanticComponentAuthorityDto::DeterministicMock
        }
    }
}

/// Maps executable-download policy to its stable transport discriminator.
#[must_use]
pub const fn semantic_runtime_executable_download_to_dto(
    policy: RuntimeExecutableDownload,
) -> SemanticRuntimeExecutableDownloadDto {
    match policy {
        RuntimeExecutableDownload::Unavailable => SemanticRuntimeExecutableDownloadDto::Unavailable,
        RuntimeExecutableDownload::DirectDistribution => {
            SemanticRuntimeExecutableDownloadDto::DirectDistribution
        }
        RuntimeExecutableDownload::ProhibitedByMacAppStore => {
            SemanticRuntimeExecutableDownloadDto::ProhibitedByMacAppStore
        }
        RuntimeExecutableDownload::AdministratorProvisioned => {
            SemanticRuntimeExecutableDownloadDto::AdministratorProvisioned
        }
        RuntimeExecutableDownload::Simulated => SemanticRuntimeExecutableDownloadDto::Simulated,
    }
}

/// Maps a complete semantic capability report to its wire representation.
#[must_use]
pub fn semantic_component_capabilities_to_dto(
    capabilities: SemanticComponentCapabilities,
) -> SemanticComponentCapabilitiesDto {
    SemanticComponentCapabilitiesDto {
        authority: semantic_component_authority_to_dto(capabilities.authority()),
        operations: capabilities
            .operations()
            .iter()
            .copied()
            .map(semantic_component_operation_to_dto)
            .collect(),
        runtime_executable_download: semantic_runtime_executable_download_to_dto(
            capabilities.runtime_executable_download(),
        ),
    }
}

/// Maps complete semantic component state to its wire representation.
#[must_use]
pub fn semantic_component_status_to_dto(
    status: SemanticComponentStatus,
) -> SemanticComponentStatusDto {
    SemanticComponentStatusDto {
        lifecycle: semantic_component_lifecycle_to_dto(status.lifecycle()),
        data_root: status.data_root().map(path_to_transport_string),
        active_model: status.active_model().map(semantic_model_selection_to_dto),
        migration: status
            .migration()
            .map(semantic_model_migration_progress_to_dto),
        components: status
            .components()
            .iter()
            .map(installed_semantic_component_to_dto)
            .collect(),
        disk_use: semantic_disk_use_to_dto(status.disk_use()),
    }
}

/// Maps an actionable semantic failure to the shared HTTP/Tauri error shape.
#[must_use]
pub fn semantic_component_error_to_dto(
    error: SemanticComponentError,
    request_id: Uuid,
) -> SemanticComponentErrorDto {
    let message = error.to_string();
    let (code, details) = match error {
        SemanticComponentError::Unavailable => (SemanticComponentErrorCodeDto::Unavailable, None),
        SemanticComponentError::AuthorityDenied {
            authority,
            operation,
        } => (
            SemanticComponentErrorCodeDto::AuthorityDenied,
            Some(SemanticComponentErrorDetailsDto::AuthorityDenied {
                authority: semantic_component_authority_to_dto(authority),
                operation: semantic_component_operation_to_dto(operation),
            }),
        ),
        SemanticComponentError::ConsentRequired => {
            (SemanticComponentErrorCodeDto::ConsentRequired, None)
        }
        SemanticComponentError::InvalidLocalModelMetadata { field } => (
            SemanticComponentErrorCodeDto::InvalidLocalModelMetadata,
            Some(
                SemanticComponentErrorDetailsDto::InvalidLocalModelMetadata {
                    field: semantic_model_import_field_to_dto(field),
                },
            ),
        ),
        SemanticComponentError::InvalidLifecycle {
            operation,
            lifecycle,
        } => (
            SemanticComponentErrorCodeDto::InvalidLifecycle,
            Some(SemanticComponentErrorDetailsDto::InvalidLifecycle {
                operation: semantic_component_operation_to_dto(operation),
                lifecycle,
            }),
        ),
        SemanticComponentError::InvalidEnrolment => {
            (SemanticComponentErrorCodeDto::InvalidEnrolment, None)
        }
        SemanticComponentError::InvalidMigrationPlan => {
            (SemanticComponentErrorCodeDto::InvalidMigrationPlan, None)
        }
        SemanticComponentError::InvalidMigrationProgress {
            completed_documents,
            total_documents,
        } => (
            SemanticComponentErrorCodeDto::InvalidMigrationProgress,
            Some(SemanticComponentErrorDetailsDto::InvalidMigrationProgress {
                completed_documents,
                total_documents,
            }),
        ),
        SemanticComponentError::ExecutableDownloadProhibited => (
            SemanticComponentErrorCodeDto::ExecutableDownloadProhibited,
            None,
        ),
        SemanticComponentError::Catalog { .. } => (SemanticComponentErrorCodeDto::Catalog, None),
        SemanticComponentError::InsufficientSpace {
            available_bytes,
            required_bytes,
            reserve_bytes,
        } => (
            SemanticComponentErrorCodeDto::InsufficientSpace,
            Some(SemanticComponentErrorDetailsDto::InsufficientSpace {
                available_bytes,
                required_bytes,
                reserve_bytes,
            }),
        ),
        SemanticComponentError::DownloadInterrupted { artifact_id, .. } => (
            SemanticComponentErrorCodeDto::DownloadInterrupted,
            Some(SemanticComponentErrorDetailsDto::Artifact { artifact_id }),
        ),
        SemanticComponentError::ArtifactVerificationFailed { artifact_id } => (
            SemanticComponentErrorCodeDto::ArtifactVerificationFailed,
            Some(SemanticComponentErrorDetailsDto::Artifact { artifact_id }),
        ),
        SemanticComponentError::ActivationFailed { artifact_id, .. } => (
            SemanticComponentErrorCodeDto::ActivationFailed,
            Some(SemanticComponentErrorDetailsDto::Artifact { artifact_id }),
        ),
        SemanticComponentError::FreeSpaceProbe { .. } => {
            (SemanticComponentErrorCodeDto::FreeSpaceProbe, None)
        }
        SemanticComponentError::State { .. } => (SemanticComponentErrorCodeDto::State, None),
        SemanticComponentError::Filesystem { .. } => {
            (SemanticComponentErrorCodeDto::Filesystem, None)
        }
        SemanticComponentError::Indexing { .. } => (SemanticComponentErrorCodeDto::Indexing, None),
        SemanticComponentError::IndexRemoval { .. } => {
            (SemanticComponentErrorCodeDto::IndexRemoval, None)
        }
        SemanticComponentError::DataMigration { .. } => {
            (SemanticComponentErrorCodeDto::DataMigration, None)
        }
        SemanticComponentError::BlockingTaskFailed { .. } => {
            (SemanticComponentErrorCodeDto::BlockingTaskFailed, None)
        }
        SemanticComponentError::DiskUseOverflow => {
            (SemanticComponentErrorCodeDto::DiskUseOverflow, None)
        }
    };
    SemanticComponentErrorDto {
        code,
        message,
        request_id,
        details,
    }
}

impl FileManagerService {
    /// Reports semantic component authority and lifecycle operations as a wire DTO.
    pub async fn semantic_component_capabilities_dto(&self) -> SemanticComponentCapabilitiesDto {
        semantic_component_capabilities_to_dto(self.semantic_component_capabilities().await)
    }

    /// Reports semantic component lifecycle and disk-use state as a wire DTO.
    pub async fn semantic_component_status_dto(
        &self,
    ) -> Result<SemanticComponentStatusDto, SemanticComponentError> {
        self.semantic_component_status()
            .await
            .map(semantic_component_status_to_dto)
    }

    /// Returns curated model profiles and exact revisions as wire DTOs.
    pub async fn semantic_component_catalog_profiles_dto(
        &self,
    ) -> Result<Vec<SemanticModelProfileDto>, SemanticComponentError> {
        self.semantic_component_catalog_profiles()
            .await
            .map(|profiles| {
                profiles
                    .into_iter()
                    .map(semantic_model_profile_to_dto)
                    .collect()
            })
    }

    /// Creates a signed installation disclosure from a transport request.
    pub async fn create_semantic_component_installation_offer(
        &self,
        request: CreateSemanticInstallationOfferRequestDto,
    ) -> Result<SemanticInstallationOfferDto, SemanticComponentError> {
        self.semantic_component_installation_offer(semantic_profile_from_dto(request.profile))
            .await
            .map(semantic_installation_offer_to_dto)
    }

    /// Explicitly accepts and installs one previously returned offer.
    pub async fn accept_semantic_component_installation_offer(
        &self,
        request: AcceptSemanticInstallationOfferRequestDto,
    ) -> Result<SemanticInstallReceiptDto, SemanticComponentError> {
        let offer_id = InstallationOfferId::new(request.offer_id)?;
        self.semantic_component_install_or_enable(SemanticInstallationConsent::accept(offer_id))
            .await
            .map(semantic_install_receipt_to_dto)
    }

    /// Installs the newest compatible signed worker patch.
    pub async fn install_semantic_component_worker_patch(
        &self,
        request: InstallSemanticWorkerPatchRequestDto,
    ) -> Result<SemanticWorkerPatchResponseDto, SemanticComponentError> {
        self.semantic_component_install_compatible_worker_patch(SemanticWorkerPatchRequest {
            component_id: request.component_id,
        })
        .await
        .map(|receipt| SemanticWorkerPatchResponseDto {
            receipt: receipt.map(semantic_install_receipt_to_dto),
        })
    }

    /// Inventories one enrolment and creates an authoritative removal plan.
    pub async fn create_semantic_component_index_removal_plan(
        &self,
        request: CreateSemanticIndexRemovalPlanRequestDto,
    ) -> Result<SemanticIndexRemovalPlanDto, SemanticComponentError> {
        self.semantic_component_plan_index_removal(request.enrolment_id)
            .await
            .map(semantic_index_removal_plan_to_dto)
    }

    /// Confirms one live authoritative enrolment-removal plan.
    pub async fn confirm_semantic_component_index_removal(
        &self,
        request: ConfirmSemanticIndexRemovalRequestDto,
    ) -> Result<SemanticIndexRemovalReceiptDto, SemanticComponentError> {
        let plan_id = SemanticIndexRemovalPlanId::new(request.plan_id)?;
        self.semantic_component_confirm_index_removal(SemanticIndexRemovalConfirmation::confirm(
            plan_id,
        ))
        .await
        .map(semantic_index_removal_receipt_to_dto)
    }

    /// Moves semantic data to a caller-selected desktop path.
    pub async fn move_semantic_component_data(
        &self,
        request: MoveSemanticDataRequestDto,
    ) -> Result<SemanticDataMoveReceiptDto, SemanticComponentError> {
        self.semantic_component_move_data(PathBuf::from(request.destination))
            .await
            .map(semantic_data_move_receipt_to_dto)
    }

    /// Uninstalls components with an explicit remaining-index decision.
    pub async fn uninstall_semantic_components(
        &self,
        request: UninstallSemanticComponentsRequestDto,
    ) -> Result<SemanticUninstallReceiptDto, SemanticComponentError> {
        self.semantic_component_uninstall(semantic_index_retention_from_dto(request.index_decision))
            .await
            .map(semantic_uninstall_receipt_to_dto)
    }

    /// Validates an expert local model and returns its migration plan.
    pub async fn import_semantic_component_local_model(
        &self,
        request: ImportSemanticLocalModelRequestDto,
    ) -> Result<SemanticModelMigrationPlanDto, SemanticComponentError> {
        let (model, profile, estimate) = semantic_local_model_import_from_dto(request);
        self.semantic_component_import_local_model(model, profile, estimate)
            .await
            .map(semantic_model_migration_plan_to_dto)
    }

    /// Plans a migration to a signed-catalog model.
    pub async fn plan_semantic_component_model_migration(
        &self,
        request: PlanSemanticModelMigrationRequestDto,
    ) -> Result<SemanticModelMigrationPlanDto, SemanticComponentError> {
        self.semantic_component_plan_model_migration(
            semantic_profile_from_dto(request.profile),
            semantic_reindex_estimate_from_dto(request.estimate),
        )
        .await
        .map(semantic_model_migration_plan_to_dto)
    }

    /// Confirms and begins one model migration.
    pub async fn confirm_semantic_component_model_migration(
        &self,
        request: ConfirmSemanticModelMigrationRequestDto,
    ) -> Result<SemanticModelMigrationProgressDto, SemanticComponentError> {
        let migration_id = semantic_model_migration_id_from_transport(request.migration_id)?;
        self.semantic_component_confirm_model_migration(
            SemanticModelMigrationConfirmation::confirm(migration_id),
        )
        .await
        .map(|progress| semantic_model_migration_progress_to_dto(&progress))
    }

    /// Persists one resumable model migration checkpoint.
    pub async fn checkpoint_semantic_component_model_migration(
        &self,
        request: CheckpointSemanticModelMigrationRequestDto,
    ) -> Result<SemanticModelMigrationProgressDto, SemanticComponentError> {
        let migration_id = semantic_model_migration_id_from_transport(request.migration_id)?;
        self.semantic_component_checkpoint_model_migration(SemanticModelMigrationCheckpoint {
            migration_id,
            completed_documents: request.completed_documents,
            resume_cursor: request.resume_cursor,
        })
        .await
        .map(|progress| semantic_model_migration_progress_to_dto(&progress))
    }

    /// Completes and activates one fully reindexed model migration.
    pub async fn complete_semantic_component_model_migration(
        &self,
        request: CompleteSemanticModelMigrationRequestDto,
    ) -> Result<SemanticModelSelectionDto, SemanticComponentError> {
        let migration_id = semantic_model_migration_id_from_transport(request.migration_id)?;
        self.semantic_component_complete_model_migration(migration_id)
            .await
            .map(|selection| semantic_model_selection_to_dto(&selection))
    }
}

const fn semantic_component_operation_to_dto(
    operation: SemanticComponentOperation,
) -> fm_transport_dto::SemanticComponentOperationDto {
    use fm_transport_dto::SemanticComponentOperationDto as Dto;
    match operation {
        SemanticComponentOperation::ViewStatus => Dto::ViewStatus,
        SemanticComponentOperation::ViewCatalog => Dto::ViewCatalog,
        SemanticComponentOperation::CreateInstallationOffer => Dto::CreateInstallationOffer,
        SemanticComponentOperation::InstallOrEnable => Dto::InstallOrEnable,
        SemanticComponentOperation::InstallWorkerPatch => Dto::InstallWorkerPatch,
        SemanticComponentOperation::PauseIndexing => Dto::PauseIndexing,
        SemanticComponentOperation::ResumeIndexing => Dto::ResumeIndexing,
        SemanticComponentOperation::RemoveIndex => Dto::RemoveIndex,
        SemanticComponentOperation::MoveData => Dto::MoveData,
        SemanticComponentOperation::UninstallComponents => Dto::UninstallComponents,
        SemanticComponentOperation::ImportLocalModel => Dto::ImportLocalModel,
        SemanticComponentOperation::PlanModelMigration => Dto::PlanModelMigration,
        SemanticComponentOperation::ConfirmModelMigration => Dto::ConfirmModelMigration,
        SemanticComponentOperation::CheckpointModelMigration => Dto::CheckpointModelMigration,
        SemanticComponentOperation::CompleteModelMigration => Dto::CompleteModelMigration,
    }
}

const fn semantic_profile_to_dto(profile: SemanticProfile) -> SemanticProfileDto {
    match profile {
        SemanticProfile::CompactMultilingual => SemanticProfileDto::CompactMultilingual,
        SemanticProfile::CompactEnglish => SemanticProfileDto::CompactEnglish,
        SemanticProfile::MultilingualQuality => SemanticProfileDto::MultilingualQuality,
    }
}

const fn semantic_profile_from_dto(profile: SemanticProfileDto) -> SemanticProfile {
    match profile {
        SemanticProfileDto::CompactMultilingual => SemanticProfile::CompactMultilingual,
        SemanticProfileDto::CompactEnglish => SemanticProfile::CompactEnglish,
        SemanticProfileDto::MultilingualQuality => SemanticProfile::MultilingualQuality,
    }
}

fn semantic_component_lifecycle_to_dto(
    lifecycle: &SemanticComponentLifecycle,
) -> SemanticComponentLifecycleDto {
    match lifecycle {
        SemanticComponentLifecycle::Unavailable => SemanticComponentLifecycleDto::Unavailable,
        SemanticComponentLifecycle::Absent => SemanticComponentLifecycleDto::Absent,
        SemanticComponentLifecycle::Offered { offer_id } => {
            SemanticComponentLifecycleDto::Offered {
                offer_id: offer_id.as_str().to_owned(),
            }
        }
        SemanticComponentLifecycle::Downloading {
            downloaded_bytes,
            total_bytes,
            resumable,
        } => SemanticComponentLifecycleDto::Downloading {
            downloaded_bytes: *downloaded_bytes,
            total_bytes: *total_bytes,
            resumable: *resumable,
        },
        SemanticComponentLifecycle::InstalledEnabled => {
            SemanticComponentLifecycleDto::InstalledEnabled
        }
        SemanticComponentLifecycle::Paused => SemanticComponentLifecycleDto::Paused,
        SemanticComponentLifecycle::Migrating { progress } => {
            SemanticComponentLifecycleDto::Migrating {
                progress: semantic_model_migration_progress_to_dto(progress),
            }
        }
        SemanticComponentLifecycle::UpdateFailedRolledBack {
            failed_version,
            active_version,
        } => SemanticComponentLifecycleDto::UpdateFailedRolledBack {
            failed_version: failed_version.clone(),
            active_version: active_version.clone(),
        },
        SemanticComponentLifecycle::LowDisk {
            available_bytes,
            required_bytes,
        } => SemanticComponentLifecycleDto::LowDisk {
            available_bytes: *available_bytes,
            required_bytes: *required_bytes,
        },
        SemanticComponentLifecycle::Uninstalled { index_decision } => {
            SemanticComponentLifecycleDto::Uninstalled {
                index_decision: semantic_index_retention_to_dto(*index_decision),
            }
        }
    }
}

fn semantic_model_selection_to_dto(
    selection: &SemanticModelSelection,
) -> SemanticModelSelectionDto {
    SemanticModelSelectionDto {
        profile: semantic_profile_to_dto(selection.profile()),
        identity: semantic_model_identity_to_dto(selection.identity()),
    }
}

fn semantic_model_identity_to_dto(identity: &SemanticModelIdentity) -> SemanticModelIdentityDto {
    SemanticModelIdentityDto {
        model_id: identity.model_id().to_owned(),
        revision: identity.revision().to_owned(),
    }
}

fn semantic_reindex_estimate_to_dto(
    estimate: SemanticReindexEstimate,
) -> SemanticReindexEstimateDto {
    SemanticReindexEstimateDto {
        documents: estimate.documents(),
        source_bytes: estimate.source_bytes(),
    }
}

const fn semantic_reindex_estimate_from_dto(
    estimate: SemanticReindexEstimateDto,
) -> SemanticReindexEstimate {
    SemanticReindexEstimate::new(estimate.documents, estimate.source_bytes)
}

fn semantic_model_migration_progress_to_dto(
    progress: &SemanticModelMigrationProgress,
) -> SemanticModelMigrationProgressDto {
    SemanticModelMigrationProgressDto {
        migration_id: progress.migration_id().as_str().to_owned(),
        completed_documents: progress.completed_documents(),
        estimate: semantic_reindex_estimate_to_dto(progress.estimate()),
        target: semantic_model_selection_to_dto(progress.target()),
        reason: semantic_model_migration_reason_to_dto(progress.reason()),
        resume_cursor: progress.resume_cursor().map(ToOwned::to_owned),
    }
}

const fn semantic_model_migration_reason_to_dto(
    reason: SemanticModelMigrationReason,
) -> SemanticModelMigrationReasonDto {
    match reason {
        SemanticModelMigrationReason::ModelChanged => SemanticModelMigrationReasonDto::ModelChanged,
        SemanticModelMigrationReason::SchemaChanged {
            from_version,
            to_version,
        } => SemanticModelMigrationReasonDto::SchemaChanged {
            from_version,
            to_version,
        },
    }
}

fn installed_semantic_component_to_dto(
    component: &InstalledSemanticComponent,
) -> InstalledSemanticComponentDto {
    InstalledSemanticComponentDto {
        artifact_id: component.artifact_id().to_owned(),
        component_id: component.component_id().to_owned(),
        kind: semantic_component_kind_to_dto(component.kind()),
        version: component.version().to_owned(),
        state: installed_semantic_component_state_to_dto(component.state()),
        installed_bytes: component.installed_bytes(),
    }
}

const fn semantic_component_kind_to_dto(kind: SemanticComponentKind) -> SemanticComponentKindDto {
    match kind {
        SemanticComponentKind::Worker => SemanticComponentKindDto::Worker,
        SemanticComponentKind::Runtime => SemanticComponentKindDto::Runtime,
        SemanticComponentKind::Model => SemanticComponentKindDto::Model,
    }
}

const fn installed_semantic_component_state_to_dto(
    state: InstalledSemanticComponentState,
) -> InstalledSemanticComponentStateDto {
    match state {
        InstalledSemanticComponentState::Active => InstalledSemanticComponentStateDto::Active,
        InstalledSemanticComponentState::Rollback => InstalledSemanticComponentStateDto::Rollback,
    }
}

fn semantic_disk_use_to_dto(disk_use: &SemanticDiskUse) -> SemanticDiskUseDto {
    SemanticDiskUseDto {
        categories: disk_use
            .categories()
            .iter()
            .copied()
            .map(semantic_category_disk_use_to_dto)
            .collect(),
        total_bytes: disk_use.total_bytes(),
    }
}

const fn semantic_category_disk_use_to_dto(
    usage: SemanticCategoryDiskUse,
) -> SemanticCategoryDiskUseDto {
    SemanticCategoryDiskUseDto {
        category: semantic_data_category_to_dto(usage.category()),
        bytes: usage.bytes(),
    }
}

const fn semantic_data_category_to_dto(category: SemanticDataCategory) -> SemanticDataCategoryDto {
    match category {
        SemanticDataCategory::Catalog => SemanticDataCategoryDto::Catalog,
        SemanticDataCategory::Extracted => SemanticDataCategoryDto::Extracted,
        SemanticDataCategory::Zvec => SemanticDataCategoryDto::Zvec,
        SemanticDataCategory::EmbeddingCache => SemanticDataCategoryDto::EmbeddingCache,
        SemanticDataCategory::Models => SemanticDataCategoryDto::Models,
        SemanticDataCategory::Workers => SemanticDataCategoryDto::Workers,
    }
}

fn semantic_model_profile_to_dto(profile: SemanticModelProfile) -> SemanticModelProfileDto {
    SemanticModelProfileDto {
        profile: semantic_profile_to_dto(profile.profile),
        recommended: profile.recommended,
        explanation: profile.explanation,
        resolved_model: semantic_model_identity_to_dto(&profile.resolved_model),
        metadata: semantic_model_metadata_to_dto(profile.metadata),
    }
}

fn semantic_model_metadata_to_dto(
    metadata: crate::semantic_components::SemanticModelMetadata,
) -> SemanticModelMetadataDto {
    SemanticModelMetadataDto {
        identity: semantic_model_identity_to_dto(&metadata.identity),
        license: semantic_license_to_dto(metadata.license),
        tokenizer: metadata.tokenizer,
        dimensions: metadata.dimensions,
        normalization: semantic_embedding_normalization_to_dto(metadata.normalization),
        runtime_component_id: metadata.runtime_component_id,
        runtime_version_requirement: metadata.runtime_version_requirement,
        language_coverage: metadata.language_coverage,
        estimated_disk_bytes: metadata.estimated_disk_bytes,
        estimated_ram_bytes: metadata.estimated_ram_bytes,
    }
}

const fn semantic_embedding_normalization_to_dto(
    normalization: SemanticEmbeddingNormalization,
) -> SemanticEmbeddingNormalizationDto {
    match normalization {
        SemanticEmbeddingNormalization::UnitLength => SemanticEmbeddingNormalizationDto::UnitLength,
        SemanticEmbeddingNormalization::None => SemanticEmbeddingNormalizationDto::None,
    }
}

const fn semantic_embedding_normalization_from_dto(
    normalization: SemanticEmbeddingNormalizationDto,
) -> SemanticEmbeddingNormalization {
    match normalization {
        SemanticEmbeddingNormalizationDto::UnitLength => SemanticEmbeddingNormalization::UnitLength,
        SemanticEmbeddingNormalizationDto::None => SemanticEmbeddingNormalization::None,
    }
}

fn semantic_license_to_dto(
    license: crate::semantic_components::SemanticLicense,
) -> SemanticLicenseDto {
    SemanticLicenseDto {
        spdx: license.spdx,
        notice: license.notice,
    }
}

fn semantic_installation_offer_to_dto(
    offer: SemanticInstallationOffer,
) -> SemanticInstallationOfferDto {
    SemanticInstallationOfferDto {
        offer_id: offer.id().as_str().to_owned(),
        catalog_revision: offer.catalog_revision,
        profile: semantic_profile_to_dto(offer.profile),
        resolved_model: semantic_model_identity_to_dto(&offer.resolved_model),
        components: offer
            .components
            .into_iter()
            .map(semantic_component_disclosure_to_dto)
            .collect(),
        embeddings_stay_local: offer.embeddings_stay_local,
        local_only_disclosure: offer.local_only_disclosure,
        data_root: path_to_transport_string(&offer.data_root),
        minimum_free_space_reserve_bytes: offer.minimum_free_space_reserve_bytes,
    }
}

fn semantic_component_disclosure_to_dto(
    disclosure: SemanticComponentDisclosure,
) -> SemanticComponentDisclosureDto {
    SemanticComponentDisclosureDto {
        artifact_id: disclosure.artifact_id,
        component_id: disclosure.component_id,
        kind: semantic_component_kind_to_dto(disclosure.kind),
        model: disclosure
            .model
            .as_ref()
            .map(semantic_model_identity_to_dto),
        version: disclosure.version,
        license: semantic_license_to_dto(disclosure.license),
        download_bytes: disclosure.download_bytes,
        estimated_installed_bytes: disclosure.estimated_installed_bytes,
        estimated_ram_bytes: disclosure.estimated_ram_bytes,
    }
}

fn semantic_install_receipt_to_dto(receipt: SemanticInstallReceipt) -> SemanticInstallReceiptDto {
    SemanticInstallReceiptDto {
        installed_artifact_ids: receipt.installed_artifact_ids,
    }
}

const fn semantic_index_retention_to_dto(
    decision: SemanticIndexRetentionDecision,
) -> SemanticIndexRetentionDecisionDto {
    match decision {
        SemanticIndexRetentionDecision::Retain => SemanticIndexRetentionDecisionDto::Retain,
        SemanticIndexRetentionDecision::Delete => SemanticIndexRetentionDecisionDto::Delete,
    }
}

const fn semantic_index_retention_from_dto(
    decision: SemanticIndexRetentionDecisionDto,
) -> SemanticIndexRetentionDecision {
    match decision {
        SemanticIndexRetentionDecisionDto::Retain => SemanticIndexRetentionDecision::Retain,
        SemanticIndexRetentionDecisionDto::Delete => SemanticIndexRetentionDecision::Delete,
    }
}

const fn semantic_index_record_counts_to_dto(
    counts: SemanticIndexRecordCounts,
) -> SemanticIndexRecordCountsDto {
    SemanticIndexRecordCountsDto {
        index_records: counts.index_records,
        extracted_files: counts.extracted_files,
        zvec_vectors: counts.zvec_vectors,
        cache_entries: counts.cache_entries,
        conversation_evidence: counts.conversation_evidence,
    }
}

fn semantic_index_removal_plan_to_dto(
    plan: SemanticIndexRemovalPlan,
) -> SemanticIndexRemovalPlanDto {
    SemanticIndexRemovalPlanDto {
        plan_id: plan.id().as_str().to_owned(),
        enrolment_id: plan.enrolment_id,
        expected: semantic_index_record_counts_to_dto(plan.expected),
    }
}

fn semantic_index_removal_receipt_to_dto(
    receipt: SemanticIndexRemovalReceipt,
) -> SemanticIndexRemovalReceiptDto {
    SemanticIndexRemovalReceiptDto {
        enrolment_id: receipt.enrolment_id,
        deleted: semantic_index_record_counts_to_dto(receipt.deleted),
        conversation_evidence_deleted: receipt.conversation_evidence_deleted,
    }
}

fn semantic_data_move_receipt_to_dto(
    receipt: SemanticDataMoveReceipt,
) -> SemanticDataMoveReceiptDto {
    SemanticDataMoveReceiptDto {
        source: path_to_transport_string(&receipt.source),
        destination: path_to_transport_string(&receipt.destination),
        verified_file_count: receipt.verified_file_count,
        verified_bytes: receipt.verified_bytes,
    }
}

const fn semantic_uninstall_receipt_to_dto(
    receipt: SemanticUninstallReceipt,
) -> SemanticUninstallReceiptDto {
    SemanticUninstallReceiptDto {
        index_decision: semantic_index_retention_to_dto(receipt.index_decision),
        removed_component_count: receipt.removed_component_count,
    }
}

fn semantic_local_model_import_from_dto(
    request: ImportSemanticLocalModelRequestDto,
) -> (
    SemanticLocalModelImportRequest,
    SemanticProfile,
    SemanticReindexEstimate,
) {
    let profile = semantic_profile_from_dto(request.profile);
    let estimate = semantic_reindex_estimate_from_dto(request.estimate);
    let model = SemanticLocalModelImportRequest {
        source_path: PathBuf::from(request.source_path),
        model_id: request.model_id,
        upstream_revision: request.upstream_revision,
        license_spdx: request.license_spdx,
        license_notice: request.license_notice,
        tokenizer: request.tokenizer,
        dimensions: request.dimensions,
        normalization: request
            .normalization
            .map(semantic_embedding_normalization_from_dto),
        runtime_component_id: request.runtime_component_id,
        runtime_version_requirement: request.runtime_version_requirement,
        language_coverage: request.language_coverage,
        estimated_disk_bytes: request.estimated_disk_bytes,
        estimated_ram_bytes: request.estimated_ram_bytes,
    };
    (model, profile, estimate)
}

fn semantic_model_migration_plan_to_dto(
    plan: SemanticModelMigrationPlan,
) -> SemanticModelMigrationPlanDto {
    SemanticModelMigrationPlanDto {
        migration_id: plan.id().as_str().to_owned(),
        from: plan.from.as_ref().map(semantic_model_selection_to_dto),
        target: semantic_model_selection_to_dto(&plan.target),
        estimate: semantic_reindex_estimate_to_dto(plan.estimate),
        reason: semantic_model_migration_reason_to_dto(plan.reason),
        requires_confirmation: plan.requires_confirmation(),
        full_reindex: plan.is_full_reindex(),
        resumable: plan.is_resumable(),
    }
}

fn semantic_model_migration_id_from_transport(
    value: String,
) -> Result<SemanticModelMigrationId, SemanticComponentError> {
    if value.trim().is_empty() {
        return Err(SemanticComponentError::InvalidMigrationPlan);
    }
    Ok(SemanticModelMigrationId::new(value))
}

const fn semantic_model_import_field_to_dto(
    field: SemanticModelImportField,
) -> SemanticModelImportFieldDto {
    match field {
        SemanticModelImportField::SourcePath => SemanticModelImportFieldDto::SourcePath,
        SemanticModelImportField::ModelId => SemanticModelImportFieldDto::ModelId,
        SemanticModelImportField::UpstreamRevision => SemanticModelImportFieldDto::UpstreamRevision,
        SemanticModelImportField::License => SemanticModelImportFieldDto::License,
        SemanticModelImportField::Tokenizer => SemanticModelImportFieldDto::Tokenizer,
        SemanticModelImportField::Dimensions => SemanticModelImportFieldDto::Dimensions,
        SemanticModelImportField::Normalization => SemanticModelImportFieldDto::Normalization,
        SemanticModelImportField::RuntimeCompatibility => {
            SemanticModelImportFieldDto::RuntimeCompatibility
        }
        SemanticModelImportField::LanguageCoverage => SemanticModelImportFieldDto::LanguageCoverage,
        SemanticModelImportField::EstimatedDiskBytes => {
            SemanticModelImportFieldDto::EstimatedDiskBytes
        }
        SemanticModelImportField::EstimatedRamBytes => {
            SemanticModelImportFieldDto::EstimatedRamBytes
        }
    }
}

fn path_to_transport_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::semantic_components::{
        FakeSemanticComponentCapability, FakeSemanticComponentScenario, SemanticComponentCapability,
    };

    #[tokio::test]
    async fn deterministic_mock_lifecycle_maps_through_the_transport_status() {
        let capability = Arc::new(FakeSemanticComponentCapability::new());
        capability.set_scenario(FakeSemanticComponentScenario::LowDisk);
        let status = capability.status().await.expect("mock status");

        let dto = semantic_component_status_to_dto(status);

        assert!(matches!(
            dto.lifecycle,
            SemanticComponentLifecycleDto::LowDisk {
                available_bytes: 50,
                required_bytes: 100
            }
        ));
    }

    #[tokio::test]
    async fn index_removal_plan_maps_authoritative_counts_and_opaque_identity() {
        let capability = FakeSemanticComponentCapability::new();
        let plan = capability
            .plan_index_removal("library-1".to_owned())
            .await
            .expect("create removal plan");
        let confirmation = SemanticIndexRemovalConfirmation::confirm(
            SemanticIndexRemovalPlanId::new(plan.id().as_str()).expect("valid plan identity"),
        );

        let dto = semantic_index_removal_plan_to_dto(plan);
        let receipt = capability
            .confirm_index_removal(confirmation)
            .await
            .expect("confirm removal plan");

        assert_eq!(dto.plan_id, "fake-index-removal-1");
        assert_eq!(dto.enrolment_id, "library-1");
        assert_eq!(dto.expected.conversation_evidence, 2);
        assert_eq!(
            semantic_index_removal_receipt_to_dto(receipt).deleted,
            dto.expected
        );
    }

    #[test]
    fn direct_and_app_store_download_policies_remain_distinct_on_the_wire() {
        assert_eq!(
            semantic_runtime_executable_download_to_dto(
                RuntimeExecutableDownload::DirectDistribution
            ),
            SemanticRuntimeExecutableDownloadDto::DirectDistribution
        );
        assert_eq!(
            semantic_runtime_executable_download_to_dto(
                RuntimeExecutableDownload::ProhibitedByMacAppStore
            ),
            SemanticRuntimeExecutableDownloadDto::ProhibitedByMacAppStore
        );
    }

    #[test]
    fn authority_denial_keeps_the_rejected_operation_typed() {
        let dto = semantic_component_error_to_dto(
            SemanticComponentError::AuthorityDenied {
                authority: SemanticComponentAuthority::AdministratorProvisioned,
                operation: SemanticComponentOperation::MoveData,
            },
            Uuid::nil(),
        );

        assert_eq!(dto.code, SemanticComponentErrorCodeDto::AuthorityDenied);
        assert_eq!(
            dto.details,
            Some(SemanticComponentErrorDetailsDto::AuthorityDenied {
                authority: SemanticComponentAuthorityDto::AdministratorProvisioned,
                operation: fm_transport_dto::SemanticComponentOperationDto::MoveData,
            })
        );
    }
}
