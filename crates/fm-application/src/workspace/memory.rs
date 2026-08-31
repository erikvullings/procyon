//! An in-memory [`WorkspaceRepository`] (spec §5.3.16 step 2).
//!
//! Not durable across process restarts; used by tests and by the
//! `WorkspaceService` unit tests in this crate. The persistent, file-backed
//! implementation is [`super::persistent::JsonFileWorkspaceRepository`].

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use fm_domain::{Workspace, WorkspaceId};

use super::error::WorkspaceError;
use super::repository::{LastActiveWorkspaceStore, WorkspaceRepository, WorkspaceSummary};

/// An in-memory workspace store, guarded by a [`Mutex`] so it can be shared
/// across async tasks.
#[derive(Debug, Default)]
pub struct InMemoryWorkspaceRepository {
    workspaces: Mutex<HashMap<WorkspaceId, Workspace>>,
    last_active: Mutex<Option<WorkspaceId>>,
}

impl InMemoryWorkspaceRepository {
    /// Creates an empty repository.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl WorkspaceRepository for InMemoryWorkspaceRepository {
    async fn list(&self) -> Result<Vec<WorkspaceSummary>, WorkspaceError> {
        let workspaces = self.workspaces.lock().expect("workspace lock poisoned");
        Ok(workspaces.values().map(WorkspaceSummary::from).collect())
    }

    async fn load(&self, id: WorkspaceId) -> Result<Option<Workspace>, WorkspaceError> {
        let workspaces = self.workspaces.lock().expect("workspace lock poisoned");
        Ok(workspaces.get(&id).cloned())
    }

    async fn save(
        &self,
        workspace: &Workspace,
        expected_revision: Option<u64>,
    ) -> Result<Workspace, WorkspaceError> {
        let mut workspaces = self.workspaces.lock().expect("workspace lock poisoned");
        let mut persisted = workspace.clone();

        match workspaces.get(&workspace.id) {
            Some(existing) => {
                if let Some(expected) = expected_revision
                    && expected != existing.revision
                {
                    return Err(WorkspaceError::RevisionConflict {
                        id: workspace.id,
                        expected: Some(expected),
                        actual: existing.revision,
                    });
                }
                persisted.created_at = existing.created_at;
                persisted.revision = existing.revision + 1;
            }
            None => {
                if let Some(expected) = expected_revision {
                    return Err(WorkspaceError::RevisionConflict {
                        id: workspace.id,
                        expected: Some(expected),
                        actual: 0,
                    });
                }
                persisted.revision = 1;
            }
        }
        persisted.updated_at = Utc::now();

        workspaces.insert(persisted.id, persisted.clone());
        Ok(persisted)
    }

    async fn delete(
        &self,
        id: WorkspaceId,
        expected_revision: Option<u64>,
    ) -> Result<(), WorkspaceError> {
        let mut workspaces = self.workspaces.lock().expect("workspace lock poisoned");
        match workspaces.get(&id) {
            Some(existing) => {
                if let Some(expected) = expected_revision
                    && expected != existing.revision
                {
                    return Err(WorkspaceError::RevisionConflict {
                        id,
                        expected: Some(expected),
                        actual: existing.revision,
                    });
                }
                workspaces.remove(&id);
                Ok(())
            }
            None => Err(WorkspaceError::NotFound { id }),
        }
    }
}

#[async_trait]
impl LastActiveWorkspaceStore for InMemoryWorkspaceRepository {
    async fn last_active_workspace_id(&self) -> Result<Option<WorkspaceId>, WorkspaceError> {
        Ok(*self.last_active.lock().expect("last-active lock poisoned"))
    }

    async fn set_last_active_workspace_id(
        &self,
        id: Option<WorkspaceId>,
    ) -> Result<(), WorkspaceError> {
        *self.last_active.lock().expect("last-active lock poisoned") = id;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use fm_domain::{
        ColumnConfiguration, DirectoryViewConfiguration, DirectoryViewMode, IconSize, Location,
        NavigationHistory, OperationCentrePreferences, PaneId, PaneState, ProviderId,
        SortDescriptor, SortDirection, TabId, TabState, Workspace, WorkspaceLayout,
    };

    use super::*;

    fn sample_workspace() -> Workspace {
        let pane_id = PaneId::new();
        let tab_id = TabId::new();
        let view = DirectoryViewConfiguration {
            sort: vec![SortDescriptor {
                column_id: "core.name".to_owned(),
                direction: SortDirection::Ascending,
            }],
            columns: vec![ColumnConfiguration {
                column_id: "core.name".to_owned(),
                width: 300,
                visible: true,
            }],
            show_hidden: false,
            folders_first: true,
            quick_filter: None,
            view_mode: DirectoryViewMode::Table,
            icon_size: IconSize::Medium,
        };
        let now = Utc::now();
        Workspace {
            schema_version: 1,
            id: WorkspaceId::new(),
            name: "Default".to_owned(),
            layout: WorkspaceLayout::Pane { pane_id },
            panes: vec![PaneState {
                id: pane_id,
                title: None,
                tabs: vec![TabState {
                    id: tab_id,
                    location: Location::new(ProviderId::new("file"), "file:///home"),
                    title_override: None,
                    history: NavigationHistory {
                        back: vec![],
                        forward: vec![],
                    },
                    view: view.clone(),
                    pinned: false,
                    transient: false,
                }],
                active_tab_id: tab_id,
                default_view: view,
            }],
            active_pane_id: pane_id,
            operation_centre: OperationCentrePreferences {
                visible: false,
                height: 0,
            },
            created_at: now,
            updated_at: now,
            revision: 1,
            ephemeral: false,
            forked_from: None,
        }
    }

    #[tokio::test]
    async fn save_then_load_round_trips_the_workspace() {
        let repository = InMemoryWorkspaceRepository::new();
        let workspace = sample_workspace();

        let saved = repository
            .save(&workspace, None)
            .await
            .expect("save must succeed");
        assert_eq!(saved.revision, 1);

        let loaded = repository
            .load(workspace.id)
            .await
            .expect("load must succeed")
            .expect("workspace must exist");
        assert_eq!(loaded, saved);
    }

    #[tokio::test]
    async fn save_increments_the_revision_on_every_update() {
        let repository = InMemoryWorkspaceRepository::new();
        let workspace = sample_workspace();

        let first = repository
            .save(&workspace, None)
            .await
            .expect("first save must succeed");
        assert_eq!(first.revision, 1);

        let mut updated = first.clone();
        updated.name = "Renamed".to_owned();
        let second = repository
            .save(&updated, Some(first.revision))
            .await
            .expect("second save must succeed");
        assert_eq!(second.revision, 2);
        assert_eq!(second.created_at, first.created_at);
    }

    #[tokio::test]
    async fn save_with_a_stale_expected_revision_returns_a_conflict() {
        let repository = InMemoryWorkspaceRepository::new();
        let workspace = sample_workspace();
        let first = repository
            .save(&workspace, None)
            .await
            .expect("first save must succeed");

        let mut updated = first.clone();
        updated.name = "Renamed again".to_owned();
        let error = repository
            .save(&updated, Some(first.revision + 41))
            .await
            .expect_err("stale expected_revision must be rejected");

        assert_eq!(
            error,
            WorkspaceError::RevisionConflict {
                id: workspace.id,
                expected: Some(first.revision + 41),
                actual: first.revision,
            }
        );
    }

    #[tokio::test]
    async fn delete_removes_the_workspace_and_a_second_delete_reports_not_found() {
        let repository = InMemoryWorkspaceRepository::new();
        let workspace = sample_workspace();
        let saved = repository
            .save(&workspace, None)
            .await
            .expect("save must succeed");

        repository
            .delete(saved.id, Some(saved.revision))
            .await
            .expect("delete must succeed");

        assert_eq!(
            repository.load(saved.id).await.expect("load must succeed"),
            None
        );
        assert_eq!(
            repository.delete(saved.id, None).await.unwrap_err(),
            WorkspaceError::NotFound { id: saved.id }
        );
    }

    #[tokio::test]
    async fn last_active_workspace_id_defaults_to_none_and_can_be_set() {
        let repository = InMemoryWorkspaceRepository::new();
        assert_eq!(repository.last_active_workspace_id().await.unwrap(), None);

        let id = WorkspaceId::new();
        repository
            .set_last_active_workspace_id(Some(id))
            .await
            .unwrap();
        assert_eq!(
            repository.last_active_workspace_id().await.unwrap(),
            Some(id)
        );
    }
}
