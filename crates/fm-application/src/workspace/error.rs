//! Errors returned by the workspace repository, migration and service layers
//! (spec §5.3.6, §5.3.8, task 0079).

use std::path::PathBuf;

use fm_domain::{WorkspaceId, WorkspaceValidationError};

/// Failure modes for workspace persistence, validation and lifecycle
/// operations. Hosts translate these into transport-specific responses; no
/// variant leaks a raw OS or serde error message beyond a plain string.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum WorkspaceError {
    /// No workspace exists with the given id.
    #[error("workspace {id} not found")]
    NotFound {
        /// The id that was looked up.
        id: WorkspaceId,
    },
    /// A `save`/`delete` call's `expected_revision` no longer matches the
    /// stored revision (spec §5.3.6 invariant 14, §5.3.10).
    #[error("workspace {id} revision conflict: expected {expected:?}, actual {actual}")]
    RevisionConflict {
        /// The workspace whose revision changed.
        id: WorkspaceId,
        /// The revision the caller last observed, if any.
        expected: Option<u64>,
        /// The revision actually stored (0 if the workspace does not exist).
        actual: u64,
    },
    /// The workspace failed one or more structural invariants (spec §5.3.6).
    #[error("workspace failed validation: {0:?}")]
    Invalid(Vec<WorkspaceValidationError>),
    /// The stored `schema_version` is newer than this build understands and
    /// has no migration path.
    #[error("workspace schema version {schema_version} is not supported")]
    UnsupportedSchemaVersion {
        /// The unsupported version.
        schema_version: u32,
    },
    /// The stored data for a workspace could not be parsed. It has been
    /// backed up rather than discarded or silently repaired.
    #[error("workspace data for {id} is corrupt; backed up to {}", backup_path.display())]
    Corrupt {
        /// The corrupt file's stem (its would-be workspace id, if parseable).
        id: String,
        /// Where the unreadable file was moved to.
        backup_path: PathBuf,
    },
    /// A filesystem operation failed.
    #[error("workspace storage I/O error: {0}")]
    Io(String),
    /// (De)serializing workspace JSON failed outside of a recognised
    /// corruption case (for example encoding a value back to disk).
    #[error("workspace serialization error: {0}")]
    Serialization(String),
    /// A [`fm_domain::WorkspaceCommand`] targeted a pane that does not exist
    /// in the workspace (task 0080).
    #[error("pane {pane_id} does not exist in workspace {workspace_id}")]
    PaneNotFound {
        /// The workspace the command targeted.
        workspace_id: WorkspaceId,
        /// The dangling pane id.
        pane_id: fm_domain::PaneId,
    },
    /// A [`fm_domain::WorkspaceCommand`] targeted a tab that does not exist
    /// in the given pane (task 0080).
    #[error("tab {tab_id} does not exist in pane {pane_id} of workspace {workspace_id}")]
    TabNotFound {
        /// The workspace the command targeted.
        workspace_id: WorkspaceId,
        /// The pane the command targeted.
        pane_id: fm_domain::PaneId,
        /// The dangling tab id.
        tab_id: fm_domain::TabId,
    },
    /// A command's own fields failed validation, distinct from a stored
    /// workspace's structural invariants (spec §5.3.6), for example an empty
    /// `RenameWorkspace` name (task 0080).
    #[error("invalid command: {0}")]
    InvalidCommand(String),
    /// An explicitly requested workspace name (via `create` or
    /// `RenameWorkspace`) collides, case-insensitively and trimmed, with an
    /// existing workspace's name - workspaces must stay distinguishable in
    /// the switcher and the native Window menu.
    #[error("a workspace named {name:?} already exists")]
    DuplicateName {
        /// The name that collided.
        name: String,
    },
    /// [`super::service::WorkspaceService::resync`] was called with a workspace id
    /// that is not an ephemeral (per-window) workspace - only ephemeral workspaces
    /// can be resynced into a named workspace.
    #[error("workspace {id} is not an ephemeral workspace")]
    NotEphemeral {
        /// The workspace that was requested to be resynced.
        id: WorkspaceId,
    },
    /// [`super::service::WorkspaceService::resync`] was given an explicit `target_id` that
    /// names an ephemeral (per-window) workspace - only a named workspace can be a resync
    /// target, since ephemeral workspaces are private, disposable per-window sessions.
    #[error("workspace {id} is ephemeral and cannot be a resync target")]
    TargetIsEphemeral {
        /// The ephemeral workspace that was requested as a resync target.
        id: WorkspaceId,
    },
}
