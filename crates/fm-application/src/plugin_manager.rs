//! Plugin lifecycle manager (task 0123).
//!
//! Manages plugin discovery, enable/disable lifecycle, icon asset serving,
//! log access, and action invocation.

use std::sync::{Arc, Mutex};

use std::path::PathBuf;

use fm_domain::{ActionContextRequirements, ActionDescriptor, ActionId, ActionSource, PluginId};
use fm_events::{
    BackendEventPayload, EventAudience, EventBus, NotificationLevelPayload, NotificationPayload,
    PluginPayload,
};
use fm_plugin_api::{
    ActionContribution, IconThemeManifest, PluginManifest, PluginPermissions, SelectedEntryContext,
};
use fm_plugin_runtime::{PluginDiscovery, PluginRuntime};
use fm_settings::Settings;
use fm_transport_dto::{
    ActionResultDto, PluginDescriptorDto, PluginLogEntryDto, PluginPermissionsDto,
};
use uuid::Uuid;

use crate::error::ApplicationError;

pub(crate) struct PluginManager {
    plugins: PluginDiscovery,
    plugin_runtime: PluginRuntime,
    settings: Arc<Mutex<Settings>>,
    settings_store: fm_settings::SettingsStore,
    events: EventBus,
}

impl PluginManager {
    pub(crate) fn new(
        plugins: PluginDiscovery,
        plugin_runtime: PluginRuntime,
        settings: Arc<Mutex<Settings>>,
        settings_store: fm_settings::SettingsStore,
        events: EventBus,
    ) -> Self {
        Self {
            plugins,
            plugin_runtime,
            settings,
            settings_store,
            events,
        }
    }

    /// See [`crate::FileManagerService::set_bundled_plugins_directory`].
    pub(crate) fn set_bundled_plugins_directory(&mut self, directory: PathBuf) {
        self.plugins = self
            .plugins
            .clone()
            .with_replaced_bundled_directory(directory);
    }

    /// Manifests and directories of every plugin that is both valid and
    /// enabled. Shared by action listing and plugin action dispatch.
    fn enabled_plugin_manifests(&self) -> Vec<(PluginManifest, std::path::PathBuf)> {
        let enabled = self
            .settings
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .enabled_plugins
            .clone();
        self.plugins
            .discover()
            .into_iter()
            .filter_map(|plugin| {
                let manifest = plugin.manifest?;
                enabled
                    .contains(&manifest.id)
                    .then_some((manifest, plugin.directory))
            })
            .collect()
    }

    /// Finds an enabled plugin's action contribution by id, along with the
    /// manifest and directory needed to invoke it.
    pub(crate) fn find_plugin_action(
        &self,
        action_id: &ActionId,
    ) -> Option<(PluginManifest, std::path::PathBuf, ActionDescriptor)> {
        self.enabled_plugin_manifests()
            .into_iter()
            .find_map(|(manifest, directory)| {
                let contributions = self.plugin_runtime.actions(&manifest, &directory).ok()?;
                let action = contributions
                    .into_iter()
                    .find(|action| action.id == action_id.as_str())?;
                let descriptor = plugin_action_descriptor(&manifest, action);
                Some((manifest, directory, descriptor))
            })
    }

    /// Runs a plugin's `invoke(action_id)` entrypoint with the caller-supplied
    /// selection.
    pub(crate) fn invoke_plugin_action(
        &self,
        action_id: &ActionId,
        manifest: &PluginManifest,
        directory: &std::path::Path,
        parameters: Option<serde_json::Value>,
    ) -> Result<ActionResultDto, ApplicationError> {
        let selection = parameters
            .map(serde_json::from_value::<PluginActionParametersDto>)
            .transpose()
            .map_err(|error| {
                ApplicationError::InvalidRequest(format!("invalid action parameters: {error}"))
            })?
            .unwrap_or_default()
            .selected_entries;

        match self
            .plugin_runtime
            .invoke_action(manifest, directory, action_id.as_str(), &selection)
        {
            Ok(outcome) => {
                if outcome.clipboard_text.is_some() {
                    self.events.publish(
                        EventAudience::Global,
                        BackendEventPayload::NotificationCreated {
                            notification: NotificationPayload {
                                id: Uuid::new_v4().to_string(),
                                level: NotificationLevelPayload::Info,
                                message: "Copied to clipboard.".to_owned(),
                            },
                        },
                    );
                }
                Ok(ActionResultDto {
                    action_id: action_id.as_str().to_owned(),
                    invoked: true,
                    operation_id: None,
                    clipboard_text: outcome.clipboard_text,
                })
            }
            Err(error) => Err(ApplicationError::InvalidRequest(format!(
                "plugin action {action_id:?} failed: {error}"
            ))),
        }
    }

    /// Returns all plugin actions from enabled plugins along with the
    /// manifest and directory needed for invocation, publishing isolation
    /// warnings for plugins that fail to load.
    pub(crate) fn list_plugin_actions(&self) -> Vec<(ActionDescriptor, PluginManifest, PathBuf)> {
        let mut actions = Vec::new();
        for (manifest, directory) in self.enabled_plugin_manifests() {
            match self.plugin_runtime.actions(&manifest, &directory) {
                Ok(contributions) => actions.extend(contributions.into_iter().map(|action| {
                    (
                        plugin_action_descriptor(&manifest, action),
                        manifest.clone(),
                        directory.clone(),
                    )
                })),
                Err(error) => {
                    self.events.publish(
                        EventAudience::Global,
                        BackendEventPayload::NotificationCreated {
                            notification: NotificationPayload {
                                id: Uuid::new_v4().to_string(),
                                level: NotificationLevelPayload::Warning,
                                message: format!("Plugin {} was isolated: {error}", manifest.id),
                            },
                        },
                    );
                }
            }
        }
        actions
    }

    /// Lists discovered plugins, retaining malformed manifests as disabled records.
    pub(crate) fn list_plugins(&self) -> Vec<PluginDescriptorDto> {
        let enabled = self
            .settings
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .enabled_plugins
            .clone();
        self.plugins
            .discover()
            .into_iter()
            .map(|plugin| {
                let id = plugin.id();
                let (name, version, description) = plugin.manifest.as_ref().map_or_else(
                    || (id.clone(), String::new(), String::new()),
                    |manifest| {
                        (
                            manifest.name.clone(),
                            manifest.version.clone(),
                            manifest.description.clone(),
                        )
                    },
                );
                let columns =
                    if plugin.is_valid() && enabled.contains(&id) {
                        match plugin.manifest.as_ref().map(|manifest| {
                            self.plugin_runtime.columns(manifest, &plugin.directory)
                        }) {
                            Some(Ok(cols)) => cols
                                .into_iter()
                                .map(|column| fm_transport_dto::PluginColumnDto {
                                    id: column.id,
                                    title: column.title,
                                })
                                .collect(),
                            Some(Err(error)) => {
                                self.events.publish(
                                    EventAudience::Global,
                                    BackendEventPayload::NotificationCreated {
                                        notification: NotificationPayload {
                                            id: Uuid::new_v4().to_string(),
                                            level: NotificationLevelPayload::Warning,
                                            message: format!("Plugin {id} was isolated: {error}"),
                                        },
                                    },
                                );
                                Vec::new()
                            }
                            None => Vec::new(),
                        }
                    } else {
                        Vec::new()
                    };
                let runtime_diagnostic = self.plugin_runtime.disabled_reason(&id);
                let permissions = plugin
                    .manifest
                    .as_ref()
                    .map(|manifest| plugin_permissions_dto(&manifest.permissions))
                    .unwrap_or_default();
                let is_enabled =
                    plugin.is_valid() && enabled.contains(&id) && runtime_diagnostic.is_none();
                let icon_theme = if plugin.is_valid() {
                    plugin.icon_theme.as_ref().map(plugin_icon_theme_dto)
                } else {
                    None
                };
                PluginDescriptorDto {
                    enabled: is_enabled,
                    id,
                    name,
                    version,
                    description,
                    diagnostic: plugin.diagnostic.or(runtime_diagnostic),
                    columns,
                    permissions,
                    icon_theme,
                }
            })
            .collect()
    }

    /// Reads one asset referenced by an enabled plugin's icon theme,
    /// rejecting any path that escapes the plugin's directory.
    pub(crate) fn plugin_icon_theme_asset(
        &self,
        plugin_id: &str,
        asset_path: &str,
    ) -> Result<String, ApplicationError> {
        let enabled = self
            .settings
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .enabled_plugins
            .clone();
        let plugin = self
            .plugins
            .discover()
            .into_iter()
            .find(|plugin| plugin.is_valid() && plugin.id() == plugin_id)
            .filter(|_plugin| enabled.iter().any(|id| id == plugin_id))
            .ok_or(ApplicationError::NotFound)?;
        let icon_theme = plugin
            .icon_theme
            .as_ref()
            .ok_or(ApplicationError::NotFound)?;
        let is_declared = icon_theme
            .icon_definitions
            .values()
            .any(|definition| definition.icon_path.to_string_lossy() == asset_path);
        if !is_declared {
            return Err(ApplicationError::NotFound);
        }
        let resolved = fm_plugin_runtime::resolve_plugin_asset(
            &plugin.directory,
            std::path::Path::new(asset_path),
        )
        .ok_or(ApplicationError::NotFound)?;
        std::fs::read_to_string(&resolved)
            .map_err(|error| ApplicationError::PlatformOperationFailed(error.to_string()))
    }

    /// Returns the bounded diagnostic log retained for one plugin.
    pub(crate) fn plugin_logs(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<PluginLogEntryDto>, ApplicationError> {
        let exists = self
            .plugins
            .discover()
            .into_iter()
            .any(|plugin| plugin.id() == plugin_id);
        if !exists {
            return Err(ApplicationError::NotFound);
        }
        Ok(self
            .plugin_runtime
            .logs(plugin_id)
            .into_iter()
            .map(|entry| PluginLogEntryDto {
                message: entry.message,
            })
            .collect())
    }

    /// Persists a plugin enablement decision after confirming its manifest is valid.
    pub(crate) fn set_plugin_enabled(
        &self,
        plugin_id: String,
        enabled: bool,
    ) -> Result<(), ApplicationError> {
        let plugin = self
            .plugins
            .discover()
            .into_iter()
            .find(|plugin| plugin.is_valid() && plugin.id() == plugin_id);
        let Some(plugin) = plugin else {
            return Err(ApplicationError::NotFound);
        };
        let manifest = plugin
            .manifest
            .as_ref()
            .expect("validated plugin has a manifest");
        let mut settings = self
            .settings
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        settings.enabled_plugins.retain(|id| id != &plugin_id);
        if enabled {
            self.plugin_runtime.reenable(&plugin_id);
            settings.enabled_plugins.push(plugin_id.clone());
            settings.enabled_plugins.sort();
        }
        self.settings_store
            .save(&settings)
            .map_err(|_| ApplicationError::Internal)?;
        self.events.publish(
            EventAudience::Global,
            BackendEventPayload::PluginChanged {
                plugin: PluginPayload {
                    id: PluginId::new(plugin_id),
                    name: manifest.name.clone(),
                    version: manifest.version.clone(),
                    enabled,
                },
            },
        );
        Ok(())
    }
}

/// Projects a manifest's declared capability grants into the wire DTO.
fn plugin_permissions_dto(permissions: &PluginPermissions) -> PluginPermissionsDto {
    PluginPermissionsDto {
        selected_entry_metadata: permissions.selected_entry_metadata,
        selected_entry_content_read: permissions.selected_entry_content_read,
        filesystem_read: permissions
            .filesystem_read
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        filesystem_write: permissions
            .filesystem_write
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        clipboard_read: permissions.clipboard_read,
        clipboard_write: permissions.clipboard_write,
        network: permissions.network.clone(),
        process_spawn: permissions.process_spawn,
        notifications: permissions.notifications,
        settings_storage: permissions.settings_storage,
    }
}

/// Projects a plugin's parsed icon-theme.json into the wire DTO.
fn plugin_icon_theme_dto(icon_theme: &IconThemeManifest) -> fm_transport_dto::PluginIconThemeDto {
    fm_transport_dto::PluginIconThemeDto {
        icon_definitions: icon_theme
            .icon_definitions
            .iter()
            .map(|(key, definition)| {
                (
                    key.clone(),
                    fm_transport_dto::PluginIconDefinitionDto {
                        icon_path: definition.icon_path.to_string_lossy().into_owned(),
                    },
                )
            })
            .collect(),
        file: icon_theme.file.clone(),
        folder: icon_theme.folder.clone(),
        symlink: icon_theme.symlink.clone(),
        file_extensions: icon_theme.file_extensions.clone(),
        file_names: icon_theme.file_names.clone(),
        folder_names: icon_theme.folder_names.clone(),
        folder_names_expanded: icon_theme.folder_names_expanded.clone(),
        mime_prefixes: icon_theme.mime_prefixes.clone(),
    }
}

/// Builds an [`ActionDescriptor`] for a plugin's declared action contribution.
fn plugin_action_descriptor(
    manifest: &PluginManifest,
    action: ActionContribution,
) -> ActionDescriptor {
    let context_requirements = if action.requires_single_selection {
        ActionContextRequirements::single_selection()
    } else {
        ActionContextRequirements::none()
    };
    ActionDescriptor {
        id: ActionId::new(action.id),
        title: action.title,
        description: Some(action.description),
        category: "plugin".to_owned(),
        default_shortcuts: Vec::new(),
        context_requirements,
        parameter_schema: None,
        source: ActionSource::Plugin {
            plugin_id: PluginId::new(manifest.id.clone()),
        },
    }
}

/// Invocation parameters a caller supplies for a plugin action that needs
/// the current selection's metadata.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginActionParametersDto {
    #[serde(default)]
    selected_entries: Vec<SelectedEntryContext>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager(dir: &tempfile::TempDir) -> (std::path::PathBuf, PluginManager) {
        let settings_dir = dir.path().join("settings");
        std::fs::create_dir_all(&settings_dir).expect("create settings dir");
        let plugins_dir = settings_dir.join("plugins");
        let events = EventBus::default();
        let settings_store = fm_settings::SettingsStore::new(&settings_dir);
        let settings = Arc::new(Mutex::new(Settings::default()));
        let plugins = PluginDiscovery::new(&plugins_dir);
        let plugin_runtime = PluginRuntime::default();
        (
            plugins_dir.clone(),
            PluginManager::new(plugins, plugin_runtime, settings, settings_store, events),
        )
    }

    #[test]
    fn set_plugin_enabled_persists_to_settings() {
        let (plugins_dir, manager) = manager(&tempfile::tempdir().expect("temp dir"));
        let plugin = plugins_dir.join("test-plugin");
        std::fs::create_dir_all(&plugin).expect("plugin directory");
        std::fs::write(
            plugin.join("plugin.toml"),
            "id='test.plugin'\nname='Test'\nversion='1'\napi_version='1'\ndescription='Test plugin'\nentrypoint='plugin.lua'",
        )
        .expect("manifest");
        std::fs::write(plugin.join("plugin.lua"), "return {}").expect("script");

        manager
            .set_plugin_enabled("test.plugin".to_owned(), true)
            .expect("enable must succeed");

        let plugins = manager.list_plugins();
        assert!(plugins.iter().any(|p| p.enabled && p.id == "test.plugin"));
    }

    #[test]
    fn plugin_logs_reports_not_found_for_undiscovered_plugin() {
        let (_plugins_dir, manager) = manager(&tempfile::tempdir().expect("temp dir"));
        let error = manager
            .plugin_logs("unknown.plugin")
            .expect_err("unknown plugin must be reported as not found");
        assert!(matches!(error, ApplicationError::NotFound));
    }

    #[test]
    fn plugin_icon_theme_asset_reports_not_found_for_unknown_plugin() {
        let (_plugins_dir, manager) = manager(&tempfile::tempdir().expect("temp dir"));
        let error = manager
            .plugin_icon_theme_asset("unknown.plugin", "icons/test.svg")
            .expect_err("unknown plugin must be reported as not found");
        assert!(matches!(error, ApplicationError::NotFound));
    }
}
