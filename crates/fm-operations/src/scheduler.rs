use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fm_domain::{Location, OperationId};
use fm_events::{
    BackendEventPayload, ConflictPolicyPayload, EntryKindPayload, EntryRefPayload, EventAudience,
    EventBus, LocationPayload, OperationConflictEntryPayload, OperationConflictPayload,
    OperationKindPayload, OperationPayload, OperationProgressDetails, OperationProgressPayload,
    OperationStatePayload, OperationUndoPayload,
};
use fm_vfs::EntryRef;
use thiserror::Error;
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::journal::{JournalEvent, JournalIntent, JournalItem, SharedJournal, SourceRevision};
use crate::retry::{JitterSource, RandomJitter, RetryPolicy, Sleeper, TokioSleeper};
use crate::{
    ConflictResolution, Operation, OperationConflict, OperationProgress, OperationState,
    OperationUndo, PauseToken, ProgressPublisher, TransitionError,
};

/// Receives durable snapshots whenever an operation changes.
pub trait OperationSnapshotObserver: Send + Sync + 'static {
    /// Records the latest snapshot. Implementations must not panic.
    fn observe(&self, operation: &Operation);
}

/// One persistable unit in an operation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanItem {
    /// Entry to process.
    pub entry: EntryRef,
    /// Payload size contributing to totals.
    pub bytes: u64,
}

impl PlanItem {
    /// Creates one plan item.
    #[must_use]
    pub const fn new(entry: EntryRef, bytes: u64) -> Self {
        Self { entry, bytes }
    }
}

/// Materialized planning result suitable for later persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationPlan {
    /// Ordered execution units.
    pub items: Vec<PlanItem>,
    /// Known number of units.
    pub total_items: Option<u64>,
    /// Known sum of payload bytes.
    pub total_bytes: Option<u64>,
}

impl OperationPlan {
    /// Builds a fully enumerated plan and computes totals before execution.
    #[must_use]
    pub fn new(items: Vec<PlanItem>) -> Self {
        let total_items = u64::try_from(items.len()).ok();
        let total_bytes = items
            .iter()
            .try_fold(0_u64, |total, item| total.checked_add(item.bytes));
        Self {
            items,
            total_items,
            total_bytes,
        }
    }
}

/// Failure from a concrete operation implementation.
#[derive(Debug, Error)]
pub enum ExecutionError {
    /// This operation kind has not landed yet.
    #[error("operation kind is not implemented")]
    NotImplemented,
    /// Uncoded internal failure; clients show a generic localised line.
    #[error("operation execution failed: {0}")]
    Failed(String),
    /// Failure with a stable code and parameters clients localise.
    #[error("{}", .0.message)]
    Coded(Box<CodedFailure>),
    /// Provider-neutral filesystem failure.
    #[error(transparent)]
    Provider(#[from] fm_vfs::VfsError),
    /// An entry failed but independent plan items may continue.
    #[error("{}", error.message)]
    Warning {
        /// Failure reported for the entry.
        error: Box<crate::OperationEntryError>,
    },
    /// This item needs an explicit user decision before it can be retried.
    #[error("operation item has a destination conflict")]
    Conflict(OperationConflict),
}

/// An operation-level failure with a stable [`crate::codes::failure`] code, structured
/// parameters and its English message as fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodedFailure {
    /// Stable machine-readable code.
    pub code: String,
    /// English fallback text.
    pub message: String,
    /// Values the localised text interpolates.
    pub params: std::collections::BTreeMap<String, String>,
}

impl CodedFailure {
    /// A failure with no parameters.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            params: std::collections::BTreeMap::new(),
        }
    }

    /// Adds one structured interpolation value.
    #[must_use]
    pub fn with_param(mut self, key: &str, value: impl ToString) -> Self {
        self.params.insert(key.to_owned(), value.to_string());
        self
    }
}

impl From<CodedFailure> for ExecutionError {
    fn from(failure: CodedFailure) -> Self {
        Self::Coded(Box::new(failure))
    }
}

impl ExecutionError {
    /// Whether the same item may succeed if tried again later (task 0301). Only provider I/O
    /// failures qualify; conflicts, authentication, permission and cancellation never do.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Provider(error) if error.is_transient())
    }

    /// A per-entry failure the engine records and continues past.
    #[must_use]
    pub fn warning(error: crate::OperationEntryError) -> Self {
        Self::Warning {
            error: Box::new(error),
        }
    }
}

/// Result of executing one plan item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionOutcome {
    /// The item was applied and contributes its planned bytes.
    Completed,
    /// The item completed as far as possible, with independent entry failures.
    CompletedWithWarnings(Vec<crate::OperationEntryError>),
    /// A provider completed a tree in one pass and reported its own item count.
    BulkCompleted(Vec<crate::OperationEntryError>),
    /// The item was deliberately skipped and contributes no bytes.
    Skipped,
}

/// Receives bytes as an executor processes the current plan item.
pub trait OperationProgressReporter: Send + Sync {
    /// Adds newly processed bytes to the operation's progress.
    fn report_bytes(&self, additional_bytes: u64);
    /// Reports a batch of completed entries without publishing one event per entry.
    fn report_items(&self, _additional_items: u64) {}
    /// Announces the private partial destination now holding `source`'s bytes, and
    /// where the finished copy will be published, so a crash leaves a journal entry
    /// that owns the partial (tasks 0299, 0300).
    fn note_partial(&self, _source: &Location, _partial: &Location, _destination: &Location) {}
    /// Records what `source` looked like before it was transferred, so a later
    /// resume can prove the source did not change (task 0299).
    fn note_source_revision(&self, _source: &Location, _revision: SourceRevision) {}
    /// Reports what verifying one copied file concluded (task 0302).
    fn note_verification(&self, _outcome: crate::VerificationOutcome) {}
    /// Reports that a copy continued from, or gave up on, an interrupted partial (task 0303).
    fn note_resume(&self, _outcome: crate::ResumeOutcome) {}
    /// Whether the notes above are being kept. Executors check this before
    /// doing extra work (such as a source `stat`) that only the journal needs.
    fn journaling(&self) -> bool {
        false
    }
}

impl<F> OperationProgressReporter for F
where
    F: Fn(u64) + Send + Sync,
{
    fn report_bytes(&self, additional_bytes: u64) {
        self(additional_bytes);
    }
}

/// Planning/execution boundary implemented by future operation-kind tasks.
#[async_trait]
pub trait OperationExecutor: Send + Sync + 'static {
    /// Enumerates work and computes totals without mutating destinations.
    async fn plan(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError>;

    /// Executes one plan item. It must only expose a final destination atomically.
    async fn execute(
        &self,
        operation: &Operation,
        item: &PlanItem,
        resolution: Option<ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError>;

    /// Discovers children after a directory has been created, avoiding an up-front tree count.
    async fn expand(
        &self,
        _operation: &Operation,
        _item: &PlanItem,
        _cancellation: &CancellationToken,
    ) -> Result<Vec<PlanItem>, ExecutionError> {
        Ok(Vec::new())
    }

    /// Removes any private/temporary destination after cancellation or failure.
    async fn cleanup_partial(&self, operation: &Operation) -> Result<(), ExecutionError>;

    /// Applies post-order finalization after all plan items have succeeded.
    async fn finish(
        &self,
        _operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        Ok(())
    }

    /// Produces the explicit inverse plan after all original mutations have completed.
    async fn undo_evidence(
        &self,
        _operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        Ok(OperationUndo::unavailable(
            crate::codes::undo::NO_EVIDENCE,
            "This operation does not retain enough evidence to be undone safely.",
        ))
    }

    /// Whether execution must wait for explicit user confirmation after planning.
    fn requires_confirmation(&self) -> bool {
        false
    }

    /// Whether a failed plan item may be run again automatically after a transient failure
    /// (task 0301). Only when re-running is idempotent: the item's partial output is private
    /// and `cleanup_partial` removes it, and nothing was published or deleted yet. The default
    /// is `false`, so an executor opts in explicitly.
    fn retry_safe(&self, _operation: &Operation) -> bool {
        false
    }
}

/// A journaled job's handle: resolved once per job so the per-item hot path
/// never clones the operation snapshot just to learn its id.
#[derive(Clone)]
struct JobJournal {
    journal: SharedJournal,
    operation: OperationId,
}

impl JobJournal {
    /// Records newly known work units.
    fn items<'a>(&self, items: impl Iterator<Item = &'a PlanItem>) {
        let items: Vec<JournalItem> = items
            .map(|item| JournalItem {
                source: item.entry.location.clone(),
                bytes: item.bytes,
            })
            .collect();
        if !items.is_empty() {
            self.journal
                .record(self.operation, JournalEvent::Items { items });
        }
    }

    /// Records that `item` finished and its destination is published.
    fn committed(&self, item: &PlanItem) {
        self.journal.record(
            self.operation,
            JournalEvent::Committed {
                source: item.entry.location.clone(),
                bytes: item.bytes,
            },
        );
    }
}

struct ItemProgressReporter<'a> {
    scheduler: &'a Scheduler,
    job: &'a Job,
    publisher: &'a Mutex<ProgressPublisher>,
    current_entry: EntryRef,
    reported_bytes: AtomicU64,
    journal: Option<&'a JobJournal>,
}

impl ItemProgressReporter<'_> {
    fn finish_completed(&self, planned_bytes: u64) {
        let remaining = planned_bytes.saturating_sub(self.reported_bytes.load(Ordering::Acquire));
        self.report_bytes(remaining);
    }

    fn rollback(&self) {
        let reported_bytes = self.reported_bytes.swap(0, Ordering::AcqRel);
        if reported_bytes == 0 {
            return;
        }
        self.publisher
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .rollback(reported_bytes);
        let mut state = self.job.lock();
        state.operation.progress.completed_bytes = state
            .operation
            .progress
            .completed_bytes
            .saturating_sub(reported_bytes);
        state.operation.progress.bytes_per_second = None;
        self.scheduler.publish_progress(&state.operation);
    }
}

impl OperationProgressReporter for ItemProgressReporter<'_> {
    fn journaling(&self) -> bool {
        self.journal.is_some()
    }

    fn note_verification(&self, outcome: crate::VerificationOutcome) {
        let mut state = self.job.lock();
        state.operation.verification.record(outcome);
    }

    fn note_resume(&self, outcome: crate::ResumeOutcome) {
        use crate::codes::entry_error as code;
        let name = self.current_entry.location.name().unwrap_or_default();
        let (code, message) = match outcome {
            crate::ResumeOutcome::Resumed { bytes } => {
                // A clean resume is good news, not a warning: it must not turn a successful
                // copy into "completed with warnings".
                tracing::info!(%name, bytes, "copy resumed from its interrupted partial");
                return;
            }
            crate::ResumeOutcome::FellBack(reason) => {
                let (code, why) = match reason {
                    crate::ResumeFallback::PartialGone => (
                        code::RESUME_FALLBACK_PARTIAL_GONE,
                        "the unfinished copy was gone",
                    ),
                    crate::ResumeFallback::SourceChanged => {
                        (code::RESUME_FALLBACK_SOURCE_CHANGED, "the source changed")
                    }
                    crate::ResumeFallback::PartialMismatch => (
                        code::RESUME_FALLBACK_PARTIAL_MISMATCH,
                        "the unfinished copy did not match the source",
                    ),
                    crate::ResumeFallback::Unverifiable => (
                        code::RESUME_FALLBACK_UNVERIFIABLE,
                        "the unfinished copy could not be checked",
                    ),
                };
                (
                    code,
                    format!("{name} was copied again from the start because {why}."),
                )
            }
        };
        let error = crate::OperationEntryError::new(self.current_entry.clone(), message)
            .with_code(code)
            .with_param("name", name);
        let mut state = self.job.lock();
        state.operation.errors.push(error);
        self.scheduler.publish_progress(&state.operation);
    }

    fn note_partial(&self, source: &Location, partial: &Location, destination: &Location) {
        if let Some(journal) = self.journal {
            journal.journal.record(
                journal.operation,
                JournalEvent::Partial {
                    source: source.clone(),
                    partial: partial.clone(),
                    destination: Some(destination.clone()),
                },
            );
        }
    }

    fn note_source_revision(&self, source: &Location, revision: SourceRevision) {
        if let Some(journal) = self.journal {
            journal.journal.record(
                journal.operation,
                JournalEvent::SourceRevision {
                    source: source.clone(),
                    revision,
                },
            );
        }
    }

    fn report_items(&self, additional_items: u64) {
        let mut state = self.job.lock();
        state.operation.progress.completed_items = state
            .operation
            .progress
            .completed_items
            .saturating_add(additional_items);
        self.scheduler.publish_progress(&state.operation);
    }

    fn report_bytes(&self, additional_bytes: u64) {
        if additional_bytes == 0 {
            return;
        }
        self.reported_bytes
            .fetch_add(additional_bytes, Ordering::AcqRel);
        let published = self
            .publisher
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .record(Instant::now(), additional_bytes);
        let mut state = self.job.lock();
        state.operation.progress.completed_bytes = state
            .operation
            .progress
            .completed_bytes
            .saturating_add(additional_bytes);
        state.operation.progress.current_entry = Some(self.current_entry.clone());
        if let Some(progress) = published {
            state.operation.progress.bytes_per_second = progress.bytes_per_second;
            self.scheduler.publish_progress(&state.operation);
        }
    }
}

/// Scheduler lookup, transition, or execution failure.
#[derive(Debug, Error)]
pub enum SchedulerError {
    /// No operation has this identifier.
    #[error("unknown operation {0}")]
    UnknownOperation(OperationId),
    /// Lifecycle transition was illegal.
    #[error(transparent)]
    Transition(#[from] TransitionError),
    /// The operation implementation failed.
    #[error(transparent)]
    Execution(Box<ExecutionError>),
}

impl From<ExecutionError> for SchedulerError {
    fn from(error: ExecutionError) -> Self {
        Self::Execution(Box::new(error))
    }
}

impl ExecutionError {
    fn code(&self) -> &str {
        match self {
            Self::Provider(error) => error.code(),
            Self::Coded(failure) => &failure.code,
            _ => crate::codes::failure::OPERATION_FAILED,
        }
    }

    /// Code-specific facts a client needs to react, such as the shortfall of an
    /// `insufficientSpace` failure (task 0272).
    fn details(&self) -> Option<serde_json::Value> {
        match self {
            Self::Provider(fm_vfs::VfsError::InsufficientSpace {
                available,
                required,
            }) => Some(serde_json::json!({
                "required": required,
                "available": available,
            })),
            Self::Provider(
                fm_vfs::VfsError::Io { message }
                | fm_vfs::VfsError::AuthenticationFailed { message },
            ) => Some(serde_json::json!({ "detail": message })),
            Self::Coded(failure) if !failure.params.is_empty() => {
                Some(serde_json::json!(failure.params))
            }
            Self::Failed(detail) => Some(serde_json::json!({ "detail": detail })),
            _ => None,
        }
    }
}

impl SchedulerError {
    fn code(&self) -> &str {
        match self {
            Self::Execution(error) => error.code(),
            _ => crate::codes::failure::OPERATION_FAILED,
        }
    }

    fn details(&self) -> Option<serde_json::Value> {
        match self {
            Self::Execution(error) => error.details(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod error_code_tests {
    use super::{ExecutionError, SchedulerError};

    #[test]
    fn provider_failure_codes_survive_scheduler_wrapping() {
        let error = SchedulerError::from(ExecutionError::Provider(
            fm_vfs::VfsError::PermissionDenied {
                location: "file:///read-only".to_owned(),
            },
        ));

        assert_eq!(error.code(), "permissionDenied");
    }

    #[test]
    fn insufficient_space_has_a_stable_operation_failure_code() {
        let error = SchedulerError::from(ExecutionError::Provider(
            fm_vfs::VfsError::InsufficientSpace {
                available: Some(100),
                required: Some(200),
            },
        ));

        assert_eq!(error.code(), "insufficientSpace");
    }

    #[test]
    fn insufficient_space_failures_carry_the_required_and_available_bytes() {
        let error = SchedulerError::from(ExecutionError::Provider(
            fm_vfs::VfsError::InsufficientSpace {
                available: Some(100),
                required: Some(200),
            },
        ));

        assert_eq!(
            error.details(),
            Some(serde_json::json!({ "required": 200, "available": 100 }))
        );
        assert_eq!(
            SchedulerError::from(ExecutionError::Failed("x".into())).details(),
            Some(serde_json::json!({ "detail": "x" }))
        );
    }
}

#[derive(Debug, Clone)]
struct PendingConflict {
    conflict: OperationConflict,
    decision: Option<ConflictResolution>,
}

/// `Job`'s mutable interruption-relevant state, all behind one lock so a
/// caller reads a consistent view of "current lifecycle state + pending
/// conflict + sticky conflict resolution" instead of racing three locks.
struct JobState {
    operation: Operation,
    pending_conflict: Option<PendingConflict>,
    apply_to_all: Option<ConflictResolution>,
}

impl JobState {
    const fn new(operation: Operation) -> Self {
        Self {
            operation,
            pending_conflict: None,
            apply_to_all: None,
        }
    }

    /// True while `run_job`'s loop must wait before starting the next unit of
    /// work: cooperatively paused, or waiting on the initial confirmation
    /// gate. A registered per-item conflict does NOT block here — the main
    /// loop defers that item and keeps processing the rest of the plan;
    /// `wait_for_conflict_decision` (called only from the deferred-items
    /// pass) is what actually waits for that conflict's resolution. Blocking
    /// here too would deadlock: nothing would ever reach the code that
    /// resolves it.
    fn blocking(&self) -> bool {
        match self.operation.state {
            OperationState::Paused => true,
            OperationState::WaitingForConflictResolution => self.pending_conflict.is_none(),
            _ => false,
        }
    }
}

/// Whether `run_job` may proceed to the next unit of work right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobSignal {
    Proceed,
    Blocked,
}

struct Job {
    state: Mutex<JobState>,
    cancellation: CancellationToken,
    completed: Notify,
    resumed: Notify,
    pause: PauseToken,
}

impl Job {
    fn new(operation: Operation) -> Self {
        Self {
            state: Mutex::new(JobState::new(operation)),
            cancellation: CancellationToken::new(),
            completed: Notify::new(),
            resumed: Notify::new(),
            pause: PauseToken::default(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, JobState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn snapshot(&self) -> Operation {
        self.lock().operation.clone()
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    /// Cancellation always overrides an in-flight pause or conflict wait: a
    /// `Proceed` signal does not by itself mean "keep running" — callers must
    /// still check `is_cancelled` separately to tell "free to run" apart from
    /// "cancelled, go clean up."
    fn signal(&self) -> JobSignal {
        if self.is_cancelled() || !self.lock().blocking() {
            JobSignal::Proceed
        } else {
            JobSignal::Blocked
        }
    }
}

/// Runs operation jobs with bounded concurrency and publishes their lifecycle.
#[derive(Clone)]
pub struct Scheduler {
    jobs: Arc<Mutex<HashMap<OperationId, Arc<Job>>>>,
    permits: Arc<Semaphore>,
    events: EventBus,
    observer: Option<Arc<dyn OperationSnapshotObserver>>,
    journal: Option<SharedJournal>,
    retry: RetryPolicy,
    sleeper: Arc<dyn Sleeper>,
    jitter: Arc<dyn JitterSource>,
}

impl Scheduler {
    /// Creates a scheduler from the settings `operation_concurrency` value.
    #[must_use]
    pub fn new(operation_concurrency: u16, events: EventBus) -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            permits: Arc::new(Semaphore::new(usize::from(operation_concurrency.max(1)))),
            events,
            observer: None,
            journal: None,
            retry: RetryPolicy::default(),
            sleeper: Arc::new(TokioSleeper),
            jitter: Arc::new(RandomJitter),
        }
    }

    /// Sets how transient transfer failures are retried (task 0301).
    #[must_use]
    pub fn with_retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = policy;
        self
    }

    /// Replaces the clock and randomness behind retries; for deterministic tests.
    #[must_use]
    pub fn with_retry_environment(
        mut self,
        policy: RetryPolicy,
        sleeper: Arc<dyn Sleeper>,
        jitter: Arc<dyn JitterSource>,
    ) -> Self {
        self.retry = policy;
        self.sleeper = sleeper;
        self.jitter = jitter;
        self
    }

    /// Adds a synchronous snapshot observer used by durable history stores.
    #[must_use]
    pub fn with_observer(mut self, observer: Arc<dyn OperationSnapshotObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Runs one plan item, retrying transient failures with bounded exponential backoff.
    ///
    /// Each retry rolls back the bytes the failed attempt reported, discards the item's private
    /// partial output, waits (cancellably) and records the attempt as a warning on the operation
    /// so it shows in progress and history. A non-transient failure, an executor that is not
    /// retry-safe, an exhausted budget and a cancellation all return the error unchanged.
    async fn execute_with_retry(
        &self,
        job: &Job,
        executor: &dyn OperationExecutor,
        item: &PlanItem,
        resolution: Option<ConflictResolution>,
        reporter: &ItemProgressReporter<'_>,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let mut attempt = 1_u32;
        loop {
            let snapshot = job.snapshot();
            let result = executor
                .execute(
                    &snapshot,
                    item,
                    resolution,
                    reporter,
                    &job.pause,
                    &job.cancellation,
                )
                .await;
            let Err(error) = &result else {
                return result;
            };
            if !error.is_transient()
                || !executor.retry_safe(&snapshot)
                || !self.retry.allows_retry_after(attempt)
                || job.is_cancelled()
            {
                return result;
            }
            reporter.rollback();
            if let Err(cleanup) = executor.cleanup_partial(&snapshot).await {
                // A partial that cannot be removed must not be built on; surface the failure.
                tracing::warn!(%cleanup, "retry abandoned: the failed attempt's partial could not be removed");
                return result;
            }
            let delay = self.retry.delay_for(attempt, self.jitter.unit());
            let cause = error.to_string();
            // Rounded up so a 500 ms wait reads "1 s", never "0 s".
            let delay_seconds = delay.as_secs_f64().ceil();
            tracing::warn!(attempt, max = self.retry.max_attempts, %cause, "transient transfer failure; retrying");
            {
                let mut state = job.lock();
                state.operation.errors.push(
                    crate::OperationEntryError::new(
                        item.entry.clone(),
                        format!(
                            "Temporary failure (attempt {attempt} of {}), retrying in {delay_seconds} s: {cause}",
                            self.retry.max_attempts
                        ),
                    )
                    .with_code(crate::codes::entry_error::TRANSFER_RETRIED)
                    .with_param("attempt", attempt)
                    .with_param("max", self.retry.max_attempts)
                    .with_param("seconds", delay_seconds)
                    .with_param("cause", &cause),
                );
                self.publish_progress(&state.operation);
            }
            if !self.sleeper.sleep(delay, &job.cancellation).await {
                return result;
            }
            // A pause requested during the backoff holds the next attempt back.
            self.wait_while_blocked(job).await;
            if job.is_cancelled() {
                return result;
            }
            attempt += 1;
        }
    }

    /// Journals transfer jobs so a restart can tell what was interrupted (task 0299).
    #[must_use]
    pub fn with_journal(mut self, journal: SharedJournal) -> Self {
        self.journal = Some(journal);
        self
    }

    /// The journal for `kind`, when jobs of that kind are journaled.
    fn journal_for(&self, kind: crate::OperationKind) -> Option<&SharedJournal> {
        match kind {
            crate::OperationKind::Copy
            | crate::OperationKind::Move
            | crate::OperationKind::Duplicate => self.journal.as_ref(),
            _ => None,
        }
    }

    /// Queues an operation and immediately returns its stable identifier.
    pub fn submit(
        &self,
        operation: Operation,
        executor: Arc<dyn OperationExecutor>,
    ) -> Result<OperationId, SchedulerError> {
        let id = operation.id;
        let kind = operation.kind;
        self.events.publish(
            EventAudience::Global,
            BackendEventPayload::OperationCreated {
                operation: operation_payload(&operation),
            },
        );
        self.observe(&operation);
        let job = Arc::new(Job::new(operation));
        self.jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Arc::clone(&job));
        let scheduler = self.clone();
        tokio::spawn(async move {
            let result = scheduler
                .run_job(Arc::clone(&job), Arc::clone(&executor))
                .await;
            if let Err(error) = result {
                if job.is_cancelled() {
                    if let Err(cleanup_error) =
                        scheduler.finish_cancelled(&job, executor.as_ref()).await
                    {
                        scheduler.fail_job(
                            &job,
                            cleanup_error.code(),
                            &cleanup_error.to_string(),
                            cleanup_error.details(),
                        );
                    }
                } else {
                    let snapshot = job.snapshot();
                    let _ = executor.cleanup_partial(&snapshot).await;
                    scheduler.fail_job(&job, error.code(), &error.to_string(), error.details());
                }
            }
            // Terminal either way: the history entry carries the outcome, so the
            // journal record (which only exists to recover interrupted jobs) goes.
            // Queued, not awaited: waiting here would keep the scheduler alive past the service.
            if let Some(journal) = scheduler.journal_for(kind) {
                journal.discard(id);
            }
            job.completed.notify_waiters();
        });
        Ok(id)
    }

    /// Returns the latest operation snapshot.
    pub fn get(&self, id: OperationId) -> Result<Operation, SchedulerError> {
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let job = jobs.get(&id).ok_or(SchedulerError::UnknownOperation(id))?;
        Ok(job.snapshot())
    }

    /// Returns snapshots of every accepted operation, newest first.
    #[must_use]
    pub fn list(&self) -> Vec<Operation> {
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let mut operations: Vec<_> = jobs.values().map(|job| job.snapshot()).collect();
        operations.sort_by_key(|operation| std::cmp::Reverse(operation.created_at));
        operations
    }

    /// Marks a running operation as paused.
    pub fn pause(&self, id: OperationId) -> Result<(), SchedulerError> {
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let job = jobs.get(&id).ok_or(SchedulerError::UnknownOperation(id))?;
        self.transition_job(job, OperationState::Paused)?;
        job.pause.pause();
        Ok(())
    }

    /// Resumes a paused operation.
    pub fn resume(&self, id: OperationId) -> Result<(), SchedulerError> {
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let job = jobs.get(&id).ok_or(SchedulerError::UnknownOperation(id))?;
        self.transition_job(job, OperationState::Running)?;
        job.pause.resume();
        job.resumed.notify_waiters();
        Ok(())
    }

    /// Confirms an operation that paused after planning for explicit approval.
    pub fn confirm(&self, id: OperationId) -> Result<(), SchedulerError> {
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let job = jobs.get(&id).ok_or(SchedulerError::UnknownOperation(id))?;
        self.transition_job(job, OperationState::Running)?;
        job.resumed.notify_waiters();
        Ok(())
    }

    /// Continues an operation waiting for a conflict decision.
    pub fn resolve_conflict(
        &self,
        id: OperationId,
        resolution: ConflictResolution,
        apply_to_all: bool,
    ) -> Result<(), SchedulerError> {
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let job = jobs.get(&id).ok_or(SchedulerError::UnknownOperation(id))?;
        {
            let mut state = job.lock();
            {
                let conflict = state.pending_conflict.as_mut().ok_or_else(|| {
                    SchedulerError::Execution(Box::new(
                        crate::CodedFailure::new(
                            crate::codes::failure::NO_PENDING_CONFLICT,
                            "operation has no pending conflict",
                        )
                        .into(),
                    ))
                })?;
                conflict.decision = Some(resolution);
            }
            if apply_to_all {
                state.apply_to_all = Some(resolution);
            }
        }
        job.resumed.notify_waiters();
        Ok(())
    }

    /// Re-publishes all unresolved conflicts for a newly connected transport.
    pub fn republish_pending_conflicts(&self) {
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        for job in jobs.values() {
            let state = job.lock();
            if let Some(pending) = state.pending_conflict.as_ref() {
                self.publish_conflict(state.operation.id, &pending.conflict);
            }
        }
    }

    /// Requests cooperative cancellation at the next safe point.
    pub fn cancel(&self, id: OperationId) -> Result<(), SchedulerError> {
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let job = jobs.get(&id).ok_or(SchedulerError::UnknownOperation(id))?;
        job.cancellation.cancel();
        job.pause.resume();
        {
            let mut state = job.lock();
            if !state.operation.state.is_terminal()
                && state.operation.state != OperationState::Cancelling
            {
                self.transition_and_publish(&mut state.operation, OperationState::Cancelling)?;
            }
        }
        job.resumed.notify_waiters();
        Ok(())
    }

    /// Waits until a job reaches a terminal state.
    pub async fn wait(&self, id: OperationId) -> Result<(), SchedulerError> {
        let job = self
            .jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
            .ok_or(SchedulerError::UnknownOperation(id))?;
        loop {
            let notified = job.completed.notified();
            if job.lock().operation.state.is_terminal() {
                return Ok(());
            }
            notified.await;
        }
    }

    #[tracing::instrument(skip_all, fields(operation_id = %job.snapshot().id))]
    async fn run_job(
        &self,
        job: Arc<Job>,
        executor: Arc<dyn OperationExecutor>,
    ) -> Result<(), SchedulerError> {
        let snapshot = job.snapshot();
        let journal = self.journal_for(snapshot.kind).map(|journal| JobJournal {
            journal: Arc::clone(journal),
            operation: snapshot.id,
        });
        if let Some(journal) = &journal {
            // Journaling degrades recovery, never the transfer itself.
            if let Err(error) = journal.journal.begin(JournalIntent::of(&snapshot)).await {
                tracing::warn!(%error, "transfer journal begin failed; job is not recoverable");
            }
        }
        drop(snapshot);
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| ExecutionError::Failed("scheduler closed".into()))?;
        if job.is_cancelled() {
            self.finish_cancelled(&job, executor.as_ref()).await?;
            return Ok(());
        }
        let run_started = std::time::Instant::now();
        self.transition_job(&job, OperationState::Planning)?;
        let transition_ms = run_started.elapsed().as_secs_f64() * 1000.0;
        let snapshot = job.snapshot();
        let plan = executor.plan(&snapshot, &job.cancellation).await?;
        let planned_ms = run_started.elapsed().as_secs_f64() * 1000.0;
        if job.is_cancelled() {
            self.finish_cancelled(&job, executor.as_ref()).await?;
            return Ok(());
        }
        if let Some(journal) = &journal {
            journal.items(plan.items.iter());
        }
        {
            let mut state = job.lock();
            state.operation.progress.total_items = plan.total_items;
            state.operation.progress.total_bytes = plan.total_bytes;
            let next_state = if executor.requires_confirmation() {
                OperationState::WaitingForConflictResolution
            } else {
                OperationState::Running
            };
            self.publish_progress(&state.operation);
            self.transition_and_publish(&mut state.operation, next_state)?;
        }
        if executor.requires_confirmation() {
            self.wait_while_blocked(&job).await;
            if job.is_cancelled() {
                self.finish_cancelled(&job, executor.as_ref()).await?;
                return Ok(());
            }
        }
        let progress = Mutex::new(ProgressPublisher::new(Duration::from_millis(100), 0.25));
        let mut deferred = Vec::new();
        let mut pending: VecDeque<PlanItem> = plan.items.into();
        loop {
            while let Some(item) = pending.pop_front() {
                self.wait_while_blocked(&job).await;
                if job.is_cancelled() {
                    self.finish_cancelled(&job, executor.as_ref()).await?;
                    return Ok(());
                }
                let resolution = job.lock().apply_to_all;
                let reporter = ItemProgressReporter {
                    scheduler: self,
                    job: &job,
                    publisher: &progress,
                    current_entry: item.entry.clone(),
                    reported_bytes: AtomicU64::new(0),
                    journal: journal.as_ref(),
                };
                let execution = self
                    .execute_with_retry(&job, executor.as_ref(), &item, resolution, &reporter)
                    .await;
                let expand = matches!(&execution, Ok(ExecutionOutcome::Completed));
                let bulk = matches!(&execution, Ok(ExecutionOutcome::BulkCompleted(_)));
                if job.is_cancelled() && execution.is_err() {
                    self.finish_cancelled(&job, executor.as_ref()).await?;
                    return Ok(());
                }
                match execution {
                    Ok(ExecutionOutcome::Completed) => {
                        reporter.finish_completed(item.bytes);
                        if let Some(journal) = &journal {
                            journal.committed(&item);
                        }
                    }
                    Ok(ExecutionOutcome::CompletedWithWarnings(errors)) => {
                        reporter.finish_completed(item.bytes);
                        if let Some(journal) = &journal {
                            journal.committed(&item);
                        }
                        job.lock().operation.errors.extend(errors);
                    }
                    Ok(ExecutionOutcome::BulkCompleted(errors)) => {
                        job.lock().operation.errors.extend(errors);
                    }
                    Ok(ExecutionOutcome::Skipped) => {}
                    Err(error) => match error {
                        ExecutionError::Warning { error } => {
                            reporter.rollback();
                            job.lock().operation.errors.push(*error);
                        }
                        ExecutionError::Conflict(conflict) => {
                            reporter.rollback();
                            if job.lock().pending_conflict.is_none() {
                                self.register_conflict(&job, conflict.clone())?;
                            }
                            deferred.push((item, conflict));
                            continue;
                        }
                        other => return Err(other.into()),
                    },
                }
                {
                    let mut state = job.lock();
                    if !bulk {
                        state.operation.progress.completed_items =
                            state.operation.progress.completed_items.saturating_add(1);
                    }
                    state.operation.progress.current_entry = Some(item.entry.clone());
                }
                if expand {
                    let children = executor
                        .expand(&job.snapshot(), &item, &job.cancellation)
                        .await?;
                    if let Some(journal) = &journal {
                        journal.items(children.iter());
                    }
                    for child in children.into_iter().rev() {
                        pending.push_front(child);
                    }
                }
                if job.is_cancelled() {
                    self.finish_cancelled(&job, executor.as_ref()).await?;
                    return Ok(());
                }
            }
            if deferred.is_empty() {
                break;
            }
            let conflicts = std::mem::take(&mut deferred);
            for (item, known_conflict) in conflicts {
                loop {
                    let sticky_resolution = {
                        let mut state = job.lock();
                        let resolution = state.apply_to_all;
                        if resolution.is_some() && state.pending_conflict.take().is_some() {
                            self.transition_and_publish(
                                &mut state.operation,
                                OperationState::Running,
                            )?;
                        }
                        resolution
                    };
                    let resolution = if let Some(resolution) = sticky_resolution {
                        resolution
                    } else {
                        if job.lock().pending_conflict.is_none() {
                            self.register_conflict(&job, known_conflict.clone())?;
                        }
                        self.wait_for_conflict_decision(&job).await?
                    };
                    if job.is_cancelled() {
                        self.finish_cancelled(&job, executor.as_ref()).await?;
                        return Ok(());
                    }
                    let snapshot = job.snapshot();
                    let reporter = ItemProgressReporter {
                        scheduler: self,
                        job: &job,
                        publisher: &progress,
                        current_entry: item.entry.clone(),
                        reported_bytes: AtomicU64::new(0),
                        journal: journal.as_ref(),
                    };
                    match executor
                        .execute(
                            &snapshot,
                            &item,
                            Some(resolution),
                            &reporter,
                            &job.pause,
                            &job.cancellation,
                        )
                        .await
                    {
                        Ok(ExecutionOutcome::Completed) => {
                            reporter.finish_completed(item.bytes);
                            if let Some(journal) = &journal {
                                journal.committed(&item);
                            }
                            let children = executor
                                .expand(&job.snapshot(), &item, &job.cancellation)
                                .await?;
                            if let Some(journal) = &journal {
                                journal.items(children.iter());
                            }
                            for child in children.into_iter().rev() {
                                pending.push_front(child);
                            }
                            break;
                        }
                        Ok(ExecutionOutcome::CompletedWithWarnings(errors)) => {
                            reporter.finish_completed(item.bytes);
                            if let Some(journal) = &journal {
                                journal.committed(&item);
                            }
                            job.lock().operation.errors.extend(errors);
                            break;
                        }
                        Ok(ExecutionOutcome::BulkCompleted(errors)) => {
                            job.lock().operation.errors.extend(errors);
                            break;
                        }
                        Ok(ExecutionOutcome::Skipped) => break,
                        Err(ExecutionError::Conflict(conflict)) => {
                            reporter.rollback();
                            self.register_conflict(&job, conflict)?;
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                let mut state = job.lock();
                state.operation.progress.completed_items =
                    state.operation.progress.completed_items.saturating_add(1);
                state.operation.progress.current_entry = Some(item.entry);
            }
        }
        self.wait_while_blocked(&job).await;
        if job.is_cancelled() {
            self.finish_cancelled(&job, executor.as_ref()).await?;
            return Ok(());
        }
        let executed_ms = run_started.elapsed().as_secs_f64() * 1000.0;
        let snapshot = job.snapshot();
        executor.finish(&snapshot, &job.cancellation).await?;
        let finished_ms = run_started.elapsed().as_secs_f64() * 1000.0;
        let snapshot = job.snapshot();
        let undo = executor.undo_evidence(&snapshot, &job.cancellation).await?;
        tracing::debug!(
            target: "fm_perf",
            op = "run_job",
            transition_ms,
            planned_ms,
            executed_ms,
            finished_ms,
            undo_evidence_done_ms = run_started.elapsed().as_secs_f64() * 1000.0,
            "scheduler job phases (cumulative ms since permit)"
        );
        let mut state = job.lock();
        state.operation.undo = undo;
        let terminal = if state.operation.errors.is_empty() {
            OperationState::Completed
        } else {
            OperationState::CompletedWithWarnings
        };
        self.transition_and_publish(&mut state.operation, terminal)?;
        self.events.publish(
            EventAudience::Global,
            BackendEventPayload::OperationCompleted {
                operation: operation_payload(&state.operation),
            },
        );
        Ok(())
    }

    async fn wait_while_blocked(&self, job: &Job) {
        loop {
            let resumed = job.resumed.notified();
            if job.signal() == JobSignal::Proceed {
                return;
            }
            resumed.await;
        }
    }

    fn register_conflict(
        &self,
        job: &Job,
        conflict: OperationConflict,
    ) -> Result<(), SchedulerError> {
        let operation_id;
        {
            let mut state = job.lock();
            state.pending_conflict = Some(PendingConflict {
                conflict: conflict.clone(),
                decision: None,
            });
            self.transition_and_publish(
                &mut state.operation,
                OperationState::WaitingForConflictResolution,
            )?;
            operation_id = state.operation.id;
        }
        self.publish_conflict(operation_id, &conflict);
        Ok(())
    }

    async fn wait_for_conflict_decision(
        &self,
        job: &Job,
    ) -> Result<ConflictResolution, SchedulerError> {
        loop {
            let notified = job.resumed.notified();
            {
                let mut state = job.lock();
                let decision = state
                    .pending_conflict
                    .as_ref()
                    .and_then(|pending| pending.decision);
                if let Some(decision) = decision {
                    state.pending_conflict = None;
                    self.transition_and_publish(&mut state.operation, OperationState::Running)?;
                    return Ok(decision);
                }
            }
            if job.is_cancelled() {
                return Ok(ConflictResolution::Skip);
            }
            notified.await;
        }
    }

    fn publish_conflict(&self, operation_id: OperationId, conflict: &OperationConflict) {
        self.events.publish(
            EventAudience::Global,
            BackendEventPayload::OperationConflict {
                conflict: OperationConflictPayload {
                    operation_id,
                    conflict_id: conflict.id.clone(),
                    message: format!("{} already exists", conflict.destination.name),
                    source: conflict_entry_payload(&conflict.source),
                    destination: conflict_entry_payload(&conflict.destination),
                },
            },
        );
    }

    async fn finish_cancelled(
        &self,
        job: &Job,
        executor: &dyn OperationExecutor,
    ) -> Result<(), SchedulerError> {
        let snapshot = job.snapshot();
        executor.cleanup_partial(&snapshot).await?;
        let mut state = job.lock();
        if state.operation.state != OperationState::Cancelling {
            self.transition_and_publish(&mut state.operation, OperationState::Cancelling)?;
        }
        self.transition_and_publish(&mut state.operation, OperationState::Cancelled)?;
        Ok(())
    }

    fn transition_job(&self, job: &Job, state: OperationState) -> Result<(), SchedulerError> {
        let mut guard = job.lock();
        self.transition_and_publish(&mut guard.operation, state)
    }

    fn transition_and_publish(
        &self,
        operation: &mut Operation,
        state: OperationState,
    ) -> Result<(), SchedulerError> {
        operation.transition(state)?;
        self.events.publish(
            EventAudience::Global,
            BackendEventPayload::OperationStateChanged {
                operation_id: operation.id,
                state: state_payload(state),
            },
        );
        self.observe(operation);
        Ok(())
    }

    fn publish_progress(&self, operation: &Operation) {
        self.events.publish(
            EventAudience::Global,
            BackendEventPayload::OperationProgress {
                progress: OperationProgressPayload {
                    operation_id: operation.id,
                    progress: progress_payload(&operation.progress),
                },
            },
        );
    }

    fn fail_job(&self, job: &Job, code: &str, message: &str, details: Option<serde_json::Value>) {
        let mut state = job.lock();
        if !state.operation.state.is_terminal() {
            if state.operation.state == OperationState::Cancelling {
                let _ = state.operation.transition(OperationState::Failed);
            } else {
                let _ = self.transition_and_publish(&mut state.operation, OperationState::Failed);
            }
        }
        self.events.publish(
            EventAudience::Global,
            BackendEventPayload::OperationFailed {
                operation_id: state.operation.id,
                code: code.into(),
                message: message.into(),
                details,
            },
        );
        self.observe(&state.operation);
    }

    fn observe(&self, operation: &Operation) {
        if let Some(observer) = &self.observer {
            observer.observe(operation);
        }
    }
}

fn operation_payload(operation: &Operation) -> OperationPayload {
    OperationPayload {
        id: operation.id,
        kind: match operation.kind {
            crate::OperationKind::CreateArchive => OperationKindPayload::CreateArchive,
            crate::OperationKind::MoveToArchive => OperationKindPayload::MoveToArchive,
            crate::OperationKind::CreateDirectory => OperationKindPayload::CreateDirectory,
            crate::OperationKind::CreateFile => OperationKindPayload::CreateFile,
            crate::OperationKind::CreateLink => OperationKindPayload::CreateLink,
            crate::OperationKind::Rename => OperationKindPayload::Rename,
            crate::OperationKind::Copy => OperationKindPayload::Copy,
            crate::OperationKind::Move => OperationKindPayload::Move,
            crate::OperationKind::Duplicate => OperationKindPayload::Duplicate,
            crate::OperationKind::Trash => OperationKindPayload::Trash,
            crate::OperationKind::Delete => OperationKindPayload::Delete,
            crate::OperationKind::Undo => OperationKindPayload::Undo,
            crate::OperationKind::EditImageMetadata => OperationKindPayload::EditImageMetadata,
            crate::OperationKind::ChangeAttributes => OperationKindPayload::ChangeAttributes,
            crate::OperationKind::SplitFile => OperationKindPayload::SplitFile,
            crate::OperationKind::CombineFiles => OperationKindPayload::CombineFiles,
            crate::OperationKind::TransformGeo => OperationKindPayload::TransformGeo,
        },
        state: state_payload(operation.state),
        sources: operation.sources.iter().map(entry_payload).collect(),
        destination: operation.destination.as_ref().map(location_payload),
        progress: progress_payload(&operation.progress),
        conflict_policy: match operation.conflict_policy {
            crate::ConflictPolicy::Ask => ConflictPolicyPayload::Ask,
            crate::ConflictPolicy::Skip => ConflictPolicyPayload::Skip,
            crate::ConflictPolicy::Overwrite => ConflictPolicyPayload::Overwrite,
            crate::ConflictPolicy::RenameNew => ConflictPolicyPayload::RenameNew,
            crate::ConflictPolicy::KeepNewer => ConflictPolicyPayload::KeepNewer,
        },
        created_at: operation.created_at,
        started_at: operation.started_at,
        completed_at: operation.completed_at,
        undo: Some(operation_undo_payload(operation)),
        undo_of: operation.undo_of,
        verification: (!operation.verification.is_empty()).then_some(
            fm_events::OperationVerificationPayload {
                verified: operation.verification.verified,
                mismatched: operation.verification.mismatched,
                not_verified: operation.verification.not_verified,
                skipped: operation.verification.skipped,
            },
        ),
    }
}

fn operation_undo_payload(operation: &Operation) -> OperationUndoPayload {
    let status = operation.undo_status();
    OperationUndoPayload {
        available: status.available,
        reason: status.reason,
        reason_code: status.reason_code,
        operation_id: status.operation_id,
    }
}

const fn state_payload(state: OperationState) -> OperationStatePayload {
    match state {
        OperationState::Queued => OperationStatePayload::Queued,
        OperationState::Planning => OperationStatePayload::Planning,
        OperationState::Running => OperationStatePayload::Running,
        OperationState::Paused => OperationStatePayload::Paused,
        OperationState::WaitingForConflictResolution => {
            OperationStatePayload::WaitingForConflictResolution
        }
        OperationState::Cancelling => OperationStatePayload::Cancelling,
        OperationState::Cancelled => OperationStatePayload::Cancelled,
        OperationState::Completed => OperationStatePayload::Completed,
        OperationState::CompletedWithWarnings => OperationStatePayload::CompletedWithWarnings,
        OperationState::Failed => OperationStatePayload::Failed,
        OperationState::Interrupted => OperationStatePayload::Interrupted,
    }
}

fn progress_payload(progress: &OperationProgress) -> OperationProgressDetails {
    OperationProgressDetails {
        completed_items: progress.completed_items,
        total_items: progress.total_items,
        completed_bytes: progress.completed_bytes,
        total_bytes: progress.total_bytes,
        current_entry: progress.current_entry.as_ref().map(entry_payload),
        bytes_per_second: progress.bytes_per_second,
    }
}

fn entry_payload(entry: &EntryRef) -> EntryRefPayload {
    EntryRefPayload {
        id: entry.id,
        location: location_payload(&entry.location),
    }
}

fn conflict_entry_payload(entry: &crate::ConflictEntry) -> OperationConflictEntryPayload {
    OperationConflictEntryPayload {
        name: entry.name.clone(),
        kind: match entry.kind {
            fm_domain::EntryKind::File => EntryKindPayload::File,
            fm_domain::EntryKind::Directory => EntryKindPayload::Directory,
            fm_domain::EntryKind::Symlink => EntryKindPayload::Symlink,
        },
        size: entry.size,
        modified_at: entry.modified_at,
    }
}

fn location_payload(location: &fm_domain::Location) -> LocationPayload {
    LocationPayload {
        provider_id: location.provider_id.clone(),
        uri: location.uri.clone(),
    }
}

#[cfg(test)]
mod job_state_tests {
    use super::{
        ConflictResolution, Job, JobSignal, JobState, OperationConflict, OperationState,
        PendingConflict,
    };
    use crate::{ConflictEntry, ConflictPolicy, Operation, OperationKind};
    use fm_domain::EntryKind;

    fn test_operation() -> Operation {
        Operation::new(OperationKind::Copy, vec![], None, ConflictPolicy::Ask)
    }

    fn test_conflict() -> OperationConflict {
        OperationConflict {
            id: "conflict-1".into(),
            source: ConflictEntry {
                name: "source.txt".into(),
                kind: EntryKind::File,
                size: None,
                modified_at: None,
            },
            destination: ConflictEntry {
                name: "source.txt".into(),
                kind: EntryKind::File,
                size: None,
                modified_at: None,
            },
        }
    }

    fn running_state() -> JobState {
        let mut operation = test_operation();
        operation.transition(OperationState::Planning).unwrap();
        operation.transition(OperationState::Running).unwrap();
        JobState::new(operation)
    }

    #[test]
    fn fresh_job_is_not_blocking() {
        let state = JobState::new(test_operation());
        assert!(!state.blocking());
    }

    #[test]
    fn paused_job_blocks() {
        let mut state = running_state();
        state.operation.transition(OperationState::Paused).unwrap();
        assert!(state.blocking());
    }

    #[test]
    fn waiting_for_confirmation_without_a_registered_conflict_blocks() {
        // `requires_confirmation()` operations reuse WaitingForConflictResolution
        // as an initial confirmation gate, with no item conflict registered.
        let mut state = running_state();
        state
            .operation
            .transition(OperationState::WaitingForConflictResolution)
            .unwrap();
        assert!(state.blocking());
    }

    #[test]
    fn waiting_with_a_registered_item_conflict_does_not_block_the_main_loop() {
        // A registered per-item conflict is resolved by the deferred-items
        // pass's own wait, not by `wait_while_blocked` in the main loop —
        // blocking here too would deadlock (see `blocking`'s doc comment).
        let mut state = running_state();
        state
            .operation
            .transition(OperationState::WaitingForConflictResolution)
            .unwrap();
        state.pending_conflict = Some(PendingConflict {
            conflict: test_conflict(),
            decision: None,
        });
        assert!(!state.blocking());
    }

    #[test]
    fn cancellation_overrides_a_pause_block() {
        let job = Job::new(running_state().operation);
        job.lock()
            .operation
            .transition(OperationState::Paused)
            .unwrap();
        assert_eq!(job.signal(), JobSignal::Blocked);

        job.cancellation.cancel();
        assert_eq!(job.signal(), JobSignal::Proceed);
        assert!(job.is_cancelled());
    }

    #[test]
    fn cancellation_overrides_a_confirmation_wait_block() {
        let job = Job::new(running_state().operation);
        job.lock()
            .operation
            .transition(OperationState::WaitingForConflictResolution)
            .unwrap();
        assert_eq!(job.signal(), JobSignal::Blocked);

        job.cancellation.cancel();
        assert_eq!(job.signal(), JobSignal::Proceed);
    }

    #[test]
    fn cancelling_the_operation_itself_also_unblocks_a_confirmation_wait() {
        let job = Job::new(running_state().operation);
        {
            let mut state = job.lock();
            state
                .operation
                .transition(OperationState::WaitingForConflictResolution)
                .unwrap();
            state
                .operation
                .transition(OperationState::Cancelling)
                .unwrap();
        }
        assert!(!job.lock().blocking());
    }

    #[test]
    fn a_registered_item_conflict_never_blocks_regardless_of_cancellation() {
        let job = Job::new(running_state().operation);
        {
            let mut state = job.lock();
            state
                .operation
                .transition(OperationState::WaitingForConflictResolution)
                .unwrap();
            state.pending_conflict = Some(PendingConflict {
                conflict: test_conflict(),
                decision: None,
            });
        }
        assert_eq!(job.signal(), JobSignal::Proceed);

        job.cancellation.cancel();
        assert_eq!(job.signal(), JobSignal::Proceed);
    }

    #[test]
    fn apply_to_all_is_readable_after_being_recorded() {
        let job = Job::new(test_operation());
        assert!(job.lock().apply_to_all.is_none());
        job.lock().apply_to_all = Some(ConflictResolution::Overwrite);
        assert_eq!(job.lock().apply_to_all, Some(ConflictResolution::Overwrite));
    }
}
