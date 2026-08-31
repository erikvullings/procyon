//! Engine-job level tests: capability gating, streamed results, operation
//! events and cancellation through the job layer rather than the core
//! functions (task 0077).

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use fm_checksum::{
    ChecksumAlgorithm, ChecksumEngine, ChecksumEngineError, ChecksumJobOptions,
    ChecksumResultsStore, ChecksumTarget, DuplicateOptions, DuplicateResultsStore,
    DuplicateScanOptions,
};
use fm_domain::{EntryId, Location, OperationId, ProviderId};
use fm_events::{
    BackendEventPayload, EventAudience, EventBus, OperationStatePayload, SessionId,
    SubscriptionEvent,
};
use fm_vfs::{EntryRef, ProviderRegistry};
use fm_vfs_local::LocalFileSystemProvider;
use uuid::Uuid;

struct Harness {
    engine: ChecksumEngine,
    checksums: Arc<ChecksumResultsStore>,
    duplicates: Arc<DuplicateResultsStore>,
    events: EventBus,
    workspace: Uuid,
}

fn harness() -> Harness {
    let checksums = Arc::new(ChecksumResultsStore::new());
    let duplicates = Arc::new(DuplicateResultsStore::new());
    let events = EventBus::new(1024);
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(LocalFileSystemProvider::new()));
    let engine = ChecksumEngine::new(
        Arc::clone(&checksums),
        Arc::clone(&duplicates),
        events.clone(),
        providers,
    );
    Harness {
        engine,
        checksums,
        duplicates,
        events,
        workspace: Uuid::new_v4(),
    }
}

impl Harness {
    fn audience(&self) -> EventAudience {
        EventAudience::Workspace(self.workspace.into())
    }
}

fn target(path: &std::path::Path, relative: &str) -> ChecksumTarget {
    let size = fs::metadata(path).map_or(0, |metadata| metadata.len());
    ChecksumTarget {
        entry: EntryRef {
            id: EntryId::new(),
            location: Location::from_native_path(path).expect("valid location"),
        },
        relative_path: relative.to_owned(),
        size,
    }
}

/// Waits for a job to finish, failing rather than hanging if it never does.
async fn await_checksum_completion(store: &ChecksumResultsStore, job_id: Uuid) {
    for _ in 0..400 {
        if let Some(page) = store.page(job_id, 0, 1)
            && page.is_complete
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("checksum job did not complete in time");
}

#[tokio::test]
async fn hashes_a_selection_and_streams_results_into_the_store() {
    let directory = tempfile::tempdir().expect("temp dir");
    let a = directory.path().join("a.txt");
    let b = directory.path().join("b.txt");
    fs::write(&a, b"abc").expect("write");
    fs::write(&b, b"").expect("write");

    let harness = harness();
    let job_id = Uuid::new_v4();
    harness
        .engine
        .start_checksums(
            job_id,
            vec![target(&a, "a.txt"), target(&b, "b.txt")],
            ChecksumJobOptions {
                algorithms: vec![ChecksumAlgorithm::Sha256, ChecksumAlgorithm::Md5],
                operation_id: Some(OperationId::from(job_id)),
            },
            harness.audience(),
        )
        .expect("job must start");

    await_checksum_completion(&harness.checksums, job_id).await;

    let page = harness
        .checksums
        .page(job_id, 0, 10)
        .expect("job must be tracked");
    assert_eq!(page.total, 2);
    assert_eq!(page.total_entries, 2);
    assert!(page.is_complete);
    assert!(!page.is_cancelled);
    assert!(!page.has_more);

    let first = &page.entries[0];
    assert_eq!(first.relative_path, "a.txt");
    assert_eq!(
        first.checksums.get(ChecksumAlgorithm::Sha256),
        Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    );
    assert_eq!(
        first.checksums.get(ChecksumAlgorithm::Md5),
        Some("900150983cd24fb0d6963f7d28e17f72")
    );
    assert!(first.error.is_none());
    assert_eq!(
        page.entries[1].checksums.get(ChecksumAlgorithm::Sha256),
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    );
}

#[tokio::test]
async fn publishes_a_results_batch_and_a_terminal_operation_state() {
    let directory = tempfile::tempdir().expect("temp dir");
    let a = directory.path().join("a.txt");
    fs::write(&a, b"abc").expect("write");

    let harness = harness();
    let mut subscriber =
        harness
            .events
            .subscribe(SessionId::new("test"), [harness.workspace.into()], None);
    let job_id = Uuid::new_v4();
    harness
        .engine
        .start_checksums(
            job_id,
            vec![target(&a, "a.txt")],
            ChecksumJobOptions {
                algorithms: vec![ChecksumAlgorithm::Sha256],
                operation_id: Some(OperationId::from(job_id)),
            },
            harness.audience(),
        )
        .expect("job must start");

    let mut saw_batch = false;
    let mut saw_completion = false;
    for _ in 0..50 {
        let Ok(Ok(SubscriptionEvent::Event(envelope))) =
            tokio::time::timeout(Duration::from_secs(5), subscriber.recv()).await
        else {
            break;
        };
        match envelope.payload {
            BackendEventPayload::ChecksumResultsBatch {
                job_id: batch_id,
                ref entries,
                is_complete,
                ..
            } => {
                assert_eq!(batch_id, job_id);
                if !entries.is_empty() {
                    assert_eq!(
                        entries[0].checksums.get("sha256").map(String::as_str),
                        Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
                    );
                    saw_batch = true;
                }
                if is_complete {
                    // keep reading for the operation state change
                }
            }
            BackendEventPayload::OperationStateChanged { state, .. } => {
                assert_eq!(state, OperationStatePayload::Completed);
                saw_completion = true;
                break;
            }
            _ => {}
        }
    }
    assert!(saw_batch, "a results batch must be published");
    assert!(
        saw_completion,
        "a terminal operation state must be published"
    );
}

#[tokio::test]
async fn rejects_a_provider_without_the_checksum_capability() {
    let harness = harness();
    // `search://` locations resolve to no registered provider here, standing
    // in for any provider that cannot checksum.
    let unsupported = Location::new(ProviderId::new("sftp"), "sftp://host/file.txt");
    let error = harness
        .engine
        .start_checksums(
            Uuid::new_v4(),
            vec![ChecksumTarget {
                entry: EntryRef {
                    id: EntryId::new(),
                    location: unsupported,
                },
                relative_path: "file.txt".to_owned(),
                size: 1,
            }],
            ChecksumJobOptions {
                algorithms: vec![ChecksumAlgorithm::Sha256],
                operation_id: None,
            },
            harness.audience(),
        )
        .expect_err("an unregistered provider must be rejected");
    assert!(
        matches!(error, ChecksumEngineError::InvalidTarget(_)),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn rejects_an_empty_algorithm_or_target_list() {
    let directory = tempfile::tempdir().expect("temp dir");
    let a = directory.path().join("a.txt");
    fs::write(&a, b"abc").expect("write");
    let harness = harness();

    let no_algorithms = harness
        .engine
        .start_checksums(
            Uuid::new_v4(),
            vec![target(&a, "a.txt")],
            ChecksumJobOptions {
                algorithms: Vec::new(),
                operation_id: None,
            },
            harness.audience(),
        )
        .expect_err("an empty algorithm list must be rejected");
    assert!(matches!(
        no_algorithms,
        ChecksumEngineError::EmptyRequest(_)
    ));

    let no_targets = harness
        .engine
        .start_checksums(
            Uuid::new_v4(),
            Vec::new(),
            ChecksumJobOptions {
                algorithms: vec![ChecksumAlgorithm::Sha256],
                operation_id: None,
            },
            harness.audience(),
        )
        .expect_err("an empty selection must be rejected");
    assert!(matches!(no_targets, ChecksumEngineError::EmptyRequest(_)));
}

#[tokio::test]
async fn records_a_per_entry_error_without_failing_the_whole_job() {
    let directory = tempfile::tempdir().expect("temp dir");
    let good = directory.path().join("good.txt");
    fs::write(&good, b"abc").expect("write");
    let missing = directory.path().join("missing.txt");

    let harness = harness();
    let job_id = Uuid::new_v4();
    harness
        .engine
        .start_checksums(
            job_id,
            vec![target(&good, "good.txt"), target(&missing, "missing.txt")],
            ChecksumJobOptions {
                algorithms: vec![ChecksumAlgorithm::Sha256],
                operation_id: None,
            },
            harness.audience(),
        )
        .expect("job must start");

    await_checksum_completion(&harness.checksums, job_id).await;
    let page = harness
        .checksums
        .page(job_id, 0, 10)
        .expect("job must be tracked");
    assert_eq!(page.total, 2);
    assert!(page.entries[0].error.is_none());
    assert!(
        page.entries[1].error.is_some(),
        "the unreadable entry must carry an error, not abort the job"
    );
}

#[tokio::test]
async fn cancelling_a_checksum_job_through_the_engine_stops_it() {
    let directory = tempfile::tempdir().expect("temp dir");
    let payload = vec![b'q'; 4 * 1024 * 1024];
    let targets: Vec<ChecksumTarget> = (0..60)
        .map(|index| {
            let path = directory.path().join(format!("f{index:02}.bin"));
            fs::write(&path, &payload).expect("write");
            target(&path, &format!("f{index:02}.bin"))
        })
        .collect();

    let harness = harness();
    let job_id = Uuid::new_v4();
    harness
        .engine
        .start_checksums(
            job_id,
            targets,
            ChecksumJobOptions {
                algorithms: vec![ChecksumAlgorithm::Sha256],
                operation_id: Some(OperationId::from(job_id)),
            },
            harness.audience(),
        )
        .expect("job must start");

    harness
        .engine
        .cancel_checksums(job_id)
        .expect("cancellation must be accepted");

    await_checksum_completion(&harness.checksums, job_id).await;
    let page = harness
        .checksums
        .page(job_id, 0, 200)
        .expect("job must be tracked");
    assert!(page.is_complete);
    assert!(
        page.is_cancelled,
        "a cancelled job must be flagged, not reported as a clean finish"
    );
    assert!(
        page.total < 60,
        "cancellation must stop before every entry is hashed, got {}",
        page.total
    );
}

#[tokio::test]
async fn cancelling_an_unknown_job_reports_not_found() {
    let harness = harness();
    assert!(matches!(
        harness.engine.cancel_checksums(Uuid::new_v4()),
        Err(ChecksumEngineError::NotFound(_))
    ));
    assert!(matches!(
        harness.engine.cancel_duplicate_scan(Uuid::new_v4()),
        Err(ChecksumEngineError::NotFound(_))
    ));
}

#[tokio::test]
async fn scans_a_root_for_duplicates_through_the_engine() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path();
    fs::create_dir_all(root.join("nested")).expect("mkdir");
    fs::write(root.join("one.txt"), b"identical payload").expect("write");
    fs::write(root.join("nested/two.txt"), b"identical payload").expect("write");
    fs::write(root.join("unique.txt"), b"something else entirely").expect("write");

    let harness = harness();
    let scan_id = Uuid::new_v4();
    harness
        .engine
        .start_duplicate_scan(
            scan_id,
            vec![Location::from_native_path(root).expect("valid root")],
            DuplicateScanOptions {
                detection: DuplicateOptions::default(),
                show_hidden: false,
                operation_id: Some(OperationId::from(scan_id)),
            },
            harness.audience(),
        )
        .expect("scan must start");

    let mut page = None;
    for _ in 0..400 {
        if let Some(current) = harness.duplicates.page(scan_id, 0, 10)
            && current.is_complete
        {
            page = Some(current);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let page = page.expect("duplicate scan did not complete in time");

    assert!(!page.is_cancelled);
    assert_eq!(page.total, 1, "exactly one duplicate group is expected");
    let group = &page.groups[0];
    assert_eq!(group.distinct_files.len(), 2);
    assert_eq!(group.size, 17);
    assert!(group.hardlink_clusters.is_empty());
    // The uniquely-sized file must never have been hashed.
    assert_eq!(page.stats.fully_hashed, 2);
}
