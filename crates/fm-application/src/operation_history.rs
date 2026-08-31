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
use std::sync::{Arc, Mutex};

use fm_domain::OperationId;
use fm_operations::{Operation, OperationSnapshotObserver};
use fm_transport_dto::{
    EntryRefDto, OperationConflictPolicyDto, OperationDto, OperationEntryErrorDto,
    OperationKindDto, OperationProgressDto, OperationStateDto,
};

use crate::DirectoryService;

pub(crate) const OPERATION_HISTORY_FILE_NAME: &str = "operation-history.json";
const OPERATION_HISTORY_MAX_ENTRIES: usize = 100;
const OPERATION_HISTORY_MAX_AGE_DAYS: i64 = 30;

/// Crash-safe operation snapshots stored beside settings.
pub(crate) struct OperationHistory {
    path: PathBuf,
    operations: Mutex<Vec<Operation>>,
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
            if !operation.state.is_terminal() {
                operation.state = fm_operations::OperationState::Interrupted;
                operation.completed_at = Some(now);
            }
        }
        let history = Self {
            path,
            operations: Mutex::new(operations),
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
        let Ok(bytes) = serde_json::to_vec_pretty(&*operations) else {
            return;
        };
        let Some(directory) = self.path.parent() else {
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
                file.write_all(&bytes)?;
                file.sync_all()
            })
            .is_ok()
        {
            let _ = fs::rename(temporary, &self.path);
        }
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
            } else {
                operations.push(operation.clone());
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
                directories.refresh_affected(&affected).await;
            });
        }
    }
}

pub(crate) fn operation_dto(operation: Operation, queue_position: Option<u64>) -> OperationDto {
    let result_summary = operation_result_summary(&operation);
    OperationDto {
        id: operation.id.into(),
        operation_type: match operation.kind {
            fm_operations::OperationKind::CreateArchive => OperationKindDto::CreateArchive,
            fm_operations::OperationKind::MoveToArchive => OperationKindDto::MoveToArchive,
            fm_operations::OperationKind::CreateDirectory => OperationKindDto::CreateDirectory,
            fm_operations::OperationKind::CreateFile => OperationKindDto::CreateFile,
            fm_operations::OperationKind::Rename => OperationKindDto::Rename,
            fm_operations::OperationKind::Copy => OperationKindDto::Copy,
            fm_operations::OperationKind::Move => OperationKindDto::Move,
            fm_operations::OperationKind::Duplicate => OperationKindDto::Duplicate,
            fm_operations::OperationKind::Trash => OperationKindDto::Trash,
            fm_operations::OperationKind::Delete => OperationKindDto::Delete,
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
            operation.errors.len()
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
