//! Host-neutral semantic-library enrolment and consent capability.
//!
//! This service is the only application-layer owner of the durable consent
//! policy, catalog, runtime state, optimistic revisions, and confirmation
//! tokens. Hosts expose its projections; neither transport is allowed to
//! mutate the core documents directly.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};

use fm_domain::{Location, WorkspaceId};
use fm_semantic_library as core;
use fm_semantic_worker::rag_retrieval::{
    RagRetrievalPolicy, RagRetrievalRequest, RagSourceRestriction,
};
use fm_semantic_worker::semantic_storage::QueryFilters;
use thiserror::Error;
use uuid::Uuid;

/// Principal controlling semantic-library policy in the current host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticLibraryAuthority {
    /// No library implementation was configured.
    Unavailable,
    /// The local desktop user controls the device-local library.
    DesktopManaged,
    /// A server administrator provisioned a read-only private library.
    AdministratorProvisioned,
    /// In-process deterministic mock behavior.
    DeterministicMock,
}

/// Explicit semantic-library operation advertised to clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemanticLibraryOperation {
    /// Read the complete safe policy/status projection.
    ViewStatus,
    /// Read effective consent for the active folder.
    ViewFolderStatus,
    /// Inspect a folder before granting durable consent.
    PreviewEnrolment,
    /// Confirm a live enrolment preview.
    Enrol,
    /// Create an authoritative destructive exclusion plan.
    PlanExclusion,
    /// Confirm an exclusion plan.
    ConfirmExclusion,
    /// Resume incomplete or failed cleanup.
    ResumeCleanup,
    /// Pause ingestion while retaining consent and data.
    Pause,
    /// Resume ingestion.
    Resume,
    /// Replace safe per-root eligibility overrides.
    UpdateEligibilityOverrides,
}

/// Authority and operations exposed by the active semantic-library service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticLibraryCapabilities {
    authority: SemanticLibraryAuthority,
    operations: Vec<SemanticLibraryOperation>,
}

impl SemanticLibraryCapabilities {
    /// Returns the authority that owns this capability.
    #[must_use]
    pub const fn authority(&self) -> SemanticLibraryAuthority {
        self.authority
    }

    /// Returns supported operations in stable presentation order.
    #[must_use]
    pub fn operations(&self) -> &[SemanticLibraryOperation] {
        &self.operations
    }

    /// Reports whether an operation is supported.
    #[must_use]
    pub fn supports(&self, operation: SemanticLibraryOperation) -> bool {
        self.operations.contains(&operation)
    }
}

/// Provider-neutral active-folder context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticFolderContext {
    /// Active workspace.
    pub workspace_id: WorkspaceId,
    /// Active provider-owned folder.
    pub location: Location,
}

impl SemanticFolderContext {
    /// Creates a folder context after the facade has verified it is active.
    #[must_use]
    pub const fn new(workspace_id: WorkspaceId, location: Location) -> Self {
        Self {
            workspace_id,
            location,
        }
    }
}

/// Effective folder consent state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticFolderConsent {
    /// This exact folder is an enrolled root.
    IncludedHere,
    /// Consent comes from a recursively enrolled ancestor.
    InheritedFromParent,
    /// An explicit exclusion revokes consent.
    Excluded,
    /// No root grants consent.
    NotIncluded,
}

/// Safe active-folder consent projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticFolderStatus {
    /// Effective state.
    pub consent: SemanticFolderConsent,
    /// Root granting or formerly granting consent.
    pub root_id: Option<String>,
    /// Most-specific exclusion, when excluded.
    pub exclusion_id: Option<String>,
    /// Whether the active workspace is attached to the granting root.
    pub workspace_referenced: bool,
    /// Whether the source root is currently reachable.
    pub source_available: bool,
    /// Sanitized unavailability explanation.
    pub unavailable_reason: Option<String>,
}

/// Completeness of a provider-owned estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticEstimateCompleteness {
    /// The provider completed its bounded enumeration.
    Estimated,
    /// Enumeration was bounded or some entries could not be inspected.
    Partial,
    /// No trustworthy estimate is currently available.
    Unavailable,
}

/// Stable provider identity discovered by the host, never sent to the worker or
/// exposed by transport projections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticRootIdentity {
    volume_id: String,
    file_id: String,
}

impl SemanticRootIdentity {
    /// Creates an opaque stable `(volume, file)` identity.
    #[must_use]
    pub fn new(volume_id: impl Into<String>, file_id: impl Into<String>) -> Self {
        Self {
            volume_id: volume_id.into(),
            file_id: file_id.into(),
        }
    }
}

/// Provider-owned estimate used to build an enrolment disclosure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticEnrolmentEstimate {
    /// Whether enumeration was complete, partial, or unavailable.
    pub completeness: SemanticEstimateCompleteness,
    /// Estimated eligible files, absent when unavailable.
    pub estimated_files: Option<u64>,
    /// Estimated source bytes, absent when unavailable.
    pub estimated_source_bytes: Option<u64>,
    /// Estimated normalized excerpt bytes, absent when unavailable.
    pub estimated_extracted_bytes: Option<u64>,
    /// Estimated vector bytes, absent when unavailable.
    pub estimated_vector_bytes: Option<u64>,
    /// Skipped entries grouped by stable policy reason.
    pub skipped_reason_counts: core::EligibilityReasonCounts,
    /// Model bytes still missing, absent when not knowable.
    pub missing_model_download_bytes: Option<u64>,
    /// Sanitized reason an estimate is unavailable.
    pub unavailable_reason: Option<String>,
    /// Provider identity retained only for consent tracking.
    pub filesystem_identity: Option<SemanticRootIdentity>,
}

impl SemanticEnrolmentEstimate {
    /// Creates an explicit unavailable result without invented numeric values.
    #[must_use]
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            completeness: SemanticEstimateCompleteness::Unavailable,
            estimated_files: None,
            estimated_source_bytes: None,
            estimated_extracted_bytes: None,
            estimated_vector_bytes: None,
            skipped_reason_counts: core::EligibilityReasonCounts::default(),
            missing_model_download_bytes: None,
            unavailable_reason: Some(reason.into()),
            filesystem_identity: None,
        }
    }
}

/// Injected provider-side estimation boundary.
///
/// Implementations enumerate through host VFS capabilities. The semantic
/// worker is deliberately absent from this interface and receives no paths.
pub trait SemanticEnrolmentEstimator: Send + Sync {
    /// Returns a bounded estimate or an explicit unavailable result.
    fn estimate(&self, location: &Location, recursive: bool) -> SemanticEnrolmentEstimate;
}

/// Estimator used until provider-neutral recursive enumeration lands.
pub struct UnavailableSemanticEnrolmentEstimator;

impl SemanticEnrolmentEstimator for UnavailableSemanticEnrolmentEstimator {
    fn estimate(&self, _location: &Location, _recursive: bool) -> SemanticEnrolmentEstimate {
        SemanticEnrolmentEstimate::unavailable(
            "A provider-neutral recursive estimate is not available for this source.",
        )
    }
}

/// Fixed deterministic estimator for tests and mock mode.
pub struct FixedSemanticEnrolmentEstimator {
    estimate: SemanticEnrolmentEstimate,
}

impl FixedSemanticEnrolmentEstimator {
    /// Creates a fixed estimator.
    #[must_use]
    pub const fn new(estimate: SemanticEnrolmentEstimate) -> Self {
        Self { estimate }
    }
}

impl SemanticEnrolmentEstimator for FixedSemanticEnrolmentEstimator {
    fn estimate(&self, _location: &Location, _recursive: bool) -> SemanticEnrolmentEstimate {
        self.estimate.clone()
    }
}

/// One skip reason and its observed/estimated count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticEligibilityReasonCount {
    /// Stable reason.
    pub reason: core::EligibilityReason,
    /// Count observed by the estimator or ingestion coordinator.
    pub count: u64,
}

/// One hard budget that a preview estimate appears to exceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticBudgetKind {
    /// Document count.
    Documents,
    /// Cumulative source bytes.
    SourceBytes,
    /// Cumulative normalized excerpt bytes.
    ExtractedBytes,
    /// Cumulative vector bytes.
    VectorBytes,
}

/// Transport-safe estimate projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticEnrolmentEstimateStatus {
    /// Completeness marker; numeric values are estimates, never exact promises.
    pub completeness: SemanticEstimateCompleteness,
    /// Estimated eligible file count.
    pub estimated_files: Option<u64>,
    /// Estimated source bytes.
    pub estimated_source_bytes: Option<u64>,
    /// Estimated normalized excerpt bytes retained locally.
    pub estimated_extracted_bytes: Option<u64>,
    /// Estimated vector storage.
    pub estimated_vector_bytes: Option<u64>,
    /// Estimated additional local bytes including a missing model.
    pub estimated_additional_local_bytes: Option<u64>,
    /// Estimated missing model download.
    pub missing_model_download_bytes: Option<u64>,
    /// Stable skipped-reason counts.
    pub skipped_reason_counts: Vec<SemanticEligibilityReasonCount>,
    /// Hard budgets the estimate appears to exceed.
    pub exceeded_budgets: Vec<SemanticBudgetKind>,
    /// Sanitized unavailability reason.
    pub unavailable_reason: Option<String>,
}

/// Confirmation-gated enrolment disclosure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticEnrolmentPreview {
    /// Opaque server-generated confirmation identity.
    pub confirmation_id: String,
    /// Policy revision this preview was built from.
    pub policy_revision: u64,
    /// Provider-neutral folder reviewed by the user.
    pub location: Location,
    /// Whether consent is recursive.
    pub recursive: bool,
    /// Bounded estimate projection.
    pub estimate: SemanticEnrolmentEstimateStatus,
    /// Explicit retention disclosure.
    pub normalized_excerpts_retained_locally: bool,
}

/// Root availability independent of consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticRootAvailability {
    /// Provider source is reachable.
    Available,
    /// Evidence remains queryable but source links cannot currently open.
    TemporarilyUnavailable {
        /// Sanitized provider diagnostic.
        reason: String,
    },
}

/// Cleanup lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticCleanupState {
    /// Consent is revoked and cleanup has not yet been planned.
    Pending,
    /// Cleanup has incomplete categories.
    Running,
    /// A category failed and can be resumed.
    Failed,
    /// Every mandatory category completed.
    Complete,
}

/// Stable destructive artifact category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticDeletionCategory {
    /// Provider occurrence records.
    Occurrences,
    /// Retained normalized excerpts.
    ExtractedContent,
    /// Derived summaries.
    Summaries,
    /// Derived labels.
    Labels,
    /// Vectors no longer referenced elsewhere.
    OrphanVectors,
    /// Saved-conversation evidence pins.
    ConversationEvidencePins,
}

/// Visible progress for one mandatory cleanup category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticDeletionCategoryStatus {
    /// Stable category.
    pub category: SemanticDeletionCategory,
    /// Authoritatively derived total.
    pub total_items: u64,
    /// Durably completed items.
    pub completed_items: u64,
    /// Whether this category committed.
    pub complete: bool,
    /// Sanitized last failure.
    pub last_error: Option<String>,
}

/// Complete cleanup projection for an exclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCleanupStatus {
    /// Opaque durable plan identity.
    pub plan_id: Option<String>,
    /// Derived overall state.
    pub status: SemanticCleanupState,
    /// Every mandatory artifact category.
    pub categories: Vec<SemanticDeletionCategoryStatus>,
}

/// One explicit exclusion beneath an enrolled root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticExclusionStatus {
    /// Stable exclusion identity.
    pub id: String,
    /// Provider-neutral excluded location.
    pub location: Location,
    /// Destructive cleanup status.
    pub cleanup: SemanticCleanupStatus,
}

/// One fixed eligibility override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticEligibilityOverrideStatus {
    /// Curated reason being overridden.
    pub reason: core::EligibilityReason,
    /// Include or exclude action.
    pub action: core::EligibilityOverride,
}

/// Safe status projection for one globally enrolled root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticRootStatus {
    /// Stable path-independent root identity.
    pub id: String,
    /// Provider-neutral location already visible in the file manager.
    pub location: Location,
    /// Whether descendants inherit consent.
    pub recursive: bool,
    /// Whether the provider supplied stable filesystem identity.
    pub stable_identity_verified: bool,
    /// Workspaces referring to this global consent.
    pub workspace_references: Vec<Uuid>,
    /// Fixed reason-based overrides; never arbitrary patterns.
    pub eligibility_overrides: Vec<SemanticEligibilityOverrideStatus>,
    /// Persisted attached vocabulary identifiers.
    pub attached_vocabulary_ids: Vec<String>,
    /// Last visible reason counts, when an estimator supplied them.
    pub eligibility_reason_counts: Vec<SemanticEligibilityReasonCount>,
    /// Current source reachability.
    pub availability: SemanticRootAvailability,
    /// Last complete catalog reconciliation.
    pub reconciliation_generation: u64,
    /// Last generation committed to runtime state.
    pub indexed_generation: u64,
    /// Explicit descendant/root exclusions.
    pub exclusions: Vec<SemanticExclusionStatus>,
}

/// Immutable model identity in the safe policy projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticLibraryModelIdentity {
    /// Curated or expert model identifier.
    pub model_id: String,
    /// Immutable upstream revision.
    pub revision: String,
    /// Embedding dimensions.
    pub dimensions: u32,
    /// Embedding-space identity.
    pub embedding_space: String,
}

/// Stable library and model identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticLibraryIdentity {
    /// Stable library UUID.
    pub library_id: String,
    /// Exact model identity.
    pub model: SemanticLibraryModelIdentity,
}

/// Safe resource-policy projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticResourceProfile {
    /// Named preset.
    pub kind: core::ResourceProfileKind,
    /// Enforced hard budgets.
    pub budgets: core::ResourceBudgets,
}

/// Complete safe status/policy projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticLibraryStatus {
    /// Whether a library is configured.
    pub available: bool,
    /// Optimistic mutation revision.
    pub revision: u64,
    /// Ingestion pause state.
    pub paused: bool,
    /// Stable library/model identity, when configured.
    pub library: Option<SemanticLibraryIdentity>,
    /// Resource policy, when configured.
    pub resource_profile: Option<SemanticResourceProfile>,
    /// Reconciliation cadence.
    pub reconciliation_interval_seconds: Option<u64>,
    /// Every globally enrolled root.
    pub roots: Vec<SemanticRootStatus>,
    /// Explicit privacy disclosure.
    pub normalized_excerpts_retained_locally: bool,
}

impl SemanticLibraryStatus {
    fn unavailable() -> Self {
        Self {
            available: false,
            revision: 0,
            paused: false,
            library: None,
            resource_profile: None,
            reconciliation_interval_seconds: None,
            roots: Vec::new(),
            normalized_excerpts_retained_locally: true,
        }
    }
}

/// Authoritative exclusion plan awaiting explicit confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticExclusionPlan {
    /// Opaque confirmation identity; deletion counts are never accepted back.
    pub confirmation_id: String,
    /// Policy revision the plan was calculated from.
    pub policy_revision: u64,
    /// Stable root whose consent will be narrowed.
    pub root_id: String,
    /// Provider-neutral excluded location.
    pub location: Location,
    /// Every mandatory category and its authoritative count.
    pub categories: Vec<SemanticDeletionCategoryStatus>,
}

/// Explicit roots and immutable identity required by a managed library.
#[derive(Debug, Clone)]
pub struct SemanticLibraryConfiguration {
    configuration_directory: PathBuf,
    semantic_data_root: PathBuf,
    library: core::DeviceLibraryIdentity,
    resource_profile: core::ResourceProfile,
}

impl SemanticLibraryConfiguration {
    /// Creates a managed configuration from explicit host-owned roots.
    #[must_use]
    pub fn new(
        configuration_directory: impl Into<PathBuf>,
        semantic_data_root: impl Into<PathBuf>,
        library: core::DeviceLibraryIdentity,
        resource_profile: core::ResourceProfile,
    ) -> Self {
        Self {
            configuration_directory: configuration_directory.into(),
            semantic_data_root: semantic_data_root.into(),
            library,
            resource_profile,
        }
    }

    /// Creates a balanced configuration without exposing core policy types to
    /// a host assembly crate.
    ///
    /// # Errors
    ///
    /// Rejects an incomplete or zero-dimensional model identity.
    pub fn balanced(
        configuration_directory: impl Into<PathBuf>,
        semantic_data_root: impl Into<PathBuf>,
        library_id: Uuid,
        model_id: impl Into<String>,
        model_revision: impl Into<String>,
        model_dimensions: u32,
        embedding_space: impl Into<String>,
    ) -> Result<Self, SemanticLibraryError> {
        let model =
            core::ModelIdentity::new(model_id, model_revision, model_dimensions, embedding_space)
                .map_err(map_policy_error)?;
        Ok(Self::new(
            configuration_directory,
            semantic_data_root,
            core::DeviceLibraryIdentity::new(core::LibraryId::from_uuid(library_id), model),
            core::ResourceProfile {
                kind: core::ResourceProfileKind::Balanced,
                budgets: core::ResourceBudgets::default(),
            },
        ))
    }
}

/// Server tenant/user identity fixed by trusted host state, never by request
/// headers or request bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticServerIdentity {
    tenant_id: core::TenantId,
    user_id: core::UserId,
}

impl SemanticServerIdentity {
    /// Creates validated opaque server identifiers.
    ///
    /// # Errors
    ///
    /// Rejects empty or structurally unsafe values.
    pub fn new(
        tenant_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Result<Self, SemanticLibraryError> {
        Ok(Self {
            tenant_id: core::TenantId::new(tenant_id.into())
                .map_err(|_| SemanticLibraryError::InvalidRequest)?,
            user_id: core::UserId::new(user_id.into())
                .map_err(|_| SemanticLibraryError::InvalidRequest)?,
        })
    }
}

/// Caller authority for exactly one semantic-library call.
///
/// The authority travels with the call instead of being baked into the service
/// instance, so one process — and one router over one service — can serve an
/// administrator and a denied principal without the service itself deciding
/// who the caller is. Hosts construct this from trusted state: a desktop
/// process is its own user, and a server derives its principal from
/// authenticated session state, never from request JSON or headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticAccessContext {
    /// The local desktop/host user drives a device-local library.
    Host,
    /// An authenticated server principal.
    Server(SemanticServerIdentity),
    /// No authenticated principal; every operation is denied.
    Anonymous,
}

impl SemanticAccessContext {
    /// Creates a server access context from trusted, already-authenticated
    /// tenant and user identifiers.
    ///
    /// # Errors
    ///
    /// Rejects empty or structurally unsafe values.
    pub fn server(
        tenant_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Result<Self, SemanticLibraryError> {
        Ok(Self::Server(SemanticServerIdentity::new(
            tenant_id, user_id,
        )?))
    }

    pub(crate) fn tenant_id(&self) -> Result<String, SemanticLibraryError> {
        match self {
            Self::Host => Ok(DEVICE_LOCAL_TENANT_ID.to_owned()),
            Self::Server(identity) => Ok(identity.tenant_id.to_string()),
            Self::Anonymous => Err(SemanticLibraryError::InvalidRequest),
        }
    }
}

/// Visible failure of one destructive cleanup category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCleanupFailure {
    message: String,
}

impl SemanticCleanupFailure {
    /// Creates a sanitized category failure.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the sanitized message persisted with the plan.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Opaque tenant identity of the single device-local desktop library.
///
/// The desktop host has no tenant administration: one device, one library, one
/// tenant. Naming it explicitly keeps the worker protocol's tenant field
/// meaningful without letting a caller choose the value.
const DEVICE_LOCAL_TENANT_ID: &str = "device-local";

/// Number of planned items one external cleanup request may cover.
///
/// Progress is checkpointed durably after every batch, so an interrupted run
/// resumes at most this many items back — and always with the same idempotency
/// key, because the key is derived from the durable checkpoint.
const CLEANUP_BATCH_ITEMS: u64 = 64;

/// One resumable unit of external deletion work.
///
/// The identity is deliberately opaque and derived only from durable state:
/// the plan, the category, and the durably checkpointed item offset the batch
/// starts at. A retry after a crash or a failure therefore recomputes exactly
/// the same [`Self::idempotency_key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCleanupRequest {
    plan_id: String,
    category: SemanticDeletionCategory,
    first_item: u64,
    item_count: u64,
    idempotency_key: String,
}

impl SemanticCleanupRequest {
    fn new(
        plan_id: &str,
        category: SemanticDeletionCategory,
        first_item: u64,
        item_count: u64,
    ) -> Self {
        Self {
            plan_id: plan_id.to_owned(),
            category,
            first_item,
            item_count,
            idempotency_key: format!("{plan_id}:{}:{first_item}", category_key(category)),
        }
    }

    /// Returns the durable cleanup plan identity.
    #[must_use]
    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    /// Returns the mandatory category being deleted.
    #[must_use]
    pub const fn category(&self) -> SemanticDeletionCategory {
        self.category
    }

    /// Returns the durably checkpointed item offset this batch starts at.
    #[must_use]
    pub const fn first_item(&self) -> u64 {
        self.first_item
    }

    /// Returns how many planned items the batch covers; zero when the category
    /// had nothing to delete.
    #[must_use]
    pub const fn item_count(&self) -> u64 {
        self.item_count
    }

    /// Returns the stable opaque key identifying this exact batch.
    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
}

/// Boundary that performs the out-of-catalog part of one deletion category.
///
/// The catalog mutation itself is always performed by the core engine under
/// the durable plan, and is inherently idempotent: it removes set members and
/// re-removing an absent member is a no-op. This trait exists because later
/// tasks delete real artifacts — extracted text, vectors, summaries — from the
/// semantic data root, and those deletions can fail independently per
/// category. Failure must be durable and resumable, never a silent discard of
/// the whole plan.
///
/// # Contract
///
/// The engine guarantees exactly-once *effect*, not exactly-once *invocation*:
/// a batch is checkpointed only after its external deletion returned, so a
/// crash between the deletion and the checkpoint — or a retry of a failed
/// plan — invokes [`Self::execute`] again with the identical
/// [`SemanticCleanupRequest::idempotency_key`]. Implementations must therefore
/// treat a repeated key as already done and succeed, never double-delete,
/// double-bill, or fail.
pub trait SemanticCleanupExecutor: Send + Sync {
    /// Performs the external deletion for one idempotent batch.
    ///
    /// # Errors
    ///
    /// Returns a sanitized failure that is persisted against the plan.
    fn execute(&self, request: &SemanticCleanupRequest) -> Result<(), SemanticCleanupFailure>;
}

/// Default executor: the authoritative catalog mutation *is* the deletion.
///
/// Version one keeps every derived artifact inside the catalog document, so
/// there is nothing else to remove, and repeating a batch is trivially safe.
/// The category-by-category durable protocol around it is real and already
/// exercised, which is what makes replacing this executor in task 0180/0181 a
/// drop-in change.
pub struct CatalogSemanticCleanupExecutor;

impl SemanticCleanupExecutor for CatalogSemanticCleanupExecutor {
    fn execute(&self, _request: &SemanticCleanupRequest) -> Result<(), SemanticCleanupFailure> {
        Ok(())
    }
}

/// Observer consulted before every durable commit step.
///
/// The default implementation permits every step. A host — or a durability
/// test — can refuse a specific step to reproduce a crash or a filesystem
/// failure at exactly that point without a second, fake persistence path.
pub trait SemanticCommitObserver: Send + Sync {
    /// Returns whether the step may run.
    ///
    /// # Errors
    ///
    /// Returning an error abandons the transaction exactly as an interrupted
    /// process would, leaving the journal for deterministic recovery.
    fn before_step(&self, step: core::CommitStep) -> Result<(), SemanticCleanupFailure>;
}

/// Observer that permits every commit step.
pub struct PermissiveSemanticCommitObserver;

impl SemanticCommitObserver for PermissiveSemanticCommitObserver {
    fn before_step(&self, _step: core::CommitStep) -> Result<(), SemanticCleanupFailure> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LibraryData {
    policy: core::SemanticLibraryPolicy,
    catalog: core::SemanticCatalog,
    state: core::SemanticLibraryState,
}

#[derive(Debug, Clone)]
struct PendingEnrolment {
    revision: u64,
    context: SemanticFolderContext,
    root: core::EnrolledRoot,
    estimate: SemanticEnrolmentEstimate,
}

#[derive(Debug, Clone)]
struct PendingExclusion {
    revision: u64,
    context: SemanticFolderContext,
    root_id: core::RootId,
    exclusion_id: core::ExclusionId,
}

/// In-memory projection of durable state.
///
/// Every field here is a cache of something authoritative on disk, so the
/// whole struct can be discarded whenever durable state might disagree.
#[derive(Debug)]
struct LibraryCache {
    data: Option<LibraryData>,
    /// Durable policy revision observed when `data` was loaded, or `None` when
    /// no policy had been written yet.
    durable_revision: Option<u64>,
    next_id: u128,
    enrolment_confirmations: BTreeMap<String, PendingEnrolment>,
    exclusion_confirmations: BTreeMap<String, PendingExclusion>,
    /// Workspaces whose deletion already succeeded in the authoritative
    /// workspace repository but whose semantic reference cleanup has not
    /// committed yet.
    ///
    /// This is an obligation, not a cache, so [`Self::invalidate`] keeps it.
    pending_workspace_detachments: HashSet<WorkspaceId>,
}

impl LibraryCache {
    /// Drops every cached projection and every outstanding confirmation.
    ///
    /// Called whenever persistence failed or an external writer moved the
    /// durable revision. Keeping a confirmation across either would let a
    /// snapshot taken before the change resurrect consent the durable state no
    /// longer grants.
    fn invalidate(&mut self) {
        self.data = None;
        self.durable_revision = None;
        self.enrolment_confirmations.clear();
        self.exclusion_confirmations.clear();
    }

    fn clear_confirmations(&mut self) {
        self.enrolment_confirmations.clear();
        self.exclusion_confirmations.clear();
    }
}

/// Server access-control list provisioned with the library.
///
/// This is the policy, not the caller: the caller arrives per call as a
/// [`SemanticAccessContext`].
struct ServerAccess {
    policy: core::ServerPolicy,
    library_id: core::LibraryId,
}

struct ManagedSemanticLibrary {
    authority: SemanticLibraryAuthority,
    read_only: bool,
    deterministic: bool,
    coordinator: Option<core::SemanticLibraryCoordinator>,
    initial_policy: core::SemanticLibraryPolicy,
    estimator: Arc<dyn SemanticEnrolmentEstimator>,
    cleanup: Arc<dyn SemanticCleanupExecutor>,
    commit_observer: Arc<dyn SemanticCommitObserver>,
    server_access: Option<ServerAccess>,
    cache: Mutex<LibraryCache>,
}

enum SemanticLibraryBackend {
    Unavailable { authority: SemanticLibraryAuthority },
    Managed(Arc<ManagedSemanticLibrary>),
}

/// Deep application capability owning semantic consent and catalog mutations.
pub struct SemanticLibraryService {
    backend: SemanticLibraryBackend,
}

impl SemanticLibraryService {
    /// Creates an inert capability that does no filesystem or network work.
    #[must_use]
    pub const fn unavailable() -> Self {
        Self {
            backend: SemanticLibraryBackend::Unavailable {
                authority: SemanticLibraryAuthority::Unavailable,
            },
        }
    }

    /// Creates an inert administrator-provisioned status capability.
    #[must_use]
    pub const fn administrator_provisioned_unconfigured() -> Self {
        Self {
            backend: SemanticLibraryBackend::Unavailable {
                authority: SemanticLibraryAuthority::AdministratorProvisioned,
            },
        }
    }

    /// Creates an explicitly rooted desktop-managed library.
    ///
    /// Construction is inert: no directories or policy files are created until
    /// the first successful mutation.
    ///
    /// # Errors
    ///
    /// Rejects an invalid model identity or resource profile.
    pub fn desktop_managed(
        configuration: SemanticLibraryConfiguration,
        estimator: Arc<dyn SemanticEnrolmentEstimator>,
    ) -> Result<Self, SemanticLibraryError> {
        Ok(Self {
            backend: SemanticLibraryBackend::Managed(Arc::new(build_managed(
                configuration,
                SemanticLibraryAuthority::DesktopManaged,
                false,
                false,
                estimator,
                None,
            )?)),
        })
    }

    /// Creates a read-only, single-private server library owned by one
    /// administrator.
    ///
    /// The administrator identity is the library's access-control list, not the
    /// caller: every call still supplies its own [`SemanticAccessContext`].
    ///
    /// # Errors
    ///
    /// Rejects an invalid model identity or resource profile.
    pub fn server_single_private(
        configuration: SemanticLibraryConfiguration,
        administrator: SemanticServerIdentity,
        enrolment_policy: core::ServerEnrolmentPolicy,
        quotas: core::HardQuotas,
        estimator: Arc<dyn SemanticEnrolmentEstimator>,
    ) -> Result<Self, SemanticLibraryError> {
        Ok(Self {
            backend: SemanticLibraryBackend::Managed(Arc::new(build_managed(
                configuration,
                SemanticLibraryAuthority::AdministratorProvisioned,
                true,
                false,
                estimator,
                Some((administrator, enrolment_policy, quotas)),
            )?)),
        })
    }

    /// Creates the initial read-only server posture with local-only enrolment
    /// policy and default hard quotas.
    ///
    /// # Errors
    ///
    /// Rejects an invalid model identity or resource profile.
    pub fn server_single_private_read_only(
        configuration: SemanticLibraryConfiguration,
        administrator: SemanticServerIdentity,
        estimator: Arc<dyn SemanticEnrolmentEstimator>,
    ) -> Result<Self, SemanticLibraryError> {
        Self::server_single_private(
            configuration,
            administrator,
            core::ServerEnrolmentPolicy::local_only(),
            core::HardQuotas::default(),
            estimator,
        )
    }

    /// Creates the deterministic no-filesystem mock state machine.
    #[must_use]
    pub fn deterministic_mock() -> Self {
        let library = core::DeviceLibraryIdentity::new(
            core::LibraryId::from_uuid(Uuid::from_u128(0x179)),
            core::ModelIdentity::new(
                "mock-semantic-model",
                "mock-revision-1",
                384,
                "mock-embedding-space",
            )
            .expect("static model identity"),
        );
        let configuration = SemanticLibraryConfiguration::new(
            PathBuf::from("mock/config"),
            PathBuf::from("mock/semantic-data"),
            library,
            core::ResourceProfile {
                kind: core::ResourceProfileKind::Balanced,
                budgets: core::ResourceBudgets::default(),
            },
        );
        let estimate = SemanticEnrolmentEstimate {
            completeness: SemanticEstimateCompleteness::Partial,
            estimated_files: Some(42),
            estimated_source_bytes: Some(4_200),
            estimated_extracted_bytes: Some(1_200),
            estimated_vector_bytes: Some(800),
            skipped_reason_counts: core::EligibilityReasonCounts::from_decisions([
                core::EligibilityDecision::Skipped([core::EligibilityReason::Hidden].into()),
                core::EligibilityDecision::Skipped(
                    [core::EligibilityReason::UnsupportedMime].into(),
                ),
            ]),
            missing_model_download_bytes: Some(500),
            unavailable_reason: None,
            filesystem_identity: Some(SemanticRootIdentity::new("mock-volume", "mock-folder")),
        };
        Self {
            backend: SemanticLibraryBackend::Managed(Arc::new(
                build_managed(
                    configuration,
                    SemanticLibraryAuthority::DeterministicMock,
                    false,
                    true,
                    Arc::new(FixedSemanticEnrolmentEstimator::new(estimate)),
                    None,
                )
                .expect("static mock policy"),
            )),
        }
    }

    /// Replaces the cleanup executor that advances destructive categories.
    #[must_use]
    pub fn with_cleanup_executor(mut self, executor: Arc<dyn SemanticCleanupExecutor>) -> Self {
        if let SemanticLibraryBackend::Managed(managed) = &mut self.backend
            && let Some(managed) = Arc::get_mut(managed)
        {
            managed.cleanup = executor;
        }
        self
    }

    /// Replaces the observer consulted before every durable commit step.
    #[must_use]
    pub fn with_commit_observer(mut self, observer: Arc<dyn SemanticCommitObserver>) -> Self {
        if let SemanticLibraryBackend::Managed(managed) = &mut self.backend
            && let Some(managed) = Arc::get_mut(managed)
        {
            managed.commit_observer = observer;
        }
        self
    }

    /// Reports authority and the operations *this caller* may perform, without
    /// touching storage.
    ///
    /// The caller matters: one process serves one router over one service, so
    /// advertising the administrator's operation list to every principal would
    /// tell a denied server user that enrolment and destructive exclusion are
    /// available and only fail them at the next call. A principal the library's
    /// access-control list rejects therefore receives no operations at all.
    #[must_use]
    pub fn capabilities(&self, access: &SemanticAccessContext) -> SemanticLibraryCapabilities {
        match self.resolved_backend() {
            None => SemanticLibraryCapabilities {
                authority: self.unresolved_authority(),
                // An unconfigured administrator-provisioned server still
                // reports its status surface, but only to a principal that an
                // eventual library would accept: an anonymous caller is not
                // one.
                operations: if self.unresolved_authority()
                    == SemanticLibraryAuthority::AdministratorProvisioned
                    && matches!(access, SemanticAccessContext::Server(_))
                {
                    vec![
                        SemanticLibraryOperation::ViewStatus,
                        SemanticLibraryOperation::ViewFolderStatus,
                    ]
                } else {
                    Vec::new()
                },
            },
            Some(managed) => SemanticLibraryCapabilities {
                authority: managed.authority,
                operations: if managed.authorize(access).is_err() {
                    Vec::new()
                } else if managed.read_only {
                    vec![
                        SemanticLibraryOperation::ViewStatus,
                        SemanticLibraryOperation::ViewFolderStatus,
                    ]
                } else {
                    vec![
                        SemanticLibraryOperation::ViewStatus,
                        SemanticLibraryOperation::ViewFolderStatus,
                        SemanticLibraryOperation::PreviewEnrolment,
                        SemanticLibraryOperation::Enrol,
                        SemanticLibraryOperation::PlanExclusion,
                        SemanticLibraryOperation::ConfirmExclusion,
                        SemanticLibraryOperation::ResumeCleanup,
                        SemanticLibraryOperation::Pause,
                        SemanticLibraryOperation::Resume,
                        SemanticLibraryOperation::UpdateEligibilityOverrides,
                    ]
                },
            },
        }
    }

    /// Fails before any request location or storage is used when an operation
    /// is unavailable for the active authority and caller.
    ///
    /// # Errors
    ///
    /// Returns [`SemanticLibraryError::AuthorityDenied`].
    pub fn ensure_operation_allowed(
        &self,
        access: &SemanticAccessContext,
        operation: SemanticLibraryOperation,
    ) -> Result<(), SemanticLibraryError> {
        let capabilities = self.capabilities(access);
        if capabilities.supports(operation) {
            Ok(())
        } else {
            Err(SemanticLibraryError::AuthorityDenied {
                authority: capabilities.authority(),
                operation,
            })
        }
    }

    /// Returns a safe policy/catalog/state projection.
    ///
    /// # Errors
    ///
    /// Returns an authorization, lock, or persistence failure.
    pub fn status(
        &self,
        access: &SemanticAccessContext,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        let Some(managed) = self.resolved_backend() else {
            return Ok(SemanticLibraryStatus::unavailable());
        };
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let data = locked.data()?.clone();
        Ok(project_status(&data))
    }

    /// Returns effective consent and source availability for an active folder.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, lock, or persistence failure.
    pub fn folder_status(
        &self,
        access: &SemanticAccessContext,
        context: &SemanticFolderContext,
    ) -> Result<SemanticFolderStatus, SemanticLibraryError> {
        self.ensure_operation_allowed(access, SemanticLibraryOperation::ViewFolderStatus)?;
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let data = locked.data()?;
        let consent = data
            .policy
            .consent_state(&context.location)
            .map_err(map_policy_error)?;
        Ok(project_folder_status(
            consent,
            &data.policy,
            &data.catalog,
            context.workspace_id,
        ))
    }

    /// Creates a bounded disclosure and opaque confirmation without changing
    /// durable consent.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, request-validation, lock, or
    /// persistence failure.
    pub fn preview_enrolment(
        &self,
        access: &SemanticAccessContext,
        context: SemanticFolderContext,
        recursive: bool,
    ) -> Result<SemanticEnrolmentPreview, SemanticLibraryError> {
        self.ensure_operation_allowed(access, SemanticLibraryOperation::PreviewEnrolment)?;
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        if context.location.provider_id.as_str() != "local" {
            return Err(SemanticLibraryError::InvalidRequest);
        }
        let mut locked = managed.lock()?;
        let (existing_root, revision, budgets) = {
            let data = locked.data()?;
            data.policy
                .consent_state(&context.location)
                .map_err(map_policy_error)?;
            (
                data.policy
                    .roots()
                    .values()
                    .find(|root| root.location() == &context.location)
                    .cloned(),
                data.policy.revision(),
                data.policy.resource_profile().budgets,
            )
        };
        let deterministic = managed.deterministic;
        let root_id = match existing_root.as_ref() {
            Some(root) => root.id(),
            None => next_root_id(&mut locked.cache, deterministic),
        };
        let estimate = managed.estimator.estimate(&context.location, recursive);
        let filesystem_identity = estimate
            .filesystem_identity
            .as_ref()
            .map(|identity| {
                core::FilesystemIdentity::new(&identity.volume_id, &identity.file_id)
                    .map_err(map_policy_error)
            })
            .transpose()?;
        let mut root = existing_root.unwrap_or_else(|| {
            core::EnrolledRoot::new(
                root_id,
                context.location.clone(),
                filesystem_identity,
                recursive,
            )
        });
        root.attach_workspace(context.workspace_id);
        let confirmation_id = next_confirmation(&mut locked.cache, deterministic, "enrol");
        locked.cache.enrolment_confirmations.insert(
            confirmation_id.clone(),
            PendingEnrolment {
                revision,
                context: context.clone(),
                root,
                estimate: estimate.clone(),
            },
        );
        Ok(SemanticEnrolmentPreview {
            confirmation_id,
            policy_revision: revision,
            location: context.location,
            recursive,
            estimate: project_estimate(&estimate, budgets),
            normalized_excerpts_retained_locally: true,
        })
    }

    /// Confirms one live enrolment preview.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, stale-revision, stale-confirmation,
    /// lock, or persistence failure.
    pub fn confirm_enrolment(
        &self,
        access: &SemanticAccessContext,
        confirmation_id: &str,
        expected_revision: u64,
        context: &SemanticFolderContext,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        self.ensure_operation_allowed(access, SemanticLibraryOperation::Enrol)?;
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let mut next = locked.checked_data(expected_revision)?.clone();
        let pending = locked
            .cache
            .enrolment_confirmations
            .get(confirmation_id)
            .cloned()
            .ok_or(SemanticLibraryError::StaleConfirmation)?;
        if pending.revision != expected_revision || pending.context != *context {
            return Err(SemanticLibraryError::StaleConfirmation);
        }
        let root_id = if let Some(existing) = next
            .policy
            .roots()
            .values()
            .find(|root| root.location() == &context.location)
            .map(core::EnrolledRoot::id)
        {
            next.policy
                .root_mut(existing)
                .ok_or(SemanticLibraryError::NotFound)?
                .attach_workspace(context.workspace_id);
            existing
        } else {
            let root_id = pending.root.id();
            next.policy
                .enrol_root(pending.root)
                .map_err(map_policy_error)?;
            // A root the user just reviewed through a live preview is
            // reachable by construction; an already-enrolled root keeps
            // whatever availability its last identity proof established.
            next.catalog.mark_root_available(root_id);
            root_id
        };
        // The skip reasons are part of the disclosure the user consented to,
        // so they are written inside the same durable transaction rather than
        // cached in memory and lost on restart.
        next.state
            .set_eligibility_reason_counts(root_id, pending.estimate.skipped_reason_counts.clone());
        locked.commit(next, core::LibraryOperation::Enrolment)?;
        locked.status()
    }

    /// Creates an authoritative destructive exclusion inventory.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, consent-state, stale-revision,
    /// lock, or persistence failure.
    pub fn plan_exclusion(
        &self,
        access: &SemanticAccessContext,
        context: SemanticFolderContext,
        expected_revision: u64,
    ) -> Result<SemanticExclusionPlan, SemanticLibraryError> {
        self.ensure_operation_allowed(access, SemanticLibraryOperation::PlanExclusion)?;
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let data = locked.checked_data(expected_revision)?;
        let root_id = match data
            .policy
            .consent_state(&context.location)
            .map_err(map_policy_error)?
        {
            core::ConsentState::IncludedHere { root_id }
            | core::ConsentState::InheritedFromParent { root_id } => root_id,
            core::ConsentState::Excluded { .. } => {
                return Err(SemanticLibraryError::AlreadyExcluded);
            }
            core::ConsentState::NotIncluded => return Err(SemanticLibraryError::NotEnrolled),
        };
        if !data
            .policy
            .root(root_id)
            .is_some_and(|root| root.workspace_references().contains(&context.workspace_id))
        {
            return Err(SemanticLibraryError::WorkspaceRequired);
        }
        let mut candidate = data.clone();
        let deterministic = managed.deterministic;
        let exclusion_id = next_exclusion_id(&mut locked.cache, deterministic);
        apply_exclusion(&mut candidate, root_id, exclusion_id, &context.location)?;
        let plan_id = candidate
            .catalog
            .begin_exclusion_cleanup(&mut candidate.policy, root_id, exclusion_id)
            .map_err(map_deletion_error)?;
        let categories = cleanup_categories(
            candidate
                .catalog
                .deletion_plan(plan_id)
                .ok_or(SemanticLibraryError::NotFound)?,
        );
        let confirmation_id = next_confirmation(&mut locked.cache, deterministic, "exclude");
        locked.cache.exclusion_confirmations.insert(
            confirmation_id.clone(),
            PendingExclusion {
                revision: expected_revision,
                context: context.clone(),
                root_id,
                exclusion_id,
            },
        );
        Ok(SemanticExclusionPlan {
            confirmation_id,
            policy_revision: expected_revision,
            root_id: root_id.to_string(),
            location: context.location,
            categories,
        })
    }

    /// Confirms an exclusion, durably revoking consent and persisting its
    /// authoritative plan before any deletion runs, then advances the plan
    /// category by category.
    ///
    /// Consent revocation and the plan commit together, so a failure while
    /// deleting leaves the scope revoked and the remaining work resumable
    /// instead of discarding either.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, stale-revision, stale-confirmation,
    /// lock, or persistence failure. A cleanup category failure is *not* an
    /// error: it is reported as a resumable failed plan.
    pub fn confirm_exclusion(
        &self,
        access: &SemanticAccessContext,
        confirmation_id: &str,
        expected_revision: u64,
        context: &SemanticFolderContext,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        self.ensure_operation_allowed(access, SemanticLibraryOperation::ConfirmExclusion)?;
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let mut next = locked.checked_data(expected_revision)?.clone();
        let pending = locked
            .cache
            .exclusion_confirmations
            .get(confirmation_id)
            .cloned()
            .ok_or(SemanticLibraryError::StaleConfirmation)?;
        if pending.revision != expected_revision || pending.context != *context {
            return Err(SemanticLibraryError::StaleConfirmation);
        }
        apply_exclusion(
            &mut next,
            pending.root_id,
            pending.exclusion_id,
            &context.location,
        )?;
        let plan_id = next
            .catalog
            .begin_exclusion_cleanup(&mut next.policy, pending.root_id, pending.exclusion_id)
            .map_err(map_deletion_error)?;
        locked.commit(next, core::LibraryOperation::ScopeRevocation)?;
        locked.advance_cleanup(plan_id)?;
        locked.status()
    }

    /// Resumes an incomplete or failed cleanup using its durable plan.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, stale-revision, not-found, lock,
    /// or persistence failure.
    pub fn resume_cleanup(
        &self,
        access: &SemanticAccessContext,
        plan_id: core::DeletionPlanId,
        expected_revision: u64,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        self.ensure_operation_allowed(access, SemanticLibraryOperation::ResumeCleanup)?;
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let data = locked.checked_data(expected_revision)?;
        let status = data
            .catalog
            .deletion_plan(plan_id)
            .ok_or(SemanticLibraryError::NotFound)?
            .status();
        if status == core::DeletionPlanStatus::Failed {
            let mut next = data.clone();
            next.catalog
                .resume_deletion(plan_id)
                .map_err(map_deletion_error)?;
            locked.commit(next, core::LibraryOperation::ExclusionCleanup)?;
        }
        locked.advance_cleanup(plan_id)?;
        locked.status()
    }

    /// Pauses ingestion without changing consent or indexed generations.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, stale-revision, lock, or
    /// persistence failure.
    pub fn pause(
        &self,
        access: &SemanticAccessContext,
        expected_revision: u64,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        self.set_paused(access, expected_revision, true)
    }

    /// Resumes explicitly paused ingestion.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, stale-revision, lock, or
    /// persistence failure.
    pub fn resume(
        &self,
        access: &SemanticAccessContext,
        expected_revision: u64,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        self.set_paused(access, expected_revision, false)
    }

    fn set_paused(
        &self,
        access: &SemanticAccessContext,
        expected_revision: u64,
        paused: bool,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        self.ensure_operation_allowed(
            access,
            if paused {
                SemanticLibraryOperation::Pause
            } else {
                SemanticLibraryOperation::Resume
            },
        )?;
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let mut next = locked.checked_data(expected_revision)?.clone();
        if paused {
            next.state.pause();
        } else {
            next.state.resume();
        }
        locked.commit(next, core::LibraryOperation::RuntimeState)?;
        locked.status()
    }

    /// Replaces fixed reason-based per-root eligibility overrides.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, unsafe-override, workspace,
    /// not-found, stale-revision, lock, or persistence failure.
    pub fn update_eligibility_overrides(
        &self,
        access: &SemanticAccessContext,
        root_id: core::RootId,
        workspace_id: WorkspaceId,
        expected_revision: u64,
        overrides: BTreeMap<core::EligibilityReason, core::EligibilityOverride>,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        self.ensure_operation_allowed(
            access,
            SemanticLibraryOperation::UpdateEligibilityOverrides,
        )?;
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        if overrides.iter().any(|(reason, action)| {
            *action == core::EligibilityOverride::Include && !is_safely_overridable(*reason)
        }) {
            return Err(SemanticLibraryError::UnsafeEligibilityOverride);
        }
        let mut locked = managed.lock()?;
        let mut next = locked.checked_data(expected_revision)?.clone();
        let root = next
            .policy
            .root_mut(root_id)
            .ok_or(SemanticLibraryError::NotFound)?;
        if !root.workspace_references().contains(&workspace_id) {
            return Err(SemanticLibraryError::WorkspaceRequired);
        }
        root.set_eligibility_overrides(overrides);
        locked.commit(next, core::LibraryOperation::Enrolment)?;
        locked.status()
    }

    /// Detaches an *already deleted* workspace while retaining global root
    /// consent and deduplicated content.
    ///
    /// This must only be called after the authoritative workspace repository
    /// has actually deleted the workspace. It is deliberately idempotent and
    /// self-healing: a failure queues the detachment and the next locked
    /// operation retries it, because the two stores cannot commit atomically
    /// and pretending otherwise would either revoke references for a workspace
    /// that survived or leave the obligation forgotten.
    ///
    /// If the process ends before a queued retry lands, the surviving scope is
    /// inert rather than dangerous: it names a workspace the repository no
    /// longer has, and every semantic call resolves its workspace before doing
    /// anything, so the scope can never be queried, fed, or previewed. It also
    /// holds no content of its own — occurrences are deduplicated and shared —
    /// so nothing extra is retained by it.
    ///
    /// Only a host-managed device-local library is mutated. A read-only
    /// administrator-provisioned server library is skipped entirely — no lock
    /// is taken and no path beneath its roots is touched — because workspace
    /// deletion is a desktop concern and a server user must never reach
    /// administrator-owned semantic storage through it.
    ///
    /// # Errors
    ///
    /// Returns a lock or persistence failure from the desktop mutation. The
    /// detachment stays queued for retry in that case.
    pub fn detach_workspace(&self, workspace_id: WorkspaceId) -> Result<(), SemanticLibraryError> {
        let Some(managed) = self.resolved_backend() else {
            return Ok(());
        };
        if managed.read_only || managed.server_access.is_some() {
            return Ok(());
        }
        managed.authorize(&SemanticAccessContext::Host)?;
        let mut locked = match managed.lock() {
            Ok(locked) => locked,
            Err(error) => {
                managed.queue_workspace_detachment(workspace_id);
                return Err(error);
            }
        };
        locked
            .cache
            .pending_workspace_detachments
            .insert(workspace_id);
        locked.load_data()?;
        // Draining the queue — including this workspace — under the lock means
        // success here is durable, and failure leaves the obligation queued for
        // the next locked operation instead of dropping it.
        locked.drain_pending_workspace_detachments()
    }

    /// Returns workspaces whose semantic detachment is still owed after a
    /// failed commit.
    #[must_use]
    pub fn pending_workspace_detachments(&self) -> HashSet<WorkspaceId> {
        self.resolved_backend()
            .map_or_else(HashSet::new, |managed| {
                managed
                    .cache
                    .lock()
                    .map(|cache| cache.pending_workspace_detachments.clone())
                    .unwrap_or_default()
            })
    }

    /// Records that an enrolled provider root is temporarily unreachable.
    ///
    /// This is an internal reconciliation input, not a user mutation. Consent
    /// and all indexed evidence remain intact.
    ///
    /// # Errors
    ///
    /// Returns a not-found, lock, or persistence failure.
    pub fn mark_root_unavailable(
        &self,
        access: &SemanticAccessContext,
        root_id: core::RootId,
        reason: core::RootUnavailabilityReason,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let mut next = locked.data()?.clone();
        if next.policy.root(root_id).is_none() {
            return Err(SemanticLibraryError::NotFound);
        }
        if matches!(
            next.catalog.root_availability(root_id),
            core::RootAvailability::TemporarilyUnavailable { .. }
        ) {
            return locked.status();
        }
        next.catalog
            .mark_root_unavailable(root_id, root_unavailability_message(reason));
        locked.commit(next, core::LibraryOperation::Reconciliation)?;
        locked.status()
    }

    /// Returns the enrolled root whose consent covers a provider location.
    ///
    /// Watchers and reconciliation schedulers use this to translate an
    /// observed provider event into the stable root identity the library
    /// speaks, without giving them the policy document.
    ///
    /// # Errors
    ///
    /// Returns a lock or persistence failure.
    pub fn root_for_location(
        &self,
        access: &SemanticAccessContext,
        location: &Location,
    ) -> Result<Option<core::RootId>, SemanticLibraryError> {
        let Some(managed) = self.resolved_backend() else {
            return Ok(None);
        };
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let data = locked.data()?;
        Ok(match data.policy.consent_state(location) {
            Ok(
                core::ConsentState::IncludedHere { root_id }
                | core::ConsentState::InheritedFromParent { root_id }
                | core::ConsentState::Excluded { root_id, .. },
            ) => Some(root_id),
            _ => None,
        })
    }

    /// Returns the enrolled root whose own location is exactly this one.
    ///
    /// Unlike [`Self::root_for_location`] this never resolves a descendant, so
    /// a caller can distinguish "this enrolled source itself is unreachable"
    /// from "one folder inside it could not be read".
    ///
    /// # Errors
    ///
    /// Returns an authorization, lock, or persistence failure.
    pub fn enrolled_root_at(
        &self,
        access: &SemanticAccessContext,
        location: &Location,
    ) -> Result<Option<core::RootId>, SemanticLibraryError> {
        let Some(managed) = self.resolved_backend() else {
            return Ok(None);
        };
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        Ok(locked
            .data()?
            .policy
            .roots()
            .values()
            .find(|root| root.location() == location)
            .map(core::EnrolledRoot::id))
    }

    /// Applies provider observations, following a move — and restoring
    /// availability — only when stable same-provider/same-volume identity
    /// proves it.
    ///
    /// This is the only way an unavailable root becomes available again: there
    /// is deliberately no "mark available" counterpart to
    /// [`Self::mark_root_unavailable`], because a path that merely exists
    /// again is not evidence. A quarantined root — one whose path was reused
    /// by a different filesystem object, or that appeared on another volume or
    /// through another provider — stays quarantined until an observation
    /// carries the enrolled `(volume, file)` identity, and a root that never
    /// had a stable identity is never restored automatically at all.
    ///
    /// Callers must therefore obtain identities from a provider capability
    /// that exposes a verified stable entry and volume identity. A directory
    /// listing alone does not, which is why nothing in the listing path calls
    /// this.
    ///
    /// # Errors
    ///
    /// Returns an invalid-request, lock, or persistence failure.
    pub fn observe_root_identity(
        &self,
        access: &SemanticAccessContext,
        root_id: core::RootId,
        observations: &[core::ObservedRootIdentity],
    ) -> Result<core::RootMoveResolution, SemanticLibraryError> {
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let mut next = locked.data()?.clone();
        let resolution = next
            .catalog
            .reconcile_root_location(&mut next.policy, root_id, observations)
            .map_err(|_| SemanticLibraryError::InvalidRequest)?;
        locked.commit(next, core::LibraryOperation::Reconciliation)?;
        Ok(resolution)
    }

    /// Commits one complete, successful enumeration of a root.
    ///
    /// Only this call may remove documents that were merely absent: pause,
    /// unavailability, and a partial crawl never do.
    ///
    /// # Errors
    ///
    /// Returns an invalid-request (paused, unavailable, unknown root), lock,
    /// or persistence failure.
    pub fn complete_reconciliation(
        &self,
        access: &SemanticAccessContext,
        root_id: core::RootId,
        observed_occurrences: &BTreeSet<core::OccurrenceId>,
    ) -> Result<u64, SemanticLibraryError> {
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let mut next = locked.data()?.clone();
        let generation = next
            .catalog
            .complete_reconciliation(&next.policy, &next.state, root_id, observed_occurrences)
            .map_err(|_| SemanticLibraryError::InvalidRequest)?;
        next.state
            .record_indexed_generation(root_id, generation)
            .map_err(|_| SemanticLibraryError::InvalidRequest)?;
        locked.commit(next, core::LibraryOperation::Reconciliation)?;
        Ok(generation)
    }

    /// Returns provider-neutral worker feed decisions plus the curated
    /// eligibility verdict for each offered candidate.
    ///
    /// The curated [`core::EligibilityPolicy`] is applied here — not in a
    /// scheduler — so exactly one implementation decides what may be ingested,
    /// and skip reasons stay visible. Locations never leave the host: the
    /// decisions carry only opaque identifiers.
    ///
    /// The tenant stamped onto every decision is derived from the
    /// already-authorized [`SemanticAccessContext`], never from a caller-chosen
    /// argument: a second, independent tenant parameter would let an
    /// authorized principal label another tenant's worker traffic.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, not-found, lock, or persistence
    /// failure.
    pub fn worker_feed_plan(
        &self,
        access: &SemanticAccessContext,
        candidates: &[SemanticFeedCandidate],
    ) -> Result<SemanticWorkerFeedPlan, SemanticLibraryError> {
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let tenant = managed.tenant_for(access)?;
        let mut locked = managed.lock()?;
        let data = locked.data()?;
        let policy = core::EligibilityPolicy::curated_defaults();
        let budgets = data.policy.resource_profile().budgets;
        let usage = core::ResourceUsage::measure(&data.catalog);
        let mut eligible = Vec::new();
        let mut decisions = Vec::new();
        for candidate in candidates {
            let root = data
                .policy
                .root(candidate.root_id)
                .ok_or(SemanticLibraryError::NotFound)?;
            let consented = matches!(
                data.policy.consent_state(&candidate.candidate.location),
                Ok(core::ConsentState::IncludedHere { .. }
                    | core::ConsentState::InheritedFromParent { .. })
            );
            if !consented {
                decisions.push(core::EligibilityDecision::Skipped(
                    [core::EligibilityReason::ExplicitlyExcluded].into(),
                ));
                continue;
            }
            let decision = policy
                .evaluate(
                    root.location(),
                    &candidate.candidate,
                    &budgets,
                    usage,
                    root.eligibility_overrides(),
                )
                .map_err(|_| SemanticLibraryError::InvalidRequest)?;
            if decision == core::EligibilityDecision::Eligible {
                eligible.push(candidate.candidate.location.clone());
            }
            decisions.push(decision);
        }
        let paused = data.state.is_paused();
        let worker_decisions = if paused {
            Vec::new()
        } else {
            data.catalog
                .worker_feed_decisions(&data.policy, &data.state, tenant)
                .map_err(|_| SemanticLibraryError::InvalidRequest)?
        };
        Ok(SemanticWorkerFeedPlan {
            paused,
            decisions: worker_decisions,
            eligible_locations: eligible,
            skipped_reason_counts: reason_counts(&core::EligibilityReasonCounts::from_decisions(
                decisions,
            )),
        })
    }

    /// Records one host-read file in the authoritative catalog.
    ///
    /// The caller supplies every workspace attached to the enrolled root so
    /// the physical occurrence retains all authorization scopes. Content and
    /// locations remain host-side; only a later worker feed decision may
    /// authorize streaming the bytes.
    pub(crate) fn record_indexing_observation(
        &self,
        access: &SemanticAccessContext,
        observation: SemanticIndexingObservation,
    ) -> Result<core::OccurrenceId, SemanticLibraryError> {
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        if observation.workspace_ids.is_empty() {
            return Err(SemanticLibraryError::WorkspaceRequired);
        }
        let mut locked = managed.lock()?;
        let mut next = locked.data()?.clone();
        let observations = observation
            .workspace_ids
            .iter()
            .copied()
            .map(|workspace_id| {
                core::CatalogObservation::new(
                    observation.entry_id,
                    observation.location.clone(),
                    observation.content_fingerprint.clone(),
                    core::OccurrenceScope::new(workspace_id, observation.root_id),
                    core::DocumentArtifacts::default(),
                    core::DocumentMeasurement::new(observation.source_bytes, 0, 0),
                )
            });
        let result = match (&managed.server_access, access) {
            (Some(server), SemanticAccessContext::Server(identity)) => {
                let server_access = core::AccessContext::new(
                    identity.tenant_id.clone(),
                    server.library_id,
                    identity.user_id.clone(),
                );
                next.catalog.ingest_for_tenant(
                    &next.policy,
                    &next.state,
                    &server.policy,
                    &server_access,
                    observations,
                )
            }
            _ => next
                .catalog
                .upsert_observations(&next.policy, &next.state, observations),
        };
        result.map_err(|_| SemanticLibraryError::InvalidRequest)?;
        let occurrence_id = next
            .catalog
            .occurrence_at(observation.entry_id, &observation.location)
            .map(core::OccurrenceRecord::id)
            .ok_or(SemanticLibraryError::InvalidRequest)?;
        locked.commit(next, core::LibraryOperation::Reconciliation)?;
        Ok(occurrence_id)
    }

    /// Resolves one path-free worker source against the current host catalog.
    ///
    /// Authorization and consent are rechecked at activation time so a stale
    /// derived index cannot disclose or open a revoked occurrence.
    pub(crate) fn resolve_occurrence(
        &self,
        access: &SemanticAccessContext,
        workspace_id: WorkspaceId,
        source_id: &str,
    ) -> Result<Option<ResolvedSemanticOccurrence>, SemanticLibraryError> {
        let occurrence_id = match core::OccurrenceId::from_str(source_id) {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let data = locked.data()?;
        let Some(occurrence) = data
            .catalog
            .authorized_occurrence(&data.policy, workspace_id, occurrence_id)
            .map_err(|_| SemanticLibraryError::InvalidRequest)?
        else {
            return Ok(None);
        };
        Ok(Some(ResolvedSemanticOccurrence {
            entry_id: occurrence.entry_id(),
            location: occurrence.location().clone(),
            available: data.catalog.source_availability(occurrence_id)
                == Some(core::SourceAvailability::Available),
        }))
    }

    /// Resolves one exact host-authorized entry to its opaque worker scope.
    pub(crate) fn resolve_summary_document(
        &self,
        access: &SemanticAccessContext,
        workspace_id: WorkspaceId,
        entry_id: fm_domain::EntryId,
        location: &Location,
    ) -> Result<Option<ResolvedSummaryDocument>, SemanticLibraryError> {
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let data = locked.data()?;
        let Some(candidate) = data.catalog.occurrence_at(entry_id, location) else {
            return Ok(None);
        };
        let Some(occurrence) = data
            .catalog
            .authorized_occurrence(&data.policy, workspace_id, candidate.id())
            .map_err(|_| SemanticLibraryError::InvalidRequest)?
        else {
            return Ok(None);
        };
        if data.catalog.source_availability(occurrence.id())
            != Some(core::SourceAvailability::Available)
        {
            return Ok(None);
        }
        let tenant_id = match access {
            SemanticAccessContext::Host => DEVICE_LOCAL_TENANT_ID.to_owned(),
            SemanticAccessContext::Server(identity) => identity.tenant_id.to_string(),
            SemanticAccessContext::Anonymous => return Err(SemanticLibraryError::InvalidRequest),
        };
        Ok(Some(ResolvedSummaryDocument {
            tenant_id,
            library_id: data.policy.library().id().to_string(),
            document_id: occurrence.document_id().to_string(),
        }))
    }

    /// Resolves a user-visible Ask scope into current worker authorization.
    pub(crate) fn resolve_rag_scope(
        &self,
        access: &SemanticAccessContext,
        workspace_id: WorkspaceId,
        selection: &RagScopeSelection,
        question: String,
        policy: RagRetrievalPolicy,
    ) -> Result<ResolvedRagScope, SemanticLibraryError> {
        let managed = self.managed_backend()?;
        managed.authorize(access)?;
        let mut locked = managed.lock()?;
        let data = locked.data()?;
        let tenant_id = match access {
            SemanticAccessContext::Host => DEVICE_LOCAL_TENANT_ID.to_owned(),
            SemanticAccessContext::Server(identity) => identity.tenant_id.to_string(),
            SemanticAccessContext::Anonymous => return Err(SemanticLibraryError::InvalidRequest),
        };
        let requested_results = match selection {
            RagScopeSelection::SemanticResults(ids) => Some(ids.iter().collect::<HashSet<_>>()),
            _ => None,
        };
        let requested_roots = match selection {
            RagScopeSelection::EnrolledRoots(ids) => {
                Some(ids.iter().copied().collect::<HashSet<_>>())
            }
            _ => None,
        };
        if let RagScopeSelection::CurrentFolder(folder) = selection {
            let authorized = data.policy.roots().values().any(|root| {
                root.workspace_references().contains(&workspace_id)
                    && root.location().provider_id == folder.provider_id
                    && (root.location() == folder
                        || (root.recursive()
                            && location_is_within_uri(&folder.uri, &root.location().uri)))
            });
            if !authorized {
                return Err(SemanticLibraryError::InvalidRequest);
            }
        }
        if let Some(root_ids) = &requested_roots
            && root_ids.iter().any(|root_id| {
                data.policy
                    .root(*root_id)
                    .is_none_or(|root| !root.workspace_references().contains(&workspace_id))
            })
        {
            return Err(SemanticLibraryError::InvalidRequest);
        }
        let mut matched_selected = match selection {
            RagScopeSelection::SelectedFiles(targets) => vec![false; targets.len()],
            _ => Vec::new(),
        };
        let mut allowed_source_ids = BTreeSet::new();
        let mut titles = HashMap::new();
        let mut unavailable = 0u64;
        for candidate in data.catalog.occurrences() {
            let Some(occurrence) = data
                .catalog
                .authorized_occurrence(&data.policy, workspace_id, candidate.id())
                .map_err(|_| SemanticLibraryError::InvalidRequest)?
            else {
                continue;
            };
            let selected = match selection {
                RagScopeSelection::EntireLibrary => true,
                RagScopeSelection::SelectedFiles(targets) => {
                    let mut matched = false;
                    for (index, (entry, location)) in targets.iter().enumerate() {
                        if occurrence.entry_id() == *entry && occurrence.location() == location {
                            matched_selected[index] = true;
                            matched = true;
                        }
                    }
                    matched
                }
                RagScopeSelection::CurrentFolder(folder) => {
                    occurrence.location().provider_id == folder.provider_id
                        && location_is_within_uri(&occurrence.location().uri, &folder.uri)
                }
                RagScopeSelection::SemanticResults(_) => requested_results
                    .as_ref()
                    .is_some_and(|ids| ids.contains(&occurrence.id().to_string())),
                RagScopeSelection::EnrolledRoots(_) => occurrence.scopes().iter().any(|scope| {
                    scope.workspace_id() == workspace_id
                        && requested_roots
                            .as_ref()
                            .is_some_and(|ids| ids.contains(&scope.root_id()))
                }),
            };
            if !selected {
                continue;
            }
            let source_id = occurrence.id().to_string();
            if data.catalog.source_availability(occurrence.id())
                != Some(core::SourceAvailability::Available)
            {
                unavailable = unavailable.saturating_add(1);
            }
            let title = occurrence
                .location()
                .uri
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .filter(|value| !value.is_empty())
                .unwrap_or("Indexed document")
                .to_owned();
            titles.insert(source_id.clone(), title);
            allowed_source_ids.insert(source_id);
        }
        if matched_selected.iter().any(|matched| !matched)
            || requested_results
                .as_ref()
                .is_some_and(|requested| requested.len() != allowed_source_ids.len())
        {
            return Err(SemanticLibraryError::InvalidRequest);
        }
        let eligible = u64::try_from(allowed_source_ids.len())
            .map_err(|_| SemanticLibraryError::InvalidRequest)?;
        Ok(ResolvedRagScope {
            retrieval: RagRetrievalRequest {
                question,
                filters: QueryFilters {
                    tenant_id,
                    library_id: Some(data.policy.library().id().to_string()),
                    include_unavailable: true,
                    ..QueryFilters::default()
                },
                source_restriction: RagSourceRestriction { allowed_source_ids },
                current_hashes: HashMap::new(),
                policy,
            },
            titles,
            eligible,
            unavailable,
        })
    }

    fn unresolved_authority(&self) -> SemanticLibraryAuthority {
        match &self.backend {
            SemanticLibraryBackend::Unavailable { authority } => *authority,
            SemanticLibraryBackend::Managed(managed) => managed.authority,
        }
    }

    /// Resolves the managed backend of this service instance.
    ///
    /// Deferred desktop composition happens one level up, in
    /// [`SemanticLibraryComposition`], which re-reads authoritative component
    /// state on every operation and replaces this whole service when its roots
    /// or model identity change. A service instance is therefore permanently
    /// bound to exactly one library.
    fn resolved_backend(&self) -> Option<Arc<ManagedSemanticLibrary>> {
        match &self.backend {
            SemanticLibraryBackend::Unavailable { .. } => None,
            SemanticLibraryBackend::Managed(managed) => Some(Arc::clone(managed)),
        }
    }

    fn managed_backend(&self) -> Result<Arc<ManagedSemanticLibrary>, SemanticLibraryError> {
        self.resolved_backend()
            .ok_or(SemanticLibraryError::Unavailable)
    }
}

/// One host-enumerated entry offered to the semantic feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticFeedCandidate {
    /// Enrolled root the entry was found beneath.
    pub root_id: core::RootId,
    /// Provider-neutral metadata gathered through VFS capabilities.
    pub candidate: core::EligibilityCandidate,
}

/// Complete host-side input for one catalog occurrence.
pub(crate) struct SemanticIndexingObservation {
    pub(crate) entry_id: fm_domain::EntryId,
    pub(crate) location: Location,
    pub(crate) content_fingerprint: core::ContentFingerprint,
    pub(crate) root_id: core::RootId,
    pub(crate) workspace_ids: Vec<WorkspaceId>,
    pub(crate) source_bytes: u64,
}

/// Provider-neutral feed plan produced from durable consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticWorkerFeedPlan {
    /// Whether ingestion is paused; queries still use existing generations.
    pub paused: bool,
    /// Path-free decisions the host may send to the isolated worker.
    pub decisions: Vec<core::WorkerFeedDecision>,
    /// Candidate locations the curated policy admitted, retained by the host.
    pub eligible_locations: Vec<Location>,
    /// Visible skip reasons and their counts.
    pub skipped_reason_counts: Vec<SemanticEligibilityReasonCount>,
}

/// Host-only location resolved from an opaque worker evidence source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedSemanticOccurrence {
    pub(crate) entry_id: fm_domain::EntryId,
    pub(crate) location: Location,
    pub(crate) available: bool,
}

/// Opaque worker identity resolved from an exact authorized host occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedSummaryDocument {
    pub(crate) tenant_id: String,
    pub(crate) library_id: String,
    pub(crate) document_id: String,
}

/// Host-side inputs for resolving one visible Ask scope.
#[derive(Debug, Clone)]
pub(crate) enum RagScopeSelection {
    /// Every occurrence authorized through the workspace.
    EntireLibrary,
    /// Exact entry/location pairs.
    SelectedFiles(Vec<(fm_domain::EntryId, Location)>),
    /// One folder and all descendants.
    CurrentFolder(Location),
    /// Opaque occurrence identities from a host-owned semantic result set.
    SemanticResults(Vec<String>),
    /// One or more enrolled roots.
    EnrolledRoots(Vec<core::RootId>),
}

/// Worker request and host-only display data derived from current authorization.
pub(crate) struct ResolvedRagScope {
    pub(crate) retrieval: RagRetrievalRequest,
    pub(crate) titles: HashMap<String, String>,
    pub(crate) eligible: u64,
    pub(crate) unavailable: u64,
}

fn location_is_within_uri(candidate: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches('/');
    candidate == folder
        || candidate
            .strip_prefix(folder)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn build_managed(
    configuration: SemanticLibraryConfiguration,
    authority: SemanticLibraryAuthority,
    read_only: bool,
    deterministic: bool,
    estimator: Arc<dyn SemanticEnrolmentEstimator>,
    server: Option<(
        SemanticServerIdentity,
        core::ServerEnrolmentPolicy,
        core::HardQuotas,
    )>,
) -> Result<ManagedSemanticLibrary, SemanticLibraryError> {
    let policy = core::SemanticLibraryPolicy::new(
        configuration.library.clone(),
        configuration.resource_profile,
    )
    .map_err(map_policy_error)?;
    let library_id = policy.library().id();
    let server_access = server.map(|(administrator, enrolment_policy, quotas)| ServerAccess {
        policy: core::ServerPolicy::single_private(
            administrator.tenant_id,
            library_id,
            administrator.user_id,
            enrolment_policy,
            quotas,
        ),
        library_id,
    });
    let initial_data = deterministic.then(|| LibraryData {
        policy: policy.clone(),
        catalog: core::SemanticCatalog::new(library_id),
        state: core::SemanticLibraryState::new(library_id),
    });
    let durable_revision = initial_data.as_ref().map(|data| data.policy.revision());
    Ok(ManagedSemanticLibrary {
        authority,
        read_only,
        deterministic,
        coordinator: (!deterministic).then(|| {
            core::SemanticLibraryCoordinator::new(
                configuration.configuration_directory,
                configuration.semantic_data_root,
            )
        }),
        initial_policy: policy,
        estimator,
        cleanup: Arc::new(CatalogSemanticCleanupExecutor),
        commit_observer: Arc::new(PermissiveSemanticCommitObserver),
        server_access,
        cache: Mutex::new(LibraryCache {
            data: initial_data,
            durable_revision,
            next_id: 1,
            enrolment_confirmations: BTreeMap::new(),
            exclusion_confirmations: BTreeMap::new(),
            pending_workspace_detachments: HashSet::new(),
        }),
    })
}

impl ManagedSemanticLibrary {
    /// Authorizes exactly one call from the caller-supplied context.
    ///
    /// A server-provisioned library only accepts an authenticated server
    /// principal that its own policy authorizes; a device-local library only
    /// accepts the host user. A context of the wrong kind is denied rather
    /// than upgraded, so a server request can never borrow desktop authority.
    fn authorize(&self, access: &SemanticAccessContext) -> Result<(), SemanticLibraryError> {
        match (&self.server_access, access) {
            (Some(server), SemanticAccessContext::Server(identity)) => server
                .policy
                .authorize(&core::AccessContext::new(
                    identity.tenant_id.clone(),
                    server.library_id,
                    identity.user_id.clone(),
                ))
                .map(|_| ())
                .map_err(|_| SemanticLibraryError::AccessDenied),
            (None, SemanticAccessContext::Host) => Ok(()),
            // The deterministic mock is an in-memory fixture with no tenant
            // isolation and no filesystem: it serves whichever authenticated
            // principal its host supplies, so the browser and desktop mock
            // runtimes behave identically. An anonymous caller is still denied.
            (None, SemanticAccessContext::Server(_)) if self.deterministic => Ok(()),
            _ => Err(SemanticLibraryError::AccessDenied),
        }
    }

    /// Rejects durable state that belongs to a different library or model than
    /// the one this service was configured with.
    ///
    /// A semantic-data root or configuration directory can be restored from a
    /// backup, copied between profiles, or left behind by a model migration.
    /// Adopting whatever policy is found there would silently expose another
    /// library's enrolled roots — and, on a server, roots the access-control
    /// list was never written for — so the mismatch is a typed failure and the
    /// old library is neither projected nor mutated.
    fn ensure_configured_identity(
        &self,
        policy: &core::SemanticLibraryPolicy,
    ) -> Result<(), SemanticLibraryError> {
        let configured = self.initial_policy.library();
        if policy.library() != configured {
            return Err(SemanticLibraryError::IncompatibleLibraryIdentity);
        }
        if self
            .server_access
            .as_ref()
            .is_some_and(|server| server.library_id != policy.library().id())
        {
            return Err(SemanticLibraryError::IncompatibleLibraryIdentity);
        }
        Ok(())
    }

    /// Returns the tenant every worker decision is stamped with, derived only
    /// from the already-authorized caller.
    ///
    /// A device-local library has no tenant administration, so it uses one
    /// fixed opaque device tenant rather than accepting a caller-supplied one.
    fn tenant_for(
        &self,
        access: &SemanticAccessContext,
    ) -> Result<core::TenantId, SemanticLibraryError> {
        match access {
            SemanticAccessContext::Server(identity) => Ok(identity.tenant_id.clone()),
            SemanticAccessContext::Host => {
                core::TenantId::new(DEVICE_LOCAL_TENANT_ID).map_err(|_| {
                    // The constant is validated by its own unit test.
                    SemanticLibraryError::InvalidRequest
                })
            }
            SemanticAccessContext::Anonymous => Err(SemanticLibraryError::AccessDenied),
        }
    }

    /// Records a detachment obligation for a workspace that no longer exists.
    fn queue_workspace_detachment(&self, workspace_id: WorkspaceId) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.pending_workspace_detachments.insert(workspace_id);
        }
    }

    /// Takes the in-process cache lock and then the cross-process library lock.
    ///
    /// The order is fixed everywhere, and the returned value owns both, so a
    /// mutation can recover, reload, check, mutate, commit, and refresh its
    /// cache without any window for another thread or process.
    fn lock(&self) -> Result<LockedLibrary<'_>, SemanticLibraryError> {
        let cache = self
            .cache
            .lock()
            .map_err(|_| SemanticLibraryError::StateUnavailable)?;
        let session = match &self.coordinator {
            Some(coordinator) => Some(coordinator.lock().map_err(map_store_error)?),
            None => None,
        };
        Ok(LockedLibrary {
            managed: self,
            cache,
            session,
        })
    }
}

/// A semantic library held under both locks.
struct LockedLibrary<'library> {
    managed: &'library ManagedSemanticLibrary,
    cache: MutexGuard<'library, LibraryCache>,
    session: Option<core::LibrarySession<'library>>,
}

impl LockedLibrary<'_> {
    /// Returns durable data, reloading whenever the cache is empty, a
    /// committed journal record is still waiting to be installed, or another
    /// process moved the durable revision.
    ///
    /// The pending-record probe comes first and is not an optimisation: a
    /// writer that died after making its intent durable left the installed
    /// documents — and therefore the installed revision — at their old values,
    /// so a cached reader that trusted the revision alone would keep serving a
    /// scope the user already revoked. Proving that nothing is pending is a
    /// directory scan, so the expensive full reload still only happens when the
    /// snapshot really is stale.
    fn data(&mut self) -> Result<&LibraryData, SemanticLibraryError> {
        self.load_data()?;
        if self.drain_pending_workspace_detachments().is_err() {
            // The failed commit discarded the cached snapshot. Reload it so
            // the caller still sees consistent durable state while the
            // detachment stays queued for the next operation.
            self.load_data()?;
        }
        self.cache
            .data
            .as_ref()
            .ok_or(SemanticLibraryError::StateUnavailable)
    }

    fn load_data(&mut self) -> Result<(), SemanticLibraryError> {
        if let Some(session) = &self.session {
            if session.has_pending_record().map_err(map_store_error)? {
                session.recover().map_err(map_store_error)?;
                // Whatever recovery installed was decided by another process,
                // so the cached snapshot and every confirmation minted from it
                // are worthless.
                self.cache.invalidate();
            }
            let durable = session.durable_revision().map_err(map_store_error)?;
            if self.cache.data.is_none() || self.cache.durable_revision != durable {
                if self.cache.data.is_some() {
                    // Another writer changed durable consent: every
                    // confirmation minted against the old snapshot is stale.
                    self.cache.clear_confirmations();
                }
                let loaded = match session.load() {
                    Ok(loaded) => LibraryData {
                        policy: loaded.policy,
                        catalog: loaded.catalog,
                        state: loaded.state,
                    },
                    Err(core::StoreError::PolicyMissing) => {
                        let library_id = self.managed.initial_policy.library().id();
                        LibraryData {
                            policy: self.managed.initial_policy.clone(),
                            catalog: core::SemanticCatalog::new(library_id),
                            state: core::SemanticLibraryState::new(library_id),
                        }
                    }
                    Err(error) => {
                        self.cache.invalidate();
                        return Err(map_store_error(error));
                    }
                };
                self.managed.ensure_configured_identity(&loaded.policy)?;
                self.cache.durable_revision =
                    session.durable_revision().map_err(map_store_error)?;
                self.cache.data = Some(loaded);
            }
        } else if self.cache.data.is_none() {
            let library_id = self.managed.initial_policy.library().id();
            self.cache.data = Some(LibraryData {
                policy: self.managed.initial_policy.clone(),
                catalog: core::SemanticCatalog::new(library_id),
                state: core::SemanticLibraryState::new(library_id),
            });
        }
        Ok(())
    }

    /// Retries workspace detachments whose durable commit failed earlier.
    ///
    /// Workspace deletion is authoritative in the workspace repository, and
    /// the semantic library is a second store: pretending the two commit
    /// atomically would be a lie. The repository deletion therefore happens
    /// first and stands on its own, while the semantic reference cleanup is
    /// idempotent and simply retried on the next locked operation until it
    /// lands. Nothing is queryable in the meantime because every semantic
    /// query path validates that the workspace still exists.
    fn drain_pending_workspace_detachments(&mut self) -> Result<(), SemanticLibraryError> {
        if self.cache.pending_workspace_detachments.is_empty() || self.cache.data.is_none() {
            return Ok(());
        }
        let pending: Vec<WorkspaceId> = self
            .cache
            .pending_workspace_detachments
            .iter()
            .copied()
            .collect();
        let Some(current) = self.cache.data.as_ref() else {
            return Ok(());
        };
        let mut next = current.clone();
        for workspace_id in &pending {
            next.policy.remove_workspace_reference(*workspace_id);
            next.catalog.remove_workspace_scopes(*workspace_id);
        }
        if next == *current {
            self.cache.pending_workspace_detachments.clear();
            return Ok(());
        }
        match self.commit(next, core::LibraryOperation::Enrolment) {
            Ok(()) => {
                self.cache.pending_workspace_detachments.clear();
                Ok(())
            }
            // Still unavailable: keep the retry queued rather than dropping
            // the obligation, and let the caller see its own failure.
            Err(error) => Err(error),
        }
    }

    /// Returns durable data only when it still matches the caller's optimistic
    /// revision, compared against what was just read from disk.
    fn checked_data(
        &mut self,
        expected_revision: u64,
    ) -> Result<&LibraryData, SemanticLibraryError> {
        let actual = self.data()?.policy.revision();
        if actual != expected_revision {
            return Err(SemanticLibraryError::StaleRevision {
                expected: expected_revision,
                actual,
            });
        }
        self.cache
            .data
            .as_ref()
            .ok_or(SemanticLibraryError::StateUnavailable)
    }

    /// Advances the durable revision, commits every participant, and refreshes
    /// the cache.
    ///
    /// Any failure — before, at, or after the commit point — discards the
    /// cached snapshot and every pending confirmation, so the next call reloads
    /// whatever deterministic recovery decided rather than trusting a snapshot
    /// that may never have landed.
    fn commit(
        &mut self,
        mut next: LibraryData,
        operation: core::LibraryOperation,
    ) -> Result<(), SemanticLibraryError> {
        if next.policy.advance_revision().is_err() {
            self.cache.invalidate();
            return Err(SemanticLibraryError::RevisionOverflow);
        }
        // Per-root runtime metadata is only meaningful while the policy still
        // has that root, and it travels in the same transaction as the policy
        // that decides it.
        next.state
            .retain_roots(|root_id| next.policy.root(root_id).is_some());
        let commit_observer = Arc::clone(&self.managed.commit_observer);
        let Some(session) = self.session.as_ref() else {
            self.cache.durable_revision = Some(next.policy.revision());
            self.cache.data = Some(next);
            self.cache.clear_confirmations();
            return Ok(());
        };
        let expected = self.cache.durable_revision;
        let outcome = (|| -> Result<(), SemanticLibraryError> {
            // The exclusive cross-process lock has been held since the session
            // was acquired, so this only guards against an in-process caller
            // that decided from a snapshot older than the last reload.
            let observed = session.durable_revision().map_err(map_store_error)?;
            if observed != expected {
                return Err(SemanticLibraryError::StaleRevision {
                    expected: expected.unwrap_or(0),
                    actual: observed.unwrap_or(0),
                });
            }
            let mut transaction = session
                .transaction(
                    operation,
                    Some(&next.policy),
                    Some(&next.catalog),
                    Some(&next.state),
                )
                .map_err(map_store_error)?;
            while let Some(step) = transaction.next_step() {
                if commit_observer.before_step(step).is_err() {
                    transaction.interrupt();
                    return Err(SemanticLibraryError::Persistence);
                }
                transaction.advance().map_err(map_store_error)?;
            }
            Ok(())
        })();
        match outcome {
            Ok(()) => {
                self.cache.durable_revision = Some(next.policy.revision());
                self.cache.data = Some(next);
                self.cache.clear_confirmations();
                Ok(())
            }
            Err(error) => {
                self.cache.invalidate();
                Err(error)
            }
        }
    }

    /// Advances every incomplete deletion category batch by batch, committing
    /// a durable checkpoint after each one.
    ///
    /// A category failure is persisted against the plan and stops the run:
    /// completed categories *and* the item progress of the failed one keep
    /// their durable checkpoints, so a resume — or a restart in the middle of
    /// a running plan — continues from the last checkpoint instead of
    /// repeating finished work or discarding the plan.
    ///
    /// Each batch is handed to the executor with a key derived from that
    /// durable checkpoint, so a retry after a crash between the external
    /// deletion and its checkpoint presents the identical key.
    fn advance_cleanup(
        &mut self,
        plan_id: core::DeletionPlanId,
    ) -> Result<SemanticCleanupState, SemanticLibraryError> {
        let plan_key = plan_id.to_string();
        for category in core::DeletionCategory::all() {
            let (complete, total_items, mut completed_items) = {
                let progress = self
                    .data()?
                    .catalog
                    .deletion_plan(plan_id)
                    .ok_or(SemanticLibraryError::NotFound)?
                    .progress(*category);
                (
                    progress.is_complete(),
                    progress.total_items(),
                    progress.completed_items(),
                )
            };
            if complete {
                continue;
            }
            let category_dto = map_deletion_category(*category);
            loop {
                let item_count = total_items
                    .saturating_sub(completed_items)
                    .min(CLEANUP_BATCH_ITEMS);
                let request = SemanticCleanupRequest::new(
                    &plan_key,
                    category_dto,
                    completed_items,
                    item_count,
                );
                if let Err(failure) = self.managed.cleanup.execute(&request) {
                    let mut next = self.data()?.clone();
                    next.catalog
                        .fail_deletion_category(plan_id, *category, failure.message())
                        .map_err(map_deletion_error)?;
                    self.commit(next, core::LibraryOperation::ExclusionCleanup)?;
                    return Ok(SemanticCleanupState::Failed);
                }
                let advanced = completed_items.saturating_add(item_count);
                let mut next = self.data()?.clone();
                if advanced >= total_items {
                    next.catalog
                        .complete_deletion_category(&mut next.policy, plan_id, *category)
                        .map_err(map_deletion_error)?;
                    self.commit(next, core::LibraryOperation::ExclusionCleanup)?;
                    break;
                }
                next.catalog
                    .checkpoint_deletion_category(plan_id, *category, advanced)
                    .map_err(map_deletion_error)?;
                self.commit(next, core::LibraryOperation::ExclusionCleanup)?;
                completed_items = advanced;
            }
        }
        Ok(SemanticCleanupState::Complete)
    }

    fn status(&mut self) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        let data = self.data()?.clone();
        Ok(project_status(&data))
    }
}

fn apply_exclusion(
    data: &mut LibraryData,
    root_id: core::RootId,
    exclusion_id: core::ExclusionId,
    location: &Location,
) -> Result<(), SemanticLibraryError> {
    if data
        .policy
        .root(root_id)
        .is_some_and(|root| root.location() == location)
    {
        data.policy
            .exclude_root(root_id, exclusion_id)
            .map_err(map_policy_error)
    } else {
        data.policy
            .exclude_descendant(root_id, exclusion_id, location.clone())
            .map_err(map_policy_error)
    }
}

fn next_confirmation(cache: &mut LibraryCache, deterministic: bool, prefix: &str) -> String {
    if deterministic {
        let value = format!("mock-{prefix}-confirmation-{}", cache.next_id);
        cache.next_id = cache.next_id.saturating_add(1);
        value
    } else {
        Uuid::new_v4().to_string()
    }
}

fn next_root_id(cache: &mut LibraryCache, deterministic: bool) -> core::RootId {
    if deterministic {
        let id = core::RootId::from_uuid(Uuid::from_u128(0x179_0000 + cache.next_id));
        cache.next_id = cache.next_id.saturating_add(1);
        id
    } else {
        core::RootId::new()
    }
}

fn next_exclusion_id(cache: &mut LibraryCache, deterministic: bool) -> core::ExclusionId {
    if deterministic {
        let id = core::ExclusionId::from_uuid(Uuid::from_u128(0x179_1000 + cache.next_id));
        cache.next_id = cache.next_id.saturating_add(1);
        id
    } else {
        core::ExclusionId::new()
    }
}

fn project_estimate(
    estimate: &SemanticEnrolmentEstimate,
    budgets: core::ResourceBudgets,
) -> SemanticEnrolmentEstimateStatus {
    let estimated_additional_local_bytes = estimate
        .estimated_extracted_bytes
        .zip(estimate.estimated_vector_bytes)
        .and_then(|(extracted, vectors)| extracted.checked_add(vectors))
        .and_then(|subtotal| {
            estimate
                .missing_model_download_bytes
                .and_then(|model| subtotal.checked_add(model))
        });
    let mut exceeded_budgets = Vec::new();
    if estimate
        .estimated_files
        .is_some_and(|value| value > budgets.max_documents)
    {
        exceeded_budgets.push(SemanticBudgetKind::Documents);
    }
    if estimate
        .estimated_source_bytes
        .is_some_and(|value| value > budgets.max_total_source_bytes)
    {
        exceeded_budgets.push(SemanticBudgetKind::SourceBytes);
    }
    if estimate
        .estimated_extracted_bytes
        .is_some_and(|value| value > budgets.max_total_extracted_bytes)
    {
        exceeded_budgets.push(SemanticBudgetKind::ExtractedBytes);
    }
    if estimate
        .estimated_vector_bytes
        .is_some_and(|value| value > budgets.max_total_vector_bytes)
    {
        exceeded_budgets.push(SemanticBudgetKind::VectorBytes);
    }
    SemanticEnrolmentEstimateStatus {
        completeness: estimate.completeness,
        estimated_files: estimate.estimated_files,
        estimated_source_bytes: estimate.estimated_source_bytes,
        estimated_extracted_bytes: estimate.estimated_extracted_bytes,
        estimated_vector_bytes: estimate.estimated_vector_bytes,
        estimated_additional_local_bytes,
        missing_model_download_bytes: estimate.missing_model_download_bytes,
        skipped_reason_counts: reason_counts(&estimate.skipped_reason_counts),
        exceeded_budgets,
        unavailable_reason: estimate.unavailable_reason.clone(),
    }
}

fn project_status(data: &LibraryData) -> SemanticLibraryStatus {
    let library = data.policy.library();
    let model = library.model();
    SemanticLibraryStatus {
        available: true,
        revision: data.policy.revision(),
        paused: data.state.is_paused(),
        library: Some(SemanticLibraryIdentity {
            library_id: library.id().to_string(),
            model: SemanticLibraryModelIdentity {
                model_id: model.model_id().to_owned(),
                revision: model.revision().to_owned(),
                dimensions: model.dimensions(),
                embedding_space: model.embedding_space().to_owned(),
            },
        }),
        resource_profile: Some(SemanticResourceProfile {
            kind: data.policy.resource_profile().kind,
            budgets: data.policy.resource_profile().budgets,
        }),
        reconciliation_interval_seconds: Some(data.policy.reconciliation_interval_seconds()),
        roots: data
            .policy
            .roots()
            .values()
            .map(|root| project_root(root, data, data.state.eligibility_reason_counts(root.id())))
            .collect(),
        normalized_excerpts_retained_locally: true,
    }
}

fn project_root(
    root: &core::EnrolledRoot,
    data: &LibraryData,
    counts: Option<&core::EligibilityReasonCounts>,
) -> SemanticRootStatus {
    SemanticRootStatus {
        id: root.id().to_string(),
        location: root.location().clone(),
        recursive: root.recursive(),
        stable_identity_verified: root.filesystem_identity().is_some(),
        workspace_references: root
            .workspace_references()
            .iter()
            .copied()
            .map(Into::into)
            .collect(),
        eligibility_overrides: root
            .eligibility_overrides()
            .iter()
            .map(|(reason, action)| SemanticEligibilityOverrideStatus {
                reason: *reason,
                action: *action,
            })
            .collect(),
        attached_vocabulary_ids: root
            .vocabulary_ids()
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect(),
        eligibility_reason_counts: counts.map_or_else(Vec::new, reason_counts),
        availability: match data.catalog.root_availability(root.id()) {
            core::RootAvailability::Available => SemanticRootAvailability::Available,
            core::RootAvailability::TemporarilyUnavailable { .. } => {
                SemanticRootAvailability::TemporarilyUnavailable {
                    reason: "The source is currently unavailable.".to_owned(),
                }
            }
        },
        reconciliation_generation: data.catalog.reconciliation_generation(root.id()),
        indexed_generation: data.state.indexed_generation(root.id()),
        exclusions: root
            .exclusions()
            .iter()
            .map(|exclusion| SemanticExclusionStatus {
                id: exclusion.id().to_string(),
                location: exclusion.location().clone(),
                cleanup: exclusion.deletion_plan_id().map_or_else(
                    || SemanticCleanupStatus {
                        plan_id: None,
                        status: match exclusion.cleanup_status() {
                            core::ExclusionCleanupStatus::Pending => SemanticCleanupState::Pending,
                            core::ExclusionCleanupStatus::Complete => {
                                SemanticCleanupState::Complete
                            }
                        },
                        categories: empty_cleanup_categories(),
                    },
                    |plan_id| {
                        data.catalog.deletion_plan(plan_id).map_or_else(
                            || SemanticCleanupStatus {
                                plan_id: Some(plan_id.to_string()),
                                status: SemanticCleanupState::Pending,
                                categories: empty_cleanup_categories(),
                            },
                            cleanup_projection,
                        )
                    },
                ),
            })
            .collect(),
    }
}

fn project_folder_status(
    consent: core::ConsentState,
    policy: &core::SemanticLibraryPolicy,
    catalog: &core::SemanticCatalog,
    workspace_id: WorkspaceId,
) -> SemanticFolderStatus {
    let (consent, root_id, exclusion_id) = match consent {
        core::ConsentState::IncludedHere { root_id } => {
            (SemanticFolderConsent::IncludedHere, Some(root_id), None)
        }
        core::ConsentState::InheritedFromParent { root_id } => (
            SemanticFolderConsent::InheritedFromParent,
            Some(root_id),
            None,
        ),
        core::ConsentState::Excluded {
            root_id,
            exclusion_id,
        } => (
            SemanticFolderConsent::Excluded,
            Some(root_id),
            Some(exclusion_id),
        ),
        core::ConsentState::NotIncluded => {
            return SemanticFolderStatus {
                consent: SemanticFolderConsent::NotIncluded,
                root_id: None,
                exclusion_id: None,
                workspace_referenced: false,
                source_available: true,
                unavailable_reason: None,
            };
        }
    };
    let availability = root_id
        .map(|id| catalog.root_availability(id))
        .unwrap_or_default();
    let workspace_referenced = root_id
        .and_then(|id| policy.root(id))
        .is_some_and(|root| root.workspace_references().contains(&workspace_id));
    let (source_available, unavailable_reason) = match availability {
        core::RootAvailability::Available => (true, None),
        core::RootAvailability::TemporarilyUnavailable { .. } => (
            false,
            Some("The source is currently unavailable.".to_owned()),
        ),
    };
    SemanticFolderStatus {
        consent,
        root_id: root_id.map(|id| id.to_string()),
        exclusion_id: exclusion_id.map(|id| id.to_string()),
        workspace_referenced,
        source_available,
        unavailable_reason,
    }
}

fn reason_counts(counts: &core::EligibilityReasonCounts) -> Vec<SemanticEligibilityReasonCount> {
    counts
        .as_map()
        .iter()
        .map(|(reason, count)| SemanticEligibilityReasonCount {
            reason: *reason,
            count: *count,
        })
        .collect()
}

fn cleanup_projection(plan: &core::ExclusionDeletionPlan) -> SemanticCleanupStatus {
    SemanticCleanupStatus {
        plan_id: Some(plan.id().to_string()),
        status: match plan.status() {
            core::DeletionPlanStatus::Running => SemanticCleanupState::Running,
            core::DeletionPlanStatus::Failed => SemanticCleanupState::Failed,
            core::DeletionPlanStatus::Complete => SemanticCleanupState::Complete,
        },
        categories: cleanup_categories(plan),
    }
}

fn cleanup_categories(plan: &core::ExclusionDeletionPlan) -> Vec<SemanticDeletionCategoryStatus> {
    core::DeletionCategory::all()
        .iter()
        .copied()
        .map(|category| {
            let progress = plan.progress(category);
            SemanticDeletionCategoryStatus {
                category: map_deletion_category(category),
                total_items: progress.total_items(),
                completed_items: progress.completed_items(),
                complete: progress.is_complete(),
                last_error: progress
                    .last_error()
                    .map(|_| "Cleanup failed; retry is available.".to_owned()),
            }
        })
        .collect()
}

fn empty_cleanup_categories() -> Vec<SemanticDeletionCategoryStatus> {
    core::DeletionCategory::all()
        .iter()
        .copied()
        .map(|category| SemanticDeletionCategoryStatus {
            category: map_deletion_category(category),
            total_items: 0,
            completed_items: 0,
            complete: false,
            last_error: None,
        })
        .collect()
}

/// Returns the stable textual discriminator used inside idempotency keys.
///
/// It is deliberately independent of `Debug`, which is a diagnostic format and
/// may change; a persisted external deletion key may not.
const fn category_key(category: SemanticDeletionCategory) -> &'static str {
    match category {
        SemanticDeletionCategory::Occurrences => "occurrences",
        SemanticDeletionCategory::ExtractedContent => "extracted-content",
        SemanticDeletionCategory::Summaries => "summaries",
        SemanticDeletionCategory::Labels => "labels",
        SemanticDeletionCategory::OrphanVectors => "orphan-vectors",
        SemanticDeletionCategory::ConversationEvidencePins => "conversation-evidence-pins",
    }
}

const fn map_deletion_category(category: core::DeletionCategory) -> SemanticDeletionCategory {
    match category {
        core::DeletionCategory::Occurrences => SemanticDeletionCategory::Occurrences,
        core::DeletionCategory::ExtractedContent => SemanticDeletionCategory::ExtractedContent,
        core::DeletionCategory::Summaries => SemanticDeletionCategory::Summaries,
        core::DeletionCategory::Labels => SemanticDeletionCategory::Labels,
        core::DeletionCategory::OrphanVectors => SemanticDeletionCategory::OrphanVectors,
        core::DeletionCategory::ConversationEvidencePins => {
            SemanticDeletionCategory::ConversationEvidencePins
        }
    }
}

const fn is_safely_overridable(reason: core::EligibilityReason) -> bool {
    matches!(
        reason,
        core::EligibilityReason::Hidden
            | core::EligibilityReason::System
            | core::EligibilityReason::ApplicationOrPackageBundle
            | core::EligibilityReason::DependencyDirectory
            | core::EligibilityReason::BuildDirectory
            | core::EligibilityReason::CacheDirectory
            | core::EligibilityReason::GitIgnored
    )
}

const fn root_unavailability_message(reason: core::RootUnavailabilityReason) -> &'static str {
    match reason {
        core::RootUnavailabilityReason::UnprovenIdentity => {
            "The source identity could not be verified."
        }
        core::RootUnavailabilityReason::AmbiguousIdentity => {
            "More than one source has the enrolled identity."
        }
        core::RootUnavailabilityReason::CrossVolume => {
            "The source appears on another volume and needs confirmation."
        }
        core::RootUnavailabilityReason::PathReused => {
            "The enrolled path now refers to a different source."
        }
        core::RootUnavailabilityReason::CrossProvider => {
            "The source appears through another provider and needs confirmation."
        }
        core::RootUnavailabilityReason::Missing => "The source is currently unavailable.",
        core::RootUnavailabilityReason::RelocationConflict => {
            "The source move conflicts with another enrolled root."
        }
    }
}

fn map_store_error(_error: core::StoreError) -> SemanticLibraryError {
    SemanticLibraryError::Persistence
}

fn map_policy_error(error: core::PolicyError) -> SemanticLibraryError {
    match error {
        core::PolicyError::UnsafeEligibilityOverride => {
            SemanticLibraryError::UnsafeEligibilityOverride
        }
        core::PolicyError::UnknownRoot(_) | core::PolicyError::UnknownExclusion(_) => {
            SemanticLibraryError::NotFound
        }
        _ => SemanticLibraryError::InvalidRequest,
    }
}

fn map_deletion_error(_error: core::DeletionError) -> SemanticLibraryError {
    SemanticLibraryError::Cleanup
}

/// Semantic-library capability failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SemanticLibraryError {
    /// No library implementation is configured.
    #[error("semantic library is unavailable")]
    Unavailable,
    /// The active host authority does not permit this operation.
    #[error("semantic library operation is not permitted by this host")]
    AuthorityDenied {
        /// Active authority.
        authority: SemanticLibraryAuthority,
        /// Rejected operation.
        operation: SemanticLibraryOperation,
    },
    /// Authenticated tenant/library/user context is not authorized.
    #[error("semantic library access denied")]
    AccessDenied,
    /// Durable state belongs to a different library or embedding model than
    /// the one this host is configured for.
    #[error("semantic library storage belongs to a different library or model")]
    IncompatibleLibraryIdentity,
    /// Request data is malformed or violates policy.
    #[error("semantic library request is invalid")]
    InvalidRequest,
    /// The supplied workspace is not attached to the active root.
    #[error("an active workspace/root is required")]
    WorkspaceRequired,
    /// The requested folder is not enrolled.
    #[error("the folder is not included in the semantic library")]
    NotEnrolled,
    /// The requested folder is already explicitly excluded.
    #[error("the folder is already excluded from the semantic library")]
    AlreadyExcluded,
    /// A fixed eligibility reason cannot safely be overridden.
    #[error("this eligibility reason cannot be overridden")]
    UnsafeEligibilityOverride,
    /// Optimistic policy revision is stale.
    #[error("semantic library policy changed; refresh and try again")]
    StaleRevision {
        /// Revision submitted by the client.
        expected: u64,
        /// Current authoritative revision.
        actual: u64,
    },
    /// Opaque confirmation is missing, consumed, stale, or for another folder.
    #[error("semantic library confirmation is stale or invalid")]
    StaleConfirmation,
    /// A root, exclusion, or cleanup plan does not exist.
    #[error("semantic library item was not found")]
    NotFound,
    /// Durable policy/catalog/state synchronization failed.
    #[error("semantic library state could not be persisted")]
    Persistence,
    /// Destructive cleanup could not be completed.
    #[error("semantic library cleanup failed")]
    Cleanup,
    /// In-process state lock is unavailable.
    #[error("semantic library state is unavailable")]
    StateUnavailable,
    /// Optimistic revision cannot advance further.
    #[error("semantic library revision overflowed")]
    RevisionOverflow,
}

/// Parses a transport root identity.
pub fn parse_semantic_root_id(value: &str) -> Result<core::RootId, SemanticLibraryError> {
    core::RootId::from_str(value).map_err(|_| SemanticLibraryError::InvalidRequest)
}

/// Parses a transport cleanup-plan identity.
pub fn parse_semantic_deletion_plan_id(
    value: &str,
) -> Result<core::DeletionPlanId, SemanticLibraryError> {
    core::DeletionPlanId::from_str(value).map_err(|_| SemanticLibraryError::InvalidRequest)
}

/// Backend-authoritative inputs a desktop library is composed from.
///
/// Composition is only allowed to be cached while every one of these is
/// unchanged. Uninstalling components, moving the semantic-data root, or
/// migrating to another embedding model all change the library that *should*
/// exist, and a service cached across such a change would keep writing consent
/// and catalog records to the previous root or under the previous embedding
/// identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesktopCompositionKey {
    data_root: PathBuf,
    model_id: String,
    model_revision: String,
}

/// How a host composes its semantic-library capability.
///
/// Server and mock hosts are fixed at construction. A desktop host is
/// deliberately *not*: a device-local library needs a semantic-data root and an
/// exact embedding identity, and both only become known when the managed
/// components of task 0178 report an installed, activated model. Component
/// state is therefore re-read on every use and the composed service is keyed on
/// it, so installing, uninstalling, moving, or migrating components turns the
/// capability on, off, or over without a restart — and construction itself
/// stays completely inert.
pub(crate) enum SemanticLibraryComposition {
    Fixed(Arc<SemanticLibraryService>),
    DesktopManagedComponents {
        configuration_directory: PathBuf,
        resolved: tokio::sync::Mutex<Option<(DesktopCompositionKey, Arc<SemanticLibraryService>)>>,
    },
}

impl SemanticLibraryComposition {
    pub(crate) fn new(
        runtime: fm_transport_dto::RuntimeKindDto,
        configuration_directory: impl Into<PathBuf>,
    ) -> Self {
        match runtime {
            fm_transport_dto::RuntimeKindDto::Mock => {
                Self::fixed(SemanticLibraryService::deterministic_mock())
            }
            // Server libraries are administrator provisioned: until an
            // administrator supplies roots and an identity there is nothing to
            // report but the authority itself.
            fm_transport_dto::RuntimeKindDto::BrowserServer => {
                Self::fixed(SemanticLibraryService::administrator_provisioned_unconfigured())
            }
            fm_transport_dto::RuntimeKindDto::Tauri => Self::DesktopManagedComponents {
                configuration_directory: configuration_directory.into(),
                resolved: tokio::sync::Mutex::new(None),
            },
        }
    }

    pub(crate) fn fixed(service: SemanticLibraryService) -> Self {
        Self::Fixed(Arc::new(service))
    }

    pub(crate) async fn resolve(
        &self,
        components: &crate::semantic_components::SemanticComponentService,
    ) -> Arc<SemanticLibraryService> {
        match self {
            Self::Fixed(service) => Arc::clone(service),
            Self::DesktopManagedComponents {
                configuration_directory,
                resolved,
            } => {
                let mut resolved = resolved.lock().await;
                // Reading component status is a pure read of durable state,
                // so re-evaluating it per operation costs no writes and cannot
                // create or activate anything by itself.
                let key = desktop_composition_key(components).await;
                let Some(key) = key else {
                    // Components were uninstalled, disabled, or never
                    // installed: drop any composed service so the capability
                    // reports itself unavailable instead of continuing to
                    // write to a root that is no longer managed.
                    *resolved = None;
                    return Arc::new(SemanticLibraryService::unavailable());
                };
                if let Some((cached_key, service)) = resolved.as_ref()
                    && *cached_key == key
                {
                    return Arc::clone(service);
                }
                let service = Arc::new(
                    desktop_library_from_components(components, configuration_directory)
                        .await
                        .unwrap_or_else(SemanticLibraryService::unavailable),
                );
                if matches!(
                    service.unresolved_authority(),
                    SemanticLibraryAuthority::Unavailable
                ) {
                    *resolved = None;
                } else {
                    *resolved = Some((key, Arc::clone(&service)));
                }
                service
            }
        }
    }
}

/// Returns the authoritative composition inputs, or `None` when no desktop
/// library may exist right now.
async fn desktop_composition_key(
    components: &crate::semantic_components::SemanticComponentService,
) -> Option<DesktopCompositionKey> {
    use crate::semantic_components::SemanticComponentLifecycle;

    let status = components.status().await.ok()?;
    if !matches!(
        status.lifecycle(),
        SemanticComponentLifecycle::InstalledEnabled
            | SemanticComponentLifecycle::Paused
            | SemanticComponentLifecycle::Migrating { .. }
    ) {
        return None;
    }
    let active = status.active_model()?;
    Some(DesktopCompositionKey {
        data_root: status.data_root()?.to_path_buf(),
        model_id: active.identity().model_id().to_owned(),
        model_revision: active.identity().revision().to_owned(),
    })
}

/// Derives a desktop library from installed managed components.
///
/// Every input is backend-authoritative: the semantic-data root and active
/// model come from durable component status, and the embedding dimensions and
/// space come from the signed catalog metadata for that exact revision. If any
/// of them is missing the capability stays unavailable — an invented root or a
/// guessed embedding space would silently produce an unqueryable library.
async fn desktop_library_from_components(
    components: &crate::semantic_components::SemanticComponentService,
    configuration_directory: &std::path::Path,
) -> Option<SemanticLibraryService> {
    let key = desktop_composition_key(components).await?;
    let profiles = components.catalog_profiles().await.ok()?;
    let metadata = profiles
        .iter()
        .map(|profile| &profile.metadata)
        .find(|metadata| {
            metadata.identity.model_id() == key.model_id
                && metadata.identity.revision() == key.model_revision
        })?;
    // The library id is derived from the immutable embedding identity, so the
    // same installed model always addresses the same device-local library and a
    // model migration cannot silently reuse another embedding space's records.
    let library_id = Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!(
            "procyon-semantic-library:{}:{}:{}",
            metadata.identity.model_id(),
            metadata.identity.revision(),
            metadata.dimensions
        )
        .as_bytes(),
    );
    SemanticLibraryService::desktop_managed(
        SemanticLibraryConfiguration::balanced(
            configuration_directory,
            key.data_root.join("library"),
            library_id,
            metadata.identity.model_id(),
            metadata.identity.revision(),
            metadata.dimensions,
            format!(
                "{}@{}",
                metadata.identity.model_id(),
                metadata.identity.revision()
            ),
        )
        .ok()?,
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .ok()
}
