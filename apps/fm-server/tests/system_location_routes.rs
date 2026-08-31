//! End-to-end system-location discovery coverage (task 0101).

mod common;

use std::sync::Arc;

use common::TestServer;
use fm_application::FileManagerService;
use fm_events::EventBus;
use fm_platform::{
    PlatformAdapter, PlatformCapabilities, PlatformError, SystemLocation, SystemLocationKind,
};
use fm_server::config::ServerConfig;
use fm_transport_dto::RuntimeKindDto;

struct DiscoveryAdapter {
    result: Result<Vec<SystemLocation>, PlatformError>,
}

impl PlatformAdapter for DiscoveryAdapter {
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::empty()
    }

    fn system_locations(&self) -> Result<Vec<SystemLocation>, PlatformError> {
        self.result
            .as_ref()
            .map(Clone::clone)
            .map_err(|error| PlatformError::Io {
                message: error.to_string(),
            })
    }
}

async fn spawn(adapter: Arc<dyn PlatformAdapter>) -> TestServer {
    let root = tempfile::tempdir().expect("must create test root");
    let config = ServerConfig {
        workspace_directory: root.path().join("workspaces"),
        settings_directory: root.path().join("settings"),
        port: 0,
        dev_mode_auth_disabled: true,
        ..ServerConfig::default()
    };
    let service = Arc::new(FileManagerService::with_platform_adapter(
        RuntimeKindDto::BrowserServer,
        config.workspace_directory.clone(),
        config.settings_directory.clone(),
        EventBus::default(),
        adapter,
    ));
    TestServer::spawn_with_service(config, service, root).await
}

#[tokio::test]
async fn discovered_cloud_directory_uses_the_existing_local_provider() {
    let directory = tempfile::tempdir().expect("cloud directory");
    let adapter = DiscoveryAdapter {
        result: Ok(vec![SystemLocation {
            name: "Example Drive".to_owned(),
            path: directory.path().to_path_buf(),
            kind: SystemLocationKind::Cloud,
            provider_hint: Some("example".to_owned()),
            protocol: None,
            server: None,
            share: None,
            read_only: None,
        }]),
    };
    let server = spawn(Arc::new(adapter)).await;
    let response = reqwest::get(format!("{}/api/v1/system-locations", server.base_url))
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("JSON response");
    assert_eq!(body[0]["kind"], "cloud");
    assert_eq!(body[0]["location"]["providerId"], "local");
    assert_eq!(body[0]["providerHint"], "example");
}

#[tokio::test]
async fn mounted_smb_share_uses_local_provider_and_preserves_mount_metadata() {
    let directory = tempfile::tempdir().expect("mounted share");
    let adapter = DiscoveryAdapter {
        result: Ok(vec![SystemLocation {
            name: "Team Files".to_owned(),
            path: directory.path().to_path_buf(),
            kind: SystemLocationKind::Network,
            provider_hint: None,
            protocol: Some("smb".to_owned()),
            server: Some("files.example.test".to_owned()),
            share: Some("team".to_owned()),
            read_only: Some(true),
        }]),
    };
    let server = spawn(Arc::new(adapter)).await;
    let response = reqwest::get(format!("{}/api/v1/system-locations", server.base_url))
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("JSON response");
    assert_eq!(body[0]["kind"], "network");
    assert_eq!(body[0]["location"]["providerId"], "local");
    assert_eq!(body[0]["protocol"], "smb");
    assert_eq!(body[0]["server"], "files.example.test");
    assert_eq!(body[0]["share"], "team");
    assert_eq!(body[0]["readOnly"], true);
}

#[tokio::test]
async fn fallback_discovery_is_an_empty_success() {
    let server = spawn(Arc::new(fm_platform::FallbackPlatformAdapter)).await;
    let response = reqwest::get(format!("{}/api/v1/system-locations", server.base_url))
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().await.expect("JSON"),
        serde_json::json!([])
    );
}

#[tokio::test]
async fn discovery_failure_is_recoverable() {
    let adapter = DiscoveryAdapter {
        result: Err(PlatformError::Io {
            message: "temporarily offline".to_owned(),
        }),
    };
    let server = spawn(Arc::new(adapter)).await;
    let response = reqwest::get(format!("{}/api/v1/system-locations", server.base_url))
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
}
