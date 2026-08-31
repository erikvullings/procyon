//! Semantic workspace mutation commands (spec §5.3.9).
//!
//! The frontend never replaces arbitrary workspace JSON; every mutation is
//! one of these focused commands, checked against the workspace's revision
//! by `fm_application`'s `WorkspaceService::apply_command` (task 0080). This
//! type carries no behaviour of its own — applying a command is an
//! `fm-application` concern (spec §3 rule 2).

use crate::ids::{PaneId, TabId, WorkspaceId};
use crate::location::Location;
use crate::workspace::{
    ColumnConfiguration, DirectoryViewMode, IconSize, PersistedFilter, SortDescriptor,
    WorkspaceLayout,
};

/// A focused workspace mutation, exactly per spec §5.3.9.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceCommand {
    /// Renames the workspace.
    RenameWorkspace {
        /// The workspace to rename.
        workspace_id: WorkspaceId,
        /// The new name.
        name: String,
        /// The revision the caller last observed.
        expected_revision: u64,
    },
    /// Changes which pane has focus.
    SetActivePane {
        /// The workspace to mutate.
        workspace_id: WorkspaceId,
        /// The pane to activate.
        pane_id: PaneId,
        /// The revision the caller last observed.
        expected_revision: u64,
    },
    /// Opens a new tab in a pane.
    AddTab {
        /// The workspace to mutate.
        workspace_id: WorkspaceId,
        /// The pane the new tab is added to.
        pane_id: PaneId,
        /// The new tab's initial location.
        location: Location,
        /// The revision the caller last observed.
        expected_revision: u64,
    },
    /// Opens a tab that must not be restored in a later application session.
    AddTransientTab {
        /// The workspace to mutate.
        workspace_id: WorkspaceId,
        /// The pane the new tab is added to.
        pane_id: PaneId,
        /// The new tab's initial location.
        location: Location,
        /// The revision the caller last observed.
        expected_revision: u64,
    },
    /// Closes a tab.
    CloseTab {
        /// The workspace to mutate.
        workspace_id: WorkspaceId,
        /// The pane the tab belongs to.
        pane_id: PaneId,
        /// The tab to close.
        tab_id: TabId,
        /// The revision the caller last observed.
        expected_revision: u64,
    },
    /// Reorders a tab or moves it to another pane.
    MoveTab {
        /// The workspace to mutate.
        workspace_id: WorkspaceId,
        /// The pane currently containing the tab.
        source_pane_id: PaneId,
        /// The tab to move.
        tab_id: TabId,
        /// The pane that should contain the tab after the move.
        target_pane_id: PaneId,
        /// Zero-based position in the target pane after removing the source tab.
        target_index: usize,
        /// The revision the caller last observed.
        expected_revision: u64,
    },
    /// Changes which tab is shown in a pane.
    ActivateTab {
        /// The workspace to mutate.
        workspace_id: WorkspaceId,
        /// The pane to mutate.
        pane_id: PaneId,
        /// The tab to activate.
        tab_id: TabId,
        /// The revision the caller last observed.
        expected_revision: u64,
    },
    /// Navigates a tab to a new location.
    NavigateTab {
        /// The workspace to mutate.
        workspace_id: WorkspaceId,
        /// The pane the tab belongs to.
        pane_id: PaneId,
        /// The tab to navigate.
        tab_id: TabId,
        /// The explicit target for push/refresh navigation. Back and forward
        /// resolve their target from the tab's authoritative history.
        location: Option<Location>,
        /// How this navigation affects back/forward history.
        navigation_mode: NavigationMode,
        /// The revision the caller last observed.
        expected_revision: u64,
    },
    /// Patches a tab's persisted view configuration (sort, columns, filters).
    UpdateView {
        /// The workspace to mutate.
        workspace_id: WorkspaceId,
        /// The pane the tab belongs to.
        pane_id: PaneId,
        /// The tab to update.
        tab_id: TabId,
        /// The fields to change; absent fields are left untouched.
        patch: DirectoryViewPatch,
        /// The revision the caller last observed.
        expected_revision: u64,
    },
    /// Replaces the workspace's pane layout tree.
    UpdateLayout {
        /// The workspace to mutate.
        workspace_id: WorkspaceId,
        /// The new layout.
        layout: WorkspaceLayout,
        /// The revision the caller last observed.
        expected_revision: u64,
    },
}

impl WorkspaceCommand {
    /// The workspace this command targets.
    #[must_use]
    pub fn workspace_id(&self) -> WorkspaceId {
        match self {
            Self::RenameWorkspace { workspace_id, .. }
            | Self::SetActivePane { workspace_id, .. }
            | Self::AddTab { workspace_id, .. }
            | Self::AddTransientTab { workspace_id, .. }
            | Self::CloseTab { workspace_id, .. }
            | Self::MoveTab { workspace_id, .. }
            | Self::ActivateTab { workspace_id, .. }
            | Self::NavigateTab { workspace_id, .. }
            | Self::UpdateView { workspace_id, .. }
            | Self::UpdateLayout { workspace_id, .. } => *workspace_id,
        }
    }

    /// The revision the caller last observed, checked before applying the
    /// mutation (spec §5.3.9 step 1, §5.3.10).
    #[must_use]
    pub fn expected_revision(&self) -> u64 {
        match self {
            Self::RenameWorkspace {
                expected_revision, ..
            }
            | Self::SetActivePane {
                expected_revision, ..
            }
            | Self::AddTab {
                expected_revision, ..
            }
            | Self::AddTransientTab {
                expected_revision, ..
            }
            | Self::CloseTab {
                expected_revision, ..
            }
            | Self::MoveTab {
                expected_revision, ..
            }
            | Self::ActivateTab {
                expected_revision, ..
            }
            | Self::NavigateTab {
                expected_revision, ..
            }
            | Self::UpdateView {
                expected_revision, ..
            }
            | Self::UpdateLayout {
                expected_revision, ..
            } => *expected_revision,
        }
    }
}

/// How a [`WorkspaceCommand::NavigateTab`] affects a tab's back/forward
/// history.
///
/// Not literally defined in spec §5.3.9's snippet; inferred from §5.3.4's
/// back/forward navigation description and flagged here as a judgment call
/// (task 0080's Agent Notes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationMode {
    /// Ordinary navigation: pushes the previous location onto `back` and
    /// clears `forward`.
    Push,
    /// Moves one entry back through history.
    Back,
    /// Moves one entry forward through history.
    Forward,
    /// Reloads the current location without touching history.
    Refresh,
}

/// A partial update to a tab's [`crate::workspace::DirectoryViewConfiguration`];
/// absent fields are left unchanged (inferred shape, task 0080's Agent Notes).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DirectoryViewPatch {
    /// Replaces the active sort descriptors, if present.
    pub sort: Option<Vec<SortDescriptor>>,
    /// Replaces the column configuration, if present.
    pub columns: Option<Vec<ColumnConfiguration>>,
    /// Replaces whether hidden entries are shown, if present.
    pub show_hidden: Option<bool>,
    /// Replaces whether directories are grouped before files, if present.
    pub folders_first: Option<bool>,
    /// Replaces the persisted quick filter, if present.
    pub quick_filter: Option<QuickFilterPatch>,
    /// Replaces the active view mode, if present (task 0134).
    pub view_mode: Option<DirectoryViewMode>,
    /// Replaces the grid tile size, if present (task 0134).
    pub icon_size: Option<IconSize>,
}

/// A patch to a tab's persisted quick filter: either clear it or set a new
/// query.
#[derive(Debug, Clone, PartialEq)]
pub enum QuickFilterPatch {
    /// Removes the persisted quick filter.
    Clear,
    /// Sets a new persisted quick filter.
    Set(PersistedFilter),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ProviderId;

    fn location() -> Location {
        Location::new(ProviderId::new("file"), "file:///Users/erik")
    }

    #[test]
    fn workspace_id_and_expected_revision_are_extracted_from_every_variant() {
        let workspace_id = WorkspaceId::new();
        let pane_id = PaneId::new();
        let tab_id = TabId::new();

        let commands = [
            WorkspaceCommand::RenameWorkspace {
                workspace_id,
                name: "Photos".to_owned(),
                expected_revision: 1,
            },
            WorkspaceCommand::SetActivePane {
                workspace_id,
                pane_id,
                expected_revision: 2,
            },
            WorkspaceCommand::AddTab {
                workspace_id,
                pane_id,
                location: location(),
                expected_revision: 3,
            },
            WorkspaceCommand::AddTransientTab {
                workspace_id,
                pane_id,
                location: location(),
                expected_revision: 4,
            },
            WorkspaceCommand::CloseTab {
                workspace_id,
                pane_id,
                tab_id,
                expected_revision: 5,
            },
            WorkspaceCommand::MoveTab {
                workspace_id,
                source_pane_id: pane_id,
                tab_id,
                target_pane_id: pane_id,
                target_index: 0,
                expected_revision: 6,
            },
            WorkspaceCommand::ActivateTab {
                workspace_id,
                pane_id,
                tab_id,
                expected_revision: 7,
            },
            WorkspaceCommand::NavigateTab {
                workspace_id,
                pane_id,
                tab_id,
                location: Some(location()),
                navigation_mode: NavigationMode::Push,
                expected_revision: 8,
            },
            WorkspaceCommand::UpdateView {
                workspace_id,
                pane_id,
                tab_id,
                patch: DirectoryViewPatch::default(),
                expected_revision: 9,
            },
            WorkspaceCommand::UpdateLayout {
                workspace_id,
                layout: WorkspaceLayout::Pane { pane_id },
                expected_revision: 10,
            },
        ];

        for (index, command) in commands.iter().enumerate() {
            assert_eq!(command.workspace_id(), workspace_id);
            assert_eq!(command.expected_revision(), index as u64 + 1);
        }
    }
}
