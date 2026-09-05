#![allow(clippy::unwrap_used, missing_docs)]

use std::path::Path;

use fm_domain::{EntryId, Location, WorkspaceId};
use fm_semantic_library::{
    CatalogObservation, ConsentState, ContentFingerprint, ConversationEvidencePin,
    ConversationPinId, DeletionCategory, DeletionPlanStatus, DerivedArtifactId,
    DeviceLibraryIdentity, DocumentArtifacts, DocumentMeasurement, EnrolledRoot,
    ExclusionCleanupStatus, ExclusionId, LibraryId, ModelIdentity, OccurrenceScope,
    ResourceBudgets, ResourceProfile, ResourceProfileKind, RootId, SemanticCatalog,
    SemanticCatalogStore, SemanticLibraryPolicy, SemanticLibraryState,
};
use tempfile::TempDir;
use uuid::Uuid;

fn location(uri: &str) -> Location {
    Location::parse(uri).unwrap()
}

fn project_temp_dir(prefix: &str) -> TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/semantic-library-tests");
    std::fs::create_dir_all(&parent).unwrap();
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)
        .unwrap()
}

fn artifact(value: u128) -> DerivedArtifactId {
    DerivedArtifactId::from_uuid(Uuid::from_u128(value))
}

fn measurement() -> DocumentMeasurement {
    DocumentMeasurement::new(64, 32, 16)
}

#[test]
fn exclusion_is_immediate_and_deletion_with_pins_is_durable_resumable_and_share_safe() {
    let library_id = LibraryId::from_uuid(Uuid::from_u128(1));
    let root_a = RootId::from_uuid(Uuid::from_u128(10));
    let root_b = RootId::from_uuid(Uuid::from_u128(11));
    let workspace_a = WorkspaceId::from(Uuid::from_u128(20));
    let workspace_b = WorkspaceId::from(Uuid::from_u128(21));
    let exclusion_id = ExclusionId::from_uuid(Uuid::from_u128(30));
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
    let mut enrolled_a = EnrolledRoot::new(root_a, location("file:///docs"), None, true);
    enrolled_a.attach_workspace(workspace_a);
    let mut enrolled_b = EnrolledRoot::new(root_b, location("file:///other"), None, true);
    enrolled_b.attach_workspace(workspace_b);
    policy.enrol_root(enrolled_a).unwrap();
    policy.enrol_root(enrolled_b).unwrap();

    let shared_inside = EntryId::from(Uuid::from_u128(100));
    let shared_outside = EntryId::from(Uuid::from_u128(101));
    let orphan_inside = EntryId::from(Uuid::from_u128(102));
    let shared_artifacts = DocumentArtifacts {
        extracted_content: [artifact(1000)].into(),
        summaries: [artifact(1001)].into(),
        labels: [artifact(1002)].into(),
        vectors: [artifact(1003)].into(),
    };
    let orphan_artifacts = DocumentArtifacts {
        extracted_content: [artifact(2000)].into(),
        summaries: [artifact(2001)].into(),
        labels: [artifact(2002)].into(),
        vectors: [artifact(2003)].into(),
    };
    let mut catalog = SemanticCatalog::new(library_id);
    let state = SemanticLibraryState::new(library_id);
    catalog
        .upsert_observations(
            &policy,
            &state,
            [
                CatalogObservation::new(
                    shared_outside,
                    location("file:///other/shared.txt"),
                    ContentFingerprint::new("sha256:shared").unwrap(),
                    OccurrenceScope::new(workspace_b, root_b),
                    shared_artifacts.clone(),
                    measurement(),
                ),
                CatalogObservation::new(
                    orphan_inside,
                    location("file:///docs/private/orphan.txt"),
                    ContentFingerprint::new("sha256:orphan").unwrap(),
                    OccurrenceScope::new(workspace_a, root_a),
                    orphan_artifacts,
                    measurement(),
                ),
                CatalogObservation::new(
                    shared_inside,
                    location("file:///docs/private/shared.txt"),
                    ContentFingerprint::new("sha256:shared").unwrap(),
                    OccurrenceScope::new(workspace_a, root_a),
                    shared_artifacts,
                    measurement(),
                ),
            ],
        )
        .unwrap();
    for (seed, entry) in [(3000, shared_inside), (3001, orphan_inside)] {
        let occurrence = catalog.occurrence_by_entry("local", entry).unwrap().id();
        catalog
            .add_conversation_pin(ConversationEvidencePin::new(
                ConversationPinId::from_uuid(Uuid::from_u128(seed)),
                occurrence,
                OccurrenceScope::new(workspace_a, root_a),
            ))
            .unwrap();
    }
    policy
        .exclude_descendant(root_a, exclusion_id, location("file:///docs/private"))
        .unwrap();
    assert!(matches!(
        policy
            .consent_state(&location("file:///docs/private/orphan.txt"))
            .unwrap(),
        ConsentState::Excluded { .. }
    ));

    let plan_id = catalog
        .begin_exclusion_cleanup(&mut policy, root_a, exclusion_id)
        .expect("plan cleanup");
    let plan = catalog.deletion_plan(plan_id).unwrap();

    assert_eq!(plan.status(), DeletionPlanStatus::Running);
    assert_eq!(plan.inventory().occurrences().len(), 2);
    assert_eq!(plan.inventory().extracted_content().len(), 1);
    assert_eq!(plan.inventory().summaries().len(), 1);
    assert_eq!(plan.inventory().labels().len(), 1);
    assert_eq!(plan.inventory().orphan_vectors().len(), 1);
    assert_eq!(plan.inventory().conversation_evidence_pins().len(), 2);
    assert_eq!(
        policy
            .exclusion(root_a, exclusion_id)
            .unwrap()
            .cleanup_status(),
        ExclusionCleanupStatus::Pending
    );

    catalog
        .complete_deletion_category(&mut policy, plan_id, DeletionCategory::Occurrences)
        .unwrap();
    catalog
        .checkpoint_deletion_category(plan_id, DeletionCategory::ConversationEvidencePins, 1)
        .unwrap();
    catalog
        .fail_deletion_category(
            plan_id,
            DeletionCategory::Summaries,
            "injected worker failure",
        )
        .unwrap();
    let directory = project_temp_dir("deletion-resume-");
    let store = SemanticCatalogStore::new(directory.path());
    store.save(&catalog).unwrap();
    let mut resumed = store.load().unwrap();

    assert_eq!(
        resumed.deletion_plan(plan_id).unwrap().status(),
        DeletionPlanStatus::Failed
    );
    assert!(matches!(
        policy
            .consent_state(&location("file:///docs/private/orphan.txt"))
            .unwrap(),
        ConsentState::Excluded { .. }
    ));
    assert!(
        resumed
            .deletion_plan(plan_id)
            .unwrap()
            .progress(DeletionCategory::Occurrences)
            .is_complete()
    );
    assert_eq!(
        resumed
            .deletion_plan(plan_id)
            .unwrap()
            .progress(DeletionCategory::ConversationEvidencePins)
            .completed_items(),
        1
    );
    resumed.resume_deletion(plan_id).unwrap();
    for category in DeletionCategory::all() {
        if !resumed
            .deletion_plan(plan_id)
            .unwrap()
            .progress(*category)
            .is_complete()
        {
            resumed
                .complete_deletion_category(&mut policy, plan_id, *category)
                .unwrap();
        }
    }

    assert_eq!(
        resumed.deletion_plan(plan_id).unwrap().status(),
        DeletionPlanStatus::Complete
    );
    assert_eq!(
        policy
            .exclusion(root_a, exclusion_id)
            .unwrap()
            .cleanup_status(),
        ExclusionCleanupStatus::Complete
    );
    assert_eq!(resumed.occurrence_count(), 1);
    assert_eq!(resumed.document_count(), 1);
    assert_eq!(resumed.conversation_pin_count(), 0);
    assert!(
        resumed
            .occurrence_by_entry("local", shared_outside)
            .is_some()
    );
}

#[test]
fn content_that_gains_an_outside_scope_occurrence_after_planning_is_not_deleted() {
    let library_id = LibraryId::from_uuid(Uuid::from_u128(2));
    let root_a = RootId::from_uuid(Uuid::from_u128(10));
    let root_b = RootId::from_uuid(Uuid::from_u128(11));
    let workspace_a = WorkspaceId::from(Uuid::from_u128(20));
    let workspace_b = WorkspaceId::from(Uuid::from_u128(21));
    let exclusion_id = ExclusionId::from_uuid(Uuid::from_u128(30));
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
    let mut enrolled_a = EnrolledRoot::new(root_a, location("file:///docs"), None, true);
    enrolled_a.attach_workspace(workspace_a);
    let mut enrolled_b = EnrolledRoot::new(root_b, location("file:///other"), None, true);
    enrolled_b.attach_workspace(workspace_b);
    policy.enrol_root(enrolled_a).unwrap();
    policy.enrol_root(enrolled_b).unwrap();
    let state = SemanticLibraryState::new(library_id);
    let artifacts = DocumentArtifacts {
        extracted_content: [artifact(1000)].into(),
        summaries: [artifact(1001)].into(),
        labels: [artifact(1002)].into(),
        vectors: [artifact(1003)].into(),
    };
    let mut catalog = SemanticCatalog::new(library_id);
    catalog
        .upsert_observations(
            &policy,
            &state,
            [CatalogObservation::new(
                EntryId::from(Uuid::from_u128(100)),
                location("file:///docs/private/only-copy.txt"),
                ContentFingerprint::new("sha256:racy").unwrap(),
                OccurrenceScope::new(workspace_a, root_a),
                artifacts.clone(),
                measurement(),
            )],
        )
        .unwrap();
    policy
        .exclude_descendant(root_a, exclusion_id, location("file:///docs/private"))
        .unwrap();
    let plan_id = catalog
        .begin_exclusion_cleanup(&mut policy, root_a, exclusion_id)
        .unwrap();

    assert_eq!(
        catalog
            .deletion_plan(plan_id)
            .unwrap()
            .inventory()
            .orphan_vectors()
            .len(),
        1
    );

    // The same content is observed in a completely different, still-consented
    // root after the destructive plan was captured.
    catalog
        .upsert_observations(
            &policy,
            &state,
            [CatalogObservation::new(
                EntryId::from(Uuid::from_u128(101)),
                location("file:///other/rediscovered.txt"),
                ContentFingerprint::new("sha256:racy").unwrap(),
                OccurrenceScope::new(workspace_b, root_b),
                artifacts,
                measurement(),
            )],
        )
        .unwrap();
    for category in DeletionCategory::all() {
        catalog
            .complete_deletion_category(&mut policy, plan_id, *category)
            .unwrap();
    }
    let surviving = catalog
        .occurrence_by_entry("local", EntryId::from(Uuid::from_u128(101)))
        .unwrap();
    let document = catalog.document(surviving.document_id()).unwrap();

    assert_eq!(
        catalog.deletion_plan(plan_id).unwrap().status(),
        DeletionPlanStatus::Complete
    );
    assert_eq!(
        policy
            .exclusion(root_a, exclusion_id)
            .unwrap()
            .cleanup_status(),
        ExclusionCleanupStatus::Complete
    );
    assert_eq!(catalog.occurrence_count(), 1);
    assert_eq!(catalog.document_count(), 1);
    assert!(
        document
            .artifacts()
            .extracted_content
            .contains(&artifact(1000)),
        "extracted content still referenced outside the revoked scope must survive"
    );
    assert!(document.artifacts().summaries.contains(&artifact(1001)));
    assert!(document.artifacts().labels.contains(&artifact(1002)));
    assert!(
        document.artifacts().vectors.contains(&artifact(1003)),
        "a vector that is no longer orphaned must not be deleted"
    );
    assert!(matches!(
        policy
            .consent_state(&location("file:///docs/private/only-copy.txt"))
            .unwrap(),
        ConsentState::Excluded { .. }
    ));
}
