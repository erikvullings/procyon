//! End-to-end REST coverage for directory listing and metadata (task 0019).

mod common;

use common::TestServer;
use fm_domain::Location;
use serde_json::{Value, json};
use uuid::Uuid;

fn location_json(path: &std::path::Path) -> Value {
    let location = Location::from_native_path(path).expect("temp path must be representable");
    json!({
        "providerId": location.provider_id.as_str(),
        "uri": location.uri,
    })
}

#[tokio::test]
async fn directory_endpoints_list_refresh_navigate_and_fetch_metadata() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let file = root.path().join("report.txt");
    std::fs::write(&file, b"contents").expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let workspace_id = Uuid::new_v4();
    let pane_id = Uuid::new_v4();
    let location = location_json(root.path());

    for endpoint in ["directories/list", "directories/refresh"] {
        let response = client
            .post(format!("{}/api/v1/{endpoint}", server.base_url))
            .json(&json!({
                "workspaceId": workspace_id,
                "paneId": pane_id,
                "requestId": Uuid::new_v4(),
                "location": location,
                "sort": [{"columnId": "core.name", "direction": "ascending"}],
                "showHidden": false,
                "foldersFirst": true,
            }))
            .send()
            .await
            .expect("request must succeed");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: Value = response.json().await.expect("body must be JSON");
        assert_eq!(body["entries"][0]["name"], "report.txt");
        assert_eq!(body["loadingState"]["type"], "loaded");
    }

    let response = client
        .post(format!("{}/api/v1/navigation/open", server.base_url))
        .json(&json!({
            "workspaceId": workspace_id,
            "paneId": pane_id,
            "requestId": Uuid::new_v4(),
            "location": location,
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let listing: Value = response.json().await.expect("body must be JSON");

    let response = client
        .post(format!("{}/api/v1/entries/metadata", server.base_url))
        .json(&json!({
            "entryId": listing["entries"][0]["id"],
            "location": location_json(&file),
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let metadata: Value = response.json().await.expect("body must be JSON");
    assert_eq!(metadata["entryId"], listing["entries"][0]["id"]);
    assert!(metadata["permissions"]["readable"].is_boolean());
}

#[tokio::test]
async fn directory_children_endpoint_lists_only_directories() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    std::fs::create_dir(root.path().join("child")).expect("create child dir");
    std::fs::write(root.path().join("file.txt"), b"contents").expect("create file");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/directories/children", server.base_url))
        .json(&json!({
            "location": location_json(root.path()),
            "showHidden": false,
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    let children = body.as_array().expect("body must be a JSON array");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["name"], "child");
    assert_eq!(children[0]["kind"], "directory");
}

#[tokio::test]
async fn directory_errors_use_stable_sanitized_application_error_dtos() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let server = TestServer::spawn().await;
    let missing = root.path().join("missing");

    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/directories/list", server.base_url))
        .json(&json!({
            "workspaceId": Uuid::new_v4(),
            "paneId": Uuid::new_v4(),
            "requestId": Uuid::new_v4(),
            "location": location_json(&missing),
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["code"], "notFound");
    assert_eq!(body["message"], "resource not found");
    assert!(
        !body
            .to_string()
            .contains(missing.to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn set_pane_activity_endpoint_accepts_a_known_pane_and_rejects_an_unknown_one() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let workspace_id = Uuid::new_v4();
    let pane_id = Uuid::new_v4();

    client
        .post(format!("{}/api/v1/directories/list", server.base_url))
        .json(&json!({
            "workspaceId": workspace_id,
            "paneId": pane_id,
            "requestId": Uuid::new_v4(),
            "location": location_json(root.path()),
        }))
        .send()
        .await
        .expect("request must succeed");

    let response = client
        .post(format!("{}/api/v1/directories/activity", server.base_url))
        .json(&json!({ "paneId": pane_id, "active": false }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);

    let response = client
        .post(format!("{}/api/v1/directories/activity", server.base_url))
        .json(&json!({ "paneId": Uuid::new_v4(), "active": true }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["code"], "notFound");
}

#[test]
fn directory_openapi_uses_the_required_stable_operation_ids() {
    let document = fm_server::openapi_document();
    let expected = [
        ("/api/v1/directories/list", "listDirectory"),
        ("/api/v1/directories/refresh", "refreshDirectory"),
        ("/api/v1/directories/children", "listDirectoryChildren"),
        ("/api/v1/navigation/open", "navigatePane"),
        ("/api/v1/entries/metadata", "getEntryMetadata"),
        ("/api/v1/directories/activity", "setPaneActivity"),
    ];

    for (path, operation_id) in expected {
        let item = document
            .paths
            .paths
            .get(path)
            .expect("path must be present");
        assert_eq!(
            item.post
                .as_ref()
                .and_then(|operation| operation.operation_id.as_deref()),
            Some(operation_id)
        );
    }
}
