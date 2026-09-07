//! Provider-neutral semantic library consent policy and catalog core.

mod authorization;
mod catalog;
mod consent;
mod deletion;
mod eligibility;
mod feed;
mod hierarchy;
mod identity;
mod ids;
mod journal;
mod lock;
mod policy;
mod preview;
mod state;
mod store;

pub use authorization::{
    AccessContext, AuthorizationError, HardQuotas, QuotaUsage, SemanticQueryRequest,
    ServerEnrolmentPolicy, ServerPolicy, TenantLibrary,
};
pub use catalog::{
    CURRENT_CATALOG_SCHEMA_VERSION, CatalogError, CatalogObservation, CatalogUsage,
    ContentFingerprint, DocumentArtifacts, DocumentMeasurement, DocumentRecord, OccurrenceRecord,
    OccurrenceScope, RootAvailability, ScopedDocument, ScopedSource, SemanticCatalog,
    SourceAvailability,
};
pub use consent::ConsentState;
pub use deletion::{
    ConversationEvidencePin, DeletionCategory, DeletionCategoryProgress, DeletionError,
    DeletionPlanStatus, ExclusionDeletionInventory, ExclusionDeletionPlan,
};
pub use eligibility::{
    EligibilityCandidate, EligibilityDecision, EligibilityEntryKind, EligibilityError,
    EligibilityOverride, EligibilityPolicy, EligibilityReason, EligibilityReasonCounts,
    ResourceUsage, symlink_target_is_confined,
};
pub use feed::{WorkerFeedAction, WorkerFeedDecision};
pub use identity::{ObservedRootIdentity, RootMoveResolution, RootUnavailabilityReason};
pub use ids::{
    ConversationPinId, DeletionPlanId, DerivedArtifactId, DocumentId, ExclusionId, IdentifierError,
    LibraryId, OccurrenceId, RootId, TenantId, UserId, VocabularyId,
};
pub use journal::{
    CommitStep, LibraryOperation, LibrarySession, LibraryTransaction, LoadedLibrary,
    RecoveredTransaction, RecoveryOutcome, SemanticLibraryCoordinator, TransactionId,
    TransactionParticipant,
};
pub use lock::LibraryLockGuard;
pub use policy::{
    CURRENT_POLICY_SCHEMA_VERSION, DescendantExclusion, DeviceLibraryIdentity, EnrolledRoot,
    ExclusionCleanupStatus, FilesystemIdentity, ModelIdentity, PolicyError, ResourceBudgets,
    ResourceProfile, ResourceProfileKind, SemanticLibraryPolicy,
};
pub use preview::{
    BudgetAssessment, BudgetKind, EnrolmentEstimate, EnrolmentEstimator, EnrolmentPreview,
    EstimateError, PreviewError,
};
pub use state::{CURRENT_LIBRARY_STATE_SCHEMA_VERSION, LibraryStateError, SemanticLibraryState};
pub use store::{
    POLICY_FILE_NAME, SemanticCatalogStore, SemanticLibraryPolicyStore, SemanticLibraryStateStore,
    StoreError,
};
