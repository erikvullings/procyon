//! Maps workspace lifecycle transitions and applied [`WorkspaceCommand`]s
//! onto the focused `workspace.*` event payloads named by spec §5.3.11, for
//! the injectable [`super::publisher::WorkspaceCommandPublisher`] seam (task
//! 0081).

use fm_domain::{
    ColumnConfiguration, DirectoryViewConfiguration, Location, PersistedFilter, SortDescriptor,
    SortDirection, SplitAxis, Workspace, WorkspaceCommand, WorkspaceLayout,
};
use fm_events::{
    BackendEventPayload, ColumnConfigurationPayload, DirectoryViewConfigurationPayload,
    LocationPayload, PersistedFilterPayload, SortDescriptorPayload, SortDirectionPayload,
    SplitDirectionPayload, WorkspaceLayoutPayload,
};

use super::command::{find_pane, find_tab};
use super::error::WorkspaceError;

/// Builds the event for a newly created workspace (spec §5.3.7).
pub(crate) fn workspace_created(workspace: &Workspace) -> BackendEventPayload {
    BackendEventPayload::WorkspaceCreated {
        revision: workspace.revision,
    }
}

/// Builds the event for a workspace becoming the active workspace, whether
/// at startup or through an explicit `openWorkspace` request (spec §5.3.7,
/// §5.3.12).
pub(crate) fn workspace_opened(workspace: &Workspace) -> BackendEventPayload {
    BackendEventPayload::WorkspaceOpened {
        revision: workspace.revision,
    }
}

/// Builds the event for a workspace ceasing to be the active workspace
/// (spec §5.3.7's workspace-switching lifecycle).
pub(crate) fn workspace_closed(revision: u64) -> BackendEventPayload {
    BackendEventPayload::WorkspaceClosed { revision }
}

/// Builds the event for a deleted workspace.
pub(crate) fn workspace_deleted(revision: u64) -> BackendEventPayload {
    BackendEventPayload::WorkspaceDeleted { revision }
}

/// Builds the focused event for an applied [`WorkspaceCommand`], reading
/// back any post-mutation state (a tab's new location or view) from
/// `persisted` since `command` itself only carries the caller's request.
pub(crate) fn command_event(
    command: &WorkspaceCommand,
    persisted: &Workspace,
) -> Result<BackendEventPayload, WorkspaceError> {
    let revision = persisted.revision;
    let event = match command {
        WorkspaceCommand::RenameWorkspace { .. } => BackendEventPayload::WorkspaceRenamed {
            revision,
            name: persisted.name.clone(),
        },
        WorkspaceCommand::SetActivePane { pane_id, .. } => {
            BackendEventPayload::WorkspaceActivePaneChanged {
                revision,
                pane_id: *pane_id,
            }
        }
        WorkspaceCommand::AddTab { pane_id, .. }
        | WorkspaceCommand::AddTransientTab { pane_id, .. } => {
            let pane = find_pane(persisted, *pane_id)?;
            let tab = find_tab(pane, persisted.id, pane.active_tab_id)?;
            BackendEventPayload::WorkspaceTabAdded {
                revision,
                pane_id: *pane_id,
                tab_id: tab.id,
                location: location_payload(&tab.location),
            }
        }
        WorkspaceCommand::CloseTab {
            pane_id, tab_id, ..
        } => BackendEventPayload::WorkspaceTabClosed {
            revision,
            pane_id: *pane_id,
            tab_id: *tab_id,
        },
        WorkspaceCommand::MoveTab {
            target_pane_id,
            tab_id,
            ..
        } => BackendEventPayload::WorkspaceTabActivated {
            revision,
            pane_id: *target_pane_id,
            tab_id: *tab_id,
        },
        WorkspaceCommand::ActivateTab {
            pane_id, tab_id, ..
        } => BackendEventPayload::WorkspaceTabActivated {
            revision,
            pane_id: *pane_id,
            tab_id: *tab_id,
        },
        WorkspaceCommand::NavigateTab {
            pane_id, tab_id, ..
        } => {
            let pane = find_pane(persisted, *pane_id)?;
            let tab = find_tab(pane, persisted.id, *tab_id)?;
            BackendEventPayload::WorkspaceTabNavigated {
                revision,
                pane_id: *pane_id,
                tab_id: *tab_id,
                location: location_payload(&tab.location),
            }
        }
        WorkspaceCommand::UpdateView {
            pane_id, tab_id, ..
        } => {
            let pane = find_pane(persisted, *pane_id)?;
            let tab = find_tab(pane, persisted.id, *tab_id)?;
            BackendEventPayload::WorkspaceTabViewChanged {
                revision,
                pane_id: *pane_id,
                tab_id: *tab_id,
                view: view_payload(&tab.view),
            }
        }
        WorkspaceCommand::UpdateLayout { .. } => BackendEventPayload::WorkspaceLayoutChanged {
            revision,
            layout: layout_payload(&persisted.layout),
        },
    };

    Ok(event)
}

fn location_payload(location: &Location) -> LocationPayload {
    LocationPayload {
        provider_id: location.provider_id.clone(),
        uri: location.uri.clone(),
    }
}

fn view_payload(view: &DirectoryViewConfiguration) -> DirectoryViewConfigurationPayload {
    DirectoryViewConfigurationPayload {
        sort: view.sort.iter().map(sort_descriptor_payload).collect(),
        columns: view.columns.iter().map(column_payload).collect(),
        show_hidden: view.show_hidden,
        folders_first: view.folders_first,
        quick_filter: view.quick_filter.as_ref().map(persisted_filter_payload),
    }
}

fn sort_descriptor_payload(sort: &SortDescriptor) -> SortDescriptorPayload {
    SortDescriptorPayload {
        column_id: sort.column_id.clone(),
        direction: match sort.direction {
            SortDirection::Ascending => SortDirectionPayload::Ascending,
            SortDirection::Descending => SortDirectionPayload::Descending,
        },
    }
}

fn column_payload(column: &ColumnConfiguration) -> ColumnConfigurationPayload {
    ColumnConfigurationPayload {
        column_id: column.column_id.clone(),
        width: column.width,
        visible: column.visible,
    }
}

fn persisted_filter_payload(filter: &PersistedFilter) -> PersistedFilterPayload {
    PersistedFilterPayload {
        query: filter.query.clone(),
    }
}

fn layout_payload(layout: &WorkspaceLayout) -> WorkspaceLayoutPayload {
    match layout {
        WorkspaceLayout::Pane { pane_id } => WorkspaceLayoutPayload::Pane { pane_id: *pane_id },
        WorkspaceLayout::Split {
            axis,
            ratio,
            first,
            second,
        } => WorkspaceLayoutPayload::Split {
            direction: match axis {
                SplitAxis::Horizontal => SplitDirectionPayload::Horizontal,
                SplitAxis::Vertical => SplitDirectionPayload::Vertical,
            },
            ratio: *ratio,
            first: Box::new(layout_payload(first)),
            second: Box::new(layout_payload(second)),
        },
    }
}
