use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fm_domain::OperationId;
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
    /// Typed implementation failure.
    #[error("operation execution failed: {0}")]
    Failed(String),
    /// Provider-neutral filesystem failure.
    #[error(transparent)]
    Provider(#[from] fm_vfs::VfsError),
    /// An entry failed but independent plan items may continue.
    #[error("{message}")]
    Warning {
        /// Entry that failed.
        entry: EntryRef,
        /// Sanitized failure text.
        message: String,
    },
    /// This item needs an explicit user decision before it can be retried.
    #[error("operation item has a destination conflict")]
    Conflict(OperationConflict),
}

/// Result of executing one plan item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionOutcome {
    /// The item was applied and contributes its planned bytes.
    Completed,
    /// The item was deliberately skipped and contributes no bytes.
    Skipped,
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
        pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError>;

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
            "This operation does not retain enough evidence to be undone safely.",
        ))
    }

    /// Whether execution must wait for explicit user confirmation after planning.
    fn requires_confirmation(&self) -> bool {
        false
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
        }
    }

    /// Adds a synchronous snapshot observer used by durable history stores.
    #[must_use]
    pub fn with_observer(mut self, observer: Arc<dyn OperationSnapshotObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Queues an operation and immediately returns its stable identifier.
    pub fn submit(
        &self,
        operation: Operation,
        executor: Arc<dyn OperationExecutor>,
    ) -> Result<OperationId, SchedulerError> {
        let id = operation.id;
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
                        scheduler.fail_job(&job, &cleanup_error.to_string());
                    }
                } else {
                    let snapshot = job.snapshot();
                    let _ = executor.cleanup_partial(&snapshot).await;
                    scheduler.fail_job(&job, &error.to_string());
                }
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
                    SchedulerError::Execution(Box::new(ExecutionError::Failed(
                        "operation has no pending conflict".into(),
                    )))
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
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| ExecutionError::Failed("scheduler closed".into()))?;
        if job.is_cancelled() {
            self.finish_cancelled(&job, executor.as_ref()).await?;
            return Ok(());
        }
        self.transition_job(&job, OperationState::Planning)?;
        let snapshot = job.snapshot();
        let plan = executor.plan(&snapshot, &job.cancellation).await?;
        if job.is_cancelled() {
            self.finish_cancelled(&job, executor.as_ref()).await?;
            return Ok(());
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
        let mut progress = ProgressPublisher::new(Duration::from_millis(100), 0.25);
        let mut deferred = Vec::new();
        for item in &plan.items {
            self.wait_while_blocked(&job).await;
            if job.is_cancelled() {
                self.finish_cancelled(&job, executor.as_ref()).await?;
                return Ok(());
            }
            let snapshot = job.snapshot();
            let resolution = job.lock().apply_to_all;
            let execution = executor
                .execute(&snapshot, item, resolution, &job.pause, &job.cancellation)
                .await;
            if job.is_cancelled() && execution.is_err() {
                self.finish_cancelled(&job, executor.as_ref()).await?;
                return Ok(());
            }
            let mut completed_bytes = item.bytes;
            match execution {
                Ok(ExecutionOutcome::Completed) => {}
                Ok(ExecutionOutcome::Skipped) => completed_bytes = 0,
                Err(error) => match error {
                    ExecutionError::Warning { entry, message } => {
                        job.lock()
                            .operation
                            .errors
                            .push(crate::OperationEntryError { entry, message });
                        completed_bytes = 0;
                    }
                    ExecutionError::Conflict(conflict) => {
                        if job.lock().pending_conflict.is_none() {
                            self.register_conflict(&job, conflict.clone())?;
                        }
                        deferred.push((item.clone(), conflict));
                        continue;
                    }
                    other => return Err(other.into()),
                },
            }
            {
                let mut state = job.lock();
                state.operation.progress.completed_items =
                    state.operation.progress.completed_items.saturating_add(1);
                state.operation.progress.completed_bytes = state
                    .operation
                    .progress
                    .completed_bytes
                    .saturating_add(completed_bytes);
                state.operation.progress.current_entry = Some(item.entry.clone());
                if let Some(rate) = progress.record(Instant::now(), completed_bytes) {
                    state.operation.progress.bytes_per_second = rate.bytes_per_second;
                    self.publish_progress(&state.operation);
                }
            }
            if job.is_cancelled() {
                self.finish_cancelled(&job, executor.as_ref()).await?;
                return Ok(());
            }
        }
        for (item, known_conflict) in deferred {
            let outcome = loop {
                let sticky_resolution = {
                    let mut state = job.lock();
                    let resolution = state.apply_to_all;
                    if resolution.is_some() && state.pending_conflict.take().is_some() {
                        self.transition_and_publish(&mut state.operation, OperationState::Running)?;
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
                match executor
                    .execute(
                        &snapshot,
                        &item,
                        Some(resolution),
                        &job.pause,
                        &job.cancellation,
                    )
                    .await
                {
                    Ok(outcome) => break outcome,
                    Err(ExecutionError::Conflict(conflict)) => {
                        self.register_conflict(&job, conflict)?;
                    }
                    Err(error) => return Err(error.into()),
                }
            };
            let completed_bytes = if outcome == ExecutionOutcome::Completed {
                item.bytes
            } else {
                0
            };
            let mut state = job.lock();
            state.operation.progress.completed_items =
                state.operation.progress.completed_items.saturating_add(1);
            state.operation.progress.completed_bytes = state
                .operation
                .progress
                .completed_bytes
                .saturating_add(completed_bytes);
            state.operation.progress.current_entry = Some(item.entry);
        }
        self.wait_while_blocked(&job).await;
        if job.is_cancelled() {
            self.finish_cancelled(&job, executor.as_ref()).await?;
            return Ok(());
        }
        let snapshot = job.snapshot();
        executor.finish(&snapshot, &job.cancellation).await?;
        let snapshot = job.snapshot();
        let undo = executor.undo_evidence(&snapshot, &job.cancellation).await?;
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

    fn fail_job(&self, job: &Job, message: &str) {
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
                code: "operationFailed".into(),
                message: message.into(),
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
            crate::OperationKind::Rename => OperationKindPayload::Rename,
            crate::OperationKind::Copy => OperationKindPayload::Copy,
            crate::OperationKind::Move => OperationKindPayload::Move,
            crate::OperationKind::Duplicate => OperationKindPayload::Duplicate,
            crate::OperationKind::Trash => OperationKindPayload::Trash,
            crate::OperationKind::Delete => OperationKindPayload::Delete,
            crate::OperationKind::Undo => OperationKindPayload::Undo,
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
    }
}

fn operation_undo_payload(operation: &Operation) -> OperationUndoPayload {
    if let Some(id) = operation.undo.undone_by {
        return OperationUndoPayload {
            available: false,
            reason: Some("This operation has already been undone.".into()),
            operation_id: Some(id),
        };
    }
    if let Some(id) = operation.undo.pending_operation {
        return OperationUndoPayload {
            available: false,
            reason: Some("Undo is already in progress for this operation.".into()),
            operation_id: Some(id),
        };
    }
    let available = operation.undo.plan.is_some()
        && matches!(
            operation.state,
            OperationState::Completed | OperationState::CompletedWithWarnings
        );
    OperationUndoPayload {
        available,
        reason: if available {
            None
        } else {
            operation
                .undo
                .unavailable_reason
                .clone()
                .or_else(|| Some("This operation cannot currently be undone safely.".into()))
        },
        operation_id: None,
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
