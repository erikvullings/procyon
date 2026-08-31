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
    /// Durable smart folders, ordered with pinned searches first by the UI.
    pub saved_searches: Vec<SavedSearchDto>,
    /// Directory-entry icon set: `"generic"` for the built-in glyphs, or a discovered plugin's id.
    pub icon_theme: String,
}
