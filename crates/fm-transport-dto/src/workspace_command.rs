//! Wire representation of [`fm_domain::WorkspaceCommand`] (spec §5.3.9):
//! semantic workspace mutations sent from a host to `POST
//! /api/v1/workspaces/{workspaceId}/commands` (or the equivalent Tauri
//! command).

use fm_domain::{DirectoryViewPatch, NavigationMode, QuickFilterPatch, WorkspaceCommand};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::location::LocationDto;
use crate::workspace::{
    ColumnConfigurationDto, DirectoryViewModeDto, IconSizeDto, OperationCentrePreferencesDto,
    PersistedFilterDto, SortDescriptorDto, WorkspaceLayoutDto,
};

/// How a tab's location changed, so navigation history can be updated
/// correctly (spec §5.3.4, §5.3.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NavigationModeDto {
    /// A forward navigation to a new location: push the previous location
    /// onto `back` and clear `forward`.
    Push,
    /// Navigated using the back button/history entry.
    Back,
    /// Navigated using the forward button/history entry.
    Forward,
    /// Reloaded the current location; history is untouched.
    Refresh,
}

impl From<NavigationMode> for NavigationModeDto {
    fn from(mode: NavigationMode) -> Self {
        match mode {
            NavigationMode::Push => Self::Push,
            NavigationMode::Back => Self::Back,
            NavigationMode::Forward => Self::Forward,
            NavigationMode::Refresh => Self::Refresh,
        }
    }
}

impl From<NavigationModeDto> for NavigationMode {
    fn from(dto: NavigationModeDto) -> Self {
        match dto {
            NavigationModeDto::Push => Self::Push,
            NavigationModeDto::Back => Self::Back,
            NavigationModeDto::Forward => Self::Forward,
            NavigationModeDto::Refresh => Self::Refresh,
        }
    }
}

/// Sets or clears a tab's persisted quick-filter as part of a
/// [`DirectoryViewPatchDto`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum QuickFilterPatchDto {
    /// Remove the tab's saved quick-filter.
    Clear,
    /// Replace the tab's saved quick-filter.
    Set {
        /// The new quick-filter query.
        filter: PersistedFilterDto,
    },
}

impl From<QuickFilterPatch> for QuickFilterPatchDto {
    fn from(patch: QuickFilterPatch) -> Self {
        match patch {
            QuickFilterPatch::Clear => Self::Clear,
            QuickFilterPatch::Set(filter) => Self::Set {
                filter: filter.into(),
            },
        }
    }
}

impl From<QuickFilterPatchDto> for QuickFilterPatch {
    fn from(dto: QuickFilterPatchDto) -> Self {
        match dto {
            QuickFilterPatchDto::Clear => Self::Clear,
            QuickFilterPatchDto::Set { filter } => Self::Set(filter.into()),
        }
    }
}

/// A partial update to a tab's persisted view configuration; only fields
/// set to `Some` are changed (spec §5.3.9 `UpdateView`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryViewPatchDto {
    /// Replace the active sort descriptors, if present.
    pub sort: Option<Vec<SortDescriptorDto>>,
    /// Replace the column configuration, if present.
    pub columns: Option<Vec<ColumnConfigurationDto>>,
    /// Replace whether hidden entries are shown, if present.
    pub show_hidden: Option<bool>,
    /// Replace whether directories are grouped before files, if present.
    pub folders_first: Option<bool>,
    /// Set or clear the saved quick-filter, if present.
    pub quick_filter: Option<QuickFilterPatchDto>,
    /// Replace the active view mode, if present (task 0134).
    pub view_mode: Option<DirectoryViewModeDto>,
    /// Replace the grid tile size, if present (task 0134).
    pub icon_size: Option<IconSizeDto>,
}

impl From<DirectoryViewPatch> for DirectoryViewPatchDto {
    fn from(patch: DirectoryViewPatch) -> Self {
        Self {
            sort: patch
                .sort
                .map(|sort| sort.into_iter().map(Into::into).collect()),
            columns: patch
                .columns
                .map(|columns| columns.into_iter().map(Into::into).collect()),
            show_hidden: patch.show_hidden,
            folders_first: patch.folders_first,
            quick_filter: patch.quick_filter.map(Into::into),
            view_mode: patch.view_mode.map(Into::into),
            icon_size: patch.icon_size.map(Into::into),
        }
    }
}

impl From<DirectoryViewPatchDto> for DirectoryViewPatch {
    fn from(dto: DirectoryViewPatchDto) -> Self {
        Self {
            sort: dto
                .sort
                .map(|sort| sort.into_iter().map(Into::into).collect()),
            columns: dto
                .columns
                .map(|columns| columns.into_iter().map(Into::into).collect()),
            show_hidden: dto.show_hidden,
            folders_first: dto.folders_first,
            quick_filter: dto.quick_filter.map(Into::into),
            view_mode: dto.view_mode.map(Into::into),
            icon_size: dto.icon_size.map(Into::into),
        }
    }
}

/// A semantic workspace mutation (spec §5.3.9). Every variant carries the
/// `workspaceId` it targets and the `expectedRevision` it was issued
/// against, so the service can detect a stale-view conflict (spec §5.3.10)
/// before applying the mutation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkspaceCommandDto {
    /// Renames the workspace.
    #[serde(rename_all = "camelCase")]
    RenameWorkspace {
        /// The target workspace.
        workspace_id: Uuid,
        /// The new, non-empty name.
        name: String,
        /// The revision this command was issued against.
        expected_revision: u64,
    },
    /// Activates a different pane.
    #[serde(rename_all = "camelCase")]
    SetActivePane {
        /// The target workspace.
        workspace_id: Uuid,
        /// The pane to activate.
        pane_id: Uuid,
        /// The revision this command was issued against.
        expected_revision: u64,
    },
    /// Appends a new tab to a pane and activates it.
    #[serde(rename_all = "camelCase")]
    AddTab {
        /// The target workspace.
        workspace_id: Uuid,
        /// The pane to append the tab to.
        pane_id: Uuid,
        /// The new tab's initial location.
        location: LocationDto,
        /// The revision this command was issued against.
        expected_revision: u64,
    },
    /// Appends a session-only tab to a pane and activates it.
    #[serde(rename_all = "camelCase")]
    AddTransientTab {
        /// The target workspace.
        workspace_id: Uuid,
        /// The pane to append the tab to.
        pane_id: Uuid,
        /// The new tab's initial location.
        location: LocationDto,
        /// The revision this command was issued against.
        expected_revision: u64,
    },
    /// Closes a tab; if it was the pane's only tab, a replacement tab at the
    /// host's home directory is created (spec §5.3.9).
    #[serde(rename_all = "camelCase")]
    CloseTab {
        /// The target workspace.
        workspace_id: Uuid,
        /// The pane the tab belongs to.
        pane_id: Uuid,
        /// The tab to close.
        tab_id: Uuid,
        /// The revision this command was issued against.
        expected_revision: u64,
    },
    /// Reorders a tab or moves it to another pane.
    #[serde(rename_all = "camelCase")]
    MoveTab {
        /// The target workspace.
        workspace_id: Uuid,
        /// The pane currently containing the tab.
        source_pane_id: Uuid,
        /// The tab to move.
        tab_id: Uuid,
        /// The pane that should contain the tab after the move.
        target_pane_id: Uuid,
        /// Zero-based position in the target pane after removing the source tab.
        target_index: usize,
        /// The revision this command was issued against.
        expected_revision: u64,
    },
    /// Activates a different tab within a pane.
    #[serde(rename_all = "camelCase")]
    ActivateTab {
        /// The target workspace.
        workspace_id: Uuid,
        /// The pane the tab belongs to.
        pane_id: Uuid,
        /// The tab to activate.
        tab_id: Uuid,
        /// The revision this command was issued against.
        expected_revision: u64,
    },
    /// Navigates a tab to a new location, updating its history according to
    /// `navigationMode`.
    #[serde(rename_all = "camelCase")]
    NavigateTab {
        /// The target workspace.
        workspace_id: Uuid,
        /// The pane the tab belongs to.
        pane_id: Uuid,
        /// The tab to navigate.
        tab_id: Uuid,
        /// The explicit target for push/refresh navigation. Omitted for
        /// backend-resolved back/forward navigation.
        #[serde(skip_serializing_if = "Option::is_none")]
        location: Option<LocationDto>,
        /// How the navigation affects history.
        navigation_mode: NavigationModeDto,
        /// The revision this command was issued against.
        expected_revision: u64,
    },
    /// Patches a tab's persisted view configuration.
    #[serde(rename_all = "camelCase")]
    UpdateView {
        /// The target workspace.
        workspace_id: Uuid,
        /// The pane the tab belongs to.
        pane_id: Uuid,
        /// The tab to patch.
        tab_id: Uuid,
        /// The fields to change.
        patch: DirectoryViewPatchDto,
        /// The revision this command was issued against.
        expected_revision: u64,
    },
    /// Replaces the workspace's pane layout tree.
    #[serde(rename_all = "camelCase")]
    UpdateLayout {
        /// The target workspace.
        workspace_id: Uuid,
        /// The new layout; must reference every existing pane exactly once.
        layout: WorkspaceLayoutDto,
        /// The revision this command was issued against.
        expected_revision: u64,
    },
    /// Updates operation-centre visibility and sizing.
    #[serde(rename_all = "camelCase")]
    UpdateOperationCentre {
        /// The target workspace.
        workspace_id: Uuid,
        /// The new operation-centre preferences.
        preferences: OperationCentrePreferencesDto,
        /// The revision this command was issued against.
        expected_revision: u64,
    },
}

impl From<WorkspaceCommand> for WorkspaceCommandDto {
    fn from(command: WorkspaceCommand) -> Self {
        match command {
            WorkspaceCommand::RenameWorkspace {
                workspace_id,
                name,
                expected_revision,
            } => Self::RenameWorkspace {
                workspace_id: workspace_id.into(),
                name,
                expected_revision,
            },
            WorkspaceCommand::SetActivePane {
                workspace_id,
                pane_id,
                expected_revision,
            } => Self::SetActivePane {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                expected_revision,
            },
            WorkspaceCommand::AddTab {
                workspace_id,
                pane_id,
                location,
                expected_revision,
            } => Self::AddTab {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                location: location.into(),
                expected_revision,
            },
            WorkspaceCommand::AddTransientTab {
                workspace_id,
                pane_id,
                location,
                expected_revision,
            } => Self::AddTransientTab {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                location: location.into(),
                expected_revision,
            },
            WorkspaceCommand::CloseTab {
                workspace_id,
                pane_id,
                tab_id,
                expected_revision,
            } => Self::CloseTab {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                tab_id: tab_id.into(),
                expected_revision,
            },
            WorkspaceCommand::MoveTab {
                workspace_id,
                source_pane_id,
                tab_id,
                target_pane_id,
                target_index,
                expected_revision,
            } => Self::MoveTab {
                workspace_id: workspace_id.into(),
                source_pane_id: source_pane_id.into(),
                tab_id: tab_id.into(),
                target_pane_id: target_pane_id.into(),
                target_index,
                expected_revision,
            },
            WorkspaceCommand::ActivateTab {
                workspace_id,
                pane_id,
                tab_id,
                expected_revision,
            } => Self::ActivateTab {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                tab_id: tab_id.into(),
                expected_revision,
            },
            WorkspaceCommand::NavigateTab {
                workspace_id,
                pane_id,
                tab_id,
                location,
                navigation_mode,
                expected_revision,
            } => Self::NavigateTab {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                tab_id: tab_id.into(),
                location: location.map(Into::into),
                navigation_mode: navigation_mode.into(),
                expected_revision,
            },
            WorkspaceCommand::UpdateView {
                workspace_id,
                pane_id,
                tab_id,
                patch,
                expected_revision,
            } => Self::UpdateView {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                tab_id: tab_id.into(),
                patch: patch.into(),
                expected_revision,
            },
            WorkspaceCommand::UpdateLayout {
                workspace_id,
                layout,
                expected_revision,
            } => Self::UpdateLayout {
                workspace_id: workspace_id.into(),
                layout: layout.into(),
                expected_revision,
            },
            WorkspaceCommand::UpdateOperationCentre {
                workspace_id,
                preferences,
                expected_revision,
            } => Self::UpdateOperationCentre {
                workspace_id: workspace_id.into(),
                preferences: preferences.into(),
                expected_revision,
            },
        }
    }
}

impl From<WorkspaceCommandDto> for WorkspaceCommand {
    fn from(dto: WorkspaceCommandDto) -> Self {
        match dto {
            WorkspaceCommandDto::RenameWorkspace {
                workspace_id,
                name,
                expected_revision,
            } => Self::RenameWorkspace {
                workspace_id: workspace_id.into(),
                name,
                expected_revision,
            },
            WorkspaceCommandDto::SetActivePane {
                workspace_id,
                pane_id,
                expected_revision,
            } => Self::SetActivePane {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                expected_revision,
            },
            WorkspaceCommandDto::AddTab {
                workspace_id,
                pane_id,
                location,
                expected_revision,
            } => Self::AddTab {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                location: location.into(),
                expected_revision,
            },
            WorkspaceCommandDto::AddTransientTab {
                workspace_id,
                pane_id,
                location,
                expected_revision,
            } => Self::AddTransientTab {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                location: location.into(),
                expected_revision,
            },
            WorkspaceCommandDto::CloseTab {
                workspace_id,
                pane_id,
                tab_id,
                expected_revision,
            } => Self::CloseTab {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                tab_id: tab_id.into(),
                expected_revision,
            },
            WorkspaceCommandDto::MoveTab {
                workspace_id,
                source_pane_id,
                tab_id,
                target_pane_id,
                target_index,
                expected_revision,
            } => Self::MoveTab {
                workspace_id: workspace_id.into(),
                source_pane_id: source_pane_id.into(),
                tab_id: tab_id.into(),
                target_pane_id: target_pane_id.into(),
                target_index,
                expected_revision,
            },
            WorkspaceCommandDto::ActivateTab {
                workspace_id,
                pane_id,
                tab_id,
                expected_revision,
            } => Self::ActivateTab {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                tab_id: tab_id.into(),
                expected_revision,
            },
            WorkspaceCommandDto::NavigateTab {
                workspace_id,
                pane_id,
                tab_id,
                location,
                navigation_mode,
                expected_revision,
            } => Self::NavigateTab {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                tab_id: tab_id.into(),
                location: location.map(Into::into),
                navigation_mode: navigation_mode.into(),
                expected_revision,
            },
            WorkspaceCommandDto::UpdateView {
                workspace_id,
                pane_id,
                tab_id,
                patch,
                expected_revision,
            } => Self::UpdateView {
                workspace_id: workspace_id.into(),
                pane_id: pane_id.into(),
                tab_id: tab_id.into(),
                patch: patch.into(),
                expected_revision,
            },
            WorkspaceCommandDto::UpdateLayout {
                workspace_id,
                layout,
                expected_revision,
            } => Self::UpdateLayout {
                workspace_id: workspace_id.into(),
                layout: layout.into(),
                expected_revision,
            },
            WorkspaceCommandDto::UpdateOperationCentre {
                workspace_id,
                preferences,
                expected_revision,
            } => Self::UpdateOperationCentre {
                workspace_id: workspace_id.into(),
                preferences: preferences.into(),
                expected_revision,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_domain::{PaneId, ProviderId, TabId, WorkspaceId};

    fn location() -> LocationDto {
        LocationDto {
            provider_id: "local".to_owned(),
            uri: "file:///Users/erik/Downloads".to_owned(),
        }
    }

    #[test]
    fn navigate_tab_dto_round_trips_through_serde_json_with_camel_case() {
        let dto = WorkspaceCommandDto::NavigateTab {
            workspace_id: Uuid::new_v4(),
            pane_id: Uuid::new_v4(),
            tab_id: Uuid::new_v4(),
            location: Some(location()),
            navigation_mode: NavigationModeDto::Push,
            expected_revision: 4,
        };
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        assert!(json.contains("\"type\":\"navigateTab\""));
        assert!(json.contains("\"navigationMode\":\"push\""));
        assert!(json.contains("\"expectedRevision\":4"));
        let parsed: WorkspaceCommandDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(dto, parsed);
    }

    #[test]
    fn back_navigation_dto_accepts_a_backend_resolved_target() {
        let json = format!(
            r#"{{"type":"navigateTab","workspaceId":"{}","paneId":"{}","tabId":"{}","navigationMode":"back","expectedRevision":4}}"#,
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );

        let parsed =
            serde_json::from_str::<WorkspaceCommandDto>(&json).expect("back target is optional");

        assert!(matches!(
            parsed,
            WorkspaceCommandDto::NavigateTab {
                navigation_mode: NavigationModeDto::Back,
                ..
            }
        ));
    }

    #[test]
    fn every_command_variant_converts_to_and_from_the_domain_type() {
        let workspace_id = WorkspaceId::new();
        let pane_id = PaneId::new();
        let tab_id = TabId::new();
        let commands = vec![
            WorkspaceCommand::RenameWorkspace {
                workspace_id,
                name: "Photos".to_owned(),
                expected_revision: 1,
            },
            WorkspaceCommand::SetActivePane {
                workspace_id,
                pane_id,
                expected_revision: 1,
            },
            WorkspaceCommand::AddTab {
                workspace_id,
                pane_id,
                location: fm_domain::Location::new(ProviderId::new("local"), "file:///tmp"),
                expected_revision: 1,
            },
            WorkspaceCommand::AddTransientTab {
                workspace_id,
                pane_id,
                location: fm_domain::Location::new(ProviderId::new("local"), "file:///tmp"),
                expected_revision: 1,
            },
            WorkspaceCommand::CloseTab {
                workspace_id,
                pane_id,
                tab_id,
                expected_revision: 1,
            },
            WorkspaceCommand::ActivateTab {
                workspace_id,
                pane_id,
                tab_id,
                expected_revision: 1,
            },
            WorkspaceCommand::NavigateTab {
                workspace_id,
                pane_id,
                tab_id,
                location: Some(fm_domain::Location::new(
                    ProviderId::new("local"),
                    "file:///tmp",
                )),
                navigation_mode: NavigationMode::Back,
                expected_revision: 1,
            },
            WorkspaceCommand::UpdateView {
                workspace_id,
                pane_id,
                tab_id,
                patch: DirectoryViewPatch {
                    show_hidden: Some(true),
                    ..Default::default()
                },
                expected_revision: 1,
            },
            WorkspaceCommand::UpdateLayout {
                workspace_id,
                layout: fm_domain::WorkspaceLayout::Pane { pane_id },
                expected_revision: 1,
            },
            WorkspaceCommand::UpdateOperationCentre {
                workspace_id,
                preferences: fm_domain::OperationCentrePreferences {
                    visible: true,
                    height: 240,
                },
                expected_revision: 1,
            },
        ];

        for command in commands {
            let dto: WorkspaceCommandDto = command.clone().into();
            let round_tripped: WorkspaceCommand = dto.into();
            assert_eq!(command, round_tripped);
        }
    }

    #[test]
    fn quick_filter_patch_dto_round_trips_both_variants() {
        for patch in [
            QuickFilterPatch::Clear,
            QuickFilterPatch::Set(fm_domain::PersistedFilter {
                query: "report".to_owned(),
            }),
        ] {
            let dto: QuickFilterPatchDto = patch.clone().into();
            let json = serde_json::to_string(&dto).expect("serialization must succeed");
            let parsed: QuickFilterPatchDto =
                serde_json::from_str(&json).expect("deserialization must succeed");
            let round_tripped: QuickFilterPatch = parsed.into();
            assert_eq!(patch, round_tripped);
        }
    }
}
