//! Public contract tests for task 0035.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fm_domain::{EntryId, EntryKind, Location, ProviderId};
use fm_events::{EventBus, SessionId, SubscriptionEvent};
use fm_operations::{
    ConflictEntry, ConflictPolicy, ConflictResolution, CycleDetector, EntryType, ExecutionError,
    ExecutionOutcome, Operation, OperationConflict, OperationExecutor, OperationKind,
    OperationPlan, OperationState, PauseToken, PlanItem, ProgressPublisher, SafetyError, Scheduler,
    validate_paths, validate_replacement,
};
use fm_vfs::EntryRef;
use tokio_util::sync::CancellationToken;

#[test]
fn a_new_operation_is_queued_with_empty_progress() {
    let operation = Operation::new(OperationKind::Copy, vec![], None, Default::default());

    assert_eq!(operation.state, OperationState::Queued);
    assert_eq!(operation.progress.completed_items, 0);
    assert_eq!(operation.progress.total_items, None);
    assert_eq!(operation.progress.completed_bytes, 0);
    assert_eq!(operation.progress.total_bytes, None);
    assert!(operation.started_at.is_none());
    assert!(operation.completed_at.is_none());
}

#[test]
fn lifecycle_rejects_illegal_transitions_with_context() {
    let mut operation = Operation::new(OperationKind::Copy, vec![], None, ConflictPolicy::Ask);
    let error = operation
        .transition(OperationState::Completed)
        .expect_err("queued cannot complete without planning and execution");

    assert_eq!(error.from, OperationState::Queued);
    assert_eq!(error.to, OperationState::Completed);
}

#[test]
fn lifecycle_allows_planning_execution_and_completion() {
    let mut operation = Operation::new(OperationKind::Copy, vec![], None, ConflictPolicy::Ask);
    operation.transition(OperationState::Planning).unwrap();
    operation.transition(OperationState::Running).unwrap();
    operation.transition(OperationState::Completed).unwrap();

    assert!(operation.started_at.is_some());
    assert!(operation.completed_at.is_some());
}

#[test]
fn planning_totals_realistically_interleaved_items() {
    let small = entry("file:///tmp/tree/a-small");
    let nested = entry("file:///tmp/tree/sub/large");
    let medium = entry("file:///tmp/tree/b-medium");
    let plan = OperationPlan::new(vec![
        PlanItem::new(small, 10),
        PlanItem::new(nested, 1_000),
        PlanItem::new(medium, 100),
    ]);

    assert_eq!(plan.total_items, Some(3));
    assert_eq!(plan.total_bytes, Some(1_110));
}

#[test]
fn progress_is_throttled_and_rate_is_smoothed() {
    let start = Instant::now();
    let mut publisher = ProgressPublisher::new(Duration::from_millis(100), 0.25);
    assert!(publisher.record(start, 100).is_some());
    assert!(
        publisher
            .record(start + Duration::from_millis(20), 1_000)
            .is_none()
    );
    let first = publisher
        .record(start + Duration::from_millis(100), 0)
        .expect("coalesced update is emitted at 10Hz boundary");
    let second = publisher
        .record(start + Duration::from_millis(200), 100)
        .expect("next update is emitted");

    assert_eq!(first.completed_bytes, 1_100);
    assert!(second.bytes_per_second.unwrap() < 10_000);
    assert!(second.bytes_per_second.unwrap() > 1_000);
}

#[test]
fn safety_rejects_same_and_nested_destinations() {
    let source = location("file:///tmp/source");
    assert_eq!(
        validate_paths(&source, &source, true),
        Err(SafetyError::SameEntry)
    );
    assert_eq!(
        validate_paths(&source, &location("file:///tmp/source/child"), true),
        Err(SafetyError::DestinationInsideSource)
    );
}

#[test]
fn safety_rejects_case_only_difference_on_insensitive_filesystem() {
    assert_eq!(
        validate_paths(
            &location("file:///tmp/Report.txt"),
            &location("file:///tmp/report.txt"),
            false,
        ),
        Err(SafetyError::CaseOnlyDifference)
    );
    assert!(
        validate_paths(
            &location("file:///tmp/Report.txt"),
            &location("file:///tmp/report.txt"),
            true,
        )
        .is_ok()
    );
}

#[test]
fn safety_compares_sftp_locations_by_connection_and_path_not_native_path() {
    // task 0104: `sftp://` locations have no native path, so this must not
    // fall through to `Location::to_native_path` (which would always report
    // `IncomparableLocations` for them, wrongly failing every same-connection
    // SFTP copy/move's planning safety check - see this task's Agent Notes).
    let connection = "11111111-1111-4111-8111-111111111111";
    let source = sftp_location(connection, "/home/erik/report.txt");
    assert_eq!(
        validate_paths(&source, &source, true),
        Err(SafetyError::SameEntry)
    );
    assert_eq!(
        validate_paths(
            &sftp_location(connection, "/home/erik"),
            &sftp_location(connection, "/home/erik/nested"),
            true,
        ),
        Err(SafetyError::DestinationInsideSource)
    );
    // Two different connections must never be treated as the same or nested
    // entry, even when their remote paths are textually identical.
    let other_connection = "22222222-2222-4222-8222-222222222222";
    assert!(
        validate_paths(
            &sftp_location(connection, "/home/erik/report.txt"),
            &sftp_location(other_connection, "/home/erik/report.txt"),
            true,
        )
        .is_ok()
    );
    // A genuinely unrelated destination on the same connection is fine.
    assert!(
        validate_paths(
            &sftp_location(connection, "/home/erik/report.txt"),
            &sftp_location(connection, "/home/erik/archive/report.txt"),
            true,
        )
        .is_ok()
    );
}

#[test]
fn safety_compares_ftp_locations_by_connection_and_path_not_native_path() {
    // task 0108: `ftp://`/`ftps://` locations have no native path either, so
    // without their own branch every same-connection FTP copy/move failed its
    // planning safety check with `IncomparableLocations` — see this task's
    // Agent Notes. Mirrors the SFTP case above.
    let connection = "11111111-1111-4111-8111-111111111111";
    let source = ftp_location("ftp", connection, "/pub/report.txt");
    assert_eq!(
        validate_paths(&source, &source, true),
        Err(SafetyError::SameEntry)
    );
    assert_eq!(
        validate_paths(
            &ftp_location("ftp", connection, "/pub"),
            &ftp_location("ftp", connection, "/pub/nested"),
            true,
        ),
        Err(SafetyError::DestinationInsideSource)
    );
    // Two different connections are never the same or a nested entry, even
    // with textually identical remote paths...
    let other_connection = "22222222-2222-4222-8222-222222222222";
    assert!(
        validate_paths(
            &ftp_location("ftp", connection, "/pub/report.txt"),
            &ftp_location("ftp", other_connection, "/pub/report.txt"),
            true,
        )
        .is_ok()
    );
    // ...and neither are the plain and TLS forms of one connection id, which
    // this workspace's FTP provider refuses to mix.
    assert!(
        validate_paths(
            &ftp_location("ftp", connection, "/pub/report.txt"),
            &ftp_location("ftps", connection, "/pub/report.txt"),
            true,
        )
        .is_ok()
    );
    assert!(
        validate_paths(
            &ftp_location("ftp", connection, "/pub/report.txt"),
            &ftp_location("ftp", connection, "/pub/archive/report.txt"),
            true,
        )
        .is_ok()
    );
}

#[test]
fn safety_compares_webdav_locations_by_connection_and_path_not_native_path() {
    // task 0147: `webdav://` locations have no native path either, same
    // reasoning as the SFTP/FTP branches above - without its own branch,
    // every same-connection WebDAV copy/move would fail planning with
    // `IncomparableLocations`.
    let connection = "11111111-1111-4111-8111-111111111111";
    let source = webdav_location(connection, "/Documents/report.txt");
    assert_eq!(
        validate_paths(&source, &source, true),
        Err(SafetyError::SameEntry)
    );
    assert_eq!(
        validate_paths(
            &webdav_location(connection, "/Documents"),
            &webdav_location(connection, "/Documents/nested"),
            true,
        ),
        Err(SafetyError::DestinationInsideSource)
    );
    let other_connection = "22222222-2222-4222-8222-222222222222";
    assert!(
        validate_paths(
            &webdav_location(connection, "/Documents/report.txt"),
            &webdav_location(other_connection, "/Documents/report.txt"),
            true,
        )
        .is_ok()
    );
    assert!(
        validate_paths(
            &webdav_location(connection, "/Documents/report.txt"),
            &webdav_location(connection, "/Documents/archive/report.txt"),
            true,
        )
        .is_ok()
    );
}

#[test]
fn safety_rejects_symlink_cycles() {
    let mut detector = CycleDetector::default();
    detector.observe(8, 42).unwrap();
    assert_eq!(detector.observe(8, 42), Err(SafetyError::SymlinkCycle));
}

#[test]
fn safety_rejects_replacing_different_entry_types() {
    assert_eq!(
        validate_replacement(EntryType::File, EntryType::Directory),
        Err(SafetyError::EntryTypeMismatch)
    );
    assert!(validate_replacement(EntryType::Directory, EntryType::Directory).is_ok());
}

#[tokio::test]
async fn scheduler_obeys_concurrency_and_publishes_full_event_sequence() {
    let bus = EventBus::new(64);
    let mut subscription = bus.subscribe_all_workspaces(SessionId::new("test"), None);
    let scheduler = Scheduler::new(1, bus);
    let executor = Arc::new(BlockingExecutor::default());
    let first = scheduler
        .submit(operation(), executor.clone())
        .expect("submission succeeds");
    let second = scheduler
        .submit(operation(), executor.clone())
        .expect("submission succeeds");

    executor.first_started.notified().await;
    tokio::task::yield_now().await;
    assert_eq!(scheduler.get(second).unwrap().state, OperationState::Queued);
    executor.release_first.notify_one();
    scheduler.wait(first).await.unwrap();
    scheduler.wait(second).await.unwrap();

    let mut names = Vec::new();
    while names
        .iter()
        .filter(|name| **name == "operation.completed")
        .count()
        < 2
    {
        let SubscriptionEvent::Event(event) = subscription.recv().await.unwrap() else {
            panic!("unexpected replay gap");
        };
        names.push(event.payload.event_name());
    }
    assert_eq!(names[0], "operation.created");
    assert!(names.contains(&"operation.progress"));
    assert_eq!(names.last(), Some(&"operation.completed"));
}

#[tokio::test]
async fn cancellation_at_safe_point_cleans_partial_destination() {
    let scheduler = Scheduler::new(1, EventBus::new(32));
    let executor = Arc::new(BlockingExecutor::default());
    let id = scheduler.submit(operation(), executor.clone()).unwrap();
    executor.first_started.notified().await;
    scheduler.cancel(id).unwrap();
    executor.release_first.notify_one();
    scheduler.wait(id).await.unwrap();

    assert_eq!(scheduler.get(id).unwrap().state, OperationState::Cancelled);
    assert!(
        executor
            .cleanup_called
            .load(std::sync::atomic::Ordering::SeqCst)
    );
}

#[derive(Default)]
struct CancellablePlanningExecutor {
    planning_started: tokio::sync::Notify,
    cleanup_called: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl OperationExecutor for CancellablePlanningExecutor {
    async fn plan(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        self.planning_started.notify_one();
        cancellation.cancelled().await;
        Err(fm_vfs::VfsError::Cancelled.into())
    }

    async fn execute(
        &self,
        _operation: &Operation,
        _item: &PlanItem,
        _resolution: Option<ConflictResolution>,
        _pause: &PauseToken,
        _cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        unreachable!("cancelled planning must not execute plan items")
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        self.cleanup_called
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn cancelling_during_planning_is_cancelled_not_failed() {
    let scheduler = Scheduler::new(1, EventBus::new(16));
    let executor = Arc::new(CancellablePlanningExecutor::default());
    let id = scheduler.submit(operation(), executor.clone()).unwrap();
    executor.planning_started.notified().await;

    scheduler.cancel(id).unwrap();
    assert_eq!(scheduler.get(id).unwrap().state, OperationState::Cancelling);
    scheduler.wait(id).await.unwrap();

    assert_eq!(scheduler.get(id).unwrap().state, OperationState::Cancelled);
    assert!(
        executor
            .cleanup_called
            .load(std::sync::atomic::Ordering::SeqCst)
    );
}

fn operation() -> Operation {
    Operation::new(
        OperationKind::Copy,
        vec![entry("file:///tmp/source")],
        Some(location("file:///tmp/destination")),
        ConflictPolicy::Ask,
    )
}

fn entry(uri: &str) -> EntryRef {
    EntryRef {
        id: EntryId::new(),
        location: location(uri),
    }
}

fn location(uri: &str) -> Location {
    Location::new(ProviderId::new("local"), uri)
}

fn sftp_location(connection_id: &str, remote_path: &str) -> Location {
    Location::new(
        ProviderId::new("sftp"),
        format!("sftp://{connection_id}{remote_path}"),
    )
}

/// Both `ftp://` and `ftps://` URIs carry the `ftp` provider id, so `scheme`
/// varies independently of the provider.
fn ftp_location(scheme: &str, connection_id: &str, remote_path: &str) -> Location {
    Location::new(
        ProviderId::new("ftp"),
        format!("{scheme}://{connection_id}{remote_path}"),
    )
}

fn webdav_location(connection_id: &str, remote_path: &str) -> Location {
    Location::new(
        ProviderId::new("webdav"),
        format!("webdav://{connection_id}{remote_path}"),
    )
}

#[derive(Default)]
struct BlockingExecutor {
    first_started: tokio::sync::Notify,
    release_first: tokio::sync::Notify,
    cleanup_called: std::sync::atomic::AtomicBool,
    executions: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl OperationExecutor for BlockingExecutor {
    async fn plan(
        &self,
        operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        Ok(OperationPlan::new(
            operation
                .sources
                .iter()
                .cloned()
                .map(|source| PlanItem::new(source, 100))
                .collect(),
        ))
    }

    async fn execute(
        &self,
        _operation: &Operation,
        _item: &PlanItem,
        _resolution: Option<fm_operations::ConflictResolution>,
        _pause: &PauseToken,
        _cancellation: &CancellationToken,
    ) -> Result<fm_operations::ExecutionOutcome, ExecutionError> {
        if self
            .executions
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            == 0
        {
            self.first_started.notify_one();
            self.release_first.notified().await;
        }
        Ok(fm_operations::ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        self.cleanup_called
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn pause_between_items_retains_totals_and_resume_continues_progress() {
    let scheduler = Scheduler::new(1, EventBus::new(32));
    let executor = Arc::new(BlockingExecutor::default());
    let operation = Operation::new(
        OperationKind::Copy,
        vec![entry("file:///first"), entry("file:///second")],
        Some(location("file:///destination")),
        ConflictPolicy::Ask,
    );
    let id = scheduler.submit(operation, executor.clone()).unwrap();
    executor.first_started.notified().await;

    scheduler.pause(id).unwrap();
    executor.release_first.notify_one();
    tokio::time::sleep(Duration::from_millis(20)).await;

    let paused = scheduler.get(id).unwrap();
    assert_eq!(paused.state, OperationState::Paused);
    assert_eq!(paused.progress.completed_items, 1);
    assert_eq!(paused.progress.total_items, Some(2));
    assert_eq!(paused.progress.completed_bytes, 100);
    assert_eq!(paused.progress.total_bytes, Some(200));

    scheduler.resume(id).unwrap();
    scheduler.wait(id).await.unwrap();
    let completed = scheduler.get(id).unwrap();
    assert_eq!(completed.state, OperationState::Completed);
    assert_eq!(completed.progress.completed_items, 2);
    assert_eq!(completed.progress.total_items, Some(2));
    assert_eq!(completed.progress.completed_bytes, 200);
    assert_eq!(completed.progress.total_bytes, Some(200));
}

struct ItemConflictExecutor {
    later_item_completed: std::sync::atomic::AtomicBool,
}

#[derive(Default)]
struct ChunkedExecutor {
    first_chunk_completed: tokio::sync::Notify,
    release_first_chunk: tokio::sync::Notify,
    chunks_completed: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl OperationExecutor for ChunkedExecutor {
    async fn plan(
        &self,
        operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        Ok(OperationPlan::new(vec![PlanItem::new(
            operation.sources[0].clone(),
            200,
        )]))
    }

    async fn execute(
        &self,
        _operation: &Operation,
        _item: &PlanItem,
        _resolution: Option<ConflictResolution>,
        pause: &PauseToken,
        _cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        self.chunks_completed
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.first_chunk_completed.notify_one();
        self.release_first_chunk.notified().await;
        pause.checkpoint().await;
        self.chunks_completed
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        Ok(())
    }
}

#[tokio::test]
async fn pause_suspends_an_active_file_at_its_next_chunk_boundary() {
    let scheduler = Scheduler::new(1, EventBus::new(16));
    let executor = Arc::new(ChunkedExecutor::default());
    let id = scheduler.submit(operation(), executor.clone()).unwrap();
    executor.first_chunk_completed.notified().await;

    scheduler.pause(id).unwrap();
    executor.release_first_chunk.notify_one();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        executor
            .chunks_completed
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );

    scheduler.resume(id).unwrap();
    scheduler.wait(id).await.unwrap();
    assert_eq!(
        executor
            .chunks_completed
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
}

#[async_trait]
impl OperationExecutor for ItemConflictExecutor {
    async fn plan(
        &self,
        operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        Ok(OperationPlan::new(
            operation
                .sources
                .iter()
                .cloned()
                .map(|entry| PlanItem::new(entry, 1))
                .collect(),
        ))
    }

    async fn execute(
        &self,
        operation: &Operation,
        item: &PlanItem,
        resolution: Option<ConflictResolution>,
        _pause: &PauseToken,
        _cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        if item.entry == operation.sources[0] && resolution.is_none() {
            return Err(ExecutionError::Conflict(OperationConflict {
                id: "conflict-1".into(),
                source: ConflictEntry {
                    name: "blocked.txt".into(),
                    kind: EntryKind::File,
                    size: Some(1),
                    modified_at: None,
                },
                destination: ConflictEntry {
                    name: "blocked.txt".into(),
                    kind: EntryKind::File,
                    size: Some(2),
                    modified_at: None,
                },
            }));
        }
        if item.entry == operation.sources[1] {
            self.later_item_completed
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        Ok(())
    }
}

#[tokio::test]
async fn a_conflict_blocks_only_its_item_and_uses_the_requested_resolution() {
    let executor = Arc::new(ItemConflictExecutor {
        later_item_completed: std::sync::atomic::AtomicBool::new(false),
    });
    let scheduler = Scheduler::new(1, EventBus::new(16));
    let operation = Operation::new(
        OperationKind::Copy,
        vec![entry("file:///blocked.txt"), entry("file:///safe.txt")],
        None,
        ConflictPolicy::Ask,
    );
    let id = scheduler.submit(operation, executor.clone()).unwrap();
    for _ in 0..200 {
        if scheduler.get(id).unwrap().state == OperationState::WaitingForConflictResolution
            && executor
                .later_item_completed
                .load(std::sync::atomic::Ordering::SeqCst)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        executor
            .later_item_completed
            .load(std::sync::atomic::Ordering::SeqCst)
    );
    scheduler
        .resolve_conflict(id, ConflictResolution::RenameNew, false)
        .unwrap();
    scheduler.wait(id).await.unwrap();
    assert_eq!(scheduler.get(id).unwrap().state, OperationState::Completed);
}
