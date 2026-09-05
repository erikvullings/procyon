#![allow(clippy::unwrap_used, missing_docs)]

use std::path::Path;

use fm_semantic_library::{
    CURRENT_LIBRARY_STATE_SCHEMA_VERSION, EligibilityDecision, EligibilityReason,
    EligibilityReasonCounts, LibraryId, RootId, SemanticLibraryState, SemanticLibraryStateStore,
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

#[test]
fn pause_is_durable_and_preserves_consent_independent_indexed_generations() {
    let library_id = LibraryId::from_uuid(Uuid::from_u128(1));
    let root_id = RootId::from_uuid(Uuid::from_u128(2));
    let mut state = SemanticLibraryState::new(library_id);
    state.record_indexed_generation(root_id, 7).unwrap();
    state.pause();
    let directory = project_temp_dir("paused-state-");
    let store = SemanticLibraryStateStore::new(directory.path());

    store.save(&state).expect("save paused state");
    let loaded = store.load().expect("load paused state");

    assert!(loaded.is_paused());
    assert_eq!(loaded.indexed_generation(root_id), 7);
    assert_eq!(loaded.library_id(), library_id);
}

#[test]
fn durable_eligibility_reason_counts_round_trip_per_root() {
    let library_id = LibraryId::from_uuid(Uuid::from_u128(1));
    let first = RootId::from_uuid(Uuid::from_u128(2));
    let second = RootId::from_uuid(Uuid::from_u128(3));
    let mut state = SemanticLibraryState::new(library_id);
    state.set_eligibility_reason_counts(
        first,
        EligibilityReasonCounts::from_decisions([
            EligibilityDecision::Skipped([EligibilityReason::Hidden].into()),
            EligibilityDecision::Skipped([EligibilityReason::Hidden].into()),
            EligibilityDecision::Skipped([EligibilityReason::UnsupportedMime].into()),
        ]),
    );
    let directory = project_temp_dir("reason-counts-");
    let store = SemanticLibraryStateStore::new(directory.path());

    store.save(&state).expect("save counts");
    let loaded = store.load().expect("load counts");

    let counts = loaded
        .eligibility_reason_counts(first)
        .expect("counts for the enrolled root");
    assert_eq!(counts.get(EligibilityReason::Hidden), 2);
    assert_eq!(counts.get(EligibilityReason::UnsupportedMime), 1);
    assert!(loaded.eligibility_reason_counts(second).is_none());

    // Dropping a root drops its metadata rather than leaking it forever.
    let mut pruned = loaded;
    pruned.retain_roots(|root_id| root_id != first);
    store.save(&pruned).expect("save pruned counts");
    assert!(
        store
            .load()
            .expect("load pruned counts")
            .eligibility_reason_counts(first)
            .is_none()
    );
}

#[test]
fn a_version_one_state_document_migrates_to_empty_reason_counts() {
    let library_id = LibraryId::from_uuid(Uuid::from_u128(1));
    let root_id = RootId::from_uuid(Uuid::from_u128(2));
    let directory = project_temp_dir("state-migration-");
    let store = SemanticLibraryStateStore::new(directory.path());
    let legacy = serde_json::json!({
        "schemaVersion": 1,
        "libraryId": library_id,
        "paused": true,
        "indexedGenerations": { root_id.to_string(): 4 },
    });
    let path = store.path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();

    let loaded = store.load().expect("a version one document must migrate");

    assert!(loaded.is_paused());
    assert_eq!(loaded.indexed_generation(root_id), 4);
    assert!(
        loaded.eligibility_reason_counts(root_id).is_none(),
        "counts a previous build only kept in memory must migrate to none, not to invented values"
    );

    // The migrated document is written back at the current schema version.
    store.save(&loaded).expect("save migrated state");
    let stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).expect("stored json");
    assert_eq!(
        stored["schemaVersion"],
        serde_json::json!(CURRENT_LIBRARY_STATE_SCHEMA_VERSION)
    );
}

#[test]
fn an_unknown_state_schema_version_is_refused() {
    let directory = project_temp_dir("state-unsupported-");
    let store = SemanticLibraryStateStore::new(directory.path());
    let path = store.path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 99,
            "libraryId": LibraryId::from_uuid(Uuid::from_u128(1)),
            "paused": false,
            "indexedGenerations": {},
        }))
        .unwrap(),
    )
    .unwrap();

    assert!(store.load_optional().is_err());
}
