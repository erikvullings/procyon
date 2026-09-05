use std::fs;
use std::path::{Path, PathBuf};

use fm_settings::{DocumentError, SettingsStore, VersionedDocument};
use serde_json::Value;
use thiserror::Error;

use crate::{
    CURRENT_POLICY_SCHEMA_VERSION, CatalogError, LibraryStateError, PolicyError, SemanticCatalog,
    SemanticLibraryPolicy, SemanticLibraryState,
};

pub(crate) const CATALOG_DIRECTORY: &str = "catalog";
pub(crate) const CATALOG_FILE_NAME: &str = "catalog.json";
const STATE_DIRECTORY: &str = "state";
pub(crate) const STATE_FILE_NAME: &str = "library-state.json";

/// File name of the semantic consent policy within the configuration directory.
pub const POLICY_FILE_NAME: &str = "semantic-library-policy.json";

impl VersionedDocument for SemanticLibraryPolicy {
    type MigrationError = PolicyError;

    const FILE_NAME: &'static str = POLICY_FILE_NAME;
    const CURRENT_SCHEMA_VERSION: u32 = CURRENT_POLICY_SCHEMA_VERSION;

    fn migrate(mut value: Value, version: u32) -> Result<Value, PolicyError> {
        match version {
            1 => {
                if let Some(roots) = value.get_mut("roots").and_then(Value::as_object_mut) {
                    for root in roots.values_mut().filter_map(Value::as_object_mut) {
                        if let Some(workspace_ids) = root.remove("workspaceIds") {
                            root.entry("workspaceReferences").or_insert(workspace_ids);
                        }
                        if let Some(exclusions) =
                            root.get_mut("exclusions").and_then(Value::as_array_mut)
                        {
                            for exclusion in exclusions.iter_mut().filter_map(Value::as_object_mut)
                            {
                                exclusion
                                    .entry("cleanupStatus")
                                    .or_insert_with(|| Value::from("complete"));
                            }
                        }
                    }
                }
                Self::migrate(value, 2)
            }
            2 => {
                // A policy written before durable revisions starts at one:
                // every in-flight optimistic token minted by an older process
                // is therefore stale rather than silently accepted.
                value["revision"] = Value::from(1_u64);
                value["schemaVersion"] = Value::from(CURRENT_POLICY_SCHEMA_VERSION);
                Ok(value)
            }
            CURRENT_POLICY_SCHEMA_VERSION => Ok(value),
            unsupported => Err(PolicyError::UnsupportedSchema(unsupported)),
        }
    }

    fn validate(&self) -> Result<(), PolicyError> {
        self.validate_structure()
    }
}

/// Low-volume semantic consent policy stored in the configuration directory.
///
/// The policy is an independently versioned sidecar of `settings.json`: it
/// reuses the settings migration machinery — strict schema-version reading,
/// migration, validation, and atomic writes — while keeping its own schema
/// version. A stale general settings write therefore cannot revoke or
/// resurrect consent, and high-volume catalog state stays out of the
/// configuration directory.
#[derive(Debug, Clone)]
pub struct SemanticLibraryPolicyStore {
    settings: SettingsStore,
}

impl SemanticLibraryPolicyStore {
    /// Creates a policy store inside an explicit configuration directory.
    #[must_use]
    pub fn new(configuration_directory: impl Into<PathBuf>) -> Self {
        Self {
            settings: SettingsStore::new(configuration_directory),
        }
    }

    /// Creates a policy store that shares an existing settings directory.
    #[must_use]
    pub const fn from_settings_store(settings: SettingsStore) -> Self {
        Self { settings }
    }

    /// Returns the durable policy file path.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.settings.document_path::<SemanticLibraryPolicy>()
    }

    /// Loads, migrates, and validates the policy.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem, JSON, schema-version, migration, or
    /// validation failure, and [`StoreError::PolicyMissing`] when no policy
    /// was ever written.
    pub fn load(&self) -> Result<SemanticLibraryPolicy, StoreError> {
        self.load_optional()?.ok_or(StoreError::PolicyMissing)
    }

    /// Loads the policy, or returns `None` when none was ever written.
    ///
    /// # Errors
    ///
    /// Returns every [`Self::load`] failure except the missing-policy case.
    pub fn load_optional(&self) -> Result<Option<SemanticLibraryPolicy>, StoreError> {
        Ok(self.settings.load_document::<SemanticLibraryPolicy>()?)
    }

    /// Validates and atomically persists the policy.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem, JSON, or validation failure,
    /// [`StoreError::IdentityChanged`] when the stable library or model
    /// identity would silently change, and [`StoreError::RevisionRegressed`]
    /// when the durable monotonic revision would move backwards.
    pub fn save(&self, policy: &SemanticLibraryPolicy) -> Result<(), StoreError> {
        if let Some(existing) = self.load_optional()? {
            if existing.library() != policy.library() {
                return Err(StoreError::IdentityChanged);
            }
            // Replaying a committed journal record installs the same revision
            // again, which is idempotent; moving backwards is not.
            if policy.revision() < existing.revision() {
                return Err(StoreError::RevisionRegressed);
            }
        }
        self.settings.save_document(policy)?;
        Ok(())
    }
}

/// Atomic JSON repository for high-volume authoritative catalog state.
///
/// Its fixed location beneath the semantic-data root keeps document state out
/// of the ordinary settings directory.
#[derive(Debug, Clone)]
pub struct SemanticCatalogStore {
    semantic_data_root: PathBuf,
}

impl SemanticCatalogStore {
    /// Creates a catalog repository beneath an explicit semantic-data root.
    #[must_use]
    pub fn new(semantic_data_root: impl Into<PathBuf>) -> Self {
        Self {
            semantic_data_root: semantic_data_root.into(),
        }
    }

    /// Returns the authoritative catalog file path.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.directory().join(CATALOG_FILE_NAME)
    }

    fn directory(&self) -> PathBuf {
        self.semantic_data_root.join(CATALOG_DIRECTORY)
    }

    /// Loads and validates the catalog.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem, JSON, or catalog validation failure.
    pub fn load(&self) -> Result<SemanticCatalog, StoreError> {
        self.load_optional()?.ok_or(StoreError::CatalogMissing)
    }

    /// Loads the catalog, or returns `None` when none was ever written.
    ///
    /// # Errors
    ///
    /// Returns every [`Self::load`] failure except the missing-file case.
    pub fn load_optional(&self) -> Result<Option<SemanticCatalog>, StoreError> {
        match fs::read(self.path()) {
            Ok(bytes) => {
                let catalog: SemanticCatalog = serde_json::from_slice(&bytes)?;
                catalog.validate()?;
                Ok(Some(catalog))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Validates and atomically persists the catalog.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem, JSON, or catalog validation failure.
    pub fn save(&self, catalog: &SemanticCatalog) -> Result<(), StoreError> {
        catalog.validate()?;
        if let Some(existing) = self.load_optional()?
            && existing.library_id() != catalog.library_id()
        {
            return Err(StoreError::IdentityChanged);
        }
        let bytes = serde_json::to_vec_pretty(catalog)?;
        write_atomically(&self.directory(), CATALOG_FILE_NAME, &bytes)
    }
}

/// Atomic JSON repository for pause and complete-generation state.
#[derive(Debug, Clone)]
pub struct SemanticLibraryStateStore {
    semantic_data_root: PathBuf,
}

impl SemanticLibraryStateStore {
    /// Creates a runtime-state store beneath a semantic-data root.
    #[must_use]
    pub fn new(semantic_data_root: impl Into<PathBuf>) -> Self {
        Self {
            semantic_data_root: semantic_data_root.into(),
        }
    }

    /// Returns the runtime-state path.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.directory().join(STATE_FILE_NAME)
    }

    fn directory(&self) -> PathBuf {
        self.semantic_data_root.join(STATE_DIRECTORY)
    }

    /// Loads and validates runtime state.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem, JSON, or state validation failure.
    pub fn load(&self) -> Result<SemanticLibraryState, StoreError> {
        self.load_optional()?.ok_or(StoreError::StateMissing)
    }

    /// Loads runtime state, migrating an older schema, or returns `None` when
    /// none was ever written.
    ///
    /// # Errors
    ///
    /// Returns every [`Self::load`] failure except the missing-file case.
    pub fn load_optional(&self) -> Result<Option<SemanticLibraryState>, StoreError> {
        match fs::read(self.path()) {
            Ok(bytes) => {
                let value: Value = serde_json::from_slice(&bytes)?;
                Ok(Some(SemanticLibraryState::migrate(value)?))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Atomically persists runtime state.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem, JSON, or state validation failure.
    pub fn save(&self, state: &SemanticLibraryState) -> Result<(), StoreError> {
        state.validate()?;
        if let Some(existing) = self.load_optional()?
            && existing.library_id() != state.library_id()
        {
            return Err(StoreError::IdentityChanged);
        }
        let bytes = serde_json::to_vec_pretty(state)?;
        write_atomically(&self.directory(), STATE_FILE_NAME, &bytes)
    }
}

pub(crate) fn write_atomically(
    directory: &Path,
    file_name: &str,
    bytes: &[u8],
) -> Result<(), StoreError> {
    fm_settings::atomic_write(directory, file_name, bytes).map_err(StoreError::Io)
}

/// Policy/catalog persistence failure.
#[derive(Debug, Error)]
pub enum StoreError {
    /// No semantic consent policy has been written yet.
    #[error("no semantic library policy has been written")]
    PolicyMissing,
    /// No semantic catalog has been written yet.
    #[error("no semantic catalog has been written")]
    CatalogMissing,
    /// No semantic runtime state has been written yet.
    #[error("no semantic library state has been written")]
    StateMissing,
    /// A normal write attempted to replace the stable library/model identity.
    #[error("semantic library or model identity cannot change without migration")]
    IdentityChanged,
    /// A write would move the durable monotonic policy revision backwards.
    #[error("semantic library policy revision cannot regress")]
    RevisionRegressed,
    /// A mutation was attempted while a committed journal record was pending.
    #[error("a committed semantic library transaction is still pending recovery")]
    PendingJournalRecord,
    /// The cross-process semantic library lock could not be acquired.
    #[error("semantic library lock is unavailable: {0}")]
    Lock(std::io::Error),
    /// The journal, catalog, or lock path is a symlink or unexpected entry.
    #[error("semantic library path is unsafe: {}", .0.display())]
    UnsafePath(std::path::PathBuf),
    /// The configuration-directory policy document could not be read.
    #[error(transparent)]
    PolicyDocument(#[from] DocumentError<PolicyError>),
    /// Policy validation failed.
    #[error(transparent)]
    Policy(#[from] PolicyError),
    /// Catalog validation failed.
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    /// Runtime-state validation failed.
    #[error(transparent)]
    State(#[from] LibraryStateError),
    /// Filesystem persistence failed.
    #[error("semantic library filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization or deserialization failed.
    #[error("semantic library JSON operation failed: {0}")]
    Json(#[from] serde_json::Error),
    /// Policy, catalog, and state on disk describe different libraries.
    #[error("semantic policy, catalog, and state belong to different libraries")]
    LibraryMismatch,
    /// A journal record described another library.
    #[error("semantic journal record belongs to a different library")]
    JournalLibraryMismatch,
    /// A journal record was structurally unusable.
    #[error("semantic journal record is malformed")]
    JournalCorrupt,
    /// A transaction step was requested out of order.
    #[error("semantic library transaction step is out of order")]
    TransactionOutOfOrder,
    /// A transaction was staged with no participating document.
    #[error("semantic library transaction has no participants")]
    EmptyTransaction,
}
