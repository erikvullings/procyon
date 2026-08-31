//! Plugin discovery and execution (task 0054).
//!
//! Enforces the declared permissions, applies execution timeouts and isolates
//! failures, so that a misbehaving plugin degrades to a notification rather
//! than taking down the application.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fm_plugin_api::{
    ActionContribution, ColumnContribution, IconThemeManifest, Permission, PluginManifest,
    SelectedEntryContext,
};
use mlua::{Error as LuaError, HookTriggers, Lua, LuaOptions, LuaSerdeExt, StdLib, Table, VmState};
use serde::de::DeserializeOwned;
use thiserror::Error;

const DEFAULT_TIMEOUT: Duration = Duration::from_millis(100);
const DEFAULT_MEMORY_LIMIT_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_INSTRUCTION_LIMIT: usize = 100_000;
const DEFAULT_FAILURE_LIMIT: u8 = 3;
const MAX_LOG_ENTRIES: usize = 100;

/// Bounded diagnostics retained for one plugin execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginLogEntry {
    /// Stable manifest identifier.
    pub plugin_id: String,
    /// A safe, user-readable failure message.
    pub message: String,
}

/// The observable result of invoking one plugin action (task 0055).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginActionOutcome {
    /// Text the plugin asked the host to write to the clipboard, if any.
    pub clipboard_text: Option<String>,
}

/// The reason a plugin call was isolated from the host application.
#[derive(Debug, Error)]
pub enum PluginRuntimeError {
    /// The plugin has been disabled after repeated failures.
    #[error("plugin {plugin_id:?} is disabled: {reason}")]
    Disabled {
        /// Stable manifest identifier.
        plugin_id: String,
        /// Automatic-disable diagnostic.
        reason: String,
    },
    /// The entrypoint could not be loaded.
    #[error("could not load plugin {plugin_id:?}: {source}")]
    Load {
        /// Stable manifest identifier.
        plugin_id: String,
        #[source]
        /// Underlying filesystem failure.
        source: std::io::Error,
    },
    /// Lua execution failed, including denied host calls and malformed results.
    #[error("plugin {plugin_id:?} failed: {message}")]
    Execution {
        /// Stable manifest identifier.
        plugin_id: String,
        /// Safe failure diagnostic.
        message: String,
    },
}

/// Restricted Lua executor for version-one plugin contributions.
///
/// Each call gets a fresh Lua VM containing only table, string, math, and UTF-8
/// helpers. It deliberately omits package, io, os, debug, and all host access
/// except the explicitly permission-checked `host` table.
#[derive(Debug)]
pub struct PluginRuntime {
    timeout: Duration,
    memory_limit_bytes: usize,
    instruction_limit: usize,
    failure_limit: u8,
    state: Mutex<RuntimeState>,
}

#[derive(Debug, Default)]
struct RuntimeState {
    failures: BTreeMap<String, u8>,
    disabled: BTreeMap<String, String>,
    logs: BTreeMap<String, VecDeque<PluginLogEntry>>,
}

impl Default for PluginRuntime {
    fn default() -> Self {
        Self::new(
            DEFAULT_TIMEOUT,
            DEFAULT_MEMORY_LIMIT_BYTES,
            DEFAULT_INSTRUCTION_LIMIT,
            DEFAULT_FAILURE_LIMIT,
        )
    }
}

impl PluginRuntime {
    /// Creates a runtime with explicit per-call resource limits.
    #[must_use]
    pub fn new(
        timeout: Duration,
        memory_limit_bytes: usize,
        instruction_limit: usize,
        failure_limit: u8,
    ) -> Self {
        Self {
            timeout,
            memory_limit_bytes,
            instruction_limit,
            failure_limit,
            state: Mutex::new(RuntimeState::default()),
        }
    }

    /// Executes the declared action contribution function from one plugin.
    ///
    /// A script returns a table whose optional `actions` member is a function
    /// returning an array of `ActionContribution` tables.
    pub fn actions(
        &self,
        manifest: &PluginManifest,
        directory: &Path,
    ) -> Result<Vec<ActionContribution>, PluginRuntimeError> {
        self.ensure_enabled(&manifest.id)?;
        if !manifest.contributions.actions {
            return Ok(Vec::new());
        }
        let result = self.execute_contribution(manifest, directory, "actions");
        match result {
            Ok(actions) => {
                self.reset_failures(&manifest.id);
                Ok(actions)
            }
            Err(message) => Err(self.record_failure(&manifest.id, message)),
        }
    }

    /// Executes declared data-only custom column declarations from one plugin.
    pub fn columns(
        &self,
        manifest: &PluginManifest,
        directory: &Path,
    ) -> Result<Vec<ColumnContribution>, PluginRuntimeError> {
        self.ensure_enabled(&manifest.id)?;
        if !manifest.contributions.columns {
            return Ok(Vec::new());
        }
        match self.execute_contribution(manifest, directory, "columns") {
            Ok(columns) => {
                self.reset_failures(&manifest.id);
                Ok(columns)
            }
            Err(message) => Err(self.record_failure(&manifest.id, message)),
        }
    }

    /// Invokes one action contribution by id, giving its entrypoint's
    /// `invoke` function access to the caller-supplied selection and the
    /// permission-gated clipboard host call (task 0055).
    ///
    /// The caller (frontend or host adapter) already knows the active
    /// pane's current selection, so `selection` is supplied directly rather
    /// than resolved from an opaque entry-id registry.
    pub fn invoke_action(
        &self,
        manifest: &PluginManifest,
        directory: &Path,
        action_id: &str,
        selection: &[SelectedEntryContext],
    ) -> Result<PluginActionOutcome, PluginRuntimeError> {
        self.ensure_enabled(&manifest.id)?;
        if !manifest.contributions.actions {
            return Err(PluginRuntimeError::Execution {
                plugin_id: manifest.id.clone(),
                message: "plugin does not contribute actions".to_owned(),
            });
        }
        match self.execute_invoke(manifest, directory, action_id, selection) {
            Ok(outcome) => {
                self.reset_failures(&manifest.id);
                Ok(outcome)
            }
            Err(message) => Err(self.record_failure(&manifest.id, message)),
        }
    }

    /// Returns bounded retained diagnostics for a plugin.
    #[must_use]
    pub fn logs(&self, plugin_id: &str) -> Vec<PluginLogEntry> {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .logs
            .get(plugin_id)
            .map(|entries| entries.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Re-enables a plugin after an automatic disablement.
    pub fn reenable(&self, plugin_id: &str) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.disabled.remove(plugin_id);
        state.failures.remove(plugin_id);
    }

    /// Returns the automatic-disable reason, if any.
    #[must_use]
    pub fn disabled_reason(&self, plugin_id: &str) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .disabled
            .get(plugin_id)
            .cloned()
    }

    fn execute_contribution<T: DeserializeOwned>(
        &self,
        manifest: &PluginManifest,
        directory: &Path,
        contribution: &str,
    ) -> Result<Vec<T>, String> {
        let entrypoint = manifest
            .entrypoint
            .as_ref()
            .ok_or_else(|| "plugin declares no entrypoint".to_owned())?;
        let source = fs::read_to_string(directory.join(entrypoint))
            .map_err(|error| format!("could not load entrypoint: {error}"))?;
        let lua = self.new_sandboxed_lua()?;
        install_host_services(&lua, manifest).map_err(|error| error.to_string())?;
        let module: Table = lua
            .load(&source)
            .eval()
            .map_err(|error| error.to_string())?;
        let function = module
            .get::<mlua::Function>(contribution)
            .map_err(|error| format!("malformed plugin result: {error}"))?;
        let value: mlua::Value = function.call(()).map_err(|error| error.to_string())?;
        lua.from_value(value)
            .map_err(|error| format!("malformed plugin result: {error}"))
    }

    fn execute_invoke(
        &self,
        manifest: &PluginManifest,
        directory: &Path,
        action_id: &str,
        selection: &[SelectedEntryContext],
    ) -> Result<PluginActionOutcome, String> {
        let entrypoint = manifest
            .entrypoint
            .as_ref()
            .ok_or_else(|| "plugin declares no entrypoint".to_owned())?;
        let source = fs::read_to_string(directory.join(entrypoint))
            .map_err(|error| format!("could not load entrypoint: {error}"))?;
        let lua = self.new_sandboxed_lua()?;
        let clipboard_text = Arc::new(Mutex::new(None::<String>));
        install_action_host_services(&lua, manifest, selection, Arc::clone(&clipboard_text))
            .map_err(|error| error.to_string())?;
        let module: Table = lua
            .load(&source)
            .eval()
            .map_err(|error| error.to_string())?;
        let function = module
            .get::<mlua::Function>("invoke")
            .map_err(|error| format!("malformed plugin result: {error}"))?;
        function
            .call::<()>(action_id.to_owned())
            .map_err(|error| error.to_string())?;
        Ok(PluginActionOutcome {
            clipboard_text: clipboard_text
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
        })
    }

    /// Creates a fresh VM containing only table, string, math and UTF-8
    /// helpers, with the shared memory limit, timeout and instruction-budget
    /// hook applied. Host services are installed separately by each caller.
    fn new_sandboxed_lua(&self) -> Result<Lua, String> {
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
            LuaOptions::default(),
        )
        .map_err(|error| error.to_string())?;
        lua.set_memory_limit(self.memory_limit_bytes)
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + self.timeout;
        let instructions = AtomicUsize::new(0);
        let instruction_limit = self.instruction_limit;
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(100),
            move |_, _| {
                let executed = instructions.fetch_add(100, Ordering::Relaxed) + 100;
                if Instant::now() >= deadline {
                    return Err(LuaError::RuntimeError(
                        "plugin execution timed out".to_owned(),
                    ));
                }
                if executed > instruction_limit {
                    return Err(LuaError::RuntimeError(
                        "plugin instruction budget exceeded".to_owned(),
                    ));
                }
                Ok(VmState::Continue)
            },
        )
        .map_err(|error| error.to_string())?;
        Ok(lua)
    }

    fn ensure_enabled(&self, plugin_id: &str) -> Result<(), PluginRuntimeError> {
        self.disabled_reason(plugin_id).map_or(Ok(()), |reason| {
            Err(PluginRuntimeError::Disabled {
                plugin_id: plugin_id.to_owned(),
                reason,
            })
        })
    }

    fn reset_failures(&self, plugin_id: &str) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .failures
            .remove(plugin_id);
    }

    fn record_failure(&self, plugin_id: &str, message: String) -> PluginRuntimeError {
        tracing::warn!(plugin_id, error = %message, "plugin call failed");
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let failure_count = {
            let failures = state.failures.entry(plugin_id.to_owned()).or_default();
            *failures = failures.saturating_add(1);
            *failures
        };
        let disabled = failure_count >= self.failure_limit;
        if disabled {
            state.disabled.insert(
                plugin_id.to_owned(),
                format!("disabled after {failure_count} consecutive failures: {message}"),
            );
        }
        let entries = state.logs.entry(plugin_id.to_owned()).or_default();
        entries.push_back(PluginLogEntry {
            plugin_id: plugin_id.to_owned(),
            message: message.clone(),
        });
        if entries.len() > MAX_LOG_ENTRIES {
            entries.pop_front();
        }
        PluginRuntimeError::Execution {
            plugin_id: plugin_id.to_owned(),
            message,
        }
    }
}

fn install_host_services(lua: &Lua, manifest: &PluginManifest) -> mlua::Result<()> {
    let host = lua.create_table()?;
    let permissions = manifest.permissions.clone();
    host.set(
        "selected_entry_metadata",
        lua.create_function(move |_, ()| {
            permissions
                .require(Permission::SelectedEntryMetadata)
                .map_err(|error| LuaError::RuntimeError(error.to_string()))
        })?,
    )?;
    lua.globals().set("host", host)
}

/// Installs the host table used by action invocation (task 0055): unlike
/// [`install_host_services`]'s declare-time permission check,
/// `selected_entry_metadata` here returns the caller-supplied selection, and
/// `clipboard_write` records its argument for the caller to read back after
/// the call returns, gated by the same permission model.
fn install_action_host_services(
    lua: &Lua,
    manifest: &PluginManifest,
    selection: &[SelectedEntryContext],
    clipboard_text: Arc<Mutex<Option<String>>>,
) -> mlua::Result<()> {
    let host = lua.create_table()?;
    let metadata_permissions = manifest.permissions.clone();
    let selection = selection.to_vec();
    host.set(
        "selected_entry_metadata",
        lua.create_function(move |lua, ()| {
            metadata_permissions
                .require(Permission::SelectedEntryMetadata)
                .map_err(|error| LuaError::RuntimeError(error.to_string()))?;
            let entries = lua.create_table()?;
            for (index, entry) in selection.iter().enumerate() {
                let entry_table = lua.create_table()?;
                entry_table.set("name", entry.name.clone())?;
                entry_table.set("uri", entry.uri.clone())?;
                entries.set(index + 1, entry_table)?;
            }
            Ok(entries)
        })?,
    )?;
    let clipboard_permissions = manifest.permissions.clone();
    host.set(
        "clipboard_write",
        lua.create_function(move |_, text: String| {
            clipboard_permissions
                .require(Permission::ClipboardWrite)
                .map_err(|error| LuaError::RuntimeError(error.to_string()))?;
            *clipboard_text
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(text);
            Ok(())
        })?,
    )?;
    lua.globals().set("host", host)
}

/// A discovered plugin, including disabled manifests and their diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPlugin {
    /// The parsed manifest when valid.
    pub manifest: Option<PluginManifest>,
    /// Directory containing `plugin.toml`.
    pub directory: PathBuf,
    /// Validation diagnostic that prevents loading, if any.
    pub diagnostic: Option<String>,
    /// The parsed `icon-theme.json`, present only for a valid `icon_theme` contribution.
    pub icon_theme: Option<IconThemeManifest>,
}

impl DiscoveredPlugin {
    /// Whether this plugin's manifest can be enabled.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.manifest.is_some()
    }

    /// Stable ID for valid plugins, or the directory name for invalid ones.
    #[must_use]
    pub fn id(&self) -> String {
        self.manifest
            .as_ref()
            .map(|manifest| manifest.id.clone())
            .or_else(|| {
                self.directory
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "invalid-plugin".to_owned())
    }
}

/// Discovers plugin manifests without allowing one malformed plugin to abort startup.
#[derive(Debug, Clone)]
pub struct PluginDiscovery {
    directories: Vec<PathBuf>,
}

impl PluginDiscovery {
    /// Scans direct child directories of `directory` for `plugin.toml` manifests.
    #[must_use]
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directories: vec![directory.into()],
        }
    }

    /// Adds a read-only bundled plugin directory after the user plugin directory.
    #[must_use]
    pub fn with_bundled_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.directories.push(directory.into());
        self
    }

    /// Replaces every bundled directory (everything after [`Self::new`]'s user directory) with
    /// a single new one.
    ///
    /// For a host that only learns its real bundled-resources location once the app has
    /// finished initializing (e.g. Tauri's `resource_dir()`, resolved from a running
    /// `AppHandle` rather than available at construction time or compile time), rather than
    /// requiring that location be known up front.
    #[must_use]
    pub fn with_replaced_bundled_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.directories.truncate(1);
        self.directories.push(directory.into());
        self
    }

    /// Returns valid and disabled plugin records in deterministic directory order.
    ///
    /// If the same plugin id is found in more than one directory (e.g. a user plugin shadowing a
    /// bundled one of the same id), only the first directory's copy is kept — directories are
    /// scanned in the order they were added to `self`, so the user plugin directory (added via
    /// [`Self::new`]) always wins over a later [`Self::with_bundled_directory`] — rather than
    /// listing both, which would otherwise duplicate that plugin's contributions (columns,
    /// actions, icon themes) in every consumer of `discover()`.
    pub fn discover(&self) -> Vec<DiscoveredPlugin> {
        let mut seen_ids = std::collections::HashSet::new();
        let mut plugins = self
            .directories
            .iter()
            .flat_map(|directory| discover_plugins(directory))
            .filter(|plugin| seen_ids.insert(plugin.id()))
            .collect::<Vec<_>>();
        plugins.sort_by_key(DiscoveredPlugin::id);
        plugins
    }
}

/// Scans direct child directories for manifests. Filesystem errors are represented as diagnostics.
#[must_use]
pub fn discover_plugins(directory: &Path) -> Vec<DiscoveredPlugin> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut directories: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();
    directories
        .into_iter()
        .filter_map(|directory| {
            let manifest_path = directory.join("plugin.toml");
            if !manifest_path.exists() {
                return None;
            }
            Some(match fs::read_to_string(&manifest_path) {
                Ok(source) => match PluginManifest::parse(&source) {
                    Ok(manifest) => load_discovered_plugin(manifest, directory),
                    Err(error) => DiscoveredPlugin {
                        manifest: None,
                        directory,
                        diagnostic: Some(error.to_string()),
                        icon_theme: None,
                    },
                },
                Err(error) => DiscoveredPlugin {
                    manifest: None,
                    directory,
                    diagnostic: Some(format!("could not read plugin.toml: {error}")),
                    icon_theme: None,
                },
            })
        })
        .collect()
}

/// Loads a valid manifest's `icon-theme.json` when declared, folding a missing/malformed/unsafe
/// icon theme into the same "invalid plugin, disabled with diagnostic" outcome used for a bad
/// `plugin.toml` — an icon-theme contribution runs no code, so this is the only validation gate
/// it gets.
fn load_discovered_plugin(manifest: PluginManifest, directory: PathBuf) -> DiscoveredPlugin {
    if !manifest.contributions.icon_theme {
        return DiscoveredPlugin {
            manifest: Some(manifest),
            directory,
            diagnostic: None,
            icon_theme: None,
        };
    }
    match load_icon_theme(&directory) {
        Ok(icon_theme) => DiscoveredPlugin {
            manifest: Some(manifest),
            directory,
            diagnostic: None,
            icon_theme: Some(icon_theme),
        },
        Err(diagnostic) => DiscoveredPlugin {
            manifest: None,
            directory,
            diagnostic: Some(diagnostic),
            icon_theme: None,
        },
    }
}

const ICON_THEME_MANIFEST_FILE_NAME: &str = "icon-theme.json";

fn load_icon_theme(directory: &Path) -> Result<IconThemeManifest, String> {
    let manifest_path = directory.join(ICON_THEME_MANIFEST_FILE_NAME);
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("could not read {ICON_THEME_MANIFEST_FILE_NAME}: {error}"))?;
    let icon_theme = IconThemeManifest::parse(&source).map_err(|error| error.to_string())?;
    for definition in icon_theme.icon_definitions.values() {
        resolve_plugin_asset(directory, &definition.icon_path).ok_or_else(|| {
            format!(
                "icon asset {:?} escapes the plugin directory",
                definition.icon_path
            )
        })?;
    }
    Ok(icon_theme)
}

/// Resolves `relative` against `directory`, returning `None` unless the resolved, canonicalized
/// path both exists and stays within `directory` — defense in depth against symlink-based
/// traversal beyond `IconThemeManifest::validate`'s lexical (`..`/absolute) check.
#[must_use]
pub fn resolve_plugin_asset(directory: &Path, relative: &Path) -> Option<PathBuf> {
    let root = fs::canonicalize(directory).ok()?;
    let resolved = fs::canonicalize(directory.join(relative)).ok()?;
    resolved.starts_with(&root).then_some(resolved)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn discovery_reports_a_malformed_plugin_as_disabled() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let invalid = temporary.path().join("invalid");
        fs::create_dir(&invalid).expect("plugin directory");
        fs::write(invalid.join("plugin.toml"), "id = 'missing-fields'").expect("manifest");

        let plugins = discover_plugins(temporary.path());

        assert_eq!(plugins.len(), 1);
        assert!(!plugins[0].is_valid());
        assert!(
            plugins[0]
                .diagnostic
                .as_deref()
                .is_some_and(|diagnostic| diagnostic.contains("invalid plugin manifest"))
        );
    }

    #[test]
    fn discovers_a_valid_icon_theme_plugin_with_no_entrypoint() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let plugin = temporary.path().join("icons");
        fs::create_dir(&plugin).expect("plugin directory");
        fs::write(
            plugin.join("plugin.toml"),
            "id='example.icons'\nname='Icons'\nversion='1'\napi_version='1'\ndescription='An icon theme'\n[contributions]\nicon_theme=true",
        )
        .expect("manifest");
        fs::write(plugin.join("folder.svg"), "<svg></svg>").expect("asset");
        fs::write(
            plugin.join("icon-theme.json"),
            r#"{"iconDefinitions":{"folder":{"iconPath":"folder.svg"}},"folder":"folder"}"#,
        )
        .expect("icon theme");

        let discovered = discover_plugins(temporary.path()).pop().expect("plugin");

        assert!(discovered.is_valid());
        assert!(discovered.manifest.expect("manifest").entrypoint.is_none());
        let icon_theme = discovered.icon_theme.expect("icon theme");
        assert_eq!(icon_theme.folder.as_deref(), Some("folder"));
    }

    #[test]
    fn plugin_discovery_deduplicates_an_id_present_in_both_directories() {
        fn write_same_plugin(root: &Path) {
            let plugin = root.join("icons");
            fs::create_dir(&plugin).expect("plugin directory");
            fs::write(
                plugin.join("plugin.toml"),
                "id='example.icons'\nname='Icons'\nversion='1'\napi_version='1'\ndescription='An icon theme'\n[contributions]\nicon_theme=true",
            )
            .expect("manifest");
        }

        let user_directory = tempfile::tempdir().expect("user plugin directory");
        let bundled_directory = tempfile::tempdir().expect("bundled plugin directory");
        write_same_plugin(user_directory.path());
        write_same_plugin(bundled_directory.path());

        let discovery = PluginDiscovery::new(user_directory.path())
            .with_bundled_directory(bundled_directory.path());
        let discovered = discovery.discover();

        assert_eq!(
            discovered.len(),
            1,
            "the shared id should only be listed once"
        );
    }

    #[test]
    fn discovery_disables_an_icon_theme_plugin_whose_icon_path_escapes_the_directory() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let plugin = temporary.path().join("icons");
        fs::create_dir(&plugin).expect("plugin directory");
        fs::write(
            plugin.join("plugin.toml"),
            "id='example.icons'\nname='Icons'\nversion='1'\napi_version='1'\ndescription='An icon theme'\n[contributions]\nicon_theme=true",
        )
        .expect("manifest");
        // A relative-but-safe-looking iconPath that does not exist is rejected the same as an
        // actual traversal attempt, since `resolve_plugin_asset` requires the target to exist.
        fs::write(
            plugin.join("icon-theme.json"),
            r#"{"iconDefinitions":{"folder":{"iconPath":"missing.svg"}}}"#,
        )
        .expect("icon theme");

        let discovered = discover_plugins(temporary.path()).pop().expect("plugin");

        assert!(!discovered.is_valid());
        assert!(
            discovered
                .diagnostic
                .as_deref()
                .is_some_and(|diagnostic| diagnostic.contains("escapes the plugin directory"))
        );
    }

    #[test]
    fn discovery_disables_an_icon_theme_plugin_with_an_unknown_definition_reference() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let plugin = temporary.path().join("icons");
        fs::create_dir(&plugin).expect("plugin directory");
        fs::write(
            plugin.join("plugin.toml"),
            "id='example.icons'\nname='Icons'\nversion='1'\napi_version='1'\ndescription='An icon theme'\n[contributions]\nicon_theme=true",
        )
        .expect("manifest");
        fs::write(plugin.join("folder.svg"), "<svg></svg>").expect("asset");
        fs::write(
            plugin.join("icon-theme.json"),
            r#"{"iconDefinitions":{"folder":{"iconPath":"folder.svg"}},"folder":"not-declared"}"#,
        )
        .expect("icon theme");

        let discovered = discover_plugins(temporary.path()).pop().expect("plugin");

        assert!(!discovered.is_valid());
        assert!(
            discovered
                .diagnostic
                .as_deref()
                .is_some_and(|diagnostic| diagnostic.contains("unknown icon definition"))
        );
    }

    #[test]
    fn discovers_the_real_catppuccin_icons_plugin_package() {
        let plugins_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let discovered = discover_plugins(&plugins_root)
            .into_iter()
            .find(|plugin| {
                plugin
                    .manifest
                    .as_ref()
                    .is_some_and(|manifest| manifest.id == "catppuccin.icons")
            })
            .expect("catppuccin.icons plugin must be discovered");

        assert!(
            discovered.is_valid(),
            "diagnostic: {:?}",
            discovered.diagnostic
        );
        let manifest = discovered.manifest.expect("manifest");
        assert!(manifest.entrypoint.is_none());
        assert!(manifest.contributions.icon_theme);

        let icon_theme = discovered.icon_theme.expect("icon theme");
        assert_eq!(icon_theme.folder.as_deref(), Some("folder"));
        assert_eq!(icon_theme.file.as_deref(), Some("file"));
        assert_eq!(icon_theme.symlink.as_deref(), Some("symlink"));
        assert_eq!(
            icon_theme.file_extensions.get("ts").map(String::as_str),
            Some("typescript")
        );
        assert_eq!(
            icon_theme.mime_prefixes.get("image/").map(String::as_str),
            Some("image")
        );
        assert_eq!(
            icon_theme.file_names.get("Cargo.toml").map(String::as_str),
            Some("cargo")
        );
        // Deliberately not an exact count: the theme grows as icons are vendored.
        assert!(icon_theme.icon_definitions.len() >= 32);
    }

    #[test]
    fn executes_a_plugin_action_declaration_in_the_restricted_lua_runtime() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let plugin = temporary.path().join("copy-path");
        fs::create_dir(&plugin).expect("plugin directory");
        fs::write(
            plugin.join("plugin.toml"),
            "id='example.copy-path'\nname='Copy Path'\nversion='1'\napi_version='1'\ndescription='Copies a path'\nentrypoint='plugin.lua'\n[contributions]\nactions=true",
        )
        .expect("manifest");
        fs::write(
            plugin.join("plugin.lua"),
            "return { actions = function() return {{ id = 'example.copy-path.copy', title = 'Copy Path', description = 'Copies the selected path' }} end }",
        )
        .expect("script");

        let discovered = discover_plugins(temporary.path()).pop().expect("plugin");
        let manifest = discovered.manifest.expect("valid manifest");
        let runtime = PluginRuntime::default();

        let actions = runtime
            .actions(&manifest, &discovered.directory)
            .expect("actions");

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "example.copy-path.copy");
    }

    #[test]
    fn executes_a_plugin_column_declaration_in_the_restricted_lua_runtime() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        fs::write(
            temporary.path().join("plugin.lua"),
            "return { columns = function() return {{ id = 'sample.fileAge', title = 'Age' }} end }",
        )
        .expect("script");
        let manifest = PluginManifest::parse(
            "id='sample.file-age'\nname='File Age'\nversion='1'\napi_version='1'\ndescription='Shows file age'\nentrypoint='plugin.lua'\n[contributions]\ncolumns=true",
        )
        .expect("manifest");

        let columns = PluginRuntime::default()
            .columns(&manifest, temporary.path())
            .expect("columns");

        assert_eq!(columns[0].id, "sample.fileAge");
    }

    #[test]
    fn isolates_malformed_plugin_column_data() {
        let temporary =
            write_script("return { columns = function() return 'not a column list' end }");
        let manifest = PluginManifest::parse(
            "id='example.columns'\nname='Columns'\nversion='1'\napi_version='1'\ndescription='Columns'\nentrypoint='plugin.lua'\n[contributions]\ncolumns=true",
        )
        .expect("manifest");

        let error = PluginRuntime::default()
            .columns(&manifest, temporary.path())
            .expect_err("malformed columns");

        assert!(error.to_string().contains("malformed plugin result"));
    }

    fn manifest() -> PluginManifest {
        PluginManifest::parse("id='example.plugin'\nname='Example'\nversion='1'\napi_version='1'\ndescription='Example'\nentrypoint='plugin.lua'\n[contributions]\nactions=true")
            .expect("valid manifest")
    }

    fn write_script(source: &str) -> tempfile::TempDir {
        let temporary = tempfile::tempdir().expect("temporary directory");
        fs::write(temporary.path().join("plugin.lua"), source).expect("script");
        temporary
    }

    #[test]
    fn isolates_a_panicking_plugin_and_retains_its_log() {
        let temporary = write_script("error('boom')");
        let runtime = PluginRuntime::default();

        let error = runtime
            .actions(&manifest(), temporary.path())
            .expect_err("plugin failure");

        assert!(error.to_string().contains("example.plugin"));
        assert!(runtime.logs("example.plugin")[0].message.contains("boom"));
    }

    #[test]
    fn aborts_an_infinite_loop_without_disabling_the_host() {
        let temporary = write_script("while true do end");
        let runtime = PluginRuntime::new(Duration::from_millis(5), 1_000_000, 10_000, 3);

        let error = runtime
            .actions(&manifest(), temporary.path())
            .expect_err("loop timeout");

        assert!(error.to_string().contains("budget") || error.to_string().contains("timed out"));
        assert!(runtime.disabled_reason("example.plugin").is_none());
    }

    #[test]
    fn denies_an_undeclared_host_permission() {
        let temporary = write_script(
            "host.selected_entry_metadata()\nreturn { actions = function() return {} end }",
        );
        let runtime = PluginRuntime::default();

        let error = runtime
            .actions(&manifest(), temporary.path())
            .expect_err("permission denial");

        assert!(error.to_string().contains("permission denied"));
    }

    #[test]
    fn isolates_malformed_plugin_data() {
        let temporary =
            write_script("return { actions = function() return 'not an action list' end }");
        let runtime = PluginRuntime::default();

        let error = runtime
            .actions(&manifest(), temporary.path())
            .expect_err("malformed result");

        assert!(error.to_string().contains("malformed plugin result"));
    }

    #[test]
    fn repeatedly_failing_plugin_is_disabled_and_can_be_reenabled() {
        let temporary = write_script("error('boom')");
        let runtime = PluginRuntime::new(Duration::from_millis(100), 1_000_000, 10_000, 2);
        let manifest = manifest();

        runtime
            .actions(&manifest, temporary.path())
            .expect_err("first failure");
        runtime
            .actions(&manifest, temporary.path())
            .expect_err("second failure");

        assert!(runtime.disabled_reason("example.plugin").is_some());
        runtime.reenable("example.plugin");
        assert!(runtime.disabled_reason("example.plugin").is_none());
    }

    fn manifest_with_clipboard_write() -> PluginManifest {
        PluginManifest::parse(
            "id='example.plugin'\nname='Example'\nversion='1'\napi_version='1'\ndescription='Example'\nentrypoint='plugin.lua'\n[permissions]\nselected_entry_metadata=true\nclipboard_write=true\n[contributions]\nactions=true",
        )
        .expect("valid manifest")
    }

    #[test]
    fn invoke_action_writes_selected_entry_metadata_to_the_clipboard_when_permitted() {
        let temporary = write_script(
            "return { invoke = function(action_id) \
                 local entries = host.selected_entry_metadata() \
                 host.clipboard_write('[' .. entries[1].name .. '](' .. entries[1].uri .. ')') \
             end }",
        );
        let selection = vec![SelectedEntryContext {
            name: "report.pdf".to_owned(),
            uri: "file:///Users/erik/Documents/report.pdf".to_owned(),
        }];

        let outcome = PluginRuntime::default()
            .invoke_action(
                &manifest_with_clipboard_write(),
                temporary.path(),
                "sample.copyMarkdownPath",
                &selection,
            )
            .expect("invocation must succeed");

        assert_eq!(
            outcome.clipboard_text.as_deref(),
            Some("[report.pdf](file:///Users/erik/Documents/report.pdf)")
        );
    }

    #[test]
    fn invoke_action_fails_visibly_without_the_clipboard_write_permission() {
        let temporary = write_script(
            "return { invoke = function(action_id) host.clipboard_write('denied') end }",
        );
        let manifest = manifest(); // no permissions declared

        let error = PluginRuntime::default()
            .invoke_action(&manifest, temporary.path(), "sample.copyMarkdownPath", &[])
            .expect_err("clipboard write must be denied without the permission");

        assert!(error.to_string().contains("permission denied"));
    }

    fn sample_copy_markdown_path_directory() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-copy-markdown-path")
    }

    fn sample_copy_markdown_path_manifest() -> PluginManifest {
        let source = fs::read_to_string(sample_copy_markdown_path_directory().join("plugin.toml"))
            .expect("sample plugin manifest must exist");
        PluginManifest::parse(&source).expect("sample plugin manifest must be valid")
    }

    #[test]
    fn sample_copy_markdown_path_declares_a_single_selection_action() {
        let manifest = sample_copy_markdown_path_manifest();

        let actions = PluginRuntime::default()
            .actions(&manifest, &sample_copy_markdown_path_directory())
            .expect("actions must be declared");

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "sample.copyMarkdownPath");
        assert!(actions[0].requires_single_selection);
    }

    #[test]
    fn sample_copy_markdown_path_escapes_spaces_parentheses_and_unicode() {
        let manifest = sample_copy_markdown_path_manifest();
        let directory = sample_copy_markdown_path_directory();
        let runtime = PluginRuntime::default();

        let plain = runtime
            .invoke_action(
                &manifest,
                &directory,
                "sample.copyMarkdownPath",
                &[SelectedEntryContext {
                    name: "report.pdf".to_owned(),
                    uri: "file:///Users/erik/Documents/report.pdf".to_owned(),
                }],
            )
            .expect("invocation must succeed");
        assert_eq!(
            plain.clipboard_text.as_deref(),
            Some("[report.pdf](file:///Users/erik/Documents/report.pdf)")
        );

        let spaced_and_parenthesized = runtime
            .invoke_action(
                &manifest,
                &directory,
                "sample.copyMarkdownPath",
                &[SelectedEntryContext {
                    name: "My File (2).txt".to_owned(),
                    uri: "file:///Users/erik/Documents/My File (2).txt".to_owned(),
                }],
            )
            .expect("invocation must succeed");
        assert_eq!(
            spaced_and_parenthesized.clipboard_text.as_deref(),
            Some("[My File (2).txt](file:///Users/erik/Documents/My%20File%20%282%29.txt)")
        );

        let unicode = runtime
            .invoke_action(
                &manifest,
                &directory,
                "sample.copyMarkdownPath",
                &[SelectedEntryContext {
                    name: "résumé.pdf".to_owned(),
                    uri: "file:///Users/erik/Documents/résumé.pdf".to_owned(),
                }],
            )
            .expect("invocation must succeed");
        assert_eq!(
            unicode.clipboard_text.as_deref(),
            Some("[résumé.pdf](file:///Users/erik/Documents/r%C3%A9sum%C3%A9.pdf)")
        );
    }

    #[test]
    fn sample_copy_markdown_path_fails_visibly_without_clipboard_write_permission() {
        let source = fs::read_to_string(sample_copy_markdown_path_directory().join("plugin.toml"))
            .expect("sample plugin manifest must exist")
            .replace("clipboard_write = true", "clipboard_write = false");
        let manifest = PluginManifest::parse(&source).expect("manifest must still be valid");

        let error = PluginRuntime::default()
            .invoke_action(
                &manifest,
                &sample_copy_markdown_path_directory(),
                "sample.copyMarkdownPath",
                &[SelectedEntryContext {
                    name: "report.pdf".to_owned(),
                    uri: "file:///Users/erik/Documents/report.pdf".to_owned(),
                }],
            )
            .expect_err("clipboard write must be denied without the permission");

        assert!(error.to_string().contains("permission denied"));
    }
}
