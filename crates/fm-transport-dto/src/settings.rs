//! Application-wide settings wire contract (specification §26).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{LocationDto, SearchQueryDto};

/// Application colour theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ThemeDto {
    /// Follow the operating system.
    Auto,
    /// Light colours.
    Light,
    /// Dark colours.
    Dark,
}

/// UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum LanguageDto {
    /// English; also the fallback locale.
    En,
    /// Dutch.
    Nl,
    /// German.
    De,
    /// French.
    Fr,
    /// Spanish.
    Es,
    /// Italian.
    It,
    /// Portuguese.
    Pt,
    /// Polish.
    Pl,
}

/// Timestamp presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DateFormatDto {
    /// Compact locale-aware format.
    Short,
    /// Descriptive locale-aware format.
    Medium,
    /// ISO-8601.
    Iso,
}

/// File-size presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SizeFormatDto {
    /// Powers of 1024.
    Binary,
    /// Powers of 1000.
    Decimal,
    /// Raw bytes.
    Bytes,
}

/// Default operation conflict choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicyDto {
    /// Ask the user.
    Ask,
    /// Replace the destination.
    Overwrite,
    /// Keep both entries.
    KeepBoth,
    /// Skip the source.
    Skip,
}

/// Layout inherited by a new workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DefaultPaneLayoutDto {
    /// Two panes.
    Dual,
    /// One pane.
    Single,
}

/// A named, provider-neutral location saved by the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FavouriteLocationDto {
    /// User-visible label, independent of the location URI.
    pub label: String,
    /// Provider-neutral location target.
    pub location: LocationDto,
}

/// How a multi-rename rule cases the composed filename.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum MultiRenameCaseTransformDto {
    /// Preserve the composed casing.
    Unchanged,
    /// Convert the whole filename to uppercase.
    Upper,
    /// Convert the whole filename to lowercase.
    Lower,
    /// Title-case the filename stem.
    Title,
}

/// Counter settings used by a multi-rename preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MultiRenameSequenceDto {
    /// First counter value.
    pub start: i64,
    /// Increment between selected entries.
    pub step: i64,
    /// Minimum number of displayed digits.
    pub padding: u32,
}

/// Complete multi-rename rule configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MultiRenameRulesDto {
    /// Search pattern.
    pub search: String,
    /// Replacement text.
    pub replace: String,
    /// Whether `search` is interpreted as a regular expression.
    pub use_regex: bool,
    /// Filename-stem mask.
    pub name_mask: String,
    /// Extension mask.
    pub extension_mask: String,
    /// Counter configuration.
    pub sequence: MultiRenameSequenceDto,
    /// Final casing operation.
    pub case_transform: MultiRenameCaseTransformDto,
}

/// A user-named reusable multi-rename rule configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MultiRenamePresetDto {
    /// Unique user-visible preset name.
    pub name: String,
    /// Rules restored when the preset is loaded.
    pub rules: MultiRenameRulesDto,
}

/// Entry-kind constraint of a saved pane filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum FilterKindDto {
    /// Files, directories and symlinks.
    Any,
    /// Regular files only.
    File,
    /// Directories only.
    Directory,
    /// Symbolic links only.
    Symlink,
}

/// Tri-state constraint of a saved pane filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum FilterToggleDto {
    /// No constraint.
    Any,
    /// Only entries that have the property.
    Only,
    /// Only entries that lack the property.
    Exclude,
}

/// Structured pane-filter criteria applied to the loaded directory listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilterCriteriaDto {
    /// `;`-separated name masks (substring or `*`/`?` glob).
    pub name: String,
    /// Entry kind constraint.
    pub kind: FilterKindDto,
    /// Smallest size in bytes.
    pub min_size: Option<u64>,
    /// Largest size in bytes.
    pub max_size: Option<u64>,
    /// Keep entries modified within this many days.
    pub modified_within_days: Option<u64>,
    /// Keep entries last modified more than this many days ago.
    pub modified_older_than_days: Option<u64>,
    /// Hidden-state constraint.
    pub hidden: FilterToggleDto,
    /// Executable-status constraint.
    pub executable: FilterToggleDto,
}

/// A user-named reusable pane filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilterPresetDto {
    /// Unique user-visible preset name.
    pub name: String,
    /// Criteria restored when the preset is loaded.
    pub criteria: FilterCriteriaDto,
}

/// A user-named reusable structured search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub struct SavedSearchDto {
    pub id: uuid::Uuid,
    pub name: String,
    pub pinned: bool,
    pub query: SearchQueryDto,
}

/// Versioned global settings. Live workspace content is deliberately absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    /// On-disk schema version.
    pub schema_version: u32,
    /// Application theme.
    pub theme: ThemeDto,
    /// UI language.
    pub language: LanguageDto,
    /// Base font size in CSS pixels.
    pub font_size: u16,
    /// Directory row height in CSS pixels.
    pub row_height: u16,
    /// Timestamp presentation.
    pub date_format: DateFormatDto,
    /// Size presentation.
    pub size_format: SizeFormatDto,
    /// Show hidden entries by default.
    pub show_hidden_files: bool,
    /// Confirm permanent deletion.
    pub confirm_permanent_delete: bool,
    /// Confirm routine copy, move, and Trash operations before they start.
    pub confirm_file_operations: bool,
    /// Default operation conflict policy.
    pub default_conflict_policy: ConflictPolicyDto,
    /// Maximum concurrent operations.
    pub operation_concurrency: u16,
    /// Layout inherited by new workspaces.
    pub default_pane_layout: DefaultPaneLayoutDto,
    /// Columns inherited by new tabs.
    pub default_columns: Vec<String>,
    /// Column widths in CSS pixels, keyed by column id, shared by every tab and pane.
    pub column_widths: BTreeMap<String, u32>,
    /// Action-to-shortcut mappings.
    pub keybindings: BTreeMap<String, String>,
    /// Enabled plugin identifiers.
    pub enabled_plugins: Vec<String>,
    /// Non-secret plugin settings keyed by plugin identifier.
    #[schema(value_type = Object)]
    pub plugin_settings: Value,
    /// Optional terminal command.
    pub terminal_command: Option<String>,
    /// Optional text-editor command for `core.edit`; `None` uses the platform default.
    pub editor_command: Option<String>,
    /// Locations inherited by new panes.
    pub default_start_locations: Vec<String>,
    /// User-managed named locations, in the order shown by the favourites menu.
    pub favourite_locations: Vec<FavouriteLocationDto>,
    /// Recently visited locations per workspace, newest first.
    pub recent_locations_by_workspace: BTreeMap<String, Vec<LocationDto>>,
    /// User-named reusable multi-rename configurations.
    pub multi_rename_presets: Vec<MultiRenamePresetDto>,
    /// User-named reusable pane filters.
    #[serde(default)]
    pub filter_presets: Vec<FilterPresetDto>,
    /// Durable smart folders, ordered with pinned searches first by the UI.
    pub saved_searches: Vec<SavedSearchDto>,
    /// Directory-entry icon set: `"generic"` for the built-in glyphs, or a discovered plugin's id.
    pub icon_theme: String,
    /// Whether the desktop host checks for signed application updates after startup.
    pub check_for_updates_automatically: bool,
    /// Whether update checks also offer beta releases (not yet approved for the stable channel).
    #[serde(default)]
    pub check_for_beta_updates: bool,
    /// Whether F3 opens the viewer in its own window instead of a tab in the active pane.
    pub viewer_in_separate_window: bool,
    /// Whether the view a user picks for a folder is remembered and restored on the next visit
    /// (task 0270). The remembered views themselves are backend-owned and not part of this DTO.
    #[serde(default = "remember_folder_views_default")]
    pub remember_folder_views: bool,
    /// Basemap of the shared map component (task 0275).
    #[serde(default)]
    pub map: MapSettingsDto,
}

/// Where basemap imagery comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum MapBasemapKindDto {
    /// A raster tile URL template, fetched by the backend.
    Raster,
    /// A MapLibre style URL fetched directly by the web view.
    Style,
    /// A local raster PMTiles file read through the VFS.
    Pmtiles,
}

/// Map component settings (task 0275). Only the fields of the selected `basemapKind` are used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MapSettingsDto {
    /// Whether requests to remote tile or style hosts are allowed at all.
    pub online_tiles: bool,
    /// Selected basemap kind.
    pub basemap_kind: MapBasemapKindDto,
    /// Raster tile template with `{z}`, `{x}` and `{y}`.
    pub url_template: String,
    /// MapLibre style document URL.
    pub style_url: String,
    /// Local `.pmtiles` file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pmtiles_location: Option<LocationDto>,
    /// Attribution text shown on the map.
    pub attribution: String,
    /// Highest zoom level a raster provider serves.
    pub max_zoom: u8,
    /// Remote host that receives requests for the selected basemap; read-only, derived by the
    /// backend. Absent for local files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_host: Option<String>,
}

impl Default for MapSettingsDto {
    fn default() -> Self {
        Self {
            online_tiles: true,
            basemap_kind: MapBasemapKindDto::Raster,
            url_template: "https://tile.openstreetmap.org/{z}/{x}/{y}.png".to_owned(),
            style_url: String::new(),
            pmtiles_location: None,
            attribution: "\u{a9} OpenStreetMap contributors".to_owned(),
            max_zoom: 19,
            remote_host: Some("tile.openstreetmap.org".to_owned()),
        }
    }
}

fn remember_folder_views_default() -> bool {
    true
}
