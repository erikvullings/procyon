//! Wire representation of [`fm_domain::Workspace`], its panes and tabs
//! (spec §5.3).

use chrono::{DateTime, Utc};
use fm_domain::{
    ColumnConfiguration, DirectoryViewConfiguration, DirectoryViewMode, IconSize,
    NavigationHistory, OperationCentrePreferences, PaneId, PaneState, PersistedFilter,
    SortDescriptor, SortDirection, SplitAxis, TabId, TabState, Workspace, WorkspaceId,
    WorkspaceLayout,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::location::LocationDto;

/// A sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SortDirectionDto {
    /// Smallest/earliest first.
    Ascending,
    /// Largest/latest first.
    Descending,
}

impl From<SortDirection> for SortDirectionDto {
    fn from(direction: SortDirection) -> Self {
        match direction {
            SortDirection::Ascending => Self::Ascending,
            SortDirection::Descending => Self::Descending,
        }
    }
}

impl From<SortDirectionDto> for SortDirection {
    fn from(direction: SortDirectionDto) -> Self {
        match direction {
            SortDirectionDto::Ascending => Self::Ascending,
            SortDirectionDto::Descending => Self::Descending,
        }
    }
}

/// A single sort descriptor: a column and a direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SortDescriptorDto {
    /// The column to sort by, e.g. `"core.name"` or a plugin-provided column.
    pub column_id: String,
    /// The direction to sort in.
    pub direction: SortDirectionDto,
}

impl From<SortDescriptor> for SortDescriptorDto {
    fn from(descriptor: SortDescriptor) -> Self {
        Self {
            column_id: descriptor.column_id,
            direction: descriptor.direction.into(),
        }
    }
}

impl From<SortDescriptorDto> for SortDescriptor {
    fn from(dto: SortDescriptorDto) -> Self {
        Self {
            column_id: dto.column_id,
            direction: dto.direction.into(),
        }
    }
}

/// Persisted width and visibility for a single directory-table column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ColumnConfigurationDto {
    /// The column's identifier, e.g. `"core.name"` or a plugin-provided column.
    pub column_id: String,
    /// The column's width in pixels.
    pub width: u32,
    /// Whether the column is currently visible.
    pub visible: bool,
}

impl From<ColumnConfiguration> for ColumnConfigurationDto {
    fn from(column: ColumnConfiguration) -> Self {
        Self {
            column_id: column.column_id,
            width: column.width,
            visible: column.visible,
        }
    }
}

impl From<ColumnConfigurationDto> for ColumnConfiguration {
    fn from(dto: ColumnConfigurationDto) -> Self {
        Self {
            column_id: dto.column_id,
            width: dto.width,
            visible: dto.visible,
        }
    }
}

/// A persisted quick-filter query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PersistedFilterDto {
    /// The filter's plain-text query.
    pub query: String,
}

impl From<PersistedFilter> for PersistedFilterDto {
    fn from(filter: PersistedFilter) -> Self {
        Self {
            query: filter.query,
        }
    }
}

impl From<PersistedFilterDto> for PersistedFilter {
    fn from(dto: PersistedFilterDto) -> Self {
        Self { query: dto.query }
    }
}

/// Back/forward navigation history for a single tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NavigationHistoryDto {
    /// Locations reachable by navigating back, most recent last.
    pub back: Vec<LocationDto>,
    /// Locations reachable by navigating forward, most recent last.
    pub forward: Vec<LocationDto>,
}

impl From<NavigationHistory> for NavigationHistoryDto {
    fn from(history: NavigationHistory) -> Self {
        Self {
            back: history.back.into_iter().map(Into::into).collect(),
            forward: history.forward.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<NavigationHistoryDto> for NavigationHistory {
    fn from(dto: NavigationHistoryDto) -> Self {
        Self {
            back: dto.back.into_iter().map(Into::into).collect(),
            forward: dto.forward.into_iter().map(Into::into).collect(),
        }
    }
}

/// The active view mode for a directory listing tab (task 0134).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub enum DirectoryViewModeDto {
    /// The existing dense, row-based listing.
    #[default]
    Table,
    /// A grid of larger thumbnails with the filename below.
    Grid,
}

impl From<DirectoryViewMode> for DirectoryViewModeDto {
    fn from(mode: DirectoryViewMode) -> Self {
        match mode {
            DirectoryViewMode::Table => Self::Table,
            DirectoryViewMode::Grid => Self::Grid,
        }
    }
}

impl From<DirectoryViewModeDto> for DirectoryViewMode {
    fn from(dto: DirectoryViewModeDto) -> Self {
        match dto {
            DirectoryViewModeDto::Table => Self::Table,
            DirectoryViewModeDto::Grid => Self::Grid,
        }
    }
}

/// Grid tile / icon size (task 0134).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub enum IconSizeDto {
    /// Icon-sized, for the directory table's icon column.
    Small,
    /// The default grid-view tile size.
    #[default]
    Medium,
    /// The largest grid-view tile size.
    Large,
}

impl From<IconSize> for IconSizeDto {
    fn from(size: IconSize) -> Self {
        match size {
            IconSize::Small => Self::Small,
            IconSize::Medium => Self::Medium,
            IconSize::Large => Self::Large,
        }
    }
}

impl From<IconSizeDto> for IconSize {
    fn from(dto: IconSizeDto) -> Self {
        match dto {
            IconSizeDto::Small => Self::Small,
            IconSizeDto::Medium => Self::Medium,
            IconSizeDto::Large => Self::Large,
        }
    }
}

/// Persisted view configuration for a directory listing: sorting, columns
/// and filters. Contains no frontend-only selection/cursor fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryViewConfigurationDto {
    /// The active sort descriptors, in priority order.
    pub sort: Vec<SortDescriptorDto>,
    /// Per-column width and visibility.
    pub columns: Vec<ColumnConfigurationDto>,
    /// Whether hidden entries are shown.
    pub show_hidden: bool,
    /// Whether directories are grouped before files.
    pub folders_first: bool,
    /// A persisted quick-filter query, if one is saved with the tab.
    pub quick_filter: Option<PersistedFilterDto>,
    /// The active view mode (task 0134).
    #[serde(default)]
    pub view_mode: DirectoryViewModeDto,
    /// Grid tile size, used only when `view_mode` is `grid` (task 0134).
    #[serde(default)]
    pub icon_size: IconSizeDto,
}

impl From<DirectoryViewConfiguration> for DirectoryViewConfigurationDto {
    fn from(view: DirectoryViewConfiguration) -> Self {
        Self {
            sort: view.sort.into_iter().map(Into::into).collect(),
            columns: view.columns.into_iter().map(Into::into).collect(),
            show_hidden: view.show_hidden,
            folders_first: view.folders_first,
            quick_filter: view.quick_filter.map(Into::into),
            view_mode: view.view_mode.into(),
            icon_size: view.icon_size.into(),
        }
    }
}

impl From<DirectoryViewConfigurationDto> for DirectoryViewConfiguration {
    fn from(dto: DirectoryViewConfigurationDto) -> Self {
        Self {
            sort: dto.sort.into_iter().map(Into::into).collect(),
            columns: dto.columns.into_iter().map(Into::into).collect(),
            show_hidden: dto.show_hidden,
            folders_first: dto.folders_first,
            quick_filter: dto.quick_filter.map(Into::into),
            view_mode: dto.view_mode.into(),
            icon_size: dto.icon_size.into(),
        }
    }
}

/// A single tab: a location, its navigation history and its view configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabStateDto {
    /// Stable identifier for this tab.
    pub id: Uuid,
    /// The location currently shown in this tab.
    pub location: LocationDto,
    /// An optional user-facing title override for this tab.
    pub title_override: Option<String>,
    /// Back/forward navigation history for this tab.
    pub history: NavigationHistoryDto,
    /// Persisted view configuration (sort, columns, filters) for this tab.
    pub view: DirectoryViewConfigurationDto,
    /// Whether this tab is pinned.
    pub pinned: bool,
    /// Whether this tab belongs only to the current application session.
    #[serde(default)]
    pub transient: bool,
}

impl From<TabState> for TabStateDto {
    fn from(tab: TabState) -> Self {
        Self {
            id: tab.id.into(),
            location: tab.location.into(),
            title_override: tab.title_override,
            history: tab.history.into(),
            view: tab.view.into(),
            pinned: tab.pinned,
            transient: tab.transient,
        }
    }
}

impl From<TabStateDto> for TabState {
    fn from(dto: TabStateDto) -> Self {
        Self {
            id: TabId::from(dto.id),
            location: dto.location.into(),
            title_override: dto.title_override,
            history: dto.history.into(),
            view: dto.view.into(),
            pinned: dto.pinned,
            transient: dto.transient,
        }
    }
}

/// A single pane, holding one or more tabs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaneStateDto {
    /// Stable identifier for this pane.
    pub id: Uuid,
    /// An optional user-facing title override for this pane.
    pub title: Option<String>,
    /// The tabs open in this pane.
    pub tabs: Vec<TabStateDto>,
    /// The tab currently shown in this pane.
    pub active_tab_id: Uuid,
    /// The view configuration new tabs in this pane start from.
    pub default_view: DirectoryViewConfigurationDto,
}

impl From<PaneState> for PaneStateDto {
    fn from(pane: PaneState) -> Self {
        Self {
            id: pane.id.into(),
            title: pane.title,
            tabs: pane.tabs.into_iter().map(Into::into).collect(),
            active_tab_id: pane.active_tab_id.into(),
            default_view: pane.default_view.into(),
        }
    }
}

impl From<PaneStateDto> for PaneState {
    fn from(dto: PaneStateDto) -> Self {
        Self {
            id: PaneId::from(dto.id),
            title: dto.title,
            tabs: dto.tabs.into_iter().map(Into::into).collect(),
            active_tab_id: TabId::from(dto.active_tab_id),
            default_view: dto.default_view.into(),
        }
    }
}

/// The axis a [`WorkspaceLayoutDto::Split`] is arranged on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SplitAxisDto {
    /// Side by side, splitter runs vertically.
    Horizontal,
    /// Stacked, splitter runs horizontally.
    Vertical,
}

impl From<SplitAxis> for SplitAxisDto {
    fn from(axis: SplitAxis) -> Self {
        match axis {
            SplitAxis::Horizontal => Self::Horizontal,
            SplitAxis::Vertical => Self::Vertical,
        }
    }
}

impl From<SplitAxisDto> for SplitAxis {
    fn from(axis: SplitAxisDto) -> Self {
        match axis {
            SplitAxisDto::Horizontal => Self::Horizontal,
            SplitAxisDto::Vertical => Self::Vertical,
        }
    }
}

/// How a workspace's panes are arranged on screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkspaceLayoutDto {
    /// A leaf holding a single pane.
    #[serde(rename_all = "camelCase")]
    Pane {
        /// The pane occupying this leaf.
        pane_id: Uuid,
    },
    /// Two regions separated by a single draggable splitter.
    #[serde(rename_all = "camelCase")]
    Split {
        /// The axis the splitter is arranged on.
        axis: SplitAxisDto,
        /// The fraction of space (0.0-1.0) given to `first`.
        ratio: f32,
        /// The first (left or top) region.
        #[schema(no_recursion)]
        first: Box<WorkspaceLayoutDto>,
        /// The second (right or bottom) region.
        #[schema(no_recursion)]
        second: Box<WorkspaceLayoutDto>,
    },
}

impl From<WorkspaceLayout> for WorkspaceLayoutDto {
    fn from(layout: WorkspaceLayout) -> Self {
        match layout {
            WorkspaceLayout::Pane { pane_id } => Self::Pane {
                pane_id: pane_id.into(),
            },
            WorkspaceLayout::Split {
                axis,
                ratio,
                first,
                second,
            } => Self::Split {
                axis: axis.into(),
                ratio,
                first: Box::new((*first).into()),
                second: Box::new((*second).into()),
            },
        }
    }
}

impl From<WorkspaceLayoutDto> for WorkspaceLayout {
    fn from(dto: WorkspaceLayoutDto) -> Self {
        match dto {
            WorkspaceLayoutDto::Pane { pane_id } => Self::Pane {
                pane_id: PaneId::from(pane_id),
            },
            WorkspaceLayoutDto::Split {
                axis,
                ratio,
                first,
                second,
            } => Self::Split {
                axis: axis.into(),
                ratio,
                first: Box::new((*first).into()),
                second: Box::new((*second).into()),
            },
        }
    }
}

/// Workspace-level operation-centre visibility and sizing preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationCentrePreferencesDto {
    /// Whether the operation centre panel is visible.
    pub visible: bool,
    /// The panel's height in pixels.
    pub height: u32,
}

impl From<OperationCentrePreferences> for OperationCentrePreferencesDto {
    fn from(preferences: OperationCentrePreferences) -> Self {
        Self {
            visible: preferences.visible,
            height: preferences.height,
        }
    }
}

impl From<OperationCentrePreferencesDto> for OperationCentrePreferences {
    fn from(dto: OperationCentrePreferencesDto) -> Self {
        Self {
            visible: dto.visible,
            height: dto.height,
        }
    }
}

/// A workspace: a named collection of panes arranged in a
/// [`WorkspaceLayoutDto`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "schemaVersion": 1,
    "id": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "name": "Default",
    "layout": {"type": "pane", "paneId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b"},
    "panes": [],
    "activePaneId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "operationCentre": {"visible": true, "height": 180},
    "createdAt": "2026-07-29T18:00:00Z",
    "updatedAt": "2026-07-29T18:40:00Z",
    "revision": 1,
    "ephemeral": false
}))]
pub struct WorkspaceDto {
    /// Version of the persisted workspace schema, for migrations.
    pub schema_version: u32,
    /// Stable identifier for this workspace.
    pub id: Uuid,
    /// A user-facing name for this workspace.
    pub name: String,
    /// How the panes are arranged on screen.
    pub layout: WorkspaceLayoutDto,
    /// The panes making up this workspace.
    pub panes: Vec<PaneStateDto>,
    /// The pane that currently has focus.
    pub active_pane_id: Uuid,
    /// Operation-centre visibility and sizing preferences for this workspace.
    pub operation_centre: OperationCentrePreferencesDto,
    /// When this workspace was first created.
    pub created_at: DateTime<Utc>,
    /// When this workspace was last persisted.
    pub updated_at: DateTime<Utc>,
    /// Monotonically increasing revision, used for optimistic conflict checks.
    pub revision: u64,
    /// True for a per-window fork created for one desktop window's private use, false
    /// for a named/template workspace that only changes when explicitly resynced.
    ///
    /// Defaults to `false` when absent so workspace files persisted before this field
    /// existed still deserialize.
    #[serde(default)]
    pub ephemeral: bool,
    /// The named workspace this ephemeral workspace was forked from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<Uuid>,
}

impl From<Workspace> for WorkspaceDto {
    fn from(workspace: Workspace) -> Self {
        Self {
            schema_version: workspace.schema_version,
            id: workspace.id.into(),
            name: workspace.name,
            layout: workspace.layout.into(),
            panes: workspace.panes.into_iter().map(Into::into).collect(),
            active_pane_id: workspace.active_pane_id.into(),
            operation_centre: workspace.operation_centre.into(),
            created_at: workspace.created_at,
            updated_at: workspace.updated_at,
            revision: workspace.revision,
            ephemeral: workspace.ephemeral,
            forked_from: workspace.forked_from.map(Into::into),
        }
    }
}

impl From<WorkspaceDto> for Workspace {
    fn from(dto: WorkspaceDto) -> Self {
        Self {
            schema_version: dto.schema_version,
            id: WorkspaceId::from(dto.id),
            name: dto.name,
            layout: dto.layout.into(),
            panes: dto.panes.into_iter().map(Into::into).collect(),
            active_pane_id: PaneId::from(dto.active_pane_id),
            operation_centre: dto.operation_centre.into(),
            created_at: dto.created_at,
            updated_at: dto.updated_at,
            revision: dto.revision,
            ephemeral: dto.ephemeral,
            forked_from: dto.forked_from.map(WorkspaceId::from),
        }
    }
}

/// A lightweight projection of a stored workspace, cheap enough to list
/// without loading every pane and tab (spec §5.3.8, `GET /api/v1/workspaces`).
///
/// The conversion from `fm_application::workspace::WorkspaceSummary` lives in
/// `fm-application` (not here): that type is defined in a higher layer this
/// crate must never depend on, and Rust's orphan rules permit the impl to
/// live on either side as long as one of the two types is local to the
/// crate writing it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "id": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "name": "Default",
    "updatedAt": "2026-07-29T18:40:00Z",
    "revision": 1,
    "ephemeral": false
}))]
pub struct WorkspaceSummaryDto {
    /// The workspace's id.
    pub id: Uuid,
    /// The workspace's user-facing name.
    pub name: String,
    /// When the workspace was last persisted.
    pub updated_at: DateTime<Utc>,
    /// The workspace's current revision.
    pub revision: u64,
    /// True for a per-window fork, excluded from the workspace switcher.
    pub ephemeral: bool,
}

/// Requests a new workspace (`POST /api/v1/workspaces`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({"name": "Photos"}))]
pub struct CreateWorkspaceRequestDto {
    /// The new workspace's name; defaults to `"Default"` when omitted.
    #[serde(default)]
    pub name: Option<String>,
}

#[cfg(test)]
mod tests {
    use fm_domain::{Location, ProviderId};

    use super::*;

    fn sample_view() -> DirectoryViewConfiguration {
        DirectoryViewConfiguration {
            sort: vec![SortDescriptor {
                column_id: "core.name".to_owned(),
                direction: SortDirection::Ascending,
            }],
            columns: vec![ColumnConfiguration {
                column_id: "core.name".to_owned(),
                width: 360,
                visible: true,
            }],
            show_hidden: true,
            folders_first: true,
            quick_filter: None,
            view_mode: DirectoryViewMode::Table,
            icon_size: IconSize::Medium,
        }
    }

    fn sample_workspace() -> Workspace {
        let pane_a = PaneId::new();
        let pane_b = PaneId::new();
        let tab = TabState {
            id: TabId::new(),
            location: Location::new(ProviderId::new("local"), "file:///Users/erik"),
            title_override: None,
            history: NavigationHistory {
                back: vec![],
                forward: vec![],
            },
            view: sample_view(),
            pinned: false,
            transient: false,
        };
        Workspace {
            schema_version: 1,
            id: WorkspaceId::new(),
            name: "Default".to_owned(),
            panes: vec![
                PaneState {
                    id: pane_a,
                    title: None,
                    tabs: vec![tab.clone()],
                    active_tab_id: tab.id,
                    default_view: sample_view(),
                },
                PaneState {
                    id: pane_b,
                    title: None,
                    tabs: vec![tab.clone()],
                    active_tab_id: tab.id,
                    default_view: sample_view(),
                },
            ],
            active_pane_id: pane_a,
            layout: WorkspaceLayout::Split {
                axis: SplitAxis::Horizontal,
                ratio: 0.5,
                first: Box::new(WorkspaceLayout::Pane { pane_id: pane_a }),
                second: Box::new(WorkspaceLayout::Pane { pane_id: pane_b }),
            },
            operation_centre: OperationCentrePreferences {
                visible: true,
                height: 180,
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
            revision: 1,
            ephemeral: false,
            forked_from: None,
        }
    }

    #[test]
    fn workspace_dto_round_trips_through_serde_json() {
        let dto: WorkspaceDto = sample_workspace().into();
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        let parsed: WorkspaceDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(dto, parsed);
    }

    #[test]
    fn workspace_dto_uses_camel_case_field_names() {
        let dto: WorkspaceDto = sample_workspace().into();
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        for field in [
            "\"schemaVersion\"",
            "\"activePaneId\"",
            "\"panes\"",
            "\"layout\"",
            "\"operationCentre\"",
            "\"createdAt\"",
            "\"updatedAt\"",
            "\"revision\"",
        ] {
            assert!(json.contains(field), "expected {json} to contain {field}");
        }
    }

    #[test]
    fn directory_view_configuration_dto_uses_camel_case_view_mode_and_icon_size() {
        let dto: DirectoryViewConfigurationDto = sample_view().into();
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        assert!(json.contains("\"viewMode\":\"table\""), "{json}");
        assert!(json.contains("\"iconSize\":\"medium\""), "{json}");
    }

    #[test]
    fn directory_view_configuration_dto_round_trips_the_grid_view_mode() {
        let dto = DirectoryViewConfigurationDto {
            view_mode: DirectoryViewModeDto::Grid,
            icon_size: IconSizeDto::Large,
            ..sample_view().into()
        };
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        let parsed: DirectoryViewConfigurationDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(dto, parsed);
        assert!(json.contains("\"viewMode\":\"grid\""), "{json}");
        assert!(json.contains("\"iconSize\":\"large\""), "{json}");
    }

    #[test]
    fn directory_view_configuration_dto_defaults_view_mode_and_icon_size_for_pre_0134_json() {
        let json = serde_json::json!({
            "sort": [],
            "columns": [],
            "showHidden": false,
            "foldersFirst": false,
            "quickFilter": null,
        });

        let parsed: DirectoryViewConfigurationDto =
            serde_json::from_value(json).expect("pre-0134 JSON must still deserialize");
        assert_eq!(parsed.view_mode, DirectoryViewModeDto::Table);
        assert_eq!(parsed.icon_size, IconSizeDto::Medium);
    }

    #[test]
    fn workspace_layout_dto_uses_a_string_discriminator_and_supports_nested_splits() {
        let layout = WorkspaceLayout::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.33,
            first: Box::new(WorkspaceLayout::Pane {
                pane_id: PaneId::new(),
            }),
            second: Box::new(WorkspaceLayout::Split {
                axis: SplitAxis::Vertical,
                ratio: 0.5,
                first: Box::new(WorkspaceLayout::Pane {
                    pane_id: PaneId::new(),
                }),
                second: Box::new(WorkspaceLayout::Pane {
                    pane_id: PaneId::new(),
                }),
            }),
        };

        let dto: WorkspaceLayoutDto = layout.clone().into();
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        assert!(json.contains("\"type\":\"split\""));

        let parsed: WorkspaceLayoutDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        let round_tripped: WorkspaceLayout = parsed.into();
        assert_eq!(layout, round_tripped);
        assert!(json.contains("\"axis\":\"horizontal\""));
    }

    #[test]
    fn workspace_dto_converts_to_and_from_the_domain_type() {
        let workspace = sample_workspace();
        let dto: WorkspaceDto = workspace.clone().into();
        let round_tripped: Workspace = dto.into();
        assert_eq!(workspace, round_tripped);
    }

    /// Verbatim JSON from spec §5.3.15 — this is the wire/persisted format
    /// `WorkspaceDto` is responsible for, unlike `fm_domain::Workspace`'s
    /// snake_case internal representation.
    const SPEC_EXAMPLE_JSON: &str = r#"{
      "schemaVersion": 1,
      "id": "985d4d6e-c37b-4135-90a0-ce0afe165fd9",
      "name": "Development",
      "revision": 12,
      "layout": {
        "type": "split",
        "axis": "horizontal",
        "ratio": 0.52,
        "first": { "type": "pane", "paneId": "11e67e3e-813c-44c5-9426-53be347ad5da" },
        "second": { "type": "pane", "paneId": "479ec0f0-0ea6-4a34-b67e-f654373596af" }
      },
      "panes": [
        {
          "id": "11e67e3e-813c-44c5-9426-53be347ad5da",
          "title": null,
          "activeTabId": "97512c58-9cf8-4f17-a931-94f0be87a1da",
          "defaultView": {
            "sort": [{ "columnId": "core.name", "direction": "ascending" }],
            "columns": [
              { "columnId": "core.name", "width": 360, "visible": true },
              { "columnId": "core.size", "width": 100, "visible": true },
              { "columnId": "core.modified", "width": 170, "visible": true }
            ],
            "showHidden": true,
            "foldersFirst": true,
            "quickFilter": null
          },
          "tabs": [
            {
              "id": "97512c58-9cf8-4f17-a931-94f0be87a1da",
              "location": { "providerId": "local", "uri": "file:///Users/erik/dev" },
              "titleOverride": null,
              "history": { "back": [], "forward": [] },
              "view": {
                "sort": [{ "columnId": "core.name", "direction": "ascending" }],
                "columns": [
                  { "columnId": "core.name", "width": 360, "visible": true },
                  { "columnId": "core.size", "width": 100, "visible": true },
                  { "columnId": "core.modified", "width": 170, "visible": true }
                ],
                "showHidden": true,
                "foldersFirst": true,
                "quickFilter": null
              },
              "pinned": false
            }
          ]
        },
        {
          "id": "479ec0f0-0ea6-4a34-b67e-f654373596af",
          "title": null,
          "activeTabId": "5e8be42f-d6ef-45fb-89ea-d77122076bc3",
          "defaultView": {
            "sort": [{ "columnId": "core.modified", "direction": "descending" }],
            "columns": [
              { "columnId": "core.name", "width": 340, "visible": true },
              { "columnId": "core.size", "width": 100, "visible": true },
              { "columnId": "core.modified", "width": 170, "visible": true }
            ],
            "showHidden": false,
            "foldersFirst": true,
            "quickFilter": null
          },
          "tabs": [
            {
              "id": "5e8be42f-d6ef-45fb-89ea-d77122076bc3",
              "location": { "providerId": "local", "uri": "file:///Users/erik/Downloads" },
              "titleOverride": null,
              "history": { "back": [], "forward": [] },
              "view": {
                "sort": [{ "columnId": "core.modified", "direction": "descending" }],
                "columns": [
                  { "columnId": "core.name", "width": 340, "visible": true },
                  { "columnId": "core.size", "width": 100, "visible": true },
                  { "columnId": "core.modified", "width": 170, "visible": true }
                ],
                "showHidden": false,
                "foldersFirst": true,
                "quickFilter": null
              },
              "pinned": false
            }
          ]
        }
      ],
      "activePaneId": "11e67e3e-813c-44c5-9426-53be347ad5da",
      "operationCentre": { "visible": true, "height": 180 },
      "createdAt": "2026-07-29T18:00:00+02:00",
      "updatedAt": "2026-07-29T18:40:00+02:00"
    }"#;

    #[test]
    fn workspace_dto_round_trips_against_the_literal_spec_example_json() {
        let dto: WorkspaceDto = serde_json::from_str(SPEC_EXAMPLE_JSON)
            .expect("the §5.3.15 example must deserialize into WorkspaceDto");

        assert_eq!(dto.schema_version, 1);
        assert_eq!(dto.name, "Development");
        assert_eq!(dto.revision, 12);
        assert_eq!(dto.panes.len(), 2);
        assert!(dto.operation_centre.visible);
        assert_eq!(dto.operation_centre.height, 180);
        assert_eq!(dto.panes[0].tabs[0].view.sort[0].column_id, "core.name");
        assert!(!dto.panes[0].tabs[0].pinned);
        assert_eq!(dto.panes[0].tabs[0].location.provider_id, "local");

        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        let round_tripped: WorkspaceDto =
            serde_json::from_str(&json).expect("re-deserialization must succeed");
        assert_eq!(dto, round_tripped);

        // Also confirm the domain round trip via the Dto<->domain conversions.
        let domain: Workspace = dto.into();
        let back_to_dto: WorkspaceDto = domain.into();
        assert_eq!(back_to_dto, round_tripped);
    }

    #[test]
    fn workspace_summary_dto_round_trips_and_uses_camel_case() {
        let summary = WorkspaceSummaryDto {
            id: Uuid::new_v4(),
            name: "Default".to_owned(),
            updated_at: Utc::now(),
            revision: 3,
            ephemeral: false,
        };
        let json = serde_json::to_string(&summary).expect("serialization must succeed");
        assert!(json.contains("\"updatedAt\""));
        let parsed: WorkspaceSummaryDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(summary, parsed);
    }

    #[test]
    fn create_workspace_request_dto_defaults_the_name_to_none() {
        let json = "{}";
        let request: CreateWorkspaceRequestDto =
            serde_json::from_str(json).expect("an empty body must deserialize");
        assert_eq!(request.name, None);
    }
}
