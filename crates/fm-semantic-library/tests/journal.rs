#![allow(clippy::unwrap_used, missing_docs)]

use std::path::{Path, PathBuf};

use fm_domain::{EntryId, Location, WorkspaceId};
use fm_semantic_library::{
    CatalogObservation, CommitStep, ConsentState, ContentFingerprint, DeletionCategory,
    DeviceLibraryIdentity, DocumentArtifacts, DocumentMeasurement, EnrolledRoot,
    ExclusionCleanupStatus, ExclusionId, LibraryId, LibraryOperation, ModelIdentity,
    OccurrenceScope, ResourceBudgets, ResourceProfile, ResourceProfileKind, RootId,
    SemanticCatalog, SemanticLibraryCoordinator, SemanticLibraryPolicy, SemanticLibraryState,
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

const LIBRARY: u128 = 1;
const ROOT: u128 = 10;
const WORKSPACE: u128 = 20;
const EXCLUSION: u128 = 30;

fn library_id() -> LibraryId {
    LibraryId::from_uuid(Uuid::from_u128(LIBRARY))
}

fn root_id() -> RootId {
    RootId::from_uuid(Uuid::from_u128(ROOT))
}

fn workspace_id() -> WorkspaceId {
    WorkspaceId::from(Uuid::from_u128(WORKSPACE))
}

fn exclusion_id() -> ExclusionId {
    ExclusionId::from_uuid(Uuid::from_u128(EXCLUSION))
}

fn seeded_library() -> (SemanticLibraryPolicy, SemanticCatalog) {
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
    let mut root = EnrolledRoot::new(root_id(), location("file:///docs"), None, true);
    root.attach_workspace(workspace_id());
    policy.enrol_root(root).unwrap();
    let mut catalog = SemanticCatalog::new(library_id());
    catalog
        .upsert_observations(
            &policy,
            &SemanticLibraryState::new(library_id()),
            [CatalogObservation::new(
                EntryId::from(Uuid::from_u128(100)),
                location("file:///docs/private/secret.txt"),
                ContentFingerprint::new("sha256:secret").unwrap(),
                OccurrenceScope::new(workspace_id(), root_id()),
                DocumentArtifacts::default(),
                DocumentMeasurement::new(64, 32, 16),
            )],
        )
        .unwrap();
    (policy, catalog)
}

fn coordinator(configuration: &Path, semantic_root: &Path) -> SemanticLibraryCoordinator {
    SemanticLibraryCoordinator::new(configuration, semantic_root)
}

fn journal_record_count(semantic_root: &Path) -> usize {
    let journal: PathBuf = semantic_root.join("journal");
    std::fs::read_dir(journal)
        .map(|entries| entries.filter_map(Result::ok).count())
        .unwrap_or(0)
}

const COMMIT_STEPS: [CommitStep; 8] = [
    CommitStep::StageCatalog,
    CommitStep::StagePolicy,
    CommitStep::StageState,
    CommitStep::RecordIntent,
    CommitStep::InstallCatalog,
    CommitStep::InstallPolicy,
    CommitStep::InstallState,
    CommitStep::ClearJournal,
];

#[test]
fn every_commit_interruption_recovers_deterministically_from_disk() {
    for completed_steps in 0..=COMMIT_STEPS.len() {
        let configuration = project_temp_dir("journal-config-");
        let semantic_root = project_temp_dir("journal-data-");
        let (mut policy, mut catalog) = seeded_library();
        let mut state = SemanticLibraryState::new(library_id());
        let baseline = coordinator(configuration.path(), semantic_root.path());
        baseline
            .lock()
            .unwrap()
            .transaction(
                LibraryOperation::Enrolment,
                Some(&policy),
                Some(&catalog),
                Some(&state),
            )
            .unwrap()
            .commit()
            .unwrap();

        policy
            .exclude_descendant(root_id(), exclusion_id(), location("file:///docs/private"))
            .unwrap();
        policy.advance_revision().unwrap();
        let plan_id = catalog
            .begin_exclusion_cleanup(&mut policy, root_id(), exclusion_id())
            .unwrap();
        // Pausing in the same transaction proves runtime state is a real third
        // participant rather than a best-effort write beside the journal.
        state.pause();
        let session = baseline.lock().unwrap();
        let mut transaction = session
            .transaction(
                LibraryOperation::ExclusionCleanup,
                Some(&policy),
                Some(&catalog),
                Some(&state),
            )
            .unwrap();
        for _ in 0..completed_steps {
            assert!(transaction.advance().unwrap().is_some());
        }
        let interrupted_after = completed_steps
            .checked_sub(1)
            .map(|index| COMMIT_STEPS[index]);
        transaction.interrupt();
        drop(session);

        // Proof that the two files really are independent: interrupting
        // between the installs leaves the catalog ahead of the policy on disk.
        let raw_policy =
            std::fs::read_to_string(configuration.path().join("semantic-library-policy.json"))
                .unwrap();
        let raw_catalog =
            std::fs::read_to_string(semantic_root.path().join("catalog").join("catalog.json"))
                .unwrap();
        if interrupted_after == Some(CommitStep::InstallCatalog) {
            assert!(
                raw_catalog.contains(&plan_id.to_string()),
                "the installed catalog must already carry the cleanup plan"
            );
            assert!(
                !raw_policy.contains(&exclusion_id().to_string()),
                "the policy write has not landed yet, which is exactly the gap the journal closes"
            );
        }

        // A crashed process leaves the journal exactly as it was; a brand new
        // coordinator reads only what is on disk.
        let restarted = coordinator(configuration.path(), semantic_root.path());
        let recovery = restarted.recover().unwrap();
        let loaded = restarted.load().unwrap();
        let committed = interrupted_after.is_some_and(CommitStep::is_at_or_after_commit);

        assert_eq!(
            journal_record_count(semantic_root.path()),
            0,
            "recovery must leave no journal record after {interrupted_after:?}"
        );
        if committed {
            assert!(
                loaded.policy.exclusion(root_id(), exclusion_id()).is_some(),
                "a committed exclusion must survive interruption after {interrupted_after:?}"
            );
            assert!(
                loaded.catalog.deletion_plan(plan_id).is_some(),
                "a committed cleanup plan must survive interruption after {interrupted_after:?}"
            );
            assert!(matches!(
                loaded
                    .policy
                    .consent_state(&location("file:///docs/private/secret.txt"))
                    .unwrap(),
                ConsentState::Excluded { .. }
            ));
            assert_eq!(
                recovery.rolled_back.len(),
                0,
                "a committed record is never rolled back after {interrupted_after:?}"
            );
            assert!(
                loaded.state.is_paused(),
                "committed runtime state must be replayed after {interrupted_after:?}"
            );
        } else {
            assert!(
                loaded.policy.exclusion(root_id(), exclusion_id()).is_none(),
                "an uncommitted exclusion must not appear after {interrupted_after:?}"
            );
            assert!(
                loaded.catalog.deletion_plan(plan_id).is_none(),
                "an uncommitted cleanup plan must not appear after {interrupted_after:?}"
            );
            assert_eq!(loaded.catalog.occurrence_count(), 1);
            assert!(
                !loaded.state.is_paused(),
                "an uncommitted pause must not appear after {interrupted_after:?}"
            );
        }
        assert_eq!(
            loaded.catalog.deletion_plan(plan_id).is_some(),
            loaded
                .policy
                .exclusion(root_id(), exclusion_id())
                .and_then(|exclusion| exclusion.deletion_plan_id())
                .is_some(),
            "policy and catalog must never disagree about a cleanup plan after {interrupted_after:?}"
        );
        assert!(restarted.recover().unwrap().is_clean());
    }
}

#[test]
fn an_interrupted_cleanup_step_resumes_without_losing_scope_denial() {
    let configuration = project_temp_dir("journal-resume-config-");
    let semantic_root = project_temp_dir("journal-resume-data-");
    let coordinator = coordinator(configuration.path(), semantic_root.path());
    let (mut policy, mut catalog) = seeded_library();
    policy
        .exclude_descendant(root_id(), exclusion_id(), location("file:///docs/private"))
        .unwrap();
    let plan_id = catalog
        .begin_exclusion_cleanup(&mut policy, root_id(), exclusion_id())
        .unwrap();
    coordinator
        .lock()
        .unwrap()
        .transaction(
            LibraryOperation::ScopeRevocation,
            Some(&policy),
            Some(&catalog),
            Some(&SemanticLibraryState::new(library_id())),
        )
        .unwrap()
        .commit()
        .unwrap();

    // Interrupt the very next cleanup step after the catalog has already been
    // installed but before the policy write lands.
    let mut loaded = coordinator.load().unwrap();
    loaded
        .catalog
        .complete_deletion_category(&mut loaded.policy, plan_id, DeletionCategory::Occurrences)
        .unwrap();
    loaded.policy.advance_revision().unwrap();
    let session = coordinator.lock().unwrap();
    let mut transaction = session
        .transaction(
            LibraryOperation::ExclusionCleanup,
            Some(&loaded.policy),
            Some(&loaded.catalog),
            Some(&loaded.state),
        )
        .unwrap();
    while transaction.next_step() != Some(CommitStep::InstallPolicy) {
        transaction.advance().unwrap();
    }
    transaction.interrupt();
    drop(session);

    let restarted = SemanticLibraryCoordinator::new(configuration.path(), semantic_root.path());
    let recovered = restarted.load().unwrap();

    assert_eq!(recovered.recovery.completed.len(), 1);
    assert!(recovered.recovery.rolled_back.is_empty());
    assert_eq!(
        recovered
            .policy
            .exclusion(root_id(), exclusion_id())
            .unwrap()
            .cleanup_status(),
        ExclusionCleanupStatus::Pending
    );
    assert!(matches!(
        recovered
            .policy
            .consent_state(&location("file:///docs/private/secret.txt"))
            .unwrap(),
        ConsentState::Excluded { .. }
    ));
    assert_eq!(recovered.catalog.occurrence_count(), 0);
    assert!(
        recovered
            .catalog
            .deletion_plan(plan_id)
            .unwrap()
            .progress(DeletionCategory::Occurrences)
            .is_complete()
    );

    let mut resumed = recovered;
    for category in DeletionCategory::all() {
        if !resumed
            .catalog
            .deletion_plan(plan_id)
            .unwrap()
            .progress(*category)
            .is_complete()
        {
            resumed
                .catalog
                .complete_deletion_category(&mut resumed.policy, plan_id, *category)
                .unwrap();
        }
    }
    resumed.policy.advance_revision().unwrap();
    restarted
        .lock()
        .unwrap()
        .transaction(
            LibraryOperation::ExclusionCleanup,
            Some(&resumed.policy),
            Some(&resumed.catalog),
            Some(&resumed.state),
        )
        .unwrap()
        .commit()
        .unwrap();
    let finished = restarted.load().unwrap();

    assert_eq!(
        finished
            .policy
            .exclusion(root_id(), exclusion_id())
            .unwrap()
            .cleanup_status(),
        ExclusionCleanupStatus::Complete
    );
    assert_eq!(finished.catalog.document_count(), 0);
    assert!(matches!(
        finished
            .policy
            .consent_state(&location("file:///docs/private/secret.txt"))
            .unwrap(),
        ConsentState::Excluded { .. }
    ));
    assert_eq!(journal_record_count(semantic_root.path()), 0);
}

#[test]
fn a_transaction_cannot_mix_libraries_or_install_a_foreign_record() {
    let configuration = project_temp_dir("journal-foreign-config-");
    let semantic_root = project_temp_dir("journal-foreign-data-");
    let coordinator = coordinator(configuration.path(), semantic_root.path());
    let (policy, catalog) = seeded_library();
    let foreign = SemanticCatalog::new(LibraryId::from_uuid(Uuid::from_u128(999)));

    let session = coordinator.lock().unwrap();
    let mixed = session.transaction(
        LibraryOperation::Enrolment,
        Some(&policy),
        Some(&foreign),
        None,
    );

    assert!(matches!(
        mixed,
        Err(fm_semantic_library::StoreError::LibraryMismatch)
    ));
    assert_eq!(journal_record_count(semantic_root.path()), 0);

    session
        .transaction(
            LibraryOperation::Enrolment,
            Some(&policy),
            Some(&catalog),
            None,
        )
        .unwrap()
        .commit()
        .unwrap();
    drop(session);

    assert_eq!(
        coordinator.load().unwrap().catalog.library_id(),
        library_id()
    );
}

#[test]
fn a_committed_record_blocks_a_second_transaction_until_recovery_installs_it() {
    let configuration = project_temp_dir("journal-pending-config-");
    let semantic_root = project_temp_dir("journal-pending-data-");
    let coordinator = coordinator(configuration.path(), semantic_root.path());
    let (mut policy, catalog) = seeded_library();
    let state = SemanticLibraryState::new(library_id());
    let session = coordinator.lock().unwrap();
    let mut first = session
        .transaction(
            LibraryOperation::Enrolment,
            Some(&policy),
            Some(&catalog),
            Some(&state),
        )
        .unwrap();
    while first.next_step() != Some(CommitStep::InstallCatalog) {
        first.advance().unwrap();
    }
    first.interrupt();

    // The intent is durable but the documents are not installed. Staging a
    // second transaction over it would write documents derived from a snapshot
    // that predates the committed one.
    policy.advance_revision().unwrap();
    let blocked = session.transaction(
        LibraryOperation::Enrolment,
        Some(&policy),
        Some(&catalog),
        Some(&state),
    );

    assert!(matches!(
        blocked,
        Err(fm_semantic_library::StoreError::PendingJournalRecord)
    ));

    // Recovery installs the committed record, after which mutation resumes.
    session.recover().unwrap();
    assert_eq!(journal_record_count(semantic_root.path()), 0);
    session
        .transaction(
            LibraryOperation::Enrolment,
            Some(&policy),
            Some(&catalog),
            Some(&state),
        )
        .unwrap()
        .commit()
        .unwrap();
    drop(session);
    assert_eq!(coordinator.load().unwrap().policy.revision(), 2);
}

#[test]
fn a_first_use_session_materialises_the_cross_process_lock_before_reading() {
    let configuration = project_temp_dir("journal-late-lock-config-");
    let parent = project_temp_dir("journal-late-lock-data-");
    // The semantic-data root deliberately does not exist yet. Recovery runs on
    // the read path too, so even the very first read has to take the
    // cross-process lock — and therefore create the root and the lock file —
    // rather than racing a process that is committing right now.
    let semantic_root = parent.path().join("data");
    let coordinator = coordinator(configuration.path(), &semantic_root);

    assert!(
        !semantic_root.exists(),
        "constructing a coordinator must stay inert"
    );

    let session = coordinator.lock().unwrap();
    assert_eq!(session.durable_revision().unwrap(), None);
    assert!(
        semantic_root.join("library.lock").is_file(),
        "the first read must materialise the cross-process lock"
    );
    assert!(
        !semantic_root.join("catalog").exists() && !semantic_root.join("state").exists(),
        "a read that finds nothing must not create library documents"
    );

    let (policy, catalog) = seeded_library();
    let state = SemanticLibraryState::new(library_id());
    session
        .transaction(
            LibraryOperation::Enrolment,
            Some(&policy),
            Some(&catalog),
            Some(&state),
        )
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(
        session.durable_revision().unwrap(),
        Some(policy.revision()),
        "the lock-held probe must report what is durable right now"
    );
    drop(session);

    assert_eq!(
        coordinator.load().unwrap().policy.revision(),
        policy.revision()
    );
}

#[test]
fn two_processes_starting_from_a_missing_root_cannot_recover_concurrently() {
    let configuration = project_temp_dir("journal-first-use-config-");
    let parent = project_temp_dir("journal-first-use-data-");
    let semantic_root = parent.path().join("data");
    // Two independent coordinators stand in for two processes: the in-process
    // registry is keyed by path, so they would share a mutex — the file lock is
    // what has to serialize them, and it only can if it exists.
    let first = coordinator(configuration.path(), &semantic_root);
    let second = coordinator(configuration.path(), &semantic_root);

    let held = first.lock().unwrap();
    let lock_path = semantic_root.join("library.lock");
    assert!(
        lock_path.is_file(),
        "the first session must have created the shared lock file"
    );

    // A second, independent open of that same lock file must not be grantable
    // while the first session holds it. `try_lock_exclusive` is what a real
    // second process would contend on.
    let probe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    assert!(
        fs2::FileExt::try_lock_exclusive(&probe).is_err(),
        "a session over a freshly created root must hold the cross-process lock"
    );
    drop(probe);
    drop(held);

    // After the first session ends, the second acquires it and sees a library
    // that is still empty rather than one it half-created itself.
    let session = second.lock().unwrap();
    assert_eq!(session.durable_revision().unwrap(), None);
    assert!(session.recover().unwrap().is_clean());
}
