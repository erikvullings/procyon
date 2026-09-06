#![allow(clippy::unwrap_used, missing_docs)]

use std::collections::BTreeMap;
use std::path::Path;

use fm_domain::{Location, WorkspaceId};
use fm_semantic_library::{
    CURRENT_POLICY_SCHEMA_VERSION, DeviceLibraryIdentity, EligibilityOverride, EligibilityReason,
    EnrolledRoot, ExclusionId, FilesystemIdentity, LibraryId, ModelIdentity, ResourceBudgets,
    ResourceProfile, ResourceProfileKind, RootId, SemanticLibraryPolicy,
    SemanticLibraryPolicyStore, VocabularyId,
};
use tempfile::TempDir;
use uuid::Uuid;

fn project_temp_dir(prefix: &str) -> TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/semantic-library-tests");
    std::fs::create_dir_all(&parent).expect("create project-local test directory");
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)
        .expect("create project-local temporary directory")
}

fn policy() -> SemanticLibraryPolicy {
    SemanticLibraryPolicy::new(
        DeviceLibraryIdentity::new(
            LibraryId::new(),
            ModelIdentity::new("model", "revision-1", 384, "space-v1")
                .expect("valid model identity"),
        ),
        ResourceProfile {
            kind: ResourceProfileKind::Balanced,
            budgets: ResourceBudgets::default(),
        },
    )
    .expect("valid policy")
}

#[test]
fn policy_round_trips_atomically_with_stable_library_and_model_identity() {
    let directory = project_temp_dir("policy-round-trip-");
    let store = SemanticLibraryPolicyStore::new(directory.path());
    let expected = policy();

    store.save(&expected).expect("save policy");
    let actual = store.load().expect("load policy");

    assert_eq!(actual, expected);
    assert_eq!(actual.schema_version(), CURRENT_POLICY_SCHEMA_VERSION);
    assert_eq!(actual.library().id(), expected.library().id());
    assert_eq!(actual.library().model(), expected.library().model());
    assert!(store.path().starts_with(directory.path()));
}

#[test]
fn legacy_root_identity_is_backfilled_once_without_overwriting_provider_evidence() {
    let mut policy = policy();
    let root_id = RootId::from_uuid(Uuid::from_u128(10));
    policy
        .enrol_root(EnrolledRoot::new(
            root_id,
            Location::parse("file:///library").unwrap(),
            None,
            true,
        ))
        .unwrap();
    let original_revision = policy.revision();

    assert!(
        policy
            .backfill_root_filesystem_identity(
                root_id,
                FilesystemIdentity::new("volume-a", "file-a").unwrap(),
            )
            .unwrap()
    );
    assert_eq!(policy.revision(), original_revision + 1);
    assert_eq!(
        policy
            .root(root_id)
            .unwrap()
            .filesystem_identity()
            .unwrap()
            .file_id(),
        "file-a"
    );

    assert!(
        !policy
            .backfill_root_filesystem_identity(
                root_id,
                FilesystemIdentity::new("volume-b", "file-b").unwrap(),
            )
            .unwrap()
    );
    assert_eq!(policy.revision(), original_revision + 1);
    assert_eq!(
        policy
            .root(root_id)
            .unwrap()
            .filesystem_identity()
            .unwrap()
            .file_id(),
        "file-a"
    );
}

#[test]
fn policy_persists_every_root_consent_and_resource_field() {
    let mut policy = policy();
    policy
        .set_reconciliation_interval_seconds(900)
        .expect("valid interval");
    let root_id = RootId::from_uuid(Uuid::from_u128(10));
    let workspace_id = WorkspaceId::from(Uuid::from_u128(11));
    let mut root = EnrolledRoot::new(
        root_id,
        Location::parse("file:///library").expect("root location"),
        Some(FilesystemIdentity::new("volume-a", "file-10").expect("stable identity")),
        true,
    );
    root.attach_workspace(workspace_id);
    root.attach_vocabulary(VocabularyId::new("taxonomy"));
    root.set_eligibility_overrides(BTreeMap::from([(
        EligibilityReason::Hidden,
        EligibilityOverride::Include,
    )]));
    policy.enrol_root(root).expect("enrol root");
    policy
        .exclude_descendant(
            root_id,
            ExclusionId::from_uuid(Uuid::from_u128(12)),
            Location::parse("file:///library/private").expect("excluded location"),
        )
        .expect("exclude descendant");

    let directory = project_temp_dir("all-policy-fields-");
    let store = SemanticLibraryPolicyStore::new(directory.path());
    store.save(&policy).expect("save policy");
    let loaded = store.load().expect("load policy");
    let loaded_root = loaded.root(root_id).expect("persisted root");

    assert_eq!(loaded.reconciliation_interval_seconds(), 900);
    assert_eq!(
        loaded_root.filesystem_identity().unwrap().volume_id(),
        "volume-a"
    );
    assert!(loaded_root.recursive());
    assert_eq!(loaded_root.workspace_references(), &[workspace_id]);
    assert!(
        loaded_root
            .vocabulary_ids()
            .contains(&VocabularyId::new("taxonomy"))
    );
    assert_eq!(
        loaded_root
            .eligibility_overrides()
            .get(&EligibilityReason::Hidden),
        Some(&EligibilityOverride::Include)
    );
    assert_eq!(loaded_root.exclusions().len(), 1);
}

#[test]
fn v1_policy_fixture_migrates_without_losing_consent_or_scope() {
    let directory = project_temp_dir("policy-v1-");
    let store = SemanticLibraryPolicyStore::new(directory.path());
    std::fs::create_dir_all(store.path().parent().expect("policy parent"))
        .expect("create policy directory");
    std::fs::write(store.path(), include_bytes!("fixtures/policy-v1.json"))
        .expect("write v1 fixture");

    let migrated = store.load().expect("migrate policy");
    let root = migrated
        .root(RootId::from_uuid(Uuid::from_u128(
            0x22222222_2222_4222_8222_222222222222,
        )))
        .expect("migrated root");

    assert_eq!(migrated.schema_version(), CURRENT_POLICY_SCHEMA_VERSION);
    assert_eq!(root.workspace_references().len(), 1);
    assert_eq!(root.exclusions().len(), 1);
    assert_eq!(
        root.exclusions()[0].cleanup_status(),
        fm_semantic_library::ExclusionCleanupStatus::Complete
    );
    assert!(
        root.vocabulary_ids()
            .contains(&VocabularyId::new("fixture-taxonomy"))
    );
}

#[test]
fn v1_policy_fixture_gains_a_durable_revision_that_stales_every_older_token() {
    let directory = project_temp_dir("policy-v1-revision-");
    let store = SemanticLibraryPolicyStore::new(directory.path());
    std::fs::create_dir_all(store.path().parent().expect("policy parent"))
        .expect("create policy directory");
    std::fs::write(store.path(), include_bytes!("fixtures/policy-v1.json"))
        .expect("write v1 fixture");

    let migrated = store.load().expect("migrate policy");

    assert_eq!(migrated.schema_version(), CURRENT_POLICY_SCHEMA_VERSION);
    assert_eq!(
        migrated.revision(),
        1,
        "a pre-revision policy starts at one so every older optimistic token is stale"
    );
}

#[test]
fn v2_policy_fixture_migrates_to_a_durable_monotonic_revision() {
    let directory = project_temp_dir("policy-v2-");
    let store = SemanticLibraryPolicyStore::new(directory.path());
    std::fs::create_dir_all(store.path().parent().expect("policy parent"))
        .expect("create policy directory");
    std::fs::write(store.path(), include_bytes!("fixtures/policy-v2.json"))
        .expect("write v2 fixture");

    let mut migrated = store.load().expect("migrate policy");
    let root = migrated
        .root(RootId::from_uuid(Uuid::from_u128(
            0x22222222_2222_4222_8222_222222222222,
        )))
        .expect("migrated root");

    assert_eq!(migrated.schema_version(), CURRENT_POLICY_SCHEMA_VERSION);
    assert_eq!(migrated.revision(), 1);
    assert_eq!(root.workspace_references().len(), 1);
    assert_eq!(root.exclusions().len(), 1);

    // The migrated revision is durable and monotonic from then on.
    migrated.advance_revision().expect("advance revision");
    store.save(&migrated).expect("save migrated policy");
    assert_eq!(store.load().expect("reload").revision(), 2);
}

#[test]
fn a_policy_write_can_never_move_the_durable_revision_backwards() {
    let directory = project_temp_dir("policy-revision-regress-");
    let store = SemanticLibraryPolicyStore::new(directory.path());
    // `stale` is the snapshot a second process would still be holding after
    // this one committed twice.
    let stale = policy();
    let mut advanced = stale.clone();
    advanced.advance_revision().expect("advance revision");
    advanced.advance_revision().expect("advance revision");
    store.save(&advanced).expect("save policy");

    assert!(matches!(
        store.save(&stale),
        Err(fm_semantic_library::StoreError::RevisionRegressed)
    ));
    // Replaying the same revision, as deterministic journal recovery does, is
    // idempotent rather than rejected.
    store.save(&advanced).expect("idempotent replay");
    assert_eq!(store.load().expect("reload").revision(), 3);
}

#[test]
fn policy_rejects_url_userinfo_and_transient_session_data() {
    let mut with_userinfo = policy();
    let userinfo_error = with_userinfo.enrol_root(EnrolledRoot::new(
        RootId::from_uuid(Uuid::from_u128(50)),
        Location::new(
            fm_domain::ProviderId::new("sftp"),
            "sftp://alice:secret@example.test/folder",
        ),
        None,
        true,
    ));
    let mut with_token = policy();
    let token_error = with_token.enrol_root(EnrolledRoot::new(
        RootId::from_uuid(Uuid::from_u128(51)),
        Location::new(
            fm_domain::ProviderId::new("sftp"),
            "sftp://example.test/folder?session=secret",
        ),
        None,
        true,
    ));

    assert!(matches!(
        userinfo_error,
        Err(fm_semantic_library::PolicyError::LocationUserInfo)
    ));
    assert!(matches!(
        token_error,
        Err(fm_semantic_library::PolicyError::TransientLocationData)
    ));
}

#[test]
fn policy_store_rejects_silent_library_or_model_identity_replacement() {
    let directory = project_temp_dir("stable-identity-");
    let store = SemanticLibraryPolicyStore::new(directory.path());
    let original = policy();
    store.save(&original).unwrap();
    let replacement = SemanticLibraryPolicy::new(
        DeviceLibraryIdentity::new(
            LibraryId::new(),
            ModelIdentity::new("other-model", "revision-2", 768, "space-v2").unwrap(),
        ),
        ResourceProfile {
            kind: ResourceProfileKind::Quality,
            budgets: ResourceBudgets::default(),
        },
    )
    .unwrap();

    assert!(matches!(
        store.save(&replacement),
        Err(fm_semantic_library::StoreError::IdentityChanged)
    ));
    assert_eq!(store.load().unwrap().library(), original.library());
}

fn write_policy_document(directory: &Path, json: &str) {
    std::fs::create_dir_all(directory).expect("create configuration directory");
    std::fs::write(directory.join(fm_semantic_library::POLICY_FILE_NAME), json)
        .expect("write policy document");
}

fn valid_policy_json(schema_version: &str) -> String {
    format!(
        r#"{{
          "schemaVersion": {schema_version},
          "revision": 1,
          "library": {{
            "id": "11111111-1111-4111-8111-111111111111",
            "model": {{
              "modelId": "fixture-model",
              "revision": "immutable-revision",
              "dimensions": 384,
              "embeddingSpace": "fixture-space"
            }}
          }},
          "resourceProfile": {{
            "kind": "balanced",
            "budgets": {{
              "maxDocuments": 1000,
              "maxSourceBytesPerDocument": 1048576,
              "maxTotalSourceBytes": 10485760,
              "maxTotalExtractedBytes": 5242880,
              "maxTotalVectorBytes": 2097152
            }}
          }},
          "reconciliationIntervalSeconds": 1800,
          "roots": {{}}
        }}"#
    )
}

#[test]
fn policy_migration_rejects_every_untrustworthy_schema_version() {
    let cases = [
        (
            "missing",
            valid_policy_json("null").replace("\"schemaVersion\": null,", ""),
            fm_settings::SchemaVersionError::Missing,
        ),
        (
            "string",
            valid_policy_json("\"2\""),
            fm_settings::SchemaVersionError::NotAnInteger,
        ),
        (
            "negative",
            valid_policy_json("-1"),
            fm_settings::SchemaVersionError::NotAnInteger,
        ),
        (
            "fractional",
            valid_policy_json("1.5"),
            fm_settings::SchemaVersionError::NotAnInteger,
        ),
        (
            "enormous",
            valid_policy_json("4294967296"),
            fm_settings::SchemaVersionError::NotAnInteger,
        ),
        (
            "zero",
            valid_policy_json("0"),
            fm_settings::SchemaVersionError::Unsupported {
                found: 0,
                current: CURRENT_POLICY_SCHEMA_VERSION,
            },
        ),
        (
            "future",
            valid_policy_json(&(CURRENT_POLICY_SCHEMA_VERSION + 1).to_string()),
            fm_settings::SchemaVersionError::Unsupported {
                found: CURRENT_POLICY_SCHEMA_VERSION + 1,
                current: CURRENT_POLICY_SCHEMA_VERSION,
            },
        ),
    ];

    for (name, json, expected) in cases {
        let directory = project_temp_dir("policy-schema-");
        write_policy_document(directory.path(), &json);
        let store = SemanticLibraryPolicyStore::new(directory.path());

        let error = store.load().expect_err(&format!(
            "a {name} schema version must never be silently accepted"
        ));

        match error {
            fm_semantic_library::StoreError::PolicyDocument(
                fm_settings::DocumentError::SchemaVersion(actual),
            ) => assert_eq!(actual, expected, "{name} version"),
            other => panic!("{name} version produced {other:?}"),
        }
    }
}

#[test]
fn a_current_schema_version_still_loads_after_the_strict_check() {
    let directory = project_temp_dir("policy-schema-ok-");
    write_policy_document(
        directory.path(),
        &valid_policy_json(&CURRENT_POLICY_SCHEMA_VERSION.to_string()),
    );

    let loaded = SemanticLibraryPolicyStore::new(directory.path())
        .load()
        .expect("current schema version loads");

    assert_eq!(loaded.schema_version(), CURRENT_POLICY_SCHEMA_VERSION);
}

#[test]
fn semantic_consent_lives_beside_settings_and_survives_a_stale_settings_write() {
    let directory = project_temp_dir("policy-settings-");
    let settings_store = fm_settings::SettingsStore::new(directory.path());
    let policy_store = SemanticLibraryPolicyStore::from_settings_store(settings_store.clone());
    let mut policy = policy();
    let root_id = RootId::from_uuid(Uuid::from_u128(60));
    policy
        .enrol_root(EnrolledRoot::new(
            root_id,
            Location::parse("file:///consented").expect("root location"),
            None,
            true,
        ))
        .expect("enrol root");
    settings_store
        .save(&fm_settings::Settings {
            theme: fm_settings::Theme::Dark,
            ..fm_settings::Settings::default()
        })
        .expect("save settings");
    policy_store.save(&policy).expect("save policy");

    // A client that still holds pre-consent general settings writes them back.
    settings_store
        .save(&fm_settings::Settings::default())
        .expect("stale settings write");

    assert_eq!(
        policy_store.load().expect("policy survives").root(root_id),
        policy.root(root_id)
    );
    assert_eq!(
        policy_store.path(),
        directory.path().join(fm_semantic_library::POLICY_FILE_NAME)
    );
    assert!(directory.path().join("settings.json").exists());
    assert_eq!(
        settings_store.load().expect("settings load").settings.theme,
        fm_settings::Theme::Auto
    );
}

#[test]
fn a_persisted_policy_never_contains_credentials_or_session_material() {
    let directory = project_temp_dir("policy-credentials-");
    let store = SemanticLibraryPolicyStore::new(directory.path());
    let mut policy = policy();
    policy
        .enrol_root(EnrolledRoot::new(
            RootId::from_uuid(Uuid::from_u128(61)),
            Location::parse("file:///documents").expect("root location"),
            Some(FilesystemIdentity::new("volume-a", "file-61").expect("identity")),
            true,
        ))
        .expect("enrol root");
    store.save(&policy).expect("save policy");

    let persisted = std::fs::read_to_string(store.path()).expect("read policy");

    assert!(!persisted.contains('@'));
    assert!(!persisted.contains("token"));
    assert!(!persisted.contains("password"));
    assert!(!persisted.contains("secret"));
}
