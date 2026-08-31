//! Plugin discovery DTOs shared by REST and Tauri.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// A declarative custom column made available by an enabled plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PluginColumnDto {
    /// Plugin-namespaced column identifier.
    pub id: String,
    /// User-facing column label.
    pub title: String,
}

/// A discovered plugin, including disabled plugins with safe diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PluginDescriptorDto {
    /// Stable manifest identifier, or a directory-derived identifier when invalid.
    pub id: String,
    /// User-facing name when available.
    pub name: String,
    /// Package version when available.
    pub version: String,
    /// Manifest description when available.
    pub description: String,
    /// Whether the valid plugin is enabled in persisted settings.
    pub enabled: bool,
    /// Validation or discovery diagnostic for disabled plugins.
    pub diagnostic: Option<String>,
    /// Data-only column declarations that the host can render safely.
    pub columns: Vec<PluginColumnDto>,
    /// Capabilities the manifest requests; ungranted capabilities are denied (spec §19).
    pub permissions: PluginPermissionsDto,
    /// The distributable icon theme this plugin contributes, when enabled and valid (task 0095).
    pub icon_theme: Option<PluginIconThemeDto>,
}

/// One icon asset a theme can reference, resolved by `GET
/// /api/v1/plugins/{pluginId}/icon-theme/asset?path=...` (or the matching Tauri command).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PluginIconDefinitionDto {
    /// SVG asset path, relative to the plugin directory; pass verbatim to the asset route.
    pub icon_path: String,
}

/// A distributable directory-entry icon theme contributed by a plugin (task 0095), read from
/// that plugin's `icon-theme.json`. Mirrors `fm_plugin_api::IconThemeManifest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PluginIconThemeDto {
    /// Every icon asset this theme can reference, keyed by a theme-local name.
    pub icon_definitions: BTreeMap<String, PluginIconDefinitionDto>,
    /// Default icon definition key for `file` entries.
    pub file: Option<String>,
    /// Default icon definition key for `directory` entries.
    pub folder: Option<String>,
    /// Default icon definition key for `symlink` entries.
    pub symlink: Option<String>,
    /// Lowercased, dot-less file extension to icon definition key.
    pub file_extensions: BTreeMap<String, String>,
    /// Exact file name to icon definition key, matched before `file_extensions`.
    pub file_names: BTreeMap<String, String>,
    /// Exact directory name (e.g. `".git"`) to icon definition key.
    pub folder_names: BTreeMap<String, String>,
    /// Exact expanded directory name to icon definition key.
    pub folder_names_expanded: BTreeMap<String, String>,
    /// MIME type prefix (e.g. `"image/"`) to icon definition key.
    pub mime_prefixes: BTreeMap<String, String>,
}

/// The manifest-declared capability grants for one plugin (spec §19), mirroring
/// `fm_plugin_api::PluginPermissions`. A field is denied when it is `false` or empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermissionsDto {
    /// Allows metadata for the current selection.
    pub selected_entry_metadata: bool,
    /// Allows bounded content reads for the current selection.
    pub selected_entry_content_read: bool,
    /// Roots the plugin may read from; denied when empty.
    pub filesystem_read: Vec<String>,
    /// Roots the plugin may write to; denied when empty.
    pub filesystem_write: Vec<String>,
    /// Allows reading from the clipboard.
    pub clipboard_read: bool,
    /// Allows writing to the clipboard.
    pub clipboard_write: bool,
    /// Network host allow-list; denied when empty.
    pub network: Vec<String>,
    /// Allows process spawning through a future restricted host service.
    pub process_spawn: bool,
    /// Allows non-blocking host notifications.
    pub notifications: bool,
    /// Allows non-secret settings storage under the plugin's identifier.
    pub settings_storage: bool,
}

/// One bounded diagnostic log entry retained for a plugin (spec §19.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PluginLogEntryDto {
    /// A safe, user-readable failure message.
    pub message: String,
}
