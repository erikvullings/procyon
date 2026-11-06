//! Bounded retry with backoff for transfers (task 0301), on a deterministic clock.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use fm_domain::{EntryId, Location, ProviderId};
use fm_events::EventBus;
use fm_operations::retry::{JitterSource, RetryPolicy, Sleeper};
use fm_operations::{
    ConflictPolicy, ExecutionError, ExecutionOutcome, Operation, OperationExecutor, OperationKind,
    OperationPlan, OperationProgressReporter, OperationState, PauseToken, PlanItem, Scheduler,
};
use fm_vfs::{EntryRef, VfsError};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

fn entry(uri: &str) -> EntryRef {
    EntryRef {
        id: EntryId::new(),
        location: Location::new(ProviderId::new("sftp"), uri),
    }
}

/// Records every wait instead of sleeping; optionally blocks until cancelled.
#[derive(Default)]
struct RecordingSleeper {
    waits: Mutex<Vec<Duration>>,
    block_until_cancelled: bool,
    entered: Notify,
}

#[async_trait]
impl Sleeper for RecordingSleeper {
    async fn sleep(&self, duration: Duration, cancellation: &CancellationToken) -> bool {
        self.waits.lock().expect("lock").push(duration);
        if self.block_until_cancelled {
            self.entered.notify_one();
            cancellation.cancelled().await;
            return false;
        }
        !cancellation.is_cancelled()
    }
}

struct MidJitter;

impl JitterSource for MidJitter {
    fn unit(&self) -> f64 {
        0.5
    }
}

fn policy() -> RetryPolicy {
    RetryPolicy {
        max_attempts: 4,
        initial_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(10),
        multiplier: 2.0,
        jitter: 0.0,
    }
}

/// Fails with the scripted errors in order, then succeeds.
struct ScriptedExecutor {
    failures: Mutex<VecDeque<VfsError>>,
    retry_safe: bool,
    executions: AtomicUsize,
    cleanups: AtomicUsize,
}

impl ScriptedExecutor {
    fn new(failures: Vec<VfsError>, retry_safe: bool) -> Arc<Self> {
        Arc::new(Self {
            failures: Mutex::new(failures.into()),
            retry_safe,
            executions: AtomicUsize::new(0),
            cleanups: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl OperationExecutor for ScriptedExecutor {
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
        progress: &dyn OperationProgressReporter,
        _pause: &PauseToken,
        _cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        // Half the bytes move before every failure, so a retry must roll them back.
        progress.report_bytes(50);
        match self.failures.lock().expect("lock").pop_front() {
            Some(error) => Err(error.into()),
            None => Ok(ExecutionOutcome::Completed),
        }
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        self.cleanups.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn retry_safe(&self, _operation: &Operation) -> bool {
        self.retry_safe
    }
}

fn io(message: &str) -> VfsError {
    VfsError::Io {
        message: message.into(),
    }
}

fn copy() -> Operation {
    Operation::new(
        OperationKind::Copy,
        vec![entry("sftp://host/a.bin")],
        Some(Location::new(ProviderId::new("sftp"), "sftp://host/dest")),
        ConflictPolicy::Ask,
    )
}

fn scheduler(sleeper: Arc<RecordingSleeper>) -> Scheduler {
    Scheduler::new(1, EventBus::new(16)).with_retry_environment(
        policy(),
        sleeper,
        Arc::new(MidJitter),
    )
}

#[tokio::test]
async fn a_transient_failure_is_retried_with_growing_waits_and_then_succeeds() {
    let sleeper = Arc::new(RecordingSleeper::default());
    let scheduler = scheduler(sleeper.clone());
    let executor = ScriptedExecutor::new(vec![io("connection reset"), io("timed out")], true);
    let id = scheduler.submit(copy(), executor.clone()).unwrap();
    scheduler.wait(id).await.unwrap();

    let operation = scheduler.get(id).unwrap();
    assert_eq!(executor.executions.load(Ordering::SeqCst), 3);
    assert_eq!(
        *sleeper.waits.lock().expect("lock"),
        vec![Duration::from_millis(100), Duration::from_millis(200)]
    );
    assert_eq!(
        executor.cleanups.load(Ordering::SeqCst),
        2,
        "the private partial is discarded before every retry"
    );
    assert_eq!(operation.state, OperationState::CompletedWithWarnings);
    let retries: Vec<_> = operation
        .errors
        .iter()
        .filter(|error| error.code.as_deref() == Some("transferRetried"))
        .collect();
    assert_eq!(retries.len(), 2, "each retry is visible in the history");
    assert_eq!(
        retries[0].params.get("attempt").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        retries[1].params.get("attempt").map(String::as_str),
        Some("2")
    );
    assert!(
        retries[0]
            .params
            .get("cause")
            .is_some_and(|cause| cause.contains("connection reset"))
    );
    assert_eq!(
        operation.progress.completed_bytes, 100,
        "bytes from failed attempts are rolled back, not double counted"
    );
}

#[tokio::test]
async fn an_exhausted_budget_fails_with_the_last_cause_after_a_bounded_number_of_attempts() {
    let sleeper = Arc::new(RecordingSleeper::default());
    let scheduler = scheduler(sleeper.clone());
    let failures = (0..10).map(|_| io("connection reset")).collect();
    let executor = ScriptedExecutor::new(failures, true);
    let id = scheduler.submit(copy(), executor.clone()).unwrap();
    let _ = scheduler.wait(id).await;

    assert_eq!(
        executor.executions.load(Ordering::SeqCst),
        4,
        "max_attempts bounds the work"
    );
    assert_eq!(sleeper.waits.lock().expect("lock").len(), 3);
    let operation = scheduler.get(id).unwrap();
    assert_eq!(operation.state, OperationState::Failed);
}

#[tokio::test]
async fn decisions_and_security_failures_are_never_retried() {
    let permanent = vec![
        VfsError::AuthenticationFailed {
            message: "bad password".into(),
        },
        VfsError::PermissionDenied {
            location: "sftp://host/a.bin".into(),
        },
        VfsError::AlreadyExists {
            location: "sftp://host/dest/a.bin".into(),
        },
        VfsError::CredentialRequired,
        VfsError::InsufficientSpace {
            available: Some(1),
            required: Some(2),
        },
        io("host key does not match the previously accepted key"),
        io("authentication failed"),
        io("input/output error"),
        io("read-only file system"),
    ];
    for error in permanent {
        let sleeper = Arc::new(RecordingSleeper::default());
        let scheduler = scheduler(sleeper.clone());
        let label = error.to_string();
        let executor = ScriptedExecutor::new(vec![error], true);
        let id = scheduler.submit(copy(), executor.clone()).unwrap();
        let _ = scheduler.wait(id).await;
        assert_eq!(
            executor.executions.load(Ordering::SeqCst),
            1,
            "retried: {label}"
        );
        assert!(
            sleeper.waits.lock().expect("lock").is_empty(),
            "waited for: {label}"
        );
        assert_eq!(
            scheduler.get(id).unwrap().state,
            OperationState::Failed,
            "{label}"
        );
    }
}

#[tokio::test]
async fn an_executor_that_is_not_retry_safe_is_never_re_run() {
    let sleeper = Arc::new(RecordingSleeper::default());
    let scheduler = scheduler(sleeper.clone());
    let executor = ScriptedExecutor::new(vec![io("connection reset")], false);
    let id = scheduler.submit(copy(), executor.clone()).unwrap();
    let _ = scheduler.wait(id).await;
    assert_eq!(executor.executions.load(Ordering::SeqCst), 1);
    assert!(sleeper.waits.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn cancelling_during_a_backoff_wait_stops_the_job_without_another_attempt() {
    let sleeper = Arc::new(RecordingSleeper {
        block_until_cancelled: true,
        ..RecordingSleeper::default()
    });
    let scheduler = scheduler(sleeper.clone());
    let executor = ScriptedExecutor::new(vec![io("connection reset")], true);
    let id = scheduler.submit(copy(), executor.clone()).unwrap();
    sleeper.entered.notified().await;
    scheduler.cancel(id).unwrap();
    let _ = scheduler.wait(id).await;

    assert_eq!(
        executor.executions.load(Ordering::SeqCst),
        1,
        "no attempt after the cancel"
    );
    assert_eq!(scheduler.get(id).unwrap().state, OperationState::Cancelled);
}

#[tokio::test]
async fn a_disabled_policy_behaves_exactly_as_before() {
    let sleeper = Arc::new(RecordingSleeper::default());
    let scheduler = Scheduler::new(1, EventBus::new(16)).with_retry_environment(
        RetryPolicy::disabled(),
        sleeper.clone(),
        Arc::new(MidJitter),
    );
    let executor = ScriptedExecutor::new(vec![io("connection reset")], true);
    let id = scheduler.submit(copy(), executor.clone()).unwrap();
    let _ = scheduler.wait(id).await;
    assert_eq!(executor.executions.load(Ordering::SeqCst), 1);
    assert!(sleeper.waits.lock().expect("lock").is_empty());
}
