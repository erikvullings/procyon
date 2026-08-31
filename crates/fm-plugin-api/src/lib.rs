//! The contract plugins are written against (task 0053).
//!
//! Deliberately free of unstable Rust ABI types so that plugins can later be
//! isolated in WebAssembly without changing the interface they see.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The only plugin ABI revision accepted by this release.
pub const API_VERSION: &str = "1";

/// A versioned plugin manifest, read from `plugin.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    /// Stable, reverse-domain-style plugin identifier.
    pub id: String,
    /// Display name shown to users.
    pub name: String,
    /// Plugin package version.
    pub version: String,
    /// Version of this stable plugin API.
    pub api_version: String,
    /// User-facing description.
    pub description: String,
    /// Plugin entrypoint relative to the manifest directory. Required only when
    /// `contributions.actions` or `contributions.columns` is set — a plugin that
    /// contributes only an icon theme runs no code and needs no entrypoint.
    #[serde(default)]
    pub entrypoint: Option<PathBuf>,
    /// Explicit capability grants; omitted capabilities are denied.
    #[serde(default)]
    pub permissions: PluginPermissions,
    /// Declarative contributions; arbitrary WebView UI is intentionally absent.
    #[serde(default)]
    pub contributions: PluginContributions,
}

impl PluginManifest {
    /// Parses and validates a manifest document.
    pub fn parse(source: &str) -> Result<Self, ManifestError> {
        let manifest: Self = toml::from_str(source).map_err(ManifestError::Toml)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validates stable schema invariants after deserialization.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.id.trim().is_empty() || self.id.contains(char::is_whitespace) {
            return Err(ManifestError::InvalidField("id"));
        }
        if self.name.trim().is_empty()
            || self.version.trim().is_empty()
            || self.description.trim().is_empty()
        {
            return Err(ManifestError::InvalidField(
                "name, version, and description",
            ));
        }
        if self.api_version != API_VERSION {
            return Err(ManifestError::UnsupportedApiVersion(
                self.api_version.clone(),
            ));
        }
        let runs_code = self.contributions.actions || self.contributions.columns;
        match &self.entrypoint {
            Some(entrypoint)
                if entrypoint.as_os_str().is_empty() || is_absolute_on_any_platform(entrypoint) =>
            {
                return Err(ManifestError::InvalidField("entrypoint"));
            }
            None if runs_code => return Err(ManifestError::InvalidField("entrypoint")),
            _ => {}
        }
        Ok(())
    }
}

/// Manifest parsing or validation failure.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// The TOML does not conform to the versioned schema.
    #[error("invalid plugin manifest: {0}")]
    Toml(#[source] toml::de::Error),
    /// The manifest declares an API revision this host does not support.
    #[error("unsupported plugin api_version {0:?}; supported version is {API_VERSION:?}")]
    UnsupportedApiVersion(String),
    /// A required field is empty or unsafe.
    #[error("invalid plugin manifest field: {0}")]
    InvalidField(&'static str),
}

/// Explicit plugin permission grants. Every field defaults to denial.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginPermissions {
    /// Allows metadata for the current selection.
    pub selected_entry_metadata: bool,
    /// Allows bounded content reads for the current selection.
    pub selected_entry_content_read: bool,
    /// Roots the plugin may read from.
    pub filesystem_read: Vec<PathBuf>,
    /// Roots the plugin may write to.
    pub filesystem_write: Vec<PathBuf>,
    /// Allows reading from the clipboard.
    pub clipboard_read: bool,
    /// Allows writing to the clipboard.
    pub clipboard_write: bool,
    /// Network host allow-list.
    pub network: Vec<String>,
    /// Allows process spawning through a future restricted host service.
    pub process_spawn: bool,
    /// Allows non-blocking host notifications.
    pub notifications: bool,
    /// Allows non-secret settings storage under the plugin's identifier.
    pub settings_storage: bool,
}

/// A host operation guarded by the manifest permission model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Selected-entry metadata.
    SelectedEntryMetadata,
    /// Selected-entry content.
    SelectedEntryContentRead,
    /// Filesystem reads.
    FilesystemRead,
    /// Filesystem writes.
    FilesystemWrite,
    /// Clipboard reads.
    ClipboardRead,
    /// Clipboard writes.
    ClipboardWrite,
    /// Network requests.
    Network,
    /// Process execution.
    ProcessSpawn,
    /// Host notifications.
    Notifications,
    /// Plugin settings storage.
    SettingsStorage,
}

/// A typed, safe denial returned by the host boundary.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("plugin permission denied: {permission:?}")]
pub struct PermissionDenied {
    /// The capability that was not granted.
    pub permission: Permission,
}

impl PluginPermissions {
    /// Ensures an unscoped host request has been declared by the plugin.
    pub fn require(&self, permission: Permission) -> Result<(), PermissionDenied> {
        let granted = match permission {
            Permission::SelectedEntryMetadata => self.selected_entry_metadata,
            Permission::SelectedEntryContentRead => self.selected_entry_content_read,
            Permission::FilesystemRead => !self.filesystem_read.is_empty(),
            Permission::FilesystemWrite => !self.filesystem_write.is_empty(),
            Permission::ClipboardRead => self.clipboard_read,
            Permission::ClipboardWrite => self.clipboard_write,
            Permission::Network => !self.network.is_empty(),
            Permission::ProcessSpawn => self.process_spawn,
            Permission::Notifications => self.notifications,
            Permission::SettingsStorage => self.settings_storage,
        };
        granted.then_some(()).ok_or(PermissionDenied { permission })
    }
}

/// The only declarative contribution families available in API version 1.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginContributions {
    /// Action, context-menu, and command-palette contributions.
    pub actions: bool,
    /// Custom data columns.
    pub columns: bool,
    /// Metadata extraction fields.
    pub metadata_extraction: bool,
    /// A directory-entry icon theme, described by a sibling `icon-theme.json`.
    /// Runs no code: no `entrypoint` is required for this contribution alone.
    pub icon_theme: bool,
}

/// One icon asset referenced by an icon theme (task 0095).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IconDefinition {
    /// SVG asset path, relative to the plugin directory. Must not escape it.
    pub icon_path: PathBuf,
}

/// A distributable directory-entry icon theme, read from `icon-theme.json` when a plugin
/// declares `contributions.icon_theme` (task 0095).
///
/// Modeled on VS Code's file icon theme contribution, trimmed to what the frontend's
/// `EntryIconRegistry` resolves: kind (`file`/`folder`/`symlink`), file extension, and MIME
/// prefix. Pure data plus SVG assets — no code runs for this contribution.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IconThemeManifest {
    /// Every icon asset this theme can reference, keyed by an arbitrary theme-local name.
    pub icon_definitions: BTreeMap<String, IconDefinition>,
    /// Default icon definition key for `file` entries.
    #[serde(default)]
    pub file: Option<String>,
    /// Default icon definition key for `directory` entries.
    #[serde(default)]
    pub folder: Option<String>,
    /// Default icon definition key for `symlink` entries.
    #[serde(default)]
    pub symlink: Option<String>,
    /// Lowercased, dot-less file extension to icon definition key.
    #[serde(default)]
    pub file_extensions: BTreeMap<String, String>,
    /// Exact file name (e.g. `"Cargo.toml"`) to icon definition key, matched before
    /// [`Self::file_extensions`] so `Cargo.lock` can differ from every other `.lock`.
    #[serde(default)]
    pub file_names: BTreeMap<String, String>,
    /// Exact directory name (e.g. `".git"`) to icon definition key, for a collapsed folder
    /// (task 0092 "folder hooks").
    #[serde(default)]
    pub folder_names: BTreeMap<String, String>,
    /// Exact directory name to icon definition key, for that same folder shown expanded.
    #[serde(default)]
    pub folder_names_expanded: BTreeMap<String, String>,
    /// MIME type prefix (e.g. `"image/"`) to icon definition key.
    #[serde(default)]
    pub mime_prefixes: BTreeMap<String, String>,
}

impl IconThemeManifest {
    /// Parses and validates an `icon-theme.json` document.
    pub fn parse(source: &str) -> Result<Self, IconThemeManifestError> {
        let manifest: Self = serde_json::from_str(source).map_err(IconThemeManifestError::Json)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validates that every reference resolves to a declared definition and every
    /// `iconPath` is a safe, plugin-directory-relative path (no absolute paths, no `..`).
    pub fn validate(&self) -> Result<(), IconThemeManifestError> {
        if self.icon_definitions.is_empty() {
            return Err(IconThemeManifestError::Empty);
        }
        for (key, definition) in &self.icon_definitions {
            if !Self::is_safe_relative_path(&definition.icon_path) {
                return Err(IconThemeManifestError::UnsafeIconPath(key.clone()));
            }
        }
        for key in [&self.file, &self.folder, &self.symlink]
            .into_iter()
            .flatten()
        {
            self.require_known_key(key)?;
        }
        for key in self
            .file_extensions
            .values()
            .chain(self.file_names.values())
            .chain(self.folder_names.values())
            .chain(self.folder_names_expanded.values())
            .chain(self.mime_prefixes.values())
        {
            self.require_known_key(key)?;
        }
        Ok(())
    }

    fn require_known_key(&self, key: &str) -> Result<(), IconThemeManifestError> {
        if self.icon_definitions.contains_key(key) {
            Ok(())
        } else {
            Err(IconThemeManifestError::UnknownIconDefinition(
                key.to_owned(),
            ))
        }
    }

    fn is_safe_relative_path(path: &Path) -> bool {
        !path.as_os_str().is_empty()
            && !is_absolute_on_any_platform(path)
            && !path
                .components()
                .any(|component| component == Component::ParentDir)
    }
}

/// A manifest is portable data, so "absolute" must not depend on the host's
/// path rules: `Path::is_absolute` accepts `/etc/passwd` as relative on
/// Windows and `C:\secrets` as relative on Unix.
fn is_absolute_on_any_platform(path: &Path) -> bool {
    if path.is_absolute() {
        return true;
    }
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    text.starts_with('/')
        || text.starts_with('\\')
        || matches!(bytes, [drive, b':', ..] if drive.is_ascii_alphabetic())
}

/// Icon theme manifest parsing or validation failure.
#[derive(Debug, Error)]
pub enum IconThemeManifestError {
    /// The JSON does not conform to the schema.
    #[error("invalid icon theme manifest: {0}")]
    Json(#[source] serde_json::Error),
    /// The manifest declares no icon assets at all.
    #[error("icon theme manifest declares no icon definitions")]
    Empty,
    /// A `file`/`folder`/`symlink`/extension/MIME mapping references an undeclared key.
    #[error("icon theme manifest references unknown icon definition {0:?}")]
    UnknownIconDefinition(String),
    /// An `iconPath` is absolute or escapes the plugin directory.
    #[error("icon theme manifest icon definition {0:?} has an unsafe iconPath")]
    UnsafeIconPath(String),
}

/// Plugin action declaration, projected into the host action registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionContribution {
    /// Plugin-namespaced action identifier.
    pub id: String,
    /// User-facing action label.
    pub title: String,
    /// User-facing action description.
    pub description: String,
    /// Whether the host must require exactly one selected entry before this
    /// action is available (task 0055's `sample.copyMarkdownPath` and
    /// similar single-entry actions).
    #[serde(default)]
    pub requires_single_selection: bool,
}

/// One selected entry's name and location, passed into an action invocation
/// (task 0055). Distinct from the host's opaque `EntryId`: the caller already
/// knows the current pane's selection details, so the host does not need its
/// own entry-id resolution registry to invoke a plugin action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedEntryContext {
    /// The entry's display name, e.g. `"report.pdf"`.
    pub name: String,
    /// The entry's location URI, e.g. `"file:///Users/erik/Documents/report.pdf"`.
    pub uri: String,
}

/// Custom column declaration. Values are data, never JavaScript/UI code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnContribution {
    /// Plugin-namespaced column identifier.
    pub id: String,
    /// User-facing column label.
    pub title: String,
}

/// Metadata extraction declaration, namespaced by plugin identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataExtractionContribution {
    /// Plugin-namespaced metadata field.
    pub field: String,
}

/// Narrow services a plugin may request from its host; each method is capability-gated.
pub trait HostServices {
    /// Reads selected-entry metadata only when permitted.
    fn selected_entry_metadata(&self) -> Result<(), PermissionDenied>;
    /// Posts a non-blocking notification only when permitted.
    fn notify(&self, message: &str) -> Result<(), PermissionDenied>;
    /// Writes text to the system clipboard only when permitted.
    fn clipboard_write(&self, text: &str) -> Result<(), PermissionDenied>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_versioned_manifest_with_explicit_permissions() {
        let manifest = PluginManifest::parse(
            r#"id = "example.copy"
name = "Copy"
version = "0.1.0"
api_version = "1"
description = "Copies a selected path"
entrypoint = "plugin.lua"

[permissions]
selected_entry_metadata = true
clipboard_write = true

[contributions]
actions = true
"#,
        )
        .expect("manifest must be valid");

        assert!(manifest.permissions.selected_entry_metadata);
        assert!(manifest.permissions.clipboard_write);
        assert!(manifest.contributions.actions);
        assert!(!manifest.contributions.columns);
    }

    #[test]
    fn rejects_unknown_api_versions() {
        let error = PluginManifest::parse(
            "id='example.plugin'\nname='Example'\nversion='1'\napi_version='99'\ndescription='Example'\nentrypoint='plugin.lua'",
        )
        .expect_err("unknown API version must be rejected");

        assert!(matches!(error, ManifestError::UnsupportedApiVersion(version) if version == "99"));
    }

    #[test]
    fn rejects_unknown_permission_keys() {
        let error = PluginManifest::parse(
            "id='example.plugin'\nname='Example'\nversion='1'\napi_version='1'\ndescription='Example'\nentrypoint='plugin.lua'\n[permissions]\nall_files=true",
        )
        .expect_err("unknown capability must be rejected");

        assert!(matches!(error, ManifestError::Toml(_)));
    }

    #[test]
    fn permissions_deny_host_calls_by_default() {
        let error = PluginPermissions::default()
            .require(Permission::ClipboardWrite)
            .expect_err("omitted permission must be denied");

        assert_eq!(error.permission, Permission::ClipboardWrite);
    }

    #[test]
    fn accepts_an_icon_theme_only_manifest_with_no_entrypoint() {
        let manifest = PluginManifest::parse(
            "id='example.icons'\nname='Icons'\nversion='1'\napi_version='1'\ndescription='An icon theme'\n[contributions]\nicon_theme=true",
        )
        .expect("icon-theme-only manifest needs no entrypoint");

        assert!(manifest.entrypoint.is_none());
        assert!(manifest.contributions.icon_theme);
    }

    #[test]
    fn rejects_an_actions_manifest_with_no_entrypoint() {
        let error = PluginManifest::parse(
            "id='example.plugin'\nname='Example'\nversion='1'\napi_version='1'\ndescription='Example'\n[contributions]\nactions=true",
        )
        .expect_err("actions contribution requires an entrypoint");

        assert!(matches!(error, ManifestError::InvalidField("entrypoint")));
    }

    #[test]
    fn parses_a_valid_icon_theme_manifest() {
        let icon_theme = IconThemeManifest::parse(
            r#"{
                "iconDefinitions": {
                    "folder": {"iconPath": "folder.svg"},
                    "psd": {"iconPath": "icons/psd.svg"}
                },
                "folder": "folder",
                "fileExtensions": {"psd": "psd"},
                "fileNames": {"Cargo.lock": "psd"},
                "mimePrefixes": {"image/": "psd"}
            }"#,
        )
        .expect("valid icon theme manifest");

        assert_eq!(icon_theme.folder.as_deref(), Some("folder"));
        assert_eq!(
            icon_theme.file_extensions.get("psd").map(String::as_str),
            Some("psd")
        );
        assert_eq!(
            icon_theme.file_names.get("Cargo.lock").map(String::as_str),
            Some("psd")
        );
    }

    #[test]
    fn parses_folder_name_hooks_for_collapsed_and_expanded_states() {
        let icon_theme = IconThemeManifest::parse(
            r#"{
                "iconDefinitions": {
                    "folder-cargo": {"iconPath": "folder-cargo.svg"},
                    "folder-cargo-open": {"iconPath": "folder-cargo-open.svg"}
                },
                "folderNames": {".cargo": "folder-cargo"},
                "folderNamesExpanded": {".cargo": "folder-cargo-open"}
            }"#,
        )
        .expect("valid icon theme manifest with folder-name hooks");

        assert_eq!(
            icon_theme.folder_names.get(".cargo").map(String::as_str),
            Some("folder-cargo")
        );
        assert_eq!(
            icon_theme
                .folder_names_expanded
                .get(".cargo")
                .map(String::as_str),
            Some("folder-cargo-open")
        );
    }

    #[test]
    fn rejects_a_folder_name_hook_pointing_at_an_undeclared_definition() {
        let error = IconThemeManifest::parse(
            r#"{
                "iconDefinitions": {"folder": {"iconPath": "folder.svg"}},
                "folderNames": {".cargo": "missing"}
            }"#,
        )
        .expect_err("unknown definition key must be rejected");

        assert!(matches!(
            error,
            IconThemeManifestError::UnknownIconDefinition(key) if key == "missing"
        ));
    }

    #[test]
    fn rejects_a_file_name_pointing_at_an_undeclared_definition() {
        let error = IconThemeManifest::parse(
            r#"{
                "iconDefinitions": {"folder": {"iconPath": "folder.svg"}},
                "fileNames": {"Cargo.lock": "missing"}
            }"#,
        )
        .expect_err("unknown definition key must be rejected");

        assert!(matches!(
            error,
            IconThemeManifestError::UnknownIconDefinition(key) if key == "missing"
        ));
    }

    #[test]
    fn rejects_an_icon_theme_manifest_referencing_an_unknown_definition() {
        let error = IconThemeManifest::parse(
            r#"{"iconDefinitions":{"folder":{"iconPath":"folder.svg"}},"file":"missing"}"#,
        )
        .expect_err("unknown definition reference must be rejected");

        assert!(
            matches!(error, IconThemeManifestError::UnknownIconDefinition(key) if key == "missing")
        );
    }

    #[test]
    fn rejects_an_icon_theme_manifest_with_a_path_traversal_icon_path() {
        let error = IconThemeManifest::parse(
            r#"{"iconDefinitions":{"folder":{"iconPath":"../../etc/passwd"}}}"#,
        )
        .expect_err("path traversal must be rejected");

        assert!(matches!(error, IconThemeManifestError::UnsafeIconPath(key) if key == "folder"));
    }

    #[test]
    fn rejects_an_icon_theme_manifest_with_an_absolute_icon_path() {
        let error = IconThemeManifest::parse(
            r#"{"iconDefinitions":{"folder":{"iconPath":"/etc/passwd"}}}"#,
        )
        .expect_err("absolute path must be rejected");

        assert!(matches!(error, IconThemeManifestError::UnsafeIconPath(key) if key == "folder"));
    }

    #[test]
    fn rejects_an_empty_icon_theme_manifest() {
        let error = IconThemeManifest::parse(r#"{"iconDefinitions":{}}"#)
            .expect_err("empty icon theme must be rejected");

        assert!(matches!(error, IconThemeManifestError::Empty));
    }

    #[test]
    fn action_contribution_defaults_to_no_selection_requirement() {
        let json = r#"{"id":"sample.action","title":"Sample","description":"An action"}"#;
        let contribution: ActionContribution =
            serde_json::from_str(json).expect("must deserialize without the new field");
        assert!(!contribution.requires_single_selection);
    }

    #[test]
    fn selected_entry_context_round_trips_through_serde_json() {
        let entry = SelectedEntryContext {
            name: "report.pdf".to_owned(),
            uri: "file:///Users/erik/Documents/report.pdf".to_owned(),
        };
        let json = serde_json::to_string(&entry).expect("serialization must succeed");
        let parsed: SelectedEntryContext =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(entry, parsed);
    }
}
