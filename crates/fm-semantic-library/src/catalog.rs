use std::collections::{BTreeMap, BTreeSet};

use fm_domain::{EntryId, Location, WorkspaceId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::hierarchy::is_same_or_descendant;
use crate::{
    AccessContext, AuthorizationError, BudgetKind, ConsentState, ConversationEvidencePin,
    ConversationPinId, DeletionPlanId, DerivedArtifactId, DocumentId, ExclusionDeletionPlan,
    LibraryId, ObservedRootIdentity, OccurrenceId, PolicyError, QuotaUsage, ResourceBudgets,
    RootId, RootMoveResolution, RootUnavailabilityReason, SemanticLibraryPolicy,
    SemanticLibraryState, SemanticQueryRequest, ServerPolicy,
};

/// Current semantic catalog JSON schema.
pub const CURRENT_CATALOG_SCHEMA_VERSION: u32 = 1;

/// Validated content fingerprint used for global document deduplication.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentFingerprint(String);

impl ContentFingerprint {
    /// Creates a non-empty opaque fingerprint.
    ///
    /// # Errors
    ///
    /// Rejects empty or whitespace-containing values.
    pub fn new(value: impl Into<String>) -> Result<Self, CatalogError> {
        let value = value.into();
        if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(CatalogError::InvalidContentFingerprint);
        }
        Ok(Self(value))
    }

    /// Returns the opaque fingerprint.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Content-derived records shared by all occurrences of one document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentArtifacts {
    /// Retained normalized excerpt records.
    pub extracted_content: BTreeSet<DerivedArtifactId>,
    /// Derived summary records.
    pub summaries: BTreeSet<DerivedArtifactId>,
    /// Accepted or generated label records.
    pub labels: BTreeSet<DerivedArtifactId>,
    /// Vector records.
    pub vectors: BTreeSet<DerivedArtifactId>,
}

impl DocumentArtifacts {
    fn merge(&mut self, other: Self) {
        self.extracted_content.extend(other.extracted_content);
        self.summaries.extend(other.summaries);
        self.labels.extend(other.labels);
        self.vectors.extend(other.vectors);
    }
}

/// Workspace/root authorization attached to a physical occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OccurrenceScope {
    workspace_id: WorkspaceId,
    root_id: RootId,
}

impl OccurrenceScope {
    /// Creates an occurrence scope.
    #[must_use]
    pub const fn new(workspace_id: WorkspaceId, root_id: RootId) -> Self {
        Self {
            workspace_id,
            root_id,
        }
    }

    /// Returns the workspace.
    #[must_use]
    pub const fn workspace_id(self) -> WorkspaceId {
        self.workspace_id
    }

    /// Returns the enrolled root.
    #[must_use]
    pub const fn root_id(self) -> RootId {
        self.root_id
    }
}

/// Authoritative byte consumption of one deduplicated document.
///
/// The host measures content once; the catalog then owns the numbers. Quota
/// enforcement never trusts a caller-supplied usage total.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMeasurement {
    source_bytes: u64,
    extracted_bytes: u64,
    vector_bytes: u64,
}

impl DocumentMeasurement {
    /// Records measured source, extracted-text, and vector bytes.
    #[must_use]
    pub const fn new(source_bytes: u64, extracted_bytes: u64, vector_bytes: u64) -> Self {
        Self {
            source_bytes,
            extracted_bytes,
            vector_bytes,
        }
    }

    /// Returns measured source bytes.
    #[must_use]
    pub const fn source_bytes(self) -> u64 {
        self.source_bytes
    }

    /// Returns measured normalized-excerpt bytes.
    #[must_use]
    pub const fn extracted_bytes(self) -> u64 {
        self.extracted_bytes
    }

    /// Returns measured vector bytes.
    #[must_use]
    pub const fn vector_bytes(self) -> u64 {
        self.vector_bytes
    }
}

/// Authoritative consumption computed from stored catalog records.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CatalogUsage {
    documents: u64,
    source_bytes: u64,
    extracted_bytes: u64,
    vector_bytes: u64,
}

impl CatalogUsage {
    /// Returns catalogued documents.
    #[must_use]
    pub const fn documents(self) -> u64 {
        self.documents
    }

    /// Returns represented source bytes.
    #[must_use]
    pub const fn source_bytes(self) -> u64 {
        self.source_bytes
    }

    /// Returns retained normalized-excerpt bytes.
    #[must_use]
    pub const fn extracted_bytes(self) -> u64 {
        self.extracted_bytes
    }

    /// Returns retained vector bytes.
    #[must_use]
    pub const fn vector_bytes(self) -> u64 {
        self.vector_bytes
    }

    /// Adds one document, refusing to wrap around a `u64` boundary.
    fn checked_add(self, measurement: DocumentMeasurement) -> Result<Self, CatalogError> {
        Ok(Self {
            documents: self
                .documents
                .checked_add(1)
                .ok_or(CatalogError::UsageOverflow)?,
            source_bytes: self
                .source_bytes
                .checked_add(measurement.source_bytes)
                .ok_or(CatalogError::UsageOverflow)?,
            extracted_bytes: self
                .extracted_bytes
                .checked_add(measurement.extracted_bytes)
                .ok_or(CatalogError::UsageOverflow)?,
            vector_bytes: self
                .vector_bytes
                .checked_add(measurement.vector_bytes)
                .ok_or(CatalogError::UsageOverflow)?,
        })
    }

    /// Adds one stored document, saturating rather than wrapping.
    ///
    /// Saturation is deliberately fail-safe: a saturated total exceeds every
    /// finite budget, so ingestion denies rather than admits.
    fn saturating_add(self, measurement: DocumentMeasurement) -> Self {
        Self {
            documents: self.documents.saturating_add(1),
            source_bytes: self.source_bytes.saturating_add(measurement.source_bytes),
            extracted_bytes: self
                .extracted_bytes
                .saturating_add(measurement.extracted_bytes),
            vector_bytes: self.vector_bytes.saturating_add(measurement.vector_bytes),
        }
    }
}

/// One provider enumeration result offered to the pure catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogObservation {
    entry_id: EntryId,
    location: Location,
    content_fingerprint: ContentFingerprint,
    scope: OccurrenceScope,
    artifacts: DocumentArtifacts,
    measurement: DocumentMeasurement,
}

impl CatalogObservation {
    /// Creates a provider-neutral observation with measured byte consumption.
    #[must_use]
    pub const fn new(
        entry_id: EntryId,
        location: Location,
        content_fingerprint: ContentFingerprint,
        scope: OccurrenceScope,
        artifacts: DocumentArtifacts,
        measurement: DocumentMeasurement,
    ) -> Self {
        Self {
            entry_id,
            location,
            content_fingerprint,
            scope,
            artifacts,
            measurement,
        }
    }
}

/// One deduplicated content document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentRecord {
    pub(crate) id: DocumentId,
    pub(crate) content_fingerprint: ContentFingerprint,
    pub(crate) artifacts: DocumentArtifacts,
    pub(crate) measurement: DocumentMeasurement,
}

impl DocumentRecord {
    /// Returns the document id.
    #[must_use]
    pub const fn id(&self) -> DocumentId {
        self.id
    }

    /// Returns content-derived artifacts.
    #[must_use]
    pub const fn artifacts(&self) -> &DocumentArtifacts {
        &self.artifacts
    }

    /// Returns authoritative measured byte consumption.
    #[must_use]
    pub const fn measurement(&self) -> DocumentMeasurement {
        self.measurement
    }
}

/// One physical/provider occurrence with all of its authorization scopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OccurrenceRecord {
    pub(crate) id: OccurrenceId,
    pub(crate) entry_id: EntryId,
    pub(crate) location: Location,
    pub(crate) document_id: DocumentId,
    pub(crate) scopes: Vec<OccurrenceScope>,
}

impl OccurrenceRecord {
    /// Returns the stable occurrence id.
    #[must_use]
    pub const fn id(&self) -> OccurrenceId {
        self.id
    }

    /// Returns the provider entry id.
    #[must_use]
    pub const fn entry_id(&self) -> EntryId {
        self.entry_id
    }

    /// Returns the host-only provider location.
    #[must_use]
    pub const fn location(&self) -> &Location {
        &self.location
    }

    /// Returns the deduplicated document id.
    #[must_use]
    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    /// Returns every workspace/root authorization scope.
    #[must_use]
    pub fn scopes(&self) -> &[OccurrenceScope] {
        &self.scopes
    }
}

/// Reconciliation availability of an enrolled root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RootAvailability {
    /// Provider root is currently reachable.
    #[default]
    Available,
    /// Provider root is temporarily unreachable; evidence remains usable.
    TemporarilyUnavailable {
        /// Sanitized, user-visible reason.
        reason: String,
    },
}

/// Whether a source link can currently be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceAvailability {
    /// At least one retained root occurrence is reachable.
    Available,
    /// Every retained root occurrence is temporarily unavailable.
    Unavailable,
}

/// Path-free authorized source returned by core query scoping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedSource {
    occurrence_id: OccurrenceId,
    matching_scopes: Vec<OccurrenceScope>,
    availability: SourceAvailability,
}

impl ScopedSource {
    /// Returns the opaque occurrence id.
    #[must_use]
    pub const fn occurrence_id(&self) -> OccurrenceId {
        self.occurrence_id
    }

    /// Returns only scopes authorized by the request.
    #[must_use]
    pub fn matching_scopes(&self) -> &[OccurrenceScope] {
        &self.matching_scopes
    }

    /// Returns source-link availability without exposing a path.
    #[must_use]
    pub const fn availability(&self) -> SourceAvailability {
        self.availability
    }
}

/// One deduplicated document visible inside an authorized query scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedDocument {
    document_id: DocumentId,
    sources: Vec<ScopedSource>,
}

impl ScopedDocument {
    /// Returns the deduplicated document id.
    #[must_use]
    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    /// Returns path-free authorized occurrences.
    #[must_use]
    pub fn sources(&self) -> &[ScopedSource] {
        &self.sources
    }
}

/// Authoritative device-local semantic catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticCatalog {
    pub(crate) schema_version: u32,
    pub(crate) library_id: LibraryId,
    pub(crate) documents: BTreeMap<DocumentId, DocumentRecord>,
    pub(crate) occurrences: BTreeMap<OccurrenceId, OccurrenceRecord>,
    #[serde(default)]
    pub(crate) root_availability: BTreeMap<RootId, RootAvailability>,
    #[serde(default)]
    pub(crate) reconciliation_generations: BTreeMap<RootId, u64>,
    #[serde(default)]
    pub(crate) conversation_pins: BTreeMap<ConversationPinId, ConversationEvidencePin>,
    #[serde(default)]
    pub(crate) deletion_plans: BTreeMap<DeletionPlanId, ExclusionDeletionPlan>,
}

impl SemanticCatalog {
    /// Creates an empty catalog for one library.
    #[must_use]
    pub fn new(library_id: LibraryId) -> Self {
        Self {
            schema_version: CURRENT_CATALOG_SCHEMA_VERSION,
            library_id,
            documents: BTreeMap::new(),
            occurrences: BTreeMap::new(),
            root_availability: BTreeMap::new(),
            reconciliation_generations: BTreeMap::new(),
            conversation_pins: BTreeMap::new(),
            deletion_plans: BTreeMap::new(),
        }
    }

    /// Returns the library id.
    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    /// Returns the number of deduplicated documents.
    #[must_use]
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    /// Returns the number of physical/provider occurrences.
    #[must_use]
    pub fn occurrence_count(&self) -> usize {
        self.occurrences.len()
    }

    /// Returns every physical/provider occurrence record.
    pub fn occurrences(&self) -> impl Iterator<Item = &OccurrenceRecord> {
        self.occurrences.values()
    }

    /// Reports whether an occurrence has at least one policy-proven scope
    /// whose enrolled root is currently reachable.
    ///
    /// # Errors
    ///
    /// Returns a provider-location traversal failure.
    pub(crate) fn feedable_scopes(
        &self,
        policy: &SemanticLibraryPolicy,
        occurrence: &OccurrenceRecord,
    ) -> Result<bool, CatalogError> {
        for scope in &occurrence.scopes {
            if self.root_availability(scope.root_id) == RootAvailability::Available
                && scope_is_proven(policy, &occurrence.location, *scope)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Returns a document record.
    #[must_use]
    pub fn document(&self, document_id: DocumentId) -> Option<&DocumentRecord> {
        self.documents.get(&document_id)
    }

    /// Finds an occurrence by provider and provider entry identity.
    #[must_use]
    pub fn occurrence_by_entry(
        &self,
        provider_id: &str,
        entry_id: EntryId,
    ) -> Option<&OccurrenceRecord> {
        self.occurrences.values().find(|occurrence| {
            occurrence.location.provider_id.as_str() == provider_id
                && occurrence.entry_id == entry_id
        })
    }

    /// Finds one exact path occurrence, including hardlink-distinct paths.
    #[must_use]
    pub fn occurrence_at(
        &self,
        entry_id: EntryId,
        location: &Location,
    ) -> Option<&OccurrenceRecord> {
        let id = occurrence_id(self.library_id, entry_id, location);
        self.occurrences.get(&id)
    }

    /// Merges observations in arbitrary order, deduplicating by content while
    /// preserving every distinct occurrence and scope.
    ///
    /// Ingestion is a single atomic check-and-apply under the single-writer
    /// contract: pause state, root availability, consent, scope proof, byte
    /// conflicts, and hard resource budgets are all evaluated against stored
    /// records before anything is written, and any failure leaves the catalog
    /// untouched.
    ///
    /// This entry point enforces the device-local budgets carried by the
    /// policy. A server host must use [`Self::ingest_for_tenant`], which
    /// additionally enforces administrator hard quotas.
    ///
    /// # Errors
    ///
    /// Rejects a library mismatch, paused ingestion, unavailable root,
    /// unconsented location, unknown scope, conflicting byte measurement, a
    /// location containing transient credential material, or a breach of the
    /// policy's hard resource budgets.
    pub fn upsert_observations(
        &mut self,
        policy: &SemanticLibraryPolicy,
        state: &SemanticLibraryState,
        observations: impl IntoIterator<Item = CatalogObservation>,
    ) -> Result<CatalogUsage, CatalogError> {
        let observations = self.prepare_ingestion(policy, state, observations)?;
        let projected = self.projected_usage(&observations)?;
        enforce_budgets(&policy.resource_profile().budgets, projected)?;
        Ok(self.apply_observations(observations))
    }

    /// Ingests on behalf of an authenticated tenant user, additionally
    /// enforcing administrator hard quotas measured from stored records.
    ///
    /// # Errors
    ///
    /// Returns every [`Self::upsert_observations`] failure plus a typed
    /// authorization or quota denial.
    pub fn ingest_for_tenant(
        &mut self,
        policy: &SemanticLibraryPolicy,
        state: &SemanticLibraryState,
        server_policy: &ServerPolicy,
        access: &AccessContext,
        observations: impl IntoIterator<Item = CatalogObservation>,
    ) -> Result<CatalogUsage, CatalogError> {
        if access.library_id != self.library_id {
            return Err(CatalogError::LibraryMismatch);
        }
        let library = server_policy.authorize(access)?;
        let observations = self.prepare_ingestion(policy, state, observations)?;
        let projected = self.projected_usage(&observations)?;
        enforce_budgets(&policy.resource_profile().budgets, projected)?;
        crate::authorization::enforce_quotas(
            library.quotas(),
            QuotaUsage::projected(policy, projected)?,
        )?;
        Ok(self.apply_observations(observations))
    }

    /// Returns authoritative consumption computed from stored records.
    #[must_use]
    pub fn measured_usage(&self) -> CatalogUsage {
        self.documents
            .values()
            .fold(CatalogUsage::default(), |usage, document| {
                usage.saturating_add(document.measurement)
            })
    }

    fn prepare_ingestion(
        &self,
        policy: &SemanticLibraryPolicy,
        state: &SemanticLibraryState,
        observations: impl IntoIterator<Item = CatalogObservation>,
    ) -> Result<Vec<CatalogObservation>, CatalogError> {
        self.ensure_library(policy, state)?;
        if state.is_paused() {
            return Err(CatalogError::IngestionPaused);
        }
        let budgets = &policy.resource_profile().budgets;
        let observations: Vec<_> = observations.into_iter().collect();
        let mut batch_fingerprints = BTreeMap::new();
        let mut batch_measurements: BTreeMap<&ContentFingerprint, DocumentMeasurement> =
            BTreeMap::new();
        for observation in &observations {
            validate_observation(policy, observation)?;
            if observation.measurement.source_bytes() > budgets.max_source_bytes_per_document {
                return Err(CatalogError::BudgetExceeded(
                    BudgetKind::SourceBytesPerDocument,
                ));
            }
            if self.root_availability(observation.scope.root_id) != RootAvailability::Available {
                return Err(CatalogError::RootUnavailable(observation.scope.root_id));
            }
            let id = occurrence_id(self.library_id, observation.entry_id, &observation.location);
            if batch_fingerprints
                .insert(id, &observation.content_fingerprint)
                .is_some_and(|existing| existing != &observation.content_fingerprint)
            {
                return Err(CatalogError::ConflictingObservation);
            }
            let document_id = document_id(self.library_id, &observation.content_fingerprint);
            if self
                .documents
                .get(&document_id)
                .is_some_and(|document| document.measurement != observation.measurement)
                || batch_measurements
                    .insert(&observation.content_fingerprint, observation.measurement)
                    .is_some_and(|existing| existing != observation.measurement)
            {
                return Err(CatalogError::ConflictingMeasurement);
            }
        }
        Ok(observations)
    }

    fn projected_usage(
        &self,
        observations: &[CatalogObservation],
    ) -> Result<CatalogUsage, CatalogError> {
        let mut projected = self.measured_usage();
        let mut counted = BTreeSet::new();
        for observation in observations {
            let document_id = document_id(self.library_id, &observation.content_fingerprint);
            if self.documents.contains_key(&document_id) || !counted.insert(document_id) {
                continue;
            }
            projected = projected.checked_add(observation.measurement)?;
        }
        Ok(projected)
    }

    fn apply_observations(&mut self, observations: Vec<CatalogObservation>) -> CatalogUsage {
        for observation in observations {
            let document_id = document_id(self.library_id, &observation.content_fingerprint);
            self.documents
                .entry(document_id)
                .and_modify(|document| document.artifacts.merge(observation.artifacts.clone()))
                .or_insert(DocumentRecord {
                    id: document_id,
                    content_fingerprint: observation.content_fingerprint,
                    artifacts: observation.artifacts,
                    measurement: observation.measurement,
                });
            let occurrence_id =
                occurrence_id(self.library_id, observation.entry_id, &observation.location);
            match self.occurrences.get_mut(&occurrence_id) {
                Some(occurrence) => {
                    occurrence.location = observation.location;
                    occurrence.document_id = document_id;
                    if !occurrence.scopes.contains(&observation.scope) {
                        occurrence.scopes.push(observation.scope);
                    }
                }
                None => {
                    self.occurrences.insert(
                        occurrence_id,
                        OccurrenceRecord {
                            id: occurrence_id,
                            entry_id: observation.entry_id,
                            location: observation.location,
                            document_id,
                            scopes: vec![observation.scope],
                        },
                    );
                }
            }
        }
        self.prune_unreferenced_documents();
        self.measured_usage()
    }

    pub(crate) fn ensure_library(
        &self,
        policy: &SemanticLibraryPolicy,
        state: &SemanticLibraryState,
    ) -> Result<(), CatalogError> {
        if policy.library().id() != self.library_id || state.library_id() != self.library_id {
            return Err(CatalogError::LibraryMismatch);
        }
        Ok(())
    }

    pub(crate) fn ensure_policy_library(
        &self,
        policy: &SemanticLibraryPolicy,
    ) -> Result<(), CatalogError> {
        policy
            .ensure_library(self.library_id)
            .map_err(|_| CatalogError::LibraryMismatch)
    }

    /// Removes authorization references for a deleted workspace without
    /// deleting roots, occurrences, documents, or derived content.
    pub fn remove_workspace_scopes(&mut self, workspace_id: WorkspaceId) {
        for occurrence in self.occurrences.values_mut() {
            occurrence
                .scopes
                .retain(|scope| scope.workspace_id != workspace_id);
        }
        self.conversation_pins
            .retain(|_, pin| pin.scope().workspace_id() != workspace_id);
    }

    /// Marks a root temporarily unavailable without changing consent or data.
    pub fn mark_root_unavailable(&mut self, root_id: RootId, reason: impl Into<String>) {
        self.root_availability.insert(
            root_id,
            RootAvailability::TemporarilyUnavailable {
                reason: reason.into(),
            },
        );
    }

    /// Records that a provider root is reachable again.
    ///
    /// Availability is observed, never assumed: ingestion, worker feeding, and
    /// reconciliation all refuse to run for a root that has not been seen.
    pub fn mark_root_available(&mut self, root_id: RootId) {
        self.root_availability
            .insert(root_id, RootAvailability::Available);
    }

    /// Returns availability independently from consent.
    #[must_use]
    pub fn root_availability(&self, root_id: RootId) -> RootAvailability {
        self.root_availability
            .get(&root_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Resolves a possible root move and updates only availability state when
    /// identity cannot prove the move.
    ///
    /// A proven move relocates the root's explicit exclusions and its
    /// catalogued occurrences together, so revoked subtrees stay revoked and
    /// retained evidence keeps a provable scope. If the relocated state cannot
    /// be proven against the policy the whole move is rolled back and the root
    /// requires confirmation instead.
    ///
    /// # Errors
    ///
    /// Returns a library, policy, root, or location validation failure.
    pub fn reconcile_root_location(
        &mut self,
        policy: &mut SemanticLibraryPolicy,
        root_id: RootId,
        observations: &[ObservedRootIdentity],
    ) -> Result<RootMoveResolution, CatalogError> {
        self.ensure_policy_library(policy)?;
        let previous_policy = policy.clone();
        let resolution = policy.resolve_root_location(root_id, observations)?;
        match &resolution {
            RootMoveResolution::Unchanged => {
                self.mark_root_available(root_id);
            }
            RootMoveResolution::ProvenMove { previous, current } => {
                match self.relocate_root_occurrences(policy, root_id, previous, current) {
                    Ok(()) => self.mark_root_available(root_id),
                    Err(_) => {
                        *policy = previous_policy;
                        self.mark_root_unavailable(
                            root_id,
                            "relocation would conflict with another enrolled root",
                        );
                        return Ok(RootMoveResolution::RetainedUnavailable {
                            reason: RootUnavailabilityReason::RelocationConflict,
                        });
                    }
                }
            }
            RootMoveResolution::RetainedUnavailable { reason } => {
                self.mark_root_unavailable(root_id, format!("{reason:?}"));
            }
        }
        Ok(resolution)
    }

    fn relocate_root_occurrences(
        &mut self,
        policy: &SemanticLibraryPolicy,
        root_id: RootId,
        previous: &Location,
        current: &Location,
    ) -> Result<(), CatalogError> {
        let mut relocated: BTreeMap<OccurrenceId, OccurrenceRecord> = BTreeMap::new();
        let mut remapped: BTreeMap<OccurrenceId, OccurrenceId> = BTreeMap::new();
        for (id, occurrence) in &self.occurrences {
            let moves = occurrence
                .scopes
                .iter()
                .any(|scope| scope.root_id == root_id)
                && is_same_or_descendant(previous, &occurrence.location)?;
            if !moves {
                relocated.insert(*id, occurrence.clone());
                continue;
            }
            let location = crate::hierarchy::rebase(previous, current, &occurrence.location)?;
            super::policy::validate_location(&location)?;
            let new_id = occurrence_id(self.library_id, occurrence.entry_id, &location);
            relocated.insert(
                new_id,
                OccurrenceRecord {
                    id: new_id,
                    entry_id: occurrence.entry_id,
                    location,
                    document_id: occurrence.document_id,
                    scopes: occurrence.scopes.clone(),
                },
            );
            remapped.insert(*id, new_id);
        }
        if relocated.len() != self.occurrences.len() {
            return Err(CatalogError::RelocationConflict);
        }
        proven_scopes_only(policy, &relocated)?;
        self.occurrences = relocated;
        for pin in self.conversation_pins.values_mut() {
            pin.relocate(&remapped);
        }
        for plan in self.deletion_plans.values_mut() {
            plan.relocate(&remapped);
        }
        Ok(())
    }

    /// Returns whether an occurrence currently has a reachable source root.
    #[must_use]
    pub fn source_availability(&self, occurrence_id: OccurrenceId) -> Option<SourceAvailability> {
        let occurrence = self.occurrences.get(&occurrence_id)?;
        Some(
            if occurrence.scopes.iter().any(|scope| {
                !matches!(
                    self.root_availability.get(&scope.root_id),
                    Some(RootAvailability::TemporarilyUnavailable { .. })
                )
            }) {
                SourceAvailability::Available
            } else {
                SourceAvailability::Unavailable
            },
        )
    }

    /// Returns the last successfully completed reconciliation generation.
    #[must_use]
    pub fn reconciliation_generation(&self, root_id: RootId) -> u64 {
        self.reconciliation_generations
            .get(&root_id)
            .copied()
            .unwrap_or(0)
    }

    /// Commits one complete enumeration and removes only occurrences absent
    /// from that successful generation.
    ///
    /// A reconciliation can only complete for a library that is ingesting and
    /// for a root that has been observed as reachable: absence is never
    /// deletion, and a paused or unavailable root must retain its evidence.
    ///
    /// # Errors
    ///
    /// Rejects a library mismatch, paused ingestion, unavailable or unknown
    /// roots, and generation overflow.
    pub fn complete_reconciliation(
        &mut self,
        policy: &SemanticLibraryPolicy,
        state: &SemanticLibraryState,
        root_id: RootId,
        observed_occurrences: &BTreeSet<OccurrenceId>,
    ) -> Result<u64, CatalogError> {
        self.ensure_library(policy, state)?;
        if state.is_paused() {
            return Err(CatalogError::IngestionPaused);
        }
        if policy.root(root_id).is_none() {
            return Err(CatalogError::UnknownRoot(root_id));
        }
        if self.root_availability(root_id) != RootAvailability::Available {
            return Err(CatalogError::RootUnavailable(root_id));
        }
        let missing: BTreeSet<_> = self
            .occurrences
            .iter()
            .filter(|(id, occurrence)| {
                !observed_occurrences.contains(id)
                    && occurrence
                        .scopes
                        .iter()
                        .any(|scope| scope.root_id == root_id)
            })
            .map(|(id, _)| *id)
            .collect();
        for occurrence_id in &missing {
            if let Some(occurrence) = self.occurrences.get_mut(occurrence_id) {
                occurrence.scopes.retain(|scope| scope.root_id != root_id);
            }
        }
        self.conversation_pins.retain(|_, pin| {
            !missing.contains(&pin.occurrence_id()) || pin.scope().root_id() != root_id
        });
        self.occurrences
            .retain(|id, occurrence| !missing.contains(id) || !occurrence.scopes.is_empty());
        self.prune_unreferenced_documents();
        self.root_availability
            .insert(root_id, RootAvailability::Available);
        let generation = self
            .reconciliation_generation(root_id)
            .checked_add(1)
            .ok_or(CatalogError::GenerationOverflow)?;
        self.reconciliation_generations.insert(root_id, generation);
        Ok(generation)
    }

    /// Returns only documents reachable through fully authorized occurrence
    /// scopes. No frontend-side filtering is required or trusted.
    ///
    /// # Errors
    ///
    /// Denies tenant, library, user, workspace, and root scope mismatches.
    pub fn query_documents(
        &self,
        policy: &SemanticLibraryPolicy,
        server_policy: &ServerPolicy,
        request: &SemanticQueryRequest,
    ) -> Result<Vec<ScopedDocument>, AuthorizationError> {
        server_policy.authorize(request.access())?;
        if request.access().library_id != self.library_id
            || policy.library().id() != self.library_id
            || request.root_ids().is_empty()
        {
            return Err(AuthorizationError::ScopeDenied);
        }
        for root_id in request.root_ids() {
            let root = policy
                .root(*root_id)
                .ok_or(AuthorizationError::ScopeDenied)?;
            if !root
                .workspace_references()
                .contains(&request.workspace_id())
            {
                return Err(AuthorizationError::ScopeDenied);
            }
        }

        let mut by_document: BTreeMap<DocumentId, Vec<ScopedSource>> = BTreeMap::new();
        for occurrence in self.occurrences.values() {
            let mut matching_scopes = Vec::new();
            for scope in &occurrence.scopes {
                if scope.workspace_id != request.workspace_id()
                    || !request.root_ids().contains(&scope.root_id)
                {
                    continue;
                }
                if scope_is_proven(policy, &occurrence.location, *scope)
                    .map_err(|_| AuthorizationError::ScopeDenied)?
                {
                    matching_scopes.push(*scope);
                }
            }
            if matching_scopes.is_empty()
                || matches!(
                    policy.consent_state(&occurrence.location),
                    Ok(ConsentState::Excluded { .. } | ConsentState::NotIncluded) | Err(_)
                )
            {
                continue;
            }
            let availability = self
                .source_availability(occurrence.id)
                .unwrap_or(SourceAvailability::Unavailable);
            by_document
                .entry(occurrence.document_id)
                .or_default()
                .push(ScopedSource {
                    occurrence_id: occurrence.id,
                    matching_scopes,
                    availability,
                });
        }
        Ok(by_document
            .into_iter()
            .map(|(document_id, sources)| ScopedDocument {
                document_id,
                sources,
            })
            .collect())
    }

    pub(crate) fn prune_unreferenced_documents(&mut self) {
        let referenced: BTreeSet<_> = self
            .occurrences
            .values()
            .map(|occurrence| occurrence.document_id)
            .collect();
        self.documents
            .retain(|document_id, _| referenced.contains(document_id));
    }

    /// Rejects a persisted or tampered catalog whose occurrence scopes are not
    /// provable against the exact enrolled root they claim.
    ///
    /// A forged scope naming a sibling root, a workspace that never referenced
    /// the root, or a location outside it is a hard failure rather than a
    /// silently trusted authorization record.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::UnprovenScope`] or a library mismatch.
    pub fn validate_scopes(&self, policy: &SemanticLibraryPolicy) -> Result<(), CatalogError> {
        self.ensure_policy_library(policy)?;
        proven_scopes_only(policy, &self.occurrences)
    }

    /// Drops occurrence scopes that the policy cannot prove and returns the
    /// affected occurrences, so a tampered catalog degrades to no access
    /// rather than to another root's access.
    ///
    /// # Errors
    ///
    /// Returns a library mismatch or provider-location failure.
    pub fn retain_proven_scopes(
        &mut self,
        policy: &SemanticLibraryPolicy,
    ) -> Result<BTreeSet<OccurrenceId>, CatalogError> {
        self.ensure_policy_library(policy)?;
        let mut unproven = BTreeSet::new();
        for (id, occurrence) in &mut self.occurrences {
            let mut proven = Vec::with_capacity(occurrence.scopes.len());
            for scope in &occurrence.scopes {
                if scope_is_proven(policy, &occurrence.location, *scope)? {
                    proven.push(*scope);
                } else {
                    unproven.insert(*id);
                }
            }
            occurrence.scopes = proven;
        }
        self.conversation_pins.retain(|_, pin| {
            self.occurrences
                .get(&pin.occurrence_id())
                .is_some_and(|occurrence| occurrence.scopes.contains(&pin.scope()))
        });
        Ok(unproven)
    }

    pub(crate) fn validate(&self) -> Result<(), CatalogError> {
        if self.schema_version != CURRENT_CATALOG_SCHEMA_VERSION {
            return Err(CatalogError::UnsupportedSchema(self.schema_version));
        }
        for (document_id, document) in &self.documents {
            if *document_id != document.id {
                return Err(CatalogError::InconsistentRecordId);
            }
        }
        for (occurrence_id, occurrence) in &self.occurrences {
            if *occurrence_id != occurrence.id
                || !self.documents.contains_key(&occurrence.document_id)
            {
                return Err(CatalogError::InconsistentRecordId);
            }
            super::policy::validate_location(&occurrence.location)?;
        }
        if self
            .conversation_pins
            .iter()
            .any(|(id, pin)| *id != pin.id())
            || self
                .deletion_plans
                .iter()
                .any(|(id, plan)| !plan.is_structurally_valid(*id))
        {
            return Err(CatalogError::InconsistentRecordId);
        }
        Ok(())
    }
}

fn validate_observation(
    policy: &SemanticLibraryPolicy,
    observation: &CatalogObservation,
) -> Result<(), CatalogError> {
    super::policy::validate_location(&observation.location)?;
    if policy.root(observation.scope.root_id).is_none() {
        return Err(CatalogError::UnknownRoot(observation.scope.root_id));
    }
    if !scope_is_proven(policy, &observation.location, observation.scope)? {
        return Err(CatalogError::UnprovenScope);
    }
    if matches!(
        policy.consent_state(&observation.location)?,
        ConsentState::Excluded { .. } | ConsentState::NotIncluded
    ) {
        return Err(CatalogError::ConsentDenied);
    }
    Ok(())
}

/// Reports whether the policy itself proves a persisted authorization scope:
/// the root exists, the workspace references it, and the location genuinely
/// lies within that exact root.
fn scope_is_proven(
    policy: &SemanticLibraryPolicy,
    location: &Location,
    scope: OccurrenceScope,
) -> Result<bool, CatalogError> {
    let Some(root) = policy.root(scope.root_id) else {
        return Ok(false);
    };
    if !root.workspace_references().contains(&scope.workspace_id) {
        return Ok(false);
    }
    Ok(*location == *root.location()
        || root.recursive() && is_same_or_descendant(root.location(), location)?)
}

fn proven_scopes_only(
    policy: &SemanticLibraryPolicy,
    occurrences: &BTreeMap<OccurrenceId, OccurrenceRecord>,
) -> Result<(), CatalogError> {
    for occurrence in occurrences.values() {
        for scope in &occurrence.scopes {
            if !scope_is_proven(policy, &occurrence.location, *scope)? {
                return Err(CatalogError::UnprovenScope);
            }
        }
    }
    Ok(())
}

fn enforce_budgets(budgets: &ResourceBudgets, projected: CatalogUsage) -> Result<(), CatalogError> {
    if projected.documents > budgets.max_documents {
        return Err(CatalogError::BudgetExceeded(BudgetKind::Documents));
    }
    if projected.source_bytes > budgets.max_total_source_bytes {
        return Err(CatalogError::BudgetExceeded(BudgetKind::SourceBytes));
    }
    if projected.extracted_bytes > budgets.max_total_extracted_bytes {
        return Err(CatalogError::BudgetExceeded(BudgetKind::ExtractedBytes));
    }
    if projected.vector_bytes > budgets.max_total_vector_bytes {
        return Err(CatalogError::BudgetExceeded(BudgetKind::VectorBytes));
    }
    Ok(())
}

fn document_id(library_id: LibraryId, fingerprint: &ContentFingerprint) -> DocumentId {
    DocumentId::from_uuid(Uuid::new_v5(
        &library_id.into_uuid(),
        fingerprint.as_str().as_bytes(),
    ))
}

fn occurrence_id(library_id: LibraryId, entry_id: EntryId, location: &Location) -> OccurrenceId {
    OccurrenceId::from_uuid(Uuid::new_v5(
        &library_id.into_uuid(),
        format!(
            "{}:{entry_id}:{}",
            location.provider_id.as_str(),
            location.uri
        )
        .as_bytes(),
    ))
}

/// Catalog validation failure.
#[derive(Debug, Error)]
pub enum CatalogError {
    /// Content fingerprint is empty or malformed.
    #[error("content fingerprint is invalid")]
    InvalidContentFingerprint,
    /// Catalog, policy, runtime state, or access context name different
    /// libraries.
    #[error("semantic catalog and policy library ids differ")]
    LibraryMismatch,
    /// An observation references an unknown root.
    #[error("semantic root {0} does not exist")]
    UnknownRoot(RootId),
    /// A workspace/root scope is not proven by the enrolled root it claims.
    #[error("occurrence scope is not proven by its enrolled root")]
    UnprovenScope,
    /// Effective consent excludes the observed location.
    #[error("catalog observation has no effective consent")]
    ConsentDenied,
    /// Ingestion is paused; indexed generations stay queryable.
    #[error("semantic ingestion is paused")]
    IngestionPaused,
    /// The root has not been observed as reachable.
    #[error("semantic root {0} is not currently available")]
    RootUnavailable(RootId),
    /// Policy or location validation failed.
    #[error(transparent)]
    Policy(#[from] PolicyError),
    /// Server authorization or hard quotas denied the mutation.
    #[error(transparent)]
    Authorization(#[from] AuthorizationError),
    /// Provider-aware path traversal failed.
    #[error("catalog location is invalid: {0}")]
    Location(#[from] fm_domain::LocationError),
    /// Successful reconciliation generation overflowed.
    #[error("semantic reconciliation generation overflowed")]
    GenerationOverflow,
    /// Measured usage overflowed `u64`.
    #[error("semantic catalog usage overflowed")]
    UsageOverflow,
    /// Persisted catalog schema is unsupported.
    #[error("unsupported semantic catalog schema version {0}")]
    UnsupportedSchema(u32),
    /// Persisted map keys or document references are inconsistent.
    #[error("semantic catalog record identity is inconsistent")]
    InconsistentRecordId,
    /// One batch assigned multiple content fingerprints to one occurrence.
    #[error("catalog batch contains conflicting observations for one occurrence")]
    ConflictingObservation,
    /// One content fingerprint was measured with different byte totals.
    #[error("catalog observation contradicts the recorded document measurement")]
    ConflictingMeasurement,
    /// Relocating a proven move would collide with an existing occurrence.
    #[error("relocating the enrolled root would collide with existing occurrences")]
    RelocationConflict,
    /// A stored hard resource budget would be exceeded.
    #[error("semantic resource budget {0:?} would be exceeded")]
    BudgetExceeded(BudgetKind),
}
