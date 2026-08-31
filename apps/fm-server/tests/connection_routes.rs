//! Integration tests for the connection REST surface (task 0103): CRUD,
//! connect/disconnect/test status transitions, and - critically - that a
//! request body containing a secret never appears anywhere in a response
//! body (spec §16, §19).

mod common;

use common::TestServer;
use serde_json::{Value, json};

fn ssh_configuration() -> Value {
    json!({
        "kind": "ssh",
        "host": "example.test",
        "port": 22,
        "username": "erik",
        "authentication": "password",
        "hostKeyPolicy": "promptOnFirstUse",
        "keepaliveSeconds": 30
    })
}

async fn create_connection(server: &TestServer, name: &str, secret: Option<Value>) -> Value {
    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/connections", server.base_url))
        .json(&json!({
            "name": name,
            "kind": "ssh",
            "configuration": ssh_configuration(),
            "secret": secret,
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    response.json().await.expect("body must be JSON")
}

#[tokio::test]
async fn list_connections_starts_empty_and_reflects_created_connections() {
    let server = TestServer::spawn().await;

    let response = reqwest::get(format!("{}/api/v1/connections", server.base_url))
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body.as_array().unwrap().len(), 0);

    create_connection(&server, "Home Server", None).await;

    let response = reqwest::get(format!("{}/api/v1/connections", server.base_url))
        .await
        .expect("request must succeed");
    let body: Value = response.json().await.expect("body must be JSON");
    let connections = body.as_array().unwrap();
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0]["name"], "Home Server");
    assert_eq!(connections[0]["status"], "disconnected");
    assert_eq!(connections[0]["hasCredential"], false);
}

#[tokio::test]
async fn get_connection_returns_the_created_connection() {
    let server = TestServer::spawn().await;
    let created = create_connection(&server, "Home Server", None).await;

    let response = reqwest::get(format!(
        "{}/api/v1/connections/{}",
        server.base_url,
        created["id"].as_str().unwrap()
    ))
    .await
    .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["id"], created["id"]);
    assert_eq!(body["kind"], "ssh");
}

#[tokio::test]
async fn get_connection_reports_not_found_for_an_unknown_id() {
    let server = TestServer::spawn().await;

    let response = reqwest::get(format!(
        "{}/api/v1/connections/{}",
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
async fn create_connection_with_a_password_never_echoes_it_back_in_the_response() {
    let server = TestServer::spawn().await;

    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/connections", server.base_url))
        .json(&json!({
            "name": "Home Server",
            "kind": "ssh",
            "configuration": ssh_configuration(),
            "secret": { "kind": "password", "password": "hunter2-super-secret" },
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);

    let raw_body = response.text().await.expect("body must be text");
    assert!(
        !raw_body.contains("hunter2-super-secret"),
        "the raw response body must never contain the submitted password"
    );
    let body: Value = serde_json::from_str(&raw_body).expect("body must be JSON");
    assert_eq!(body["hasCredential"], true);
    assert!(body.get("secret").is_none());
    assert!(body.get("credentialRef").is_none());
    assert!(body.get("credential_ref").is_none());
}

#[tokio::test]
async fn update_connection_with_a_new_password_never_echoes_it_back_either() {
    let server = TestServer::spawn().await;
    let created = create_connection(
        &server,
        "Home Server",
        Some(json!({ "kind": "password", "password": "old-password" })),
    )
    .await;

    let response = reqwest::Client::new()
        .put(format!(
            "{}/api/v1/connections/{}",
            server.base_url,
            created["id"].as_str().unwrap()
        ))
        .json(&json!({
            "name": "Renamed Server",
            "kind": "ssh",
            "configuration": ssh_configuration(),
            "secret": { "kind": "password", "password": "brand-new-password" },
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let raw_body = response.text().await.expect("body must be text");
    assert!(!raw_body.contains("brand-new-password"));
    assert!(!raw_body.contains("old-password"));
    let body: Value = serde_json::from_str(&raw_body).expect("body must be JSON");
    assert_eq!(body["name"], "Renamed Server");
    assert_eq!(body["id"], created["id"]);
    assert_eq!(body["hasCredential"], true);
}

#[tokio::test]
async fn delete_connection_removes_it_and_a_second_delete_reports_not_found() {
    let server = TestServer::spawn().await;
    let created = create_connection(&server, "Home Server", None).await;
    let id = created["id"].as_str().unwrap();

    let response = reqwest::Client::new()
        .delete(format!("{}/api/v1/connections/{id}", server.base_url))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);

    let response = reqwest::Client::new()
        .delete(format!("{}/api/v1/connections/{id}", server.base_url))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn connect_then_disconnect_transitions_the_status() {
    let server = TestServer::spawn().await;
    let created = create_connection(
        &server,
        "Home Server",
        Some(json!({ "kind": "password", "password": "hunter2" })),
    )
    .await;
    let id = created["id"].as_str().unwrap();

    // Task 0104 registered a real SSH dialer for `ConnectionKind::Ssh`, so
    // `connect` now genuinely attempts a handshake rather than reporting the
    // pre-0104 "no dialer registered" stand-in success (see
    // `fm_connections::ConnectionService`'s module doc). `example.test` is
    // not a reachable host, so the honest, correct outcome here is `failed`
    // - proving the REST layer really reaches the real dialer end to end.
    // Real connect/host-key/auth behaviour against a live SSH server is
    // covered by `fm-ssh`'s and `fm-application`'s own fixture-backed tests.
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/connections/{id}/connect",
            server.base_url
        ))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["status"], "failed");

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/connections/{id}/disconnect",
            server.base_url
        ))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["status"], "disconnected");
}

#[tokio::test]
async fn test_connection_reports_authentication_required_without_a_credential() {
    let server = TestServer::spawn().await;
    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/connections", server.base_url))
        .json(&json!({
            "name": "Home Server",
            "kind": "ssh",
            "configuration": ssh_configuration(),
            "secret": Value::Null,
        }))
        .send()
        .await
        .expect("request must succeed");
    let created: Value = response.json().await.expect("body must be JSON");
    let id = created["id"].as_str().unwrap();

    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/connections/{id}/test", server.base_url))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    // The ssh fixture uses password authentication (spec §6.3) with no
    // stored credential, so it must report AuthenticationRequired rather
    // than a false Connected.
    assert_eq!(body["status"], "authenticationRequired");
}

#[tokio::test]
async fn create_connection_with_invalid_configuration_reports_a_structured_400() {
    let server = TestServer::spawn().await;

    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/connections", server.base_url))
        .json(&json!({
            "name": "",
            "kind": "ssh",
            "configuration": {
                "kind": "ssh",
                "host": "",
                "port": 0,
                "username": "",
                "authentication": "agent",
                "hostKeyPolicy": "promptOnFirstUse",
                "keepaliveSeconds": Value::Null,
            },
            "secret": Value::Null,
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["code"], "invalidRequest");
}
