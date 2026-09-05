#![allow(clippy::unwrap_used, missing_docs)]

use fm_domain::Location;
use fm_semantic_library::{
    ConsentState, DeviceLibraryIdentity, EnrolledRoot, ExclusionId, LibraryId, ModelIdentity,
    ResourceBudgets, ResourceProfile, ResourceProfileKind, RootId, SemanticLibraryPolicy,
};
use uuid::Uuid;

fn location(uri: &str) -> Location {
    Location::parse(uri).expect("valid location")
}

fn policy() -> SemanticLibraryPolicy {
    SemanticLibraryPolicy::new(
        DeviceLibraryIdentity::new(
            LibraryId::from_uuid(Uuid::from_u128(1)),
            ModelIdentity::new("model", "revision", 384, "space").expect("model"),
        ),
        ResourceProfile {
            kind: ResourceProfileKind::Balanced,
            budgets: ResourceBudgets::default(),
        },
    )
    .expect("policy")
}

#[test]
fn consent_distinguishes_direct_inherited_and_unrelated_folders_structurally() {
    let root_id = RootId::from_uuid(Uuid::from_u128(10));
    let mut policy = policy();
    policy
        .enrol_root(EnrolledRoot::new(
            root_id,
            location("file:///docs"),
            None,
            true,
        ))
        .expect("enrol root");

    assert_eq!(
        policy.consent_state(&location("file:///docs")).unwrap(),
        ConsentState::IncludedHere { root_id }
    );
    assert_eq!(
        policy
            .consent_state(&location("file:///docs/reports"))
            .unwrap(),
        ConsentState::InheritedFromParent { root_id }
    );
    assert_eq!(
        policy.consent_state(&location("file:///docs-old")).unwrap(),
        ConsentState::NotIncluded
    );
}

#[test]
fn most_specific_exclusion_wins_and_nested_root_cannot_silently_restore_consent() {
    let ancestor_root = RootId::from_uuid(Uuid::from_u128(20));
    let nested_root = RootId::from_uuid(Uuid::from_u128(21));
    let private = ExclusionId::from_uuid(Uuid::from_u128(22));
    let deeper = ExclusionId::from_uuid(Uuid::from_u128(23));
    let mut policy = policy();
    policy
        .enrol_root(EnrolledRoot::new(
            ancestor_root,
            location("file:///docs"),
            None,
            true,
        ))
        .unwrap();
    policy
        .exclude_descendant(ancestor_root, private, location("file:///docs/private"))
        .unwrap();
    policy
        .exclude_descendant(ancestor_root, deeper, location("file:///docs/private/deep"))
        .unwrap();
    policy
        .enrol_root(EnrolledRoot::new(
            nested_root,
            location("file:///docs/private/deep/explicit-root"),
            None,
            true,
        ))
        .unwrap();

    assert_eq!(
        policy
            .consent_state(&location(
                "file:///docs/private/deep/explicit-root/file.txt"
            ))
            .unwrap(),
        ConsentState::Excluded {
            root_id: ancestor_root,
            exclusion_id: deeper,
        }
    );
}

#[test]
fn non_recursive_root_does_not_grant_descendant_consent() {
    let root_id = RootId::from_uuid(Uuid::from_u128(30));
    let mut policy = policy();
    policy
        .enrol_root(EnrolledRoot::new(
            root_id,
            location("file:///single"),
            None,
            false,
        ))
        .unwrap();

    assert_eq!(
        policy.consent_state(&location("file:///single")).unwrap(),
        ConsentState::IncludedHere { root_id }
    );
    assert_eq!(
        policy
            .consent_state(&location("file:///single/child"))
            .unwrap(),
        ConsentState::NotIncluded
    );
}

#[test]
fn explicit_root_revocation_excludes_the_root_and_all_descendants() {
    let root_id = RootId::from_uuid(Uuid::from_u128(40));
    let exclusion_id = ExclusionId::from_uuid(Uuid::from_u128(41));
    let mut policy = policy();
    policy
        .enrol_root(EnrolledRoot::new(
            root_id,
            location("file:///revoked"),
            None,
            true,
        ))
        .unwrap();

    policy.exclude_root(root_id, exclusion_id).unwrap();

    assert_eq!(
        policy.consent_state(&location("file:///revoked")).unwrap(),
        ConsentState::Excluded {
            root_id,
            exclusion_id,
        }
    );
    assert_eq!(
        policy
            .consent_state(&location("file:///revoked/child"))
            .unwrap(),
        ConsentState::Excluded {
            root_id,
            exclusion_id,
        }
    );
}
