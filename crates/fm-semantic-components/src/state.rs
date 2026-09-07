use std::collections::BTreeMap;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use semver::Version;

use crate::{
    ArtifactId, ArtifactKind, ComponentId, DataCategory, LocalModelImport, ModelIdentity,
    SemanticDataRoot, SemanticProfile, Sha256Digest,
};

const STATE_FILE_NAME: &str = "semantic-components.json";
const LOCK_FILE_NAME: &str = ".semantic-components.lock";
const STATE_SCHEMA_VERSION: u32 = 1;

/// The abstract profile and exact immutable model used by an index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedModelSelection {
    profile: SemanticProfile,
    identity: ModelIdentity,
}

/// Opaque identity of one explicit embedding-space migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MigrationId(Uuid);

impl MigrationId {
    /// Creates a unique migration identity.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Reconstructs an identity read from an application boundary.
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    /// Returns the UUID used to persist and correlate this migration.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for MigrationId {
    fn default() -> Self {
        Self::new()
    }
}

/// Estimated work disclosed before a full reindex is confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReindexEstimate {
    documents: u64,
    source_bytes: u64,
}

impl ReindexEstimate {
    /// Creates a reindex estimate.
    #[must_use]
    pub const fn new(documents: u64, source_bytes: u64) -> Self {
        Self {
            documents,
            source_bytes,
        }
    }

    /// Returns the estimated document count.
    #[must_use]
    pub const fn documents(self) -> u64 {
        self.documents
    }

    /// Returns the estimated input byte count.
    #[must_use]
    pub const fn source_bytes(self) -> u64 {
        self.source_bytes
    }
}

/// Confirmation-gated plan for replacing an embedding space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMigrationPlan {
    id: MigrationId,
    from: Option<ResolvedModelSelection>,
    target: ResolvedModelSelection,
    estimate: ReindexEstimate,
    reason: ReindexReason,
}

/// Why a confirmation-gated full reindex is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReindexReason {
    /// The exact model identity or embedding contract changes.
    ModelChanged,
    /// Stored index schema changes even if the model remains the same.
    SchemaChanged {
        /// Existing schema version.
        from_version: u32,
        /// Replacement schema version.
        to_version: u32,
    },
}

impl ModelMigrationPlan {
    /// Returns the opaque migration identity.
    #[must_use]
    pub const fn id(&self) -> MigrationId {
        self.id
    }

    /// Returns the currently active embedding space captured by the plan.
    #[must_use]
    pub const fn from(&self) -> Option<&ResolvedModelSelection> {
        self.from.as_ref()
    }

    /// Returns the requested replacement embedding space.
    #[must_use]
    pub const fn target(&self) -> &ResolvedModelSelection {
        &self.target
    }

    /// Returns the estimated full-reindex work.
    #[must_use]
    pub const fn estimate(&self) -> ReindexEstimate {
        self.estimate
    }

    /// Returns why a full reindex is mandatory.
    #[must_use]
    pub const fn reason(&self) -> ReindexReason {
        self.reason
    }

    /// Reports that activation requires explicit confirmation.
    #[must_use]
    pub const fn requires_confirmation(&self) -> bool {
        true
    }

    /// Reports that every existing embedding must be rebuilt.
    #[must_use]
    pub const fn is_full_reindex(&self) -> bool {
        true
    }

    /// Reports that reindex checkpoints may be resumed.
    #[must_use]
    pub const fn is_resumable(&self) -> bool {
        true
    }

    /// Records explicit confirmation without starting work.
    #[must_use]
    pub fn confirm(self) -> ConfirmedModelMigration {
        ConfirmedModelMigration(self)
    }
}

/// An explicitly confirmed model migration.
#[derive(Debug)]
pub struct ConfirmedModelMigration(ModelMigrationPlan);

/// Persisted resumable progress for a confirmed full reindex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingModelMigration {
    plan: ModelMigrationPlan,
    completed_documents: u64,
    resume_cursor: Option<String>,
}

impl PendingModelMigration {
    /// Returns the confirmed migration plan.
    #[must_use]
    pub const fn plan(&self) -> &ModelMigrationPlan {
        &self.plan
    }

    /// Returns the number of completely reindexed documents.
    #[must_use]
    pub const fn completed_documents(&self) -> u64 {
        self.completed_documents
    }

    /// Returns an opaque durable resume cursor, when available.
    #[must_use]
    pub fn resume_cursor(&self) -> Option<&str> {
        self.resume_cursor.as_deref()
    }
}

impl ResolvedModelSelection {
    /// Returns the persisted abstract profile.
    #[must_use]
    pub const fn profile(&self) -> SemanticProfile {
        self.profile
    }

    /// Returns the exact immutable model revision used by the index.
    #[must_use]
    pub const fn identity(&self) -> &ModelIdentity {
        &self.identity
    }
}

/// Durable semantic component state containing no credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticState {
    schema_version: u32,
    data_root: SemanticDataRoot,
    active_model: Option<ResolvedModelSelection>,
    #[serde(default)]
    active_index_schema_version: Option<u32>,
    #[serde(default)]
    pending_model_migration: Option<PendingModelMigration>,
    #[serde(default)]
    installed_components: Vec<InstalledComponent>,
    #[serde(default)]
    last_working_workers: Vec<InstalledComponent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    retained_models: Vec<InstalledComponent>,
}

impl SemanticState {
    /// Creates state using an injected platform app-data directory.
    #[must_use]
    pub fn from_app_data(app_data: &Path) -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            data_root: SemanticDataRoot::from_app_data(app_data),
            active_model: None,
            active_index_schema_version: None,
            pending_model_migration: None,
            installed_components: Vec::new(),
            last_working_workers: Vec::new(),
            retained_models: Vec::new(),
        }
    }

    /// Returns the configured semantic-data root.
    #[must_use]
    pub const fn data_root(&self) -> &SemanticDataRoot {
        &self.data_root
    }

    /// Returns the embedding space currently used by the index.
    #[must_use]
    pub const fn active_model(&self) -> Option<&ResolvedModelSelection> {
        self.active_model.as_ref()
    }

    /// Returns the durable schema version of the active index.
    #[must_use]
    pub const fn active_index_schema_version(&self) -> Option<u32> {
        self.active_index_schema_version
    }

    /// Activates the first embedding space without allowing replacement.
    ///
    /// # Errors
    ///
    /// Returns [`SemanticStateError::ModelMigrationRequired`] when an embedding
    /// space already exists.
    pub fn activate_initial_model(
        &mut self,
        profile: SemanticProfile,
        identity: ModelIdentity,
        index_schema_version: u32,
    ) -> Result<(), SemanticStateError> {
        if self.active_model.is_some() {
            return Err(SemanticStateError::ModelMigrationRequired);
        }
        if index_schema_version == 0 {
            return Err(SemanticStateError::InvalidIndexSchemaVersion);
        }
        self.active_model = Some(ResolvedModelSelection { profile, identity });
        self.active_index_schema_version = Some(index_schema_version);
        Ok(())
    }

    /// Plans an explicit local-model migration without changing active state.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the target already is active or another
    /// migration is pending.
    pub fn plan_local_model_migration(
        &self,
        import: &LocalModelImport,
        profile: SemanticProfile,
        estimate: ReindexEstimate,
    ) -> Result<ModelMigrationPlan, SemanticStateError> {
        self.plan_model_migration(profile, import.metadata().identity().clone(), estimate)
    }

    /// Plans a curated or local embedding-model change without activating it.
    ///
    /// # Errors
    ///
    /// Returns a typed error when another migration is pending or the exact
    /// profile and model already are active.
    pub fn plan_model_migration(
        &self,
        profile: SemanticProfile,
        identity: ModelIdentity,
        estimate: ReindexEstimate,
    ) -> Result<ModelMigrationPlan, SemanticStateError> {
        self.plan_migration(
            ResolvedModelSelection { profile, identity },
            estimate,
            ReindexReason::ModelChanged,
            true,
        )
    }

    /// Plans a schema-affecting change while retaining the exact model.
    ///
    /// # Errors
    ///
    /// Returns a typed error for missing active state, invalid versions, or an
    /// already pending migration.
    pub fn plan_schema_migration(
        &self,
        from_version: u32,
        to_version: u32,
        estimate: ReindexEstimate,
    ) -> Result<ModelMigrationPlan, SemanticStateError> {
        if from_version == 0 || to_version == 0 || from_version == to_version {
            return Err(SemanticStateError::InvalidSchemaMigration {
                from_version,
                to_version,
            });
        }
        if self.active_index_schema_version != Some(from_version) {
            return Err(SemanticStateError::InvalidSchemaMigration {
                from_version,
                to_version,
            });
        }
        let target = self
            .active_model
            .clone()
            .ok_or(SemanticStateError::MissingActiveModel)?;
        self.plan_migration(
            target,
            estimate,
            ReindexReason::SchemaChanged {
                from_version,
                to_version,
            },
            false,
        )
    }

    fn plan_migration(
        &self,
        target: ResolvedModelSelection,
        estimate: ReindexEstimate,
        reason: ReindexReason,
        require_distinct_target: bool,
    ) -> Result<ModelMigrationPlan, SemanticStateError> {
        if self.pending_model_migration.is_some() {
            return Err(SemanticStateError::MigrationAlreadyPending);
        }
        if require_distinct_target && self.active_model.as_ref() == Some(&target) {
            return Err(SemanticStateError::MigrationNotDistinct);
        }
        Ok(ModelMigrationPlan {
            id: MigrationId::new(),
            from: self.active_model.clone(),
            target,
            estimate,
            reason,
        })
    }

    /// Starts a previously confirmed migration while retaining the active model.
    ///
    /// # Errors
    ///
    /// Returns a typed error for stale plans or an existing pending migration.
    pub fn begin_model_migration(
        &mut self,
        confirmed: ConfirmedModelMigration,
    ) -> Result<(), SemanticStateError> {
        if self.pending_model_migration.is_some() {
            return Err(SemanticStateError::MigrationAlreadyPending);
        }
        if confirmed.0.from != self.active_model {
            return Err(SemanticStateError::StaleMigration);
        }
        self.pending_model_migration = Some(PendingModelMigration {
            plan: confirmed.0,
            completed_documents: 0,
            resume_cursor: None,
        });
        Ok(())
    }

    /// Returns the durable in-progress model migration.
    #[must_use]
    pub const fn pending_model_migration(&self) -> Option<&PendingModelMigration> {
        self.pending_model_migration.as_ref()
    }

    /// Advances a resumable full-reindex checkpoint.
    ///
    /// # Errors
    ///
    /// Returns a typed error when no migration exists or progress regresses or
    /// exceeds the estimate.
    pub fn checkpoint_model_migration(
        &mut self,
        completed_documents: u64,
        resume_cursor: Option<String>,
    ) -> Result<(), SemanticStateError> {
        let pending = self
            .pending_model_migration
            .as_mut()
            .ok_or(SemanticStateError::NoPendingMigration)?;
        if completed_documents < pending.completed_documents
            || completed_documents > pending.plan.estimate.documents
        {
            return Err(SemanticStateError::InvalidMigrationProgress {
                completed: completed_documents,
                total: pending.plan.estimate.documents,
            });
        }
        pending.completed_documents = completed_documents;
        pending.resume_cursor = resume_cursor;
        Ok(())
    }

    /// Activates the target only after the full reindex reaches its estimate.
    ///
    /// # Errors
    ///
    /// Returns a typed error when no migration exists or work is incomplete.
    pub fn complete_model_migration(&mut self) -> Result<(), SemanticStateError> {
        let pending = self
            .pending_model_migration
            .as_ref()
            .ok_or(SemanticStateError::NoPendingMigration)?;
        if pending.completed_documents < pending.plan.estimate.documents {
            return Err(SemanticStateError::MigrationIncomplete {
                completed: pending.completed_documents,
                total: pending.plan.estimate.documents,
            });
        }
        let pending = self
            .pending_model_migration
            .take()
            .ok_or(SemanticStateError::NoPendingMigration)?;
        if let ReindexReason::SchemaChanged { to_version, .. } = pending.plan.reason {
            self.active_index_schema_version = Some(to_version);
        }
        self.active_model = Some(pending.plan.target);
        Ok(())
    }

    /// Returns installed component records.
    #[must_use]
    pub fn installed_components(&self) -> &[InstalledComponent] {
        &self.installed_components
    }

    /// Returns the active installation of a logical component.
    #[must_use]
    pub fn installed_component(&self, component_id: &ComponentId) -> Option<&InstalledComponent> {
        self.installed_components
            .iter()
            .find(|component| component.component_id() == component_id)
    }

    /// Returns the retained previous worker for rollback.
    #[must_use]
    pub fn last_working_worker(&self, component_id: &ComponentId) -> Option<&InstalledComponent> {
        self.last_working_workers
            .iter()
            .find(|component| component.component_id() == component_id)
    }

    pub(crate) fn rollback_workers(&self) -> &[InstalledComponent] {
        &self.last_working_workers
    }

    /// Returns the installed artifact matching an exact model identity,
    /// including a model retained while its replacement is staged.
    #[must_use]
    pub fn installed_model(&self, identity: &ModelIdentity) -> Option<&InstalledComponent> {
        self.installed_components
            .iter()
            .chain(&self.retained_models)
            .find(|component| {
                matches!(component.kind(), ArtifactKind::Model(model) if model == identity)
            })
    }

    pub(crate) fn retained_models(&self) -> &[InstalledComponent] {
        &self.retained_models
    }

    pub(crate) fn retained_component(
        &self,
        artifact_id: &ArtifactId,
    ) -> Option<&InstalledComponent> {
        self.installed_components
            .iter()
            .chain(&self.last_working_workers)
            .chain(&self.retained_models)
            .find(|component| component.artifact_id() == artifact_id)
    }

    pub(crate) fn ensure_model_can_activate(
        &self,
        profile: SemanticProfile,
        identity: &ModelIdentity,
    ) -> Result<(), SemanticStateError> {
        if self
            .active_model
            .as_ref()
            .is_some_and(|active| active.profile != profile || active.identity != *identity)
        {
            return Err(SemanticStateError::ModelMigrationRequired);
        }
        Ok(())
    }

    pub(crate) fn ensure_index_schema_can_activate(
        &self,
        offered_version: u32,
    ) -> Result<(), SemanticStateError> {
        if let Some(active_version) = self.active_index_schema_version
            && active_version != offered_version
        {
            return Err(SemanticStateError::IndexSchemaMigrationRequired {
                active_version,
                offered_version,
            });
        }
        Ok(())
    }

    pub(crate) fn activate_installed_model(
        &mut self,
        profile: SemanticProfile,
        identity: ModelIdentity,
        index_schema_version: u32,
    ) -> Result<(), SemanticStateError> {
        self.ensure_model_can_activate(profile, &identity)?;
        self.ensure_index_schema_can_activate(index_schema_version)?;
        if self.active_model.is_none() {
            self.active_model = Some(ResolvedModelSelection { profile, identity });
            self.active_index_schema_version = Some(index_schema_version);
        }
        Ok(())
    }

    pub(crate) fn replace_data_root(
        &mut self,
        data_root: SemanticDataRoot,
    ) -> Result<(), SemanticStateError> {
        let old_root = self.data_root.path();
        let mut rebased = Vec::with_capacity(
            self.installed_components.len()
                + self.last_working_workers.len()
                + self.retained_models.len(),
        );
        for component in self
            .installed_components
            .iter()
            .chain(&self.last_working_workers)
            .chain(&self.retained_models)
        {
            let relative = component
                .installed_path
                .strip_prefix(old_root)
                .map_err(|_| SemanticStateError::ComponentOutsideDataRoot {
                    path: component.installed_path.clone(),
                })?;
            rebased.push(data_root.path().join(relative));
        }
        for (component, path) in self
            .installed_components
            .iter_mut()
            .chain(&mut self.last_working_workers)
            .chain(&mut self.retained_models)
            .zip(rebased)
        {
            component.installed_path = path;
        }
        self.data_root = data_root;
        Ok(())
    }

    pub(crate) fn record_installed(
        &mut self,
        artifact_id: ArtifactId,
        component_id: ComponentId,
        kind: ArtifactKind,
        version: Version,
        checksum: Sha256Digest,
        installed_path: PathBuf,
    ) {
        let installed = InstalledComponent {
            artifact_id,
            component_id: component_id.clone(),
            kind: kind.clone(),
            version,
            checksum,
            installed_path,
        };
        if let Some(index) = self
            .installed_components
            .iter()
            .position(|component| component.component_id == component_id)
        {
            let previous = std::mem::replace(&mut self.installed_components[index], installed);
            if matches!(kind, ArtifactKind::Worker) {
                if let Some(existing) = self
                    .last_working_workers
                    .iter_mut()
                    .find(|component| component.component_id == component_id)
                {
                    *existing = previous;
                } else {
                    self.last_working_workers.push(previous);
                }
            } else if matches!(kind, ArtifactKind::Model(_)) {
                if let Some(existing) = self
                    .retained_models
                    .iter_mut()
                    .find(|component| component.component_id == component_id)
                {
                    *existing = previous;
                } else {
                    self.retained_models.push(previous);
                }
            }
        } else {
            self.installed_components.push(installed);
        }
    }

    pub(crate) fn apply_uninstall(&mut self, delete_indexes: bool) -> u64 {
        let removed = u64::try_from(
            self.installed_components.len()
                + self.last_working_workers.len()
                + self.retained_models.len(),
        )
        .unwrap_or(u64::MAX);
        self.installed_components.clear();
        self.last_working_workers.clear();
        self.retained_models.clear();
        self.pending_model_migration = None;
        if delete_indexes {
            self.active_model = None;
            self.active_index_schema_version = None;
        }
        removed
    }

    pub(crate) fn validate(&self) -> Result<(), SemanticStateError> {
        if self.schema_version != STATE_SCHEMA_VERSION {
            return Err(SemanticStateError::UnsupportedSchema {
                version: self.schema_version,
            });
        }
        if self.active_index_schema_version == Some(0)
            || (self.active_model.is_none() && self.active_index_schema_version.is_some())
        {
            return Err(SemanticStateError::InvalidPersistedState);
        }
        if self.data_root.path().as_os_str().is_empty()
            || self
                .data_root
                .path()
                .as_os_str()
                .to_string_lossy()
                .contains('\0')
            || self.data_root.path().components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            })
        {
            return Err(SemanticStateError::InvalidPersistedState);
        }
        crate::data_root::ensure_no_symlink_components(self.data_root.path())?;
        if let Some(pending) = &self.pending_model_migration {
            if pending.plan.from != self.active_model
                || pending.completed_documents > pending.plan.estimate.documents
            {
                return Err(SemanticStateError::InvalidPersistedState);
            }
            match pending.plan.reason {
                ReindexReason::ModelChanged
                    if self.active_model.as_ref() == Some(&pending.plan.target) =>
                {
                    return Err(SemanticStateError::InvalidPersistedState);
                }
                ReindexReason::SchemaChanged {
                    from_version,
                    to_version,
                } if from_version == 0
                    || to_version == 0
                    || from_version == to_version
                    || self.active_index_schema_version != Some(from_version)
                    || self.active_model.as_ref() != Some(&pending.plan.target) =>
                {
                    return Err(SemanticStateError::InvalidPersistedState);
                }
                _ => {}
            }
        }
        let mut active_components = BTreeMap::new();
        for component in &self.installed_components {
            if active_components
                .insert(component.component_id(), component.artifact_id())
                .is_some()
            {
                return Err(SemanticStateError::InvalidPersistedState);
            }
            let category = match component.kind() {
                ArtifactKind::Model(_) => DataCategory::Models,
                ArtifactKind::Worker | ArtifactKind::Runtime => DataCategory::Workers,
            };
            validate_component_path(
                &self.data_root.category_path(category),
                &component.installed_path,
            )?;
        }
        let mut rollback_components = BTreeMap::new();
        for component in &self.last_working_workers {
            if !matches!(component.kind, ArtifactKind::Worker)
                || rollback_components
                    .insert(component.component_id(), component.artifact_id())
                    .is_some()
            {
                return Err(SemanticStateError::InvalidPersistedState);
            }
            validate_component_path(
                &self.data_root.category_path(DataCategory::Workers),
                &component.installed_path,
            )?;
        }
        let mut retained_models = BTreeMap::new();
        for component in &self.retained_models {
            if !matches!(component.kind, ArtifactKind::Model(_))
                || retained_models
                    .insert(component.component_id(), component.artifact_id())
                    .is_some()
            {
                return Err(SemanticStateError::InvalidPersistedState);
            }
            validate_component_path(
                &self.data_root.category_path(DataCategory::Models),
                &component.installed_path,
            )?;
        }
        Ok(())
    }

    fn validate_referenced_payloads(&self) -> Result<(), SemanticStateError> {
        for component in self
            .installed_components
            .iter()
            .chain(&self.last_working_workers)
            .chain(&self.retained_models)
        {
            match fs::symlink_metadata(component.installed_path()) {
                Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                    return Err(SemanticStateError::ReferencedPayloadUnavailable {
                        artifact: component.artifact_id().clone(),
                        path: component.installed_path().to_owned(),
                    });
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err(SemanticStateError::ReferencedPayloadUnavailable {
                        artifact: component.artifact_id().clone(),
                        path: component.installed_path().to_owned(),
                    });
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

fn validate_component_path(root: &Path, path: &Path) -> Result<(), SemanticStateError> {
    let relative =
        path.strip_prefix(root)
            .map_err(|_| SemanticStateError::ComponentOutsideDataRoot {
                path: path.to_owned(),
            })?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(SemanticStateError::ComponentOutsideDataRoot {
            path: path.to_owned(),
        });
    }
    let root_metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(metadata) = root_metadata {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(SemanticStateError::UnsafeStatePath {
                path: root.to_owned(),
            });
        }
        let canonical_root = fs::canonicalize(root)?;
        let mut current = root.to_owned();
        for component in relative.components() {
            let std::path::Component::Normal(name) = component else {
                return Err(SemanticStateError::ComponentOutsideDataRoot {
                    path: path.to_owned(),
                });
            };
            current.push(name);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(SemanticStateError::UnsafeStatePath { path: current });
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(error.into()),
            }
        }
        if path.exists() && !fs::canonicalize(path)?.starts_with(canonical_root) {
            return Err(SemanticStateError::ComponentOutsideDataRoot {
                path: path.to_owned(),
            });
        }
    }
    Ok(())
}

/// One installed and verified semantic component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledComponent {
    artifact_id: ArtifactId,
    component_id: ComponentId,
    kind: ArtifactKind,
    version: Version,
    checksum: Sha256Digest,
    installed_path: PathBuf,
}

impl InstalledComponent {
    /// Returns the installed artifact identifier.
    #[must_use]
    pub const fn artifact_id(&self) -> &ArtifactId {
        &self.artifact_id
    }

    /// Returns the logical component identifier.
    #[must_use]
    pub const fn component_id(&self) -> &ComponentId {
        &self.component_id
    }

    /// Returns the installed component role.
    #[must_use]
    pub const fn kind(&self) -> &ArtifactKind {
        &self.kind
    }

    /// Returns the exact installed version.
    #[must_use]
    pub const fn version(&self) -> &Version {
        &self.version
    }

    /// Returns the verified artifact checksum.
    #[must_use]
    pub const fn checksum(&self) -> Sha256Digest {
        self.checksum
    }

    /// Returns the installed payload path.
    #[must_use]
    pub fn installed_path(&self) -> &Path {
        &self.installed_path
    }
}

/// Filesystem durability boundary used for directory-entry commits.
pub trait FilesystemDurability: Send + Sync {
    /// Synchronizes directory metadata after a rename or removal.
    ///
    /// # Errors
    ///
    /// Returns the platform filesystem error when the directory entry cannot
    /// be made durable.
    fn sync_directory(&self, directory: &Path) -> Result<(), std::io::Error>;
}

#[derive(Debug)]
struct SystemFilesystemDurability;

impl FilesystemDurability for SystemFilesystemDurability {
    fn sync_directory(&self, directory: &Path) -> Result<(), std::io::Error> {
        system_sync_directory(directory)
    }
}

/// Atomic JSON repository for semantic component state.
#[derive(Clone)]
pub struct SemanticStateStore {
    directory: PathBuf,
    process_lock: Arc<Mutex<()>>,
    durability: Arc<dyn FilesystemDurability>,
}

impl std::fmt::Debug for SemanticStateStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SemanticStateStore")
            .field("directory", &self.directory)
            .finish_non_exhaustive()
    }
}

impl SemanticStateStore {
    /// Creates a repository in an injected configuration directory.
    #[must_use]
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self::with_filesystem_durability(directory, Arc::new(SystemFilesystemDurability))
    }

    /// Creates a repository with an injected directory durability boundary.
    ///
    /// This is primarily useful for platform adapters and deterministic fault
    /// injection around atomic rename commits.
    #[must_use]
    pub fn with_filesystem_durability(
        directory: impl Into<PathBuf>,
        durability: Arc<dyn FilesystemDurability>,
    ) -> Self {
        let directory = directory.into();
        Self {
            process_lock: process_lock_for(&directory),
            directory,
            durability,
        }
    }

    /// Loads state, defaulting its data root from injected app data when absent.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem or serialization failure.
    pub fn load_or_default(&self, app_data: &Path) -> Result<SemanticState, SemanticStateError> {
        self.with_exclusive_lock(|| self.load_or_default_unlocked(app_data))
    }

    pub(crate) fn load_or_default_unlocked(
        &self,
        app_data: &Path,
    ) -> Result<SemanticState, SemanticStateError> {
        crate::installer::recover_interrupted_uninstall(self, app_data)?;
        let state = self.read_state_unlocked(app_data)?;
        state.validate_referenced_payloads()?;
        Ok(state)
    }

    pub(crate) fn read_state_unlocked(
        &self,
        app_data: &Path,
    ) -> Result<SemanticState, SemanticStateError> {
        let path = self.path();
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(SemanticStateError::UnsafeStatePath { path })
            }
            Ok(_) => {
                let bytes = read_file_no_follow(&path)?;
                let state: SemanticState = serde_json::from_slice(&bytes)?;
                state.validate()?;
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let state = SemanticState::from_app_data(app_data);
                state.validate()?;
                Ok(state)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Atomically writes state through a synced sibling temporary file.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem or serialization failure.
    pub fn save(&self, state: &SemanticState) -> Result<(), SemanticStateError> {
        self.with_exclusive_lock(|| self.save_unlocked(state))
    }

    pub(crate) fn save_unlocked(&self, state: &SemanticState) -> Result<(), SemanticStateError> {
        self.ensure_directory()?;
        state.validate()?;
        state.validate_referenced_payloads()?;
        let state_path = self.path();
        let previous = match fs::symlink_metadata(&state_path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(SemanticStateError::UnsafeStatePath { path: state_path });
            }
            Ok(_) => Some(read_file_no_follow(&state_path)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let temporary = self
            .directory
            .join(format!(".{STATE_FILE_NAME}.{}.tmp", Uuid::new_v4()));
        let bytes = serde_json::to_vec_pretty(state)?;
        let result: Result<(), SemanticStateError> = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, &state_path)?;
            if let Err(commit_error) = self.sync_directory(&self.directory) {
                return match self.restore_previous_state(&state_path, previous.as_deref()) {
                    Ok(()) => Err(commit_error.into()),
                    Err(rollback_error) => Err(SemanticStateError::CommitOutcomeUnknown {
                        commit_error: commit_error.to_string(),
                        rollback_error: rollback_error.to_string(),
                    }),
                };
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn restore_previous_state(
        &self,
        state_path: &Path,
        previous: Option<&[u8]>,
    ) -> Result<(), std::io::Error> {
        if let Some(previous) = previous {
            let rollback = self
                .directory
                .join(format!(".{STATE_FILE_NAME}.{}.rollback", Uuid::new_v4()));
            let restore = (|| {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&rollback)?;
                file.write_all(previous)?;
                file.sync_all()?;
                fs::rename(&rollback, state_path)?;
                self.sync_directory(&self.directory)?;
                if read_file_no_follow(state_path)? != previous {
                    return Err(std::io::Error::other(
                        "restored semantic state did not match the previous bytes",
                    ));
                }
                Ok(())
            })();
            if restore.is_err() {
                let _ = fs::remove_file(rollback);
            }
            restore
        } else {
            match fs::remove_file(state_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            self.sync_directory(&self.directory)?;
            match fs::symlink_metadata(state_path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Ok(_) => Err(std::io::Error::other(
                    "semantic state remained visible after rollback",
                )),
                Err(error) => Err(error),
            }
        }
    }

    fn path(&self) -> PathBuf {
        self.directory.join(STATE_FILE_NAME)
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn sync_directory(&self, directory: &Path) -> Result<(), std::io::Error> {
        self.durability.sync_directory(directory)
    }

    pub(crate) fn with_exclusive_lock<T, E>(
        &self,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<SemanticStateError>,
    {
        let _process_guard = self
            .process_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.ensure_directory().map_err(E::from)?;
        let lock_path = self.directory.join(LOCK_FILE_NAME);
        match fs::symlink_metadata(&lock_path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(E::from(SemanticStateError::UnsafeStatePath {
                    path: lock_path,
                }));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(E::from(error.into())),
        }
        let mut lock_options = OpenOptions::new();
        lock_options
            .read(true)
            .write(true)
            .create(true)
            .truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            lock_options.custom_flags(libc::O_NOFOLLOW);
        }
        let lock = lock_options
            .open(&lock_path)
            .map_err(SemanticStateError::from)
            .map_err(E::from)?;
        let metadata = fs::symlink_metadata(&lock_path)
            .map_err(SemanticStateError::from)
            .map_err(E::from)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(E::from(SemanticStateError::UnsafeStatePath {
                path: lock_path,
            }));
        }
        fs2::FileExt::lock_exclusive(&lock)
            .map_err(SemanticStateError::from)
            .map_err(E::from)?;
        operation()
    }

    fn ensure_directory(&self) -> Result<(), SemanticStateError> {
        crate::data_root::ensure_no_symlink_components(&self.directory)?;
        fs::create_dir_all(&self.directory)?;
        let metadata = fs::symlink_metadata(&self.directory)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(SemanticStateError::UnsafeStatePath {
                path: self.directory.clone(),
            });
        }
        Ok(())
    }
}

pub(crate) fn read_file_no_follow(path: &Path) -> Result<Vec<u8>, std::io::Error> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn process_lock_for(directory: &Path) -> Arc<Mutex<()>> {
    static PROCESS_LOCKS: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

    let key = if directory.is_absolute() {
        directory.to_owned()
    } else {
        std::env::current_dir()
            .map(|current| current.join(directory))
            .unwrap_or_else(|_| directory.to_owned())
    };
    let mut locks = PROCESS_LOCKS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}

fn system_sync_directory(directory: &Path) -> Result<(), std::io::Error> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(directory)?
            .sync_all()
    }
    #[cfg(not(windows))]
    fs::File::open(directory)?.sync_all()
}

/// Semantic state validation or persistence failure.
#[derive(Debug, Error)]
pub enum SemanticStateError {
    /// The state directory or lock file is a symlink or unexpected entry.
    #[error("semantic state contains an unsafe path: {}", path.display())]
    UnsafeStatePath {
        /// Rejected state path.
        path: PathBuf,
    },
    /// An interrupted uninstall journal was malformed or disagreed with state.
    #[error("semantic uninstall recovery journal is inconsistent")]
    InvalidUninstallJournal,
    /// Persisted state violated a lifecycle or structural invariant.
    #[error("semantic component state is invalid")]
    InvalidPersistedState,
    /// Semantic data layout recovery failed.
    #[error(transparent)]
    DataRoot(#[from] crate::DataRootError),
    /// Replacing an active embedding space requires an explicit migration.
    #[error("an active embedding space can only be replaced by a model migration")]
    ModelMigrationRequired,
    /// Replacing an active index schema requires an explicit migration.
    #[error(
        "active index schema {active_version} can only be replaced by an explicit migration to {offered_version}"
    )]
    IndexSchemaMigrationRequired {
        /// Existing active schema.
        active_version: u32,
        /// Schema offered by the installation.
        offered_version: u32,
    },
    /// A second model migration was requested while one is pending.
    #[error("a model migration is already pending")]
    MigrationAlreadyPending,
    /// The requested migration would not change the active embedding space.
    #[error("model migration must target a distinct embedding space")]
    MigrationNotDistinct,
    /// Active model state changed after the plan was created.
    #[error("model migration plan is stale")]
    StaleMigration,
    /// No model migration is pending.
    #[error("no model migration is pending")]
    NoPendingMigration,
    /// A schema migration had zero or unchanged versions.
    #[error("invalid index schema migration from {from_version} to {to_version}")]
    InvalidSchemaMigration {
        /// Existing schema version.
        from_version: u32,
        /// Replacement schema version.
        to_version: u32,
    },
    /// A schema migration was requested without an active model/index.
    #[error("schema migration requires an active model")]
    MissingActiveModel,
    /// An active index schema must be non-zero.
    #[error("active index schema version must be non-zero")]
    InvalidIndexSchemaVersion,
    /// Persisted component path cannot be safely rebased during data movement.
    #[error("installed component is outside the semantic data root: {}", path.display())]
    ComponentOutsideDataRoot {
        /// Invalid installed component path.
        path: PathBuf,
    },
    /// Durable state references an installed payload that is absent or not a regular file.
    #[error(
        "installed artifact `{}` has no usable payload at {}",
        artifact.as_str(),
        path.display()
    )]
    ReferencedPayloadUnavailable {
        /// Referenced artifact identity.
        artifact: ArtifactId,
        /// Missing or unsafe payload path.
        path: PathBuf,
    },
    /// Reindex progress regressed or exceeded its estimate.
    #[error("invalid reindex progress {completed} of {total} documents")]
    InvalidMigrationProgress {
        /// Reported completed documents.
        completed: u64,
        /// Estimated document total.
        total: u64,
    },
    /// Reindex work has not reached its estimate.
    #[error("model migration is incomplete: {completed} of {total} documents")]
    MigrationIncomplete {
        /// Completed documents.
        completed: u64,
        /// Estimated document total.
        total: u64,
    },
    /// A persisted state schema is newer or otherwise unsupported.
    #[error("unsupported semantic state schema version {version}")]
    UnsupportedSchema {
        /// Unsupported schema version.
        version: u32,
    },
    /// State filesystem operation failed.
    #[error("semantic state filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    /// State rename became visible and restoring the prior state could not be
    /// durably verified. Callers must retain resources referenced by either
    /// state until recovery determines the visible commit.
    #[error(
        "semantic state commit outcome is unknown after `{commit_error}`; rollback failed: {rollback_error}"
    )]
    CommitOutcomeUnknown {
        /// Directory-sync failure after the new state rename.
        commit_error: String,
        /// Failure while restoring or verifying the previous state.
        rollback_error: String,
    },
    /// State serialization failed.
    #[error("semantic state serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
}

impl SemanticStateError {
    pub(crate) const fn commit_outcome_unknown(&self) -> bool {
        matches!(self, Self::CommitOutcomeUnknown { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelId, ModelRevision};

    #[test]
    fn staging_a_model_revision_retains_the_current_payload() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = SemanticState::from_app_data(directory.path());
        let component = ComponentId::new("fixture.embedding").unwrap();
        let first = ModelIdentity::new(
            ModelId::new("fixture.model").unwrap(),
            ModelRevision::new("revision-a").unwrap(),
        );
        let second = ModelIdentity::new(
            ModelId::new("fixture.model").unwrap(),
            ModelRevision::new("revision-b").unwrap(),
        );

        state.record_installed(
            ArtifactId::new("model-a").unwrap(),
            component.clone(),
            ArtifactKind::Model(first.clone()),
            "1.0.0".parse().unwrap(),
            Sha256Digest::calculate(b"model-a"),
            directory.path().join("model-a"),
        );
        state.record_installed(
            ArtifactId::new("model-b").unwrap(),
            component,
            ArtifactKind::Model(second.clone()),
            "2.0.0".parse().unwrap(),
            Sha256Digest::calculate(b"model-b"),
            directory.path().join("model-b"),
        );

        assert_eq!(
            state
                .installed_model(&first)
                .unwrap()
                .artifact_id()
                .as_str(),
            "model-a"
        );
        assert_eq!(
            state
                .installed_model(&second)
                .unwrap()
                .artifact_id()
                .as_str(),
            "model-b"
        );
    }
}
