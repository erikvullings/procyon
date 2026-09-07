#![allow(clippy::unwrap_used, missing_docs)]

use std::path::Path;

use fm_domain::{EntryId, Location, WorkspaceId};
use fm_semantic_library::{
    CatalogError, CatalogObservation, ContentFingerprint, DerivedArtifactId, DeviceLibraryIdentity,
    DocumentArtifacts, DocumentMeasurement, EnrolledRoot, LibraryId, ModelIdentity,
    OccurrenceScope, ResourceBudgets, ResourceProfile, ResourceProfileKind, RootId,
    SemanticCatalog, SemanticCatalogStore, SemanticLibraryPolicy, SemanticLibraryState,
    SourceAvailability, TenantId,
};
use tempfile::TempDir;
use uuid::Uuid;

fn location(uri: &str) -> Location {
    Location::parse(uri).expect("valid location")
}

fn workspace(value: u128) -> WorkspaceId {
    WorkspaceId::from(Uuid::from_u128(value))
}

fn root(value: u128) -> RootId {
    RootId::from_uuid(Uuid::from_u128(value))
}

fn project_temp_dir(prefix: &str) -> TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/semantic-library-tests");
    std::fs::create_dir_all(&parent).expect("create project-local test directory");
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)
        .expect("create project-local temporary directory")
}

fn policy_with_overlapping_roots() -> (SemanticLibraryPolicy, RootId, RootId) {
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
    let parent_id = root(10);
    let nested_id = root(11);
    let mut parent = EnrolledRoot::new(parent_id, location("file:///docs"), None, true);
    parent.attach_workspace(workspace(20));
    parent.attach_workspace(workspace(21));
    let mut nested = EnrolledRoot::new(nested_id, location("file:///docs/team"), None, true);
    nested.attach_workspace(workspace(20));
    nested.attach_workspace(workspace(21));
    policy.enrol_root(parent).unwrap();
    policy.enrol_root(nested).unwrap();
    (policy, parent_id, nested_id)
}

fn measurement() -> DocumentMeasurement {
    DocumentMeasurement::new(64, 32, 16)
}

fn active(policy: &SemanticLibraryPolicy) -> SemanticLibraryState {
    SemanticLibraryState::new(policy.library().id())
}

fn artifacts(seed: u128) -> DocumentArtifacts {
    DocumentArtifacts {
        extracted_content: [DerivedArtifactId::from_uuid(Uuid::from_u128(seed))].into(),
        summaries: [DerivedArtifactId::from_uuid(Uuid::from_u128(seed + 1))].into(),
        labels: [DerivedArtifactId::from_uuid(Uuid::from_u128(seed + 2))].into(),
        vectors: [DerivedArtifactId::from_uuid(Uuid::from_u128(seed + 3))].into(),
    }
}

#[test]
fn unsorted_interleaved_overlapping_roots_deduplicate_content_and_preserve_scopes() {
    let (policy, parent, nested) = policy_with_overlapping_roots();
    let mut catalog = SemanticCatalog::new(policy.library().id());
    let shared_fingerprint = ContentFingerprint::new("sha256:shared").unwrap();
    let entry_a = EntryId::from(Uuid::from_u128(100));
    let entry_b = EntryId::from(Uuid::from_u128(101));
    let observations = vec![
        CatalogObservation::new(
            entry_b,
            location("file:///docs/team/b.txt"),
            shared_fingerprint.clone(),
            OccurrenceScope::new(workspace(21), nested),
            artifacts(1000),
            measurement(),
        ),
        CatalogObservation::new(
            entry_a,
            location("file:///docs/a.txt"),
            shared_fingerprint.clone(),
            OccurrenceScope::new(workspace(20), parent),
            artifacts(1000),
            measurement(),
        ),
        CatalogObservation::new(
            entry_b,
            location("file:///docs/team/b.txt"),
            shared_fingerprint.clone(),
            OccurrenceScope::new(workspace(20), parent),
            artifacts(1000),
            measurement(),
        ),
        CatalogObservation::new(
            entry_b,
            location("file:///docs/team/b.txt"),
            shared_fingerprint,
            OccurrenceScope::new(workspace(20), nested),
            artifacts(1000),
            measurement(),
        ),
    ];

    catalog
        .upsert_observations(&policy, &active(&policy), observations)
        .expect("catalog observations");

    assert_eq!(catalog.document_count(), 1);
    assert_eq!(catalog.occurrence_count(), 2);
    let occurrence = catalog
        .occurrence_by_entry("local", entry_b)
        .expect("shared occurrence");
    assert_eq!(occurrence.scopes().len(), 3);
    assert!(
        occurrence
            .scopes()
            .contains(&OccurrenceScope::new(workspace(20), parent))
    );
    assert!(
        occurrence
            .scopes()
            .contains(&OccurrenceScope::new(workspace(20), nested))
    );
    assert!(
        occurrence
            .scopes()
            .contains(&OccurrenceScope::new(workspace(21), nested))
    );
}

#[test]
fn deleting_a_workspace_removes_only_scope_references_not_consent_or_content() {
    let (mut policy, parent, _) = policy_with_overlapping_roots();
    let mut catalog = SemanticCatalog::new(policy.library().id());
    let entry = EntryId::from(Uuid::from_u128(200));
    for workspace_id in [workspace(20), workspace(21)] {
        catalog
            .upsert_observations(
                &policy,
                &active(&policy),
                [CatalogObservation::new(
                    entry,
                    location("file:///docs/shared.txt"),
                    ContentFingerprint::new("sha256:workspace-shared").unwrap(),
                    OccurrenceScope::new(workspace_id, parent),
                    artifacts(2000),
                    measurement(),
                )],
            )
            .unwrap();
    }

    policy.remove_workspace_reference(workspace(20));
    catalog.remove_workspace_scopes(workspace(20));

    assert!(policy.root(parent).is_some(), "root consent must remain");
    assert_eq!(
        policy.root(parent).unwrap().workspace_references(),
        &[workspace(21)]
    );
    assert_eq!(catalog.document_count(), 1);
    assert_eq!(catalog.occurrence_count(), 1);
    assert_eq!(
        catalog
            .occurrence_by_entry("local", entry)
            .unwrap()
            .scopes(),
        &[OccurrenceScope::new(workspace(21), parent)]
    );
}

#[test]
fn unavailable_roots_retain_evidence_until_a_successful_complete_reconciliation() {
    let (policy, parent, _) = policy_with_overlapping_roots();
    let mut catalog = SemanticCatalog::new(policy.library().id());
    let entry = EntryId::from(Uuid::from_u128(300));
    catalog
        .upsert_observations(
            &policy,
            &active(&policy),
            [CatalogObservation::new(
                entry,
                location("file:///docs/offline.txt"),
                ContentFingerprint::new("sha256:offline").unwrap(),
                OccurrenceScope::new(workspace(20), parent),
                artifacts(3000),
                measurement(),
            )],
        )
        .unwrap();
    let occurrence_id = catalog.occurrence_by_entry("local", entry).unwrap().id();

    catalog.mark_root_unavailable(parent, "volume not mounted");

    assert_eq!(catalog.document_count(), 1);
    assert_eq!(catalog.occurrence_count(), 1);
    assert_eq!(
        catalog.source_availability(occurrence_id).unwrap(),
        SourceAvailability::Unavailable
    );
    assert_eq!(catalog.reconciliation_generation(parent), 0);
    assert!(matches!(
        catalog.complete_reconciliation(&policy, &active(&policy), parent, &Default::default()),
        Err(CatalogError::RootUnavailable(_))
    ));
    assert_eq!(catalog.occurrence_count(), 1);

    catalog.mark_root_available(parent);
    catalog
        .complete_reconciliation(&policy, &active(&policy), parent, &Default::default())
        .expect("complete empty scan");

    assert_eq!(catalog.document_count(), 0);
    assert_eq!(catalog.occurrence_count(), 0);
    assert_eq!(catalog.reconciliation_generation(parent), 1);
}

#[test]
fn high_volume_catalog_is_atomically_persisted_under_the_semantic_data_root() {
    let (policy, parent, _) = policy_with_overlapping_roots();
    let mut catalog = SemanticCatalog::new(policy.library().id());
    catalog
        .upsert_observations(
            &policy,
            &active(&policy),
            [CatalogObservation::new(
                EntryId::from(Uuid::from_u128(400)),
                location("file:///docs/persisted.txt"),
                ContentFingerprint::new("sha256:persisted").unwrap(),
                OccurrenceScope::new(workspace(20), parent),
                artifacts(4000),
                measurement(),
            )],
        )
        .unwrap();
    let directory = project_temp_dir("catalog-store-");
    let store = SemanticCatalogStore::new(directory.path());

    store.save(&catalog).expect("save catalog");

    assert_eq!(store.load().expect("load catalog"), catalog);
    assert_eq!(
        store.path(),
        directory.path().join("catalog").join("catalog.json")
    );
}

#[test]
fn worker_feed_decisions_are_opaque_and_never_contain_filesystem_paths() {
    let (policy, parent, _) = policy_with_overlapping_roots();
    let mut catalog = SemanticCatalog::new(policy.library().id());
    catalog
        .upsert_observations(
            &policy,
            &active(&policy),
            [CatalogObservation::new(
                EntryId::from(Uuid::from_u128(500)),
                location("file:///docs/secret-name.txt"),
                ContentFingerprint::new("sha256:worker").unwrap(),
                OccurrenceScope::new(workspace(20), parent),
                DocumentArtifacts::default(),
                measurement(),
            )],
        )
        .unwrap();

    let decisions = catalog
        .worker_feed_decisions(
            &policy,
            &active(&policy),
            TenantId::new("device-local").unwrap(),
        )
        .unwrap();
    let serialized = serde_json::to_string(&decisions).unwrap();

    assert_eq!(decisions.len(), 1);
    assert!(!serialized.contains("file:///"));
    assert!(!serialized.contains("secret-name"));
    assert!(!serialized.contains("location"));
}

#[test]
fn rejected_observation_batch_does_not_partially_mutate_the_catalog() {
    let (policy, parent, _) = policy_with_overlapping_roots();
    let mut catalog = SemanticCatalog::new(policy.library().id());

    let result = catalog.upsert_observations(
        &policy,
        &active(&policy),
        [
            CatalogObservation::new(
                EntryId::from(Uuid::from_u128(600)),
                location("file:///docs/valid.txt"),
                ContentFingerprint::new("sha256:valid").unwrap(),
                OccurrenceScope::new(workspace(20), parent),
                DocumentArtifacts::default(),
                measurement(),
            ),
            CatalogObservation::new(
                EntryId::from(Uuid::from_u128(601)),
                location("file:///docs/invalid.txt"),
                ContentFingerprint::new("sha256:invalid").unwrap(),
                OccurrenceScope::new(workspace(20), root(999)),
                DocumentArtifacts::default(),
                measurement(),
            ),
        ],
    );

    assert!(result.is_err());
    assert_eq!(catalog.document_count(), 0);
    assert_eq!(catalog.occurrence_count(), 0);
}

#[test]
fn distinct_paths_with_the_same_filesystem_entry_identity_remain_distinct_occurrences() {
    let (policy, parent, _) = policy_with_overlapping_roots();
    let mut catalog = SemanticCatalog::new(policy.library().id());
    let shared_entry_id = EntryId::from(Uuid::from_u128(700));

    catalog
        .upsert_observations(
            &policy,
            &active(&policy),
            [
                CatalogObservation::new(
                    shared_entry_id,
                    location("file:///docs/hardlink-a.txt"),
                    ContentFingerprint::new("sha256:hardlink").unwrap(),
                    OccurrenceScope::new(workspace(20), parent),
                    DocumentArtifacts::default(),
                    measurement(),
                ),
                CatalogObservation::new(
                    shared_entry_id,
                    location("file:///docs/hardlink-b.txt"),
                    ContentFingerprint::new("sha256:hardlink").unwrap(),
                    OccurrenceScope::new(workspace(20), parent),
                    DocumentArtifacts::default(),
                    measurement(),
                ),
            ],
        )
        .unwrap();

    assert_eq!(catalog.document_count(), 1);
    assert_eq!(catalog.occurrence_count(), 2);
}
