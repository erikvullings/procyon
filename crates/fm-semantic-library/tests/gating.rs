#![allow(clippy::unwrap_used, missing_docs)]

use fm_domain::{EntryId, Location, WorkspaceId};
use fm_semantic_library::{
    CatalogError, CatalogObservation, ContentFingerprint, DeviceLibraryIdentity, DocumentArtifacts,
    DocumentMeasurement, EnrolledRoot, FilesystemIdentity, LibraryId, ModelIdentity,
    ObservedRootIdentity, OccurrenceScope, ResourceBudgets, ResourceProfile, ResourceProfileKind,
    RootId, RootMoveResolution, RootUnavailabilityReason, SemanticCatalog, SemanticLibraryPolicy,
    SemanticLibraryState, TenantId,
};
use uuid::Uuid;

fn location(uri: &str) -> Location {
    Location::parse(uri).unwrap()
}

fn measurement() -> DocumentMeasurement {
    DocumentMeasurement::new(128, 64, 32)
}

fn tenant() -> TenantId {
    TenantId::new("device-local").unwrap()
}

const WORKSPACE: u128 = 20;
const ROOT: u128 = 10;

fn library() -> (SemanticLibraryPolicy, RootId, WorkspaceId) {
    let root_id = RootId::from_uuid(Uuid::from_u128(ROOT));
    let workspace_id = WorkspaceId::from(Uuid::from_u128(WORKSPACE));
    let mut policy = SemanticLibraryPolicy::new(
        DeviceLibraryIdentity::new(
            LibraryId::from_uuid(Uuid::from_u128(1)),
            ModelIdentity::new("model", "revision", 384, "space").unwrap(),
        ),
        ResourceProfile {
            kind: ResourceProfileKind::Balanced,
            budgets: ResourceBudgets::default(),
        },
    )
    .unwrap();
    let mut root = EnrolledRoot::new(
        root_id,
        location("file:///docs"),
        Some(FilesystemIdentity::new("volume-a", "directory-7").unwrap()),
        true,
    );
    root.attach_workspace(workspace_id);
    policy.enrol_root(root).unwrap();
    (policy, root_id, workspace_id)
}

fn observation(
    name: &str,
    entry: u128,
    fingerprint: &str,
    root_id: RootId,
    workspace_id: WorkspaceId,
) -> CatalogObservation {
    CatalogObservation::new(
        EntryId::from(Uuid::from_u128(entry)),
        location(name),
        ContentFingerprint::new(fingerprint).unwrap(),
        OccurrenceScope::new(workspace_id, root_id),
        DocumentArtifacts::default(),
        measurement(),
    )
}

#[test]
fn pause_stops_ingestion_and_feeding_while_preserving_indexed_generations() {
    let (policy, root_id, workspace_id) = library();
    let mut catalog = SemanticCatalog::new(policy.library().id());
    let mut state = SemanticLibraryState::new(policy.library().id());
    catalog
        .upsert_observations(
            &policy,
            &state,
            [observation(
                "file:///docs/indexed.txt",
                100,
                "sha256:indexed",
                root_id,
                workspace_id,
            )],
        )
        .unwrap();
    let generation = catalog
        .complete_reconciliation(
            &policy,
            &state,
            root_id,
            &catalog.occurrences().map(|entry| entry.id()).collect(),
        )
        .unwrap();
    state
        .record_indexed_generation(root_id, generation)
        .unwrap();

    state.pause();

    assert!(matches!(
        catalog.upsert_observations(
            &policy,
            &state,
            [observation(
                "file:///docs/late.txt",
                101,
                "sha256:late",
                root_id,
                workspace_id,
            )],
        ),
        Err(CatalogError::IngestionPaused)
    ));
    assert!(matches!(
        catalog.worker_feed_decisions(&policy, &state, tenant()),
        Err(CatalogError::IngestionPaused)
    ));
    assert!(matches!(
        catalog.complete_reconciliation(&policy, &state, root_id, &Default::default()),
        Err(CatalogError::IngestionPaused)
    ));
    assert_eq!(catalog.occurrence_count(), 1);
    assert_eq!(catalog.document_count(), 1);
    assert_eq!(state.indexed_generation(root_id), generation);
    assert_eq!(catalog.reconciliation_generation(root_id), generation);
}

#[test]
fn an_unavailable_root_never_accepts_or_feeds_new_content() {
    let (policy, root_id, workspace_id) = library();
    let mut catalog = SemanticCatalog::new(policy.library().id());
    let state = SemanticLibraryState::new(policy.library().id());
    catalog
        .upsert_observations(
            &policy,
            &state,
            [observation(
                "file:///docs/retained.txt",
                200,
                "sha256:retained",
                root_id,
                workspace_id,
            )],
        )
        .unwrap();

    catalog.mark_root_unavailable(root_id, "volume not mounted");

    assert!(matches!(
        catalog.upsert_observations(
            &policy,
            &state,
            [observation(
                "file:///docs/new.txt",
                201,
                "sha256:new",
                root_id,
                workspace_id,
            )],
        ),
        Err(CatalogError::RootUnavailable(unavailable)) if unavailable == root_id
    ));
    assert!(
        catalog
            .worker_feed_decisions(&policy, &state, tenant())
            .unwrap()
            .is_empty()
    );
    assert_eq!(catalog.occurrence_count(), 1);

    catalog.mark_root_available(root_id);

    assert_eq!(
        catalog
            .worker_feed_decisions(&policy, &state, tenant())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_reused_path_stops_ingestion_and_feeding_without_deleting_evidence() {
    let (mut policy, root_id, workspace_id) = library();
    let mut catalog = SemanticCatalog::new(policy.library().id());
    let state = SemanticLibraryState::new(policy.library().id());
    catalog
        .upsert_observations(
            &policy,
            &state,
            [observation(
                "file:///docs/original.txt",
                300,
                "sha256:original",
                root_id,
                workspace_id,
            )],
        )
        .unwrap();

    let resolution = catalog
        .reconcile_root_location(
            &mut policy,
            root_id,
            &[ObservedRootIdentity::new(
                location("file:///docs"),
                Some(FilesystemIdentity::new("volume-a", "replacement-directory").unwrap()),
            )],
        )
        .unwrap();

    assert_eq!(
        resolution,
        RootMoveResolution::RetainedUnavailable {
            reason: RootUnavailabilityReason::PathReused,
        }
    );
    assert!(matches!(
        catalog.upsert_observations(
            &policy,
            &state,
            [observation(
                "file:///docs/impostor.txt",
                301,
                "sha256:impostor",
                root_id,
                workspace_id,
            )],
        ),
        Err(CatalogError::RootUnavailable(unavailable)) if unavailable == root_id
    ));
    assert!(
        catalog
            .worker_feed_decisions(&policy, &state, tenant())
            .unwrap()
            .is_empty()
    );
    assert_eq!(catalog.occurrence_count(), 1);
    assert_eq!(catalog.document_count(), 1);
}
