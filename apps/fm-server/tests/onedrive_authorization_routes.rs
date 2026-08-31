//! Integration tests for the OneDrive authorization REST surface (task
//! 0110): begin/poll/cancel through real HTTP requests against a spawned
//! server, exercising the thin-handler <-> `FileManagerService` wiring.
//! Deep authorization-flow behaviour (personal/business fixture success,
//! scope validation, Conditional Access, refresh rotation, ...) is already
//! covered by `fm-application`'s own `onedrive` module tests against
//! loopback OAuth/Graph fixtures; these tests only need to prove the HTTP
//! layer maps requests/responses/errors correctly, so every case here
//! avoids any real network call (validation failures short-circuit before
//! one, and cancelling an attempt aborts its wait before it ever reaches
//! the token endpoint).

mod common;

use common::TestServer;
use serde_json::{Value, json};

fn onedrive_configuration() -> Value {
    json!({ "kind": "oneDrive" })
}

fn ssh_configuration() -> Value {
    json!({
        "kind": "ssh",
        "host": "example.test",
        "port": 22,
        "username": "erik",
        "authentication": "agent",
        "hostKeyPolicy": "promptOnFirstUse",
    })
}

async fn create_connection(server: &TestServer, name: &str, configuration: Value) -> Value {
    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/connections", server.base_url))
        .json(&json!({
            "name": name,
            "kind": configuration["kind"],
            "configuration": configuration,
            "secret": Value::Null,
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    response.json().await.expect("body must be JSON")
}

#[tokio::test]
async fn begin_onedrive_authorization_returns_an_attempt_id_and_authorization_url() {
    let server = TestServer::spawn().await;
    let connection = create_connection(&server, "My OneDrive", onedrive_configuration()).await;

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/connections/{}/onedrive/authorize",
            server.base_url,
            connection["id"].as_str().unwrap()
        ))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let body: Value = response.json().await.expect("body must be JSON");
    assert!(body["attemptId"].as_str().is_some());
    let url = body["authorizationUrl"]
        .as_str()
        .expect("authorizationUrl present");
    assert!(url.starts_with("https://login.microsoftonline.com/"));
    assert!(url.contains("code_challenge="));
    assert!(!url.contains("client_secret"));
}

#[tokio::test]
async fn begin_onedrive_authorization_reports_not_found_for_an_unknown_connection() {
    let server = TestServer::spawn().await;

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/connections/{}/onedrive/authorize",
            server.base_url,
            uuid::Uuid::new_v4()
        ))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["code"], "notFound");
}

#[tokio::test]
async fn begin_onedrive_authorization_rejects_a_non_onedrive_connection() {
    let server = TestServer::spawn().await;
    let connection = create_connection(&server, "Home Server", ssh_configuration()).await;

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/connections/{}/onedrive/authorize",
            server.base_url,
            connection["id"].as_str().unwrap()
        ))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["code"], "invalidRequest");
}

#[tokio::test]
async fn begin_onedrive_authorization_rejects_a_second_concurrent_attempt_for_the_same_connection()
{
    let server = TestServer::spawn().await;
    let connection = create_connection(&server, "My OneDrive", onedrive_configuration()).await;
    let authorize_url = format!(
        "{}/api/v1/connections/{}/onedrive/authorize",
        server.base_url,
        connection["id"].as_str().unwrap()
    );

    let first = reqwest::Client::new()
        .post(&authorize_url)
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), reqwest::StatusCode::CREATED);

    let second = reqwest::Client::new()
        .post(&authorize_url)
        .send()
        .await
        .unwrap();
    assert_eq!(second.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_onedrive_authorization_attempt_reports_not_found_for_an_unknown_attempt() {
    let server = TestServer::spawn().await;

    let response = reqwest::get(format!(
        "{}/api/v1/onedrive/authorizations/{}",
        server.base_url,
        uuid::Uuid::new_v4()
    ))
    .await
    .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cancel_onedrive_authorization_reports_not_found_for_an_unknown_attempt() {
    let server = TestServer::spawn().await;

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/onedrive/authorizations/{}/cancel",
            server.base_url,
            uuid::Uuid::new_v4()
        ))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn begin_poll_and_cancel_an_authorization_attempt_reflects_cancellation() {
    let server = TestServer::spawn().await;
    let connection = create_connection(&server, "My OneDrive", onedrive_configuration()).await;

    let begin: Value = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/connections/{}/onedrive/authorize",
            server.base_url,
            connection["id"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let attempt_id = begin["attemptId"].as_str().unwrap();

    let pending: Value = reqwest::get(format!(
        "{}/api/v1/onedrive/authorizations/{attempt_id}",
        server.base_url
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(pending["status"]["state"], "pending");

    let cancel_response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/onedrive/authorizations/{attempt_id}/cancel",
            server.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(cancel_response.status(), reqwest::StatusCode::OK);

    // The background task notices cancellation asynchronously; poll briefly
    // for the terminal state.
    let mut state = pending["status"]["state"].as_str().unwrap().to_owned();
    for _ in 0..100 {
        if state == "cancelled" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let attempt: Value = reqwest::get(format!(
            "{}/api/v1/onedrive/authorizations/{attempt_id}",
            server.base_url
        ))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
        state = attempt["status"]["state"].as_str().unwrap().to_owned();
    }
    assert_eq!(state, "cancelled");

    // Beginning a new attempt for the same connection is allowed again now
    // that the previous one has finished.
    let second = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/connections/{}/onedrive/authorize",
            server.base_url,
            connection["id"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(second.status(), reqwest::StatusCode::CREATED);
}
