#![allow(clippy::unwrap_used, missing_docs)]

use fm_domain::{EntryId, Location, WorkspaceId};
use fm_semantic_library::{
    AccessContext, AuthorizationError, CatalogObservation, ContentFingerprint,
    DeviceLibraryIdentity, DocumentArtifacts, DocumentMeasurement, EnrolledRoot, ExclusionId,
    HardQuotas, LibraryId, ModelIdentity, OccurrenceScope, ResourceBudgets, ResourceProfile,
    ResourceProfileKind, RootId, SemanticCatalog, SemanticLibraryPolicy, SemanticLibraryState,
    SemanticQueryRequest, ServerEnrolmentPolicy, ServerPolicy, TenantId, TenantLibrary, UserId,
};
use uuid::Uuid;

fn tenant(value: &str) -> TenantId {
    TenantId::new(value).unwrap()
}

fn user(value: &str) -> UserId {
    UserId::new(value).unwrap()
}

fn library(value: u128) -> LibraryId {
    LibraryId::from_uuid(Uuid::from_u128(value))
}

fn measurement() -> DocumentMeasurement {
    DocumentMeasurement::new(64, 32, 16)
}

#[test]
fn default_server_policy_allows_only_its_single_private_admin() {
    let policy = ServerPolicy::single_private(
        tenant("tenant-a"),
        library(1),
        user("admin-a"),
        ServerEnrolmentPolicy::local_only(),
        HardQuotas::default(),
    );

    assert!(
        policy
            .authorize(&AccessContext::new(
                tenant("tenant-a"),
                library(1),
                user("admin-a"),
            ))
            .is_ok()
    );
    assert_eq!(
        policy.authorize(&AccessContext::new(
            tenant("tenant-a"),
            library(1),
            user("other-user"),
        )),
        Err(AuthorizationError::UserDenied)
    );
    assert_eq!(
        policy.authorize(&AccessContext::new(
            tenant("tenant-b"),
            library(1),
            user("admin-a"),
        )),
        Err(AuthorizationError::TenantDenied)
    );
    assert_eq!(
        policy.authorize(&AccessContext::new(
            tenant("tenant-a"),
            library(2),
            user("admin-a"),
        )),
        Err(AuthorizationError::LibraryDenied)
    );
}

#[test]
fn administrator_defined_policy_rejects_cross_tenant_access() {
    let policy = multi_tenant_policy();

    assert_eq!(
        policy.authorize(&AccessContext::new(
            tenant("tenant-b"),
            library(1),
            user("admin-a"),
        )),
        Err(AuthorizationError::LibraryDenied)
    );
}

#[test]
fn administrator_defined_policy_rejects_wrong_user() {
    let policy = multi_tenant_policy();

    assert_eq!(
        policy.authorize(&AccessContext::new(
            tenant("tenant-a"),
            library(1),
            user("admin-b"),
        )),
        Err(AuthorizationError::UserDenied)
    );
}

#[test]
fn administrator_defined_policy_rejects_sibling_library_access() {
    let policy = multi_tenant_policy();

    assert_eq!(
        policy.authorize(&AccessContext::new(
            tenant("tenant-a"),
            library(2),
            user("admin-a"),
        )),
        Err(AuthorizationError::LibraryDenied)
    );
}

#[test]
fn duplicate_library_id_across_tenants_is_denied_as_ambiguous() {
    let shared = library(77);
    let policy = ServerPolicy::administrator_defined([
        TenantLibrary::new(
            tenant("tenant-a"),
            shared,
            [user("admin-a")],
            [],
            ServerEnrolmentPolicy::local_only(),
            HardQuotas::default(),
        ),
        TenantLibrary::new(
            tenant("tenant-b"),
            shared,
            [user("admin-b")],
            [],
            ServerEnrolmentPolicy::local_only(),
            HardQuotas::default(),
        ),
    ]);

    assert_eq!(
        policy.authorize(&AccessContext::new(
            tenant("tenant-a"),
            shared,
            user("admin-a"),
        )),
        Err(AuthorizationError::LibraryDenied)
    );
}

fn multi_tenant_policy() -> ServerPolicy {
    ServerPolicy::administrator_defined([
        TenantLibrary::new(
            tenant("tenant-a"),
            library(1),
            [user("admin-a")],
            [],
            ServerEnrolmentPolicy::local_only(),
            HardQuotas::default(),
        ),
        TenantLibrary::new(
            tenant("tenant-b"),
            library(2),
            [user("admin-b")],
            [],
            ServerEnrolmentPolicy::local_only(),
            HardQuotas::default(),
        ),
    ])
}

#[test]
fn query_enforces_workspace_and_root_occurrence_scope_even_while_paused() {
    let library_id = library(1);
    let admin = user("admin-a");
    let server = ServerPolicy::single_private(
        tenant("tenant-a"),
        library_id,
        admin.clone(),
        ServerEnrolmentPolicy::local_only(),
        HardQuotas::default(),
    );
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
    let workspace_a = WorkspaceId::from(Uuid::from_u128(20));
    let workspace_b = WorkspaceId::from(Uuid::from_u128(21));
    let root_a = RootId::from_uuid(Uuid::from_u128(30));
    let root_b = RootId::from_uuid(Uuid::from_u128(31));
    let mut enrolled_a =
        EnrolledRoot::new(root_a, Location::parse("file:///a").unwrap(), None, true);
    enrolled_a.attach_workspace(workspace_a);
    enrolled_a.attach_workspace(workspace_b);
    let mut enrolled_b =
        EnrolledRoot::new(root_b, Location::parse("file:///b").unwrap(), None, true);
    enrolled_b.attach_workspace(workspace_a);
    policy.enrol_root(enrolled_a).unwrap();
    policy.enrol_root(enrolled_b).unwrap();
    let entry_a = EntryId::from(Uuid::from_u128(40));
    let entry_b = EntryId::from(Uuid::from_u128(41));
    let mut catalog = SemanticCatalog::new(library_id);
    let state = SemanticLibraryState::new(library_id);
    catalog
        .upsert_observations(
            &policy,
            &state,
            [
                CatalogObservation::new(
                    entry_b,
                    Location::parse("file:///b/b.txt").unwrap(),
                    ContentFingerprint::new("sha256:b").unwrap(),
                    OccurrenceScope::new(workspace_a, root_b),
                    DocumentArtifacts::default(),
                    measurement(),
                ),
                CatalogObservation::new(
                    entry_a,
                    Location::parse("file:///a/a.txt").unwrap(),
                    ContentFingerprint::new("sha256:a").unwrap(),
                    OccurrenceScope::new(workspace_a, root_a),
                    DocumentArtifacts::default(),
                    measurement(),
                ),
                CatalogObservation::new(
                    entry_a,
                    Location::parse("file:///a/a.txt").unwrap(),
                    ContentFingerprint::new("sha256:a").unwrap(),
                    OccurrenceScope::new(workspace_b, root_a),
                    DocumentArtifacts::default(),
                    measurement(),
                ),
            ],
        )
        .unwrap();
    let mut runtime = state.clone();
    runtime.pause();
    let access = AccessContext::new(tenant("tenant-a"), library_id, admin);

    let results = catalog
        .query_documents(
            &policy,
            &server,
            &SemanticQueryRequest::new(access.clone(), workspace_a, [root_a]),
        )
        .expect("authorized scoped query");

    assert!(runtime.is_paused());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].sources().len(), 1);
    assert_eq!(
        catalog.query_documents(
            &policy,
            &server,
            &SemanticQueryRequest::new(access.clone(), workspace_b, [root_b]),
        ),
        Err(AuthorizationError::ScopeDenied)
    );
    assert_eq!(
        catalog.query_documents(
            &policy,
            &server,
            &SemanticQueryRequest::new(
                access.clone(),
                workspace_a,
                [RootId::from_uuid(Uuid::from_u128(999))],
            ),
        ),
        Err(AuthorizationError::ScopeDenied)
    );
    assert_eq!(
        catalog.query_documents(
            &policy,
            &server,
            &SemanticQueryRequest::new(
                AccessContext::new(tenant("tenant-a"), library_id, user("intruder")),
                workspace_a,
                [root_a],
            ),
        ),
        Err(AuthorizationError::UserDenied)
    );
    policy
        .exclude_descendant(
            root_a,
            ExclusionId::from_uuid(Uuid::from_u128(50)),
            Location::parse("file:///a/a.txt").unwrap(),
        )
        .unwrap();
    assert!(
        catalog
            .query_documents(
                &policy,
                &server,
                &SemanticQueryRequest::new(access, workspace_a, [root_a]),
            )
            .unwrap()
            .is_empty()
    );
}
