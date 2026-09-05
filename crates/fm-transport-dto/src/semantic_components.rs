//! Wire types for managed semantic component lifecycle operations (task 0178).
//!
//! These types deliberately duplicate the application-layer vocabulary. They
//! are the stable HTTP/Tauri ABI and must not make application types part of
//! that wire contract.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Principal that owns semantic component lifecycle changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticComponentAuthorityDto {
    /// No semantic component implementation is configured.
    Unavailable,
    /// The local desktop user manages optional components.
    DesktopManaged,
    /// A server administrator provisions components outside Procyon.
    AdministratorProvisioned,
    /// An in-process mock provides deterministic lifecycle behavior.
    DeterministicMock,
}

/// Explicit semantic component operation advertised by the active capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum SemanticComponentOperationDto {
    ViewStatus,
    ViewCatalog,
    CreateInstallationOffer,
    InstallOrEnable,
    InstallWorkerPatch,
    PauseIndexing,
    ResumeIndexing,
    RemoveIndex,
    MoveData,
    UninstallComponents,
    ImportLocalModel,
    PlanModelMigration,
    ConfirmModelMigration,
    CheckpointModelMigration,
    CompleteModelMigration,
}

/// Policy governing executable semantic component downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticRuntimeExecutableDownloadDto {
    /// No component implementation is configured.
    Unavailable,
    /// A directly distributed desktop build may download signed executables.
    DirectDistribution,
    /// A Mac App Store build cannot download executable code.
    ProhibitedByMacAppStore,
    /// Executables are installed by a server administrator.
    AdministratorProvisioned,
    /// A mock runtime simulates downloads without filesystem access.
    Simulated,
}

/// Authority and operations available for semantic component management.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticComponentCapabilitiesDto {
    /// Principal that owns component lifecycle changes.
    pub authority: SemanticComponentAuthorityDto,
    /// Explicit operations supported by this capability.
    pub operations: Vec<SemanticComponentOperationDto>,
    /// Executable-download policy for the current distribution.
    pub runtime_executable_download: SemanticRuntimeExecutableDownloadDto,
}

/// Abstract model profile selected independently of a concrete model revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticProfileDto {
    /// Recommended compact multilingual model profile.
    CompactMultilingual,
    /// Compact English-only model profile.
    CompactEnglish,
    /// Larger multilingual model profile prioritising retrieval quality.
    MultilingualQuality,
}

/// Required handling of index data during component uninstall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticIndexRetentionDecisionDto {
    /// Keep index data for a later reinstall.
    Retain,
    /// Delete extracted, vector, and embedding-cache data.
    Delete,
}

/// Exact immutable identity of a model revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticModelIdentityDto {
    /// Stable model identifier.
    pub model_id: String,
    /// Immutable upstream revision.
    pub revision: String,
}

/// Abstract profile plus the exact model revision used by an index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticModelSelectionDto {
    /// Persisted abstract profile.
    pub profile: SemanticProfileDto,
    /// Exact immutable model identity.
    pub identity: SemanticModelIdentityDto,
}

/// Estimated work disclosed before a full reindex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticReindexEstimateDto {
    /// Estimated document count.
    pub documents: u64,
    /// Estimated source bytes.
    pub source_bytes: u64,
}

/// Reason a model migration requires a full reindex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "reason",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SemanticModelMigrationReasonDto {
    /// The model identity or embedding contract changes.
    ModelChanged,
    /// The persisted index schema changes.
    #[schema(rename_all = "camelCase")]
    SchemaChanged {
        /// Existing schema version.
        from_version: u32,
        /// Replacement schema version.
        to_version: u32,
    },
}

/// Durable, resumable model migration progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticModelMigrationProgressDto {
    /// Opaque migration identity.
    pub migration_id: String,
    /// Documents durably reindexed.
    pub completed_documents: u64,
    /// Total work estimate disclosed before confirmation.
    pub estimate: SemanticReindexEstimateDto,
    /// Exact target model selection.
    pub target: SemanticModelSelectionDto,
    /// Reason a full reindex is required.
    pub reason: SemanticModelMigrationReasonDto,
    /// Opaque resume cursor, when one has been persisted.
    pub resume_cursor: Option<String>,
}

/// Observable lifecycle of optional semantic components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SemanticComponentLifecycleDto {
    /// No component implementation is configured.
    Unavailable,
    /// Components are not installed.
    Absent,
    /// A complete installation disclosure awaits explicit consent.
    #[schema(rename_all = "camelCase")]
    Offered {
        /// Opaque offer identity.
        offer_id: String,
    },
    /// A resumable component download is in progress or interrupted.
    #[schema(rename_all = "camelCase")]
    Downloading {
        /// Durable bytes already downloaded.
        downloaded_bytes: u64,
        /// Total signed download size.
        total_bytes: u64,
        /// Whether the retained partial download can be resumed.
        resumable: bool,
    },
    /// Components are installed and indexing is enabled.
    InstalledEnabled,
    /// Indexing is explicitly paused.
    Paused,
    /// A confirmed full reindex is in progress.
    #[schema(rename_all = "camelCase")]
    Migrating {
        /// Durable migration state.
        progress: SemanticModelMigrationProgressDto,
    },
    /// A failed worker update retained or restored the last working version.
    #[schema(rename_all = "camelCase")]
    UpdateFailedRolledBack {
        /// Version whose activation failed.
        failed_version: String,
        /// Last known working active version.
        active_version: String,
    },
    /// Installation cannot proceed until disk space is freed.
    #[schema(rename_all = "camelCase")]
    LowDisk {
        /// Bytes currently available.
        available_bytes: u64,
        /// Bytes required including the configured reserve.
        required_bytes: u64,
    },
    /// Components were removed with an explicit index-data decision.
    #[schema(rename_all = "camelCase")]
    Uninstalled {
        /// Decision applied to remaining index data.
        index_decision: SemanticIndexRetentionDecisionDto,
    },
}

/// Stable semantic-data disk-use category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticDataCategoryDto {
    /// Signed catalog state and resumable downloads.
    Catalog,
    /// Extracted text and structured content.
    Extracted,
    /// Zvec index data.
    Zvec,
    /// Reusable embedding cache.
    EmbeddingCache,
    /// Installed model packages.
    Models,
    /// Versioned worker and runtime bundles.
    Workers,
}

/// Actual disk use for one semantic-data category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticCategoryDiskUseDto {
    /// Measured category.
    pub category: SemanticDataCategoryDto,
    /// Actual occupied bytes.
    pub bytes: u64,
}

/// Stable per-category and aggregate semantic disk use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticDiskUseDto {
    /// Every known category in stable presentation order.
    pub categories: Vec<SemanticCategoryDiskUseDto>,
    /// Aggregate occupied bytes.
    pub total_bytes: u64,
}

/// Installed semantic component role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticComponentKindDto {
    /// Isolated semantic worker executable.
    Worker,
    /// Embedding runtime executable or library.
    Runtime,
    /// Model package.
    Model,
}

/// Whether an installed component is active or retained for rollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum InstalledSemanticComponentStateDto {
    /// Active installation.
    Active,
    /// Previous working worker retained for rollback.
    Rollback,
}

/// One installed semantic component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstalledSemanticComponentDto {
    /// Signed artifact identifier.
    pub artifact_id: String,
    /// Logical component identifier.
    pub component_id: String,
    /// Component role.
    pub kind: SemanticComponentKindDto,
    /// Exact installed version.
    pub version: String,
    /// Active or rollback state.
    pub state: InstalledSemanticComponentStateDto,
    /// Actual installed bytes.
    pub installed_bytes: u64,
}

/// Complete observable semantic component state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticComponentStatusDto {
    /// Current explicit lifecycle state.
    pub lifecycle: SemanticComponentLifecycleDto,
    /// Configured semantic-data root, when reportable.
    pub data_root: Option<String>,
    /// Active profile and exact model revision.
    pub active_model: Option<SemanticModelSelectionDto>,
    /// Resumable migration state, when a full reindex is pending.
    pub migration: Option<SemanticModelMigrationProgressDto>,
    /// Installed active and rollback components.
    pub components: Vec<InstalledSemanticComponentDto>,
    /// Stable per-category disk usage.
    pub disk_use: SemanticDiskUseDto,
}

/// Embedding normalization contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SemanticEmbeddingNormalizationDto {
    /// Embeddings have unit L2 length.
    UnitLength,
    /// Embeddings retain runtime magnitudes.
    None,
}

/// Model license disclosed before installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticLicenseDto {
    /// SPDX license expression.
    pub spdx: String,
    /// Human-readable attribution or notice.
    pub notice: String,
}

/// Complete immutable metadata for one model revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticModelMetadataDto {
    /// Exact model identity.
    pub identity: SemanticModelIdentityDto,
    /// Model license.
    pub license: SemanticLicenseDto,
    /// Exact tokenizer identity.
    pub tokenizer: String,
    /// Embedding vector dimensions.
    pub dimensions: u32,
    /// Stored embedding normalization.
    pub normalization: SemanticEmbeddingNormalizationDto,
    /// Required logical runtime component.
    pub runtime_component_id: String,
    /// Accepted runtime versions.
    pub runtime_version_requirement: String,
    /// Declared language coverage.
    pub language_coverage: Vec<String>,
    /// Estimated installed model bytes.
    pub estimated_disk_bytes: u64,
    /// Estimated peak RAM bytes.
    pub estimated_ram_bytes: u64,
}

/// Curated model profile and its exact signed-catalog resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticModelProfileDto {
    /// Abstract user-facing profile.
    pub profile: SemanticProfileDto,
    /// Whether setup recommends this profile.
    pub recommended: bool,
    /// Stable explanation of the profile trade-off.
    pub explanation: String,
    /// Exact model identity resolved by the signed catalog.
    pub resolved_model: SemanticModelIdentityDto,
    /// Complete immutable model metadata.
    pub metadata: SemanticModelMetadataDto,
}

/// One component disclosed before installation consent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticComponentDisclosureDto {
    /// Signed artifact identifier.
    pub artifact_id: String,
    /// Logical component identifier.
    pub component_id: String,
    /// Component role.
    pub kind: SemanticComponentKindDto,
    /// Exact model identity for a model package.
    pub model: Option<SemanticModelIdentityDto>,
    /// Exact signed component version.
    pub version: String,
    /// License displayed before consent.
    pub license: SemanticLicenseDto,
    /// Compressed download bytes.
    pub download_bytes: u64,
    /// Estimated installed bytes.
    pub estimated_installed_bytes: u64,
    /// Estimated peak RAM bytes.
    pub estimated_ram_bytes: u64,
}

/// Complete signed first-install disclosure awaiting explicit consent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticInstallationOfferDto {
    /// Opaque identity accepted by the installation action.
    pub offer_id: String,
    /// Immutable signed catalog revision.
    pub catalog_revision: String,
    /// Selected abstract profile.
    pub profile: SemanticProfileDto,
    /// Exact model revision resolved by the catalog.
    pub resolved_model: SemanticModelIdentityDto,
    /// Worker, runtime, and model disclosures.
    pub components: Vec<SemanticComponentDisclosureDto>,
    /// Whether inference and semantic data remain on this device.
    pub embeddings_stay_local: bool,
    /// Stable local-only privacy disclosure.
    pub local_only_disclosure: String,
    /// Proposed semantic-data root.
    pub data_root: String,
    /// Free bytes reserved beyond installed-size estimates.
    pub minimum_free_space_reserve_bytes: u64,
}

/// Successful install-or-enable result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticInstallReceiptDto {
    /// Signed artifacts installed or verified as already installed.
    pub installed_artifact_ids: Vec<String>,
}

/// Result of checking for and applying a compatible worker patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticWorkerPatchResponseDto {
    /// Installation receipt, or `None` when no compatible patch is available.
    pub receipt: Option<SemanticInstallReceiptDto>,
}

/// Successful component uninstall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticUninstallReceiptDto {
    /// Explicit decision applied to remaining index data.
    pub index_decision: SemanticIndexRetentionDecisionDto,
    /// Active and rollback component records removed.
    pub removed_component_count: u64,
}

/// Expected or removed enrolment-derived records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticIndexRecordCountsDto {
    /// Index records and enrolment metadata.
    pub index_records: u64,
    /// Extracted content files.
    pub extracted_files: u64,
    /// Zvec vectors.
    pub zvec_vectors: u64,
    /// Embedding-cache records.
    pub cache_entries: u64,
    /// Saved-conversation evidence records.
    pub conversation_evidence: u64,
}

/// Creates an authoritative semantic-index removal plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSemanticIndexRemovalPlanRequestDto {
    /// Opaque enrolment identity to inventory.
    pub enrolment_id: String,
}

/// Authoritative deletion inventory awaiting explicit confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticIndexRemovalPlanDto {
    /// Opaque identity of the live removal plan.
    pub plan_id: String,
    /// Opaque enrolment identity.
    pub enrolment_id: String,
    /// Authoritative records that confirmation will delete.
    pub expected: SemanticIndexRecordCountsDto,
}

/// Confirms one live authoritative semantic-index removal plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmSemanticIndexRemovalRequestDto {
    /// Opaque identity returned by the planning request.
    pub plan_id: String,
}

/// Verified enrolment-derived data removal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticIndexRemovalReceiptDto {
    /// Opaque removed enrolment identity.
    pub enrolment_id: String,
    /// Actual records removed by category.
    pub deleted: SemanticIndexRecordCountsDto,
    /// Whether saved-conversation evidence was also deleted.
    pub conversation_evidence_deleted: bool,
}

/// Successful verified semantic-data root move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticDataMoveReceiptDto {
    /// Retained source root.
    pub source: String,
    /// New active root.
    pub destination: String,
    /// Files independently verified.
    pub verified_file_count: u64,
    /// Bytes independently verified.
    pub verified_bytes: u64,
}

/// Confirmation-gated model migration plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticModelMigrationPlanDto {
    /// Opaque plan identity.
    pub migration_id: String,
    /// Current selection captured by the plan.
    pub from: Option<SemanticModelSelectionDto>,
    /// Exact target selection.
    pub target: SemanticModelSelectionDto,
    /// Estimated full-reindex work.
    pub estimate: SemanticReindexEstimateDto,
    /// Why full reindexing is mandatory.
    pub reason: SemanticModelMigrationReasonDto,
    /// Always true: activation requires explicit confirmation.
    pub requires_confirmation: bool,
    /// Always true: all embeddings must be rebuilt.
    pub full_reindex: bool,
    /// Always true: durable checkpoints can resume the work.
    pub resumable: bool,
}

/// Creates a complete installation disclosure for one curated profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSemanticInstallationOfferRequestDto {
    /// Abstract profile to resolve through the signed catalog.
    pub profile: SemanticProfileDto,
}

/// Explicitly accepts one previously returned installation offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptSemanticInstallationOfferRequestDto {
    /// Opaque offer identity; no artifact location is accepted.
    pub offer_id: String,
}

/// Requests the newest compatible signed worker patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallSemanticWorkerPatchRequestDto {
    /// Logical installed worker component.
    pub component_id: String,
}

/// Moves the semantic-data root through pause-copy-verify-switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MoveSemanticDataRequestDto {
    /// Native desktop path. Browser/server runtimes reject this request.
    pub destination: String,
}

/// Uninstalls components with an explicit index-data decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UninstallSemanticComponentsRequestDto {
    /// Required handling of remaining index data.
    pub index_decision: SemanticIndexRetentionDecisionDto,
}

/// Expert-mode local model import plus its mandatory migration disclosure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportSemanticLocalModelRequestDto {
    /// Existing local model package path. Browser/server runtimes reject it.
    pub source_path: String,
    /// Opaque model identifier.
    pub model_id: String,
    /// Exact immutable upstream revision.
    pub upstream_revision: String,
    /// SPDX license expression.
    pub license_spdx: String,
    /// Human-readable license notice.
    pub license_notice: String,
    /// Exact tokenizer identifier.
    pub tokenizer: String,
    /// Embedding dimensions.
    pub dimensions: u32,
    /// Required normalization metadata.
    pub normalization: Option<SemanticEmbeddingNormalizationDto>,
    /// Required logical runtime component.
    pub runtime_component_id: String,
    /// Accepted runtime semantic versions.
    pub runtime_version_requirement: String,
    /// Declared language coverage.
    pub language_coverage: Vec<String>,
    /// Estimated installed disk bytes.
    pub estimated_disk_bytes: u64,
    /// Estimated peak RAM bytes.
    pub estimated_ram_bytes: u64,
    /// Abstract profile assigned to the imported model.
    pub profile: SemanticProfileDto,
    /// Full-reindex work disclosed before confirmation.
    pub estimate: SemanticReindexEstimateDto,
}

/// Plans migration to a signed-catalog model resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanSemanticModelMigrationRequestDto {
    /// Target abstract profile.
    pub profile: SemanticProfileDto,
    /// Full-reindex work disclosed before confirmation.
    pub estimate: SemanticReindexEstimateDto,
}

/// Explicitly confirms and starts one returned model migration plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmSemanticModelMigrationRequestDto {
    /// Opaque migration plan identity.
    pub migration_id: String,
}

/// Persists a resumable model migration checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointSemanticModelMigrationRequestDto {
    /// Migration being advanced.
    pub migration_id: String,
    /// Completely reindexed documents.
    pub completed_documents: u64,
    /// Opaque resume cursor.
    pub resume_cursor: Option<String>,
}

/// Activates a fully reindexed model migration target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompleteSemanticModelMigrationRequestDto {
    /// Migration to complete.
    pub migration_id: String,
}

/// Stable machine-readable semantic component failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum SemanticComponentErrorCodeDto {
    Unavailable,
    AuthorityDenied,
    ConsentRequired,
    InvalidLocalModelMetadata,
    InvalidLifecycle,
    InvalidEnrolment,
    InvalidMigrationPlan,
    InvalidMigrationProgress,
    ExecutableDownloadProhibited,
    Catalog,
    InsufficientSpace,
    DownloadInterrupted,
    ArtifactVerificationFailed,
    ActivationFailed,
    FreeSpaceProbe,
    State,
    Filesystem,
    Indexing,
    IndexRemoval,
    DataMigration,
    BlockingTaskFailed,
    DiskUseOverflow,
}

/// Required expert local-model metadata field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum SemanticModelImportFieldDto {
    SourcePath,
    ModelId,
    UpstreamRevision,
    License,
    Tokenizer,
    Dimensions,
    Normalization,
    RuntimeCompatibility,
    LanguageCoverage,
    EstimatedDiskBytes,
    EstimatedRamBytes,
}

/// Typed context attached to an actionable semantic component failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SemanticComponentErrorDetailsDto {
    /// A read-only authority rejected a mutation.
    #[schema(rename_all = "camelCase")]
    AuthorityDenied {
        /// Active authority.
        authority: SemanticComponentAuthorityDto,
        /// Rejected operation.
        operation: SemanticComponentOperationDto,
    },
    /// Expert local-model metadata was invalid.
    #[schema(rename_all = "camelCase")]
    InvalidLocalModelMetadata {
        /// Invalid required field.
        field: SemanticModelImportFieldDto,
    },
    /// An action was invalid for the current lifecycle state.
    #[schema(rename_all = "camelCase")]
    InvalidLifecycle {
        /// Rejected operation.
        operation: SemanticComponentOperationDto,
        /// Human-readable current lifecycle.
        lifecycle: String,
    },
    /// A checkpoint regressed or exceeded the disclosed estimate.
    #[schema(rename_all = "camelCase")]
    InvalidMigrationProgress {
        /// Rejected completed document count.
        completed_documents: u64,
        /// Disclosed total document estimate.
        total_documents: u64,
    },
    /// Available storage cannot satisfy the estimate and reserve.
    #[schema(rename_all = "camelCase")]
    InsufficientSpace {
        /// Bytes currently available.
        available_bytes: u64,
        /// Required installed bytes including the reserve.
        required_bytes: u64,
        /// Explicit reserve included in the requirement.
        reserve_bytes: u64,
    },
    /// A signed artifact-specific failure.
    #[schema(rename_all = "camelCase")]
    Artifact {
        /// Signed artifact identity.
        artifact_id: String,
    },
}

/// Structured semantic component error shared by HTTP and Tauri.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SemanticComponentErrorDto {
    /// Stable machine-readable failure code.
    pub code: SemanticComponentErrorCodeDto,
    /// Sanitized user-readable explanation.
    pub message: String,
    /// Correlates the failure with one HTTP or Tauri request.
    pub request_id: Uuid,
    /// Typed actionable context, when applicable.
    pub details: Option<SemanticComponentErrorDetailsDto>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn expected_keys<const N: usize>(keys: [&str; N]) -> BTreeSet<String> {
        keys.into_iter().map(str::to_owned).collect()
    }

    fn serialized_property_keys<T: Serialize>(values: &[T]) -> BTreeSet<String> {
        values
            .iter()
            .flat_map(|value| {
                serde_json::to_value(value)
                    .expect("serialize DTO")
                    .as_object()
                    .expect("DTO serializes as an object")
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn schema_property_keys<T: utoipa::PartialSchema>() -> BTreeSet<String> {
        let schema = serde_json::to_value(T::schema()).expect("serialize DTO schema");
        schema["oneOf"]
            .as_array()
            .expect("enum schema uses oneOf")
            .iter()
            .flat_map(|variant| {
                variant["properties"]
                    .as_object()
                    .expect("enum variant schema has properties")
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn migration_progress() -> SemanticModelMigrationProgressDto {
        SemanticModelMigrationProgressDto {
            migration_id: "migration-1".to_owned(),
            completed_documents: 1,
            estimate: SemanticReindexEstimateDto {
                documents: 2,
                source_bytes: 3,
            },
            target: SemanticModelSelectionDto {
                profile: SemanticProfileDto::CompactMultilingual,
                identity: SemanticModelIdentityDto {
                    model_id: "model-1".to_owned(),
                    revision: "revision-1".to_owned(),
                },
            },
            reason: SemanticModelMigrationReasonDto::ModelChanged,
            resume_cursor: None,
        }
    }

    #[test]
    fn semantic_component_capabilities_use_transport_discriminators() {
        let dto = SemanticComponentCapabilitiesDto {
            authority: SemanticComponentAuthorityDto::DesktopManaged,
            operations: vec![
                SemanticComponentOperationDto::ViewStatus,
                SemanticComponentOperationDto::InstallOrEnable,
            ],
            runtime_executable_download: SemanticRuntimeExecutableDownloadDto::DirectDistribution,
        };

        let value = serde_json::to_value(dto).expect("serialize semantic capabilities");

        assert_eq!(value["authority"], "desktopManaged");
        assert_eq!(value["operations"][1], "installOrEnable");
        assert_eq!(value["runtimeExecutableDownload"], "directDistribution");
    }

    #[test]
    fn lifecycle_variants_are_explicitly_tagged_and_round_trip() {
        let lifecycle = SemanticComponentLifecycleDto::Downloading {
            downloaded_bytes: 40,
            total_bytes: 100,
            resumable: true,
        };

        let value = serde_json::to_value(&lifecycle).expect("serialize lifecycle");
        assert_eq!(value["state"], "downloading");
        assert_eq!(value["downloadedBytes"], 40);
        assert_eq!(
            serde_json::from_value::<SemanticComponentLifecycleDto>(value)
                .expect("deserialize lifecycle"),
            lifecycle
        );
    }

    #[test]
    fn struct_variant_json_fields_use_camel_case() {
        assert_eq!(
            serialized_property_keys(&[SemanticModelMigrationReasonDto::SchemaChanged {
                from_version: 1,
                to_version: 2,
            }]),
            expected_keys(["reason", "fromVersion", "toVersion"])
        );

        assert_eq!(
            serialized_property_keys(&[
                SemanticComponentLifecycleDto::Offered {
                    offer_id: "offer-1".to_owned(),
                },
                SemanticComponentLifecycleDto::Downloading {
                    downloaded_bytes: 1,
                    total_bytes: 2,
                    resumable: true,
                },
                SemanticComponentLifecycleDto::Migrating {
                    progress: migration_progress(),
                },
                SemanticComponentLifecycleDto::UpdateFailedRolledBack {
                    failed_version: "2".to_owned(),
                    active_version: "1".to_owned(),
                },
                SemanticComponentLifecycleDto::LowDisk {
                    available_bytes: 1,
                    required_bytes: 2,
                },
                SemanticComponentLifecycleDto::Uninstalled {
                    index_decision: SemanticIndexRetentionDecisionDto::Retain,
                },
            ]),
            expected_keys([
                "state",
                "offerId",
                "downloadedBytes",
                "totalBytes",
                "resumable",
                "progress",
                "failedVersion",
                "activeVersion",
                "availableBytes",
                "requiredBytes",
                "indexDecision",
            ])
        );

        assert_eq!(
            serialized_property_keys(&[
                SemanticComponentErrorDetailsDto::AuthorityDenied {
                    authority: SemanticComponentAuthorityDto::DesktopManaged,
                    operation: SemanticComponentOperationDto::InstallOrEnable,
                },
                SemanticComponentErrorDetailsDto::InvalidLocalModelMetadata {
                    field: SemanticModelImportFieldDto::ModelId,
                },
                SemanticComponentErrorDetailsDto::InvalidLifecycle {
                    operation: SemanticComponentOperationDto::PauseIndexing,
                    lifecycle: "absent".to_owned(),
                },
                SemanticComponentErrorDetailsDto::InvalidMigrationProgress {
                    completed_documents: 2,
                    total_documents: 1,
                },
                SemanticComponentErrorDetailsDto::InsufficientSpace {
                    available_bytes: 1,
                    required_bytes: 2,
                    reserve_bytes: 1,
                },
                SemanticComponentErrorDetailsDto::Artifact {
                    artifact_id: "artifact-1".to_owned(),
                },
            ]),
            expected_keys([
                "kind",
                "authority",
                "operation",
                "field",
                "lifecycle",
                "completedDocuments",
                "totalDocuments",
                "availableBytes",
                "requiredBytes",
                "reserveBytes",
                "artifactId",
            ])
        );
    }

    #[test]
    fn struct_variant_schema_fields_use_camel_case() {
        assert_eq!(
            schema_property_keys::<SemanticModelMigrationReasonDto>(),
            expected_keys(["reason", "fromVersion", "toVersion"])
        );
        assert_eq!(
            schema_property_keys::<SemanticComponentLifecycleDto>(),
            expected_keys([
                "state",
                "offerId",
                "downloadedBytes",
                "totalBytes",
                "resumable",
                "progress",
                "failedVersion",
                "activeVersion",
                "availableBytes",
                "requiredBytes",
                "indexDecision",
            ])
        );
        assert_eq!(
            schema_property_keys::<SemanticComponentErrorDetailsDto>(),
            expected_keys([
                "kind",
                "authority",
                "operation",
                "field",
                "lifecycle",
                "completedDocuments",
                "totalDocuments",
                "availableBytes",
                "requiredBytes",
                "reserveBytes",
                "artifactId",
            ])
        );
    }

    #[test]
    fn installation_requests_cannot_smuggle_artifact_urls() {
        let request = serde_json::json!({
            "offerId": "offer-1",
            "artifactUrl": "https://attacker.invalid/worker"
        });

        assert!(
            serde_json::from_value::<AcceptSemanticInstallationOfferRequestDto>(request).is_err()
        );
    }

    #[test]
    fn worker_patch_request_contains_only_the_component_identity() {
        let request = InstallSemanticWorkerPatchRequestDto {
            component_id: "worker".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(request).expect("serialize worker patch request"),
            serde_json::json!({"componentId": "worker"})
        );
    }

    #[test]
    fn index_removal_plan_requests_use_opaque_camel_case_plan_ids() {
        let create = CreateSemanticIndexRemovalPlanRequestDto {
            enrolment_id: "enrolment-1".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(create).expect("serialize plan request"),
            serde_json::json!({"enrolmentId": "enrolment-1"})
        );

        let plan = SemanticIndexRemovalPlanDto {
            plan_id: "plan-1".to_owned(),
            enrolment_id: "enrolment-1".to_owned(),
            expected: SemanticIndexRecordCountsDto {
                index_records: 1,
                extracted_files: 2,
                zvec_vectors: 3,
                cache_entries: 4,
                conversation_evidence: 5,
            },
        };
        let value = serde_json::to_value(plan).expect("serialize removal plan");
        assert_eq!(value["planId"], "plan-1");
        assert_eq!(value["enrolmentId"], "enrolment-1");
        assert_eq!(value["expected"]["conversationEvidence"], 5);

        let confirm = ConfirmSemanticIndexRemovalRequestDto {
            plan_id: "plan-1".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(confirm).expect("serialize confirmation"),
            serde_json::json!({"planId": "plan-1"})
        );
        assert!(
            serde_json::from_value::<ConfirmSemanticIndexRemovalRequestDto>(serde_json::json!({
                "planId": "plan-1",
                "expected": {}
            }))
            .is_err()
        );
    }

    #[test]
    fn semantic_errors_round_trip_with_typed_actionable_details() {
        let error = SemanticComponentErrorDto {
            code: SemanticComponentErrorCodeDto::InsufficientSpace,
            message: "insufficient free space".to_owned(),
            request_id: Uuid::nil(),
            details: Some(SemanticComponentErrorDetailsDto::InsufficientSpace {
                available_bytes: 10,
                required_bytes: 20,
                reserve_bytes: 5,
            }),
        };

        let json = serde_json::to_string(&error).expect("serialize error");
        let decoded: SemanticComponentErrorDto =
            serde_json::from_str(&json).expect("deserialize error");
        assert_eq!(decoded, error);
    }
}
