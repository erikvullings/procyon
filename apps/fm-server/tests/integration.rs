//! Integration test for task 0008: boots the real Axum host on an ephemeral
//! port and exercises it exactly as a client would.

mod common;

use common::TestServer;
use utoipa::openapi::{OpenApi, OpenApiVersion};

#[tokio::test]
async fn health_endpoint_returns_ok_status() {
    let server = TestServer::spawn().await;

    let response = reqwest::get(format!("{}/api/v1/health", server.base_url))
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn runtime_endpoint_returns_the_capabilities_shape() {
    let server = TestServer::spawn().await;

    let response = reqwest::get(format!("{}/api/v1/runtime", server.base_url))
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["runtime"], "browserServer");
    assert!(matches!(
        body["platform"].as_str(),
        Some("macos" | "windows" | "linux" | "unknown")
    ));
    assert_eq!(body["clipboard"], true);
    assert_eq!(body["nativeMenus"], false);
    assert_eq!(body["platformContextMenu"], false);
    assert_eq!(body["serverAdministration"], false);
}

#[tokio::test]
async fn openapi_document_parses_as_openapi_31() {
    let server = TestServer::spawn().await;

    let response = reqwest::get(format!("{}/api/v1/openapi.json", server.base_url))
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let text = response.text().await.expect("body must be text");
    let document: OpenApi =
        serde_json::from_str(&text).expect("body must parse as an OpenAPI document");
    assert!(document.openapi == OpenApiVersion::Version31);
    assert!(document.paths.paths.contains_key("/api/v1/health"));
    assert!(document.paths.paths.contains_key("/api/v1/runtime"));
    assert!(document.paths.paths.contains_key("/api/v1/workspaces"));
}

#[tokio::test]
async fn swagger_ui_is_served_at_docs() {
    let server = TestServer::spawn().await;

    let response = reqwest::get(format!("{}/api/v1/docs/", server.base_url))
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn every_response_carries_a_request_id() {
    let server = TestServer::spawn().await;

    let response = reqwest::get(format!("{}/api/v1/health", server.base_url))
        .await
        .expect("request must succeed");
    assert!(response.headers().contains_key("x-request-id"));

    let missing = reqwest::get(format!("{}/api/v1/does-not-exist", server.base_url))
        .await
        .expect("request must succeed");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
    assert!(missing.headers().contains_key("x-request-id"));
    let body: serde_json::Value = missing.json().await.expect("body must be JSON");
    assert_eq!(body["code"], "notFound");
    assert!(body["requestId"].is_string());
}
