//! Action-system domain types (spec §18).
//!
//! `ActionDescriptor` describes one entry in the action registry. Menus,
//! context menus, toolbars, the command palette and keyboard shortcuts all
//! invoke the same registered action rather than a bespoke handler. The
//! registry itself (registration, lookup, invocation) is application-layer
//! behaviour and lives in `fm-application`; this module holds only the plain,
//! serializable data types it operates on.

use serde::{Deserialize, Serialize};

use crate::ids::{ActionId, EntryId, PaneId, PluginId};

/// A keyboard shortcut assigned to an action (spec §18).
///
/// `ctrl`/`meta` are kept as separate fields, rather than one "primary
/// modifier", so the chord losslessly round-trips through the frontend
/// keybinding dispatcher (task 0050), which resolves Cmd-vs-Ctrl per platform.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyChord {
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

/// Whether an action is built in or contributed by a plugin (spec §18, §19).
///
/// Kept open (rather than a closed, core-only type) so a later plugin loader
/// (task 0053) can attribute an action to its contributing plugin without a
/// breaking change here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ActionSource {
    /// Registered by the application itself.
    Core,
    /// Contributed by an enabled plugin.
    Plugin {
        /// The contributing plugin.
        plugin_id: PluginId,
    },
}

/// Predicates the backend re-validates before invoking an action (spec §18).
///
/// A closed set of simple predicates rather than an arbitrary expression
/// language: every rule needed by the core actions registered in task 0049 is
/// expressible as "the feature exists" and "at least one/exactly one entry is
/// selected". Richer rules can be added here later without changing
/// `ActionDescriptor`'s shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionContextRequirements {
    /// Whether the action's feature is implemented yet. `false` means the
    /// action is registered (so menus and the palette can list it) but
    /// always reports unavailable until its owning task lands.
    #[serde(default = "default_true")]
    pub feature_available: bool,
    /// At least one entry must be selected.
    #[serde(default)]
    pub requires_selection: bool,
    /// Exactly one entry must be selected.
    #[serde(default)]
    pub requires_single_selection: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ActionContextRequirements {
    fn default() -> Self {
        Self::none()
    }
}

impl ActionContextRequirements {
    /// No requirements: available regardless of the current selection.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            feature_available: true,
            requires_selection: false,
            requires_single_selection: false,
        }
    }

    /// Registered but not yet implemented; always reports unavailable.
    #[must_use]
    pub const fn unimplemented() -> Self {
        Self {
            feature_available: false,
            requires_selection: false,
            requires_single_selection: false,
        }
    }

    /// Requires at least one selected entry.
    #[must_use]
    pub const fn selection() -> Self {
        Self {
            feature_available: true,
            requires_selection: true,
            requires_single_selection: false,
        }
    }

    /// Requires exactly one selected entry.
    #[must_use]
    pub const fn single_selection() -> Self {
        Self {
            feature_available: true,
            requires_selection: true,
            requires_single_selection: true,
        }
    }

    /// Evaluates these requirements against an invocation's context.
    #[must_use]
    pub fn is_satisfied_by(&self, context: &ActionInvocationContext) -> bool {
        if !self.feature_available {
            return false;
        }
        if self.requires_single_selection {
            return context.selected_entry_ids.len() == 1;
        }
        if self.requires_selection {
            return !context.selected_entry_ids.is_empty();
        }
        true
    }
}

/// Typed invocation context supplied by the caller: the active pane, the
/// current selection, and the cursor entry (spec §18). The backend
/// re-validates [`ActionContextRequirements`] against this rather than
/// trusting the frontend's own advisory availability evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionInvocationContext {
    /// The pane the action was invoked from, if any.
    #[serde(default)]
    pub pane_id: Option<PaneId>,
    /// Currently selected entries in that pane.
    #[serde(default)]
    pub selected_entry_ids: Vec<EntryId>,
    /// The entry under the keyboard cursor, if any.
    #[serde(default)]
    pub cursor_entry_id: Option<EntryId>,
}

/// Describes one invokable action (spec §18).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionDescriptor {
    /// Stable action identifier, e.g. `"core.copy"`.
    pub id: ActionId,
    /// Short, user-facing label.
    pub title: String,
    /// Longer, optional user-facing description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Grouping used by menus and the command palette. Left as a plain
    /// string (rather than a closed enum) so plugin-contributed actions can
    /// introduce their own categories without a breaking change.
    pub category: String,
    /// Default keyboard shortcuts, before user overrides (tasks 0030/0050).
    #[serde(default)]
    pub default_shortcuts: Vec<KeyChord>,
    /// Predicates the backend re-validates before invoking this action.
    pub context_requirements: ActionContextRequirements,
    /// Optional JSON schema describing `InvokeActionRequest.parameters`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_schema: Option<serde_json::Value>,
    /// Whether this is a core action or contributed by a plugin.
    pub source: ActionSource,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(context_requirements: ActionContextRequirements) -> ActionDescriptor {
        ActionDescriptor {
            id: ActionId::new("core.rename"),
            title: "Rename".to_owned(),
            description: None,
            category: "fileOperations".to_owned(),
            default_shortcuts: vec![KeyChord {
                key: "F2".to_owned(),
                ..KeyChord::default()
            }],
            context_requirements,
            parameter_schema: None,
            source: ActionSource::Core,
        }
    }

    #[test]
    fn none_requirements_are_satisfied_regardless_of_selection() {
        let context = ActionInvocationContext::default();
        assert!(ActionContextRequirements::none().is_satisfied_by(&context));
    }

    #[test]
    fn unimplemented_requirements_are_never_satisfied() {
        let mut context = ActionInvocationContext::default();
        context.selected_entry_ids.push(EntryId::new());
        assert!(!ActionContextRequirements::unimplemented().is_satisfied_by(&context));
    }

    #[test]
    fn selection_requirement_needs_at_least_one_entry() {
        let requirements = ActionContextRequirements::selection();
        let empty = ActionInvocationContext::default();
        assert!(!requirements.is_satisfied_by(&empty));

        let mut one = ActionInvocationContext::default();
        one.selected_entry_ids.push(EntryId::new());
        assert!(requirements.is_satisfied_by(&one));

        let mut two = ActionInvocationContext::default();
        two.selected_entry_ids.push(EntryId::new());
        two.selected_entry_ids.push(EntryId::new());
        assert!(requirements.is_satisfied_by(&two));
    }

    #[test]
    fn single_selection_requirement_rejects_zero_or_many() {
        let requirements = ActionContextRequirements::single_selection();
        let empty = ActionInvocationContext::default();
        assert!(!requirements.is_satisfied_by(&empty));

        let mut one = ActionInvocationContext::default();
        one.selected_entry_ids.push(EntryId::new());
        assert!(requirements.is_satisfied_by(&one));

        let mut two = ActionInvocationContext::default();
        two.selected_entry_ids.push(EntryId::new());
        two.selected_entry_ids.push(EntryId::new());
        assert!(!requirements.is_satisfied_by(&two));
    }

    #[test]
    fn empty_context_requirements_json_defaults_to_available_with_no_requirements() {
        let requirements: ActionContextRequirements =
            serde_json::from_str("{}").expect("an empty object must deserialize");
        assert_eq!(requirements, ActionContextRequirements::none());
    }

    #[test]
    fn action_source_serializes_with_a_kind_tag() {
        let core = serde_json::to_string(&ActionSource::Core).expect("must serialize");
        assert_eq!(core, r#"{"kind":"core"}"#);

        let plugin = serde_json::to_string(&ActionSource::Plugin {
            plugin_id: PluginId::new("sample.plugin"),
        })
        .expect("must serialize");
        assert_eq!(plugin, r#"{"kind":"plugin","pluginId":"sample.plugin"}"#);
    }

    #[test]
    fn action_descriptor_round_trips_through_serde_json() {
        let descriptor = descriptor(ActionContextRequirements::single_selection());
        let json = serde_json::to_string(&descriptor).expect("serialization must succeed");
        let parsed: ActionDescriptor =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(descriptor, parsed);
    }
}
