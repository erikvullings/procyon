//! Applies a [`WorkspaceCommand`] to a loaded [`Workspace`] (spec §5.3.9
//! steps 2-3: validate the command, apply the mutation).
//!
//! Revision verification (step 1), persistence and revision increment (steps
//! 4-5) are `WorkspaceService::apply_command`'s concern; this module only
//! knows how to mutate an in-memory `Workspace`, so the mutation logic can be
//! unit-tested without a repository.

use std::path::Path;

use fm_domain::{
    DirectoryViewConfiguration, DirectoryViewPatch, Location, MAX_NAVIGATION_HISTORY_LEN,
    NavigationHistory, NavigationMode, PaneId, PaneState, QuickFilterPatch, TabId, TabState,
    Workspace, WorkspaceCommand,
};

use super::default_workspace::{build_tab, location_for};
use super::error::WorkspaceError;

/// Applies `command`'s mutation to `workspace` in place, then re-validates
/// every structural invariant (spec §5.3.6) as a final safety net.
///
/// `home_directory` builds the replacement when closing or moving a pane's
/// last tab (spec §5.3.4).
pub(crate) fn apply(
    workspace: &mut Workspace,
    command: WorkspaceCommand,
    home_directory: &Path,
) -> Result<(), WorkspaceError> {
    match command {
        WorkspaceCommand::RenameWorkspace { name, .. } => {
            if name.trim().is_empty() {
                return Err(WorkspaceError::InvalidCommand(
                    "workspace name must not be empty".to_owned(),
                ));
            }
            workspace.name = name;
        }
        WorkspaceCommand::SetActivePane { pane_id, .. } => {
            find_pane(workspace, pane_id)?;
            workspace.active_pane_id = pane_id;
        }
        WorkspaceCommand::AddTab {
            pane_id, location, ..
        } => {
            let pane = find_pane_mut(workspace, pane_id)?;
            let mut tab = build_tab(location);
            tab.view = pane.default_view.clone();
            pane.active_tab_id = tab.id;
            pane.tabs.push(tab);
        }
        WorkspaceCommand::AddTransientTab {
            pane_id, location, ..
        } => {
            let pane = find_pane_mut(workspace, pane_id)?;
            let mut tab = build_tab(location);
            tab.view = pane.default_view.clone();
            tab.transient = true;
            pane.active_tab_id = tab.id;
            pane.tabs.push(tab);
        }
        WorkspaceCommand::CloseTab {
            pane_id, tab_id, ..
        } => close_tab(workspace, pane_id, tab_id, home_directory)?,
        WorkspaceCommand::MoveTab {
            source_pane_id,
            tab_id,
            target_pane_id,
            target_index,
            ..
        } => move_tab(
            workspace,
            source_pane_id,
            tab_id,
            target_pane_id,
            target_index,
            home_directory,
        )?,
        WorkspaceCommand::ActivateTab {
            pane_id, tab_id, ..
        } => {
            workspace.active_pane_id = pane_id;
            let workspace_id = workspace.id;
            let pane = find_pane_mut(workspace, pane_id)?;
            if !pane.tabs.iter().any(|tab| tab.id == tab_id) {
                return Err(WorkspaceError::TabNotFound {
                    workspace_id,
                    pane_id,
                    tab_id,
                });
            }
            pane.active_tab_id = tab_id;
        }
        WorkspaceCommand::NavigateTab {
            pane_id,
            tab_id,
            location,
            navigation_mode,
            ..
        } => {
            let tab = find_tab_mut(workspace, pane_id, tab_id)?;
            navigate(tab, location, navigation_mode)?;
        }
        WorkspaceCommand::UpdateView {
            pane_id,
            tab_id,
            patch,
            ..
        } => {
            let tab = find_tab_mut(workspace, pane_id, tab_id)?;
            apply_view_patch(&mut tab.view, patch);
        }
        WorkspaceCommand::UpdateLayout { layout, .. } => {
            workspace.layout = layout;
        }
    }

    workspace.validate().map_err(WorkspaceError::Invalid)
}

pub(super) fn find_pane(
    workspace: &Workspace,
    pane_id: PaneId,
) -> Result<&PaneState, WorkspaceError> {
    workspace
        .panes
        .iter()
        .find(|pane| pane.id == pane_id)
        .ok_or(WorkspaceError::PaneNotFound {
            workspace_id: workspace.id,
            pane_id,
        })
}

fn find_pane_mut(
    workspace: &mut Workspace,
    pane_id: PaneId,
) -> Result<&mut PaneState, WorkspaceError> {
    let workspace_id = workspace.id;
    workspace
        .panes
        .iter_mut()
        .find(|pane| pane.id == pane_id)
        .ok_or(WorkspaceError::PaneNotFound {
            workspace_id,
            pane_id,
        })
}

fn find_tab_mut(
    workspace: &mut Workspace,
    pane_id: PaneId,
    tab_id: TabId,
) -> Result<&mut TabState, WorkspaceError> {
    let workspace_id = workspace.id;
    let pane = find_pane_mut(workspace, pane_id)?;
    pane.tabs
        .iter_mut()
        .find(|tab| tab.id == tab_id)
        .ok_or(WorkspaceError::TabNotFound {
            workspace_id,
            pane_id,
            tab_id,
        })
}

/// Immutable counterpart to [`find_tab_mut`], used to read back a tab's
/// post-mutation state when building an event payload.
pub(super) fn find_tab(
    pane: &PaneState,
    workspace_id: fm_domain::WorkspaceId,
    tab_id: TabId,
) -> Result<&TabState, WorkspaceError> {
    pane.tabs
        .iter()
        .find(|tab| tab.id == tab_id)
        .ok_or(WorkspaceError::TabNotFound {
            workspace_id,
            pane_id: pane.id,
            tab_id,
        })
}

/// Closes a tab, replacing it with a fresh tab at the home directory if it
/// was the pane's last tab (spec §5.3.4), rather than leaving an invalid
/// empty pane.
fn close_tab(
    workspace: &mut Workspace,
    pane_id: PaneId,
    tab_id: TabId,
    home_directory: &Path,
) -> Result<(), WorkspaceError> {
    let workspace_id = workspace.id;
    let pane = find_pane_mut(workspace, pane_id)?;
    let position =
        pane.tabs
            .iter()
            .position(|tab| tab.id == tab_id)
            .ok_or(WorkspaceError::TabNotFound {
                workspace_id,
                pane_id,
                tab_id,
            })?;

    let was_active = pane.active_tab_id == tab_id;
    pane.tabs.remove(position);

    if pane.tabs.is_empty() {
        let mut replacement = build_tab(location_for(home_directory));
        replacement.view = pane.default_view.clone();
        pane.active_tab_id = replacement.id;
        pane.tabs.push(replacement);
    } else if was_active {
        // Activates the tab that was immediately before the closed one, or
        // the new first tab if the closed tab was the first (judgment call,
        // not specified by spec §5.3.9 — see task 0080's Agent Notes).
        let next_active_index = position.saturating_sub(1);
        pane.active_tab_id = pane.tabs[next_active_index].id;
    }

    Ok(())
}

fn move_tab(
    workspace: &mut Workspace,
    source_pane_id: PaneId,
    tab_id: TabId,
    target_pane_id: PaneId,
    target_index: usize,
    home_directory: &Path,
) -> Result<(), WorkspaceError> {
    let workspace_id = workspace.id;
    find_pane(workspace, target_pane_id)?;
    let source = find_pane_mut(workspace, source_pane_id)?;
    let source_index =
        source
            .tabs
            .iter()
            .position(|tab| tab.id == tab_id)
            .ok_or(WorkspaceError::TabNotFound {
                workspace_id,
                pane_id: source_pane_id,
                tab_id,
            })?;

    if source_pane_id == target_pane_id {
        let tab = source.tabs.remove(source_index);
        source.tabs.insert(target_index.min(source.tabs.len()), tab);
        return Ok(());
    }

    let tab = source.tabs.remove(source_index);
    if source.tabs.is_empty() {
        let mut replacement = build_tab(location_for(home_directory));
        replacement.view = source.default_view.clone();
        source.active_tab_id = replacement.id;
        source.tabs.push(replacement);
    } else if source.active_tab_id == tab_id {
        source.active_tab_id = source.tabs[source_index.min(source.tabs.len() - 1)].id;
    }

    let target = find_pane_mut(workspace, target_pane_id)?;
    target.tabs.insert(target_index.min(target.tabs.len()), tab);
    target.active_tab_id = tab_id;
    workspace.active_pane_id = target_pane_id;
    Ok(())
}

/// Mutates a tab's location and navigation history per [`NavigationMode`]
/// (inferred rules, task 0080's Agent Notes — not literally specified).
fn navigate(
    tab: &mut TabState,
    location: Option<Location>,
    mode: NavigationMode,
) -> Result<(), WorkspaceError> {
    match mode {
        NavigationMode::Push => {
            let location = location.ok_or_else(|| {
                WorkspaceError::InvalidCommand(
                    "push navigation requires an explicit location".to_owned(),
                )
            })?;
            if tab.location != location {
                push_history_entry(&mut tab.history.back, tab.location.clone());
            }
            tab.history.forward.clear();
            tab.location = location;
            enforce_history_bound(&mut tab.history);
        }
        NavigationMode::Back => {
            if let Some(target) = tab.history.back.pop() {
                push_history_entry(&mut tab.history.forward, tab.location.clone());
                tab.location = target;
                enforce_history_bound(&mut tab.history);
            }
        }
        NavigationMode::Forward => {
            if let Some(target) = tab.history.forward.pop() {
                push_history_entry(&mut tab.history.back, tab.location.clone());
                tab.location = target;
                enforce_history_bound(&mut tab.history);
            }
        }
        NavigationMode::Refresh => {
            tab.location = location.ok_or_else(|| {
                WorkspaceError::InvalidCommand(
                    "refresh navigation requires an explicit location".to_owned(),
                )
            })?;
        }
    }
    Ok(())
}

/// Pushes `location` onto `stack`, unless it would create a consecutive
/// duplicate (spec §5.3.4: "consecutive duplicate locations are removed").
fn push_history_entry(stack: &mut Vec<Location>, location: Location) {
    if stack.last() != Some(&location) {
        stack.push(location);
    }
}

/// Trims the oldest history entries until the combined `back`/`forward`
/// length is within [`MAX_NAVIGATION_HISTORY_LEN`] (spec §5.3.6 invariant 10).
fn enforce_history_bound(history: &mut NavigationHistory) {
    while history.back.len() + history.forward.len() > MAX_NAVIGATION_HISTORY_LEN {
        if !history.back.is_empty() {
            history.back.remove(0);
        } else if !history.forward.is_empty() {
            history.forward.remove(0);
        } else {
            break;
        }
    }
}

fn apply_view_patch(view: &mut DirectoryViewConfiguration, patch: DirectoryViewPatch) {
    if let Some(sort) = patch.sort {
        view.sort = sort;
    }
    if let Some(columns) = patch.columns {
        view.columns = columns;
    }
    if let Some(show_hidden) = patch.show_hidden {
        view.show_hidden = show_hidden;
    }
    if let Some(folders_first) = patch.folders_first {
        view.folders_first = folders_first;
    }
    if let Some(quick_filter) = patch.quick_filter {
        view.quick_filter = match quick_filter {
            QuickFilterPatch::Clear => None,
            QuickFilterPatch::Set(filter) => Some(filter),
        };
    }
    if let Some(view_mode) = patch.view_mode {
        view.view_mode = view_mode;
    }
    if let Some(icon_size) = patch.icon_size {
        view.icon_size = icon_size;
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use fm_domain::{PersistedFilter, ProviderId, SplitAxis, WorkspaceLayout};

    use super::super::default_workspace::default_workspace;
    use super::*;

    fn home() -> &'static Path {
        Path::new("/Users/erik")
    }

    fn workspace() -> Workspace {
        default_workspace(home(), None)
    }

    fn location(uri: &str) -> Location {
        Location::new(ProviderId::new("file"), uri.to_owned())
    }

    #[test]
    fn rename_workspace_changes_the_name() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        apply(
            &mut ws,
            WorkspaceCommand::RenameWorkspace {
                workspace_id,
                name: "Photos".to_owned(),
                expected_revision,
            },
            home(),
        )
        .expect("rename must succeed");

        assert_eq!(ws.name, "Photos");
    }

    #[test]
    fn rename_workspace_rejects_an_empty_name() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let error = apply(
            &mut ws,
            WorkspaceCommand::RenameWorkspace {
                workspace_id,
                name: "   ".to_owned(),
                expected_revision,
            },
            home(),
        )
        .unwrap_err();

        assert!(matches!(error, WorkspaceError::InvalidCommand(_)));
    }

    #[test]
    fn set_active_pane_activates_an_existing_pane() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let other_pane = ws.panes[1].id;

        apply(
            &mut ws,
            WorkspaceCommand::SetActivePane {
                workspace_id,
                pane_id: other_pane,
                expected_revision,
            },
            home(),
        )
        .expect("set active pane must succeed");

        assert_eq!(ws.active_pane_id, other_pane);
    }

    #[test]
    fn set_active_pane_rejects_an_unknown_pane() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let error = apply(
            &mut ws,
            WorkspaceCommand::SetActivePane {
                workspace_id,
                pane_id: PaneId::new(),
                expected_revision,
            },
            home(),
        )
        .unwrap_err();

        assert!(matches!(error, WorkspaceError::PaneNotFound { .. }));
    }

    #[test]
    fn add_tab_appends_and_activates_a_new_tab() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        let tabs_before = ws.panes[0].tabs.len();

        apply(
            &mut ws,
            WorkspaceCommand::AddTab {
                workspace_id,
                pane_id,
                location: location("file:///Users/erik/Downloads"),
                expected_revision,
            },
            home(),
        )
        .expect("add tab must succeed");

        let pane = ws.panes.iter().find(|pane| pane.id == pane_id).unwrap();
        assert_eq!(pane.tabs.len(), tabs_before + 1);
        assert_eq!(pane.active_tab_id, pane.tabs.last().unwrap().id);
    }

    #[test]
    fn close_tab_removes_it_and_activates_the_previous_tab() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        apply(
            &mut ws,
            WorkspaceCommand::AddTab {
                workspace_id,
                pane_id,
                location: location("file:///Users/erik/Downloads"),
                expected_revision,
            },
            home(),
        )
        .unwrap();
        let pane = ws.panes.iter().find(|pane| pane.id == pane_id).unwrap();
        let first_tab_id = pane.tabs[0].id;
        let second_tab_id = pane.tabs[1].id;

        apply(
            &mut ws,
            WorkspaceCommand::CloseTab {
                workspace_id,
                pane_id,
                tab_id: second_tab_id,
                expected_revision,
            },
            home(),
        )
        .expect("close tab must succeed");

        let pane = ws.panes.iter().find(|pane| pane.id == pane_id).unwrap();
        assert_eq!(pane.tabs.len(), 1);
        assert_eq!(pane.active_tab_id, first_tab_id);
    }

    #[test]
    fn close_tab_on_a_panes_last_tab_creates_a_replacement_at_the_home_directory() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        let original_tab_id = ws.panes[0].tabs[0].id;

        apply(
            &mut ws,
            WorkspaceCommand::CloseTab {
                workspace_id,
                pane_id,
                tab_id: original_tab_id,
                expected_revision,
            },
            home(),
        )
        .expect("close tab must succeed");

        let pane = ws.panes.iter().find(|pane| pane.id == pane_id).unwrap();
        assert_eq!(pane.tabs.len(), 1);
        assert_ne!(pane.tabs[0].id, original_tab_id);
        assert_eq!(pane.tabs[0].location, location_for(home()));
        assert_eq!(pane.active_tab_id, pane.tabs[0].id);
        assert!(ws.validate().is_ok());
    }

    #[test]
    fn close_tab_rejects_an_unknown_tab() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        let error = apply(
            &mut ws,
            WorkspaceCommand::CloseTab {
                workspace_id,
                pane_id,
                tab_id: TabId::new(),
                expected_revision,
            },
            home(),
        )
        .unwrap_err();

        assert!(matches!(error, WorkspaceError::TabNotFound { .. }));
    }

    #[test]
    fn move_tab_reorders_within_a_pane() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let pane_id = ws.active_pane_id;
        let expected_revision = ws.revision;
        apply(
            &mut ws,
            WorkspaceCommand::AddTab {
                workspace_id,
                pane_id,
                location: location("file:///Users/erik/Downloads"),
                expected_revision,
            },
            home(),
        )
        .unwrap();
        let moved_tab_id = ws.panes[0].tabs[1].id;

        apply(
            &mut ws,
            WorkspaceCommand::MoveTab {
                workspace_id,
                source_pane_id: pane_id,
                tab_id: moved_tab_id,
                target_pane_id: pane_id,
                target_index: 0,
                expected_revision,
            },
            home(),
        )
        .expect("move tab must succeed");

        assert_eq!(ws.panes[0].tabs[0].id, moved_tab_id);
        assert_eq!(ws.panes[0].active_tab_id, moved_tab_id);
    }

    #[test]
    fn move_tab_to_another_pane_preserves_it_and_replaces_an_empty_source() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let source_pane_id = ws.panes[0].id;
        let target_pane_id = ws.panes[1].id;
        let tab_id = ws.panes[0].tabs[0].id;
        let original = ws.panes[0].tabs[0].clone();
        let expected_revision = ws.revision;

        apply(
            &mut ws,
            WorkspaceCommand::MoveTab {
                workspace_id,
                source_pane_id,
                tab_id,
                target_pane_id,
                target_index: 0,
                expected_revision,
            },
            home(),
        )
        .expect("cross-pane move must succeed");

        assert_eq!(ws.panes[0].tabs.len(), 1);
        assert_ne!(ws.panes[0].tabs[0].id, tab_id);
        assert_eq!(ws.panes[0].tabs[0].location, location_for(home()));
        assert_eq!(ws.panes[1].tabs[0], original);
        assert_eq!(ws.panes[1].active_tab_id, tab_id);
        assert_eq!(ws.active_pane_id, target_pane_id);
    }

    #[test]
    fn activate_tab_changes_the_active_tab_and_pane_atomically() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.panes[1].id;
        apply(
            &mut ws,
            WorkspaceCommand::AddTab {
                workspace_id,
                pane_id,
                location: location("file:///Users/erik/Downloads"),
                expected_revision,
            },
            home(),
        )
        .unwrap();
        let pane = ws.panes.iter().find(|pane| pane.id == pane_id).unwrap();
        let first_tab_id = pane.tabs[0].id;

        apply(
            &mut ws,
            WorkspaceCommand::ActivateTab {
                workspace_id,
                pane_id,
                tab_id: first_tab_id,
                expected_revision,
            },
            home(),
        )
        .expect("activate tab must succeed");

        let pane = ws.panes.iter().find(|pane| pane.id == pane_id).unwrap();
        assert_eq!(pane.active_tab_id, first_tab_id);
        assert_eq!(ws.active_pane_id, pane_id);
    }

    #[test]
    fn navigate_tab_push_records_history_and_clears_forward() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        let tab_id = ws.panes[0].tabs[0].id;
        let original_location = ws.panes[0].tabs[0].location.clone();

        apply(
            &mut ws,
            WorkspaceCommand::NavigateTab {
                workspace_id,
                pane_id,
                tab_id,
                location: Some(location("file:///Users/erik/Downloads")),
                navigation_mode: NavigationMode::Push,
                expected_revision,
            },
            home(),
        )
        .expect("navigate must succeed");

        let tab = ws.panes[0].tabs.iter().find(|t| t.id == tab_id).unwrap();
        assert_eq!(tab.location, location("file:///Users/erik/Downloads"));
        assert_eq!(tab.history.back, vec![original_location]);
        assert!(tab.history.forward.is_empty());
    }

    #[test]
    fn navigate_tab_push_does_not_duplicate_consecutive_locations() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        let tab_id = ws.panes[0].tabs[0].id;
        let current = ws.panes[0].tabs[0].location.clone();

        apply(
            &mut ws,
            WorkspaceCommand::NavigateTab {
                workspace_id,
                pane_id,
                tab_id,
                location: Some(current.clone()),
                navigation_mode: NavigationMode::Push,
                expected_revision,
            },
            home(),
        )
        .expect("navigate must succeed");

        let tab = ws.panes[0].tabs.iter().find(|t| t.id == tab_id).unwrap();
        assert!(tab.history.back.is_empty());
    }

    #[test]
    fn navigate_tab_back_and_forward_resolve_targets_from_backend_history() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        let tab_id = ws.panes[0].tabs[0].id;
        let original_location = ws.panes[0].tabs[0].location.clone();
        let downloads = location("file:///Users/erik/Downloads");

        apply(
            &mut ws,
            WorkspaceCommand::NavigateTab {
                workspace_id,
                pane_id,
                tab_id,
                location: Some(downloads.clone()),
                navigation_mode: NavigationMode::Push,
                expected_revision,
            },
            home(),
        )
        .unwrap();

        apply(
            &mut ws,
            WorkspaceCommand::NavigateTab {
                workspace_id,
                pane_id,
                tab_id,
                location: None,
                navigation_mode: NavigationMode::Back,
                expected_revision,
            },
            home(),
        )
        .expect("navigate back must succeed");

        let tab = ws.panes[0].tabs.iter().find(|t| t.id == tab_id).unwrap();
        assert_eq!(tab.location, original_location);
        assert!(tab.history.back.is_empty());
        assert_eq!(tab.history.forward, vec![downloads.clone()]);

        apply(
            &mut ws,
            WorkspaceCommand::NavigateTab {
                workspace_id,
                pane_id,
                tab_id,
                location: None,
                navigation_mode: NavigationMode::Forward,
                expected_revision,
            },
            home(),
        )
        .expect("navigate forward must succeed");

        let tab = ws.panes[0].tabs.iter().find(|t| t.id == tab_id).unwrap();
        assert_eq!(tab.location, downloads);
        assert_eq!(tab.history.back, vec![original_location]);
        assert!(tab.history.forward.is_empty());
    }

    #[test]
    fn navigate_tab_back_with_empty_history_is_a_no_op() {
        let mut ws = workspace();
        let original = ws.clone();

        apply(
            &mut ws,
            WorkspaceCommand::NavigateTab {
                workspace_id: original.id,
                pane_id: original.active_pane_id,
                tab_id: original.panes[0].tabs[0].id,
                location: None,
                navigation_mode: NavigationMode::Back,
                expected_revision: original.revision,
            },
            home(),
        )
        .expect("empty history must be a successful no-op");

        assert_eq!(ws, original);
    }

    #[test]
    fn navigate_tab_refresh_does_not_touch_history() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        let tab_id = ws.panes[0].tabs[0].id;
        let current = ws.panes[0].tabs[0].location.clone();

        apply(
            &mut ws,
            WorkspaceCommand::NavigateTab {
                workspace_id,
                pane_id,
                tab_id,
                location: Some(current.clone()),
                navigation_mode: NavigationMode::Refresh,
                expected_revision,
            },
            home(),
        )
        .expect("navigate must succeed");

        let tab = ws.panes[0].tabs.iter().find(|t| t.id == tab_id).unwrap();
        assert_eq!(tab.location, current);
        assert!(tab.history.back.is_empty());
        assert!(tab.history.forward.is_empty());
    }

    #[test]
    fn navigate_tab_rejects_an_unknown_tab() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        let error = apply(
            &mut ws,
            WorkspaceCommand::NavigateTab {
                workspace_id,
                pane_id,
                tab_id: TabId::new(),
                location: Some(location("file:///Users/erik/Downloads")),
                navigation_mode: NavigationMode::Push,
                expected_revision,
            },
            home(),
        )
        .unwrap_err();

        assert!(matches!(error, WorkspaceError::TabNotFound { .. }));
    }

    #[test]
    fn navigation_history_is_bounded_after_many_pushes() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        let tab_id = ws.panes[0].tabs[0].id;

        for i in 0..(MAX_NAVIGATION_HISTORY_LEN + 10) {
            apply(
                &mut ws,
                WorkspaceCommand::NavigateTab {
                    workspace_id,
                    pane_id,
                    tab_id,
                    location: Some(location(&format!("file:///Users/erik/dir-{i}"))),
                    navigation_mode: NavigationMode::Push,
                    expected_revision,
                },
                home(),
            )
            .expect("navigate must succeed");
        }

        let tab = ws.panes[0].tabs.iter().find(|t| t.id == tab_id).unwrap();
        assert!(tab.history.back.len() + tab.history.forward.len() <= MAX_NAVIGATION_HISTORY_LEN);
        assert!(ws.validate().is_ok());
    }

    #[test]
    fn update_view_patches_only_the_given_fields() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        let tab_id = ws.panes[0].tabs[0].id;
        let original_columns = ws.panes[0].tabs[0].view.columns.clone();

        apply(
            &mut ws,
            WorkspaceCommand::UpdateView {
                workspace_id,
                pane_id,
                tab_id,
                patch: DirectoryViewPatch {
                    show_hidden: Some(true),
                    quick_filter: Some(QuickFilterPatch::Set(PersistedFilter {
                        query: "report".to_owned(),
                    })),
                    ..Default::default()
                },
                expected_revision,
            },
            home(),
        )
        .expect("update view must succeed");

        let tab = ws.panes[0].tabs.iter().find(|t| t.id == tab_id).unwrap();
        assert!(tab.view.show_hidden);
        assert_eq!(tab.view.quick_filter.as_ref().unwrap().query, "report");
        assert_eq!(tab.view.columns, original_columns);
    }

    #[test]
    fn update_view_clears_the_quick_filter() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        let tab_id = ws.panes[0].tabs[0].id;
        ws.panes[0].tabs[0].view.quick_filter = Some(PersistedFilter {
            query: "report".to_owned(),
        });

        apply(
            &mut ws,
            WorkspaceCommand::UpdateView {
                workspace_id,
                pane_id,
                tab_id,
                patch: DirectoryViewPatch {
                    quick_filter: Some(QuickFilterPatch::Clear),
                    ..Default::default()
                },
                expected_revision,
            },
            home(),
        )
        .expect("update view must succeed");

        let tab = ws.panes[0].tabs.iter().find(|t| t.id == tab_id).unwrap();
        assert!(tab.view.quick_filter.is_none());
    }

    #[test]
    fn update_view_patches_view_mode_and_icon_size_independently_of_other_fields() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;
        let tab_id = ws.panes[0].tabs[0].id;
        let original_sort = ws.panes[0].tabs[0].view.sort.clone();
        assert_eq!(
            ws.panes[0].tabs[0].view.view_mode,
            fm_domain::DirectoryViewMode::Table
        );

        apply(
            &mut ws,
            WorkspaceCommand::UpdateView {
                workspace_id,
                pane_id,
                tab_id,
                patch: DirectoryViewPatch {
                    view_mode: Some(fm_domain::DirectoryViewMode::Grid),
                    icon_size: Some(fm_domain::IconSize::Large),
                    ..Default::default()
                },
                expected_revision,
            },
            home(),
        )
        .expect("update view must succeed");

        let tab = ws.panes[0].tabs.iter().find(|t| t.id == tab_id).unwrap();
        assert_eq!(tab.view.view_mode, fm_domain::DirectoryViewMode::Grid);
        assert_eq!(tab.view.icon_size, fm_domain::IconSize::Large);
        assert_eq!(tab.view.sort, original_sort);
    }

    #[test]
    fn update_layout_replaces_the_layout_tree() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let left_pane = ws.panes[0].id;
        let right_pane = ws.panes[1].id;
        // Swap the axis while keeping both existing panes referenced, so the
        // new layout stays valid (invariant 7: every pane appears exactly
        // once in the layout).
        let new_layout = WorkspaceLayout::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.5,
            first: Box::new(WorkspaceLayout::Pane { pane_id: left_pane }),
            second: Box::new(WorkspaceLayout::Pane {
                pane_id: right_pane,
            }),
        };

        apply(
            &mut ws,
            WorkspaceCommand::UpdateLayout {
                workspace_id,
                layout: new_layout.clone(),
                expected_revision,
            },
            home(),
        )
        .expect("update layout must succeed");

        assert_eq!(ws.layout, new_layout);
    }

    #[test]
    fn update_layout_rejects_a_layout_that_drops_a_pane() {
        let mut ws = workspace();
        let workspace_id = ws.id;
        let expected_revision = ws.revision;
        let pane_id = ws.active_pane_id;

        let error = apply(
            &mut ws,
            WorkspaceCommand::UpdateLayout {
                workspace_id,
                layout: WorkspaceLayout::Split {
                    axis: SplitAxis::Horizontal,
                    ratio: 0.5,
                    first: Box::new(WorkspaceLayout::Pane { pane_id }),
                    second: Box::new(WorkspaceLayout::Pane {
                        pane_id: PaneId::new(),
                    }),
                },
                expected_revision,
            },
            home(),
        )
        .unwrap_err();

        assert!(matches!(error, WorkspaceError::Invalid(_)));
    }
}
