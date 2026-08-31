use chrono::{DateTime, Utc};
use fm_domain::{EntryKind, Location, OperationId};
use fm_vfs::EntryRef;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A backend-owned mutating filesystem job (specification §17).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    /// Stable job identifier.
    pub id: OperationId,
    /// Semantic mutation requested by the caller.
    pub kind: OperationKind,
    /// Current lifecycle state.
    pub state: OperationState,
    /// Entries affected by the mutation.
    pub sources: Vec<EntryRef>,
    /// Destination, when the operation has one.
    pub destination: Option<Location>,
    /// Current counters and rate.
    pub progress: OperationProgress,
    /// Conflict behavior selected by the caller.
    pub conflict_policy: ConflictPolicy,
    /// Time the job was accepted.
    pub created_at: DateTime<Utc>,
    /// Time planning began.
    pub started_at: Option<DateTime<Utc>>,
    /// Time a terminal state was reached.
    pub completed_at: Option<DateTime<Utc>>,
    /// Entry-scoped failures that did not abort the whole operation.
    pub errors: Vec<OperationEntryError>,
    /// Persisted evidence and lifecycle for safely reversing this operation.
    #[serde(default)]
    pub undo: OperationUndo,
    /// Original operation reversed by this job, for audit/history linkage.
    #[serde(default)]
    pub undo_of: Option<OperationId>,
}

impl Operation {
    /// Creates a queued operation with no reported work.
    #[must_use]
    pub fn new(
        kind: OperationKind,
        sources: Vec<EntryRef>,
        destination: Option<Location>,
        conflict_policy: ConflictPolicy,
    ) -> Self {
        Self {
            id: OperationId::new(),
            kind,
            state: OperationState::Queued,
            sources,
            destination,
            progress: OperationProgress::default(),
            conflict_policy,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            errors: Vec::new(),
            undo: OperationUndo::default(),
            undo_of: None,
        }
    }

    /// Applies a legal lifecycle transition and timestamps lifecycle edges.
    pub fn transition(&mut self, next: OperationState) -> Result<(), TransitionError> {
        if !self.state.can_transition_to(next) {
            return Err(TransitionError {
                from: self.state,
                to: next,
            });
        }

        self.state = next;
        let now = Utc::now();
        if next == OperationState::Planning && self.started_at.is_none() {
            self.started_at = Some(now);
        }

        if next.is_terminal() {
            self.completed_at = Some(now);
        }
        Ok(())
    }
}

/// A conservative identity/revision snapshot used before applying an inverse mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryFingerprint {
    /// Stable provider identity observed after the original operation.
    pub entry: EntryRef,
    /// Whether repeated provider inspection proved the entry id stable.
    #[serde(default)]
    pub stable_id: bool,
    /// Entry kind, which must remain unchanged.
    pub kind: EntryKind,
    /// Logical size reported by the provider.
    pub size: Option<u64>,
    /// Last modification timestamp reported by the provider.
    pub modified_at: Option<DateTime<Utc>>,
    /// BLAKE3 content hash used when the provider has no stable identity.
    #[serde(default)]
    pub content_hash: Option<String>,
}

/// One inverse mutation retained with a completed operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum UndoAction {
    /// Move an unchanged entry back to its original location.
    MoveBack {
        /// Location occupied before the original operation.
        original: Location,
        /// Fingerprint observed at the original location before mutation.
        original_fingerprint: EntryFingerprint,
        /// Fingerprint at the location produced by the original operation.
        current: EntryFingerprint,
    },
    /// Copy an unchanged entry tree back across providers, then remove the moved output.
    CrossProviderMoveBack {
        /// Location occupied before the original operation.
        original: Location,
        /// Root fingerprint observed before the original operation.
        original_fingerprint: EntryFingerprint,
        /// Root at the destination produced by the original operation.
        current_root: EntryFingerprint,
        /// Every output entry, used to reject modifications anywhere in the tree.
        current_entries: Vec<EntryFingerprint>,
    },
    /// Remove entries created by copy/duplicate, after verifying every fingerprint.
    RemoveCreated {
        /// Created entries, including descendants, in parent-before-child order.
        entries: Vec<EntryFingerprint>,
    },
    /// Restore a platform-trash entry to its recorded original location.
    RestoreTrash {
        /// Location occupied before trashing.
        original: Location,
        /// Fingerprint at the native trash location returned by the platform.
        trashed: EntryFingerprint,
    },
}

/// Explicit inverse plan persisted with operation history.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoPlan {
    /// Ordered inverse mutations.
    pub actions: Vec<UndoAction>,
}

/// Persisted undo evidence and whether it has already been consumed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationUndo {
    /// Safe inverse plan, absent when the operation cannot be reversed.
    pub plan: Option<UndoPlan>,
    /// Stable user-facing explanation when no plan can be offered.
    pub unavailable_reason: Option<String>,
    /// Undo job currently reserved for this operation.
    pub pending_operation: Option<OperationId>,
    /// Successfully completed undo job.
    pub undone_by: Option<OperationId>,
}

impl Default for OperationUndo {
    fn default() -> Self {
        Self {
            plan: None,
            unavailable_reason: Some(
                "Undo is available only after the operation completes.".into(),
            ),
            pending_operation: None,
            undone_by: None,
        }
    }
}

impl OperationUndo {
    /// Records a safe persisted inverse plan.
    #[must_use]
    pub fn available(plan: UndoPlan) -> Self {
        Self {
            plan: Some(plan),
            unavailable_reason: None,
            pending_operation: None,
            undone_by: None,
        }
    }

    /// Records why no safe inverse can be offered.
    #[must_use]
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            plan: None,
            unavailable_reason: Some(reason.into()),
            pending_operation: None,
            undone_by: None,
        }
    }
}

/// One non-fatal per-entry operation failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationEntryError {
    /// Entry that could not be processed.
    pub entry: EntryRef,
    /// Sanitized provider/application error.
    pub message: String,
}

/// Operation lifecycle states from specification §17.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationState {
    /// Waiting for a scheduler permit.
    Queued,
    /// Enumerating work and computing totals.
    Planning,
    /// Performing mutations.
    Running,
    /// Cooperatively paused.
    Paused,
    /// Waiting for a user confirmation or conflict decision.
    WaitingForConflictResolution,
    /// Cancellation requested; finishing the current safe unit.
    Cancelling,
    /// Cancelled without a partial destination file.
    Cancelled,
    /// Finished successfully.
    Completed,
    /// Finished with non-fatal warnings.
    CompletedWithWarnings,
    /// Finished unsuccessfully.
    Failed,
    /// Recovered after the backend stopped before a terminal transition.
    Interrupted,
}

impl OperationState {
    /// Whether this state may legally advance to `next`.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        use OperationState::{
            Cancelled, Cancelling, Completed, CompletedWithWarnings, Failed, Paused, Planning,
            Queued, Running, WaitingForConflictResolution,
        };
        matches!(
            (self, next),
            (Queued, Planning | Cancelling | Failed)
                | (
                    Planning,
                    Running | WaitingForConflictResolution | Cancelling | Failed
                )
                | (
                    Running,
                    Paused
                        | WaitingForConflictResolution
                        | Cancelling
                        | Completed
                        | CompletedWithWarnings
                        | Failed
                )
                | (Paused, Running | Cancelling | Failed)
                | (WaitingForConflictResolution, Running | Cancelling | Failed)
                | (Cancelling, Cancelled | Failed)
        )
    }

    /// Whether no further transition is permitted.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Cancelled
                | Self::Completed
                | Self::CompletedWithWarnings
                | Self::Failed
                | Self::Interrupted
        )
    }
}

/// Initial operation kinds from specification §17.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    /// Package selected entries into an archive.
    CreateArchive,
    /// Package selected entries into an archive and remove the originals on success.
    MoveToArchive,
    /// Create a directory.
    CreateDirectory,
    /// Create an empty file.
    CreateFile,
    /// Rename one entry.
    Rename,
    /// Copy entries.
    Copy,
    /// Move entries.
    Move,
    /// Duplicate entries.
    Duplicate,
    /// Send entries to trash.
    Trash,
    /// Permanently delete entries.
    Delete,
    /// Safely reverse a completed operation.
    Undo,
}

/// Observable progress for one operation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OperationProgress {
    /// Completed plan items.
    pub completed_items: u64,
    /// Planned item count, if feasible to compute.
    pub total_items: Option<u64>,
    /// Completed payload bytes.
    pub completed_bytes: u64,
    /// Planned payload bytes, if feasible to compute.
    pub total_bytes: Option<u64>,
    /// Entry currently being processed.
    pub current_entry: Option<EntryRef>,
    /// Exponentially smoothed transfer rate.
    pub bytes_per_second: Option<u64>,
}

/// Conflict policies from specification §17.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    /// Ask the user.
    #[default]
    Ask,
    /// Skip a conflicting source.
    Skip,
    /// Replace an existing destination of the same entry kind.
    Overwrite,
    /// Generate a non-conflicting destination name.
    RenameNew,
    /// Keep whichever entry is newer.
    KeepNewer,
}

/// Metadata shown to a user before choosing how to resolve a collision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictEntry {
    /// Display name of the entry.
    pub name: String,
    /// Entry kind used to reject unsafe cross-kind replacement.
    pub kind: EntryKind,
    /// Logical size in bytes, when the provider reports it.
    pub size: Option<u64>,
    /// Last modification time, when the provider reports it.
    pub modified_at: Option<DateTime<Utc>>,
}

/// One item-scoped collision awaiting an engine-owned decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationConflict {
    /// Stable identifier retained while the item is pending.
    pub id: String,
    /// Source metadata.
    pub source: ConflictEntry,
    /// Existing destination metadata.
    pub destination: ConflictEntry,
}

/// A user-selectable resolution passed back to the conflicting executor item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Skip this source item.
    Skip,
    /// Replace the same-kind destination.
    Overwrite,
    /// Choose a duplicate-style destination name.
    RenameNew,
}

/// A rejected operation lifecycle transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("illegal operation state transition from {from:?} to {to:?}")]
pub struct TransitionError {
    /// Current state.
    pub from: OperationState,
    /// Requested state.
    pub to: OperationState,
}
