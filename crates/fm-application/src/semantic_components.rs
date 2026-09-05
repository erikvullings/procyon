//! Host-neutral lifecycle boundary for optional semantic components.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fm_semantic_components as core;
use uuid::Uuid;

pub use fm_semantic_components::SemanticProfile;

/// Principal allowed to manage semantic components in the current host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticComponentAuthority {
    /// No semantic component implementation was configured.
    Unavailable,
    /// The local desktop user manages optional components.
    DesktopManaged,
    /// Components are provisioned outside Procyon by a server administrator.
    AdministratorProvisioned,
    /// Deterministic in-process mock authority.
    DeterministicMock,
}

/// Explicit semantic component operation advertised by a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemanticComponentOperation {
    /// Read component and disk-use status.
    ViewStatus,
    /// Read curated profile and model metadata.
    ViewCatalog,
    /// Create a complete disclosure before installation consent.
    CreateInstallationOffer,
    /// Install the offered components or enable an existing installation.
    InstallOrEnable,
    /// Install an eligible signed worker patch without changing embedding space.
    InstallWorkerPatch,
    /// Pause indexing without uninstalling components.
    PauseIndexing,
    /// Resume explicitly paused indexing.
    ResumeIndexing,
    /// Remove all data derived from one enrolment.
    RemoveIndex,
    /// Move the semantic-data root with verification.
    MoveData,
    /// Uninstall components with an explicit index decision.
    UninstallComponents,
    /// Validate a local model and create a migration plan.
    ImportLocalModel,
    /// Plan a curated model migration.
    PlanModelMigration,
    /// Confirm and begin a model migration.
    ConfirmModelMigration,
    /// Persist a resumable model migration checkpoint.
    CheckpointModelMigration,
    /// Complete a fully reindexed model migration.
    CompleteModelMigration,
}

/// Whether this distribution may download executable semantic components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeExecutableDownload {
    /// No component implementation is available.
    Unavailable,
    /// Executables may be downloaded by a direct desktop distribution.
    DirectDistribution,
    /// Executable downloads are prohibited in a Mac App Store build.
    ProhibitedByMacAppStore,
    /// Executables are provisioned by a server administrator.
    AdministratorProvisioned,
    /// Executable downloads are simulated without filesystem access.
    Simulated,
}

/// Operations and executable-download policy exposed by the active capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticComponentCapabilities {
    authority: SemanticComponentAuthority,
    operations: Vec<SemanticComponentOperation>,
    runtime_executable_download: RuntimeExecutableDownload,
}

impl SemanticComponentCapabilities {
    /// Creates a capability report and normalizes duplicate operations.
    #[must_use]
    pub fn new(
        authority: SemanticComponentAuthority,
        mut operations: Vec<SemanticComponentOperation>,
        runtime_executable_download: RuntimeExecutableDownload,
    ) -> Self {
        operations.sort_unstable();
        operations.dedup();
        Self {
            authority,
            operations,
            runtime_executable_download,
        }
    }

    /// Returns who owns component lifecycle mutations.
    #[must_use]
    pub const fn authority(&self) -> SemanticComponentAuthority {
        self.authority
    }

    /// Returns supported operations in stable presentation order.
    #[must_use]
    pub fn operations(&self) -> &[SemanticComponentOperation] {
        &self.operations
    }

    /// Returns the executable-download policy for this distribution.
    #[must_use]
    pub const fn runtime_executable_download(&self) -> RuntimeExecutableDownload {
        self.runtime_executable_download
    }

    /// Reports whether one explicit operation is supported.
    #[must_use]
    pub fn supports(&self, operation: SemanticComponentOperation) -> bool {
        self.operations.contains(&operation)
    }

    fn unavailable() -> Self {
        Self::new(
            SemanticComponentAuthority::Unavailable,
            Vec::new(),
            RuntimeExecutableDownload::Unavailable,
        )
    }

    pub(crate) fn administrator_provisioned() -> Self {
        Self::new(
            SemanticComponentAuthority::AdministratorProvisioned,
            vec![
                SemanticComponentOperation::ViewStatus,
                SemanticComponentOperation::ViewCatalog,
            ],
            RuntimeExecutableDownload::AdministratorProvisioned,
        )
    }

    fn deterministic_mock() -> Self {
        Self::new(
            SemanticComponentAuthority::DeterministicMock,
            vec![
                SemanticComponentOperation::ViewStatus,
                SemanticComponentOperation::ViewCatalog,
                SemanticComponentOperation::CreateInstallationOffer,
                SemanticComponentOperation::InstallOrEnable,
                SemanticComponentOperation::InstallWorkerPatch,
                SemanticComponentOperation::PauseIndexing,
                SemanticComponentOperation::ResumeIndexing,
                SemanticComponentOperation::RemoveIndex,
                SemanticComponentOperation::MoveData,
                SemanticComponentOperation::UninstallComponents,
                SemanticComponentOperation::ImportLocalModel,
                SemanticComponentOperation::PlanModelMigration,
                SemanticComponentOperation::ConfirmModelMigration,
                SemanticComponentOperation::CheckpointModelMigration,
                SemanticComponentOperation::CompleteModelMigration,
            ],
            RuntimeExecutableDownload::Simulated,
        )
    }

    fn desktop_managed(distribution: DesktopSemanticDistribution, can_remove_index: bool) -> Self {
        let mut operations = vec![
            SemanticComponentOperation::ViewStatus,
            SemanticComponentOperation::ViewCatalog,
            SemanticComponentOperation::CreateInstallationOffer,
            SemanticComponentOperation::InstallOrEnable,
            SemanticComponentOperation::InstallWorkerPatch,
            SemanticComponentOperation::PauseIndexing,
            SemanticComponentOperation::ResumeIndexing,
            SemanticComponentOperation::MoveData,
            SemanticComponentOperation::UninstallComponents,
            SemanticComponentOperation::ImportLocalModel,
            SemanticComponentOperation::PlanModelMigration,
            SemanticComponentOperation::ConfirmModelMigration,
        ];
        if can_remove_index {
            operations.push(SemanticComponentOperation::RemoveIndex);
        }
        Self::new(
            SemanticComponentAuthority::DesktopManaged,
            operations,
            match distribution {
                DesktopSemanticDistribution::Direct => {
                    RuntimeExecutableDownload::DirectDistribution
                }
                DesktopSemanticDistribution::MacAppStore => {
                    RuntimeExecutableDownload::ProhibitedByMacAppStore
                }
            },
        )
    }
}

/// Desktop distribution policy for runtime-downloaded executable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopSemanticDistribution {
    /// Directly distributed desktop build that may install signed executables.
    Direct,
    /// Sandboxed Mac App Store build that may not download executable code.
    MacAppStore,
}

/// Observable lifecycle of optional semantic components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticComponentLifecycle {
    /// No component implementation was configured.
    Unavailable,
    /// Components are not installed.
    Absent,
    /// A signed installation disclosure is awaiting explicit consent.
    Offered {
        /// Opaque identity of the disclosed offer.
        offer_id: InstallationOfferId,
    },
    /// A resumable component download is in progress or interrupted.
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
    /// Indexing is explicitly paused while components remain installed.
    Paused,
    /// A confirmed full reindex is in progress.
    Migrating {
        /// Durable migration checkpoint.
        progress: SemanticModelMigrationProgress,
    },
    /// A failed update retained or restored the last working component.
    UpdateFailedRolledBack {
        /// Version whose activation failed.
        failed_version: String,
        /// Last known working version that remains active.
        active_version: String,
    },
    /// Installation cannot proceed without freeing disk space.
    LowDisk {
        /// Bytes currently available.
        available_bytes: u64,
        /// Bytes required including the reserve.
        required_bytes: u64,
    },
    /// Components were removed with an explicit index retention decision.
    Uninstalled {
        /// Decision applied to remaining index data.
        index_decision: SemanticIndexRetentionDecision,
    },
}

/// Semantic data category reported independently for stable disk accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SemanticDataCategory {
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

impl SemanticDataCategory {
    /// Returns all categories in stable presentation order.
    #[must_use]
    pub const fn all() -> &'static [Self; 6] {
        &[
            Self::Catalog,
            Self::Extracted,
            Self::Zvec,
            Self::EmbeddingCache,
            Self::Models,
            Self::Workers,
        ]
    }
}

/// Actual disk use for one semantic data category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticCategoryDiskUse {
    category: SemanticDataCategory,
    bytes: u64,
}

impl SemanticCategoryDiskUse {
    /// Creates one category measurement.
    #[must_use]
    pub const fn new(category: SemanticDataCategory, bytes: u64) -> Self {
        Self { category, bytes }
    }

    /// Returns the measured category.
    #[must_use]
    pub const fn category(self) -> SemanticDataCategory {
        self.category
    }

    /// Returns actual occupied bytes.
    #[must_use]
    pub const fn bytes(self) -> u64 {
        self.bytes
    }
}

/// Stable per-category and aggregate semantic disk use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticDiskUse {
    categories: Vec<SemanticCategoryDiskUse>,
    total_bytes: u64,
}

impl SemanticDiskUse {
    /// Creates a zero-byte report containing every known category.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            categories: SemanticDataCategory::all()
                .iter()
                .copied()
                .map(|category| SemanticCategoryDiskUse::new(category, 0))
                .collect(),
            total_bytes: 0,
        }
    }

    /// Normalizes arbitrary measurements into stable category order.
    ///
    /// Missing categories are reported as zero. Repeated categories use the
    /// final supplied measurement.
    ///
    /// # Errors
    ///
    /// Returns [`SemanticComponentError::DiskUseOverflow`] if the aggregate
    /// cannot be represented.
    pub fn from_categories(
        measurements: impl IntoIterator<Item = SemanticCategoryDiskUse>,
    ) -> Result<Self, SemanticComponentError> {
        let measured: BTreeMap<SemanticDataCategory, u64> = measurements
            .into_iter()
            .map(|usage| (usage.category, usage.bytes))
            .collect();
        let categories: Vec<_> = SemanticDataCategory::all()
            .iter()
            .copied()
            .map(|category| {
                SemanticCategoryDiskUse::new(
                    category,
                    measured.get(&category).copied().unwrap_or(0),
                )
            })
            .collect();
        let total_bytes = categories
            .iter()
            .try_fold(0_u64, |total, usage| total.checked_add(usage.bytes));
        Ok(Self {
            categories,
            total_bytes: total_bytes.ok_or(SemanticComponentError::DiskUseOverflow)?,
        })
    }

    /// Returns every category in stable order, including zero-byte categories.
    #[must_use]
    pub fn categories(&self) -> &[SemanticCategoryDiskUse] {
        &self.categories
    }

    /// Returns aggregate bytes across all categories.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
}

/// Exact immutable model identity selected for an index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticModelIdentity {
    model_id: String,
    revision: String,
}

impl SemanticModelIdentity {
    /// Creates an application-level exact model identity.
    #[must_use]
    pub fn new(model_id: impl Into<String>, revision: impl Into<String>) -> Self {
        Self {
            model_id: model_id.into(),
            revision: revision.into(),
        }
    }

    /// Returns the opaque model identifier.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the immutable upstream revision.
    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }
}

/// Abstract profile plus the exact model revision used by an index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticModelSelection {
    profile: SemanticProfile,
    identity: SemanticModelIdentity,
}

impl SemanticModelSelection {
    /// Creates one resolved model selection.
    #[must_use]
    pub const fn new(profile: SemanticProfile, identity: SemanticModelIdentity) -> Self {
        Self { profile, identity }
    }

    /// Returns the persisted abstract profile.
    #[must_use]
    pub const fn profile(&self) -> SemanticProfile {
        self.profile
    }

    /// Returns the exact model identity used by the index.
    #[must_use]
    pub const fn identity(&self) -> &SemanticModelIdentity {
        &self.identity
    }
}

/// Opaque identity of one explicit model migration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticModelMigrationId(String);

impl SemanticModelMigrationId {
    /// Creates an opaque migration identity supplied by a host adapter.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the opaque identity for correlation and checkpoints.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Estimated work disclosed before model migration confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticReindexEstimate {
    documents: u64,
    source_bytes: u64,
}

impl SemanticReindexEstimate {
    /// Creates an estimated full-reindex workload.
    #[must_use]
    pub const fn new(documents: u64, source_bytes: u64) -> Self {
        Self {
            documents,
            source_bytes,
        }
    }

    /// Returns estimated document count.
    #[must_use]
    pub const fn documents(self) -> u64 {
        self.documents
    }

    /// Returns estimated source bytes.
    #[must_use]
    pub const fn source_bytes(self) -> u64 {
        self.source_bytes
    }
}

/// Durable resumable model migration state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticModelMigrationProgress {
    migration_id: SemanticModelMigrationId,
    completed_documents: u64,
    estimate: SemanticReindexEstimate,
    target: SemanticModelSelection,
    reason: SemanticModelMigrationReason,
    resume_cursor: Option<String>,
}

impl SemanticModelMigrationProgress {
    /// Creates one durable resumable migration checkpoint.
    #[must_use]
    pub fn new(
        migration_id: SemanticModelMigrationId,
        completed_documents: u64,
        estimate: SemanticReindexEstimate,
        target: SemanticModelSelection,
        reason: SemanticModelMigrationReason,
        resume_cursor: Option<String>,
    ) -> Self {
        Self {
            migration_id,
            completed_documents,
            estimate,
            target,
            reason,
            resume_cursor,
        }
    }

    /// Returns the migration being resumed.
    #[must_use]
    pub const fn migration_id(&self) -> &SemanticModelMigrationId {
        &self.migration_id
    }

    /// Returns durably completed documents.
    #[must_use]
    pub const fn completed_documents(&self) -> u64 {
        self.completed_documents
    }

    /// Returns the disclosed total document estimate.
    #[must_use]
    pub const fn total_documents(&self) -> u64 {
        self.estimate.documents()
    }

    /// Returns the full estimated workload disclosed before confirmation.
    #[must_use]
    pub const fn estimate(&self) -> SemanticReindexEstimate {
        self.estimate
    }

    /// Returns the exact model selection being built.
    #[must_use]
    pub const fn target(&self) -> &SemanticModelSelection {
        &self.target
    }

    /// Returns why the full reindex was required.
    #[must_use]
    pub const fn reason(&self) -> SemanticModelMigrationReason {
        self.reason
    }

    /// Returns the opaque durable cursor, when present.
    #[must_use]
    pub fn resume_cursor(&self) -> Option<&str> {
        self.resume_cursor.as_deref()
    }
}

/// Installed component role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticComponentKind {
    /// Isolated semantic worker executable.
    Worker,
    /// Embedding runtime executable or library.
    Runtime,
    /// Model package.
    Model,
}

/// Whether an installed component is active or retained for rollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstalledSemanticComponentState {
    /// Active installation.
    Active,
    /// Previous working worker retained for rollback.
    Rollback,
}

/// One installed semantic component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledSemanticComponent {
    artifact_id: String,
    component_id: String,
    kind: SemanticComponentKind,
    version: String,
    state: InstalledSemanticComponentState,
    installed_bytes: u64,
}

impl InstalledSemanticComponent {
    /// Creates an installed component status entry.
    #[must_use]
    pub fn new(
        artifact_id: impl Into<String>,
        component_id: impl Into<String>,
        kind: SemanticComponentKind,
        version: impl Into<String>,
        state: InstalledSemanticComponentState,
        installed_bytes: u64,
    ) -> Self {
        Self {
            artifact_id: artifact_id.into(),
            component_id: component_id.into(),
            kind,
            version: version.into(),
            state,
            installed_bytes,
        }
    }

    /// Returns the signed artifact identifier.
    #[must_use]
    pub fn artifact_id(&self) -> &str {
        &self.artifact_id
    }

    /// Returns the logical component identifier.
    #[must_use]
    pub fn component_id(&self) -> &str {
        &self.component_id
    }

    /// Returns the component role.
    #[must_use]
    pub const fn kind(&self) -> SemanticComponentKind {
        self.kind
    }

    /// Returns the exact installed version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Returns active or rollback state.
    #[must_use]
    pub const fn state(&self) -> InstalledSemanticComponentState {
        self.state
    }

    /// Returns actual installed bytes.
    #[must_use]
    pub const fn installed_bytes(&self) -> u64 {
        self.installed_bytes
    }
}

/// Current semantic component state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticComponentStatus {
    lifecycle: SemanticComponentLifecycle,
    data_root: Option<PathBuf>,
    active_model: Option<SemanticModelSelection>,
    migration: Option<SemanticModelMigrationProgress>,
    components: Vec<InstalledSemanticComponent>,
    disk_use: SemanticDiskUse,
}

impl SemanticComponentStatus {
    /// Creates a complete status reported by a host implementation.
    #[must_use]
    pub const fn new(
        lifecycle: SemanticComponentLifecycle,
        data_root: Option<PathBuf>,
        active_model: Option<SemanticModelSelection>,
        migration: Option<SemanticModelMigrationProgress>,
        components: Vec<InstalledSemanticComponent>,
        disk_use: SemanticDiskUse,
    ) -> Self {
        Self {
            lifecycle,
            data_root,
            active_model,
            migration,
            components,
            disk_use,
        }
    }

    /// Creates an absent status for a reported configured root.
    #[must_use]
    pub fn absent(data_root: Option<PathBuf>) -> Self {
        Self {
            lifecycle: SemanticComponentLifecycle::Absent,
            data_root,
            active_model: None,
            migration: None,
            components: Vec::new(),
            disk_use: SemanticDiskUse::empty(),
        }
    }

    /// Returns the current explicit lifecycle state.
    #[must_use]
    pub const fn lifecycle(&self) -> &SemanticComponentLifecycle {
        &self.lifecycle
    }

    /// Returns the configured semantic-data root when one is reportable.
    #[must_use]
    pub fn data_root(&self) -> Option<&std::path::Path> {
        self.data_root.as_deref()
    }

    /// Returns the active abstract profile and exact model revision.
    #[must_use]
    pub const fn active_model(&self) -> Option<&SemanticModelSelection> {
        self.active_model.as_ref()
    }

    /// Returns resumable migration state when a full reindex is pending.
    #[must_use]
    pub const fn migration(&self) -> Option<&SemanticModelMigrationProgress> {
        self.migration.as_ref()
    }

    /// Returns installed active and rollback components.
    #[must_use]
    pub fn components(&self) -> &[InstalledSemanticComponent] {
        &self.components
    }

    /// Returns stable per-category disk usage.
    #[must_use]
    pub const fn disk_use(&self) -> &SemanticDiskUse {
        &self.disk_use
    }

    fn unavailable() -> Self {
        Self {
            lifecycle: SemanticComponentLifecycle::Unavailable,
            data_root: None,
            active_model: None,
            migration: None,
            components: Vec::new(),
            disk_use: SemanticDiskUse::empty(),
        }
    }

    fn fake(lifecycle: SemanticComponentLifecycle) -> Self {
        let migration = match &lifecycle {
            SemanticComponentLifecycle::Migrating { progress } => Some(progress.clone()),
            _ => None,
        };
        Self {
            lifecycle,
            data_root: Some(PathBuf::from("mock/semantic")),
            active_model: None,
            migration,
            components: Vec::new(),
            disk_use: SemanticDiskUse::empty(),
        }
    }
}

/// Curated model metadata displayed independently of transport DTOs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticModelProfile {
    /// Abstract user-facing quality and language profile.
    pub profile: SemanticProfile,
    /// Whether setup recommends this profile.
    pub recommended: bool,
    /// Stable explanation of the profile trade-off.
    pub explanation: String,
    /// Exact model identity resolved by the signed catalog.
    pub resolved_model: SemanticModelIdentity,
    /// Complete immutable model metadata.
    pub metadata: SemanticModelMetadata,
}

/// Embedding normalization contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticEmbeddingNormalization {
    /// Embeddings have unit L2 length.
    UnitLength,
    /// Embeddings retain runtime magnitudes.
    None,
}

/// Model license disclosed before installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticLicense {
    /// SPDX license expression.
    pub spdx: String,
    /// Human-readable attribution or notice.
    pub notice: String,
}

/// Complete immutable metadata for one model revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticModelMetadata {
    /// Exact model identity.
    pub identity: SemanticModelIdentity,
    /// Model license.
    pub license: SemanticLicense,
    /// Exact tokenizer identity.
    pub tokenizer: String,
    /// Embedding vector dimensions.
    pub dimensions: u32,
    /// Stored embedding normalization.
    pub normalization: SemanticEmbeddingNormalization,
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

/// Opaque identity of one signed installation disclosure.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstallationOfferId(String);

impl InstallationOfferId {
    /// Parses a non-empty opaque offer identity received from a transport.
    ///
    /// # Errors
    ///
    /// Returns [`SemanticComponentError::ConsentRequired`] for an empty value.
    pub fn new(value: impl Into<String>) -> Result<Self, SemanticComponentError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SemanticComponentError::ConsentRequired);
        }
        Ok(Self(value))
    }

    /// Returns the opaque offer identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One component disclosed by a signed catalog before consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticComponentDisclosure {
    /// Signed artifact identifier.
    pub artifact_id: String,
    /// Logical component identifier.
    pub component_id: String,
    /// Component role.
    pub kind: SemanticComponentKind,
    /// Exact model identity for model packages.
    pub model: Option<SemanticModelIdentity>,
    /// Exact signed component version.
    pub version: String,
    /// License displayed before consent.
    pub license: SemanticLicense,
    /// Compressed download bytes.
    pub download_bytes: u64,
    /// Estimated installed bytes.
    pub estimated_installed_bytes: u64,
    /// Estimated peak RAM bytes.
    pub estimated_ram_bytes: u64,
}

/// Complete first-install disclosure backed by one verified signed catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticInstallationOffer {
    id: InstallationOfferId,
    /// Immutable signed catalog revision.
    pub catalog_revision: String,
    /// Selected abstract profile.
    pub profile: SemanticProfile,
    /// Exact model revision resolved by that catalog.
    pub resolved_model: SemanticModelIdentity,
    /// Worker, runtime, and model disclosures.
    pub components: Vec<SemanticComponentDisclosure>,
    /// Whether embedding inference and semantic data remain on this device.
    pub embeddings_stay_local: bool,
    /// Stable local-only privacy disclosure.
    pub local_only_disclosure: String,
    /// Proposed semantic-data root.
    pub data_root: PathBuf,
    /// Free bytes reserved beyond installed-size estimates.
    pub minimum_free_space_reserve_bytes: u64,
}

impl SemanticInstallationOffer {
    /// Returns the opaque identity of this reviewed disclosure.
    #[must_use]
    pub const fn id(&self) -> &InstallationOfferId {
        &self.id
    }

    /// Converts this reviewed disclosure into explicit installation consent.
    #[must_use]
    pub fn consent(self) -> SemanticInstallationConsent {
        SemanticInstallationConsent { offer_id: self.id }
    }
}

/// Proof that one previously returned installation offer was explicitly accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticInstallationConsent {
    offer_id: InstallationOfferId,
}

impl SemanticInstallationConsent {
    /// Records explicit acceptance of a previously displayed offer identity.
    ///
    /// Capabilities still reject identities that were not issued by that
    /// specific live capability instance.
    #[must_use]
    pub const fn accept(offer_id: InstallationOfferId) -> Self {
        Self { offer_id }
    }
}

/// Successful install-or-enable result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticInstallReceipt {
    /// Signed artifacts installed or verified as already installed.
    pub installed_artifact_ids: Vec<String>,
}

/// Request to apply the newest compatible signed worker patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticWorkerPatchRequest {
    /// Logical installed worker component.
    pub component_id: String,
}

/// Required handling of remaining indexes during component uninstall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticIndexRetentionDecision {
    /// Retain remaining index data for a later reinstall.
    Retain,
    /// Delete remaining extracted, vector, and embedding-cache data.
    Delete,
}

/// Successful component uninstall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticUninstallReceipt {
    /// Explicit decision applied to remaining index data.
    pub index_decision: SemanticIndexRetentionDecision,
    /// Active and rollback component records removed.
    pub removed_component_count: u64,
}

/// Expected or removed enrolment-derived records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticIndexRecordCounts {
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

/// Explicit request to remove every record derived from one enrolment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveSemanticIndexRequest {
    /// Opaque enrolment identity.
    pub enrolment_id: String,
    /// Expected counts that must all be met.
    pub expected: SemanticIndexRecordCounts,
}

/// Opaque identity of one live, backend-generated index-removal plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticIndexRemovalPlanId(String);

impl SemanticIndexRemovalPlanId {
    /// Reconstructs an opaque plan identity at a transport boundary.
    ///
    /// # Errors
    ///
    /// Returns a removal error for an empty token.
    pub fn new(value: impl Into<String>) -> Result<Self, SemanticComponentError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SemanticComponentError::IndexRemoval {
                message: "index removal confirmation token is empty".to_owned(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the opaque plan identity.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Authoritative deletion inventory awaiting explicit confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticIndexRemovalPlan {
    id: SemanticIndexRemovalPlanId,
    /// Opaque enrolment identity.
    pub enrolment_id: String,
    /// Authoritative records that will be deleted in every category.
    pub expected: SemanticIndexRecordCounts,
}

impl SemanticIndexRemovalPlan {
    /// Returns the opaque confirmation identity.
    #[must_use]
    pub const fn id(&self) -> &SemanticIndexRemovalPlanId {
        &self.id
    }

    /// Confirms this exact authoritative inventory.
    #[must_use]
    pub fn confirm(self) -> SemanticIndexRemovalConfirmation {
        SemanticIndexRemovalConfirmation { plan_id: self.id }
    }
}

/// Proof of consent to one live authoritative index-removal plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticIndexRemovalConfirmation {
    plan_id: SemanticIndexRemovalPlanId,
}

impl SemanticIndexRemovalConfirmation {
    /// Reconstructs explicit confirmation of a previously returned plan.
    #[must_use]
    pub const fn confirm(plan_id: SemanticIndexRemovalPlanId) -> Self {
        Self { plan_id }
    }
}

/// Verified enrolment-derived data removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticIndexRemovalReceipt {
    /// Opaque removed enrolment identity.
    pub enrolment_id: String,
    /// Actual records removed by category.
    pub deleted: SemanticIndexRecordCounts,
    /// Always true for a successful removal.
    pub conversation_evidence_deleted: bool,
}

/// Successful verified semantic-data root move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticDataMoveReceipt {
    /// Retained source root.
    pub source: PathBuf,
    /// New active root.
    pub destination: PathBuf,
    /// Files independently verified.
    pub verified_file_count: u64,
    /// Bytes independently verified.
    pub verified_bytes: u64,
}

/// Raw expert-mode metadata for a local model file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticLocalModelImportRequest {
    /// Existing local model package.
    pub source_path: PathBuf,
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
    pub normalization: Option<SemanticEmbeddingNormalization>,
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
}

/// Why an explicit full reindex is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticModelMigrationReason {
    /// The model identity or embedding contract changes.
    ModelChanged,
    /// The stored index schema changes.
    SchemaChanged {
        /// Existing schema version.
        from_version: u32,
        /// Replacement schema version.
        to_version: u32,
    },
}

/// Confirmation-gated model migration plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticModelMigrationPlan {
    id: SemanticModelMigrationId,
    /// Current selection captured by the plan.
    pub from: Option<SemanticModelSelection>,
    /// Exact target selection.
    pub target: SemanticModelSelection,
    /// Estimated full-reindex work.
    pub estimate: SemanticReindexEstimate,
    /// Why full reindexing is mandatory.
    pub reason: SemanticModelMigrationReason,
}

impl SemanticModelMigrationPlan {
    /// Returns the opaque plan identity.
    #[must_use]
    pub const fn id(&self) -> &SemanticModelMigrationId {
        &self.id
    }

    /// Reports that activation requires explicit confirmation.
    #[must_use]
    pub const fn requires_confirmation(&self) -> bool {
        true
    }

    /// Reports that all embeddings must be rebuilt.
    #[must_use]
    pub const fn is_full_reindex(&self) -> bool {
        true
    }

    /// Reports that durable checkpoints can resume the work.
    #[must_use]
    pub const fn is_resumable(&self) -> bool {
        true
    }

    /// Converts this reviewed plan into explicit confirmation.
    #[must_use]
    pub fn confirm(self) -> SemanticModelMigrationConfirmation {
        SemanticModelMigrationConfirmation {
            migration_id: self.id,
        }
    }
}

/// Proof that one returned migration plan was explicitly confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticModelMigrationConfirmation {
    migration_id: SemanticModelMigrationId,
}

impl SemanticModelMigrationConfirmation {
    /// Records explicit confirmation of a previously displayed plan identity.
    ///
    /// Capabilities reject identities that do not refer to one of their live
    /// plans.
    #[must_use]
    pub const fn confirm(migration_id: SemanticModelMigrationId) -> Self {
        Self { migration_id }
    }
}

/// Durable full-reindex checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticModelMigrationCheckpoint {
    /// Migration being advanced.
    pub migration_id: SemanticModelMigrationId,
    /// Completely reindexed documents.
    pub completed_documents: u64,
    /// Opaque resume cursor.
    pub resume_cursor: Option<String>,
}

/// Required expert local-model metadata field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticModelImportField {
    /// Local package path.
    SourcePath,
    /// Model identifier.
    ModelId,
    /// Immutable upstream revision.
    UpstreamRevision,
    /// License information.
    License,
    /// Tokenizer identity.
    Tokenizer,
    /// Embedding dimensions.
    Dimensions,
    /// Embedding normalization.
    Normalization,
    /// Runtime compatibility.
    RuntimeCompatibility,
    /// Language coverage.
    LanguageCoverage,
    /// Installed-size estimate.
    EstimatedDiskBytes,
    /// Peak-RAM estimate.
    EstimatedRamBytes,
}

/// Actionable semantic component lifecycle failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SemanticComponentError {
    /// No semantic component implementation was configured.
    #[error("semantic component capability is unavailable")]
    Unavailable,
    /// The active authority cannot perform a mutation.
    #[error("{authority:?} authority cannot perform {operation:?}")]
    AuthorityDenied {
        /// Read-only or unavailable authority.
        authority: SemanticComponentAuthority,
        /// Rejected explicit operation.
        operation: SemanticComponentOperation,
    },
    /// An install request was not derived from a live reviewed offer.
    #[error("installation requires explicit consent to a live offer")]
    ConsentRequired,
    /// An expert local model omitted or invalidated required metadata.
    #[error("local model metadata field {field:?} is invalid")]
    InvalidLocalModelMetadata {
        /// Invalid field.
        field: SemanticModelImportField,
    },
    /// An operation is not valid in the current lifecycle state.
    #[error("{operation:?} is not valid while semantic components are {lifecycle}")]
    InvalidLifecycle {
        /// Rejected operation.
        operation: SemanticComponentOperation,
        /// Current lifecycle description.
        lifecycle: String,
    },
    /// An enrolment identity was empty or unsafe.
    #[error("semantic enrolment identifier is invalid")]
    InvalidEnrolment,
    /// A migration confirmation or checkpoint did not identify a live plan.
    #[error("model migration plan is stale or unknown")]
    InvalidMigrationPlan,
    /// A checkpoint regressed or exceeded the disclosed estimate.
    #[error("invalid model migration progress {completed_documents} of {total_documents}")]
    InvalidMigrationProgress {
        /// Rejected completed document count.
        completed_documents: u64,
        /// Disclosed total document estimate.
        total_documents: u64,
    },
    /// The current distribution prohibits runtime-downloaded executable code.
    #[error("this distribution prohibits downloading executable semantic components")]
    ExecutableDownloadProhibited,
    /// Signed catalog lookup or compatibility validation failed.
    #[error("semantic component catalog error: {message}")]
    Catalog {
        /// Actionable catalog diagnostic.
        message: String,
    },
    /// Available storage cannot satisfy the estimate plus reserve.
    #[error(
        "insufficient free space: {available_bytes} available, {required_bytes} required including {reserve_bytes} reserve"
    )]
    InsufficientSpace {
        /// Available bytes.
        available_bytes: u64,
        /// Required installed bytes plus reserve.
        required_bytes: u64,
        /// Explicit reserve included in the requirement.
        reserve_bytes: u64,
    },
    /// A resumable artifact download was interrupted.
    #[error("download of `{artifact_id}` was interrupted: {message}")]
    DownloadInterrupted {
        /// Signed artifact identity.
        artifact_id: String,
        /// Source diagnostic.
        message: String,
    },
    /// Downloaded or installed bytes failed signed integrity validation.
    #[error("artifact `{artifact_id}` failed integrity validation")]
    ArtifactVerificationFailed {
        /// Rejected signed artifact.
        artifact_id: String,
    },
    /// A verified component could not be activated.
    #[error("artifact `{artifact_id}` could not be activated: {message}")]
    ActivationFailed {
        /// Rejected signed artifact.
        artifact_id: String,
        /// Activation diagnostic.
        message: String,
    },
    /// Free-space probing failed before installation.
    #[error("semantic component free-space probe failed: {message}")]
    FreeSpaceProbe {
        /// Probe diagnostic.
        message: String,
    },
    /// Durable component state could not be loaded or persisted.
    #[error("semantic component state error: {message}")]
    State {
        /// State diagnostic.
        message: String,
    },
    /// Component filesystem work failed.
    #[error("semantic component filesystem error: {message}")]
    Filesystem {
        /// Filesystem diagnostic.
        message: String,
    },
    /// Indexing could not be paused or resumed.
    #[error("semantic indexing lifecycle error: {message}")]
    Indexing {
        /// Indexing diagnostic.
        message: String,
    },
    /// Enrolment-derived deletion failed.
    #[error("semantic index removal failed: {message}")]
    IndexRemoval {
        /// Removal diagnostic.
        message: String,
    },
    /// Data-root migration failed before its verified switch.
    #[error("semantic data migration failed: {message}")]
    DataMigration {
        /// Migration diagnostic.
        message: String,
    },
    /// A blocking filesystem task could not complete.
    #[error("semantic component blocking task failed: {message}")]
    BlockingTaskFailed {
        /// Join failure diagnostic.
        message: String,
    },
    /// Aggregate disk usage overflowed its representation.
    #[error("semantic component disk usage overflowed")]
    DiskUseOverflow,
}

/// Narrow host-neutral interface for semantic component lifecycle management.
#[async_trait]
pub trait SemanticComponentCapability: Send + Sync {
    /// Reports authority and supported operations without touching component storage.
    async fn capabilities(&self) -> SemanticComponentCapabilities;

    /// Reports current lifecycle state.
    async fn status(&self) -> Result<SemanticComponentStatus, SemanticComponentError>;

    /// Returns catalog-backed profile choices and exact resolved model revisions.
    async fn catalog_profiles(&self) -> Result<Vec<SemanticModelProfile>, SemanticComponentError>;

    /// Creates a complete signed disclosure before consent.
    async fn installation_offer(
        &self,
        profile: SemanticProfile,
    ) -> Result<SemanticInstallationOffer, SemanticComponentError>;

    /// Installs or enables exactly one previously reviewed offer.
    async fn install_or_enable(
        &self,
        consent: SemanticInstallationConsent,
    ) -> Result<SemanticInstallReceipt, SemanticComponentError>;

    /// Applies a compatible signed worker patch when one is available.
    async fn install_compatible_worker_patch(
        &self,
        request: SemanticWorkerPatchRequest,
    ) -> Result<Option<SemanticInstallReceipt>, SemanticComponentError>;

    /// Pauses indexing as a distinct lifecycle action.
    async fn pause_indexing(&self) -> Result<(), SemanticComponentError>;

    /// Resumes indexing as a distinct lifecycle action.
    async fn resume_indexing(&self) -> Result<(), SemanticComponentError>;

    /// Removes every record derived from one enrolment, including conversation evidence.
    async fn remove_index(
        &self,
        request: RemoveSemanticIndexRequest,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError>;

    /// Inventories one enrolment and creates an opaque confirmation plan.
    async fn plan_index_removal(
        &self,
        _enrolment_id: String,
    ) -> Result<SemanticIndexRemovalPlan, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    /// Executes one live backend-generated removal plan.
    async fn confirm_index_removal(
        &self,
        _confirmation: SemanticIndexRemovalConfirmation,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    /// Moves the semantic-data root through pause-copy-verify-switch.
    async fn move_data(
        &self,
        destination: PathBuf,
    ) -> Result<SemanticDataMoveReceipt, SemanticComponentError>;

    /// Uninstalls components with an explicit index retention decision.
    async fn uninstall_components(
        &self,
        index_decision: SemanticIndexRetentionDecision,
    ) -> Result<SemanticUninstallReceipt, SemanticComponentError>;

    /// Validates expert local-model metadata and creates a distinct migration plan.
    async fn import_local_model(
        &self,
        request: SemanticLocalModelImportRequest,
        profile: SemanticProfile,
        estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError>;

    /// Creates a migration plan to the signed catalog resolution for a profile.
    async fn plan_model_migration(
        &self,
        profile: SemanticProfile,
        estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError>;

    /// Begins a previously reviewed and explicitly confirmed full reindex.
    async fn confirm_model_migration(
        &self,
        confirmation: SemanticModelMigrationConfirmation,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError>;

    /// Persists a resumable full-reindex checkpoint.
    async fn checkpoint_model_migration(
        &self,
        checkpoint: SemanticModelMigrationCheckpoint,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError>;

    /// Activates the target after the confirmed full reindex completes.
    async fn complete_model_migration(
        &self,
        migration_id: SemanticModelMigrationId,
    ) -> Result<SemanticModelSelection, SemanticComponentError>;
}

/// Inert capability used when no component implementation is configured.
pub struct UnavailableSemanticComponentCapability;

impl UnavailableSemanticComponentCapability {
    /// Creates an unavailable capability without touching component storage.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for UnavailableSemanticComponentCapability {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SemanticComponentCapability for UnavailableSemanticComponentCapability {
    async fn capabilities(&self) -> SemanticComponentCapabilities {
        SemanticComponentCapabilities::unavailable()
    }

    async fn status(&self) -> Result<SemanticComponentStatus, SemanticComponentError> {
        Ok(SemanticComponentStatus::unavailable())
    }

    async fn catalog_profiles(&self) -> Result<Vec<SemanticModelProfile>, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn installation_offer(
        &self,
        _profile: SemanticProfile,
    ) -> Result<SemanticInstallationOffer, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn install_or_enable(
        &self,
        _consent: SemanticInstallationConsent,
    ) -> Result<SemanticInstallReceipt, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn install_compatible_worker_patch(
        &self,
        _request: SemanticWorkerPatchRequest,
    ) -> Result<Option<SemanticInstallReceipt>, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn pause_indexing(&self) -> Result<(), SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn resume_indexing(&self) -> Result<(), SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn remove_index(
        &self,
        _request: RemoveSemanticIndexRequest,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn move_data(
        &self,
        _destination: PathBuf,
    ) -> Result<SemanticDataMoveReceipt, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn uninstall_components(
        &self,
        _index_decision: SemanticIndexRetentionDecision,
    ) -> Result<SemanticUninstallReceipt, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn import_local_model(
        &self,
        _request: SemanticLocalModelImportRequest,
        _profile: SemanticProfile,
        _estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn plan_model_migration(
        &self,
        _profile: SemanticProfile,
        _estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn confirm_model_migration(
        &self,
        _confirmation: SemanticModelMigrationConfirmation,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn checkpoint_model_migration(
        &self,
        _checkpoint: SemanticModelMigrationCheckpoint,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn complete_model_migration(
        &self,
        _migration_id: SemanticModelMigrationId,
    ) -> Result<SemanticModelSelection, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }
}

/// Read-only server capability whose components are administrator provisioned.
pub struct AdministratorProvisionedSemanticComponentCapability {
    status: SemanticComponentStatus,
    profiles: Vec<SemanticModelProfile>,
}

impl AdministratorProvisionedSemanticComponentCapability {
    /// Creates a read-only capability from administrator-reported state.
    #[must_use]
    pub fn new(status: SemanticComponentStatus, profiles: Vec<SemanticModelProfile>) -> Self {
        Self { status, profiles }
    }

    fn denied(operation: SemanticComponentOperation) -> SemanticComponentError {
        SemanticComponentError::AuthorityDenied {
            authority: SemanticComponentAuthority::AdministratorProvisioned,
            operation,
        }
    }
}

/// Deterministic lifecycle scenarios available to mock-mode callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeSemanticComponentScenario {
    /// Components have never been installed.
    Absent,
    /// A complete offer awaits consent.
    Offered,
    /// A partial download can resume.
    DownloadingResumable,
    /// Components are installed and enabled.
    InstalledEnabled,
    /// Indexing is paused.
    Paused,
    /// A confirmed full reindex has a durable checkpoint.
    Migrating,
    /// An update failed and the last worker was restored.
    UpdateFailedRolledBack,
    /// Installation is blocked on required free space.
    LowDisk,
    /// Components were removed while indexes were retained.
    UninstalledRetainingIndex,
    /// Components and indexes were deleted.
    UninstalledDeletingIndex,
}

impl FakeSemanticComponentScenario {
    /// Returns every deterministic mock scenario.
    #[must_use]
    pub const fn all() -> &'static [Self; 10] {
        &[
            Self::Absent,
            Self::Offered,
            Self::DownloadingResumable,
            Self::InstalledEnabled,
            Self::Paused,
            Self::Migrating,
            Self::UpdateFailedRolledBack,
            Self::LowDisk,
            Self::UninstalledRetainingIndex,
            Self::UninstalledDeletingIndex,
        ]
    }

    /// Returns the lifecycle value represented by this scenario.
    #[must_use]
    pub fn lifecycle(self) -> SemanticComponentLifecycle {
        match self {
            Self::Absent => SemanticComponentLifecycle::Absent,
            Self::Offered => SemanticComponentLifecycle::Offered {
                offer_id: InstallationOfferId("fake-scenario-offer".to_owned()),
            },
            Self::DownloadingResumable => SemanticComponentLifecycle::Downloading {
                downloaded_bytes: 40,
                total_bytes: 100,
                resumable: true,
            },
            Self::InstalledEnabled => SemanticComponentLifecycle::InstalledEnabled,
            Self::Paused => SemanticComponentLifecycle::Paused,
            Self::Migrating => SemanticComponentLifecycle::Migrating {
                progress: SemanticModelMigrationProgress::new(
                    SemanticModelMigrationId::new("fake-scenario-migration"),
                    4,
                    SemanticReindexEstimate::new(10, 100),
                    SemanticModelSelection::new(
                        SemanticProfile::CompactMultilingual,
                        fake_model_identity(SemanticProfile::CompactMultilingual),
                    ),
                    SemanticModelMigrationReason::ModelChanged,
                    Some("fake-resume-cursor".to_owned()),
                ),
            },
            Self::UpdateFailedRolledBack => SemanticComponentLifecycle::UpdateFailedRolledBack {
                failed_version: "1.0.1".to_owned(),
                active_version: "1.0.0".to_owned(),
            },
            Self::LowDisk => SemanticComponentLifecycle::LowDisk {
                available_bytes: 50,
                required_bytes: 100,
            },
            Self::UninstalledRetainingIndex => SemanticComponentLifecycle::Uninstalled {
                index_decision: SemanticIndexRetentionDecision::Retain,
            },
            Self::UninstalledDeletingIndex => SemanticComponentLifecycle::Uninstalled {
                index_decision: SemanticIndexRetentionDecision::Delete,
            },
        }
    }
}

struct FakeSemanticComponentState {
    status: SemanticComponentStatus,
    live_offers: BTreeMap<InstallationOfferId, SemanticModelSelection>,
    plans: BTreeMap<SemanticModelMigrationId, SemanticModelMigrationPlan>,
    index_removal_plans: BTreeMap<SemanticIndexRemovalPlanId, (String, SemanticIndexRecordCounts)>,
    next_offer: u64,
    next_migration: u64,
    next_index_removal: u64,
}

/// Controllable deterministic semantic component capability for mock mode.
pub struct FakeSemanticComponentCapability {
    state: Mutex<FakeSemanticComponentState>,
}

impl Default for FakeSemanticComponentCapability {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeSemanticComponentCapability {
    /// Creates a fake in the absent state without touching the filesystem.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(FakeSemanticComponentState {
                status: SemanticComponentStatus::fake(SemanticComponentLifecycle::Absent),
                live_offers: BTreeMap::new(),
                plans: BTreeMap::new(),
                index_removal_plans: BTreeMap::new(),
                next_offer: 1,
                next_migration: 1,
                next_index_removal: 1,
            }),
        }
    }

    /// Selects one deterministic lifecycle scenario.
    pub fn set_scenario(&self, scenario: FakeSemanticComponentScenario) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status = SemanticComponentStatus::fake(scenario.lifecycle());
    }

    fn next_migration_plan(
        state: &mut FakeSemanticComponentState,
        profile: SemanticProfile,
        identity: SemanticModelIdentity,
        estimate: SemanticReindexEstimate,
    ) -> SemanticModelMigrationPlan {
        let id = SemanticModelMigrationId::new(format!("fake-migration-{}", state.next_migration));
        state.next_migration = state.next_migration.saturating_add(1);
        let plan = SemanticModelMigrationPlan {
            id: id.clone(),
            from: state.status.active_model.clone(),
            target: SemanticModelSelection::new(profile, identity),
            estimate,
            reason: SemanticModelMigrationReason::ModelChanged,
        };
        state.plans.insert(id, plan.clone());
        plan
    }
}

#[async_trait]
impl SemanticComponentCapability for FakeSemanticComponentCapability {
    async fn capabilities(&self) -> SemanticComponentCapabilities {
        SemanticComponentCapabilities::deterministic_mock()
    }

    async fn status(&self) -> Result<SemanticComponentStatus, SemanticComponentError> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status
            .clone())
    }

    async fn catalog_profiles(&self) -> Result<Vec<SemanticModelProfile>, SemanticComponentError> {
        Ok(fake_profiles())
    }

    async fn installation_offer(
        &self,
        profile: SemanticProfile,
    ) -> Result<SemanticInstallationOffer, SemanticComponentError> {
        let resolved_model = fake_model_identity(profile);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = InstallationOfferId(format!("fake-offer-{}", state.next_offer));
        state.next_offer = state.next_offer.saturating_add(1);
        state.live_offers.insert(
            id.clone(),
            SemanticModelSelection::new(profile, resolved_model.clone()),
        );
        state.status.lifecycle = SemanticComponentLifecycle::Offered {
            offer_id: id.clone(),
        };
        Ok(fake_installation_offer(id, profile, resolved_model))
    }

    async fn install_or_enable(
        &self,
        consent: SemanticInstallationConsent,
    ) -> Result<SemanticInstallReceipt, SemanticComponentError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let selection = state
            .live_offers
            .remove(&consent.offer_id)
            .ok_or(SemanticComponentError::ConsentRequired)?;
        state.status.lifecycle = SemanticComponentLifecycle::InstalledEnabled;
        state.status.active_model = Some(selection);
        state.status.components = fake_installed_components();
        state.status.disk_use = SemanticDiskUse {
            categories: vec![
                SemanticCategoryDiskUse::new(SemanticDataCategory::Catalog, 1),
                SemanticCategoryDiskUse::new(SemanticDataCategory::Extracted, 0),
                SemanticCategoryDiskUse::new(SemanticDataCategory::Zvec, 0),
                SemanticCategoryDiskUse::new(SemanticDataCategory::EmbeddingCache, 0),
                SemanticCategoryDiskUse::new(SemanticDataCategory::Models, 300),
                SemanticCategoryDiskUse::new(SemanticDataCategory::Workers, 130),
            ],
            total_bytes: 431,
        };
        Ok(SemanticInstallReceipt {
            installed_artifact_ids: vec![
                "fake-worker-artifact".to_owned(),
                "fake-runtime-artifact".to_owned(),
                "fake-model-artifact".to_owned(),
            ],
        })
    }

    async fn install_compatible_worker_patch(
        &self,
        _request: SemanticWorkerPatchRequest,
    ) -> Result<Option<SemanticInstallReceipt>, SemanticComponentError> {
        Ok(None)
    }

    async fn pause_indexing(&self) -> Result<(), SemanticComponentError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.status.lifecycle != SemanticComponentLifecycle::InstalledEnabled {
            return Err(invalid_fake_lifecycle(
                SemanticComponentOperation::PauseIndexing,
                &state.status.lifecycle,
            ));
        }
        state.status.lifecycle = SemanticComponentLifecycle::Paused;
        Ok(())
    }

    async fn resume_indexing(&self) -> Result<(), SemanticComponentError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.status.lifecycle != SemanticComponentLifecycle::Paused {
            return Err(invalid_fake_lifecycle(
                SemanticComponentOperation::ResumeIndexing,
                &state.status.lifecycle,
            ));
        }
        state.status.lifecycle = SemanticComponentLifecycle::InstalledEnabled;
        Ok(())
    }

    async fn remove_index(
        &self,
        request: RemoveSemanticIndexRequest,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        if request.enrolment_id.is_empty()
            || !request
                .enrolment_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(SemanticComponentError::InvalidEnrolment);
        }
        Ok(SemanticIndexRemovalReceipt {
            enrolment_id: request.enrolment_id,
            deleted: request.expected,
            conversation_evidence_deleted: true,
        })
    }

    async fn plan_index_removal(
        &self,
        enrolment_id: String,
    ) -> Result<SemanticIndexRemovalPlan, SemanticComponentError> {
        core::EnrolmentId::new(enrolment_id.clone())
            .map_err(|_| SemanticComponentError::InvalidEnrolment)?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id =
            SemanticIndexRemovalPlanId(format!("fake-index-removal-{}", state.next_index_removal));
        state.next_index_removal = state.next_index_removal.saturating_add(1);
        let expected = SemanticIndexRecordCounts {
            index_records: 3,
            extracted_files: 2,
            zvec_vectors: 3,
            cache_entries: 1,
            conversation_evidence: 2,
        };
        state
            .index_removal_plans
            .insert(id.clone(), (enrolment_id.clone(), expected));
        Ok(SemanticIndexRemovalPlan {
            id,
            enrolment_id,
            expected,
        })
    }

    async fn confirm_index_removal(
        &self,
        confirmation: SemanticIndexRemovalConfirmation,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        let (enrolment_id, deleted) = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .index_removal_plans
            .remove(&confirmation.plan_id)
            .ok_or_else(|| SemanticComponentError::IndexRemoval {
                message: "index removal plan is stale or unknown".to_owned(),
            })?;
        Ok(SemanticIndexRemovalReceipt {
            enrolment_id,
            deleted,
            conversation_evidence_deleted: true,
        })
    }

    async fn move_data(
        &self,
        destination: PathBuf,
    ) -> Result<SemanticDataMoveReceipt, SemanticComponentError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let source = state
            .status
            .data_root
            .replace(destination.clone())
            .unwrap_or_else(|| PathBuf::from("mock/semantic"));
        Ok(SemanticDataMoveReceipt {
            source,
            destination,
            verified_file_count: 0,
            verified_bytes: 0,
        })
    }

    async fn uninstall_components(
        &self,
        index_decision: SemanticIndexRetentionDecision,
    ) -> Result<SemanticUninstallReceipt, SemanticComponentError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.status.lifecycle = SemanticComponentLifecycle::Uninstalled { index_decision };
        state.status.components.clear();
        if index_decision == SemanticIndexRetentionDecision::Delete {
            state.status.active_model = None;
            state.status.migration = None;
            state.status.disk_use = SemanticDiskUse::empty();
        }
        Ok(SemanticUninstallReceipt {
            index_decision,
            removed_component_count: 3,
        })
    }

    async fn import_local_model(
        &self,
        request: SemanticLocalModelImportRequest,
        profile: SemanticProfile,
        estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError> {
        validate_fake_local_model(&request)?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(Self::next_migration_plan(
            &mut state,
            profile,
            SemanticModelIdentity::new(request.model_id, request.upstream_revision),
            estimate,
        ))
    }

    async fn plan_model_migration(
        &self,
        profile: SemanticProfile,
        estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(Self::next_migration_plan(
            &mut state,
            profile,
            fake_model_identity(profile),
            estimate,
        ))
    }

    async fn confirm_model_migration(
        &self,
        confirmation: SemanticModelMigrationConfirmation,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let plan = state
            .plans
            .get(&confirmation.migration_id)
            .cloned()
            .ok_or(SemanticComponentError::InvalidMigrationPlan)?;
        let progress = SemanticModelMigrationProgress::new(
            confirmation.migration_id,
            0,
            plan.estimate,
            plan.target,
            plan.reason,
            None,
        );
        state.status.migration = Some(progress.clone());
        state.status.lifecycle = SemanticComponentLifecycle::Migrating {
            progress: progress.clone(),
        };
        Ok(progress)
    }

    async fn checkpoint_model_migration(
        &self,
        checkpoint: SemanticModelMigrationCheckpoint,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let plan = state
            .plans
            .get(&checkpoint.migration_id)
            .cloned()
            .ok_or(SemanticComponentError::InvalidMigrationPlan)?;
        let total_documents = plan.estimate.documents();
        let previous = state
            .status
            .migration
            .as_ref()
            .filter(|progress| progress.migration_id == checkpoint.migration_id)
            .ok_or(SemanticComponentError::InvalidMigrationPlan)?;
        if checkpoint.completed_documents < previous.completed_documents
            || checkpoint.completed_documents > total_documents
        {
            return Err(SemanticComponentError::InvalidMigrationProgress {
                completed_documents: checkpoint.completed_documents,
                total_documents,
            });
        }
        let progress = SemanticModelMigrationProgress::new(
            checkpoint.migration_id,
            checkpoint.completed_documents,
            plan.estimate,
            plan.target,
            plan.reason,
            checkpoint.resume_cursor,
        );
        state.status.migration = Some(progress.clone());
        state.status.lifecycle = SemanticComponentLifecycle::Migrating {
            progress: progress.clone(),
        };
        Ok(progress)
    }

    async fn complete_model_migration(
        &self,
        migration_id: SemanticModelMigrationId,
    ) -> Result<SemanticModelSelection, SemanticComponentError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let plan = state
            .plans
            .get(&migration_id)
            .cloned()
            .ok_or(SemanticComponentError::InvalidMigrationPlan)?;
        let completed = state
            .status
            .migration
            .as_ref()
            .filter(|progress| progress.migration_id == migration_id)
            .map_or(0, SemanticModelMigrationProgress::completed_documents);
        if completed < plan.estimate.documents() {
            return Err(SemanticComponentError::InvalidMigrationProgress {
                completed_documents: completed,
                total_documents: plan.estimate.documents(),
            });
        }
        state.plans.remove(&migration_id);
        state.status.active_model = Some(plan.target.clone());
        state.status.migration = None;
        state.status.lifecycle = SemanticComponentLifecycle::InstalledEnabled;
        Ok(plan.target)
    }
}

fn invalid_fake_lifecycle(
    operation: SemanticComponentOperation,
    lifecycle: &SemanticComponentLifecycle,
) -> SemanticComponentError {
    SemanticComponentError::InvalidLifecycle {
        operation,
        lifecycle: format!("{lifecycle:?}"),
    }
}

fn fake_model_identity(profile: SemanticProfile) -> SemanticModelIdentity {
    let suffix = match profile {
        SemanticProfile::CompactMultilingual => "compact-multilingual",
        SemanticProfile::CompactEnglish => "compact-english",
        SemanticProfile::MultilingualQuality => "multilingual-quality",
    };
    SemanticModelIdentity::new(format!("fake-{suffix}"), format!("fake-{suffix}-revision"))
}

fn fake_model_metadata(profile: SemanticProfile) -> SemanticModelMetadata {
    SemanticModelMetadata {
        identity: fake_model_identity(profile),
        license: SemanticLicense {
            spdx: "MIT".to_owned(),
            notice: "Deterministic mock model; no package is downloaded.".to_owned(),
        },
        tokenizer: format!("fake-{profile:?}-tokenizer"),
        dimensions: 384,
        normalization: SemanticEmbeddingNormalization::UnitLength,
        runtime_component_id: "fake-runtime".to_owned(),
        runtime_version_requirement: "^1.0".to_owned(),
        language_coverage: match profile {
            SemanticProfile::CompactEnglish => vec!["en".to_owned()],
            SemanticProfile::CompactMultilingual | SemanticProfile::MultilingualQuality => {
                vec!["en".to_owned(), "nl".to_owned()]
            }
        },
        estimated_disk_bytes: 300,
        estimated_ram_bytes: 400,
    }
}

fn fake_profiles() -> Vec<SemanticModelProfile> {
    SemanticProfile::all()
        .iter()
        .copied()
        .map(|profile| SemanticModelProfile {
            profile,
            recommended: profile == SemanticProfile::recommended(),
            explanation: profile.explanation().to_owned(),
            resolved_model: fake_model_identity(profile),
            metadata: fake_model_metadata(profile),
        })
        .collect()
}

fn fake_installed_components() -> Vec<InstalledSemanticComponent> {
    vec![
        InstalledSemanticComponent::new(
            "fake-worker-artifact",
            "fake-worker",
            SemanticComponentKind::Worker,
            "1.0.0",
            InstalledSemanticComponentState::Active,
            60,
        ),
        InstalledSemanticComponent::new(
            "fake-runtime-artifact",
            "fake-runtime",
            SemanticComponentKind::Runtime,
            "1.0.0",
            InstalledSemanticComponentState::Active,
            70,
        ),
        InstalledSemanticComponent::new(
            "fake-model-artifact",
            "fake-model",
            SemanticComponentKind::Model,
            "1.0.0",
            InstalledSemanticComponentState::Active,
            300,
        ),
    ]
}

fn fake_installation_offer(
    id: InstallationOfferId,
    profile: SemanticProfile,
    resolved_model: SemanticModelIdentity,
) -> SemanticInstallationOffer {
    let license = SemanticLicense {
        spdx: "MIT".to_owned(),
        notice: "Deterministic mock component; no package is downloaded.".to_owned(),
    };
    SemanticInstallationOffer {
        id,
        catalog_revision: "fake-signed-catalog-revision".to_owned(),
        profile,
        resolved_model: resolved_model.clone(),
        components: vec![
            SemanticComponentDisclosure {
                artifact_id: "fake-worker-artifact".to_owned(),
                component_id: "fake-worker".to_owned(),
                kind: SemanticComponentKind::Worker,
                model: None,
                version: "1.0.0".to_owned(),
                license: license.clone(),
                download_bytes: 100,
                estimated_installed_bytes: 200,
                estimated_ram_bytes: 50,
            },
            SemanticComponentDisclosure {
                artifact_id: "fake-runtime-artifact".to_owned(),
                component_id: "fake-runtime".to_owned(),
                kind: SemanticComponentKind::Runtime,
                model: None,
                version: "1.0.0".to_owned(),
                license: license.clone(),
                download_bytes: 100,
                estimated_installed_bytes: 200,
                estimated_ram_bytes: 100,
            },
            SemanticComponentDisclosure {
                artifact_id: "fake-model-artifact".to_owned(),
                component_id: "fake-model".to_owned(),
                kind: SemanticComponentKind::Model,
                model: Some(resolved_model),
                version: "1.0.0".to_owned(),
                license,
                download_bytes: 200,
                estimated_installed_bytes: 300,
                estimated_ram_bytes: 400,
            },
        ],
        embeddings_stay_local: true,
        local_only_disclosure: "Embedding inference and semantic index data stay on this device."
            .to_owned(),
        data_root: PathBuf::from("mock/semantic"),
        minimum_free_space_reserve_bytes: 1_024,
    }
}

fn validate_fake_local_model(
    request: &SemanticLocalModelImportRequest,
) -> Result<(), SemanticComponentError> {
    let missing = if request.source_path.as_os_str().is_empty() {
        Some(SemanticModelImportField::SourcePath)
    } else if request.model_id.trim().is_empty() {
        Some(SemanticModelImportField::ModelId)
    } else if request.upstream_revision.trim().is_empty() {
        Some(SemanticModelImportField::UpstreamRevision)
    } else if request.license_spdx.trim().is_empty() {
        Some(SemanticModelImportField::License)
    } else if request.tokenizer.trim().is_empty() {
        Some(SemanticModelImportField::Tokenizer)
    } else if request.dimensions == 0 {
        Some(SemanticModelImportField::Dimensions)
    } else if request.normalization.is_none() {
        Some(SemanticModelImportField::Normalization)
    } else if request.runtime_component_id.trim().is_empty()
        || request.runtime_version_requirement.trim().is_empty()
    {
        Some(SemanticModelImportField::RuntimeCompatibility)
    } else if request.language_coverage.is_empty()
        || request
            .language_coverage
            .iter()
            .any(|language| language.trim().is_empty())
    {
        Some(SemanticModelImportField::LanguageCoverage)
    } else if request.estimated_disk_bytes == 0 {
        Some(SemanticModelImportField::EstimatedDiskBytes)
    } else if request.estimated_ram_bytes == 0 {
        Some(SemanticModelImportField::EstimatedRamBytes)
    } else {
        None
    };
    match missing {
        Some(field) => Err(SemanticComponentError::InvalidLocalModelMetadata { field }),
        None => Ok(()),
    }
}

#[async_trait]
impl SemanticComponentCapability for AdministratorProvisionedSemanticComponentCapability {
    async fn capabilities(&self) -> SemanticComponentCapabilities {
        SemanticComponentCapabilities::administrator_provisioned()
    }

    async fn status(&self) -> Result<SemanticComponentStatus, SemanticComponentError> {
        Ok(self.status.clone())
    }

    async fn catalog_profiles(&self) -> Result<Vec<SemanticModelProfile>, SemanticComponentError> {
        Ok(self.profiles.clone())
    }

    async fn installation_offer(
        &self,
        _profile: SemanticProfile,
    ) -> Result<SemanticInstallationOffer, SemanticComponentError> {
        Err(Self::denied(
            SemanticComponentOperation::CreateInstallationOffer,
        ))
    }

    async fn install_or_enable(
        &self,
        _consent: SemanticInstallationConsent,
    ) -> Result<SemanticInstallReceipt, SemanticComponentError> {
        Err(Self::denied(SemanticComponentOperation::InstallOrEnable))
    }

    async fn install_compatible_worker_patch(
        &self,
        _request: SemanticWorkerPatchRequest,
    ) -> Result<Option<SemanticInstallReceipt>, SemanticComponentError> {
        Err(Self::denied(SemanticComponentOperation::InstallWorkerPatch))
    }

    async fn pause_indexing(&self) -> Result<(), SemanticComponentError> {
        Err(Self::denied(SemanticComponentOperation::PauseIndexing))
    }

    async fn resume_indexing(&self) -> Result<(), SemanticComponentError> {
        Err(Self::denied(SemanticComponentOperation::ResumeIndexing))
    }

    async fn remove_index(
        &self,
        _request: RemoveSemanticIndexRequest,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        Err(Self::denied(SemanticComponentOperation::RemoveIndex))
    }

    async fn plan_index_removal(
        &self,
        _enrolment_id: String,
    ) -> Result<SemanticIndexRemovalPlan, SemanticComponentError> {
        Err(Self::denied(SemanticComponentOperation::RemoveIndex))
    }

    async fn confirm_index_removal(
        &self,
        _confirmation: SemanticIndexRemovalConfirmation,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        Err(Self::denied(SemanticComponentOperation::RemoveIndex))
    }

    async fn move_data(
        &self,
        _destination: PathBuf,
    ) -> Result<SemanticDataMoveReceipt, SemanticComponentError> {
        Err(Self::denied(SemanticComponentOperation::MoveData))
    }

    async fn uninstall_components(
        &self,
        _index_decision: SemanticIndexRetentionDecision,
    ) -> Result<SemanticUninstallReceipt, SemanticComponentError> {
        Err(Self::denied(
            SemanticComponentOperation::UninstallComponents,
        ))
    }

    async fn import_local_model(
        &self,
        _request: SemanticLocalModelImportRequest,
        _profile: SemanticProfile,
        _estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError> {
        Err(Self::denied(SemanticComponentOperation::ImportLocalModel))
    }

    async fn plan_model_migration(
        &self,
        _profile: SemanticProfile,
        _estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError> {
        Err(Self::denied(SemanticComponentOperation::PlanModelMigration))
    }

    async fn confirm_model_migration(
        &self,
        _confirmation: SemanticModelMigrationConfirmation,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError> {
        Err(Self::denied(
            SemanticComponentOperation::ConfirmModelMigration,
        ))
    }

    async fn checkpoint_model_migration(
        &self,
        _checkpoint: SemanticModelMigrationCheckpoint,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError> {
        Err(Self::denied(
            SemanticComponentOperation::CheckpointModelMigration,
        ))
    }

    async fn complete_model_migration(
        &self,
        _migration_id: SemanticModelMigrationId,
    ) -> Result<SemanticModelSelection, SemanticComponentError> {
        Err(Self::denied(
            SemanticComponentOperation::CompleteModelMigration,
        ))
    }
}

/// Failure returned by a host-owned enrolment data remover.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("enrolment-derived data removal failed: {message}")]
pub struct SemanticIndexRemovalError {
    message: String,
}

impl SemanticIndexRemovalError {
    /// Creates an index-removal diagnostic.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Host boundary that deletes all data described by an immutable deletion plan.
pub trait SemanticIndexRemover: Send + Sync {
    /// Removes every planned category, including saved-conversation evidence.
    ///
    /// # Errors
    ///
    /// Returns a host diagnostic when deletion could not complete.
    fn remove(
        &self,
        plan: &core::EnrolmentDeletionPlan,
    ) -> Result<core::EnrolmentDeletionCounts, SemanticIndexRemovalError>;
}

/// Host boundary that inventories every enrolment-derived record before deletion.
pub trait SemanticIndexInventory: Send + Sync {
    /// Returns authoritative counts, including saved-conversation evidence.
    ///
    /// # Errors
    ///
    /// Returns a host diagnostic when inventory cannot be read consistently.
    fn counts(
        &self,
        enrolment_id: &core::EnrolmentId,
    ) -> Result<core::EnrolmentDeletionCounts, SemanticIndexRemovalError>;
}

/// Managed desktop policy and signed artifact selection.
pub struct ManagedSemanticComponentConfiguration {
    /// Signed worker/runtime base artifacts; the selected model is always
    /// derived from the requested profile in the trusted catalog.
    pub runtime_and_worker_artifacts: Vec<core::ArtifactId>,
    /// Platform, protocol, and installed-runtime compatibility context.
    pub environment: core::InstallEnvironment,
    /// Distribution policy controlling executable downloads.
    pub distribution: DesktopSemanticDistribution,
    /// Bytes that must remain free beyond installed-size estimates.
    pub minimum_free_space_reserve_bytes: u64,
}

/// Injected host adapters used by the synchronous component engine.
pub struct ManagedSemanticComponentAdapters {
    /// Catalog-ID-only artifact source.
    pub artifact_source: Arc<dyn core::ArtifactSource>,
    /// Free-space probe checked before downloads.
    pub free_space: Arc<dyn core::FreeSpaceProbe>,
    /// Worker/runtime activation validation.
    pub activation: Arc<dyn core::ActivationProbe>,
    /// Indexing pause controller used for explicit pauses and data moves.
    pub indexing: Arc<dyn core::IndexingController>,
    /// Worker shutdown boundary required before uninstalling executable components.
    pub quiescer: Arc<dyn core::ComponentQuiescer>,
    /// Authoritative enrolment inventory. Removal is unavailable without it.
    pub index_inventory: Option<Arc<dyn SemanticIndexInventory>>,
    /// Enrolment data removal implementation. Removal is unavailable without it.
    pub index_remover: Option<Arc<dyn SemanticIndexRemover>>,
}

struct PendingManagedOffer {
    offer: core::InstallationOffer,
    total_download_bytes: u64,
}

struct PendingIndexRemoval {
    enrolment_id: core::EnrolmentId,
    expected: core::EnrolmentDeletionCounts,
}

type IndexAdapters = (
    Arc<dyn SemanticIndexInventory>,
    Arc<dyn SemanticIndexRemover>,
);

/// Desktop-managed adapter over the signed component lifecycle engine.
pub struct ManagedSemanticComponentCapability {
    manager: core::ComponentManager,
    catalog: Arc<core::TrustedCatalog>,
    configuration: ManagedSemanticComponentConfiguration,
    adapters: ManagedSemanticComponentAdapters,
    offers: Mutex<BTreeMap<InstallationOfferId, PendingManagedOffer>>,
    index_removal_plans: Mutex<BTreeMap<SemanticIndexRemovalPlanId, PendingIndexRemoval>>,
    migration_plans: Mutex<BTreeMap<SemanticModelMigrationId, core::ModelMigrationPlan>>,
    lifecycle: Mutex<SemanticComponentLifecycle>,
    mutation: tokio::sync::Mutex<()>,
    paused: tokio::sync::Mutex<Option<Box<dyn core::IndexingPauseGuard>>>,
}

impl ManagedSemanticComponentCapability {
    /// Creates a managed adapter without loading state or initializing data directories.
    #[must_use]
    pub fn new(
        manager: core::ComponentManager,
        catalog: Arc<core::TrustedCatalog>,
        configuration: ManagedSemanticComponentConfiguration,
        adapters: ManagedSemanticComponentAdapters,
    ) -> Self {
        Self {
            manager,
            catalog,
            configuration,
            adapters,
            offers: Mutex::new(BTreeMap::new()),
            index_removal_plans: Mutex::new(BTreeMap::new()),
            migration_plans: Mutex::new(BTreeMap::new()),
            lifecycle: Mutex::new(SemanticComponentLifecycle::Absent),
            mutation: tokio::sync::Mutex::new(()),
            paused: tokio::sync::Mutex::new(None),
        }
    }

    fn executable_download_is_prohibited(&self) -> bool {
        self.configuration.distribution == DesktopSemanticDistribution::MacAppStore
            && self
                .configuration
                .runtime_and_worker_artifacts
                .iter()
                .filter_map(|artifact_id| self.catalog.artifact(artifact_id))
                .any(|artifact| {
                    matches!(
                        artifact.kind(),
                        core::ArtifactKind::Worker | core::ArtifactKind::Runtime
                    )
                })
    }

    fn set_lifecycle(&self, lifecycle: SemanticComponentLifecycle) {
        *self
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = lifecycle;
    }

    async fn set_lifecycle_after_component_mutation(&self, lifecycle: SemanticComponentLifecycle) {
        self.set_lifecycle(lifecycle_after_component_mutation(&self.paused, lifecycle).await);
    }

    fn retain_migration_plan(&self, plan: core::ModelMigrationPlan) -> SemanticModelMigrationPlan {
        let mapped = map_model_migration_plan(&plan);
        let id = mapped.id.clone();
        let mut plans = self
            .migration_plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        plans.clear();
        plans.insert(id, plan);
        mapped
    }

    fn index_adapters(&self) -> Result<IndexAdapters, SemanticComponentError> {
        let denied = || SemanticComponentError::AuthorityDenied {
            authority: SemanticComponentAuthority::DesktopManaged,
            operation: SemanticComponentOperation::RemoveIndex,
        };
        Ok((
            Arc::clone(self.adapters.index_inventory.as_ref().ok_or_else(denied)?),
            Arc::clone(self.adapters.index_remover.as_ref().ok_or_else(denied)?),
        ))
    }

    async fn remove_authoritative_index(
        &self,
        enrolment: core::EnrolmentId,
        confirmed: core::EnrolmentDeletionCounts,
        inventory: Arc<dyn SemanticIndexInventory>,
        remover: Arc<dyn SemanticIndexRemover>,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        let indexing = Arc::clone(&self.adapters.indexing);
        let manager = self.manager.clone();
        let result = run_component_blocking(move || {
            manager.run_serialized_lifecycle(|| {
                let authoritative = inventory.counts(&enrolment).map_err(|error| {
                    SemanticComponentError::IndexRemoval {
                        message: error.to_string(),
                    }
                })?;
                if authoritative != confirmed {
                    return Err(SemanticComponentError::IndexRemoval {
                        message: "confirmation no longer matches authoritative enrolment inventory"
                            .to_owned(),
                    });
                }
                let plan = core::EnrolmentDeletionPlan::new(enrolment, authoritative);
                let _pause =
                    indexing
                        .pause()
                        .map_err(|error| SemanticComponentError::Indexing {
                            message: error.to_string(),
                        })?;
                let deleted = remover.remove(&plan).map_err(|error| {
                    SemanticComponentError::IndexRemoval {
                        message: error.to_string(),
                    }
                })?;
                plan.complete(deleted)
                    .map_err(|error| SemanticComponentError::IndexRemoval {
                        message: error.to_string(),
                    })
            })
        })
        .await?;
        Ok(SemanticIndexRemovalReceipt {
            enrolment_id: result.enrolment_id().as_str().to_owned(),
            deleted: from_core_counts(result.deleted()),
            conversation_evidence_deleted: result.conversation_evidence_deleted(),
        })
    }
}

#[async_trait]
impl SemanticComponentCapability for ManagedSemanticComponentCapability {
    async fn capabilities(&self) -> SemanticComponentCapabilities {
        SemanticComponentCapabilities::desktop_managed(
            self.configuration.distribution,
            self.adapters.index_inventory.is_some() && self.adapters.index_remover.is_some(),
        )
    }

    async fn status(&self) -> Result<SemanticComponentStatus, SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        let manager = self.manager.clone();
        let (state, report) =
            run_component_blocking(move || manager.state_and_status().map_err(map_status_error))
                .await?;
        let observed = self
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        Ok(map_managed_status(&state, &report, observed))
    }

    async fn catalog_profiles(&self) -> Result<Vec<SemanticModelProfile>, SemanticComponentError> {
        SemanticProfile::all()
            .iter()
            .copied()
            .map(|profile| map_catalog_profile(&self.catalog, profile))
            .collect()
    }

    async fn installation_offer(
        &self,
        profile: SemanticProfile,
    ) -> Result<SemanticInstallationOffer, SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        if self.executable_download_is_prohibited() {
            return Err(SemanticComponentError::ExecutableDownloadProhibited);
        }
        let manager = self.manager.clone();
        let catalog = Arc::clone(&self.catalog);
        let artifact_ids = self
            .catalog
            .installation_artifacts(profile, &self.configuration.runtime_and_worker_artifacts)
            .map_err(map_catalog_error)?;
        let target = self.configuration.environment.target().clone();
        let protocol_version = self.configuration.environment.protocol_version();
        let reserve = self.configuration.minimum_free_space_reserve_bytes;
        let core_offer = run_component_blocking(move || {
            let state = manager.state().map_err(map_install_error)?;
            catalog
                .installation_offer(
                    profile,
                    &artifact_ids,
                    &target,
                    protocol_version,
                    state.data_root().path(),
                    reserve,
                )
                .map_err(map_catalog_error)
        })
        .await?;
        let id = InstallationOfferId(Uuid::new_v4().to_string());
        let offer = map_installation_offer(&core_offer, id.clone());
        let total_download_bytes = offer.components.iter().try_fold(0_u64, |total, component| {
            total.checked_add(component.download_bytes)
        });
        let total_download_bytes =
            total_download_bytes.ok_or_else(|| SemanticComponentError::State {
                message: "component download-size sum overflowed".to_owned(),
            })?;
        {
            let mut offers = self
                .offers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            offers.clear();
            offers.insert(
                id.clone(),
                PendingManagedOffer {
                    offer: core_offer,
                    total_download_bytes,
                },
            );
        }
        self.set_lifecycle_after_component_mutation(SemanticComponentLifecycle::Offered {
            offer_id: id,
        })
        .await;
        Ok(offer)
    }

    async fn install_or_enable(
        &self,
        consent: SemanticInstallationConsent,
    ) -> Result<SemanticInstallReceipt, SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        let pending = self
            .offers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&consent.offer_id)
            .ok_or(SemanticComponentError::ConsentRequired)?;
        let total_download_bytes = pending.total_download_bytes;
        self.set_lifecycle(SemanticComponentLifecycle::Downloading {
            downloaded_bytes: 0,
            total_bytes: total_download_bytes,
            resumable: true,
        });
        let manager = self.manager.clone();
        let catalog = Arc::clone(&self.catalog);
        let environment = self.configuration.environment.clone();
        let source = Arc::clone(&self.adapters.artifact_source);
        let free_space = Arc::clone(&self.adapters.free_space);
        let activation = Arc::clone(&self.adapters.activation);
        let result = run_component_blocking(move || {
            manager
                .install(
                    pending.offer.consent(),
                    &catalog,
                    &environment,
                    source.as_ref(),
                    free_space.as_ref(),
                    activation.as_ref(),
                )
                .map_err(map_install_error)
        })
        .await;
        match result {
            Ok(receipt) => {
                self.set_lifecycle_after_component_mutation(
                    SemanticComponentLifecycle::InstalledEnabled,
                )
                .await;
                Ok(SemanticInstallReceipt {
                    installed_artifact_ids: receipt
                        .installed_artifacts()
                        .iter()
                        .map(|artifact| artifact.as_str().to_owned())
                        .collect(),
                })
            }
            Err(error) => {
                let lifecycle = if let SemanticComponentError::InsufficientSpace {
                    available_bytes,
                    required_bytes,
                    ..
                } = &error
                {
                    SemanticComponentLifecycle::LowDisk {
                        available_bytes: *available_bytes,
                        required_bytes: *required_bytes,
                    }
                } else if !matches!(error, SemanticComponentError::DownloadInterrupted { .. }) {
                    SemanticComponentLifecycle::Absent
                } else {
                    SemanticComponentLifecycle::Downloading {
                        downloaded_bytes: 0,
                        total_bytes: total_download_bytes,
                        resumable: true,
                    }
                };
                self.set_lifecycle_after_component_mutation(lifecycle).await;
                Err(error)
            }
        }
    }

    async fn install_compatible_worker_patch(
        &self,
        request: SemanticWorkerPatchRequest,
    ) -> Result<Option<SemanticInstallReceipt>, SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        if self.configuration.distribution == DesktopSemanticDistribution::MacAppStore {
            return Err(SemanticComponentError::ExecutableDownloadProhibited);
        }
        let component_id =
            core::ComponentId::new(request.component_id).map_err(map_catalog_error)?;
        let manager = self.manager.clone();
        let catalog = Arc::clone(&self.catalog);
        let environment = self.configuration.environment.clone();
        let reserve = self.configuration.minimum_free_space_reserve_bytes;
        let selected = run_component_blocking(move || {
            let state = manager.state().map_err(map_install_error)?;
            let installed = state.installed_component(&component_id).ok_or_else(|| {
                SemanticComponentError::State {
                    message: format!("component `{}` is not installed", component_id.as_str()),
                }
            })?;
            let current_version = installed.version().clone();
            let Some(active_index_schema_version) = state.active_index_schema_version() else {
                return Ok(None);
            };
            let mut installed_runtimes = environment.installed_runtimes().clone();
            for component in state.installed_components() {
                if matches!(component.kind(), core::ArtifactKind::Runtime) {
                    installed_runtimes.insert(
                        component.component_id().clone(),
                        component.version().clone(),
                    );
                }
            }
            let update = catalog.worker_patch_update(
                &component_id,
                &current_version,
                active_index_schema_version,
                environment.target(),
                environment.protocol_version(),
                &installed_runtimes,
                reserve,
            );
            let Some(update) = update else {
                return Ok(None);
            };
            let artifact = catalog.artifact(update.artifact_id()).ok_or_else(|| {
                SemanticComponentError::Catalog {
                    message: "selected worker patch disappeared from the catalog".to_owned(),
                }
            })?;
            Ok(Some((
                update,
                current_version.to_string(),
                artifact.version().to_string(),
                artifact.resources().download_bytes(),
            )))
        })
        .await?;
        let Some((update, active_version, failed_version, download_bytes)) = selected else {
            return Ok(None);
        };
        self.set_lifecycle(SemanticComponentLifecycle::Downloading {
            downloaded_bytes: 0,
            total_bytes: download_bytes,
            resumable: true,
        });
        let manager = self.manager.clone();
        let catalog = Arc::clone(&self.catalog);
        let environment = self.configuration.environment.clone();
        let source = Arc::clone(&self.adapters.artifact_source);
        let free_space = Arc::clone(&self.adapters.free_space);
        let activation = Arc::clone(&self.adapters.activation);
        let result = run_component_blocking(move || {
            manager
                .install_worker_patch_update(
                    update,
                    &catalog,
                    &environment,
                    source.as_ref(),
                    free_space.as_ref(),
                    activation.as_ref(),
                )
                .map_err(map_install_error)
        })
        .await;
        match result {
            Ok(receipt) => {
                self.set_lifecycle_after_component_mutation(
                    SemanticComponentLifecycle::InstalledEnabled,
                )
                .await;
                Ok(Some(SemanticInstallReceipt {
                    installed_artifact_ids: receipt
                        .installed_artifacts()
                        .iter()
                        .map(|artifact| artifact.as_str().to_owned())
                        .collect(),
                }))
            }
            Err(error) => {
                let lifecycle = match &error {
                    SemanticComponentError::InsufficientSpace {
                        available_bytes,
                        required_bytes,
                        ..
                    } => SemanticComponentLifecycle::LowDisk {
                        available_bytes: *available_bytes,
                        required_bytes: *required_bytes,
                    },
                    _ => SemanticComponentLifecycle::UpdateFailedRolledBack {
                        failed_version,
                        active_version,
                    },
                };
                self.set_lifecycle_after_component_mutation(lifecycle).await;
                Err(error)
            }
        }
    }

    async fn pause_indexing(&self) -> Result<(), SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        let mut paused = self.paused.lock().await;
        if paused.is_some() {
            return Err(SemanticComponentError::InvalidLifecycle {
                operation: SemanticComponentOperation::PauseIndexing,
                lifecycle: "paused".to_owned(),
            });
        }
        let indexing = Arc::clone(&self.adapters.indexing);
        let manager = self.manager.clone();
        let guard = run_component_blocking(move || {
            manager.run_serialized_lifecycle(|| {
                indexing
                    .pause()
                    .map_err(|error| SemanticComponentError::Indexing {
                        message: error.to_string(),
                    })
            })
        })
        .await?;
        *paused = Some(guard);
        self.set_lifecycle(SemanticComponentLifecycle::Paused);
        Ok(())
    }

    async fn resume_indexing(&self) -> Result<(), SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        let guard = self.paused.lock().await.take().ok_or_else(|| {
            SemanticComponentError::InvalidLifecycle {
                operation: SemanticComponentOperation::ResumeIndexing,
                lifecycle: "not paused".to_owned(),
            }
        })?;
        let manager = self.manager.clone();
        run_component_blocking(move || {
            manager.run_serialized_lifecycle(|| {
                drop(guard);
                Ok(())
            })
        })
        .await?;
        self.set_lifecycle(SemanticComponentLifecycle::InstalledEnabled);
        Ok(())
    }

    async fn remove_index(
        &self,
        request: RemoveSemanticIndexRequest,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        let enrolment = core::EnrolmentId::new(request.enrolment_id.clone())
            .map_err(|_| SemanticComponentError::InvalidEnrolment)?;
        let (inventory, remover) = self.index_adapters()?;
        self.remove_authoritative_index(
            enrolment,
            to_core_counts(request.expected),
            inventory,
            remover,
        )
        .await
    }

    async fn plan_index_removal(
        &self,
        enrolment_id: String,
    ) -> Result<SemanticIndexRemovalPlan, SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        let enrolment = core::EnrolmentId::new(enrolment_id.clone())
            .map_err(|_| SemanticComponentError::InvalidEnrolment)?;
        let (inventory, _) = self.index_adapters()?;
        let manager = self.manager.clone();
        let inventory_enrolment = enrolment.clone();
        let expected = run_component_blocking(move || {
            manager.run_serialized_lifecycle(|| {
                inventory.counts(&inventory_enrolment).map_err(|error| {
                    SemanticComponentError::IndexRemoval {
                        message: error.to_string(),
                    }
                })
            })
        })
        .await?;
        let id = SemanticIndexRemovalPlanId(Uuid::new_v4().to_string());
        self.index_removal_plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                id.clone(),
                PendingIndexRemoval {
                    enrolment_id: enrolment,
                    expected,
                },
            );
        Ok(SemanticIndexRemovalPlan {
            id,
            enrolment_id,
            expected: from_core_counts(expected),
        })
    }

    async fn confirm_index_removal(
        &self,
        confirmation: SemanticIndexRemovalConfirmation,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        let pending = self
            .index_removal_plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&confirmation.plan_id)
            .ok_or_else(|| SemanticComponentError::IndexRemoval {
                message: "index removal plan is stale or unknown".to_owned(),
            })?;
        let (inventory, remover) = self.index_adapters()?;
        self.remove_authoritative_index(pending.enrolment_id, pending.expected, inventory, remover)
            .await
    }

    async fn move_data(
        &self,
        destination: PathBuf,
    ) -> Result<SemanticDataMoveReceipt, SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        let manager = self.manager.clone();
        let indexing = Arc::clone(&self.adapters.indexing);
        let receipt = run_component_blocking(move || {
            manager
                .move_data_root(&destination, indexing.as_ref(), &NeverCancelled)
                .map_err(|error| SemanticComponentError::DataMigration {
                    message: error.to_string(),
                })
        })
        .await?;
        Ok(SemanticDataMoveReceipt {
            source: receipt.source().to_owned(),
            destination: receipt.destination().to_owned(),
            verified_file_count: receipt.verified_file_count(),
            verified_bytes: receipt.verified_bytes(),
        })
    }

    async fn uninstall_components(
        &self,
        index_decision: SemanticIndexRetentionDecision,
    ) -> Result<SemanticUninstallReceipt, SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        let existing_pause = self.paused.lock().await.take();
        let was_paused = existing_pause.is_some();
        let manager = self.manager.clone();
        let quiescer = Arc::clone(&self.adapters.quiescer);
        let core_decision = match index_decision {
            SemanticIndexRetentionDecision::Retain => core::UninstallIndexDecision::Retain,
            SemanticIndexRetentionDecision::Delete => core::UninstallIndexDecision::Delete,
        };
        let receipt = run_component_blocking(move || {
            let result = match existing_pause {
                Some(pause) => {
                    manager.uninstall_while_paused(core_decision, pause, quiescer.as_ref())
                }
                None => manager.uninstall(core_decision, quiescer.as_ref()),
            };
            result.map_err(|error| SemanticComponentError::Filesystem {
                message: error.to_string(),
            })
        })
        .await;
        let receipt = match receipt {
            Ok(receipt) => receipt,
            Err(error) => {
                if was_paused {
                    self.set_lifecycle(SemanticComponentLifecycle::InstalledEnabled);
                }
                return Err(error);
            }
        };
        self.offers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.migration_plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.index_removal_plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.set_lifecycle(SemanticComponentLifecycle::Uninstalled { index_decision });
        Ok(SemanticUninstallReceipt {
            index_decision,
            removed_component_count: receipt.removed_component_count(),
        })
    }

    async fn import_local_model(
        &self,
        request: SemanticLocalModelImportRequest,
        profile: SemanticProfile,
        estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        let manager = self.manager.clone();
        let result = run_component_blocking(move || {
            let import = core::LocalModelImport::validate(to_core_import_request(request))
                .map_err(map_model_import_error)?;
            manager
                .plan_local_model_migration(
                    &import,
                    profile,
                    core::ReindexEstimate::new(estimate.documents(), estimate.source_bytes()),
                )
                .map_err(map_state_error)
        })
        .await?;
        Ok(self.retain_migration_plan(result))
    }

    async fn plan_model_migration(
        &self,
        profile: SemanticProfile,
        estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        let identity = self
            .catalog
            .resolve_profile(profile)
            .cloned()
            .ok_or_else(|| SemanticComponentError::Catalog {
                message: format!("profile {profile:?} has no signed model resolution"),
            })?;
        let manager = self.manager.clone();
        let plan = run_component_blocking(move || {
            manager
                .plan_model_migration(
                    profile,
                    identity,
                    core::ReindexEstimate::new(estimate.documents(), estimate.source_bytes()),
                )
                .map_err(map_state_error)
        })
        .await?;
        Ok(self.retain_migration_plan(plan))
    }

    async fn confirm_model_migration(
        &self,
        confirmation: SemanticModelMigrationConfirmation,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError> {
        let _mutation = self.mutation.lock().await;
        let plan = self
            .migration_plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&confirmation.migration_id)
            .cloned()
            .ok_or(SemanticComponentError::InvalidMigrationPlan)?;
        let manager = self.manager.clone();
        let pending = run_component_blocking(move || {
            manager
                .begin_model_migration(plan.confirm())
                .map_err(map_state_error)
        })
        .await?;
        self.migration_plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&confirmation.migration_id);
        let progress = map_pending_migration(&pending);
        self.set_lifecycle(SemanticComponentLifecycle::Migrating {
            progress: progress.clone(),
        });
        Ok(progress)
    }

    async fn checkpoint_model_migration(
        &self,
        _checkpoint: SemanticModelMigrationCheckpoint,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError> {
        Err(SemanticComponentError::AuthorityDenied {
            authority: SemanticComponentAuthority::DesktopManaged,
            operation: SemanticComponentOperation::CheckpointModelMigration,
        })
    }

    async fn complete_model_migration(
        &self,
        _migration_id: SemanticModelMigrationId,
    ) -> Result<SemanticModelSelection, SemanticComponentError> {
        Err(SemanticComponentError::AuthorityDenied {
            authority: SemanticComponentAuthority::DesktopManaged,
            operation: SemanticComponentOperation::CompleteModelMigration,
        })
    }
}

struct NeverCancelled;

async fn lifecycle_after_component_mutation(
    paused: &tokio::sync::Mutex<Option<Box<dyn core::IndexingPauseGuard>>>,
    lifecycle: SemanticComponentLifecycle,
) -> SemanticComponentLifecycle {
    if paused.lock().await.is_some() {
        SemanticComponentLifecycle::Paused
    } else {
        lifecycle
    }
}

impl core::DataMigrationCancellation for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }
}

async fn run_component_blocking<T, F>(work: F) -> Result<T, SemanticComponentError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, SemanticComponentError> + Send + 'static,
{
    tokio::task::spawn_blocking(work).await.map_err(|error| {
        SemanticComponentError::BlockingTaskFailed {
            message: error.to_string(),
        }
    })?
}

fn map_installation_offer(
    offer: &core::InstallationOffer,
    id: InstallationOfferId,
) -> SemanticInstallationOffer {
    SemanticInstallationOffer {
        id,
        catalog_revision: offer.catalog_revision().as_str().to_owned(),
        profile: offer.profile(),
        resolved_model: map_model_identity(offer.resolved_model()),
        components: offer
            .components()
            .iter()
            .map(|component| {
                let (kind, model) = map_artifact_kind(component.kind());
                SemanticComponentDisclosure {
                    artifact_id: component.artifact_id().as_str().to_owned(),
                    component_id: component.component_id().as_str().to_owned(),
                    kind,
                    model,
                    version: component.version().to_string(),
                    license: SemanticLicense {
                        spdx: component.license().spdx().to_owned(),
                        notice: component.license().notice().to_owned(),
                    },
                    download_bytes: component.download_bytes(),
                    estimated_installed_bytes: component.estimated_installed_bytes(),
                    estimated_ram_bytes: component.estimated_ram_bytes(),
                }
            })
            .collect(),
        embeddings_stay_local: offer.local_only_disclosure().embeddings_stay_local(),
        local_only_disclosure: offer.local_only_disclosure().text().to_owned(),
        data_root: offer.semantic_data_root().to_owned(),
        minimum_free_space_reserve_bytes: offer.minimum_free_space_reserve_bytes(),
    }
}

fn map_catalog_profile(
    catalog: &core::TrustedCatalog,
    profile: SemanticProfile,
) -> Result<SemanticModelProfile, SemanticComponentError> {
    let identity =
        catalog
            .resolve_profile(profile)
            .ok_or_else(|| SemanticComponentError::Catalog {
                message: format!("profile {profile:?} has no signed model resolution"),
            })?;
    let model = catalog
        .model(identity)
        .ok_or_else(|| SemanticComponentError::Catalog {
            message: format!(
                "resolved model `{}` at `{}` is absent",
                identity.model_id().as_str(),
                identity.revision().as_str()
            ),
        })?;
    Ok(SemanticModelProfile {
        profile,
        recommended: profile == SemanticProfile::recommended(),
        explanation: profile.explanation().to_owned(),
        resolved_model: map_model_identity(identity),
        metadata: map_model_metadata(model.metadata()),
    })
}

fn map_model_metadata(metadata: &core::ModelMetadata) -> SemanticModelMetadata {
    SemanticModelMetadata {
        identity: map_model_identity(metadata.identity()),
        license: SemanticLicense {
            spdx: metadata.license().spdx().to_owned(),
            notice: metadata.license().notice().to_owned(),
        },
        tokenizer: metadata.tokenizer().as_str().to_owned(),
        dimensions: metadata.dimensions(),
        normalization: match metadata.normalization() {
            core::EmbeddingNormalization::UnitLength => SemanticEmbeddingNormalization::UnitLength,
            core::EmbeddingNormalization::None => SemanticEmbeddingNormalization::None,
        },
        runtime_component_id: metadata.runtime().component_id().as_str().to_owned(),
        runtime_version_requirement: metadata.runtime().version_requirement().to_string(),
        language_coverage: metadata.language_coverage().to_vec(),
        estimated_disk_bytes: metadata.estimated_disk_bytes(),
        estimated_ram_bytes: metadata.estimated_ram_bytes(),
    }
}

fn map_model_identity(identity: &core::ModelIdentity) -> SemanticModelIdentity {
    SemanticModelIdentity::new(identity.model_id().as_str(), identity.revision().as_str())
}

fn map_model_selection(selection: &core::ResolvedModelSelection) -> SemanticModelSelection {
    SemanticModelSelection::new(
        selection.profile(),
        map_model_identity(selection.identity()),
    )
}

fn map_model_migration_plan(plan: &core::ModelMigrationPlan) -> SemanticModelMigrationPlan {
    SemanticModelMigrationPlan {
        id: SemanticModelMigrationId::new(plan.id().as_uuid().to_string()),
        from: plan.from().map(map_model_selection),
        target: map_model_selection(plan.target()),
        estimate: SemanticReindexEstimate::new(
            plan.estimate().documents(),
            plan.estimate().source_bytes(),
        ),
        reason: match plan.reason() {
            core::ReindexReason::ModelChanged => SemanticModelMigrationReason::ModelChanged,
            core::ReindexReason::SchemaChanged {
                from_version,
                to_version,
            } => SemanticModelMigrationReason::SchemaChanged {
                from_version,
                to_version,
            },
        },
    }
}

fn map_pending_migration(pending: &core::PendingModelMigration) -> SemanticModelMigrationProgress {
    SemanticModelMigrationProgress::new(
        SemanticModelMigrationId::new(pending.plan().id().as_uuid().to_string()),
        pending.completed_documents(),
        SemanticReindexEstimate::new(
            pending.plan().estimate().documents(),
            pending.plan().estimate().source_bytes(),
        ),
        map_model_selection(pending.plan().target()),
        match pending.plan().reason() {
            core::ReindexReason::ModelChanged => SemanticModelMigrationReason::ModelChanged,
            core::ReindexReason::SchemaChanged {
                from_version,
                to_version,
            } => SemanticModelMigrationReason::SchemaChanged {
                from_version,
                to_version,
            },
        },
        pending.resume_cursor().map(str::to_owned),
    )
}

fn to_core_import_request(
    request: SemanticLocalModelImportRequest,
) -> core::LocalModelImportRequest {
    core::LocalModelImportRequest {
        source_path: request.source_path,
        model_id: request.model_id,
        upstream_revision: request.upstream_revision,
        license_spdx: request.license_spdx,
        license_notice: request.license_notice,
        tokenizer: request.tokenizer,
        dimensions: request.dimensions,
        normalization: request
            .normalization
            .map(|normalization| match normalization {
                SemanticEmbeddingNormalization::UnitLength => {
                    core::EmbeddingNormalization::UnitLength
                }
                SemanticEmbeddingNormalization::None => core::EmbeddingNormalization::None,
            }),
        runtime_component_id: request.runtime_component_id,
        runtime_version_requirement: request.runtime_version_requirement,
        language_coverage: request.language_coverage,
        estimated_disk_bytes: request.estimated_disk_bytes,
        estimated_ram_bytes: request.estimated_ram_bytes,
    }
}

fn map_model_import_error(error: core::ModelImportError) -> SemanticComponentError {
    match error {
        core::ModelImportError::MissingMetadata { field }
        | core::ModelImportError::InvalidMetadata { field } => {
            SemanticComponentError::InvalidLocalModelMetadata {
                field: map_model_import_field(field),
            }
        }
        core::ModelImportError::InvalidSource => {
            SemanticComponentError::InvalidLocalModelMetadata {
                field: SemanticModelImportField::SourcePath,
            }
        }
        core::ModelImportError::SourceTooLarge => SemanticComponentError::Filesystem {
            message: "local model package size overflowed".to_owned(),
        },
        core::ModelImportError::Io(error) => SemanticComponentError::Filesystem {
            message: error.to_string(),
        },
    }
}

fn map_model_import_field(field: core::ModelImportField) -> SemanticModelImportField {
    match field {
        core::ModelImportField::SourcePath => SemanticModelImportField::SourcePath,
        core::ModelImportField::ModelId => SemanticModelImportField::ModelId,
        core::ModelImportField::UpstreamRevision => SemanticModelImportField::UpstreamRevision,
        core::ModelImportField::License => SemanticModelImportField::License,
        core::ModelImportField::Tokenizer => SemanticModelImportField::Tokenizer,
        core::ModelImportField::Dimensions => SemanticModelImportField::Dimensions,
        core::ModelImportField::Normalization => SemanticModelImportField::Normalization,
        core::ModelImportField::RuntimeCompatibility => {
            SemanticModelImportField::RuntimeCompatibility
        }
        core::ModelImportField::LanguageCoverage => SemanticModelImportField::LanguageCoverage,
        core::ModelImportField::EstimatedDiskBytes => SemanticModelImportField::EstimatedDiskBytes,
        core::ModelImportField::EstimatedRamBytes => SemanticModelImportField::EstimatedRamBytes,
    }
}

fn map_artifact_kind(
    kind: &core::ArtifactKind,
) -> (SemanticComponentKind, Option<SemanticModelIdentity>) {
    match kind {
        core::ArtifactKind::Worker => (SemanticComponentKind::Worker, None),
        core::ArtifactKind::Runtime => (SemanticComponentKind::Runtime, None),
        core::ArtifactKind::Model(identity) => (
            SemanticComponentKind::Model,
            Some(map_model_identity(identity)),
        ),
    }
}

fn map_managed_status(
    state: &core::SemanticState,
    report: &core::SemanticStatusReport,
    observed: SemanticComponentLifecycle,
) -> SemanticComponentStatus {
    let migration = state.pending_model_migration().map(map_pending_migration);
    let lifecycle = if let Some(progress) = migration.clone() {
        SemanticComponentLifecycle::Migrating { progress }
    } else if matches!(
        observed,
        SemanticComponentLifecycle::Offered { .. }
            | SemanticComponentLifecycle::Downloading { .. }
            | SemanticComponentLifecycle::Paused
            | SemanticComponentLifecycle::UpdateFailedRolledBack { .. }
            | SemanticComponentLifecycle::LowDisk { .. }
            | SemanticComponentLifecycle::Uninstalled { .. }
    ) {
        observed
    } else if report.components().is_empty() {
        SemanticComponentLifecycle::Absent
    } else {
        SemanticComponentLifecycle::InstalledEnabled
    };
    let components = report
        .components()
        .iter()
        .map(|component| InstalledSemanticComponent {
            artifact_id: component.artifact_id().as_str().to_owned(),
            component_id: component.component_id().as_str().to_owned(),
            kind: map_artifact_kind(component.kind()).0,
            version: component.version().to_string(),
            state: match component.status() {
                core::ComponentLifecycleStatus::Active => InstalledSemanticComponentState::Active,
                core::ComponentLifecycleStatus::Rollback => {
                    InstalledSemanticComponentState::Rollback
                }
            },
            installed_bytes: component.installed_bytes(),
        })
        .collect();
    let categories = report
        .disk_use()
        .categories()
        .iter()
        .map(|usage| {
            SemanticCategoryDiskUse::new(map_data_category(usage.category()), usage.bytes())
        })
        .collect();
    SemanticComponentStatus {
        lifecycle,
        data_root: Some(state.data_root().path().to_owned()),
        active_model: state.active_model().map(|selection| {
            SemanticModelSelection::new(
                selection.profile(),
                map_model_identity(selection.identity()),
            )
        }),
        migration,
        components,
        disk_use: SemanticDiskUse {
            categories,
            total_bytes: report.disk_use().total_bytes(),
        },
    }
}

fn map_data_category(category: core::DataCategory) -> SemanticDataCategory {
    match category {
        core::DataCategory::Catalog => SemanticDataCategory::Catalog,
        core::DataCategory::Extracted => SemanticDataCategory::Extracted,
        core::DataCategory::Zvec => SemanticDataCategory::Zvec,
        core::DataCategory::EmbeddingCache => SemanticDataCategory::EmbeddingCache,
        core::DataCategory::Models => SemanticDataCategory::Models,
        core::DataCategory::Workers => SemanticDataCategory::Workers,
    }
}

fn to_core_counts(counts: SemanticIndexRecordCounts) -> core::EnrolmentDeletionCounts {
    core::EnrolmentDeletionCounts {
        index_records: counts.index_records,
        extracted_files: counts.extracted_files,
        zvec_vectors: counts.zvec_vectors,
        cache_entries: counts.cache_entries,
        conversation_evidence: counts.conversation_evidence,
    }
}

fn from_core_counts(counts: core::EnrolmentDeletionCounts) -> SemanticIndexRecordCounts {
    SemanticIndexRecordCounts {
        index_records: counts.index_records,
        extracted_files: counts.extracted_files,
        zvec_vectors: counts.zvec_vectors,
        cache_entries: counts.cache_entries,
        conversation_evidence: counts.conversation_evidence,
    }
}

fn map_catalog_error(error: core::CatalogError) -> SemanticComponentError {
    SemanticComponentError::Catalog {
        message: error.to_string(),
    }
}

fn map_status_error(error: core::SemanticStatusError) -> SemanticComponentError {
    match error {
        core::SemanticStatusError::SizeOverflow => SemanticComponentError::DiskUseOverflow,
        core::SemanticStatusError::State(error) => map_state_error(error),
        core::SemanticStatusError::UnsafeEntry { path } => SemanticComponentError::Filesystem {
            message: format!(
                "semantic status encountered an unsafe entry: {}",
                path.display()
            ),
        },
        core::SemanticStatusError::Io(error) => SemanticComponentError::Filesystem {
            message: error.to_string(),
        },
    }
}

fn map_install_error(error: core::InstallError) -> SemanticComponentError {
    match error {
        core::InstallError::CatalogChanged | core::InstallError::DataRootChanged => {
            SemanticComponentError::ConsentRequired
        }
        core::InstallError::InsufficientSpace {
            available_bytes,
            required_bytes,
            reserve_bytes,
        } => SemanticComponentError::InsufficientSpace {
            available_bytes,
            required_bytes,
            reserve_bytes,
        },
        core::InstallError::DownloadInterrupted { artifact, source } => {
            SemanticComponentError::DownloadInterrupted {
                artifact_id: artifact.as_str().to_owned(),
                message: source.to_string(),
            }
        }

        core::InstallError::EmptyDownloadChunk { artifact } => {
            SemanticComponentError::DownloadInterrupted {
                artifact_id: artifact.as_str().to_owned(),
                message: "source returned an empty non-final chunk".to_owned(),
            }
        }
        core::InstallError::InstalledArtifactInvalid { artifact }
        | core::InstallError::ChecksumMismatch { artifact }
        | core::InstallError::ArtifactSizeMismatch { artifact, .. } => {
            SemanticComponentError::ArtifactVerificationFailed {
                artifact_id: artifact.as_str().to_owned(),
            }
        }
        core::InstallError::ActivationFailed { artifact, source } => {
            SemanticComponentError::ActivationFailed {
                artifact_id: artifact.as_str().to_owned(),
                message: source.to_string(),
            }
        }
        core::InstallError::Catalog(error) => map_catalog_error(error),
        core::InstallError::FreeSpace(error) => SemanticComponentError::FreeSpaceProbe {
            message: error.to_string(),
        },
        core::InstallError::DataRoot(error) => SemanticComponentError::Filesystem {
            message: error.to_string(),
        },
        core::InstallError::UnsafePath { path } => SemanticComponentError::Filesystem {
            message: format!("unsafe component installation path: {}", path.display()),
        },
        core::InstallError::State(error) => SemanticComponentError::State {
            message: error.to_string(),
        },
        core::InstallError::Io(error) => SemanticComponentError::Filesystem {
            message: error.to_string(),
        },
        core::InstallError::ComponentNotInstalled { component } => SemanticComponentError::State {
            message: format!("component `{}` is not installed", component.as_str()),
        },
        core::InstallError::StaleWorkerUpdate { component } => SemanticComponentError::State {
            message: format!("worker update for `{}` is stale", component.as_str()),
        },
        core::InstallError::MissingActiveModel => SemanticComponentError::State {
            message: "automatic worker update requires an active model".to_owned(),
        },
        core::InstallError::SizeOverflow => SemanticComponentError::State {
            message: "component size estimate overflowed".to_owned(),
        },
    }
}

fn map_state_error(error: core::SemanticStateError) -> SemanticComponentError {
    match error {
        core::SemanticStateError::StaleMigration | core::SemanticStateError::NoPendingMigration => {
            SemanticComponentError::InvalidMigrationPlan
        }
        core::SemanticStateError::InvalidMigrationProgress { completed, total }
        | core::SemanticStateError::MigrationIncomplete { completed, total } => {
            SemanticComponentError::InvalidMigrationProgress {
                completed_documents: completed,
                total_documents: total,
            }
        }
        other => SemanticComponentError::State {
            message: other.to_string(),
        },
    }
}

impl From<core::SemanticStateError> for SemanticComponentError {
    fn from(error: core::SemanticStateError) -> Self {
        map_state_error(error)
    }
}

/// Delegates semantic component intentions to one host-provided capability.
pub struct SemanticComponentService {
    capability: Arc<dyn SemanticComponentCapability>,
}

impl SemanticComponentService {
    /// Creates a service over one lifecycle capability.
    #[must_use]
    pub fn new(capability: Arc<dyn SemanticComponentCapability>) -> Self {
        Self { capability }
    }

    /// Creates an inert service that performs no filesystem or network work.
    #[must_use]
    pub fn unavailable() -> Self {
        Self::new(Arc::new(UnavailableSemanticComponentCapability::new()))
    }

    /// Reports authority, supported operations, and executable-download policy.
    pub async fn capabilities(&self) -> SemanticComponentCapabilities {
        self.capability.capabilities().await
    }

    /// Reports current lifecycle and disk-use state.
    pub async fn status(&self) -> Result<SemanticComponentStatus, SemanticComponentError> {
        self.capability.status().await
    }

    /// Returns signed-catalog profile choices.
    pub async fn catalog_profiles(
        &self,
    ) -> Result<Vec<SemanticModelProfile>, SemanticComponentError> {
        self.capability.catalog_profiles().await
    }

    /// Creates a complete disclosure before consent.
    pub async fn installation_offer(
        &self,
        profile: SemanticProfile,
    ) -> Result<SemanticInstallationOffer, SemanticComponentError> {
        self.capability.installation_offer(profile).await
    }

    /// Installs or enables one explicitly accepted offer.
    pub async fn install_or_enable(
        &self,
        consent: SemanticInstallationConsent,
    ) -> Result<SemanticInstallReceipt, SemanticComponentError> {
        self.capability.install_or_enable(consent).await
    }

    /// Applies the newest compatible signed worker patch, if available.
    pub async fn install_compatible_worker_patch(
        &self,
        request: SemanticWorkerPatchRequest,
    ) -> Result<Option<SemanticInstallReceipt>, SemanticComponentError> {
        self.capability
            .install_compatible_worker_patch(request)
            .await
    }

    /// Pauses indexing.
    pub async fn pause_indexing(&self) -> Result<(), SemanticComponentError> {
        self.capability.pause_indexing().await
    }

    /// Resumes indexing.
    pub async fn resume_indexing(&self) -> Result<(), SemanticComponentError> {
        self.capability.resume_indexing().await
    }

    /// Removes every record derived from one enrolment.
    pub async fn remove_index(
        &self,
        request: RemoveSemanticIndexRequest,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        self.capability.remove_index(request).await
    }

    /// Creates an authoritative, opaque confirmation plan for enrolment deletion.
    pub async fn plan_index_removal(
        &self,
        enrolment_id: String,
    ) -> Result<SemanticIndexRemovalPlan, SemanticComponentError> {
        self.capability.plan_index_removal(enrolment_id).await
    }

    /// Executes one live authoritative enrolment-deletion plan.
    pub async fn confirm_index_removal(
        &self,
        confirmation: SemanticIndexRemovalConfirmation,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        self.capability.confirm_index_removal(confirmation).await
    }

    /// Moves semantic data through the capability.
    pub async fn move_data(
        &self,
        destination: PathBuf,
    ) -> Result<SemanticDataMoveReceipt, SemanticComponentError> {
        self.capability.move_data(destination).await
    }

    /// Uninstalls components using an explicit index decision.
    pub async fn uninstall_components(
        &self,
        index_decision: SemanticIndexRetentionDecision,
    ) -> Result<SemanticUninstallReceipt, SemanticComponentError> {
        self.capability.uninstall_components(index_decision).await
    }

    /// Validates and plans an expert local-model migration.
    pub async fn import_local_model(
        &self,
        request: SemanticLocalModelImportRequest,
        profile: SemanticProfile,
        estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError> {
        self.capability
            .import_local_model(request, profile, estimate)
            .await
    }

    /// Plans migration to one signed-catalog profile resolution.
    pub async fn plan_model_migration(
        &self,
        profile: SemanticProfile,
        estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError> {
        self.capability
            .plan_model_migration(profile, estimate)
            .await
    }

    /// Confirms and begins a model migration.
    pub async fn confirm_model_migration(
        &self,
        confirmation: SemanticModelMigrationConfirmation,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError> {
        self.capability.confirm_model_migration(confirmation).await
    }

    /// Persists a model migration checkpoint.
    pub async fn checkpoint_model_migration(
        &self,
        checkpoint: SemanticModelMigrationCheckpoint,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError> {
        self.capability.checkpoint_model_migration(checkpoint).await
    }

    /// Completes a fully reindexed model migration.
    pub async fn complete_model_migration(
        &self,
        migration_id: SemanticModelMigrationId,
    ) -> Result<SemanticModelSelection, SemanticComponentError> {
        self.capability.complete_model_migration(migration_id).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    struct TestPauseGuard {
        resumed: Arc<AtomicBool>,
    }

    impl core::IndexingPauseGuard for TestPauseGuard {}

    impl Drop for TestPauseGuard {
        fn drop(&mut self) {
            self.resumed.store(true, Ordering::SeqCst);
        }
    }

    async fn assert_outcome_preserves_pause_and_resume(outcome: SemanticComponentLifecycle) {
        let resumed = Arc::new(AtomicBool::new(false));
        let paused: tokio::sync::Mutex<Option<Box<dyn core::IndexingPauseGuard>>> =
            tokio::sync::Mutex::new(Some(Box::new(TestPauseGuard {
                resumed: Arc::clone(&resumed),
            })));

        assert_eq!(
            lifecycle_after_component_mutation(&paused, outcome).await,
            SemanticComponentLifecycle::Paused
        );
        assert!(paused.lock().await.is_some());

        drop(paused.lock().await.take().expect("resume guard remains"));
        assert!(resumed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn install_outcome_preserves_pause_and_leaves_resume_available() {
        assert_outcome_preserves_pause_and_resume(SemanticComponentLifecycle::InstalledEnabled)
            .await;
    }

    #[tokio::test]
    async fn patch_outcome_preserves_pause_and_leaves_resume_available() {
        assert_outcome_preserves_pause_and_resume(
            SemanticComponentLifecycle::UpdateFailedRolledBack {
                failed_version: "1.2.2".to_owned(),
                active_version: "1.2.1".to_owned(),
            },
        )
        .await;
    }
}
