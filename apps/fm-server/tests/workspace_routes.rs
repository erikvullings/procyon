//! Integration tests for the workspace REST surface (task 0080): a full
//! command -> REST round trip for every `WorkspaceCommand` variant, the
//! stale-revision conflict shape (spec §5.3.10) and the last-tab-close
//! replacement behaviour (spec §5.3.4).

mod common;

use common::TestServer;
use serde_json::{Value, json};

async fn create_workspace(server: &TestServer, name: &str) -> Value {
    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/workspaces", server.base_url))
        .json(&json!({ "name": name }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    response.json().await.expect("body must be JSON")
}

#[tokio::test]
async fn list_workspaces_starts_empty_and_reflects_created_workspaces() {
    let server = TestServer::spawn().await;

    let response = reqwest::get(format!("{}/api/v1/workspaces", server.base_url))
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body.as_array().unwrap().len(), 0);

    create_workspace(&server, "Photos").await;

    let response = reqwest::get(format!("{}/api/v1/workspaces", server.base_url))
        .await
        .expect("request must succeed");
    let body: Value = response.json().await.expect("body must be JSON");
    let summaries = body.as_array().unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0]["name"], "Photos");
}

#[tokio::test]
async fn get_workspace_returns_the_created_workspace() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;

    let response = reqwest::get(format!(
        "{}/api/v1/workspaces/{}",
        server.base_url,
        created["id"].as_str().unwrap()
    ))
    .await
    .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["id"], created["id"]);
    assert_eq!(body["panes"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn get_workspace_reports_not_found_for_an_unknown_id() {
    let server = TestServer::spawn().await;

    let response = reqwest::get(format!(
        "{}/api/v1/workspaces/{}",
        server.base_url,
        uuid::Uuid::new_v4()
    ))
    .await
    .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["code"], "notFound");
}

#[tokio::test]
async fn open_workspace_selects_it_as_last_active() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/workspaces/{}/open",
            server.base_url,
            created["id"].as_str().unwrap()
        ))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["id"], created["id"]);
}

#[tokio::test]
async fn delete_workspace_removes_it() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;

    let response = reqwest::Client::new()
        .delete(format!(
            "{}/api/v1/workspaces/{}?expectedRevision={}",
            server.base_url,
            created["id"].as_str().unwrap(),
            created["revision"]
        ))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);

    let response = reqwest::get(format!("{}/api/v1/workspaces", server.base_url))
        .await
        .expect("request must succeed");
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body.as_array().unwrap().len(), 0);
}

/// Applies a `WorkspaceCommandDto` and returns the resulting `WorkspaceDto`.
async fn apply_command(server: &TestServer, workspace_id: &Value, command: Value) -> Value {
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/workspaces/{}/commands",
            server.base_url,
            workspace_id
                .as_str()
                .expect("workspace id must be a string")
        ))
        .json(&command)
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK, "{command}");
    response.json().await.expect("body must be JSON")
}

#[tokio::test]
async fn rename_workspace_command_renames_and_increments_the_revision() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;

    let updated = apply_command(
        &server,
        &created["id"],
        json!({
            "type": "renameWorkspace",
            "workspaceId": created["id"],
            "name": "Renamed",
            "expectedRevision": created["revision"],
        }),
    )
    .await;

    assert_eq!(updated["name"], "Renamed");
    assert!(updated["revision"].as_u64().unwrap() > created["revision"].as_u64().unwrap());
}

#[tokio::test]
async fn set_active_pane_command_activates_the_given_pane() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;
    let other_pane = created["panes"][1]["id"].clone();

    let updated = apply_command(
        &server,
        &created["id"],
        json!({
            "type": "setActivePane",
            "workspaceId": created["id"],
            "paneId": other_pane,
            "expectedRevision": created["revision"],
        }),
    )
    .await;

    assert_eq!(updated["activePaneId"], other_pane);
}

#[tokio::test]
async fn add_tab_command_appends_and_activates_a_new_tab() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;
    let pane_id = created["activePaneId"].clone();
    let tabs_before = created["panes"][0]["tabs"].as_array().unwrap().len();

    let updated = apply_command(
        &server,
        &created["id"],
        json!({
            "type": "addTab",
            "workspaceId": created["id"],
            "paneId": pane_id,
            "location": {"providerId": "local", "uri": "file:///tmp"},
            "expectedRevision": created["revision"],
        }),
    )
    .await;

    let pane = updated["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pane| pane["id"] == pane_id)
        .unwrap();
    assert_eq!(pane["tabs"].as_array().unwrap().len(), tabs_before + 1);
}

#[tokio::test]
async fn close_tab_command_on_a_panes_last_tab_creates_a_replacement() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;
    let pane_id = created["activePaneId"].clone();
    let pane = created["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pane| pane["id"] == pane_id)
        .unwrap();
    let tab_id = pane["tabs"][0]["id"].clone();

    let updated = apply_command(
        &server,
        &created["id"],
        json!({
            "type": "closeTab",
            "workspaceId": created["id"],
            "paneId": pane_id,
            "tabId": tab_id,
            "expectedRevision": created["revision"],
        }),
    )
    .await;

    let pane = updated["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pane| pane["id"] == pane_id)
        .unwrap();
    let tabs = pane["tabs"].as_array().unwrap();
    assert_eq!(tabs.len(), 1);
    assert_ne!(tabs[0]["id"], tab_id);
}

#[tokio::test]
async fn activate_tab_command_changes_the_panes_active_tab() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;
    let pane_id = created["activePaneId"].clone();

    let with_new_tab = apply_command(
        &server,
        &created["id"],
        json!({
            "type": "addTab",
            "workspaceId": created["id"],
            "paneId": pane_id,
            "location": {"providerId": "local", "uri": "file:///tmp"},
            "expectedRevision": created["revision"],
        }),
    )
    .await;
    let pane = with_new_tab["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pane| pane["id"] == pane_id)
        .unwrap();
    let first_tab_id = pane["tabs"][0]["id"].clone();

    let updated = apply_command(
        &server,
        &created["id"],
        json!({
            "type": "activateTab",
            "workspaceId": created["id"],
            "paneId": pane_id,
            "tabId": first_tab_id,
            "expectedRevision": with_new_tab["revision"],
        }),
    )
    .await;

    let pane = updated["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pane| pane["id"] == pane_id)
        .unwrap();
    assert_eq!(pane["activeTabId"], first_tab_id);
}

#[tokio::test]
async fn navigate_tab_command_updates_location_and_history() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;
    let pane_id = created["activePaneId"].clone();
    let tab_id = created["panes"][0]["tabs"][0]["id"].clone();

    let updated = apply_command(
        &server,
        &created["id"],
        json!({
            "type": "navigateTab",
            "workspaceId": created["id"],
            "paneId": pane_id,
            "tabId": tab_id,
            "location": {"providerId": "local", "uri": "file:///Users/erik/Downloads"},
            "navigationMode": "push",
            "expectedRevision": created["revision"],
        }),
    )
    .await;

    let pane = updated["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pane| pane["id"] == pane_id)
        .unwrap();
    let tab = pane["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tab| tab["id"] == tab_id)
        .unwrap();
    assert_eq!(tab["location"]["uri"], "file:///Users/erik/Downloads");

    let restored = apply_command(
        &server,
        &created["id"],
        json!({
            "type": "navigateTab",
            "workspaceId": created["id"],
            "paneId": pane_id,
            "tabId": tab_id,
            "navigationMode": "back",
            "expectedRevision": updated["revision"],
        }),
    )
    .await;
    let restored_tab = restored["panes"][0]["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tab| tab["id"] == tab_id)
        .unwrap();
    assert_ne!(
        restored_tab["location"]["uri"],
        "file:///Users/erik/Downloads"
    );
    assert_eq!(tab["location"]["uri"], "file:///Users/erik/Downloads");
    assert_eq!(tab["history"]["back"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn update_view_command_patches_only_the_given_fields() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;
    let pane_id = created["activePaneId"].clone();
    let tab_id = created["panes"][0]["tabs"][0]["id"].clone();

    let updated = apply_command(
        &server,
        &created["id"],
        json!({
            "type": "updateView",
            "workspaceId": created["id"],
            "paneId": pane_id,
            "tabId": tab_id,
            "patch": {"showHidden": true},
            "expectedRevision": created["revision"],
        }),
    )
    .await;

    let pane = updated["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pane| pane["id"] == pane_id)
        .unwrap();
    let tab = pane["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tab| tab["id"] == tab_id)
        .unwrap();
    assert_eq!(tab["view"]["showHidden"], true);
}

#[tokio::test]
async fn update_layout_command_replaces_the_layout_tree() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;
    let left_pane = created["panes"][0]["id"].clone();
    let right_pane = created["panes"][1]["id"].clone();

    let updated = apply_command(
        &server,
        &created["id"],
        json!({
            "type": "updateLayout",
            "workspaceId": created["id"],
            "layout": {
                "type": "split",
                "axis": "vertical",
                "ratio": 0.5,
                "first": {"type": "pane", "paneId": left_pane},
                "second": {"type": "pane", "paneId": right_pane},
            },
            "expectedRevision": created["revision"],
        }),
    )
    .await;

    assert_eq!(updated["layout"]["axis"], "vertical");
}

#[tokio::test]
async fn a_stale_expected_revision_reports_the_spec_5_3_10_conflict_shape() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/workspaces/{}/commands",
            server.base_url,
            created["id"].as_str().unwrap()
        ))
        .json(&json!({
            "type": "renameWorkspace",
            "workspaceId": created["id"],
            "name": "Renamed",
            "expectedRevision": created["revision"].as_u64().unwrap() + 1,
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["code"], "workspaceRevisionConflict");
    assert_eq!(body["details"]["workspaceId"], created["id"]);
    assert_eq!(
        body["details"]["expectedRevision"],
        created["revision"].as_u64().unwrap() + 1
    );
    assert_eq!(body["details"]["actualRevision"], created["revision"]);
}

#[tokio::test]
async fn a_command_whose_body_workspace_id_does_not_match_the_path_is_rejected() {
    let server = TestServer::spawn().await;
    let created = create_workspace(&server, "Default").await;
    let other = create_workspace(&server, "Other").await;

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/workspaces/{}/commands",
            server.base_url,
            created["id"].as_str().unwrap()
        ))
        .json(&json!({
            "type": "renameWorkspace",
            "workspaceId": other["id"],
            "name": "Renamed",
            "expectedRevision": other["revision"],
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
}
