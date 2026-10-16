//! The operator-opted-in, read-only desktop semantic-library attachment
//! (task 0250) is served only to authenticated callers and never writes to
//! the desktop profile or its shared stores.

#![allow(clippy::unwrap_used)]

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

use fm_server::auth::SessionManager;
use fm_server::config::ServerConfig;
use fm_transport_dto::{SemanticLibraryAuthorityDto, SemanticLibraryCapabilitiesDto};
use reqwest::StatusCode;

fn project_temp_dir(prefix: &str) -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/server-attachment-tests");
    std::fs::create_dir_all(&parent).expect("create project-local test directory");
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)
        .expect("create project-local temporary directory")
}

#[tokio::test]
async fn attached_server_requires_a_token_and_never_writes_the_desktop_profile() {
    let directory = project_temp_dir("attached-");
    let profile = directory.path().join("procyon");
    std::fs::create_dir_all(&profile).unwrap();
    let profile = profile.canonicalize().unwrap();
    let config = ServerConfig {
        bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: 0,
        workspace_directory: profile.join("workspaces"),
        settings_directory: profile.clone(),
        semantic_desktop_profile: Some(profile.clone()),
        ..ServerConfig::default()
    };
    assert!(config.semantic_desktop_attachment().unwrap().is_some());
    let token = SessionManager::new(config.session_secret.clone(), false).issue_token();
    let router = fm_server::build_router(&config);
    let listener = tokio::net::TcpListener::bind((config.bind_address, 0))
        .await
        .unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::new();
    let capabilities_url = format!("{base_url}/api/v1/semantic/library/capabilities");

    let anonymous = client.get(&capabilities_url).send().await.unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    // With no desktop model or consent yet, the attachment is unavailable
    // rather than creating a library of its own.
    let response = client
        .get(&capabilities_url)
        .bearer_auth(token.as_str())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let capabilities: SemanticLibraryCapabilitiesDto = response.json().await.unwrap();
    assert_eq!(
        capabilities.authority,
        SemanticLibraryAuthorityDto::Unavailable
    );
    assert!(capabilities.operations.is_empty());

    let import = client
        .post(format!("{base_url}/api/v1/semantic/vocabularies/import"))
        .bearer_auth(token.as_str())
        .json(&serde_json::json!({ "skosJson": "{}" }))
        .send()
        .await
        .unwrap();
    assert_eq!(import.status(), StatusCode::FORBIDDEN);

    assert!(!profile.join("semantic-components").exists());
    assert!(!profile.join("semantic").exists());
    assert!(!profile.join("semantic-library-policy.json").exists());
    server.abort();
}
