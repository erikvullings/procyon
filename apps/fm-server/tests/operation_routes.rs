//! Operations REST contract and lifecycle integration tests.

mod common;

use fm_events::{BackendEventPayload, OperationStatePayload, SessionId, SubscriptionEvent};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn start_retry_uses_stable_id_and_copy_emits_full_lifecycle() {
    let server = common::TestServer::spawn().await;
    let root = tempfile::tempdir().expect("must create a temporary operation root");
    let source = root.path().join("source.txt");
    let destination = root.path().join("destination");
    tokio::fs::write(&source, b"copy through the operation route")
        .await
        .expect("must write the source fixture");
    tokio::fs::create_dir(&destination)
        .await
        .expect("must create the destination fixture");
    let mut events = server
        .event_bus
        .subscribe_all_workspaces(SessionId::new("operations-test"), None);
    let client = reqwest::Client::new();
    let request = json!({
        "type": "copy",
        "sources": [{"providerId":"local","uri": common::file_uri(&source)}],
        "destination": {"providerId":"local","uri": common::file_uri(&destination)},
        "conflictPolicy": "ask"
    });
    let first: serde_json::Value = client
        .post(format!("{}/api/v1/operations", server.base_url))
        .header("Idempotency-Key", "same-request")
        .json(&request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let retry: serde_json::Value = client
        .post(format!("{}/api/v1/operations", server.base_url))
        .header("Idempotency-Key", "same-request")
        .json(&request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(first["id"], retry["id"]);

    // The list endpoint only shows non-terminal operations (by design, it's an
    // "in progress" view), and this copy is small enough to sometimes finish
    // before this request lands - so it may legitimately show either 0 (already
    // completed) or 1 (still active) entries, never more.
    let listed: serde_json::Value = client
        .get(format!("{}/api/v1/operations", server.base_url))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let listed_operations = listed["operations"].as_array().expect("operations array");
    assert!(listed_operations.len() <= 1, "got {listed_operations:?}");
    if let Some(operation) = listed_operations.first() {
        assert_eq!(operation["id"], first["id"]);
    }
    let id = first["id"].as_str().unwrap();
    let fetched: serde_json::Value = client
        .get(format!("{}/api/v1/operations/{id}", server.base_url))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fetched["id"], first["id"]);

    let mut payloads = Vec::new();
    while !payloads.iter().any(|payload: &BackendEventPayload| {
        matches!(payload, BackendEventPayload::OperationCompleted { .. })
    }) {
        let SubscriptionEvent::Event(event) = events.recv().await.unwrap() else {
            panic!("unexpected replay gap")
        };
        payloads.push(event.payload);
    }
    assert_eq!(
        payloads
            .iter()
            .map(BackendEventPayload::event_name)
            .collect::<Vec<_>>(),
        [
            "operation.created",
            "operation.stateChanged",
            "operation.progress",
            "operation.stateChanged",
            "operation.progress",
            "operation.stateChanged",
            "operation.completed",
        ]
    );
    let states = payloads
        .iter()
        .filter_map(|payload| match payload {
            BackendEventPayload::OperationStateChanged { state, .. } => Some(*state),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        states,
        [
            OperationStatePayload::Planning,
            OperationStatePayload::Running,
            OperationStatePayload::Completed,
        ]
    );
}

#[test]
fn openapi_reserves_all_stable_operation_ids() {
    let document = fm_server::openapi_document();
    let json = serde_json::to_value(document).unwrap();
    let text = json.to_string();
    for operation_id in [
        "listOperations",
        "startOperation",
        "getOperation",
        "cancelOperation",
        "pauseOperation",
        "resumeOperation",
        "resolveOperationConflict",
    ] {
        assert!(text.contains(operation_id), "missing {operation_id}");
    }
}

#[tokio::test]
async fn resolve_conflict_route_applies_the_requested_decision() {
    let server = common::TestServer::spawn().await;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("same.txt");
    let destination = root.path().join("destination");
    tokio::fs::create_dir(&destination).await.unwrap();
    tokio::fs::write(&source, b"source").await.unwrap();
    tokio::fs::write(destination.join("same.txt"), b"existing")
        .await
        .unwrap();
    let client = reqwest::Client::new();
    let operation: serde_json::Value = client
        .post(format!("{}/api/v1/operations", server.base_url))
        .json(&json!({
            "type": "copy",
            "sources": [{"providerId":"local","uri": common::file_uri(&source)}],
            "destination": {"providerId":"local","uri": common::file_uri(&destination)},
            "conflictPolicy": "ask"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = operation["id"].as_str().unwrap();
    for _ in 0..200 {
        let current: serde_json::Value = client
            .get(format!("{}/api/v1/operations/{id}", server.base_url))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if current["state"] == "waitingForConflictResolution" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    client
        .post(format!(
            "{}/api/v1/operations/{id}/resolve-conflict",
            server.base_url
        ))
        .json(&json!({"resolution":"skip", "applyToAllSimilar":false}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    for _ in 0..200 {
        let current: serde_json::Value = client
            .get(format!("{}/api/v1/operations/{id}", server.base_url))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if current["state"] == "completed" {
            assert_eq!(current["progress"]["completedBytes"], 0);
            assert_eq!(
                tokio::fs::read(destination.join("same.txt")).await.unwrap(),
                b"existing"
            );
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("operation did not complete after REST conflict resolution")
}

#[tokio::test]
async fn resolve_conflict_route_confirms_a_permanent_directory_delete() {
    let server = common::TestServer::spawn().await;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("delete-me");
    tokio::fs::create_dir(&source).await.unwrap();
    tokio::fs::write(source.join("child.txt"), b"contents")
        .await
        .unwrap();
    let client = reqwest::Client::new();
    let operation: serde_json::Value = client
        .post(format!("{}/api/v1/operations", server.base_url))
        .json(&json!({
            "type": "delete",
            "sources": [{"providerId":"local","uri": common::file_uri(&source)}],
            "conflictPolicy": "ask"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = operation["id"].as_str().unwrap();

    for _ in 0..200 {
        let current: serde_json::Value = client
            .get(format!("{}/api/v1/operations/{id}", server.base_url))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        if current["state"] == "waitingForConflictResolution" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    client
        .post(format!(
            "{}/api/v1/operations/{id}/resolve-conflict",
            server.base_url
        ))
        .json(&json!({"resolution":"confirm", "applyToAllSimilar":false}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    for _ in 0..200 {
        let current: serde_json::Value = client
            .get(format!("{}/api/v1/operations/{id}", server.base_url))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        if current["state"] == "completed" {
            assert!(!source.exists());
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("directory delete did not complete after REST confirmation")
}
