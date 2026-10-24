//! Crash-safe operation history: persists terminal operation snapshots beside settings so
//! `FileManagerService::list_operations`/`get_operation` can serve them after a restart, and
//! observes the scheduler to keep that persisted record and affected directory listings in sync.
//!
//! Split out of the `FileManagerService` facade (task 0119) — this cluster (history persistence,
//! the operation/directory-refresh observer, and operation-DTO conversion) was self-contained and
//! had no dependency on the rest of the facade beyond `DirectoryService`.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use fm_domain::OperationId;
use fm_operations::{Operation, OperationSnapshotObserver, OperationState, UndoPlan};
use fm_transport_dto::{
    EntryRefDto, OperationConflictPolicyDto, OperationDto, OperationEntryErrorDto,
    OperationKindDto, OperationProgressDto, OperationStateDto, OperationUndoDto,
};

use crate::DirectoryService;

pub(crate) const OPERATION_HISTORY_FILE_NAME: &str = "operation-history.json";
const OPERATION_HISTORY_MAX_ENTRIES: usize = 100;
const OPERATION_HISTORY_MAX_AGE_DAYS: i64 = 30;
/// Entry-scoped failures kept per operation in the persisted history. A cancelled delete of a
/// 5.7M-item cache directory once accumulated 38k errors (15 MB), which made every history save
/// serialize and fsync a 19 MB file on the scheduler's critical path.
const OPERATION_HISTORY_MAX_ENTRY_ERRORS: usize = 200;
const OMITTED_ERRORS_SUFFIX: &str = " further errors are omitted from the saved history";

/// Crash-safe operation snapshots stored beside settings.
pub(crate) struct OperationHistory {
    operations: Arc<Mutex<Vec<Operation>>>,
    writer: HistoryWriter,
}

/// Write-behind persistence for [`OperationHistory`].
///
/// Every scheduler state transition used to serialize and `fsync` the whole history on the
/// scheduler's own thread; on macOS that `fsync` costs roughly 300 ms, so a single-file delete
/// (queued, planning, running, completed) took over a second to reach the UI. Transitions now only
/// mark the history dirty; a dedicated thread coalesces bursts into one durable write of the
/// latest snapshot. Dropping the history drains the pending write.
struct HistoryWriter {
    shared: Arc<WriterShared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Default)]
struct WriterState {
    dirty: bool,
    writing: bool,
    closed: bool,
}

#[derive(Default)]
struct WriterShared {
    state: Mutex<WriterState>,
    wake: Condvar,
    idle: Condvar,
}

impl HistoryWriter {
    fn spawn(path: PathBuf, operations: Arc<Mutex<Vec<Operation>>>) -> Self {
        let shared = Arc::new(WriterShared::default());
        let worker = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("operation-history-writer".into())
            .spawn(move || {
                loop {
                    {
                        let mut state = worker.state.lock().unwrap_or_else(|e| e.into_inner());
                        while !state.dirty && !state.closed {
                            state = worker.wake.wait(state).unwrap_or_else(|e| e.into_inner());
                        }
                        if !state.dirty {
                            return;
                        }
                        state.dirty = false;
                        state.writing = true;
                    }
                    // Snapshot under the lock, serialize outside it: `observe` needs the same lock on
                    // the scheduler's thread, and serializing every retained undo plan took ~300 ms
                    // in debug builds, stalling each job's first state transition.
                    let started = std::time::Instant::now();
                    let snapshot = operations.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    let snapshot_ms = started.elapsed().as_secs_f64() * 1000.0;
                    if let Ok(bytes) = serde_json::to_vec_pretty(&snapshot) {
                        let serialized_ms = started.elapsed().as_secs_f64() * 1000.0;
                        write_history_file(&path, &bytes);
                        tracing::debug!(
                            target: "fm_perf",
                            op = "history_save",
                            operations = snapshot.len(),
                            bytes = bytes.len(),
                            snapshot_ms,
                            serialized_ms,
                            total_ms = started.elapsed().as_secs_f64() * 1000.0,
                            "operation history saved"
                        );
                    }
                    let mut state = worker.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.writing = false;
                    worker.idle.notify_all();
                }
            })
            .ok();
        Self {
            shared,
            thread: Mutex::new(thread),
        }
    }

    /// Marks the history dirty; returns immediately.
    fn request_save(&self) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.dirty = true;
        self.shared.wake.notify_one();
    }

    /// Blocks until every requested save has reached disk.
    #[cfg(test)]
    fn flush(&self) {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        while state.dirty || state.writing {
            state = self
                .shared
                .idle
                .wait(state)
                .unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Drop for HistoryWriter {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            state.closed = true;
            self.shared.wake.notify_one();
        }
        if let Some(thread) = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = thread.join();
        }
    }
}

/// Bounds the persisted per-entry failures of one operation, replacing the excess with a single
/// summary entry so the error count stays visible.
fn cap_entry_errors(operation: &mut Operation) {
    if operation.errors.len() <= OPERATION_HISTORY_MAX_ENTRY_ERRORS {
        return;
    }
    let omitted = operation.errors.len() - OPERATION_HISTORY_MAX_ENTRY_ERRORS;
    let mut summary = operation.errors[OPERATION_HISTORY_MAX_ENTRY_ERRORS].clone();
    operation
        .errors
        .truncate(OPERATION_HISTORY_MAX_ENTRY_ERRORS);
    summary.message = format!("{omitted}{OMITTED_ERRORS_SUFFIX}");
    operation.errors.push(summary);
}

/// Total entry failures, including those folded into a [`cap_entry_errors`] summary entry.
fn entry_error_count(operation: &Operation) -> usize {
    let omitted = operation
        .errors
        .last()
        .and_then(|error| error.message.strip_suffix(OMITTED_ERRORS_SUFFIX))
        .and_then(|count| count.parse::<usize>().ok());
    match omitted {
        Some(omitted) => operation.errors.len() - 1 + omitted,
        None => operation.errors.len(),
    }
}

/// Atomically replaces the history file (temporary file, `fsync`, rename).
fn write_history_file(path: &Path, bytes: &[u8]) {
    let Some(directory) = path.parent() else {
        return;
    };
    if fs::create_dir_all(directory).is_err() {
        return;
    }
    let temporary = directory.join(format!(
        ".{OPERATION_HISTORY_FILE_NAME}.{}.tmp",
        std::process::id()
    ));
    if fs::File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .is_ok()
    {
        let _ = fs::rename(temporary, path);
    }
}

/// Bridges the scheduler's [`OperationSnapshotObserver`] callback to both history persistence
/// (via [`OperationHistory`]) and refreshing any directory listings an operation touched.
pub(crate) struct ApplicationOperationObserver {
    history: Arc<OperationHistory>,
    directories: DirectoryService,
}

impl ApplicationOperationObserver {
    pub(crate) fn new(history: Arc<OperationHistory>, directories: DirectoryService) -> Self {
        Self {
            history,
            directories,
        }
    }
}

impl OperationHistory {
    pub(crate) fn load(directory: &Path) -> Self {
        let path = directory.join(OPERATION_HISTORY_FILE_NAME);
        let mut operations = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<Operation>>(&bytes).ok())
            .unwrap_or_default();
        let now = chrono::Utc::now();
        for operation in &mut operations {
            cap_entry_errors(operation);
            if !operation.state.is_terminal() {
                operation.state = fm_operations::OperationState::Interrupted;
                operation.completed_at = Some(now);
            }
        }
        let interrupted_undo_ids = operations
            .iter()
            .filter(|operation| {
                operation.undo_of.is_some() && operation.state == OperationState::Interrupted
            })
            .map(|operation| operation.id)
            .collect::<HashSet<_>>();
        let known_ids = operations
            .iter()
            .map(|operation| operation.id)
            .collect::<HashSet<_>>();
        for operation in &mut operations {
            let Some(pending_id) = operation.undo.pending_operation else {
                continue;
            };
            if interrupted_undo_ids.contains(&pending_id) {
                operation.undo.pending_operation = None;
                operation.undo.plan = None;
                operation.undo.unavailable_reason = Some(
                    "The previous undo was interrupted, so its remaining effects cannot be reversed safely."
                        .into(),
                );
            } else if !known_ids.contains(&pending_id) {
                operation.undo.pending_operation = None;
            }
        }
        let operations = Arc::new(Mutex::new(operations));
        let history = Self {
            operations: Arc::clone(&operations),
            writer: HistoryWriter::spawn(path, operations),
        };
        history.prune_and_save();
        history
    }

    pub(crate) fn list(&self) -> Vec<OperationDto> {
        self.operations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .filter(|operation| operation.state.is_terminal())
            .cloned()
            .map(|operation| operation_dto(operation, None))
            .collect()
    }

    pub(crate) fn get(&self, id: OperationId) -> Option<OperationDto> {
        self.operations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .find(|operation| operation.id == id && operation.state.is_terminal())
            .cloned()
            .map(|operation| operation_dto(operation, None))
    }

    pub(crate) fn record(&self, id: OperationId) -> Option<Operation> {
        self.operations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .find(|operation| operation.id == id && operation.state.is_terminal())
            .cloned()
    }

    pub(crate) fn reserve_undo(
        &self,
        id: OperationId,
        undo_id: OperationId,
    ) -> Result<(Operation, UndoPlan), String> {
        let result = {
            let mut operations = self
                .operations
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let operation = operations
                .iter_mut()
                .find(|operation| operation.id == id && operation.state.is_terminal())
                .ok_or_else(|| "Operation history entry was not found.".to_owned())?;
            if operation.undo.undone_by.is_some() {
                return Err("This operation has already been undone.".into());
            }
            if operation.undo.pending_operation.is_some() {
                return Err("Undo is already in progress for this operation.".into());
            }
            let plan = operation.undo.plan.clone().ok_or_else(|| {
                operation
                    .undo
                    .unavailable_reason
                    .clone()
                    .unwrap_or_else(|| "This operation cannot be undone safely.".into())
            })?;
            operation.undo.pending_operation = Some(undo_id);
            Ok((operation.clone(), plan))
        };
        if result.is_ok() {
            self.prune_and_save();
        }
        result
    }

    pub(crate) fn release_undo(&self, id: OperationId, undo_id: OperationId) {
        {
            let mut operations = self
                .operations
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(operation) = operations.iter_mut().find(|operation| operation.id == id)
                && operation.undo.pending_operation == Some(undo_id)
            {
                operation.undo.pending_operation = None;
            }
        }
        self.prune_and_save();
    }

    fn prune_and_save(&self) {
        let mut operations = self
            .operations
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let cutoff = chrono::Utc::now() - chrono::Duration::days(OPERATION_HISTORY_MAX_AGE_DAYS);
        operations.retain(|operation| {
            !operation.state.is_terminal()
                || operation
                    .completed_at
                    .is_none_or(|completed_at| completed_at >= cutoff)
        });
        let mut terminal = operations
            .iter()
            .enumerate()
            .filter(|(_, operation)| operation.state.is_terminal())
            .map(|(index, operation)| (index, operation.completed_at))
            .collect::<Vec<_>>();
        terminal.sort_by_key(|(_, completed_at)| *completed_at);
        let excess = terminal.len().saturating_sub(OPERATION_HISTORY_MAX_ENTRIES);
        let remove = terminal
            .into_iter()
            .take(excess)
            .map(|(index, _)| index)
            .collect::<HashSet<_>>();
        if !remove.is_empty() {
            *operations = operations
                .iter()
                .enumerate()
                .filter(|(index, _)| !remove.contains(index))
                .map(|(_, operation)| operation.clone())
                .collect();
        }
        drop(operations);
        self.writer.request_save();
    }
}

impl OperationSnapshotObserver for OperationHistory {
    fn observe(&self, operation: &Operation) {
        {
            let mut operations = self
                .operations
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(previous) = operations
                .iter_mut()
                .find(|previous| previous.id == operation.id)
            {
                *previous = operation.clone();
                cap_entry_errors(previous);
            } else {
                let mut stored = operation.clone();
                cap_entry_errors(&mut stored);
                operations.push(stored);
            }
            if let Some(original_id) = operation.undo_of
                && operation.state.is_terminal()
                && let Some(original) = operations
                    .iter_mut()
                    .find(|candidate| candidate.id == original_id)
            {
                original.undo.pending_operation = None;
                if matches!(
                    operation.state,
                    OperationState::Completed | OperationState::CompletedWithWarnings
                ) {
                    original.undo.undone_by = Some(operation.id);
                } else if operation.progress.completed_items > 0 {
                    original.undo.plan = None;
                    original.undo.unavailable_reason = Some(
                        "The previous undo completed only partially, so it cannot be retried safely."
                            .into(),
                    );
                }
            }
        }
        self.prune_and_save();
    }
}

impl OperationSnapshotObserver for ApplicationOperationObserver {
    fn observe(&self, operation: &Operation) {
        self.history.observe(operation);
        if !operation.state.is_terminal() {
            return;
        }
        let millis = |from: chrono::DateTime<chrono::Utc>,
                      to: Option<chrono::DateTime<chrono::Utc>>| {
            to.map(|to| (to - from).num_milliseconds())
        };
        tracing::debug!(
            target: "fm_perf",
            op = "operation_terminal",
            kind = ?operation.kind,
            state = ?operation.state,
            queue_to_start_ms = ?millis(operation.created_at, operation.started_at),
            created_to_completed_ms = ?millis(operation.created_at, operation.completed_at),
            "operation reached a terminal state"
        );
        let mut affected = HashSet::new();
        for source in &operation.sources {
            affected.insert(source.location.clone());
            if let Ok(Some(parent)) = source.location.parent() {
                affected.insert(parent);
            }
        }
        if let Some(destination) = &operation.destination {
            affected.insert(destination.clone());
            if let Ok(Some(parent)) = destination.parent() {
                affected.insert(parent);
            }
        }
        if affected.is_empty() {
            return;
        }
        let directories = self.directories.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let started = std::time::Instant::now();
                directories.refresh_affected(&affected).await;
                tracing::debug!(
                    target: "fm_perf",
                    op = "refresh_affected",
                    ms = started.elapsed().as_secs_f64() * 1000.0,
                    "post-operation pane refresh published"
                );
            });
        }
    }
}

pub(crate) fn operation_dto(mut operation: Operation, queue_position: Option<u64>) -> OperationDto {
    // Summarize first: capping replaces the excess errors with one summary entry, and a running
    // operation that fails on millions of entries must not ship them all to the client.
    let result_summary = operation_result_summary(&operation);
    cap_entry_errors(&mut operation);
    let undo = operation_undo_dto(&operation);
    OperationDto {
        id: operation.id.into(),
        operation_type: match operation.kind {
            fm_operations::OperationKind::CreateArchive => OperationKindDto::CreateArchive,
            fm_operations::OperationKind::MoveToArchive => OperationKindDto::MoveToArchive,
            fm_operations::OperationKind::CreateDirectory => OperationKindDto::CreateDirectory,
            fm_operations::OperationKind::CreateFile => OperationKindDto::CreateFile,
            fm_operations::OperationKind::CreateLink => OperationKindDto::CreateLink,
            fm_operations::OperationKind::Rename => OperationKindDto::Rename,
            fm_operations::OperationKind::Copy => OperationKindDto::Copy,
            fm_operations::OperationKind::Move => OperationKindDto::Move,
            fm_operations::OperationKind::Duplicate => OperationKindDto::Duplicate,
            fm_operations::OperationKind::Trash => OperationKindDto::Trash,
            fm_operations::OperationKind::Delete => OperationKindDto::Delete,
            fm_operations::OperationKind::Undo => OperationKindDto::Undo,
            fm_operations::OperationKind::EditImageMetadata => OperationKindDto::EditImageMetadata,
            fm_operations::OperationKind::ChangeAttributes => OperationKindDto::ChangeAttributes,
            fm_operations::OperationKind::SplitFile => OperationKindDto::SplitFile,
            fm_operations::OperationKind::CombineFiles => OperationKindDto::CombineFiles,
            fm_operations::OperationKind::TransformGeo => OperationKindDto::TransformGeo,
        },
        state: match operation.state {
            fm_operations::OperationState::Queued => OperationStateDto::Queued,
            fm_operations::OperationState::Planning => OperationStateDto::Planning,
            fm_operations::OperationState::Running => OperationStateDto::Running,
            fm_operations::OperationState::Paused => OperationStateDto::Paused,
            fm_operations::OperationState::WaitingForConflictResolution => {
                OperationStateDto::WaitingForConflictResolution
            }
            fm_operations::OperationState::Cancelling => OperationStateDto::Cancelling,
            fm_operations::OperationState::Cancelled => OperationStateDto::Cancelled,
            fm_operations::OperationState::Completed => OperationStateDto::Completed,
            fm_operations::OperationState::CompletedWithWarnings => {
                OperationStateDto::CompletedWithWarnings
            }
            fm_operations::OperationState::Failed => OperationStateDto::Failed,
            fm_operations::OperationState::Interrupted => OperationStateDto::Interrupted,
        },
        sources: operation
            .sources
            .into_iter()
            .map(|entry| EntryRefDto {
                id: entry.id.into(),
                location: entry.location.into(),
            })
            .collect(),
        destination: operation.destination.map(Into::into),
        progress: OperationProgressDto {
            completed_items: operation.progress.completed_items,
            total_items: operation.progress.total_items,
            completed_bytes: operation.progress.completed_bytes,
            total_bytes: operation.progress.total_bytes,
            current_entry: operation.progress.current_entry.map(|entry| EntryRefDto {
                id: entry.id.into(),
                location: entry.location.into(),
            }),
            bytes_per_second: operation.progress.bytes_per_second,
        },
        conflict_policy: match operation.conflict_policy {
            fm_operations::ConflictPolicy::Ask => OperationConflictPolicyDto::Ask,
            fm_operations::ConflictPolicy::Skip => OperationConflictPolicyDto::Skip,
            fm_operations::ConflictPolicy::Overwrite => OperationConflictPolicyDto::Overwrite,
            fm_operations::ConflictPolicy::RenameNew => OperationConflictPolicyDto::RenameNew,
            fm_operations::ConflictPolicy::KeepNewer => OperationConflictPolicyDto::KeepNewer,
        },
        created_at: operation.created_at,
        started_at: operation.started_at,
        completed_at: operation.completed_at,
        errors: operation
            .errors
            .into_iter()
            .map(|error| OperationEntryErrorDto {
                entry: EntryRefDto {
                    id: error.entry.id.into(),
                    location: error.entry.location.into(),
                },
                message: error.message,
            })
            .collect(),
        queue_position,
        result_summary,
        undo,
        undo_of: operation.undo_of.map(Into::into),
    }
}

fn operation_undo_dto(operation: &Operation) -> OperationUndoDto {
    if let Some(id) = operation.undo.undone_by {
        return OperationUndoDto {
            available: false,
            reason: Some("This operation has already been undone.".into()),
            operation_id: Some(id.into()),
        };
    }
    if let Some(id) = operation.undo.pending_operation {
        return OperationUndoDto {
            available: false,
            reason: Some("Undo is already in progress for this operation.".into()),
            operation_id: Some(id.into()),
        };
    }
    let available = operation.undo.plan.is_some()
        && matches!(
            operation.state,
            OperationState::Completed | OperationState::CompletedWithWarnings
        );
    OperationUndoDto {
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

fn operation_result_summary(operation: &Operation) -> Option<String> {
    match operation.state {
        fm_operations::OperationState::Completed => Some(format!(
            "Completed {} items.",
            operation.progress.completed_items
        )),
        fm_operations::OperationState::CompletedWithWarnings => Some(format!(
            "Completed with {} warnings.",
            entry_error_count(operation)
        )),
        fm_operations::OperationState::Cancelled => Some(format!(
            "Cancelled after {} items.",
            operation.progress.completed_items
        )),
        fm_operations::OperationState::Failed => Some("Operation failed.".into()),
        fm_operations::OperationState::Interrupted => Some(format!(
            "Interrupted after {} items; it was not resumed.",
            operation.progress.completed_items
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use fm_operations::{ConflictPolicy, OperationKind};

    use super::*;

    fn planning_operation() -> Operation {
        let mut operation =
            Operation::new(OperationKind::Delete, vec![], None, ConflictPolicy::Ask);
        operation
            .transition(OperationState::Planning)
            .expect("queued operation starts planning");
        operation
    }

    #[test]
    fn observed_transitions_are_persisted_by_the_write_behind_thread() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let history = OperationHistory::load(directory.path());
        let operation = planning_operation();

        history.observe(&operation);
        history.writer.flush();

        let reloaded = OperationHistory::load(directory.path());
        assert_eq!(
            reloaded.list().len(),
            1,
            "in-flight operation is restored as interrupted"
        );
    }

    #[test]
    fn persisted_entry_errors_are_bounded_with_a_summary_entry() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let history = OperationHistory::load(directory.path());
        let mut operation = planning_operation();
        let entry = fm_vfs::EntryRef {
            id: fm_domain::EntryId::new(),
            location: fm_domain::Location::from_native_path(directory.path())
                .expect("local location"),
        };
        operation.errors = (0..1_000)
            .map(|index| fm_operations::OperationEntryError {
                entry: entry.clone(),
                message: format!("failure {index}"),
            })
            .collect();
        operation
            .transition(OperationState::Cancelling)
            .expect("planning operation can be cancelled");
        operation
            .transition(OperationState::Cancelled)
            .expect("cancelling operation is cancelled");

        history.observe(&operation);

        let record = history.record(operation.id).expect("terminal record");
        assert_eq!(record.errors.len(), OPERATION_HISTORY_MAX_ENTRY_ERRORS + 1);
        assert_eq!(
            record.errors.last().map(|error| error.message.as_str()),
            Some("800 further errors are omitted from the saved history")
        );
        assert_eq!(entry_error_count(&record), 1_000);
    }

    #[test]
    fn dropping_the_history_drains_the_pending_write() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let operation = planning_operation();
        {
            let history = OperationHistory::load(directory.path());
            history.observe(&operation);
        }

        let reloaded = OperationHistory::load(directory.path());
        assert_eq!(reloaded.list().len(), 1);
    }

    #[test]
    fn a_burst_of_transitions_keeps_only_the_latest_snapshot() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let history = OperationHistory::load(directory.path());
        let mut operation = planning_operation();
        history.observe(&operation);
        operation
            .transition(OperationState::Running)
            .expect("planning operation starts running");
        history.observe(&operation);
        operation
            .transition(OperationState::Completed)
            .expect("running operation completes");
        history.observe(&operation);
        history.writer.flush();

        let reloaded = OperationHistory::load(directory.path());
        let listed = reloaded.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].state, OperationStateDto::Completed);
    }
}
