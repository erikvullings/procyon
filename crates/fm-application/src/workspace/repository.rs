//! The workspace persistence abstraction (spec §5.3.8).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fm_domain::{Workspace, WorkspaceId};

use super::error::WorkspaceError;

/// A lightweight projection of a stored workspace, cheap enough to list
/// without loading every pane and tab (spec §5.3.8's `list`).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceSummary {
    /// The workspace's id.
    pub id: WorkspaceId,
    /// The workspace's user-facing name.
    pub name: String,
    /// When the workspace was last persisted.
    pub updated_at: DateTime<Utc>,
    /// The workspace's current revision.
    pub revision: u64,
    /// True for a per-window fork, excluded from the workspace switcher.
    pub ephemeral: bool,
}

impl From<&Workspace> for WorkspaceSummary {
    fn from(workspace: &Workspace) -> Self {
        Self {
            id: workspace.id,
            name: workspace.name.clone(),
            updated_at: workspace.updated_at,
            revision: workspace.revision,
            ephemeral: workspace.ephemeral,
        }
    }
}

/// Persists and retrieves whole [`Workspace`] definitions (spec §5.3.8).
///
/// Every method returns a typed [`WorkspaceError`] rather than panicking or
/// leaking a storage-specific error type. `save` and `delete` enforce an
/// optimistic revision check whenever `expected_revision` is `Some` (spec
/// §5.3.6 invariant 14, §5.3.10); `save` always returns the workspace with
/// `revision` incremented from whatever was previously stored (1 for a new
/// workspace) and `updated_at` refreshed, regardless of what the caller
/// passed in.
#[async_trait]
pub trait WorkspaceRepository: Send + Sync {
    /// Lists every stored workspace as a lightweight summary.
    async fn list(&self) -> Result<Vec<WorkspaceSummary>, WorkspaceError>;

    /// Loads a full workspace definition, or `None` if it does not exist.
    async fn load(&self, id: WorkspaceId) -> Result<Option<Workspace>, WorkspaceError>;

    /// Persists a workspace, returning the persisted copy.
    async fn save(
        &self,
        workspace: &Workspace,
        expected_revision: Option<u64>,
    ) -> Result<Workspace, WorkspaceError>;

    /// Deletes a workspace, enforcing the same optimistic revision check as
    /// [`WorkspaceRepository::save`].
    async fn delete(
        &self,
        id: WorkspaceId,
        expected_revision: Option<u64>,
    ) -> Result<(), WorkspaceError>;
}

/// Tracks which workspace was last active, so startup can reselect it (spec
/// §5.3.7).
///
/// Kept as a separate trait from [`WorkspaceRepository`] because the
/// specification's own repository trait (§5.3.8) does not define this
/// concern, but the startup lifecycle still needs somewhere durable to store
/// it; every [`WorkspaceRepository`] implementation in this crate implements
/// both.
#[async_trait]
pub trait LastActiveWorkspaceStore: Send + Sync {
    /// Returns the last-active workspace id, if one has been recorded.
    async fn last_active_workspace_id(&self) -> Result<Option<WorkspaceId>, WorkspaceError>;

    /// Records the last-active workspace id.
    async fn set_last_active_workspace_id(
        &self,
        id: Option<WorkspaceId>,
    ) -> Result<(), WorkspaceError>;
}
