//! Action registry REST contract integration tests.

mod common;

use serde_json::json;

#[tokio::test]
async fn list_actions_includes_every_core_action_with_camel_case_fields() {
    let server = common::TestServer::spawn().await;
    let client = reqwest::Client::new();

    let actions: serde_json::Value = client
        .get(format!("{}/api/v1/actions", server.base_url))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let actions = actions.as_array().expect("actions must be a JSON array");
    let copy = actions
        .iter()
        .find(|action| action["id"] == "core.copy")
        .expect("core.copy must be registered");
    assert_eq!(copy["source"], json!({"kind": "core"}));
    assert_eq!(copy["contextRequirements"]["requiresSelection"], true);
    assert!(copy["defaultShortcuts"].is_array());
}

#[tokio::test]
async fn invoke_action_returns_not_found_for_an_unknown_action() {
    let server = common::TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!(
            "{}/api/v1/actions/does.not.exist/invoke",
            server.base_url
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["code"], "actionNotFound");
}

#[tokio::test]
async fn invoke_action_returns_conflict_when_context_requirements_are_not_met() {
    let server = common::TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!(
            "{}/api/v1/actions/core.rename/invoke",
            server.base_url
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["code"], "actionUnavailable");
}

#[tokio::test]
async fn invoke_action_delegates_create_directory_and_returns_an_operation_id() {
    let server = common::TestServer::spawn().await;
    let root = tempfile::tempdir().expect("must create a temporary root");
    let client = reqwest::Client::new();

    let response = client
        .post(format!(
            "{}/api/v1/actions/core.createDirectory/invoke",
            server.base_url
        ))
        .json(&json!({
            "parameters": {
                "type": "createDirectory",
                "sources": [],
                "destination": {"providerId": "local", "uri": common::file_uri(root.path())},
                "conflictPolicy": "ask",
                "name": "child"
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["actionId"], "core.createDirectory");
    assert_eq!(body["invoked"], true);
    assert!(body["operationId"].is_string());
}

#[tokio::test]
async fn invoke_action_returns_invoked_without_an_operation_for_selection_actions() {
    let server = common::TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!(
            "{}/api/v1/actions/core.selectAll/invoke",
            server.base_url
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["invoked"], true);
    assert!(body["operationId"].is_null());
}
