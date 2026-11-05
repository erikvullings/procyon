//! Scheduler integration for the transfer journal (task 0299).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fm_domain::{EntryId, Location, OperationId, ProviderId};
use fm_events::EventBus;
use fm_operations::journal::{
    FileOperationJournal, JournalError, JournalEvent, JournalIntent, OperationJournal,
    SourceRevision,
};
use fm_operations::{
    ConflictPolicy, ExecutionError, ExecutionOutcome, Operation, OperationExecutor, OperationKind,
    OperationPlan, OperationProgressReporter, OperationState, PauseToken, PlanItem, Scheduler,
};
use fm_vfs::EntryRef;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

fn location(uri: &str) -> Location {
    Location::new(ProviderId::new("local"), uri)
}

fn entry(uri: &str) -> EntryRef {
    EntryRef {
        id: EntryId::new(),
        location: location(uri),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    Begin(OperationKind),
    Event(JournalEvent),
    Finish,
}

/// In-memory journal that remembers every call in order.
#[derive(Default)]
struct RecordingJournal {
    seen: Mutex<Vec<Seen>>,
    fail_begin: bool,
}

impl RecordingJournal {
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("seen").clone()
    }
}

#[async_trait]
impl OperationJournal for RecordingJournal {
    async fn begin(&self, intent: JournalIntent) -> Result<(), JournalError> {
        if self.fail_begin {
            return Err(JournalError::Io("disk full".into()));
        }
        self.seen
            .lock()
            .expect("seen")
            .push(Seen::Begin(intent.kind));
        Ok(())
    }

    fn record(&self, _operation: OperationId, event: JournalEvent) {
        self.seen.lock().expect("seen").push(Seen::Event(event));
    }

    async fn finish(&self, _operation: OperationId) {
        self.seen.lock().expect("seen").push(Seen::Finish);
    }
}

/// Transfers each source like a copy would: announces a revision and a private
/// partial, then completes. Optionally fails or blocks on the second item.
#[derive(Default)]
struct TransferExecutor {
    fail_second: bool,
    block_second: bool,
    second_started: Notify,
    release_second: Notify,
    executed: AtomicUsize,
}

#[async_trait]
impl OperationExecutor for TransferExecutor {
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
        item: &PlanItem,
        _resolution: Option<fm_operations::ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        _pause: &PauseToken,
        _cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let index = self.executed.fetch_add(1, Ordering::SeqCst);
        progress.note_source_revision(
            &item.entry.location,
            SourceRevision {
                size: Some(item.bytes),
                modified_at_ms: Some(7),
                validator: None,
            },
        );
        progress.note_partial(
            &item.entry.location,
            &location(&format!("file:///dest/.fm-copy-{index}")),
        );
        if index == 1 {
            if self.block_second {
                self.second_started.notify_one();
                self.release_second.notified().await;
            }
            if self.fail_second {
                return Err(ExecutionError::Failed("connection reset".into()));
            }
        }
        Ok(ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        Ok(())
    }
}

fn copy_operation() -> Operation {
    Operation::new(
        OperationKind::Copy,
        vec![entry("file:///src/a"), entry("file:///src/b")],
        Some(location("file:///dest")),
        ConflictPolicy::Ask,
    )
}

/// Waits for the journal's `finish`, which runs just after a job is terminal.
async fn finished(journal: &RecordingJournal) {
    for _ in 0..200 {
        if journal.seen().last() == Some(&Seen::Finish) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

fn committed(seen: &[Seen]) -> Vec<String> {
    seen.iter()
        .filter_map(|item| match item {
            Seen::Event(JournalEvent::Committed { source, .. }) => Some(source.uri.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_completed_copy_is_journaled_from_intent_to_removal_in_order() {
    let journal = Arc::new(RecordingJournal::default());
    let scheduler = Scheduler::new(1, EventBus::new(16)).with_journal(journal.clone());
    let id = scheduler
        .submit(copy_operation(), Arc::new(TransferExecutor::default()))
        .unwrap();
    scheduler.wait(id).await.unwrap();
    finished(&journal).await;

    let seen = journal.seen();
    assert_eq!(seen.first(), Some(&Seen::Begin(OperationKind::Copy)));
    assert_eq!(seen.last(), Some(&Seen::Finish));
    assert!(matches!(
        seen.get(1),
        Some(Seen::Event(JournalEvent::Items { items })) if items.len() == 2
    ));
    let partials = seen
        .iter()
        .filter(|item| matches!(item, Seen::Event(JournalEvent::Partial { .. })))
        .count();
    let revisions = seen
        .iter()
        .filter(|item| matches!(item, Seen::Event(JournalEvent::SourceRevision { .. })))
        .count();
    assert_eq!((partials, revisions), (2, 2));
    assert_eq!(committed(&seen), vec!["file:///src/a", "file:///src/b"]);
    // A partial is announced before its item commits.
    let first_partial = seen
        .iter()
        .position(|item| matches!(item, Seen::Event(JournalEvent::Partial { .. })))
        .unwrap();
    let first_commit = seen
        .iter()
        .position(|item| matches!(item, Seen::Event(JournalEvent::Committed { .. })))
        .unwrap();
    assert!(first_partial < first_commit);
}

#[tokio::test]
async fn a_running_job_has_committed_only_what_finished_and_is_not_yet_finished() {
    let journal = Arc::new(RecordingJournal::default());
    let scheduler = Scheduler::new(1, EventBus::new(16)).with_journal(journal.clone());
    let executor = Arc::new(TransferExecutor {
        block_second: true,
        ..TransferExecutor::default()
    });
    let id = scheduler
        .submit(copy_operation(), executor.clone())
        .unwrap();
    executor.second_started.notified().await;

    // The state a crash right now would leave behind.
    let seen = journal.seen();
    assert_eq!(committed(&seen), vec!["file:///src/a"]);
    assert!(!seen.contains(&Seen::Finish));

    executor.release_second.notify_one();
    scheduler.wait(id).await.unwrap();
}

#[tokio::test]
async fn a_failed_copy_still_clears_its_journal_record_and_keeps_its_commits() {
    let journal = Arc::new(RecordingJournal::default());
    let scheduler = Scheduler::new(1, EventBus::new(16)).with_journal(journal.clone());
    let executor = Arc::new(TransferExecutor {
        fail_second: true,
        ..TransferExecutor::default()
    });
    let id = scheduler.submit(copy_operation(), executor).unwrap();
    let _ = scheduler.wait(id).await;
    finished(&journal).await;

    assert_eq!(scheduler.get(id).unwrap().state, OperationState::Failed);
    let seen = journal.seen();
    assert_eq!(committed(&seen), vec!["file:///src/a"]);
    assert_eq!(seen.last(), Some(&Seen::Finish));
}

#[tokio::test]
async fn non_transfer_operations_are_not_journaled() {
    let journal = Arc::new(RecordingJournal::default());
    let scheduler = Scheduler::new(1, EventBus::new(16)).with_journal(journal.clone());
    let delete = Operation::new(
        OperationKind::Delete,
        vec![entry("file:///src/a"), entry("file:///src/b")],
        None,
        ConflictPolicy::Ask,
    );
    let id = scheduler
        .submit(delete, Arc::new(TransferExecutor::default()))
        .unwrap();
    scheduler.wait(id).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(journal.seen().is_empty());
}

#[tokio::test]
async fn a_journal_that_cannot_begin_never_fails_the_transfer() {
    let journal = Arc::new(RecordingJournal {
        fail_begin: true,
        ..RecordingJournal::default()
    });
    let scheduler = Scheduler::new(1, EventBus::new(16)).with_journal(journal);
    let id = scheduler
        .submit(copy_operation(), Arc::new(TransferExecutor::default()))
        .unwrap();
    scheduler.wait(id).await.unwrap();
    assert_eq!(scheduler.get(id).unwrap().state, OperationState::Completed);
}

#[tokio::test]
async fn a_file_journal_survives_an_interrupted_job_and_empties_after_a_finished_one() {
    let directory = tempfile::tempdir().expect("dir");
    let (journal, _) = FileOperationJournal::open(directory.path()).expect("open");
    let journal = Arc::new(journal);
    let scheduler = Scheduler::new(1, EventBus::new(16)).with_journal(journal.clone());

    // A finished job removes its record.
    let done = scheduler
        .submit(copy_operation(), Arc::new(TransferExecutor::default()))
        .unwrap();
    scheduler.wait(done).await.unwrap();
    for _ in 0..200 {
        journal.flush().await;
        if journal_files(directory.path()) == 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(journal_files(directory.path()), 0);

    // A job still running when the process dies leaves its record behind.
    let executor = Arc::new(TransferExecutor {
        block_second: true,
        ..TransferExecutor::default()
    });
    let running = scheduler
        .submit(copy_operation(), executor.clone())
        .unwrap();
    executor.second_started.notified().await;
    journal.flush().await;
    assert_eq!(journal_files(directory.path()), 1);

    // "Crash": nothing finishes the running job; reopen the directory.
    let snapshot_dir = tempfile::tempdir().expect("copy");
    for file in std::fs::read_dir(directory.path()).unwrap().flatten() {
        if file.path().is_file() {
            std::fs::copy(file.path(), snapshot_dir.path().join(file.file_name())).unwrap();
        }
    }
    let (_reopened, recovery) = FileOperationJournal::open(snapshot_dir.path()).expect("reopen");
    assert_eq!(recovery.jobs.len(), 1);
    let job = &recovery.jobs[0];
    assert_eq!(job.intent.operation_id, running);
    assert_eq!(job.intent.kind, OperationKind::Copy);
    assert_eq!(job.items.len(), 2);
    assert!(job.items[0].committed, "the finished item is committed");
    assert!(!job.items[1].committed, "the interrupted item is not");
    assert!(
        job.items[1].partial.is_some(),
        "the interrupted item's private partial is owned by the job"
    );
    assert_eq!(
        job.items[1].revision.as_ref().and_then(|r| r.size),
        Some(100)
    );

    executor.release_second.notify_one();
    scheduler.wait(running).await.unwrap();
}

fn journal_files(directory: &std::path::Path) -> usize {
    std::fs::read_dir(directory)
        .expect("list journal directory")
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "journal"))
        .count()
}
