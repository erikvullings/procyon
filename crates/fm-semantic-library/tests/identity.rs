#![allow(clippy::unwrap_used, missing_docs)]

use fm_domain::{EntryId, Location, WorkspaceId};
use fm_semantic_library::{
    CatalogObservation, ContentFingerprint, DeviceLibraryIdentity, DocumentArtifacts,
    DocumentMeasurement, EnrolledRoot, ExclusionId, FilesystemIdentity, LibraryId, ModelIdentity,
    ObservedRootIdentity, OccurrenceScope, ResourceBudgets, ResourceProfile, ResourceProfileKind,
    RootAvailability, RootId, RootMoveResolution, RootUnavailabilityReason, SemanticCatalog,
    SemanticLibraryPolicy, SemanticLibraryState,
};
use uuid::Uuid;

fn location(uri: &str) -> Location {
    Location::parse(uri).expect("valid location")
}

fn policy_with_root() -> (SemanticLibraryPolicy, RootId) {
    let root_id = RootId::from_uuid(Uuid::from_u128(10));
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
    policy
        .enrol_root(EnrolledRoot::new(
            root_id,
            location("file:///before"),
            Some(FilesystemIdentity::new("volume-a", "directory-7").unwrap()),
            true,
        ))
        .unwrap();
    (policy, root_id)
}

#[test]
fn matching_provider_volume_and_file_identity_proves_a_rename() {
    let (mut policy, root_id) = policy_with_root();
    let mut catalog = SemanticCatalog::new(policy.library().id());
    let destination = location("file:///after");

    let resolution = catalog
        .reconcile_root_location(
            &mut policy,
            root_id,
            &[ObservedRootIdentity::new(
                destination.clone(),
                Some(FilesystemIdentity::new("volume-a", "directory-7").unwrap()),
            )],
        )
        .unwrap();

    assert_eq!(
        resolution,
        RootMoveResolution::ProvenMove {
            previous: location("file:///before"),
            current: destination.clone(),
        }
    );
    assert_eq!(policy.root(root_id).unwrap().location(), &destination);
    assert_eq!(
        catalog.root_availability(root_id),
        RootAvailability::Available
    );
}

#[test]
fn reused_path_with_a_different_identity_never_inherits_consent() {
    let (mut policy, root_id) = policy_with_root();
    let mut catalog = SemanticCatalog::new(policy.library().id());

    let resolution = catalog
        .reconcile_root_location(
            &mut policy,
            root_id,
            &[ObservedRootIdentity::new(
                location("file:///before"),
                Some(FilesystemIdentity::new("volume-a", "different-file").unwrap()),
            )],
        )
        .unwrap();

    assert_eq!(
        resolution,
        RootMoveResolution::RetainedUnavailable {
            reason: RootUnavailabilityReason::PathReused,
        }
    );
    assert_eq!(
        policy.root(root_id).unwrap().location(),
        &location("file:///before")
    );
    assert!(matches!(
        catalog.root_availability(root_id),
        RootAvailability::TemporarilyUnavailable { .. }
    ));
}

#[test]
fn ambiguous_identity_requires_confirmation_and_retains_old_location() {
    let (mut policy, root_id) = policy_with_root();
    let mut catalog = SemanticCatalog::new(policy.library().id());
    let identity = FilesystemIdentity::new("volume-a", "directory-7").unwrap();

    let resolution = catalog
        .reconcile_root_location(
            &mut policy,
            root_id,
            &[
                ObservedRootIdentity::new(location("file:///candidate-a"), Some(identity.clone())),
                ObservedRootIdentity::new(location("file:///candidate-b"), Some(identity)),
            ],
        )
        .unwrap();

    assert_eq!(
        resolution,
        RootMoveResolution::RetainedUnavailable {
            reason: RootUnavailabilityReason::AmbiguousIdentity,
        }
    );
    assert_eq!(
        policy.root(root_id).unwrap().location(),
        &location("file:///before")
    );
    assert!(matches!(
        catalog.root_availability(root_id),
        RootAvailability::TemporarilyUnavailable { .. }
    ));
}

#[test]
fn same_file_number_on_another_volume_requires_confirmation() {
    let (mut policy, root_id) = policy_with_root();
    let mut catalog = SemanticCatalog::new(policy.library().id());

    let resolution = catalog
        .reconcile_root_location(
            &mut policy,
            root_id,
            &[ObservedRootIdentity::new(
                location("file:///other-volume/root"),
                Some(FilesystemIdentity::new("volume-b", "directory-7").unwrap()),
            )],
        )
        .unwrap();

    assert_eq!(
        resolution,
        RootMoveResolution::RetainedUnavailable {
            reason: RootUnavailabilityReason::CrossVolume,
        }
    );
    assert_eq!(
        policy.root(root_id).unwrap().location(),
        &location("file:///before")
    );
    assert!(matches!(
        catalog.root_availability(root_id),
        RootAvailability::TemporarilyUnavailable { .. }
    ));
}

#[test]
fn path_only_candidate_cannot_prove_a_rename() {
    let (mut policy, root_id) = policy_with_root();
    let mut catalog = SemanticCatalog::new(policy.library().id());

    let resolution = catalog
        .reconcile_root_location(
            &mut policy,
            root_id,
            &[ObservedRootIdentity::new(
                location("file:///possible"),
                None,
            )],
        )
        .unwrap();

    assert_eq!(
        resolution,
        RootMoveResolution::RetainedUnavailable {
            reason: RootUnavailabilityReason::UnprovenIdentity,
        }
    );
}

#[test]
fn a_proven_move_relocates_exclusions_and_occurrences_without_restoring_consent() {
    let (mut policy, root_id) = policy_with_root();
    let exclusion_id = ExclusionId::from_uuid(Uuid::from_u128(30));
    let workspace_id = WorkspaceId::from(Uuid::from_u128(20));
    policy
        .root_mut(root_id)
        .unwrap()
        .attach_workspace(workspace_id);
    policy
        .exclude_descendant(root_id, exclusion_id, location("file:///before/private"))
        .unwrap();
    let mut catalog = SemanticCatalog::new(policy.library().id());
    let state = SemanticLibraryState::new(policy.library().id());
    catalog
        .upsert_observations(
            &policy,
            &state,
            [CatalogObservation::new(
                EntryId::from(Uuid::from_u128(100)),
                location("file:///before/reports/q1.txt"),
                ContentFingerprint::new("sha256:q1").unwrap(),
                OccurrenceScope::new(workspace_id, root_id),
                DocumentArtifacts::default(),
                DocumentMeasurement::new(64, 32, 16),
            )],
        )
        .unwrap();
    let plan_id = catalog
        .begin_exclusion_cleanup(&mut policy, root_id, exclusion_id)
        .unwrap();

    let resolution = catalog
        .reconcile_root_location(
            &mut policy,
            root_id,
            &[ObservedRootIdentity::new(
                location("file:///after"),
                Some(FilesystemIdentity::new("volume-a", "directory-7").unwrap()),
            )],
        )
        .unwrap();
    let exclusion = policy.exclusion(root_id, exclusion_id).unwrap();

    assert_eq!(
        resolution,
        RootMoveResolution::ProvenMove {
            previous: location("file:///before"),
            current: location("file:///after"),
        }
    );
    assert_eq!(exclusion.id(), exclusion_id);
    assert_eq!(exclusion.location(), &location("file:///after/private"));
    assert_eq!(
        exclusion.cleanup_status(),
        fm_semantic_library::ExclusionCleanupStatus::Pending
    );
    assert_eq!(exclusion.deletion_plan_id(), Some(plan_id));
    assert!(matches!(
        policy
            .consent_state(&location("file:///after/private/secret.txt"))
            .unwrap(),
        fm_semantic_library::ConsentState::Excluded { .. }
    ));
    assert!(matches!(
        policy
            .consent_state(&location("file:///before/private/secret.txt"))
            .unwrap(),
        fm_semantic_library::ConsentState::NotIncluded
    ));
    assert_eq!(
        catalog.occurrences().next().unwrap().location(),
        &location("file:///after/reports/q1.txt")
    );
    assert!(catalog.validate_scopes(&policy).is_ok());
    assert_eq!(catalog.occurrence_count(), 1);
    assert_eq!(catalog.document_count(), 1);
}

#[test]
fn an_unproven_move_leaves_exclusions_and_occurrences_exactly_where_they_were() {
    let (mut policy, root_id) = policy_with_root();
    let exclusion_id = ExclusionId::from_uuid(Uuid::from_u128(31));
    let workspace_id = WorkspaceId::from(Uuid::from_u128(21));
    policy
        .root_mut(root_id)
        .unwrap()
        .attach_workspace(workspace_id);
    policy
        .exclude_descendant(root_id, exclusion_id, location("file:///before/private"))
        .unwrap();
    let mut catalog = SemanticCatalog::new(policy.library().id());

    for observations in [
        vec![ObservedRootIdentity::new(
            location("file:///other-volume/after"),
            Some(FilesystemIdentity::new("volume-b", "directory-7").unwrap()),
        )],
        vec![
            ObservedRootIdentity::new(
                location("file:///candidate-a"),
                Some(FilesystemIdentity::new("volume-a", "directory-7").unwrap()),
            ),
            ObservedRootIdentity::new(
                location("file:///candidate-b"),
                Some(FilesystemIdentity::new("volume-a", "directory-7").unwrap()),
            ),
        ],
        vec![ObservedRootIdentity::new(
            location("sftp://55555555-5555-4555-8555-555555555555/after"),
            Some(FilesystemIdentity::new("volume-a", "directory-7").unwrap()),
        )],
    ] {
        let resolution = catalog
            .reconcile_root_location(&mut policy, root_id, &observations)
            .unwrap();

        assert!(matches!(
            resolution,
            RootMoveResolution::RetainedUnavailable { .. }
        ));
        assert_eq!(
            policy.root(root_id).unwrap().location(),
            &location("file:///before")
        );
        assert_eq!(
            policy.exclusion(root_id, exclusion_id).unwrap().location(),
            &location("file:///before/private")
        );
        assert!(matches!(
            catalog.root_availability(root_id),
            RootAvailability::TemporarilyUnavailable { .. }
        ));
    }
}

#[test]
fn a_move_that_cannot_be_proven_for_an_overlapping_root_is_rolled_back_whole() {
    let outer_id = RootId::from_uuid(Uuid::from_u128(40));
    let nested_id = RootId::from_uuid(Uuid::from_u128(41));
    let workspace_id = WorkspaceId::from(Uuid::from_u128(22));
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
    let mut outer = EnrolledRoot::new(
        outer_id,
        location("file:///before"),
        Some(FilesystemIdentity::new("volume-a", "directory-7").unwrap()),
        true,
    );
    outer.attach_workspace(workspace_id);
    let mut nested = EnrolledRoot::new(nested_id, location("file:///before/team"), None, true);
    nested.attach_workspace(workspace_id);
    policy.enrol_root(outer).unwrap();
    policy.enrol_root(nested).unwrap();
    let state = SemanticLibraryState::new(policy.library().id());
    let mut catalog = SemanticCatalog::new(policy.library().id());
    for root in [outer_id, nested_id] {
        catalog
            .upsert_observations(
                &policy,
                &state,
                [CatalogObservation::new(
                    EntryId::from(Uuid::from_u128(100)),
                    location("file:///before/team/shared.txt"),
                    ContentFingerprint::new("sha256:shared").unwrap(),
                    OccurrenceScope::new(workspace_id, root),
                    DocumentArtifacts::default(),
                    DocumentMeasurement::new(64, 32, 16),
                )],
            )
            .unwrap();
    }
    let before = catalog.clone();

    let resolution = catalog
        .reconcile_root_location(
            &mut policy,
            outer_id,
            &[ObservedRootIdentity::new(
                location("file:///after"),
                Some(FilesystemIdentity::new("volume-a", "directory-7").unwrap()),
            )],
        )
        .unwrap();

    assert_eq!(
        resolution,
        RootMoveResolution::RetainedUnavailable {
            reason: RootUnavailabilityReason::RelocationConflict,
        }
    );
    assert_eq!(
        policy.root(outer_id).unwrap().location(),
        &location("file:///before")
    );
    assert_eq!(
        policy.root(nested_id).unwrap().location(),
        &location("file:///before/team")
    );
    assert_eq!(
        catalog.occurrences().next().unwrap().location(),
        before.occurrences().next().unwrap().location()
    );
    assert_eq!(catalog.occurrence_count(), before.occurrence_count());
    assert!(catalog.validate_scopes(&policy).is_ok());
    assert!(matches!(
        catalog.root_availability(outer_id),
        RootAvailability::TemporarilyUnavailable { .. }
    ));
}
