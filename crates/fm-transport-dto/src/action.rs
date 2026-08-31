//! Wire types for the backend action registry (spec §18).

use fm_domain::{
    ActionContextRequirements, ActionDescriptor, ActionInvocationContext, ActionSource, KeyChord,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// A keyboard shortcut assigned to an action (spec §18).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeyChordDto {
    /// The non-modifier key, e.g. `"c"` or `"F2"`.
    pub key: String,
    /// Control key (the Windows/Linux primary modifier).
    #[serde(default)]
    pub ctrl: bool,
    /// Shift key.
    #[serde(default)]
    pub shift: bool,
    /// Alt/Option key.
    #[serde(default)]
    pub alt: bool,
    /// Command/Meta key (the macOS primary modifier).
    #[serde(default)]
    pub meta: bool,
}

impl From<KeyChord> for KeyChordDto {
    fn from(chord: KeyChord) -> Self {
        Self {
            key: chord.key,
            ctrl: chord.ctrl,
            shift: chord.shift,
            alt: chord.alt,
            meta: chord.meta,
        }
    }
}

impl From<KeyChordDto> for KeyChord {
    fn from(dto: KeyChordDto) -> Self {
        Self {
            key: dto.key,
            ctrl: dto.ctrl,
            shift: dto.shift,
            alt: dto.alt,
            meta: dto.meta,
        }
    }
}

/// Whether an action is built in or contributed by a plugin (spec §18, §19).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ActionSourceDto {
    /// Registered by the application itself.
    Core,
    /// Contributed by an enabled plugin.
    Plugin {
        /// The contributing plugin's stable id.
        ///
        /// Explicit rename: utoipa's `ToSchema` derive does not honour the
        /// container's `rename_all_fields`/`rename_all` for struct-like enum
        /// variant fields, so without this the generated OpenAPI schema (and
        /// the Orval client built from it) would advertise `plugin_id`
        /// while the actual wire JSON (governed by serde) is `pluginId`.
        #[schema(rename = "pluginId")]
        plugin_id: String,
    },
}

impl From<ActionSource> for ActionSourceDto {
    fn from(source: ActionSource) -> Self {
        match source {
            ActionSource::Core => Self::Core,
            ActionSource::Plugin { plugin_id } => Self::Plugin {
                plugin_id: plugin_id.as_str().to_owned(),
            },
        }
    }
}

/// Predicates the backend re-validates before invoking an action (spec §18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActionContextRequirementsDto {
    /// Whether the action's feature is implemented yet.
    pub feature_available: bool,
    /// At least one entry must be selected.
    pub requires_selection: bool,
    /// Exactly one entry must be selected.
    pub requires_single_selection: bool,
}

impl From<ActionContextRequirements> for ActionContextRequirementsDto {
    fn from(requirements: ActionContextRequirements) -> Self {
        Self {
            feature_available: requirements.feature_available,
            requires_selection: requirements.requires_selection,
            requires_single_selection: requirements.requires_single_selection,
        }
    }
}

/// Describes one invokable action (spec §18).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "id": "core.rename",
    "title": "Rename",
    "description": null,
    "category": "fileOperations",
    "defaultShortcuts": [{"key": "F2", "ctrl": false, "shift": false, "alt": false, "meta": false}],
    "contextRequirements": {"featureAvailable": true, "requiresSelection": true, "requiresSingleSelection": true},
    "parameterSchema": null,
    "source": {"kind": "core"}
}))]
pub struct ActionDescriptorDto {
    /// Stable action identifier, e.g. `"core.copy"`.
    pub id: String,
    /// Short, user-facing label.
    pub title: String,
    /// Longer, optional user-facing description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Grouping used by menus and the command palette.
    pub category: String,
    /// Default keyboard shortcuts, before user overrides.
    #[serde(default)]
    pub default_shortcuts: Vec<KeyChordDto>,
    /// Predicates the backend re-validates before invoking this action.
    pub context_requirements: ActionContextRequirementsDto,
    /// Optional JSON schema describing `InvokeActionRequest.parameters`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub parameter_schema: Option<serde_json::Value>,
    /// Whether this is a core action or contributed by a plugin.
    pub source: ActionSourceDto,
}

impl From<ActionDescriptor> for ActionDescriptorDto {
    fn from(descriptor: ActionDescriptor) -> Self {
        Self {
            id: descriptor.id.as_str().to_owned(),
            title: descriptor.title,
            description: descriptor.description,
            category: descriptor.category,
            default_shortcuts: descriptor
                .default_shortcuts
                .into_iter()
                .map(Into::into)
                .collect(),
            context_requirements: descriptor.context_requirements.into(),
            parameter_schema: descriptor.parameter_schema,
            source: descriptor.source.into(),
        }
    }
}

/// Typed context supplied with an action invocation: the active pane, the
/// current selection, and the cursor entry (spec §18). The backend
/// re-validates `ActionContextRequirements` against this rather than trusting
/// the frontend's own advisory availability evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActionInvocationContextDto {
    /// The pane the action was invoked from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Uuid>,
    /// Currently selected entries in that pane.
    #[serde(default)]
    pub selected_entry_ids: Vec<Uuid>,
    /// The entry under the keyboard cursor, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_entry_id: Option<Uuid>,
}

impl From<ActionInvocationContextDto> for ActionInvocationContext {
    fn from(dto: ActionInvocationContextDto) -> Self {
        Self {
            pane_id: dto.pane_id.map(Into::into),
            selected_entry_ids: dto.selected_entry_ids.into_iter().map(Into::into).collect(),
            cursor_entry_id: dto.cursor_entry_id.map(Into::into),
        }
    }
}

/// Requests invocation of a registered action, identified by the
/// `actionId` path parameter (spec §18).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InvokeActionRequestDto {
    /// Action-specific parameters. For mutating actions this deserializes as
    /// a [`crate::StartOperationRequestDto`] (its `type` field is ignored and
    /// overridden by the invoked action id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub parameters: Option<serde_json::Value>,
    /// The invoking pane, selection and cursor entry.
    #[serde(default)]
    pub context: ActionInvocationContextDto,
}

/// Result of invoking one action (spec §18).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActionResultDto {
    /// The invoked action's id, echoed back for convenience.
    pub action_id: String,
    /// Whether the action ran (always `true` on a non-error response).
    pub invoked: bool,
    /// The operation started on behalf of this action, for mutating actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<Uuid>,
    /// Text a plugin action asked the host to write to the clipboard, e.g.
    /// `sample.copyMarkdownPath` (spec §20). The caller performs the actual
    /// OS/browser clipboard write; the backend only authorizes and
    /// generates the content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clipboard_text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_domain::ActionId;

    #[test]
    fn action_descriptor_dto_round_trips_through_serde_json() {
        let dto = ActionDescriptorDto {
            id: "core.rename".to_owned(),
            title: "Rename".to_owned(),
            description: None,
            category: "fileOperations".to_owned(),
            default_shortcuts: vec![KeyChordDto {
                key: "F2".to_owned(),
                ..KeyChordDto::default()
            }],
            context_requirements: ActionContextRequirementsDto {
                feature_available: true,
                requires_selection: true,
                requires_single_selection: true,
            },
            parameter_schema: None,
            source: ActionSourceDto::Core,
        };

        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        let parsed: ActionDescriptorDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(dto, parsed);
    }

    #[test]
    fn action_descriptor_dto_matches_the_spec_18_camel_case_shape() {
        let json = serde_json::to_string(&ActionDescriptorDto {
            id: "core.copy".to_owned(),
            title: "Copy".to_owned(),
            description: None,
            category: "fileOperations".to_owned(),
            default_shortcuts: Vec::new(),
            context_requirements: ActionContextRequirementsDto {
                feature_available: true,
                requires_selection: true,
                requires_single_selection: false,
            },
            parameter_schema: None,
            source: ActionSourceDto::Core,
        })
        .expect("serialization must succeed");

        assert!(json.contains("\"defaultShortcuts\""));
        assert!(json.contains("\"contextRequirements\""));
        assert!(json.contains("\"featureAvailable\":true"));
        assert!(json.contains("\"requiresSelection\":true"));
        assert!(json.contains("\"source\":{\"kind\":\"core\"}"));
    }

    #[test]
    fn action_source_dto_plugin_variant_serializes_its_field_as_camel_case() {
        let json = serde_json::to_string(&ActionSourceDto::Plugin {
            plugin_id: "example.plugin".to_owned(),
        })
        .expect("serialization must succeed");

        assert_eq!(
            json,
            "{\"kind\":\"plugin\",\"pluginId\":\"example.plugin\"}"
        );

        let parsed: ActionSourceDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(
            parsed,
            ActionSourceDto::Plugin {
                plugin_id: "example.plugin".to_owned()
            }
        );
    }

    #[test]
    fn from_action_descriptor_converts_the_id_to_a_plain_string() {
        let descriptor = ActionDescriptor {
            id: ActionId::new("core.copy"),
            title: "Copy".to_owned(),
            description: None,
            category: "fileOperations".to_owned(),
            default_shortcuts: Vec::new(),
            context_requirements: ActionContextRequirements::selection(),
            parameter_schema: None,
            source: ActionSource::Core,
        };

        let dto: ActionDescriptorDto = descriptor.into();
        assert_eq!(dto.id, "core.copy");
    }

    #[test]
    fn invoke_action_request_dto_defaults_context_when_absent() {
        let request: InvokeActionRequestDto =
            serde_json::from_str("{}").expect("an empty object must deserialize");
        assert_eq!(request.context, ActionInvocationContextDto::default());
        assert!(request.parameters.is_none());
    }

    #[test]
    fn action_invocation_context_dto_converts_uuids_to_domain_ids() {
        let pane_id = Uuid::new_v4();
        let entry_id = Uuid::new_v4();
        let dto = ActionInvocationContextDto {
            pane_id: Some(pane_id),
            selected_entry_ids: vec![entry_id],
            cursor_entry_id: Some(entry_id),
        };

        let context: ActionInvocationContext = dto.into();
        assert_eq!(context.pane_id, Some(pane_id.into()));
        assert_eq!(context.selected_entry_ids, vec![entry_id.into()]);
        assert_eq!(context.cursor_entry_id, Some(entry_id.into()));
    }

    #[test]
    fn action_result_dto_omits_operation_id_when_absent() {
        let dto = ActionResultDto {
            action_id: "core.selectAll".to_owned(),
            invoked: true,
            operation_id: None,
            clipboard_text: None,
        };
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        assert!(!json.contains("operationId"));
        assert!(!json.contains("clipboardText"));
    }

    #[test]
    fn action_result_dto_includes_clipboard_text_when_present() {
        let dto = ActionResultDto {
            action_id: "sample.copyMarkdownPath".to_owned(),
            invoked: true,
            operation_id: None,
            clipboard_text: Some("[report.pdf](file:///report.pdf)".to_owned()),
        };
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        assert!(json.contains("\"clipboardText\":\"[report.pdf](file:///report.pdf)\""));
    }
}
