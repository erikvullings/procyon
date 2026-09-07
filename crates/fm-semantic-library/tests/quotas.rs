#![allow(clippy::unwrap_used, missing_docs)]

use fm_domain::{EntryId, Location, WorkspaceId};
use fm_semantic_library::{
    AccessContext, AuthorizationError, BudgetKind, CatalogError, CatalogObservation,
    ContentFingerprint, DeviceLibraryIdentity, DocumentArtifacts, DocumentMeasurement,
    EnrolledRoot, HardQuotas, LibraryId, ModelIdentity, OccurrenceScope, QuotaUsage,
    ResourceBudgets, ResourceProfile, ResourceProfileKind, RootId, SemanticCatalog,
    SemanticLibraryPolicy, SemanticLibraryState, ServerEnrolmentPolicy, ServerPolicy, TenantId,
    TenantLibrary, UserId,
};
use uuid::Uuid;

fn location(uri: &str) -> Location {
    Location::parse(uri).unwrap()
}

fn library_id() -> LibraryId {
    LibraryId::from_uuid(Uuid::from_u128(1))
}

fn root_id() -> RootId {
    RootId::from_uuid(Uuid::from_u128(10))
}

fn workspace_id() -> WorkspaceId {
    WorkspaceId::from(Uuid::from_u128(20))
}

fn policy_with(budgets: ResourceBudgets) -> SemanticLibraryPolicy {
    let mut policy = SemanticLibraryPolicy::new(
        DeviceLibraryIdentity::new(
            library_id(),
            ModelIdentity::new("model", "revision", 384, "space").unwrap(),
        ),
        ResourceProfile {
            kind: ResourceProfileKind::Balanced,
            budgets,
        },
    )
    .unwrap();
    let mut root = EnrolledRoot::new(root_id(), location("file:///docs"), None, true);
    root.attach_workspace(workspace_id());
    policy.enrol_root(root).unwrap();
    policy
}

fn generous_budgets() -> ResourceBudgets {
    ResourceBudgets {
        max_documents: 100,
        max_source_bytes_per_document: 1_000,
        max_total_source_bytes: 10_000,
        max_total_extracted_bytes: 10_000,
        max_total_vector_bytes: 10_000,
    }
}

fn observation(
    entry: u128,
    fingerprint: &str,
    measurement: DocumentMeasurement,
) -> CatalogObservation {
    CatalogObservation::new(
        EntryId::from(Uuid::from_u128(entry)),
        location(&format!("file:///docs/{fingerprint}.txt")),
        ContentFingerprint::new(fingerprint).unwrap(),
        OccurrenceScope::new(workspace_id(), root_id()),
        DocumentArtifacts::default(),
        measurement,
    )
}

#[test]
fn usage_is_measured_from_stored_records_not_from_caller_claims() {
    let policy = policy_with(generous_budgets());
    let mut catalog = SemanticCatalog::new(library_id());
    let state = SemanticLibraryState::new(library_id());

    let usage = catalog
        .upsert_observations(
            &policy,
            &state,
            [
                observation(100, "sha256-a", DocumentMeasurement::new(400, 200, 100)),
                observation(101, "sha256-b", DocumentMeasurement::new(300, 150, 50)),
            ],
        )
        .unwrap();

    assert_eq!(usage.documents(), 2);
    assert_eq!(usage.source_bytes(), 700);
    assert_eq!(usage.extracted_bytes(), 350);
    assert_eq!(usage.vector_bytes(), 150);
    assert_eq!(
        QuotaUsage::measure(&policy, &catalog).unwrap().catalog(),
        usage
    );
    assert_eq!(QuotaUsage::measure(&policy, &catalog).unwrap().roots(), 1);
}

#[test]
fn a_forged_zero_byte_claim_cannot_shrink_recorded_usage_or_unlock_a_budget() {
    let policy = policy_with(ResourceBudgets {
        max_total_source_bytes: 1_000,
        ..generous_budgets()
    });
    let mut catalog = SemanticCatalog::new(library_id());
    let state = SemanticLibraryState::new(library_id());
    catalog
        .upsert_observations(
            &policy,
            &state,
            [observation(
                100,
                "sha256-large",
                DocumentMeasurement::new(900, 10, 10),
            )],
        )
        .unwrap();

    let forged = catalog.upsert_observations(
        &policy,
        &state,
        [observation(
            100,
            "sha256-large",
            DocumentMeasurement::new(0, 0, 0),
        )],
    );
    let over_budget = catalog.upsert_observations(
        &policy,
        &state,
        [observation(
            101,
            "sha256-next",
            DocumentMeasurement::new(200, 10, 10),
        )],
    );

    assert!(matches!(forged, Err(CatalogError::ConflictingMeasurement)));
    assert!(matches!(
        over_budget,
        Err(CatalogError::BudgetExceeded(BudgetKind::SourceBytes))
    ));
    assert_eq!(catalog.measured_usage().source_bytes(), 900);
    assert_eq!(catalog.document_count(), 1);
}

#[test]
fn a_stale_usage_snapshot_cannot_carry_a_second_batch_over_the_boundary() {
    let policy = policy_with(ResourceBudgets {
        max_documents: 2,
        ..generous_budgets()
    });
    let mut catalog = SemanticCatalog::new(library_id());
    let state = SemanticLibraryState::new(library_id());
    let empty_snapshot = QuotaUsage::measure(&policy, &catalog).unwrap();

    catalog
        .upsert_observations(
            &policy,
            &state,
            [
                observation(100, "sha256-one", DocumentMeasurement::new(10, 5, 5)),
                observation(101, "sha256-two", DocumentMeasurement::new(10, 5, 5)),
            ],
        )
        .unwrap();
    let rejected = catalog.upsert_observations(
        &policy,
        &state,
        [observation(
            102,
            "sha256-three",
            DocumentMeasurement::new(10, 5, 5),
        )],
    );

    assert_eq!(empty_snapshot.catalog().documents(), 0);
    assert!(matches!(
        rejected,
        Err(CatalogError::BudgetExceeded(BudgetKind::Documents))
    ));
    assert_eq!(catalog.document_count(), 2);
    assert_eq!(catalog.occurrence_count(), 2);
}

#[test]
fn tenant_ingestion_enforces_hard_quotas_measured_from_the_catalog() {
    let policy = policy_with(generous_budgets());
    let mut catalog = SemanticCatalog::new(library_id());
    let state = SemanticLibraryState::new(library_id());
    let admin = UserId::new("admin-a").unwrap();
    let server = ServerPolicy::administrator_defined([TenantLibrary::new(
        TenantId::new("tenant-a").unwrap(),
        library_id(),
        [admin.clone()],
        [],
        ServerEnrolmentPolicy::local_only(),
        HardQuotas {
            max_roots: 4,
            max_documents: 4,
            max_source_bytes: 500,
            max_extracted_bytes: 500,
            max_vector_bytes: 500,
        },
    )]);
    let access = AccessContext::new(TenantId::new("tenant-a").unwrap(), library_id(), admin);

    catalog
        .ingest_for_tenant(
            &policy,
            &state,
            &server,
            &access,
            [observation(
                100,
                "sha256-first",
                DocumentMeasurement::new(400, 100, 100),
            )],
        )
        .unwrap();
    let denied = catalog.ingest_for_tenant(
        &policy,
        &state,
        &server,
        &access,
        [observation(
            101,
            "sha256-second",
            DocumentMeasurement::new(200, 100, 100),
        )],
    );
    let intruder = catalog.ingest_for_tenant(
        &policy,
        &state,
        &server,
        &AccessContext::new(
            TenantId::new("tenant-a").unwrap(),
            library_id(),
            UserId::new("intruder").unwrap(),
        ),
        [observation(
            102,
            "sha256-third",
            DocumentMeasurement::new(1, 1, 1),
        )],
    );

    assert!(matches!(
        denied,
        Err(CatalogError::Authorization(
            AuthorizationError::QuotaExceeded
        ))
    ));
    assert!(matches!(
        intruder,
        Err(CatalogError::Authorization(AuthorizationError::UserDenied))
    ));
    assert_eq!(catalog.document_count(), 1);
    assert_eq!(catalog.measured_usage().source_bytes(), 400);
}

#[test]
fn root_enrolment_authorization_projects_roots_from_durable_policy_records() {
    let policy = policy_with(generous_budgets());
    let catalog = SemanticCatalog::new(library_id());
    let admin = UserId::new("admin-a").unwrap();
    let member = UserId::new("member-a").unwrap();
    let server = ServerPolicy::administrator_defined([TenantLibrary::new(
        TenantId::new("tenant-a").unwrap(),
        library_id(),
        [admin.clone()],
        [member.clone()],
        ServerEnrolmentPolicy::local_only(),
        HardQuotas {
            max_roots: 1,
            max_documents: 10,
            max_source_bytes: 100,
            max_extracted_bytes: 100,
            max_vector_bytes: 50,
        },
    )]);
    let context =
        |user_id| AccessContext::new(TenantId::new("tenant-a").unwrap(), library_id(), user_id);

    assert_eq!(
        server.authorize_root_enrolment(&context(member), "local", &policy, &catalog),
        Err(AuthorizationError::AdministratorRequired)
    );
    assert_eq!(
        server.authorize_root_enrolment(&context(admin.clone()), "sftp", &policy, &catalog),
        Err(AuthorizationError::ProviderDenied)
    );
    assert_eq!(
        server.authorize_root_enrolment(&context(admin.clone()), "local", &policy, &catalog),
        Err(AuthorizationError::QuotaExceeded)
    );

    let empty_policy = SemanticLibraryPolicy::new(
        DeviceLibraryIdentity::new(
            library_id(),
            ModelIdentity::new("model", "revision", 384, "space").unwrap(),
        ),
        ResourceProfile {
            kind: ResourceProfileKind::Balanced,
            budgets: generous_budgets(),
        },
    )
    .unwrap();

    assert!(
        server
            .authorize_root_enrolment(&context(admin), "local", &empty_policy, &catalog)
            .is_ok()
    );
}

#[test]
fn a_per_document_source_ceiling_is_enforced_against_the_measured_record() {
    let policy = policy_with(ResourceBudgets {
        max_source_bytes_per_document: 100,
        ..generous_budgets()
    });
    let mut catalog = SemanticCatalog::new(library_id());
    let state = SemanticLibraryState::new(library_id());

    let rejected = catalog.upsert_observations(
        &policy,
        &state,
        [observation(
            100,
            "sha256-huge",
            DocumentMeasurement::new(101, 1, 1),
        )],
    );

    assert!(matches!(
        rejected,
        Err(CatalogError::BudgetExceeded(
            BudgetKind::SourceBytesPerDocument
        ))
    ));
    assert_eq!(catalog.document_count(), 0);
}
