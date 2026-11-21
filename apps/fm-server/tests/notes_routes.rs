//! HTTP coverage for notes (task 0337): setup on a server suggests no folder, the notes folder
//! must lie inside the accessible roots, and notes are searched and read inside it only.

mod common;

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use common::TestServer;
use fm_application::FileManagerService;
use fm_events::EventBus;
use fm_server::config::ServerConfig;
use fm_transport_dto::RuntimeKindDto;
use reqwest::StatusCode;
use serde_json::{Value, json};

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

fn location(path: &std::path::Path) -> Value {
    let location =
        fm_domain::Location::from_native_path(&path.canonicalize().expect("temp path must exist"))
            .expect("temp path must be representable");
    json!({ "providerId": location.provider_id.as_str(), "uri": location.uri })
}

#[tokio::test]
async fn notes_live_inside_the_accessible_roots_and_are_searched_and_read_there() {
    let allowed = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let notes = allowed.path().join("notes");
    std::fs::create_dir(&notes).unwrap();
    std::fs::write(
        notes.join("csr.md"),
        "---\ntitle: Create a CSR\ntags: [openssl]\n---\n# Create a CSR\nopenssl req -new\n",
    )
    .unwrap();
    let server = spawn(vec![allowed.path().to_path_buf()]).await;
    let client = reqwest::Client::new();
    let url = |path: &str| format!("{}/api/v1/notes/{path}", server.base_url);

    let setup: Value = client
        .get(url("setup"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(setup["root"], Value::Null);
    assert_eq!(setup["suggestedRoot"], Value::Null);

    // Nothing to search before a folder is chosen.
    let unset = client
        .post(url("search"))
        .json(&json!({ "query": "csr" }))
        .send()
        .await
        .unwrap();
    assert_eq!(unset.status(), StatusCode::BAD_REQUEST);
    let body: Value = unset.json().await.unwrap();
    assert_eq!(body["details"]["reason"], "notesNotConfigured");

    let denied = client
        .post(url("root"))
        .json(&json!({ "location": location(outside.path()) }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    let stored: Value = client
        .post(url("root"))
        .json(&json!({ "location": location(&notes) }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(stored["rootExists"], true);

    let found: Value = client
        .post(url("search"))
        .json(&json!({ "query": "openssl csr" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(found["strategy"], "text");
    assert_eq!(found["hits"][0]["path"], "csr.md");
    assert_eq!(found["hits"][0]["title"], "Create a CSR");

    let note: Value = client
        .post(url("read"))
        .json(&json!({ "path": "csr.md" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        note["content"]
            .as_str()
            .unwrap()
            .contains("openssl req -new")
    );
    assert!(!note["revision"].as_str().unwrap().is_empty());

    std::fs::write(allowed.path().join("secret.md"), "outside the notes folder").unwrap();
    let escaped = client
        .post(url("read"))
        .json(&json!({ "path": "../secret.md" }))
        .send()
        .await
        .unwrap();
    assert_eq!(escaped.status(), StatusCode::BAD_REQUEST);
    let body: Value = escaped.json().await.unwrap();
    assert_eq!(body["details"]["reason"], "notesPathUnavailable");
}
