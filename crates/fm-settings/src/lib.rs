//! Settings persistence (task 0030).
//!
//! Settings are versioned and migrated rather than discarded when the schema
//! changes (specification §26), and are written atomically so a crash cannot
//! leave a half-written configuration behind.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use fm_domain::{Location, SavedSearch};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Current on-disk settings schema.
pub const CURRENT_SCHEMA_VERSION: u32 = 6;
/// Stable settings filename within the platform configuration directory.
pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// UI language (task 0098).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    /// English; also the fallback locale.
    #[default]
    En,
    /// Dutch.
    Nl,
}

/// Application colour theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Follow the operating-system preference.
    #[default]
    Auto,
    /// Light colours.
    Light,
    /// Dark colours.
    Dark,
}

/// Timestamp presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DateFormat {
    /// Compact locale-aware date and time.
    Short,
    /// More descriptive locale-aware date and time.
    #[default]
    Medium,
    /// ISO-8601 representation.
    Iso,
}

/// File-size presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SizeFormat {
    /// Powers of 1024.
    #[default]
    Binary,
    /// Powers of 1000.
    Decimal,
    /// Unscaled byte count.
    Bytes,
}

/// Reserved `icon_theme` value selecting the built-in generic glyphs, rather than a distributable
/// icon theme plugin's id (task 0095).
pub const GENERIC_ICON_THEME: &str = "generic";
const LEGACY_CATPPUCCIN_ICON_THEME: &str = "catppuccin";
const CATPPUCCIN_ICON_THEME_PLUGIN: &str = "catppuccin.icons";

/// Default choice for file-operation conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    /// Ask before resolving.
    #[default]
    Ask,
    /// Replace the destination.
    Overwrite,
    /// Keep both entries.
    KeepBoth,
    /// Skip the source entry.
    Skip,
}

/// Initial pane arrangement inherited by a newly created workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DefaultPaneLayout {
    /// Two panes arranged horizontally.
    #[default]
    Dual,
    /// One pane.
    Single,
}

/// A named, provider-neutral location saved by the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavouriteLocation {
    /// User-visible label, independent of the location's URI.
    pub label: String,
    /// Provider-neutral target, never a host-specific raw path.
    pub location: Location,
}

/// How a multi-rename rule cases the composed filename.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MultiRenameCaseTransform {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiRenameSequence {
    /// First counter value.
    pub start: i64,
    /// Increment between selected entries.
    pub step: i64,
    /// Minimum number of displayed digits.
    pub padding: u32,
}

/// The complete rule configuration saved in a multi-rename preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiRenameRules {
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
    pub sequence: MultiRenameSequence,
    /// Final casing operation.
    pub case_transform: MultiRenameCaseTransform,
}

/// A user-named reusable multi-rename rule configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiRenamePreset {
    /// Unique user-visible preset name.
    pub name: String,
    /// Rules restored when the preset is loaded.
    pub rules: MultiRenameRules,
}

/// Versioned, application-wide settings. Live workspace content is excluded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// On-disk schema version.
    pub schema_version: u32,
    /// Application theme.
    pub theme: Theme,
    /// UI language.
    pub language: Language,
    /// Base font size in CSS pixels.
    pub font_size: u16,
    /// Directory row height in CSS pixels.
    pub row_height: u16,
    /// Timestamp presentation.
    pub date_format: DateFormat,
    /// File-size presentation.
    pub size_format: SizeFormat,
    /// Whether hidden entries are shown by default.
    pub show_hidden_files: bool,
    /// Whether permanent deletion requires confirmation.
    pub confirm_permanent_delete: bool,
    /// Default operation conflict policy.
    pub default_conflict_policy: ConflictPolicy,
    /// Maximum concurrent operations.
    pub operation_concurrency: u16,
    /// Layout inherited by new workspaces.
    pub default_pane_layout: DefaultPaneLayout,
    /// Columns inherited by new directory tabs.
    pub default_columns: Vec<String>,
    /// Column widths in CSS pixels, keyed by column id. Shared by every tab and pane rather than
    /// persisted per tab, so resizing a column anywhere applies everywhere.
    pub column_widths: BTreeMap<String, u32>,
    /// Action identifier to shortcut mapping.
    pub keybindings: BTreeMap<String, String>,
    /// Enabled plugin identifiers.
    pub enabled_plugins: Vec<String>,
    /// Non-secret plugin configuration. Secrets must use platform secret storage.
    pub plugin_settings: BTreeMap<String, Value>,
    /// Optional terminal executable or command.
    pub terminal_command: Option<String>,
    /// Optional text-editor executable or command for `core.edit` (task 0086); `None` falls back
    /// to the platform adapter's default text-editor association.
    pub editor_command: Option<String>,
    /// Locations inherited by newly created panes.
    pub default_start_locations: Vec<String>,
    /// User-managed named locations, in the order shown by the favourites menu.
    pub favourite_locations: Vec<FavouriteLocation>,
    /// Recently visited locations per workspace, newest first.
    pub recent_locations_by_workspace: BTreeMap<String, Vec<Location>>,
    /// User-named reusable multi-rename configurations.
    pub multi_rename_presets: Vec<MultiRenamePreset>,
    /// Durable smart folders. Queries contain provider-neutral locations but
    /// never credentials or connection-session tokens.
    pub saved_searches: Vec<SavedSearch>,
    /// Directory-entry icon set: `"generic"` for the built-in glyphs, or a discovered plugin's id
    /// (task 0095).
    pub icon_theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            theme: Theme::Auto,
            language: Language::En,
            font_size: 13,
            row_height: 20,
            date_format: DateFormat::Medium,
            size_format: SizeFormat::Binary,
            show_hidden_files: false,
            confirm_permanent_delete: true,
            default_conflict_policy: ConflictPolicy::Ask,
            operation_concurrency: 2,
            default_pane_layout: DefaultPaneLayout::Dual,
            default_columns: vec![
                "core.name".into(),
                "core.extension".into(),
                "core.size".into(),
                "core.modified".into(),
            ],
            column_widths: BTreeMap::new(),
            keybindings: BTreeMap::new(),
            enabled_plugins: Vec::new(),
            plugin_settings: BTreeMap::new(),
            terminal_command: None,
            editor_command: None,
            default_start_locations: Vec::new(),
            favourite_locations: Vec::new(),
            recent_locations_by_workspace: BTreeMap::new(),
            multi_rename_presets: Vec::new(),
            saved_searches: Vec::new(),
            icon_theme: GENERIC_ICON_THEME.to_owned(),
        }
    }
}

/// Successful load plus a user-visible recovery warning when defaults were used.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadOutcome {
    /// Loaded or default settings.
    pub settings: Settings,
    /// Recovery warning suitable for a frontend notification.
    pub warning: Option<String>,
}

/// Settings storage failure.
#[derive(Debug, Error)]
pub enum SettingsError {
    /// The platform did not expose a user configuration directory.
    #[error("the platform has no user configuration directory")]
    ConfigDirectoryUnavailable,
    /// A filesystem operation failed.
    #[error("settings filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    /// Settings serialization failed.
    #[error("settings serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// JSON settings repository rooted in one configuration directory.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    directory: PathBuf,
}

impl SettingsStore {
    /// Creates a store rooted in an explicit directory.
    #[must_use]
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// Creates the production store under the platform-appropriate config directory.
    pub fn platform_default() -> Result<Self, SettingsError> {
        let directory = dirs::config_dir()
            .ok_or(SettingsError::ConfigDirectoryUnavailable)?
            .join("fm");
        Ok(Self::new(directory))
    }

    /// Loads and migrates settings. Invalid/unreadable files are backed up.
    pub fn load(&self) -> Result<LoadOutcome, SettingsError> {
        let path = self.path();
        if !path.exists() {
            return Ok(LoadOutcome {
                settings: Settings::default(),
                warning: None,
            });
        }
        match fs::read(&path).ok().and_then(|bytes| migrate(&bytes).ok()) {
            Some(settings) => Ok(LoadOutcome {
                settings,
                warning: None,
            }),
            None => {
                fs::create_dir_all(&self.directory)?;
                let backup = self
                    .directory
                    .join(format!("{SETTINGS_FILE_NAME}.corrupt-{}", unix_timestamp()));
                fs::rename(&path, &backup)?;
                Ok(LoadOutcome {
                    settings: Settings::default(),
                    warning: Some(format!(
                        "Settings could not be read. Defaults were loaded and the original was backed up to {}.",
                        backup.display()
                    )),
                })
            }
        }
    }

    /// Atomically persists settings using a temporary sibling and rename.
    pub fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        fs::create_dir_all(&self.directory)?;
        let path = self.path();
        let temporary = self
            .directory
            .join(format!(".{SETTINGS_FILE_NAME}.{}.tmp", std::process::id()));
        let mut current = settings.clone();
        current.schema_version = CURRENT_SCHEMA_VERSION;
        for saved in &mut current.saved_searches {
            for location in &mut saved.query.scope.locations {
                location.uri = sanitize_persisted_location(&location.uri);
            }
        }
        let bytes = serde_json::to_vec_pretty(&current)?;
        {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }

        fn sanitize_persisted_location(uri: &str) -> String {
            let without_transient = uri.split(['?', '#']).next().unwrap_or(uri);
            let Some((scheme, remainder)) = without_transient.split_once("://") else {
                return without_transient.to_owned();
            };
            let authority_end = remainder.find('/').unwrap_or(remainder.len());
            let (authority, path) = remainder.split_at(authority_end);
            let sanitized_authority = authority.rsplit_once('@').map_or_else(
                || authority.to_owned(),
                |(userinfo, host)| {
                    let username = userinfo
                        .split_once(':')
                        .map_or(userinfo, |(username, _)| username);
                    if username.is_empty() {
                        host.to_owned()
                    } else {
                        format!("{username}@{host}")
                    }
                },
            );
            format!("{scheme}://{sanitized_authority}{path}")
        }
        fs::rename(&temporary, path)?;
        Ok(())
    }

    fn path(&self) -> PathBuf {
        self.directory.join(SETTINGS_FILE_NAME)
    }
}

fn migrate(bytes: &[u8]) -> Result<Settings, serde_json::Error> {
    let mut value: Value = serde_json::from_slice(bytes)?;
    let version = value
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if (1..u64::from(CURRENT_SCHEMA_VERSION)).contains(&version) {
        value["schemaVersion"] = Value::from(CURRENT_SCHEMA_VERSION);
    } else if version != u64::from(CURRENT_SCHEMA_VERSION) {
        return Err(<serde_json::Error as serde::de::Error>::custom(
            "unsupported settings schema version",
        ));
    }
    if value.get("iconTheme").and_then(Value::as_str) == Some(LEGACY_CATPPUCCIN_ICON_THEME) {
        value["iconTheme"] = Value::from(CATPPUCCIN_ICON_THEME_PLUGIN);
        if !value.get("enabledPlugins").is_some_and(Value::is_array) {
            value["enabledPlugins"] = Value::Array(Vec::new());
        }
        let enabled = value
            .get_mut("enabledPlugins")
            .and_then(Value::as_array_mut)
            .expect("enabledPlugins was initialized as an array");
        if !enabled
            .iter()
            .any(|plugin| plugin.as_str() == Some(CATPPUCCIN_ICON_THEME_PLUGIN))
        {
            enabled.push(Value::from(CATPPUCCIN_ICON_THEME_PLUGIN));
        }
    }
    serde_json::from_value(value)
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn default_settings_show_the_extension_column() {
        assert!(
            Settings::default()
                .default_columns
                .contains(&"core.extension".to_owned())
        );
    }

    #[test]
    fn settings_round_trip_every_application_default() {
        let directory = tempdir().expect("temp directory");
        let store = SettingsStore::new(directory.path());
        let settings = Settings {
            theme: Theme::Dark,
            font_size: 15,
            row_height: 31,
            date_format: DateFormat::Iso,
            size_format: SizeFormat::Decimal,
            show_hidden_files: true,
            confirm_permanent_delete: false,
            default_conflict_policy: ConflictPolicy::KeepBoth,
            operation_concurrency: 7,
            default_pane_layout: DefaultPaneLayout::Single,
            default_columns: vec!["core.name".into(), "core.size".into()],
            keybindings: [("copy".into(), "Ctrl+C".into())].into(),
            enabled_plugins: vec!["archive".into()],
            plugin_settings: [("archive".into(), serde_json::json!({"level": 3}))].into(),
            terminal_command: Some("alacritty".into()),
            editor_command: Some("code --wait".into()),
            default_start_locations: vec!["file:///tmp".into()],
            favourite_locations: vec![FavouriteLocation {
                label: "Temporary files".into(),
                location: Location::new(fm_domain::ProviderId::new("local"), "file:///tmp"),
            }],
            recent_locations_by_workspace: [(
                "workspace-1".into(),
                vec![Location::new(
                    fm_domain::ProviderId::new("local"),
                    "file:///tmp",
                )],
            )]
            .into(),
            multi_rename_presets: vec![MultiRenamePreset {
                name: "Number photos".into(),
                rules: MultiRenameRules {
                    search: "IMG_".into(),
                    replace: String::new(),
                    use_regex: false,
                    name_mask: "holiday-[C]".into(),
                    extension_mask: "[E]".into(),
                    sequence: MultiRenameSequence {
                        start: 10,
                        step: 2,
                        padding: 3,
                    },
                    case_transform: MultiRenameCaseTransform::Lower,
                },
            }],
            icon_theme: "sample.icons".into(),
            ..Settings::default()
        };

        store.save(&settings).expect("save settings");

        assert_eq!(store.load().expect("load settings").settings, settings);
    }

    #[test]
    fn migrates_the_legacy_catppuccin_theme_to_the_enabled_plugin() {
        let settings = migrate(
            br#"{
                "schemaVersion": 2,
                "iconTheme": "catppuccin",
                "enabledPlugins": []
            }"#,
        )
        .expect("migrate legacy icon theme");

        assert_eq!(settings.icon_theme, CATPPUCCIN_ICON_THEME_PLUGIN);
        assert!(
            settings
                .enabled_plugins
                .contains(&CATPPUCCIN_ICON_THEME_PLUGIN.to_owned())
        );
    }

    #[test]
    fn v1_fixture_migrates_to_the_current_schema_without_losing_values() {
        let directory = tempdir().expect("temp directory");
        fs::write(
            directory.path().join(SETTINGS_FILE_NAME),
            r#"{
              "schemaVersion": 1,
              "theme": "light",
              "fontSize": 16,
              "rowHeight": 30,
              "dateFormat": "iso",
              "sizeFormat": "bytes",
              "showHiddenFiles": true,
              "confirmPermanentDelete": false,
              "defaultConflictPolicy": "overwrite",
              "operationConcurrency": 3
            }"#,
        )
        .expect("write v1 fixture");

        let loaded = SettingsStore::new(directory.path())
            .load()
            .expect("migrate fixture");

        assert_eq!(loaded.settings.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded.settings.theme, Theme::Light);
        assert_eq!(loaded.settings.font_size, 16);
        assert!(loaded.settings.show_hidden_files);
        assert_eq!(
            loaded.settings.default_columns,
            Settings::default().default_columns
        );
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn v3_fixture_migrates_to_the_current_schema_with_the_default_locale() {
        let directory = tempdir().expect("temp directory");
        fs::write(
            directory.path().join(SETTINGS_FILE_NAME),
            r#"{
              "schemaVersion": 3,
              "theme": "dark"
            }"#,
        )
        .expect("write v3 fixture");

        let loaded = SettingsStore::new(directory.path())
            .load()
            .expect("migrate fixture");

        assert_eq!(loaded.settings.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded.settings.language, Settings::default().language);
    }

    #[test]
    fn v4_fixture_migrates_to_empty_multi_rename_presets() {
        let directory = tempdir().expect("temp directory");
        fs::write(
            directory.path().join(SETTINGS_FILE_NAME),
            r#"{
              "schemaVersion": 4,
              "theme": "dark"
            }"#,
        )
        .expect("write v4 fixture");

        let loaded = SettingsStore::new(directory.path())
            .load()
            .expect("migrate fixture");

        assert_eq!(loaded.settings.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(loaded.settings.multi_rename_presets.is_empty());
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn v5_fixture_migrates_to_empty_saved_searches() {
        let settings =
            migrate(br#"{"schemaVersion":5,"theme":"dark"}"#).expect("migrate v5 settings");

        assert_eq!(settings.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(settings.saved_searches.is_empty());
    }

    #[test]
    fn saved_search_scope_drops_credentials_and_transient_tokens() {
        let directory = tempdir().expect("temp directory");
        let store = SettingsStore::new(directory.path());
        let mut settings = Settings::default();
        settings.saved_searches.push(fm_domain::SavedSearch {
            id: "11111111-1111-4111-8111-111111111111".parse().unwrap(),
            name: "Remote reports".to_owned(),
            pinned: true,
            query: fm_domain::SearchQuery {
                schema_version: fm_domain::SEARCH_QUERY_SCHEMA_VERSION,
                scope: fm_domain::SearchScope {
                    locations: vec![Location::new(
                        fm_domain::ProviderId::new("sftp"),
                        "sftp://alice:secret@example.test/reports?token=temporary#session",
                    )],
                    recurse: true,
                    show_hidden: false,
                },
                name: None,
                entry_kinds: vec![fm_domain::SearchEntryKind::File],
                mime_types: Vec::new(),
                min_size_bytes: None,
                max_size_bytes: None,
                modified_after: None,
                modified_before: None,
                content: None,
                git_statuses: Vec::new(),
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
        });

        store.save(&settings).expect("save settings");
        let persisted = fs::read_to_string(directory.path().join(SETTINGS_FILE_NAME)).unwrap();
        let loaded = store.load().expect("load settings").settings;

        assert!(!persisted.contains("secret"));
        assert!(!persisted.contains("temporary"));
        assert_eq!(
            loaded.saved_searches[0].query.scope.locations[0].uri,
            "sftp://alice@example.test/reports"
        );
    }

    #[test]
    fn v2_fixture_migrates_to_favourites_and_workspace_recents() {
        let directory = tempdir().expect("temp directory");
        fs::write(
            directory.path().join(SETTINGS_FILE_NAME),
            r#"{
              "schemaVersion": 2,
              "theme": "dark"
            }"#,
        )
        .expect("write v2 fixture");

        let loaded = SettingsStore::new(directory.path())
            .load()
            .expect("migrate fixture");

        assert_eq!(loaded.settings.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(loaded.settings.favourite_locations.is_empty());
        assert!(loaded.settings.recent_locations_by_workspace.is_empty());
    }

    #[test]
    fn corrupt_settings_are_backed_up_and_defaults_are_returned_with_a_warning() {
        let directory = tempdir().expect("temp directory");
        fs::write(directory.path().join(SETTINGS_FILE_NAME), "{not json")
            .expect("write corrupt settings");

        let loaded = SettingsStore::new(directory.path())
            .load()
            .expect("recover corrupt settings");

        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.warning.is_some());
        let backups = fs::read_dir(directory.path())
            .expect("read settings directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
            .count();
        assert_eq!(backups, 1);
        assert!(!directory.path().join(SETTINGS_FILE_NAME).exists());
    }
}
