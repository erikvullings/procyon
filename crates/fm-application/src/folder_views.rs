//! Per-folder view memory (task 0270).
//!
//! When a user changes how a folder is shown (sort, columns, view mode, hidden files, folders
//! first), the backend remembers that view in the settings file, keyed by the folder's location.
//! Navigating a tab to a folder that has a remembered view restores it. Folders never customised
//! keep the defaults. Because the memory sits behind `apply_workspace_command`, both hosts and
//! every client behave identically, and no client-side storage is involved.

use fm_domain::{
    DirectoryViewConfiguration, DirectoryViewPatch, Location, NavigationMode, PaneId, TabId,
    Workspace, WorkspaceCommand,
};
use fm_settings::FolderViewMemory;

/// What `apply_workspace_command` must do around a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewMemoryHook {
    /// Nothing to remember or restore.
    None,
    /// The user changed the tab's view: remember it for the tab's folder.
    Remember { pane_id: PaneId, tab_id: TabId },
    /// The tab moved to another folder: restore that folder's remembered view, if any.
    Restore { pane_id: PaneId, tab_id: TabId },
}

/// Whether a view patch changes anything worth remembering (a quick filter alone is not).
fn touches_remembered_fields(patch: &DirectoryViewPatch) -> bool {
    patch.sort.is_some()
        || patch.columns.is_some()
        || patch.show_hidden.is_some()
        || patch.folders_first.is_some()
        || patch.view_mode.is_some()
        || patch.icon_size.is_some()
}

pub(crate) fn hook_for(command: &WorkspaceCommand) -> ViewMemoryHook {
    match command {
        WorkspaceCommand::UpdateView {
            pane_id,
            tab_id,
            patch,
            ..
        } if touches_remembered_fields(patch) => ViewMemoryHook::Remember {
            pane_id: *pane_id,
            tab_id: *tab_id,
        },
        WorkspaceCommand::NavigateTab {
            pane_id,
            tab_id,
            navigation_mode: NavigationMode::Push | NavigationMode::Back | NavigationMode::Forward,
            ..
        } => ViewMemoryHook::Restore {
            pane_id: *pane_id,
            tab_id: *tab_id,
        },
        _ => ViewMemoryHook::None,
    }
}

/// A tab's folder and current view, if the tab exists.
pub(crate) fn tab_view(
    workspace: &Workspace,
    pane_id: PaneId,
    tab_id: TabId,
) -> Option<(Location, DirectoryViewConfiguration)> {
    let pane = workspace.panes.iter().find(|pane| pane.id == pane_id)?;
    let tab = pane.tabs.iter().find(|tab| tab.id == tab_id)?;
    Some((tab.location.clone(), tab.view.clone()))
}

/// The memory to store for `view` at `location`.
pub(crate) fn memory_from_view(
    location: Location,
    view: &DirectoryViewConfiguration,
) -> FolderViewMemory {
    FolderViewMemory {
        location,
        sort: view.sort.clone(),
        columns: view.columns.clone(),
        show_hidden: view.show_hidden,
        folders_first: view.folders_first,
        view_mode: view.view_mode,
        icon_size: view.icon_size,
    }
}

/// The patch that makes `view` equal to `memory`, or `None` when it already does.
pub(crate) fn patch_to_restore(
    memory: &FolderViewMemory,
    view: &DirectoryViewConfiguration,
) -> Option<DirectoryViewPatch> {
    let patch = DirectoryViewPatch {
        sort: (memory.sort != view.sort).then(|| memory.sort.clone()),
        columns: (memory.columns != view.columns).then(|| memory.columns.clone()),
        show_hidden: (memory.show_hidden != view.show_hidden).then_some(memory.show_hidden),
        folders_first: (memory.folders_first != view.folders_first).then_some(memory.folders_first),
        quick_filter: None,
        view_mode: (memory.view_mode != view.view_mode).then_some(memory.view_mode),
        icon_size: (memory.icon_size != view.icon_size).then_some(memory.icon_size),
    };
    touches_remembered_fields(&patch).then_some(patch)
}

#[cfg(test)]
mod tests {
    use fm_domain::{
        ColumnConfiguration, DirectoryViewMode, IconSize, QuickFilterPatch, SortDescriptor,
        SortDirection,
    };

    use super::*;

    fn view() -> DirectoryViewConfiguration {
        DirectoryViewConfiguration {
            sort: vec![SortDescriptor {
                column_id: "core.name".into(),
                direction: SortDirection::Ascending,
            }],
            columns: vec![ColumnConfiguration {
                column_id: "core.name".into(),
                width: 200,
                visible: true,
            }],
            show_hidden: false,
            folders_first: true,
            quick_filter: None,
            view_mode: DirectoryViewMode::Table,
            icon_size: IconSize::default(),
        }
    }

    fn location() -> Location {
        Location::parse("file:///photos").expect("location")
    }

    #[test]
    fn an_identical_view_needs_no_restore() {
        let memory = memory_from_view(location(), &view());
        assert_eq!(patch_to_restore(&memory, &view()), None);
    }

    #[test]
    fn only_the_differing_fields_are_patched() {
        let mut remembered = view();
        remembered.view_mode = DirectoryViewMode::Grid;
        remembered.sort = vec![SortDescriptor {
            column_id: "core.modified".into(),
            direction: SortDirection::Descending,
        }];
        let memory = memory_from_view(location(), &remembered);

        let patch = patch_to_restore(&memory, &view()).expect("a patch");

        assert_eq!(patch.view_mode, Some(DirectoryViewMode::Grid));
        assert_eq!(patch.sort, Some(remembered.sort));
        assert_eq!(patch.columns, None);
        assert_eq!(patch.show_hidden, None);
        assert_eq!(patch.quick_filter.map(|_| ()), None);
    }

    #[test]
    fn a_quick_filter_alone_is_not_remembered() {
        let patch = DirectoryViewPatch {
            quick_filter: Some(QuickFilterPatch::Clear),
            ..DirectoryViewPatch::default()
        };
        assert!(!touches_remembered_fields(&patch));
        assert!(touches_remembered_fields(&DirectoryViewPatch {
            folders_first: Some(false),
            ..DirectoryViewPatch::default()
        }));
    }
}
