use std::collections::{BTreeMap, BTreeSet};

use fm_domain::{Location, WorkspaceId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::hierarchy::{depth, is_same_or_descendant, rebase};
use crate::{
    DeletionPlanId, EligibilityOverride, EligibilityReason, ExclusionId, LibraryId, RootId,
    VocabularyId,
};

/// Current durable semantic-library policy schema.
pub const CURRENT_POLICY_SCHEMA_VERSION: u32 = 3;

/// Exact embedding identity applied to every document in a library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelIdentity {
    model_id: String,
    revision: String,
    dimensions: u32,
    embedding_space: String,
}

impl ModelIdentity {
    /// Creates an immutable model identity.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidModelIdentity`] for empty identifiers or
    /// zero-dimensional models.
    pub fn new(
        model_id: impl Into<String>,
        revision: impl Into<String>,
        dimensions: u32,
        embedding_space: impl Into<String>,
    ) -> Result<Self, PolicyError> {
        let identity = Self {
            model_id: model_id.into(),
            revision: revision.into(),
            dimensions,
            embedding_space: embedding_space.into(),
        };
        identity.validate()?;
        Ok(identity)
    }

    fn validate(&self) -> Result<(), PolicyError> {
        if self.model_id.trim().is_empty()
            || self.revision.trim().is_empty()
            || self.embedding_space.trim().is_empty()
            || self.dimensions == 0
        {
            return Err(PolicyError::InvalidModelIdentity);
        }
        Ok(())
    }

    /// Returns the curated or expert model identifier.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the immutable upstream revision.
    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }

    /// Returns embedding dimensions.
    #[must_use]
    pub const fn dimensions(&self) -> u32 {
        self.dimensions
    }

    /// Returns the embedding-space identity.
    #[must_use]
    pub fn embedding_space(&self) -> &str {
        &self.embedding_space
    }
}

/// Stable device-local library and its immutable embedding identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLibraryIdentity {
    id: LibraryId,
    model: ModelIdentity,
}

impl DeviceLibraryIdentity {
    /// Creates a library identity.
    #[must_use]
    pub const fn new(id: LibraryId, model: ModelIdentity) -> Self {
        Self { id, model }
    }

    /// Returns the stable library id.
    #[must_use]
    pub const fn id(&self) -> LibraryId {
        self.id
    }

    /// Returns the exact model identity.
    #[must_use]
    pub const fn model(&self) -> &ModelIdentity {
        &self.model
    }
}

/// Resource/quality preset chosen for semantic processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceProfileKind {
    /// Minimise local storage and memory use.
    Compact,
    /// Balance resource use and coverage.
    Balanced,
    /// Prioritise quality within explicit hard budgets.
    Quality,
}

/// Hard resource ceilings applied before content is fed to the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceBudgets {
    /// Maximum number of documents in the library.
    pub max_documents: u64,
    /// Maximum source bytes for one document.
    pub max_source_bytes_per_document: u64,
    /// Maximum cumulative source bytes represented by the library.
    pub max_total_source_bytes: u64,
    /// Maximum cumulative extracted-text bytes.
    pub max_total_extracted_bytes: u64,
    /// Maximum cumulative vector bytes.
    pub max_total_vector_bytes: u64,
}

impl Default for ResourceBudgets {
    fn default() -> Self {
        Self {
            max_documents: 1_000_000,
            max_source_bytes_per_document: 512 * 1024 * 1024,
            max_total_source_bytes: 4 * 1024 * 1024 * 1024 * 1024,
            max_total_extracted_bytes: 1024 * 1024 * 1024 * 1024,
            max_total_vector_bytes: 1024 * 1024 * 1024 * 1024,
        }
    }
}

impl ResourceBudgets {
    fn validate(self) -> Result<(), PolicyError> {
        if self.max_documents == 0
            || self.max_source_bytes_per_document == 0
            || self.max_total_source_bytes == 0
            || self.max_total_extracted_bytes == 0
            || self.max_total_vector_bytes == 0
            || self.max_source_bytes_per_document > self.max_total_source_bytes
        {
            return Err(PolicyError::InvalidResourceBudgets);
        }
        Ok(())
    }
}

/// Selected resource preset and its enforceable budgets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceProfile {
    /// Human-facing preset.
    pub kind: ResourceProfileKind,
    /// Hard ceilings.
    pub budgets: ResourceBudgets,
}

/// Provider-neutral stable identity, when a provider can prove one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemIdentity {
    volume_id: String,
    file_id: String,
}

impl FilesystemIdentity {
    /// Creates a stable `(volume, file)` identity.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidFilesystemIdentity`] for empty values.
    pub fn new(
        volume_id: impl Into<String>,
        file_id: impl Into<String>,
    ) -> Result<Self, PolicyError> {
        let identity = Self {
            volume_id: volume_id.into(),
            file_id: file_id.into(),
        };
        identity.validate()?;
        Ok(identity)
    }

    /// Returns the provider's opaque volume identity.
    #[must_use]
    pub fn volume_id(&self) -> &str {
        &self.volume_id
    }

    /// Returns the provider's opaque file identity.
    #[must_use]
    pub fn file_id(&self) -> &str {
        &self.file_id
    }

    fn validate(&self) -> Result<(), PolicyError> {
        if self.volume_id.trim().is_empty() || self.file_id.trim().is_empty() {
            Err(PolicyError::InvalidFilesystemIdentity)
        } else {
            Ok(())
        }
    }
}

/// Cleanup state for an exclusion whose consent is already revoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExclusionCleanupStatus {
    /// Queries exclude the scope while destructive cleanup is outstanding.
    Pending,
    /// Every required deletion category completed.
    Complete,
}

/// Explicit descendant exclusion within an enrolled root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescendantExclusion {
    id: ExclusionId,
    location: Location,
    cleanup_status: ExclusionCleanupStatus,
    #[serde(default)]
    deletion_plan_id: Option<DeletionPlanId>,
}

impl DescendantExclusion {
    /// Returns the exclusion id.
    #[must_use]
    pub const fn id(&self) -> ExclusionId {
        self.id
    }

    /// Returns the excluded provider-neutral location.
    #[must_use]
    pub const fn location(&self) -> &Location {
        &self.location
    }

    /// Returns deletion cleanup state. Consent is revoked in either state.
    #[must_use]
    pub const fn cleanup_status(&self) -> ExclusionCleanupStatus {
        self.cleanup_status
    }

    /// Returns the durable deletion plan attached to this exclusion.
    #[must_use]
    pub const fn deletion_plan_id(&self) -> Option<DeletionPlanId> {
        self.deletion_plan_id
    }

    pub(crate) fn attach_deletion_plan(
        &mut self,
        plan_id: DeletionPlanId,
    ) -> Result<(), PolicyError> {
        if self.cleanup_status != ExclusionCleanupStatus::Pending || self.deletion_plan_id.is_some()
        {
            return Err(PolicyError::ExclusionCleanupAlreadyPlanned);
        }
        self.deletion_plan_id = Some(plan_id);
        Ok(())
    }

    pub(crate) fn complete_cleanup(&mut self, plan_id: DeletionPlanId) -> Result<(), PolicyError> {
        if self.deletion_plan_id != Some(plan_id) {
            return Err(PolicyError::ExclusionPlanMismatch);
        }
        self.cleanup_status = ExclusionCleanupStatus::Complete;
        Ok(())
    }

    /// Returns the same exclusion — identity, cleanup status, and attached
    /// deletion plan preserved — positioned relative to a moved root.
    fn relocated(
        &self,
        previous_root: &Location,
        current_root: &Location,
    ) -> Result<Self, PolicyError> {
        Ok(Self {
            id: self.id,
            location: rebase(previous_root, current_root, &self.location)
                .map_err(PolicyError::InvalidLocation)?,
            cleanup_status: self.cleanup_status,
            deletion_plan_id: self.deletion_plan_id,
        })
    }
}

/// One durable root consent shared by any number of workspace references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrolledRoot {
    id: RootId,
    location: Location,
    filesystem_identity: Option<FilesystemIdentity>,
    recursive: bool,
    #[serde(default)]
    exclusions: Vec<DescendantExclusion>,
    #[serde(default)]
    eligibility_overrides: BTreeMap<EligibilityReason, EligibilityOverride>,
    #[serde(default)]
    vocabulary_ids: BTreeSet<VocabularyId>,
    #[serde(default)]
    workspace_references: Vec<WorkspaceId>,
}

impl EnrolledRoot {
    /// Creates provider-neutral root consent.
    #[must_use]
    pub fn new(
        id: RootId,
        location: Location,
        filesystem_identity: Option<FilesystemIdentity>,
        recursive: bool,
    ) -> Self {
        Self {
            id,
            location,
            filesystem_identity,
            recursive,
            exclusions: Vec::new(),
            eligibility_overrides: BTreeMap::new(),
            vocabulary_ids: BTreeSet::new(),
            workspace_references: Vec::new(),
        }
    }

    /// Returns the stable root id.
    #[must_use]
    pub const fn id(&self) -> RootId {
        self.id
    }

    /// Returns the current provider-neutral location.
    #[must_use]
    pub const fn location(&self) -> &Location {
        &self.location
    }

    /// Returns this root moved to `current`, with every explicit descendant
    /// exclusion repositioned relative to the new location.
    ///
    /// Exclusion identities, cleanup status, and attached deletion plans are
    /// preserved: a proven move must not silently restore consent to a
    /// previously revoked subtree.
    pub(crate) fn relocated(&self, current: &Location) -> Result<Self, PolicyError> {
        let mut relocated = self.clone();
        relocated.exclusions = self
            .exclusions
            .iter()
            .map(|exclusion| exclusion.relocated(&self.location, current))
            .collect::<Result<Vec<_>, _>>()?;
        relocated.location = current.clone();
        Ok(relocated)
    }

    /// Returns stable provider filesystem identity when available.
    #[must_use]
    pub const fn filesystem_identity(&self) -> Option<&FilesystemIdentity> {
        self.filesystem_identity.as_ref()
    }

    /// Reports recursive consent.
    #[must_use]
    pub const fn recursive(&self) -> bool {
        self.recursive
    }

    /// Adds a workspace reference without creating another root consent.
    pub fn attach_workspace(&mut self, workspace_id: WorkspaceId) {
        if !self.workspace_references.contains(&workspace_id) {
            self.workspace_references.push(workspace_id);
        }
    }

    /// Removes only a workspace reference, not root consent.
    pub fn detach_workspace(&mut self, workspace_id: WorkspaceId) {
        self.workspace_references
            .retain(|candidate| *candidate != workspace_id);
    }

    /// Returns all workspace references.
    #[must_use]
    pub fn workspace_references(&self) -> &[WorkspaceId] {
        &self.workspace_references
    }

    /// Attaches a vocabulary.
    pub fn attach_vocabulary(&mut self, vocabulary_id: VocabularyId) {
        self.vocabulary_ids.insert(vocabulary_id);
    }

    /// Returns attached vocabulary ids.
    #[must_use]
    pub const fn vocabulary_ids(&self) -> &BTreeSet<VocabularyId> {
        &self.vocabulary_ids
    }

    /// Replaces per-root eligibility overrides.
    pub fn set_eligibility_overrides(
        &mut self,
        overrides: BTreeMap<EligibilityReason, EligibilityOverride>,
    ) {
        self.eligibility_overrides = overrides;
    }

    /// Returns per-root eligibility overrides.
    #[must_use]
    pub const fn eligibility_overrides(&self) -> &BTreeMap<EligibilityReason, EligibilityOverride> {
        &self.eligibility_overrides
    }

    /// Returns explicit descendant exclusions.
    #[must_use]
    pub fn exclusions(&self) -> &[DescendantExclusion] {
        &self.exclusions
    }
}

/// Durable low-volume consent policy, independent of ordinary UI settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticLibraryPolicy {
    schema_version: u32,
    library: DeviceLibraryIdentity,
    /// Durable monotonic mutation counter.
    ///
    /// This is the *only* optimistic-concurrency authority. A process-local
    /// counter cannot see another process's committed mutation, so a second
    /// service over the same roots would silently overwrite it; comparing the
    /// caller's expected revision against the value just re-read from disk
    /// under the cross-process lock turns that lost update into an explicit
    /// stale-revision failure.
    revision: u64,
    resource_profile: ResourceProfile,
    reconciliation_interval_seconds: u64,
    #[serde(default)]
    roots: BTreeMap<RootId, EnrolledRoot>,
}

impl SemanticLibraryPolicy {
    /// Creates an empty policy with a 30-minute reconciliation interval.
    ///
    /// # Errors
    ///
    /// Returns a validation error for invalid model or resource metadata.
    pub fn new(
        library: DeviceLibraryIdentity,
        resource_profile: ResourceProfile,
    ) -> Result<Self, PolicyError> {
        let policy = Self {
            schema_version: CURRENT_POLICY_SCHEMA_VERSION,
            library,
            revision: 1,
            resource_profile,
            reconciliation_interval_seconds: 30 * 60,
            roots: BTreeMap::new(),
        };
        policy.validate_structure()?;
        Ok(policy)
    }

    /// Returns the durable monotonic mutation revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Advances the durable revision by exactly one.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::RevisionOverflow`] when the counter is exhausted.
    pub fn advance_revision(&mut self) -> Result<u64, PolicyError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(PolicyError::RevisionOverflow)?;
        Ok(self.revision)
    }

    /// Returns the durable schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Returns the library and model identity.
    #[must_use]
    pub const fn library(&self) -> &DeviceLibraryIdentity {
        &self.library
    }

    /// Returns the resource profile and hard budgets.
    #[must_use]
    pub const fn resource_profile(&self) -> &ResourceProfile {
        &self.resource_profile
    }

    /// Changes the reconciliation interval.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidReconciliationInterval`] outside one
    /// minute through one day.
    pub fn set_reconciliation_interval_seconds(&mut self, seconds: u64) -> Result<(), PolicyError> {
        if !(60..=24 * 60 * 60).contains(&seconds) {
            return Err(PolicyError::InvalidReconciliationInterval);
        }
        self.reconciliation_interval_seconds = seconds;
        Ok(())
    }

    /// Returns the reconciliation interval in seconds.
    #[must_use]
    pub const fn reconciliation_interval_seconds(&self) -> u64 {
        self.reconciliation_interval_seconds
    }

    /// Adds a new path-independent root consent.
    ///
    /// # Errors
    ///
    /// Rejects duplicate ids, unsafe persisted locations, and invalid
    /// filesystem identities.
    pub fn enrol_root(&mut self, root: EnrolledRoot) -> Result<(), PolicyError> {
        validate_location(&root.location)?;
        if let Some(identity) = &root.filesystem_identity {
            identity.validate()?;
        }
        if self.roots.contains_key(&root.id) {
            return Err(PolicyError::DuplicateRoot(root.id));
        }
        self.roots.insert(root.id, root);
        Ok(())
    }

    /// Returns a root by stable id.
    #[must_use]
    pub fn root(&self, root_id: RootId) -> Option<&EnrolledRoot> {
        self.roots.get(&root_id)
    }

    /// Returns a mutable root by stable id.
    #[must_use]
    pub fn root_mut(&mut self, root_id: RootId) -> Option<&mut EnrolledRoot> {
        self.roots.get_mut(&root_id)
    }

    /// Returns every globally consented root.
    #[must_use]
    pub const fn roots(&self) -> &BTreeMap<RootId, EnrolledRoot> {
        &self.roots
    }

    /// Rejects combining this policy with another library's catalog or state
    /// before any mutation is attempted.
    pub(crate) fn ensure_library(&self, library_id: LibraryId) -> Result<(), PolicyError> {
        if self.library.id() == library_id {
            Ok(())
        } else {
            Err(PolicyError::LibraryMismatch)
        }
    }

    pub(crate) fn replace_root(&mut self, root: EnrolledRoot) -> Result<(), PolicyError> {
        if !self.roots.contains_key(&root.id) {
            return Err(PolicyError::UnknownRoot(root.id));
        }
        self.roots.insert(root.id, root);
        Ok(())
    }

    /// Removes one workspace's references while preserving global root consent.
    pub fn remove_workspace_reference(&mut self, workspace_id: WorkspaceId) {
        for root in self.roots.values_mut() {
            root.detach_workspace(workspace_id);
        }
    }

    /// Evaluates effective consent without consulting availability.
    ///
    /// # Errors
    ///
    /// Returns a provider-location validation failure.
    pub fn consent_state(&self, location: &Location) -> Result<crate::ConsentState, PolicyError> {
        crate::consent::evaluate(self, location)
    }

    pub(crate) fn resolve_root_location(
        &mut self,
        root_id: RootId,
        observations: &[crate::ObservedRootIdentity],
    ) -> Result<crate::RootMoveResolution, PolicyError> {
        crate::identity::resolve(self, root_id, observations)
    }

    /// Creates a bounded disclosure preview from an injected host estimator.
    ///
    /// # Errors
    ///
    /// Returns a typed root, estimator, or arithmetic validation failure.
    pub fn preview_enrolment(
        &self,
        root_id: RootId,
        estimator: &impl crate::EnrolmentEstimator,
    ) -> Result<crate::EnrolmentPreview, crate::PreviewError> {
        crate::preview::preview(self, root_id, estimator)
    }

    /// Adds an immediately effective descendant exclusion.
    ///
    /// # Errors
    ///
    /// The location must be a proper provider-aware descendant of the root.
    pub fn exclude_descendant(
        &mut self,
        root_id: RootId,
        exclusion_id: ExclusionId,
        location: Location,
    ) -> Result<(), PolicyError> {
        validate_location(&location)?;
        let root = self
            .roots
            .get_mut(&root_id)
            .ok_or(PolicyError::UnknownRoot(root_id))?;
        if location == root.location
            || !is_same_or_descendant(&root.location, &location)
                .map_err(PolicyError::InvalidLocation)?
        {
            return Err(PolicyError::InvalidExclusionLocation);
        }
        if root
            .exclusions
            .iter()
            .any(|exclusion| exclusion.id == exclusion_id)
        {
            return Err(PolicyError::DuplicateExclusion(exclusion_id));
        }
        root.exclusions.push(DescendantExclusion {
            id: exclusion_id,
            location,
            cleanup_status: ExclusionCleanupStatus::Pending,
            deletion_plan_id: None,
        });
        Ok(())
    }

    /// Revokes consent for an enrolled root while retaining its record until
    /// destructive cleanup completes.
    ///
    /// # Errors
    ///
    /// Rejects an unknown root or duplicate exclusion id.
    pub fn exclude_root(
        &mut self,
        root_id: RootId,
        exclusion_id: ExclusionId,
    ) -> Result<(), PolicyError> {
        let root = self
            .roots
            .get_mut(&root_id)
            .ok_or(PolicyError::UnknownRoot(root_id))?;
        if root
            .exclusions
            .iter()
            .any(|exclusion| exclusion.id == exclusion_id)
        {
            return Err(PolicyError::DuplicateExclusion(exclusion_id));
        }
        root.exclusions.push(DescendantExclusion {
            id: exclusion_id,
            location: root.location.clone(),
            cleanup_status: ExclusionCleanupStatus::Pending,
            deletion_plan_id: None,
        });
        Ok(())
    }

    /// Returns an explicit exclusion.
    #[must_use]
    pub fn exclusion(
        &self,
        root_id: RootId,
        exclusion_id: ExclusionId,
    ) -> Option<&DescendantExclusion> {
        self.root(root_id)?
            .exclusions
            .iter()
            .find(|exclusion| exclusion.id == exclusion_id)
    }

    pub(crate) fn exclusion_mut(
        &mut self,
        root_id: RootId,
        exclusion_id: ExclusionId,
    ) -> Result<&mut DescendantExclusion, PolicyError> {
        self.root_mut(root_id)
            .ok_or(PolicyError::UnknownRoot(root_id))?
            .exclusions
            .iter_mut()
            .find(|exclusion| exclusion.id == exclusion_id)
            .ok_or(PolicyError::UnknownExclusion(exclusion_id))
    }

    pub(crate) fn validate_structure(&self) -> Result<(), PolicyError> {
        if self.schema_version != CURRENT_POLICY_SCHEMA_VERSION {
            return Err(PolicyError::UnsupportedSchema(self.schema_version));
        }
        if self.revision == 0 {
            return Err(PolicyError::InvalidRevision);
        }
        self.library.model.validate()?;
        self.resource_profile.budgets.validate()?;
        if !(60..=24 * 60 * 60).contains(&self.reconciliation_interval_seconds) {
            return Err(PolicyError::InvalidReconciliationInterval);
        }
        for (id, root) in &self.roots {
            if *id != root.id {
                return Err(PolicyError::InvalidRootKey);
            }
            validate_location(&root.location)?;
            if root.eligibility_overrides.iter().any(|(reason, action)| {
                *action == EligibilityOverride::Include && !reason.can_be_explicitly_included()
            }) {
                return Err(PolicyError::UnsafeEligibilityOverride);
            }
            for exclusion in &root.exclusions {
                validate_location(&exclusion.location)?;
                if !is_same_or_descendant(&root.location, &exclusion.location)
                    .map_err(PolicyError::InvalidLocation)?
                {
                    return Err(PolicyError::InvalidExclusionLocation);
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_location(location: &Location) -> Result<(), PolicyError> {
    if location.uri.contains(['?', '#']) {
        return Err(PolicyError::TransientLocationData);
    }
    let authority = location
        .uri
        .split_once("://")
        .map(|(_, remainder)| remainder.split('/').next().unwrap_or(remainder))
        .unwrap_or_default();
    if authority.contains('@') || authority.to_ascii_lowercase().contains("%40") {
        return Err(PolicyError::LocationUserInfo);
    }
    Location::try_new(location.provider_id.clone(), location.uri.clone())
        .map_err(PolicyError::InvalidLocation)?;
    depth(location).map_err(PolicyError::InvalidLocation)?;
    Ok(())
}

/// Semantic policy validation failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PolicyError {
    /// Model identity is incomplete.
    #[error("semantic model identity is invalid")]
    InvalidModelIdentity,
    /// One or more hard resource budgets are zero or inconsistent.
    #[error("semantic resource budgets are invalid")]
    InvalidResourceBudgets,
    /// Stable filesystem identity is incomplete.
    #[error("stable filesystem identity is invalid")]
    InvalidFilesystemIdentity,
    /// Reconciliation must occur between once per minute and once per day.
    #[error("semantic reconciliation interval is invalid")]
    InvalidReconciliationInterval,
    /// The persisted schema cannot be loaded.
    #[error("unsupported semantic policy schema version {0}")]
    UnsupportedSchema(u32),
    /// A root id was enrolled more than once.
    #[error("semantic root {0} is already enrolled")]
    DuplicateRoot(RootId),
    /// No root has this id.
    #[error("semantic root {0} does not exist")]
    UnknownRoot(RootId),
    /// An exclusion id was reused within one root.
    #[error("semantic exclusion {0} already exists")]
    DuplicateExclusion(ExclusionId),
    /// An exclusion was not a proper descendant of its root.
    #[error("semantic exclusion is not a proper descendant of its root")]
    InvalidExclusionLocation,
    /// A persisted map key disagreed with its root record.
    #[error("semantic root map contains an inconsistent id")]
    InvalidRootKey,
    /// Provider location parsing failed.
    #[error("semantic policy location is invalid: {0}")]
    InvalidLocation(fm_domain::LocationError),
    /// Query/fragment data may contain transient credentials or session tokens.
    #[error("semantic policy locations cannot contain query or fragment data")]
    TransientLocationData,
    /// URL userinfo must never enter semantic policy.
    #[error("semantic policy locations cannot contain URL userinfo")]
    LocationUserInfo,
    /// Hard safety and resource exclusions cannot be overridden.
    #[error("semantic policy cannot override a hard eligibility constraint")]
    UnsafeEligibilityOverride,
    /// No exclusion has this id under the requested root.
    #[error("semantic exclusion {0} does not exist")]
    UnknownExclusion(ExclusionId),
    /// Exclusion already has a cleanup plan or is complete.
    #[error("semantic exclusion cleanup is already planned")]
    ExclusionCleanupAlreadyPlanned,
    /// Completion referenced a different exclusion deletion plan.
    #[error("semantic exclusion deletion plan does not match")]
    ExclusionPlanMismatch,
    /// Policy, catalog, state, or journal record name different libraries.
    #[error("semantic policy belongs to a different library")]
    LibraryMismatch,
    /// A durable revision of zero cannot be compared optimistically.
    #[error("semantic policy revision must be at least one")]
    InvalidRevision,
    /// The durable revision counter is exhausted.
    #[error("semantic policy revision overflowed")]
    RevisionOverflow,
}
