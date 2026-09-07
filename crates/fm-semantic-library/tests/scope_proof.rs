#![allow(clippy::unwrap_used, missing_docs)]

use std::path::Path;

use fm_domain::{EntryId, Location, WorkspaceId};
use fm_semantic_library::{
    AccessContext, AuthorizationError, CatalogError, CatalogObservation, ContentFingerprint,
    DeviceLibraryIdentity, DocumentArtifacts, DocumentMeasurement, EnrolledRoot, HardQuotas,
    LibraryId, LibraryOperation, ModelIdentity, OccurrenceScope, ResourceBudgets, ResourceProfile,
    ResourceProfileKind, RootId, SemanticCatalog, SemanticCatalogStore, SemanticLibraryCoordinator,
    SemanticLibraryPolicy, SemanticLibraryState, SemanticQueryRequest, ServerEnrolmentPolicy,
    ServerPolicy, TenantId, UserId,
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

fn library_id() -> LibraryId {
    LibraryId::from_uuid(Uuid::from_u128(1))
}

fn private_root() -> RootId {
    RootId::from_uuid(Uuid::from_u128(10))
}

fn sibling_root() -> RootId {
    RootId::from_uuid(Uuid::from_u128(11))
}

fn workspace_id() -> WorkspaceId {
    WorkspaceId::from(Uuid::from_u128(20))
}

/// Two sibling roots in one workspace: `private` holds the sensitive document,
/// `sibling` is what a forged scope record would try to borrow authority from.
fn two_root_library() -> (SemanticLibraryPolicy, SemanticCatalog) {
    let mut policy = SemanticLibraryPolicy::new(
        DeviceLibraryIdentity::new(
            library_id(),
            ModelIdentity::new("model", "revision", 384, "space").unwrap(),
        ),
        ResourceProfile {
            kind: ResourceProfileKind::Balanced,
            budgets: ResourceBudgets::default(),
        },
    )
    .unwrap();
    let mut private = EnrolledRoot::new(private_root(), location("file:///private"), None, true);
    private.attach_workspace(workspace_id());
    let mut sibling = EnrolledRoot::new(sibling_root(), location("file:///sibling"), None, true);
    sibling.attach_workspace(workspace_id());
    policy.enrol_root(private).unwrap();
    policy.enrol_root(sibling).unwrap();
    let mut catalog = SemanticCatalog::new(library_id());
    catalog
        .upsert_observations(
            &policy,
            &SemanticLibraryState::new(library_id()),
            [CatalogObservation::new(
                EntryId::from(Uuid::from_u128(100)),
                location("file:///private/dossier.txt"),
                ContentFingerprint::new("sha256:dossier").unwrap(),
                OccurrenceScope::new(workspace_id(), private_root()),
                DocumentArtifacts::default(),
                DocumentMeasurement::new(64, 32, 16),
            )],
        )
        .unwrap();
    (policy, catalog)
}

fn server() -> (ServerPolicy, AccessContext) {
    let admin = UserId::new("admin-a").unwrap();
    let tenant = TenantId::new("tenant-a").unwrap();
    (
        ServerPolicy::single_private(
            tenant.clone(),
            library_id(),
            admin.clone(),
            ServerEnrolmentPolicy::local_only(),
            HardQuotas::default(),
        ),
        AccessContext::new(tenant, library_id(), admin),
    )
}

#[test]
fn a_tampered_on_disk_scope_cannot_borrow_a_sibling_roots_authority() {
    let (policy, catalog) = two_root_library();
    let semantic_root = project_temp_dir("forged-scope-");
    let store = SemanticCatalogStore::new(semantic_root.path());
    store.save(&catalog).unwrap();

    // An attacker with write access to the catalog file adds a scope naming a
    // root that never contained the document.
    let raw = std::fs::read_to_string(store.path()).unwrap();
    let mut tampered: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let occurrence = tampered
        .get_mut("occurrences")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    occurrence
        .get_mut("scopes")
        .and_then(serde_json::Value::as_array_mut)
        .unwrap()
        .push(serde_json::json!({
            "workspaceId": workspace_id().to_string(),
            "rootId": sibling_root().to_string(),
        }));
    std::fs::write(store.path(), serde_json::to_vec_pretty(&tampered).unwrap()).unwrap();
    let loaded = store.load().unwrap();
    let (server_policy, access) = server();

    assert!(matches!(
        loaded.validate_scopes(&policy),
        Err(CatalogError::UnprovenScope)
    ));
    assert!(
        loaded
            .query_documents(
                &policy,
                &server_policy,
                &SemanticQueryRequest::new(access.clone(), workspace_id(), [sibling_root()]),
            )
            .unwrap()
            .is_empty(),
        "a forged scope must not expose the document through the sibling root"
    );
    assert_eq!(
        loaded
            .query_documents(
                &policy,
                &server_policy,
                &SemanticQueryRequest::new(access, workspace_id(), [private_root()]),
            )
            .unwrap()
            .len(),
        1,
        "the genuine scope must still work"
    );
}

#[test]
fn loading_through_the_coordinator_drops_unprovable_scopes() {
    let configuration = project_temp_dir("forged-scope-config-");
    let semantic_root = project_temp_dir("forged-scope-data-");
    let coordinator = SemanticLibraryCoordinator::new(configuration.path(), semantic_root.path());
    let (policy, catalog) = two_root_library();
    coordinator
        .lock()
        .unwrap()
        .transaction(
            LibraryOperation::Enrolment,
            Some(&policy),
            Some(&catalog),
            None,
        )
        .unwrap()
        .commit()
        .unwrap();
    let catalog_path = coordinator.catalog_store().path();
    let mut tampered: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&catalog_path).unwrap()).unwrap();
    tampered
        .get_mut("occurrences")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap()
        .values_mut()
        .next()
        .unwrap()
        .get_mut("scopes")
        .and_then(serde_json::Value::as_array_mut)
        .unwrap()
        .push(serde_json::json!({
            "workspaceId": workspace_id().to_string(),
            "rootId": sibling_root().to_string(),
        }));
    std::fs::write(&catalog_path, serde_json::to_vec_pretty(&tampered).unwrap()).unwrap();

    let loaded = coordinator.load().unwrap();
    let occurrence = loaded.catalog.occurrences().next().unwrap();

    assert_eq!(loaded.unproven_scopes.len(), 1);
    assert_eq!(
        occurrence.scopes(),
        &[OccurrenceScope::new(workspace_id(), private_root())]
    );
    assert!(loaded.catalog.validate_scopes(&loaded.policy).is_ok());
}

#[test]
fn a_scope_naming_a_workspace_that_never_referenced_the_root_is_denied() {
    let (mut policy, catalog) = two_root_library();
    let (server_policy, access) = server();
    let intruding_workspace = WorkspaceId::from(Uuid::from_u128(21));
    policy
        .root_mut(private_root())
        .unwrap()
        .attach_workspace(intruding_workspace);

    // Ingest under the second workspace, then revoke that workspace's
    // reference: the persisted scope must stop proving anything.
    let mut catalog = catalog;
    catalog
        .upsert_observations(
            &policy,
            &SemanticLibraryState::new(library_id()),
            [CatalogObservation::new(
                EntryId::from(Uuid::from_u128(100)),
                location("file:///private/dossier.txt"),
                ContentFingerprint::new("sha256:dossier").unwrap(),
                OccurrenceScope::new(intruding_workspace, private_root()),
                DocumentArtifacts::default(),
                DocumentMeasurement::new(64, 32, 16),
            )],
        )
        .unwrap();
    policy.remove_workspace_reference(intruding_workspace);

    assert!(matches!(
        catalog.validate_scopes(&policy),
        Err(CatalogError::UnprovenScope)
    ));
    assert_eq!(
        catalog.query_documents(
            &policy,
            &server_policy,
            &SemanticQueryRequest::new(
                AccessContext::new(
                    access.tenant_id.clone(),
                    access.library_id,
                    access.user_id.clone()
                ),
                intruding_workspace,
                [private_root()],
            ),
        ),
        Err(AuthorizationError::ScopeDenied)
    );
}
