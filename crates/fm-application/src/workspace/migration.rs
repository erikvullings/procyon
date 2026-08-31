//! Schema migrations for persisted [`fm_domain::Workspace`] JSON (spec §5.3.6
//! invariant 13, §5.3.8).
//!
//! Migrations operate on [`serde_json::Value`] rather than a typed struct,
//! because an older schema version cannot, by definition, be represented by
//! the current [`fm_domain::Workspace`] type.

use chrono::Utc;
use fm_domain::CURRENT_WORKSPACE_SCHEMA_VERSION;
use serde_json::{Value, json};

use super::error::WorkspaceError;

/// Migrates a raw workspace JSON value forward to
/// [`CURRENT_WORKSPACE_SCHEMA_VERSION`], applying each version's migration in
/// turn. A value with no `schema_version` field is treated as schema version
/// 0.
///
/// Version 0 names the field set task 0078 shipped for [`fm_domain::Workspace`]
/// before this task (0079) introduced `schema_version`-gated migrations and a
/// real persistence layer: no workspace file existed on disk before this
/// task, so there is no historical v0 file format to match beyond that one.
pub(super) fn migrate_workspace_json(mut value: Value) -> Result<Value, WorkspaceError> {
    let mut version = u32::try_from(
        value
            .get("schema_version")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    )
    .unwrap_or(u32::MAX);

    if version > CURRENT_WORKSPACE_SCHEMA_VERSION {
        return Err(WorkspaceError::UnsupportedSchemaVersion {
            schema_version: version,
        });
    }

    while version < CURRENT_WORKSPACE_SCHEMA_VERSION {
        value = match version {
            0 => migrate_v0_to_v1(value)?,
            1 => migrate_v1_to_v2(value)?,
            2 => migrate_v2_to_v3(value)?,
            3 => migrate_v3_to_v4(value)?,
            other => {
                return Err(WorkspaceError::UnsupportedSchemaVersion {
                    schema_version: other,
                });
            }
        };
        version += 1;
    }

    Ok(value)
}

/// Repairs the local-provider alias emitted by the original default workspace
/// builder. The registered provider is named `local`; only `file:` locations
/// carrying the obsolete `file` provider id are changed.
fn migrate_v1_to_v2(mut value: Value) -> Result<Value, WorkspaceError> {
    let object = value.as_object_mut().ok_or_else(|| {
        WorkspaceError::Serialization("workspace JSON is not an object".to_owned())
    })?;
    object.insert("schema_version".to_owned(), json!(2));

    if let Some(panes) = object.get_mut("panes").and_then(Value::as_array_mut) {
        for pane in panes {
            let Some(tabs) = pane.get_mut("tabs").and_then(Value::as_array_mut) else {
                continue;
            };
            for tab in tabs {
                normalize_local_location(&mut tab["location"]);
                if let Some(history) = tab.get_mut("history") {
                    for direction in ["back", "forward"] {
                        if let Some(locations) =
                            history.get_mut(direction).and_then(Value::as_array_mut)
                        {
                            for location in locations {
                                normalize_local_location(location);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(value)
}

/// Repairs locations written before task 0060, when the default workspace
/// built its URI as `format!("file://{path}")`. That only produced a valid URI
/// for POSIX paths; a Windows path became `file://C:\Users\erik`, which every
/// later listing rejected as an invalid URI.
fn migrate_v2_to_v3(mut value: Value) -> Result<Value, WorkspaceError> {
    let object = value.as_object_mut().ok_or_else(|| {
        WorkspaceError::Serialization("workspace JSON is not an object".to_owned())
    })?;
    object.insert("schema_version".to_owned(), json!(3));

    if let Some(panes) = object.get_mut("panes").and_then(Value::as_array_mut) {
        for pane in panes {
            let Some(tabs) = pane.get_mut("tabs").and_then(Value::as_array_mut) else {
                continue;
            };
            for tab in tabs {
                repair_native_path_location(&mut tab["location"]);
                if let Some(history) = tab.get_mut("history") {
                    for direction in ["back", "forward"] {
                        if let Some(locations) =
                            history.get_mut(direction).and_then(Value::as_array_mut)
                        {
                            for location in locations {
                                repair_native_path_location(location);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(value)
}

/// Adds `ephemeral`/`forked_from` (ephemeral per-window workspaces): every
/// pre-existing workspace file predates the concept, so it becomes a named
/// (non-ephemeral) workspace with no fork lineage.
fn migrate_v3_to_v4(mut value: Value) -> Result<Value, WorkspaceError> {
    let object = value.as_object_mut().ok_or_else(|| {
        WorkspaceError::Serialization("workspace JSON is not an object".to_owned())
    })?;
    object.insert("schema_version".to_owned(), json!(4));
    object.entry("ephemeral").or_insert(json!(false));
    object.entry("forked_from").or_insert(json!(null));

    Ok(value)
}

/// A `file:` URI whose path does not start with `/` is a raw native path that
/// was concatenated rather than encoded. Reconversion uses the host's own
/// path rules, which is correct because a workspace file is only ever read on
/// the machine that wrote it.
fn repair_native_path_location(location: &mut Value) {
    let Some(object) = location.as_object_mut() else {
        return;
    };
    let Some(uri) = object.get("uri").and_then(Value::as_str) else {
        return;
    };
    let Some(path) = uri.strip_prefix("file://") else {
        return;
    };
    if path.is_empty() || path.starts_with('/') {
        return;
    }
    if let Ok(repaired) = fm_domain::Location::from_native_path(std::path::Path::new(path)) {
        object.insert("uri".to_owned(), json!(repaired.uri));
    }
}

fn normalize_local_location(location: &mut Value) {
    let Some(object) = location.as_object_mut() else {
        return;
    };
    let is_file_uri = object
        .get("uri")
        .and_then(Value::as_str)
        .is_some_and(|uri| uri.starts_with("file:"));
    let uses_obsolete_alias = object.get("provider_id").and_then(Value::as_str) == Some("file");
    if is_file_uri && uses_obsolete_alias {
        object.insert("provider_id".to_owned(), json!("local"));
    }
}

/// Upgrades the pre-schema-versioning field set to schema version 1: adds
/// `operation_centre`/`created_at`/`updated_at`/`revision` at the workspace
/// level, `title`/`default_view` per pane, and `title_override`/`pinned` per
/// tab, each defaulted to a value consistent with what already existed
/// (§5.3.6 invariant 13's "the workspace schema is supported or can be
/// migrated").
fn migrate_v0_to_v1(mut value: Value) -> Result<Value, WorkspaceError> {
    let now = Utc::now().to_rfc3339();

    let object = value.as_object_mut().ok_or_else(|| {
        WorkspaceError::Serialization("workspace JSON is not an object".to_owned())
    })?;

    object.insert("schema_version".to_owned(), json!(1));
    object.entry("created_at").or_insert(json!(now));
    object.entry("updated_at").or_insert(json!(now));
    object.entry("revision").or_insert(json!(1));
    object
        .entry("operation_centre")
        .or_insert(json!({ "visible": false, "height": 0 }));

    if let Some(panes) = object.get_mut("panes").and_then(Value::as_array_mut) {
        for pane in panes {
            let default_view = pane
                .get("tabs")
                .and_then(Value::as_array)
                .and_then(|tabs| tabs.first())
                .and_then(|tab| tab.get("view"))
                .cloned()
                .unwrap_or_else(default_directory_view_configuration);

            let Some(pane_object) = pane.as_object_mut() else {
                continue;
            };
            pane_object.entry("title").or_insert(json!(null));
            pane_object.entry("default_view").or_insert(default_view);

            if let Some(tabs) = pane_object.get_mut("tabs").and_then(Value::as_array_mut) {
                for tab in tabs {
                    if let Some(tab_object) = tab.as_object_mut() {
                        tab_object.entry("title_override").or_insert(json!(null));
                        tab_object.entry("pinned").or_insert(json!(false));
                    }
                }
            }
        }
    }

    Ok(value)
}

fn default_directory_view_configuration() -> Value {
    json!({
        "sort": [],
        "columns": [],
        "show_hidden": false,
        "folders_first": false,
        "quick_filter": null,
    })
}

#[cfg(test)]
mod tests {
    use fm_domain::Workspace;
    use serde_json::json;

    use super::*;

    /// A representative pre-0079 (task 0078-shaped) workspace: no
    /// `schema_version`, `operation_centre`, `created_at`, `updated_at`,
    /// `revision`, per-pane `title`/`default_view` or per-tab
    /// `title_override`/`pinned`.
    fn v0_fixture() -> Value {
        json!({
            "id": "985d4d6e-c37b-4135-90a0-ce0afe165fd9",
            "name": "Development",
            "layout": {
                "type": "pane",
                "paneId": "11e67e3e-813c-44c5-9426-53be347ad5da"
            },
            "active_pane_id": "11e67e3e-813c-44c5-9426-53be347ad5da",
            "panes": [
                {
                    "id": "11e67e3e-813c-44c5-9426-53be347ad5da",
                    "active_tab_id": "97512c58-9cf8-4f17-a931-94f0be87a1da",
                    "tabs": [
                        {
                            "id": "97512c58-9cf8-4f17-a931-94f0be87a1da",
                            "location": { "provider_id": "local", "uri": "file:///Users/erik/dev" },
                            "history": { "back": [], "forward": [] },
                            "view": {
                                "sort": [{ "column_id": "core.name", "direction": "Ascending" }],
                                "columns": [{ "column_id": "core.name", "width": 360, "visible": true }],
                                "show_hidden": true,
                                "folders_first": true,
                                "quick_filter": null
                            }
                        }
                    ]
                }
            ]
        })
    }

    #[test]
    fn migrates_a_v0_fixture_forward_into_a_valid_current_workspace() {
        let migrated = migrate_workspace_json(v0_fixture()).expect("migration must succeed");
        let workspace: Workspace =
            serde_json::from_value(migrated).expect("migrated JSON must deserialize");

        assert_eq!(workspace.schema_version, CURRENT_WORKSPACE_SCHEMA_VERSION);
        assert_eq!(workspace.revision, 1);
        assert_eq!(workspace.panes[0].title, None);
        assert_eq!(
            workspace.panes[0].default_view.sort[0].column_id,
            "core.name"
        );
        assert!(!workspace.panes[0].tabs[0].pinned);
        assert_eq!(workspace.panes[0].tabs[0].title_override, None);
        assert!(workspace.validate().is_ok());
    }

    #[test]
    fn migration_is_idempotent_once_already_at_the_current_version() {
        let migrated_once =
            migrate_workspace_json(v0_fixture()).expect("first migration must succeed");
        let migrated_twice =
            migrate_workspace_json(migrated_once.clone()).expect("second migration must succeed");

        assert_eq!(migrated_once, migrated_twice);
    }

    #[test]
    fn migrates_file_provider_aliases_in_tabs_and_history_to_local() {
        let mut value = migrate_v0_to_v1(v0_fixture()).expect("v1 fixture must migrate");
        value["panes"][0]["tabs"][0]["location"]["provider_id"] = json!("file");
        value["panes"][0]["tabs"][0]["history"]["back"] =
            json!([{ "provider_id": "file", "uri": "file:///Users" }]);

        let migrated = migrate_workspace_json(value).expect("alias migration must succeed");

        assert_eq!(
            migrated["schema_version"],
            json!(CURRENT_WORKSPACE_SCHEMA_VERSION)
        );
        assert_eq!(
            migrated["panes"][0]["tabs"][0]["location"]["provider_id"],
            json!("local")
        );
        assert_eq!(
            migrated["panes"][0]["tabs"][0]["history"]["back"][0]["provider_id"],
            json!("local")
        );
    }

    /// Workspaces written before task 0060 stored the home directory as
    /// `format!("file://{path}")`, which produced an unparseable URI on Windows.
    #[cfg(windows)]
    #[test]
    fn repairs_windows_native_paths_stored_as_malformed_file_uris() {
        let mut value = migrate_v0_to_v1(v0_fixture()).expect("v1 fixture must migrate");
        value["panes"][0]["tabs"][0]["location"] =
            json!({ "provider_id": "local", "uri": r"file://C:\Users\erik" });
        value["panes"][0]["tabs"][0]["history"]["back"] =
            json!([{ "provider_id": "local", "uri": r"file://C:\Users" }]);

        let migrated = migrate_workspace_json(value).expect("repair migration must succeed");

        assert_eq!(
            migrated["panes"][0]["tabs"][0]["location"]["uri"],
            json!("file:///C:/Users/erik")
        );
        assert_eq!(
            migrated["panes"][0]["tabs"][0]["history"]["back"][0]["uri"],
            json!("file:///C:/Users")
        );
    }

    #[test]
    fn already_valid_locations_are_left_untouched_by_the_repair() {
        let mut value = migrate_v0_to_v1(v0_fixture()).expect("v1 fixture must migrate");
        value["panes"][0]["tabs"][0]["location"] =
            json!({ "provider_id": "local", "uri": "file:///Users/erik" });

        let migrated = migrate_workspace_json(value).expect("repair migration must succeed");

        assert_eq!(
            migrated["panes"][0]["tabs"][0]["location"]["uri"],
            json!("file:///Users/erik")
        );
    }

    #[test]
    fn migrates_a_pre_ephemeral_workspace_into_a_named_non_ephemeral_one() {
        let migrated = migrate_workspace_json(v0_fixture()).expect("migration must succeed");
        let workspace: Workspace =
            serde_json::from_value(migrated).expect("migrated JSON must deserialize");

        assert!(!workspace.ephemeral);
        assert_eq!(workspace.forked_from, None);
    }

    #[test]
    fn a_schema_version_newer_than_current_is_rejected() {
        let mut value = v0_fixture();
        value["schema_version"] = json!(CURRENT_WORKSPACE_SCHEMA_VERSION + 1);

        let error =
            migrate_workspace_json(value).expect_err("future schema version must be rejected");
        assert_eq!(
            error,
            WorkspaceError::UnsupportedSchemaVersion {
                schema_version: CURRENT_WORKSPACE_SCHEMA_VERSION + 1
            }
        );
    }
}
