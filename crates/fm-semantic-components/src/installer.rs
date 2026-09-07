use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::catalog::ensure_artifact_compatibility;
use crate::state::read_file_no_follow;
use crate::{
    ArtifactId, ArtifactKind, CatalogArtifact, CatalogError, ComponentId, DataCategory,
    DataRootError, InstallationOffer, ManifestRevision, ModelIdentity, SemanticDataRoot,
    SemanticProfile, SemanticState, SemanticStateError, SemanticStateStore, Sha256Digest,
    TargetTriple, TrustedCatalog,
};

/// Offset-based request for one trusted catalog artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactRequest {
    artifact_id: ArtifactId,
    offset: u64,
}

impl ArtifactRequest {
    /// Creates a resume request for one signed-catalog artifact.
    #[must_use]
    pub const fn new(artifact_id: ArtifactId, offset: u64) -> Self {
        Self {
            artifact_id,
            offset,
        }
    }

    /// Returns the opaque signed-catalog artifact identifier.
    #[must_use]
    pub const fn artifact_id(&self) -> &ArtifactId {
        &self.artifact_id
    }

    /// Returns the byte offset from which a retained partial download resumes.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }
}

/// One sequential artifact response chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactChunk {
    bytes: Vec<u8>,
    complete: bool,
}

impl ArtifactChunk {
    /// Creates a chunk and marks whether it finishes the artifact.
    #[must_use]
    pub fn new(bytes: Vec<u8>, complete: bool) -> Self {
        Self { bytes, complete }
    }

    /// Returns the chunk's bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns whether this chunk finishes the artifact.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.complete
    }
}

/// Source boundary that can request only catalog artifact IDs, never URLs.
pub trait ArtifactSource: Send + Sync {
    /// Reads the next chunk at an exact resume offset.
    ///
    /// # Errors
    ///
    /// Returns a typed source availability or range failure.
    fn read(&self, request: &ArtifactRequest) -> Result<ArtifactChunk, ArtifactSourceError>;
}

/// Artifact source failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ArtifactSourceError {
    /// Artifact data could not currently be obtained.
    #[error("artifact source unavailable: {0}")]
    Unavailable(String),
    /// The source could not honour the requested offset.
    #[error("artifact source rejected resume offset {offset}")]
    InvalidOffset {
        /// Rejected byte offset.
        offset: u64,
    },
}

/// Injected free-space query used before any download.
pub trait FreeSpaceProbe: Send + Sync {
    /// Returns bytes available at the semantic-data filesystem.
    ///
    /// # Errors
    ///
    /// Returns a typed platform probing failure.
    fn available_bytes(&self, path: &Path) -> Result<u64, FreeSpaceError>;
}

/// Free-space query failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("free-space query failed: {message}")]
pub struct FreeSpaceError {
    message: String,
}

impl FreeSpaceError {
    /// Creates a probe failure without exposing platform-specific error types.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Post-download worker/runtime validation boundary.
pub trait ActivationProbe: Send + Sync {
    /// Validates a verified staged artifact before it can become active.
    ///
    /// # Errors
    ///
    /// Returns a typed component startup or compatibility failure.
    fn validate(
        &self,
        artifact: &CatalogArtifact,
        installed_path: &Path,
    ) -> Result<(), ActivationError>;
}

/// Host boundary that permanently quiesces indexing and semantic workers before uninstall.
pub trait ComponentQuiescer: Send + Sync {
    /// Stops worker activity and releases component/index file handles.
    ///
    /// # Errors
    ///
    /// Returns a host diagnostic when workers cannot be stopped safely.
    fn quiesce(&self) -> Result<(), QuiesceError>;
}

/// Worker/indexing quiescence failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("semantic workers could not be quiesced: {message}")]
pub struct QuiesceError {
    message: String,
}

impl QuiesceError {
    /// Creates a quiescence diagnostic.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Component activation validation failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("component activation validation failed: {message}")]
pub struct ActivationError {
    message: String,
}

impl ActivationError {
    /// Creates an activation validation failure.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Host platform and already installed runtime versions used for compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallEnvironment {
    target: TargetTriple,
    protocol_version: u32,
    installed_runtimes: BTreeMap<ComponentId, Version>,
}

impl InstallEnvironment {
    /// Creates an explicit installation compatibility context.
    #[must_use]
    pub const fn new(
        target: TargetTriple,
        protocol_version: u32,
        installed_runtimes: BTreeMap<ComponentId, Version>,
    ) -> Self {
        Self {
            target,
            protocol_version,
            installed_runtimes,
        }
    }

    /// Returns the target operating system and architecture.
    #[must_use]
    pub const fn target(&self) -> &TargetTriple {
        &self.target
    }

    /// Returns the semantic worker protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> u32 {
        self.protocol_version
    }

    /// Returns runtime versions already available to installed components.
    #[must_use]
    pub const fn installed_runtimes(&self) -> &BTreeMap<ComponentId, Version> {
        &self.installed_runtimes
    }
}

/// Unforgeable-by-default record of consent to one signed installation offer.
#[derive(Debug)]
pub struct InstallationConsent {
    catalog_revision: ManifestRevision,
    profile: SemanticProfile,
    resolved_model: ModelIdentity,
    artifact_ids: Vec<ArtifactId>,
    semantic_data_root: PathBuf,
    minimum_free_space_reserve_bytes: u64,
}

/// Catalog-authorized compatible worker patch that may install automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerPatchUpdate {
    catalog_revision: ManifestRevision,
    component_id: ComponentId,
    current_version: Version,
    current_index_schema_version: u32,
    artifact_id: ArtifactId,
    minimum_free_space_reserve_bytes: u64,
}

impl WorkerPatchUpdate {
    pub(crate) fn new(
        catalog_revision: ManifestRevision,
        component_id: ComponentId,
        current_version: Version,
        current_index_schema_version: u32,
        artifact_id: ArtifactId,
        minimum_free_space_reserve_bytes: u64,
    ) -> Self {
        Self {
            catalog_revision,
            component_id,
            current_version,
            current_index_schema_version,
            artifact_id,
            minimum_free_space_reserve_bytes,
        }
    }

    /// Returns the signed patch artifact.
    #[must_use]
    pub const fn artifact_id(&self) -> &ArtifactId {
        &self.artifact_id
    }
}

impl InstallationConsent {
    pub(crate) fn from_offer(offer: InstallationOffer) -> Self {
        let (
            catalog_revision,
            profile,
            resolved_model,
            artifact_ids,
            semantic_data_root,
            minimum_free_space_reserve_bytes,
        ) = offer.into_installation_parts();
        Self {
            catalog_revision,
            profile,
            resolved_model,
            artifact_ids,
            semantic_data_root,
            minimum_free_space_reserve_bytes,
        }
    }
}

/// Successful atomic installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReceipt {
    installed_artifacts: Vec<ArtifactId>,
    cleanup_issues: Vec<ComponentCleanupIssue>,
}

/// Non-fatal cleanup diagnostic after a component state commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentCleanupIssue {
    path: PathBuf,
    message: String,
}

/// Required decision for semantic index data during component uninstall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallIndexDecision {
    /// Keep extracted data, vectors, and embedding cache for later reinstall.
    Retain,
    /// Delete extracted data, vectors, and embedding cache.
    Delete,
}

const UNINSTALL_JOURNAL_FILE_NAME: &str = "semantic-uninstall.json";
const UNINSTALL_JOURNAL_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct UninstallJournal {
    format_version: u32,
    transaction_id: Uuid,
    delete_indexes: bool,
    original_state: SemanticState,
    target_state: SemanticState,
    staged_categories: Vec<DataCategory>,
}

/// Successful component uninstall result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UninstallReceipt {
    index_decision: UninstallIndexDecision,
    removed_component_count: u64,
}

impl UninstallReceipt {
    /// Returns the explicit index handling decision.
    #[must_use]
    pub const fn index_decision(self) -> UninstallIndexDecision {
        self.index_decision
    }

    /// Returns active and rollback component records removed from state.
    #[must_use]
    pub const fn removed_component_count(self) -> u64 {
        self.removed_component_count
    }
}

impl InstallReceipt {
    /// Returns every artifact activated by the transaction.
    #[must_use]
    pub fn installed_artifacts(&self) -> &[ArtifactId] {
        &self.installed_artifacts
    }

    /// Returns cleanup operations that could not be durably completed.
    ///
    /// Active and rollback payloads remain intact; a later installation retries
    /// collection.
    #[must_use]
    pub fn cleanup_issues(&self) -> &[ComponentCleanupIssue] {
        &self.cleanup_issues
    }
}

impl ComponentCleanupIssue {
    /// Returns the stale managed path that could not be collected.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the filesystem diagnostic.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Consent-gated semantic component lifecycle engine.
#[derive(Debug, Clone)]
pub struct ComponentManager {
    store: SemanticStateStore,
    app_data: PathBuf,
}

struct InstallActivation<'a> {
    probe: &'a dyn ActivationProbe,
    activate_model: bool,
}

impl ComponentManager {
    /// Creates a manager using injected state and platform app-data locations.
    #[must_use]
    pub fn new(store: SemanticStateStore, app_data: impl Into<PathBuf>) -> Self {
        Self {
            store,
            app_data: app_data.into(),
        }
    }

    /// Runs one host lifecycle mutation under the same in-process and durable
    /// lock used by component state transactions.
    ///
    /// # Errors
    ///
    /// Returns lock/recovery failures through the caller's error type.
    pub fn run_serialized_lifecycle<T, E>(
        &self,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<SemanticStateError>,
    {
        self.store.with_exclusive_lock(|| {
            recover_interrupted_uninstall(&self.store, &self.app_data)?;
            operation()
        })
    }

    /// Loads current durable component state.
    ///
    /// # Errors
    ///
    /// Returns a typed state persistence failure.
    pub fn state(&self) -> Result<SemanticState, InstallError> {
        self.store.with_exclusive_lock(|| {
            self.store
                .load_or_default_unlocked(&self.app_data)
                .map_err(InstallError::State)
        })
    }

    /// Reports installed components and actual per-category disk use.
    ///
    /// # Errors
    ///
    /// Returns a typed state or filesystem scanning failure.
    pub fn status(&self) -> Result<crate::SemanticStatusReport, crate::SemanticStatusError> {
        self.state_and_status().map(|(_, report)| report)
    }

    /// Resolves and re-verifies one active installed payload against its trusted catalog entry.
    ///
    /// Returns `Ok(None)` when the component is not active or belongs to another artifact
    /// generation. The file type, length, and SHA-256 are checked again before its path leaves the
    /// component manager so worker launch never trusts stale durable state alone.
    ///
    /// # Errors
    ///
    /// Returns a typed state or integrity error when durable state cannot be read or the recorded
    /// payload is missing, unsafe, truncated, or tampered.
    pub fn verified_installed_payload(
        &self,
        artifact: &CatalogArtifact,
    ) -> Result<Option<PathBuf>, InstallError> {
        let state = self.state()?;
        let Some(installed) = state.installed_component(artifact.component_id()) else {
            return Ok(None);
        };
        if installed.artifact_id() != artifact.id()
            || installed.version() != artifact.version()
            || installed.checksum() != artifact.checksum()
        {
            return Ok(None);
        }
        verify_installed_artifact(artifact, installed.installed_path())?;
        Ok(Some(installed.installed_path().to_owned()))
    }

    /// Loads one state snapshot and its corresponding filesystem report.
    ///
    /// # Errors
    ///
    /// Returns a typed state or filesystem scanning failure.
    pub fn state_and_status(
        &self,
    ) -> Result<(SemanticState, crate::SemanticStatusReport), crate::SemanticStatusError> {
        self.store.with_exclusive_lock(|| {
            let state = self.store.load_or_default_unlocked(&self.app_data)?;
            let report = crate::report::build_status(&state)?;
            Ok((state, report))
        })
    }

    /// Moves semantic data with pause-copy-verify-switch semantics.
    ///
    /// Source data is retained after the verified switch.
    ///
    /// # Errors
    ///
    /// Returns a typed pause, cancellation, filesystem, validation, or state
    /// persistence failure. Active state remains unchanged on failure.
    pub fn move_data_root(
        &self,
        destination: &Path,
        indexing: &dyn crate::IndexingController,
        cancellation: &dyn crate::DataMigrationCancellation,
    ) -> Result<crate::DataRootMigrationReceipt, crate::DataRootMigrationError> {
        self.store.with_exclusive_lock(|| {
            crate::data_root::migrate(
                &self.store,
                &self.app_data,
                destination,
                indexing,
                cancellation,
            )
        })
    }

    /// Plans an explicit migration to a validated local model.
    ///
    /// # Errors
    ///
    /// Returns a typed state error when the target is not distinct or another
    /// migration is already pending.
    pub fn plan_local_model_migration(
        &self,
        import: &crate::LocalModelImport,
        profile: crate::SemanticProfile,
        estimate: crate::ReindexEstimate,
    ) -> Result<crate::ModelMigrationPlan, crate::SemanticStateError> {
        self.store.with_exclusive_lock(|| {
            self.store
                .load_or_default_unlocked(&self.app_data)?
                .plan_local_model_migration(import, profile, estimate)
        })
    }

    /// Plans an explicit migration to an exact curated model identity.
    ///
    /// # Errors
    ///
    /// Returns a typed state error when the target is not distinct or another
    /// migration is already pending.
    pub fn plan_model_migration(
        &self,
        profile: crate::SemanticProfile,
        identity: crate::ModelIdentity,
        estimate: crate::ReindexEstimate,
    ) -> Result<crate::ModelMigrationPlan, crate::SemanticStateError> {
        self.store.with_exclusive_lock(|| {
            self.store
                .load_or_default_unlocked(&self.app_data)?
                .plan_model_migration(profile, identity, estimate)
        })
    }

    /// Begins and durably persists a confirmed full reindex.
    ///
    /// # Errors
    ///
    /// Returns a typed state error for a stale plan or persistence failure.
    pub fn begin_model_migration(
        &self,
        confirmed: crate::ConfirmedModelMigration,
    ) -> Result<crate::PendingModelMigration, crate::SemanticStateError> {
        self.store.with_exclusive_lock(|| {
            let mut state = self.store.load_or_default_unlocked(&self.app_data)?;
            state.begin_model_migration(confirmed)?;
            self.store.save_unlocked(&state)?;
            state
                .pending_model_migration()
                .cloned()
                .ok_or(crate::SemanticStateError::NoPendingMigration)
        })
    }

    /// Advances and durably persists one resumable full-reindex checkpoint.
    ///
    /// # Errors
    ///
    /// Returns a typed state error for a stale identity, invalid progress, or
    /// persistence failure.
    pub fn checkpoint_model_migration(
        &self,
        migration_id: crate::MigrationId,
        completed_documents: u64,
        resume_cursor: Option<String>,
    ) -> Result<crate::PendingModelMigration, crate::SemanticStateError> {
        self.store.with_exclusive_lock(|| {
            let mut state = self.store.load_or_default_unlocked(&self.app_data)?;
            if state
                .pending_model_migration()
                .is_none_or(|pending| pending.plan().id() != migration_id)
            {
                return Err(crate::SemanticStateError::StaleMigration);
            }
            state.checkpoint_model_migration(completed_documents, resume_cursor)?;
            self.store.save_unlocked(&state)?;
            state
                .pending_model_migration()
                .cloned()
                .ok_or(crate::SemanticStateError::NoPendingMigration)
        })
    }

    /// Activates a fully reindexed target and durably clears its checkpoint.
    ///
    /// # Errors
    ///
    /// Returns a typed state error for a stale identity, incomplete work, or
    /// persistence failure.
    pub fn complete_model_migration(
        &self,
        migration_id: crate::MigrationId,
    ) -> Result<crate::ResolvedModelSelection, crate::SemanticStateError> {
        self.store.with_exclusive_lock(|| {
            let mut state = self.store.load_or_default_unlocked(&self.app_data)?;
            if state
                .pending_model_migration()
                .is_none_or(|pending| pending.plan().id() != migration_id)
            {
                return Err(crate::SemanticStateError::StaleMigration);
            }
            state.complete_model_migration()?;
            self.store.save_unlocked(&state)?;
            state
                .active_model()
                .cloned()
                .ok_or(crate::SemanticStateError::MissingActiveModel)
        })
    }

    /// Installs exactly the components covered by explicit consent.
    ///
    /// # Errors
    ///
    /// Rejects changed catalogs, incompatible artifacts, insufficient free
    /// space, failed downloads, invalid checksums, and activation failures.
    pub fn install(
        &self,
        consent: InstallationConsent,
        catalog: &TrustedCatalog,
        environment: &InstallEnvironment,
        source: &dyn ArtifactSource,
        free_space: &dyn FreeSpaceProbe,
        activation: &dyn ActivationProbe,
    ) -> Result<InstallReceipt, InstallError> {
        self.store.with_exclusive_lock(|| {
            self.install_locked(
                consent,
                catalog,
                environment,
                source,
                free_space,
                InstallActivation {
                    probe: activation,
                    activate_model: true,
                },
            )
        })
    }

    /// Installs and verifies the artifacts for a reviewed model migration
    /// without replacing the active embedding space.
    ///
    /// The migration transaction activates the staged model only after its
    /// durable reindex checkpoint has completed.
    pub fn install_for_model_migration(
        &self,
        consent: InstallationConsent,
        catalog: &TrustedCatalog,
        environment: &InstallEnvironment,
        source: &dyn ArtifactSource,
        free_space: &dyn FreeSpaceProbe,
        activation: &dyn ActivationProbe,
    ) -> Result<InstallReceipt, InstallError> {
        self.store.with_exclusive_lock(|| {
            self.install_locked(
                consent,
                catalog,
                environment,
                source,
                free_space,
                InstallActivation {
                    probe: activation,
                    activate_model: false,
                },
            )
        })
    }

    fn install_locked(
        &self,
        consent: InstallationConsent,
        catalog: &TrustedCatalog,
        environment: &InstallEnvironment,
        source: &dyn ArtifactSource,
        free_space: &dyn FreeSpaceProbe,
        activation: InstallActivation<'_>,
    ) -> Result<InstallReceipt, InstallError> {
        if consent.catalog_revision != *catalog.revision() {
            return Err(InstallError::CatalogChanged);
        }
        let mut state = self
            .store
            .load_or_default_unlocked(&self.app_data)
            .map_err(InstallError::State)?;
        if state.data_root().path() != consent.semantic_data_root {
            return Err(InstallError::DataRootChanged);
        }
        if activation.activate_model {
            state.ensure_model_can_activate(consent.profile, &consent.resolved_model)?;
        }

        let artifacts: Vec<CatalogArtifact> = consent
            .artifact_ids
            .iter()
            .map(|id| {
                catalog
                    .artifact(id)
                    .cloned()
                    .ok_or_else(|| CatalogError::UnknownArtifact { id: id.clone() })
            })
            .collect::<Result<_, _>>()?;
        let mut runtime_versions = environment.installed_runtimes.clone();
        for component in state.installed_components() {
            if matches!(component.kind(), ArtifactKind::Runtime) {
                runtime_versions.insert(
                    component.component_id().clone(),
                    component.version().clone(),
                );
            }
        }
        for artifact in &artifacts {
            if matches!(artifact.kind(), ArtifactKind::Runtime) {
                runtime_versions
                    .insert(artifact.component_id().clone(), artifact.version().clone());
            }
        }
        for artifact in &artifacts {
            ensure_artifact_compatibility(
                artifact,
                &environment.target,
                environment.protocol_version,
                &runtime_versions,
            )?;
        }
        let offered_index_schema_version =
            artifacts.iter().find_map(|artifact| match artifact.kind() {
                ArtifactKind::Model(identity) if identity == &consent.resolved_model => {
                    Some(artifact.compatibility().index_schema_version())
                }
                _ => None,
            });
        if activation.activate_model
            && let Some(index_schema_version) = offered_index_schema_version
        {
            state.ensure_index_schema_can_activate(index_schema_version)?;
        }

        state.data_root().initialize()?;
        let mut already_installed = Vec::new();
        let mut retained_for_activation = Vec::new();
        let mut pending_artifacts = Vec::new();
        for artifact in artifacts {
            if let Some(installed) = state.retained_component(artifact.id()) {
                let was_active = state
                    .installed_component(artifact.component_id())
                    .is_some_and(|active| active.artifact_id() == artifact.id());
                let installed_path = installed.installed_path().to_owned();
                verify_installed_artifact(&artifact, installed.installed_path())?;
                already_installed.push(artifact.id().clone());
                if !was_active {
                    retained_for_activation.push((artifact, installed_path));
                }
            } else {
                ensure_artifact_paths_safe(state.data_root(), &artifact)?;
                recover_interrupted_commit(state.data_root(), &artifact)?;
                pending_artifacts.push(artifact);
            }
        }

        let mut required_bytes = consent.minimum_free_space_reserve_bytes;
        for artifact in &pending_artifacts {
            let retained_bytes = retained_partial_bytes(state.data_root(), artifact)?;
            let peak_artifact_bytes = artifact
                .resources()
                .download_bytes()
                .max(artifact.resources().installed_bytes());
            required_bytes = required_bytes
                .checked_add(peak_artifact_bytes.saturating_sub(retained_bytes))
                .ok_or(InstallError::SizeOverflow)?;
        }
        let available = free_space.available_bytes(&consent.semantic_data_root)?;
        if available < required_bytes {
            return Err(InstallError::InsufficientSpace {
                available_bytes: available,
                required_bytes,
                reserve_bytes: consent.minimum_free_space_reserve_bytes,
            });
        }

        let mut prepared = Vec::with_capacity(pending_artifacts.len());
        for artifact in &pending_artifacts {
            match prepare_artifact(state.data_root(), artifact, source, activation.probe) {
                Ok(item) => prepared.push(item),
                Err(error) => {
                    restore_prepared(&prepared);
                    return Err(error);
                }
            }
        }

        let mut committed = Vec::with_capacity(prepared.len());
        let mut remaining = prepared.into_iter();
        while let Some(item) = remaining.next() {
            let parent = match item.final_directory.parent() {
                Some(parent) => parent.to_owned(),
                None => {
                    let mut uncommitted = vec![item];
                    uncommitted.extend(remaining);
                    restore_prepared(&uncommitted);
                    rollback_committed(&committed);
                    return Err(InstallError::UnsafePath {
                        path: uncommitted[0].final_directory.clone(),
                    });
                }
            };
            let rename_result =
                ensure_confined_directory(state.data_root(), &parent).and_then(|()| {
                    reject_symlink_or_unexpected_type(&item.ready_directory, true)?;
                    fs::rename(&item.ready_directory, &item.final_directory).map_err(Into::into)
                });
            if let Err(error) = rename_result {
                let mut uncommitted = vec![item];
                uncommitted.extend(remaining);
                restore_prepared(&uncommitted);
                rollback_committed(&committed);
                return Err(error);
            }
            committed.push(CommittedArtifact {
                artifact: item.artifact,
                partial_path: item.partial_path,
                final_directory: item.final_directory.clone(),
                installed_path: item.final_directory.join("payload"),
            });
            if let Err(error) = self
                .store
                .sync_directory(&item.final_directory)
                .and_then(|()| self.store.sync_directory(&parent))
            {
                let uncommitted: Vec<_> = remaining.collect();
                restore_prepared(&uncommitted);
                rollback_committed(&committed);
                return Err(error.into());
            }
        }

        for item in &committed {
            state.record_installed(
                item.artifact.id().clone(),
                item.artifact.component_id().clone(),
                item.artifact.kind().clone(),
                item.artifact.version().clone(),
                item.artifact.checksum(),
                item.installed_path.clone(),
            );
        }
        for (artifact, installed_path) in retained_for_activation {
            state.record_installed(
                artifact.id().clone(),
                artifact.component_id().clone(),
                artifact.kind().clone(),
                artifact.version().clone(),
                artifact.checksum(),
                installed_path,
            );
        }
        if activation.activate_model
            && let Some(index_schema_version) = offered_index_schema_version
        {
            state.activate_installed_model(
                consent.profile,
                consent.resolved_model,
                index_schema_version,
            )?;
        }
        if let Err(error) = self.store.save_unlocked(&state) {
            if !error.commit_outcome_unknown() {
                rollback_committed(&committed);
            }
            return Err(error.into());
        }
        let cleanup_issues = collect_superseded_component_versions(&self.store, &state);
        already_installed.extend(committed.into_iter().map(|item| item.artifact.id().clone()));
        Ok(InstallReceipt {
            installed_artifacts: already_installed,
            cleanup_issues,
        })
    }

    /// Installs a catalog-authorized compatible worker patch without model or
    /// schema migration.
    ///
    /// # Errors
    ///
    /// Returns a typed error if active state changed after update selection or
    /// if the normal atomic installation checks fail.
    pub fn install_worker_patch_update(
        &self,
        update: WorkerPatchUpdate,
        catalog: &TrustedCatalog,
        environment: &InstallEnvironment,
        source: &dyn ArtifactSource,
        free_space: &dyn FreeSpaceProbe,
        activation: &dyn ActivationProbe,
    ) -> Result<InstallReceipt, InstallError> {
        self.store.with_exclusive_lock(|| {
            self.install_worker_patch_update_locked(
                update,
                catalog,
                environment,
                source,
                free_space,
                activation,
            )
        })
    }

    fn install_worker_patch_update_locked(
        &self,
        update: WorkerPatchUpdate,
        catalog: &TrustedCatalog,
        environment: &InstallEnvironment,
        source: &dyn ArtifactSource,
        free_space: &dyn FreeSpaceProbe,
        activation: &dyn ActivationProbe,
    ) -> Result<InstallReceipt, InstallError> {
        let state = self
            .store
            .load_or_default_unlocked(&self.app_data)
            .map_err(InstallError::State)?;
        let active_worker = state
            .installed_component(&update.component_id)
            .ok_or_else(|| InstallError::ComponentNotInstalled {
                component: update.component_id.clone(),
            })?;
        if active_worker.version() != &update.current_version {
            return Err(InstallError::StaleWorkerUpdate {
                component: update.component_id,
            });
        }
        if state.active_index_schema_version() != Some(update.current_index_schema_version) {
            return Err(InstallError::StaleWorkerUpdate {
                component: update.component_id,
            });
        }
        let active_model = state
            .active_model()
            .ok_or(InstallError::MissingActiveModel)?;
        self.install_locked(
            InstallationConsent {
                catalog_revision: update.catalog_revision,
                profile: active_model.profile(),
                resolved_model: active_model.identity().clone(),
                artifact_ids: vec![update.artifact_id],
                semantic_data_root: state.data_root().path().to_owned(),
                minimum_free_space_reserve_bytes: update.minimum_free_space_reserve_bytes,
            },
            catalog,
            environment,
            source,
            free_space,
            InstallActivation {
                probe: activation,
                activate_model: true,
            },
        )
    }

    /// Removes installed components using an explicit index retention decision.
    ///
    /// # Errors
    ///
    /// Returns a typed state or filesystem failure. Component/category moves
    /// are restored if durable state cannot be switched.
    pub fn uninstall(
        &self,
        index_decision: UninstallIndexDecision,
        quiescer: &dyn ComponentQuiescer,
    ) -> Result<UninstallReceipt, UninstallError> {
        self.store.with_exclusive_lock(|| {
            self.ensure_no_pending_migration()?;
            quiescer.quiesce()?;
            self.uninstall_locked(index_decision)
        })
    }

    /// Removes installed components while holding an existing indexing pause.
    ///
    /// # Errors
    ///
    /// Returns a typed state or filesystem failure.
    pub fn uninstall_while_paused(
        &self,
        index_decision: UninstallIndexDecision,
        pause: Box<dyn crate::IndexingPauseGuard>,
        quiescer: &dyn ComponentQuiescer,
    ) -> Result<UninstallReceipt, UninstallError> {
        self.store.with_exclusive_lock(|| {
            self.ensure_no_pending_migration()?;
            quiescer.quiesce()?;
            drop(pause);
            self.uninstall_locked(index_decision)
        })
    }

    fn uninstall_locked(
        &self,
        index_decision: UninstallIndexDecision,
    ) -> Result<UninstallReceipt, UninstallError> {
        let state = self.store.load_or_default_unlocked(&self.app_data)?;
        if state.pending_model_migration().is_some() {
            return Err(UninstallError::MigrationInProgress);
        }
        let root = state.data_root().clone();
        root.initialize()?;
        let transaction_id = Uuid::new_v4();
        let tombstone = uninstall_tombstone(&root, transaction_id);
        let categories = uninstall_categories(index_decision == UninstallIndexDecision::Delete);
        let mut target_state = state.clone();
        let removed_component_count =
            target_state.apply_uninstall(index_decision == UninstallIndexDecision::Delete);
        let mut journal = UninstallJournal {
            format_version: UNINSTALL_JOURNAL_FORMAT_VERSION,
            transaction_id,
            delete_indexes: index_decision == UninstallIndexDecision::Delete,
            original_state: state,
            target_state: target_state.clone(),
            staged_categories: Vec::new(),
        };
        write_uninstall_journal(&self.store, &journal)?;
        fs::create_dir(&tombstone)?;
        for category in categories {
            journal.staged_categories.push(category);
            if let Err(error) = write_uninstall_journal(&self.store, &journal) {
                let _ = recover_interrupted_uninstall(&self.store, &self.app_data);
                return Err(error.into());
            }
            let source = root.category_path(category);
            let destination = tombstone.join(category.directory_name());
            if let Err(error) = fs::rename(&source, &destination) {
                let _ = recover_interrupted_uninstall(&self.store, &self.app_data);
                return Err(error.into());
            }
            self.store.sync_directory(root.path())?;
            self.store.sync_directory(&tombstone)?;
        }

        for category in &journal.staged_categories {
            if let Err(error) = fs::create_dir(root.category_path(*category)) {
                let _ = recover_interrupted_uninstall(&self.store, &self.app_data);
                return Err(error.into());
            }
        }
        if let Err(error) = self.store.save_unlocked(&target_state) {
            let _ = recover_interrupted_uninstall(&self.store, &self.app_data);
            return Err(error.into());
        }
        finish_uninstall(&self.store, &journal)?;
        Ok(UninstallReceipt {
            index_decision,
            removed_component_count,
        })
    }

    fn ensure_no_pending_migration(&self) -> Result<(), UninstallError> {
        if self
            .store
            .load_or_default_unlocked(&self.app_data)?
            .pending_model_migration()
            .is_some()
        {
            return Err(UninstallError::MigrationInProgress);
        }
        Ok(())
    }
}

pub(crate) fn recover_interrupted_uninstall(
    store: &SemanticStateStore,
    app_data: &Path,
) -> Result<(), SemanticStateError> {
    let Some(journal) = read_uninstall_journal(store)? else {
        return Ok(());
    };
    validate_uninstall_journal(&journal)?;
    let current = store.read_state_unlocked(app_data)?;
    if current == journal.target_state {
        finish_uninstall(store, &journal)?;
        return Ok(());
    }
    if current != journal.original_state {
        return Err(SemanticStateError::InvalidUninstallJournal);
    }
    rollback_uninstall(store, &journal)
}

fn validate_uninstall_journal(journal: &UninstallJournal) -> Result<(), SemanticStateError> {
    journal.original_state.validate()?;
    journal.target_state.validate()?;
    if journal.format_version != UNINSTALL_JOURNAL_FORMAT_VERSION
        || journal.original_state.data_root() != journal.target_state.data_root()
    {
        return Err(SemanticStateError::InvalidUninstallJournal);
    }
    let mut expected = journal.original_state.clone();
    expected.apply_uninstall(journal.delete_indexes);
    if expected != journal.target_state {
        return Err(SemanticStateError::InvalidUninstallJournal);
    }
    let allowed: Vec<_> = uninstall_categories(journal.delete_indexes);
    let mut seen = std::collections::BTreeSet::new();
    if journal
        .staged_categories
        .iter()
        .any(|category| !allowed.contains(category) || !seen.insert(*category))
    {
        return Err(SemanticStateError::InvalidUninstallJournal);
    }
    Ok(())
}

fn uninstall_categories(delete_indexes: bool) -> Vec<DataCategory> {
    let mut categories = vec![DataCategory::Models, DataCategory::Workers];
    if delete_indexes {
        categories.extend([
            DataCategory::Extracted,
            DataCategory::Zvec,
            DataCategory::EmbeddingCache,
        ]);
    }
    categories
}

fn uninstall_tombstone(root: &SemanticDataRoot, transaction_id: Uuid) -> PathBuf {
    root.category_path(DataCategory::Catalog)
        .join(format!(".uninstall-{transaction_id}"))
}

fn journal_path(store: &SemanticStateStore) -> PathBuf {
    store.directory().join(UNINSTALL_JOURNAL_FILE_NAME)
}

fn read_uninstall_journal(
    store: &SemanticStateStore,
) -> Result<Option<UninstallJournal>, SemanticStateError> {
    let path = journal_path(store);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(SemanticStateError::UnsafeStatePath { path })
        }
        Ok(_) => serde_json::from_slice(&read_file_no_follow(&path)?)
            .map(Some)
            .map_err(Into::into),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn write_uninstall_journal(
    store: &SemanticStateStore,
    journal: &UninstallJournal,
) -> Result<(), SemanticStateError> {
    let path = journal_path(store);
    let temporary = store.directory().join(format!(
        ".{UNINSTALL_JOURNAL_FILE_NAME}.{}.tmp",
        Uuid::new_v4()
    ));
    let bytes = serde_json::to_vec_pretty(journal)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        store.sync_directory(store.directory())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(Into::into)
}

fn finish_uninstall(
    store: &SemanticStateStore,
    journal: &UninstallJournal,
) -> Result<(), SemanticStateError> {
    let root = journal.target_state.data_root();
    root.initialize()?;
    let tombstone = uninstall_tombstone(root, journal.transaction_id);
    match fs::symlink_metadata(&tombstone) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(SemanticStateError::UnsafeStatePath { path: tombstone });
        }
        Ok(_) => {
            ensure_tree_has_no_symlinks(&tombstone)?;
            fs::remove_dir_all(&tombstone)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    remove_uninstall_journal(store)
}

fn rollback_uninstall(
    store: &SemanticStateStore,
    journal: &UninstallJournal,
) -> Result<(), SemanticStateError> {
    let root = journal.original_state.data_root();
    let tombstone = uninstall_tombstone(root, journal.transaction_id);
    for category in journal.staged_categories.iter().rev() {
        let source = tombstone.join(category.directory_name());
        let destination = root.category_path(*category);
        match fs::symlink_metadata(&source) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(SemanticStateError::UnsafeStatePath { path: source });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                reject_state_symlink_or_non_directory(&destination)?;
                continue;
            }
            Err(error) => return Err(error.into()),
        }
        match fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(SemanticStateError::UnsafeStatePath { path: destination });
            }
            Ok(_) => {
                if fs::read_dir(&destination)?.next().transpose()?.is_some() {
                    return Err(SemanticStateError::InvalidUninstallJournal);
                }
                fs::remove_dir(&destination)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        fs::rename(source, destination)?;
    }
    match fs::symlink_metadata(&tombstone) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(SemanticStateError::UnsafeStatePath { path: tombstone });
        }
        Ok(_) => fs::remove_dir(&tombstone)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    remove_uninstall_journal(store)
}

fn remove_uninstall_journal(store: &SemanticStateStore) -> Result<(), SemanticStateError> {
    let path = journal_path(store);
    match fs::remove_file(path) {
        Ok(()) => store.sync_directory(store.directory()).map_err(Into::into),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn reject_state_symlink_or_non_directory(path: &Path) -> Result<(), SemanticStateError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SemanticStateError::UnsafeStatePath {
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn ensure_tree_has_no_symlinks(path: &Path) -> Result<(), SemanticStateError> {
    reject_state_symlink_or_non_directory(path)?;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)?;
        if metadata.file_type().is_symlink() {
            return Err(SemanticStateError::UnsafeStatePath { path: child });
        }
        if metadata.is_dir() {
            ensure_tree_has_no_symlinks(&child)?;
        } else if !metadata.is_file() {
            return Err(SemanticStateError::UnsafeStatePath { path: child });
        }
    }
    Ok(())
}

struct PreparedArtifact {
    artifact: CatalogArtifact,
    partial_path: PathBuf,
    ready_directory: PathBuf,
    final_directory: PathBuf,
}

struct CommittedArtifact {
    artifact: CatalogArtifact,
    partial_path: PathBuf,
    final_directory: PathBuf,
    installed_path: PathBuf,
}

fn prepare_artifact(
    root: &SemanticDataRoot,
    artifact: &CatalogArtifact,
    source: &dyn ArtifactSource,
    activation: &dyn ActivationProbe,
) -> Result<PreparedArtifact, InstallError> {
    let downloads = root.category_path(DataCategory::Catalog).join("downloads");
    ensure_confined_directory(root, &downloads)?;
    let partial_path = downloads.join(format!("{}.partial", artifact.id().as_str()));
    download_artifact(artifact, &partial_path, source)?;

    let final_directory = final_artifact_directory(root, artifact);
    let ready_directory = staging_artifact_directory(root, artifact);
    ensure_confined_directory(root, &ready_directory)?;
    let installed_path = ready_directory.join("payload");
    fs::rename(&partial_path, &installed_path)?;
    make_worker_executable(artifact, &installed_path)?;
    if let Err(source_error) = activation.validate(artifact, &installed_path) {
        let _ = fs::rename(&installed_path, &partial_path);
        let _ = fs::remove_dir_all(&ready_directory);
        return Err(InstallError::ActivationFailed {
            artifact: artifact.id().clone(),
            source: source_error,
        });
    }

    Ok(PreparedArtifact {
        artifact: artifact.clone(),
        partial_path,
        ready_directory,
        final_directory,
    })
}

fn make_worker_executable(
    artifact: &CatalogArtifact,
    installed_path: &Path,
) -> std::io::Result<()> {
    #[cfg(unix)]
    if matches!(artifact.kind(), ArtifactKind::Worker) {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(installed_path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = (artifact, installed_path);
    Ok(())
}

fn recover_interrupted_commit(
    root: &SemanticDataRoot,
    artifact: &CatalogArtifact,
) -> Result<(), InstallError> {
    ensure_artifact_paths_safe(root, artifact)?;
    let final_directory = final_artifact_directory(root, artifact);
    let staging_directory = staging_artifact_directory(root, artifact);
    let downloads = root.category_path(DataCategory::Catalog).join("downloads");
    ensure_confined_directory(root, &downloads)?;
    let partial_path = downloads.join(format!("{}.partial", artifact.id().as_str()));
    for directory in [&final_directory, &staging_directory] {
        match fs::symlink_metadata(directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(InstallError::UnsafePath {
                    path: directory.clone(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        }
        let payload = directory.join("payload");
        if verify_artifact_file(artifact, &payload).is_ok() {
            if partial_path.exists() {
                fs::remove_file(&partial_path)?;
            }
            fs::rename(&payload, &partial_path)?;
        }
        fs::remove_dir_all(directory)?;
    }
    Ok(())
}

fn verify_installed_artifact(artifact: &CatalogArtifact, path: &Path) -> Result<(), InstallError> {
    verify_artifact_file(artifact, path).map_err(|()| InstallError::InstalledArtifactInvalid {
        artifact: artifact.id().clone(),
    })
}

fn verify_artifact_file(artifact: &CatalogArtifact, path: &Path) -> Result<(), ()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ())?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != artifact.resources().download_bytes()
    {
        return Err(());
    }
    if checksum_file(path).map_err(|_| ())? != artifact.checksum() {
        return Err(());
    }
    Ok(())
}

fn artifact_data_category(artifact: &CatalogArtifact) -> DataCategory {
    match artifact.kind() {
        ArtifactKind::Model(_) => DataCategory::Models,
        ArtifactKind::Worker | ArtifactKind::Runtime => DataCategory::Workers,
    }
}

fn final_artifact_directory(root: &SemanticDataRoot, artifact: &CatalogArtifact) -> PathBuf {
    root.category_path(artifact_data_category(artifact))
        .join(artifact.component_id().as_str())
        .join(artifact.version().to_string())
        .join(artifact.id().as_str())
}

fn staging_artifact_directory(root: &SemanticDataRoot, artifact: &CatalogArtifact) -> PathBuf {
    root.category_path(artifact_data_category(artifact))
        .join(".staging")
        .join(artifact.id().as_str())
}

fn retained_partial_bytes(
    root: &SemanticDataRoot,
    artifact: &CatalogArtifact,
) -> Result<u64, InstallError> {
    let partial_path = root
        .category_path(DataCategory::Catalog)
        .join("downloads")
        .join(format!("{}.partial", artifact.id().as_str()));
    match fs::symlink_metadata(&partial_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(InstallError::UnsafePath { path: partial_path })
        }
        Ok(metadata) if metadata.len() <= artifact.resources().download_bytes() => {
            Ok(metadata.len())
        }
        Ok(_) => {
            fs::remove_file(partial_path)?;
            Ok(0)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

fn ensure_artifact_paths_safe(
    root: &SemanticDataRoot,
    artifact: &CatalogArtifact,
) -> Result<(), InstallError> {
    let final_directory = final_artifact_directory(root, artifact);
    let final_parent = final_directory
        .parent()
        .ok_or_else(|| InstallError::UnsafePath {
            path: final_directory.clone(),
        })?;
    ensure_confined_directory(root, final_parent)?;
    ensure_confined_directory(
        root,
        &root
            .category_path(artifact_data_category(artifact))
            .join(".staging"),
    )?;
    ensure_confined_directory(
        root,
        &root.category_path(DataCategory::Catalog).join("downloads"),
    )?;
    reject_symlink_or_unexpected_type_if_present(&final_directory, true)?;
    reject_symlink_or_unexpected_type_if_present(&staging_artifact_directory(root, artifact), true)
}

fn ensure_confined_directory(
    root: &SemanticDataRoot,
    directory: &Path,
) -> Result<(), InstallError> {
    let canonical_root = fs::canonicalize(root.path())?;
    let relative = directory
        .strip_prefix(root.path())
        .map_err(|_| InstallError::UnsafePath {
            path: directory.to_owned(),
        })?;
    let mut current = root.path().to_owned();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(InstallError::UnsafePath {
                path: directory.to_owned(),
            });
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(InstallError::UnsafePath {
                    path: current.clone(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)?;
            }
            Err(error) => return Err(error.into()),
        }
        if !fs::canonicalize(&current)?.starts_with(&canonical_root) {
            return Err(InstallError::UnsafePath {
                path: current.clone(),
            });
        }
    }
    Ok(())
}

fn reject_symlink_or_unexpected_type_if_present(
    path: &Path,
    directory: bool,
) -> Result<(), InstallError> {
    match fs::symlink_metadata(path) {
        Ok(_) => reject_symlink_or_unexpected_type(path, directory),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn reject_symlink_or_unexpected_type(path: &Path, directory: bool) -> Result<(), InstallError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(InstallError::UnsafePath {
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn download_artifact(
    artifact: &CatalogArtifact,
    partial_path: &Path,
    source: &dyn ArtifactSource,
) -> Result<(), InstallError> {
    let expected_bytes = artifact.resources().download_bytes();
    let mut offset = match fs::symlink_metadata(partial_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(InstallError::UnsafePath {
                path: partial_path.to_owned(),
            });
        }
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error.into()),
    };
    if offset > expected_bytes {
        fs::remove_file(partial_path)?;
        offset = 0;
    }
    if offset < expected_bytes {
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut partial = options.open(partial_path)?;
        loop {
            let request = ArtifactRequest {
                artifact_id: artifact.id().clone(),
                offset,
            };
            let chunk =
                source
                    .read(&request)
                    .map_err(|source| InstallError::DownloadInterrupted {
                        artifact: artifact.id().clone(),
                        source,
                    })?;
            if chunk.bytes.is_empty() && !chunk.complete {
                return Err(InstallError::EmptyDownloadChunk {
                    artifact: artifact.id().clone(),
                });
            }
            partial.write_all(&chunk.bytes)?;
            partial.sync_data()?;
            offset = offset
                .checked_add(u64::try_from(chunk.bytes.len()).unwrap_or(u64::MAX))
                .ok_or(InstallError::SizeOverflow)?;
            if offset > expected_bytes {
                drop(partial);
                fs::remove_file(partial_path)?;
                return Err(InstallError::ArtifactSizeMismatch {
                    artifact: artifact.id().clone(),
                    expected_bytes,
                    actual_bytes: offset,
                });
            }
            if chunk.complete {
                break;
            }
        }
        partial.sync_all()?;
    }
    if offset != expected_bytes {
        return Err(InstallError::ArtifactSizeMismatch {
            artifact: artifact.id().clone(),
            expected_bytes,
            actual_bytes: offset,
        });
    }
    let actual_checksum = checksum_file(partial_path)?;
    if actual_checksum != artifact.checksum() {
        fs::remove_file(partial_path)?;
        return Err(InstallError::ChecksumMismatch {
            artifact: artifact.id().clone(),
        });
    }
    Ok(())
}

fn checksum_file(path: &Path) -> Result<Sha256Digest, std::io::Error> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(Sha256Digest::from_bytes(digest.finalize().into()))
}

fn restore_prepared(prepared: &[PreparedArtifact]) {
    for item in prepared {
        let payload = item.ready_directory.join("payload");
        if payload.exists() {
            let _ = fs::rename(payload, &item.partial_path);
        }
        let _ = fs::remove_dir_all(&item.ready_directory);
    }
}

fn rollback_committed(committed: &[CommittedArtifact]) {
    for item in committed {
        if item.installed_path.exists() {
            if let Some(parent) = item.partial_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::rename(&item.installed_path, &item.partial_path);
        }
        let _ = fs::remove_dir_all(&item.final_directory);
    }
}

fn collect_superseded_component_versions(
    store: &SemanticStateStore,
    state: &SemanticState,
) -> Vec<ComponentCleanupIssue> {
    let protected: BTreeSet<PathBuf> = state
        .installed_components()
        .iter()
        .chain(state.rollback_workers())
        .chain(state.retained_models())
        .filter_map(|component| component.installed_path().parent().map(Path::to_owned))
        .collect();
    let mut issues = Vec::new();
    for category in [DataCategory::Models, DataCategory::Workers] {
        let category_root = state.data_root().category_path(category);
        let component_directories = safe_child_directories(&category_root, &mut issues);
        for component_directory in component_directories {
            if component_directory
                .file_name()
                .is_some_and(|name| name == ".staging")
            {
                continue;
            }
            for version_directory in safe_child_directories(&component_directory, &mut issues) {
                for artifact_directory in safe_child_directories(&version_directory, &mut issues) {
                    if protected.contains(&artifact_directory) {
                        continue;
                    }
                    let payload = artifact_directory.join("payload");
                    let payload_is_regular = fs::symlink_metadata(&payload).is_ok_and(|metadata| {
                        metadata.is_file() && !metadata.file_type().is_symlink()
                    });
                    if !payload_is_regular {
                        continue;
                    }
                    if let Err(error) = ensure_tree_has_no_symlinks(&artifact_directory)
                        .map_err(|error| error.to_string())
                        .and_then(|()| {
                            fs::remove_dir_all(&artifact_directory)
                                .map_err(|error| error.to_string())
                        })
                        .and_then(|()| {
                            store
                                .sync_directory(&version_directory)
                                .map_err(|error| error.to_string())
                        })
                    {
                        issues.push(ComponentCleanupIssue {
                            path: artifact_directory,
                            message: error,
                        });
                    }
                }
                remove_empty_managed_directory(&version_directory, store, &mut issues);
            }
            remove_empty_managed_directory(&component_directory, store, &mut issues);
        }
    }
    issues
}

fn safe_child_directories(
    directory: &Path,
    issues: &mut Vec<ComponentCleanupIssue>,
) -> Vec<PathBuf> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            issues.push(ComponentCleanupIssue {
                path: directory.to_owned(),
                message: error.to_string(),
            });
            return Vec::new();
        }
    };
    let mut directories = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                issues.push(ComponentCleanupIssue {
                    path: directory.to_owned(),
                    message: error.to_string(),
                });
                continue;
            }
        };
        let path = entry.path();
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                directories.push(path);
            }
            Ok(_) => {}
            Err(error) => issues.push(ComponentCleanupIssue {
                path,
                message: error.to_string(),
            }),
        }
    }
    directories.sort();
    directories
}

fn remove_empty_managed_directory(
    directory: &Path,
    store: &SemanticStateStore,
    issues: &mut Vec<ComponentCleanupIssue>,
) {
    let is_empty = match fs::read_dir(directory) {
        Ok(mut entries) => entries.next().is_none(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            issues.push(ComponentCleanupIssue {
                path: directory.to_owned(),
                message: error.to_string(),
            });
            return;
        }
    };
    if !is_empty {
        return;
    }
    let Some(parent) = directory.parent() else {
        return;
    };
    if let Err(error) = fs::remove_dir(directory).and_then(|()| store.sync_directory(parent)) {
        issues.push(ComponentCleanupIssue {
            path: directory.to_owned(),
            message: error.to_string(),
        });
    }
}

/// Component installation failure.
#[derive(Debug, Error)]
pub enum InstallError {
    /// An installation path escaped the root or traversed a symlink/non-directory.
    #[error("component installation encountered an unsafe path: {}", path.display())]
    UnsafePath {
        /// Rejected path.
        path: PathBuf,
    },
    /// The catalog changed after the user reviewed the offer.
    #[error("the signed catalog changed after installation consent")]
    CatalogChanged,
    /// The consented location no longer matches durable state.
    #[error("semantic data root changed after installation consent")]
    DataRootChanged,
    /// The selected update's component is no longer installed.
    #[error("component `{}` is not installed", component.as_str())]
    ComponentNotInstalled {
        /// Missing logical component.
        component: ComponentId,
    },
    /// Installed worker state changed after patch selection.
    #[error("worker update for `{}` is stale", component.as_str())]
    StaleWorkerUpdate {
        /// Logical worker component.
        component: ComponentId,
    },
    /// Automatic worker updates require an established semantic profile.
    #[error("automatic worker update requires an active model")]
    MissingActiveModel,
    /// Durable state referred to a missing, truncated, or tampered payload.
    #[error("installed artifact `{}` is missing or invalid", artifact.as_str())]
    InstalledArtifactInvalid {
        /// Invalid installed artifact.
        artifact: ArtifactId,
    },
    /// A size sum overflowed.
    #[error("component size estimate overflowed")]
    SizeOverflow,
    /// Available space cannot satisfy installation estimates plus reserve.
    #[error(
        "insufficient free space: {available_bytes} available, {required_bytes} required including {reserve_bytes} reserve"
    )]
    InsufficientSpace {
        /// Available bytes.
        available_bytes: u64,
        /// Installed estimate plus reserve.
        required_bytes: u64,
        /// Explicit reserve included in the requirement.
        reserve_bytes: u64,
    },
    /// The source stopped before a complete artifact was available.
    #[error("download of `{}` was interrupted: {source}", artifact.as_str())]
    DownloadInterrupted {
        /// Interrupted artifact.
        artifact: ArtifactId,
        /// Source failure.
        source: ArtifactSourceError,
    },
    /// A source returned no bytes without marking the artifact complete.
    #[error("download of `{}` returned an empty non-final chunk", artifact.as_str())]
    EmptyDownloadChunk {
        /// Invalid artifact response.
        artifact: ArtifactId,
    },
    /// Downloaded bytes did not match the signed size.
    #[error(
        "artifact `{}` has {actual_bytes} bytes; expected {expected_bytes}",
        artifact.as_str()
    )]
    ArtifactSizeMismatch {
        /// Invalid artifact.
        artifact: ArtifactId,
        /// Signed expected length.
        expected_bytes: u64,
        /// Downloaded length.
        actual_bytes: u64,
    },
    /// Downloaded bytes did not match the signed SHA-256 digest.
    #[error("artifact `{}` failed SHA-256 verification", artifact.as_str())]
    ChecksumMismatch {
        /// Tampered artifact.
        artifact: ArtifactId,
    },
    /// A staged artifact failed activation validation.
    #[error("artifact `{}` could not be activated: {source}", artifact.as_str())]
    ActivationFailed {
        /// Rejected artifact.
        artifact: ArtifactId,
        /// Activation diagnostic.
        source: ActivationError,
    },
    /// Signed catalog lookup or validation failed.
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    /// Free-space probing failed.
    #[error(transparent)]
    FreeSpace(#[from] FreeSpaceError),
    /// Semantic data layout creation failed.
    #[error(transparent)]
    DataRoot(#[from] DataRootError),
    /// Durable state failed.
    #[error(transparent)]
    State(#[from] SemanticStateError),
    /// Installation filesystem operation failed.
    #[error("component installation filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Component uninstall failure.
#[derive(Debug, Error)]
pub enum UninstallError {
    /// Components cannot be removed while a full reindex is active.
    #[error("cannot uninstall semantic components while a model migration is active")]
    MigrationInProgress,
    /// Indexing/workers could not be quiesced before component removal.
    #[error(transparent)]
    Quiesce(#[from] QuiesceError),
    /// Semantic data layout operation failed.
    #[error(transparent)]
    DataRoot(#[from] DataRootError),
    /// Durable semantic state failed.
    #[error(transparent)]
    State(#[from] SemanticStateError),
    /// Component removal filesystem operation failed.
    #[error("component uninstall filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}
