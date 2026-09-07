#![allow(clippy::unwrap_used, missing_docs)]

use fm_domain::{EntryId, Location, WorkspaceId};
use fm_semantic_library::{
    AccessContext, AuthorizationError, CatalogError, CatalogObservation, ContentFingerprint,
    DeletionCategory, DeletionError, DeviceLibraryIdentity, DocumentArtifacts, DocumentMeasurement,
    EnrolledRoot, ExclusionId, FilesystemIdentity, HardQuotas, LibraryId, ModelIdentity,
    ObservedRootIdentity, OccurrenceScope, PolicyError, ResourceBudgets, ResourceProfile,
    ResourceProfileKind, RootId, SemanticCatalog, SemanticLibraryPolicy, SemanticLibraryState,
    ServerEnrolmentPolicy, ServerPolicy, TenantId, UserId,
};
use uuid::Uuid;

fn location(uri: &str) -> Location {
    Location::parse(uri).unwrap()
}

fn root_id() -> RootId {
    RootId::from_uuid(Uuid::from_u128(10))
}

fn workspace_id() -> WorkspaceId {
    WorkspaceId::from(Uuid::from_u128(20))
}

fn exclusion_id() -> ExclusionId {
    ExclusionId::from_uuid(Uuid::from_u128(30))
}

fn policy_for(library_id: LibraryId) -> SemanticLibraryPolicy {
    let mut policy = SemanticLibraryPolicy::new(
        DeviceLibraryIdentity::new(
            library_id,
            ModelIdentity::new("model", "revision", 384, "space").unwrap(),
        ),
        ResourceProfile {
            kind: ResourceProfileKind::Balanced,
            budgets: ResourceBudgets::default(),
        },
    )
    .unwrap();
    let mut root = EnrolledRoot::new(
        root_id(),
        location("file:///docs"),
        Some(FilesystemIdentity::new("volume-a", "directory-7").unwrap()),
        true,
    );
    root.attach_workspace(workspace_id());
    policy.enrol_root(root).unwrap();
    policy
}

fn seeded() -> (SemanticLibraryPolicy, SemanticCatalog, SemanticLibraryState) {
    let library_id = LibraryId::from_uuid(Uuid::from_u128(1));
    let policy = policy_for(library_id);
    let state = SemanticLibraryState::new(library_id);
    let mut catalog = SemanticCatalog::new(library_id);
    catalog
        .upsert_observations(
            &policy,
            &state,
            [CatalogObservation::new(
                EntryId::from(Uuid::from_u128(100)),
                location("file:///docs/private/secret.txt"),
                ContentFingerprint::new("sha256:secret").unwrap(),
                OccurrenceScope::new(workspace_id(), root_id()),
                DocumentArtifacts::default(),
                DocumentMeasurement::new(64, 32, 16),
            )],
        )
        .unwrap();
    (policy, catalog, state)
}

fn foreign_policy() -> SemanticLibraryPolicy {
    policy_for(LibraryId::from_uuid(Uuid::from_u128(999)))
}

#[test]
fn a_foreign_policy_cannot_ingest_feed_or_reconcile() {
    let (_, mut catalog, state) = seeded();
    let foreign = foreign_policy();

    let ingested = catalog.upsert_observations(
        &foreign,
        &state,
        [CatalogObservation::new(
            EntryId::from(Uuid::from_u128(101)),
            location("file:///docs/injected.txt"),
            ContentFingerprint::new("sha256:injected").unwrap(),
            OccurrenceScope::new(workspace_id(), root_id()),
            DocumentArtifacts::default(),
            DocumentMeasurement::new(1, 1, 1),
        )],
    );
    let fed =
        catalog.worker_feed_decisions(&foreign, &state, TenantId::new("device-local").unwrap());
    let reconciled =
        catalog.complete_reconciliation(&foreign, &state, root_id(), &Default::default());
    let mut moved = foreign_policy();
    let relocated = catalog.reconcile_root_location(
        &mut moved,
        root_id(),
        &[ObservedRootIdentity::new(
            location("file:///elsewhere"),
            Some(FilesystemIdentity::new("volume-a", "directory-7").unwrap()),
        )],
    );

    assert!(matches!(ingested, Err(CatalogError::LibraryMismatch)));
    assert!(matches!(fed, Err(CatalogError::LibraryMismatch)));
    assert!(matches!(reconciled, Err(CatalogError::LibraryMismatch)));
    assert!(matches!(relocated, Err(CatalogError::LibraryMismatch)));
    assert_eq!(
        moved.root(root_id()).unwrap().location(),
        &location("file:///docs")
    );
    assert_eq!(catalog.occurrence_count(), 1);
    assert_eq!(catalog.document_count(), 1);
}

#[test]
fn a_foreign_runtime_state_cannot_drive_ingestion_or_reconciliation() {
    let (policy, mut catalog, _) = seeded();
    let foreign_state = SemanticLibraryState::new(LibraryId::from_uuid(Uuid::from_u128(999)));

    let ingested = catalog.upsert_observations(
        &policy,
        &foreign_state,
        [CatalogObservation::new(
            EntryId::from(Uuid::from_u128(101)),
            location("file:///docs/injected.txt"),
            ContentFingerprint::new("sha256:injected").unwrap(),
            OccurrenceScope::new(workspace_id(), root_id()),
            DocumentArtifacts::default(),
            DocumentMeasurement::new(1, 1, 1),
        )],
    );
    let reconciled =
        catalog.complete_reconciliation(&policy, &foreign_state, root_id(), &Default::default());

    assert!(matches!(ingested, Err(CatalogError::LibraryMismatch)));
    assert!(matches!(reconciled, Err(CatalogError::LibraryMismatch)));
    assert_eq!(catalog.occurrence_count(), 1);
}

#[test]
fn a_foreign_policy_cannot_plan_or_apply_destructive_cleanup() {
    let (mut policy, mut catalog, _) = seeded();
    policy
        .exclude_descendant(root_id(), exclusion_id(), location("file:///docs/private"))
        .unwrap();
    let plan_id = catalog
        .begin_exclusion_cleanup(&mut policy, root_id(), exclusion_id())
        .unwrap();
    let mut foreign = foreign_policy();
    foreign
        .exclude_descendant(root_id(), exclusion_id(), location("file:///docs/private"))
        .unwrap();

    let planned = catalog.begin_exclusion_cleanup(&mut foreign, root_id(), exclusion_id());
    let applied =
        catalog.complete_deletion_category(&mut foreign, plan_id, DeletionCategory::Occurrences);

    assert!(matches!(
        planned,
        Err(DeletionError::Policy(PolicyError::LibraryMismatch))
    ));
    assert!(matches!(
        applied,
        Err(DeletionError::Policy(PolicyError::LibraryMismatch))
    ));
    assert_eq!(catalog.occurrence_count(), 1);
    assert_eq!(catalog.document_count(), 1);
    assert_eq!(catalog.conversation_pin_count(), 0);
}

#[test]
fn an_access_context_for_another_library_cannot_ingest_or_enrol() {
    let (policy, mut catalog, state) = seeded();
    let admin = UserId::new("admin-a").unwrap();
    let tenant = TenantId::new("tenant-a").unwrap();
    let server = ServerPolicy::single_private(
        tenant.clone(),
        LibraryId::from_uuid(Uuid::from_u128(999)),
        admin.clone(),
        ServerEnrolmentPolicy::local_only(),
        HardQuotas::default(),
    );
    let foreign_access = AccessContext::new(
        tenant,
        LibraryId::from_uuid(Uuid::from_u128(999)),
        admin.clone(),
    );

    let ingested = catalog.ingest_for_tenant(
        &policy,
        &state,
        &server,
        &foreign_access,
        [CatalogObservation::new(
            EntryId::from(Uuid::from_u128(101)),
            location("file:///docs/injected.txt"),
            ContentFingerprint::new("sha256:injected").unwrap(),
            OccurrenceScope::new(workspace_id(), root_id()),
            DocumentArtifacts::default(),
            DocumentMeasurement::new(1, 1, 1),
        )],
    );
    let enrolled = server.authorize_root_enrolment(&foreign_access, "local", &policy, &catalog);

    assert!(matches!(ingested, Err(CatalogError::LibraryMismatch)));
    assert_eq!(enrolled, Err(AuthorizationError::LibraryDenied));
    assert_eq!(catalog.occurrence_count(), 1);
}
