//! Action discovery and invocation coordination (task 0119).

use std::sync::{Arc, Mutex};

use fm_domain::{ActionId, Location};
use fm_platform::PlatformAdapter;
use fm_settings::Settings;
use fm_transport_dto::{
    ActionDescriptorDto, ActionResultDto, InvokeActionRequestDto, StartOperationRequestDto,
};

use crate::action::ActionRegistry;
use crate::error::ApplicationError;
use crate::operation_requests::mutating_operation_kind;
use crate::operations_coordinator::OperationsCoordinator;
use crate::platform_mapping::{PlatformActionKind, map_platform_error, platform_action_kind};
use crate::plugin_manager::PluginManager;

pub(crate) struct ActionInvoker {
    actions: ActionRegistry,
    platform: Arc<dyn PlatformAdapter>,
    settings: Arc<Mutex<Settings>>,
}

impl ActionInvoker {
    pub(crate) fn new(
        actions: ActionRegistry,
        platform: Arc<dyn PlatformAdapter>,
        settings: Arc<Mutex<Settings>>,
    ) -> Self {
        Self {
            actions,
            platform,
            settings,
        }
    }

    #[must_use]
    pub(crate) fn list(&self, plugins: &PluginManager) -> Vec<ActionDescriptorDto> {
        let mut actions = self.actions.list();
        for (descriptor, _, _) in plugins.list_plugin_actions() {
            actions.push(descriptor);
        }
        actions.sort_by(|left, right| left.id.cmp(&right.id));
        actions.into_iter().map(Into::into).collect()
    }

    pub(crate) fn invoke(
        &self,
        action_id: String,
        request: InvokeActionRequestDto,
        idempotency_key: Option<String>,
        plugins: &PluginManager,
        operations: &OperationsCoordinator,
    ) -> Result<ActionResultDto, ApplicationError> {
        let action_id = ActionId::new(action_id);
        let context = request.context.into();
        if let Some((manifest, directory, descriptor)) = plugins.find_plugin_action(&action_id) {
            if !descriptor.context_requirements.is_satisfied_by(&context) {
                return Err(ApplicationError::ActionUnavailable(action_id));
            }
            return plugins.invoke_plugin_action(
                &action_id,
                &manifest,
                &directory,
                request.parameters,
            );
        }
        self.actions.require_available(&action_id, &context)?;
        if let Some(kind) = platform_action_kind(&action_id) {
            return self.invoke_platform(&action_id, kind, request.parameters);
        }
        let Some(operation_type) = mutating_operation_kind(&action_id) else {
            return Ok(ActionResultDto {
                action_id: action_id.as_str().to_owned(),
                invoked: true,
                operation_id: None,
                clipboard_text: None,
            });
        };
        let parameters = request.parameters.ok_or_else(|| {
            ApplicationError::InvalidRequest(format!(
                "action {action_id:?} requires parameters describing the operation"
            ))
        })?;
        let mut operation_request: StartOperationRequestDto = serde_json::from_value(parameters)
            .map_err(|error| {
                ApplicationError::InvalidRequest(format!("invalid action parameters: {error}"))
            })?;
        operation_request.operation_type = operation_type;
        let operation = operations.start(operation_request, idempotency_key)?;
        Ok(ActionResultDto {
            action_id: action_id.as_str().to_owned(),
            invoked: true,
            operation_id: Some(operation.id),
            clipboard_text: None,
        })
    }

    fn invoke_platform(
        &self,
        action_id: &ActionId,
        kind: PlatformActionKind,
        parameters: Option<serde_json::Value>,
    ) -> Result<ActionResultDto, ApplicationError> {
        let uri = parameters
            .as_ref()
            .and_then(|value| value.get("uri"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                ApplicationError::InvalidRequest(format!(
                    "action {action_id:?} requires a `uri` string parameter"
                ))
            })?;
        let location = Location::parse(uri).map_err(|error| {
            ApplicationError::InvalidRequest(format!("invalid `uri` parameter: {error}"))
        })?;
        let path = location.to_native_path().map_err(|error| {
            if kind == PlatformActionKind::QuickLook {
                ApplicationError::ActionUnavailable(action_id.clone())
            } else {
                ApplicationError::InvalidRequest(format!("invalid `uri` parameter: {error}"))
            }
        })?;
        let result = match kind {
            PlatformActionKind::Open => self.platform.open_with_default_application(&path),
            PlatformActionKind::OpenWithChooser => self.platform.open_with_chooser(&path),
            PlatformActionKind::QuickLook => self.platform.quick_look(&path),
            PlatformActionKind::Reveal => self.platform.reveal_in_file_manager(&path),
            PlatformActionKind::EditInTextEditor => {
                let command = self
                    .settings
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .editor_command
                    .clone();
                self.platform.open_in_text_editor(&path, command.as_deref())
            }
            PlatformActionKind::OpenTerminal => {
                let command = self
                    .settings
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .terminal_command
                    .clone();
                self.platform.open_terminal(&path, command.as_deref())
            }
        };
        result.map_err(|error| map_platform_error(action_id, error))?;
        Ok(ActionResultDto {
            action_id: action_id.as_str().to_owned(),
            invoked: true,
            operation_id: None,
            clipboard_text: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use fm_events::EventBus;
    use fm_platform::{FallbackPlatformAdapter, PlatformCapabilities};
    use fm_plugin_runtime::{PluginDiscovery, PluginRuntime};
    use fm_settings::{Settings, SettingsStore};

    use super::ActionInvoker;
    use crate::action::ActionRegistry;
    use crate::plugin_manager::PluginManager;

    #[test]
    fn list_includes_core_actions() {
        let directory = tempfile::tempdir().expect("temp directory");
        let settings = Arc::new(Mutex::new(Settings::default()));
        let settings_store = SettingsStore::new(directory.path());
        let plugins = Arc::new(PluginManager::new(
            PluginDiscovery::new(directory.path().join("plugins")),
            PluginRuntime::default(),
            Arc::clone(&settings),
            settings_store,
            EventBus::default(),
        ));
        let invoker = ActionInvoker::new(
            ActionRegistry::with_core_actions(PlatformCapabilities::empty()),
            Arc::new(FallbackPlatformAdapter),
            settings,
        );

        assert!(
            invoker
                .list(&plugins)
                .iter()
                .any(|action| action.id == "core.copy")
        );
    }
}
