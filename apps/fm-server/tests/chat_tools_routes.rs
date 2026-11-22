//! HTTP coverage for the chat's tools folder (task 0339): on a server, the folder the assistant may
//! look into must lie inside the accessible roots. A folder outside them is refused before any
//! model or file is touched.

mod common;

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use common::TestServer;
use fm_application::FileManagerService;
use fm_events::EventBus;
use fm_server::config::ServerConfig;
use fm_transport_dto::RuntimeKindDto;
use serde_json::{Value, json};
use uuid::Uuid;

async fn spawn(roots: Vec<std::path::PathBuf>) -> TestServer {
    let workspace_directory = tempfile::tempdir().expect("must create workspace directory");
    let config = ServerConfig {
        dev_mode_auth_disabled: true,
        roots,
        bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: 0,
        workspace_directory: workspace_directory.path().to_path_buf(),
        settings_directory: workspace_directory.path().join("config"),
        ..ServerConfig::default()
    };
    let service = Arc::new(FileManagerService::from_config(
        fm_application::ServiceConfig::new(
            RuntimeKindDto::BrowserServer,
            config.workspace_directory.clone(),
            config.settings_directory.clone(),
        )
        .events(EventBus::new(8)),
    ));
    TestServer::spawn_with_service(config, service, workspace_directory).await
}

fn folder_url(path: &std::path::Path) -> String {
    url::Url::from_directory_path(path.canonicalize().expect("temp path must exist"))
        .expect("temp path must be a file URL")
        .to_string()
}

/// The events of one streamed reply.
async fn reply_events(server: &TestServer, tools_folder: Option<String>) -> Vec<Value> {
    let body = json!({
        "requestId": Uuid::new_v4(),
        "workspaceId": Uuid::new_v4(),
        "conversationId": Uuid::new_v4(),
        "content": "What is in a.txt?",
        "toolsFolder": tools_folder,
    });
    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/chat/reply/stream", server.base_url))
        .json(&body)
        .send()
        .await
        .expect("the stream must answer");
    assert!(response.status().is_success());
    let text = response.text().await.expect("a body");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("each line is one JSON event"))
        .collect()
}

fn failure_of(events: &[Value]) -> Option<&str> {
    events
        .iter()
        .find(|event| event["type"] == "failed")
        .and_then(|event| event["reason"].as_str())
}

#[tokio::test]
async fn a_tools_folder_outside_the_accessible_roots_is_refused() {
    let allowed = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("a.txt"), "secret").unwrap();
    let server = spawn(vec![allowed.path().to_path_buf()]).await;

    for folder in [
        folder_url(outside.path()),
        // A path that walks out of the root reads the same.
        format!("{}../", folder_url(allowed.path())),
        "not a url".to_owned(),
    ] {
        let events = reply_events(&server, Some(folder.clone())).await;
        assert_eq!(
            failure_of(&events),
            Some("invalidRequest"),
            "{folder}: {events:?}"
        );
        assert_eq!(
            events.len(),
            1,
            "nothing else happens for {folder}: {events:?}"
        );
    }
}

#[tokio::test]
async fn a_tools_folder_inside_the_roots_passes_the_check() {
    let allowed = tempfile::tempdir().unwrap();
    let server = spawn(vec![allowed.path().to_path_buf()]).await;

    // With no chat model assigned the reply then fails for that reason, which shows the folder
    // itself was accepted.
    let events = reply_events(&server, Some(folder_url(allowed.path()))).await;
    assert_ne!(failure_of(&events), Some("invalidRequest"), "{events:?}");
    let events = reply_events(&server, None).await;
    assert_ne!(failure_of(&events), Some("invalidRequest"), "{events:?}");
}
