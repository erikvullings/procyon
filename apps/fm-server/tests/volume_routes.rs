//! End-to-end mounted-volume discovery coverage (task 0144).

mod common;

use std::sync::Arc;

use common::TestServer;
use fm_application::FileManagerService;
use fm_events::EventBus;
use fm_platform::{MountedVolume, PlatformAdapter, PlatformCapabilities, PlatformError};
use fm_server::config::ServerConfig;
use fm_transport_dto::RuntimeKindDto;

struct DiscoveryAdapter {
    capabilities: PlatformCapabilities,
    result: Result<Vec<MountedVolume>, PlatformError>,
}

impl PlatformAdapter for DiscoveryAdapter {
    fn capabilities(&self) -> PlatformCapabilities {
        self.capabilities
    }

    fn mounted_volumes(&self) -> Result<Vec<MountedVolume>, PlatformError> {
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
async fn discovered_volume_uses_the_existing_local_provider() {
    let directory = tempfile::tempdir().expect("volume mount point");
    let adapter = DiscoveryAdapter {
        capabilities: PlatformCapabilities::MOUNTED_VOLUMES,
        result: Ok(vec![MountedVolume {
            name: "Macintosh HD".to_owned(),
            mount_point: directory.path().to_path_buf(),
        }]),
    };
    let server = spawn(Arc::new(adapter)).await;
    let response = reqwest::get(format!("{}/api/v1/volumes", server.base_url))
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("JSON response");
    assert_eq!(body[0]["name"], "Macintosh HD");
    assert_eq!(body[0]["location"]["providerId"], "local");
}

#[tokio::test]
async fn adapter_without_the_capability_reports_an_empty_success() {
    let adapter = DiscoveryAdapter {
        capabilities: PlatformCapabilities::empty(),
        result: Ok(Vec::new()),
    };
    let server = spawn(Arc::new(adapter)).await;
    let response = reqwest::get(format!("{}/api/v1/volumes", server.base_url))
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().await.expect("JSON"),
        serde_json::json!([])
    );
}

#[tokio::test]
async fn fallback_discovery_is_an_empty_success() {
    let server = spawn(Arc::new(fm_platform::FallbackPlatformAdapter)).await;
    let response = reqwest::get(format!("{}/api/v1/volumes", server.base_url))
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
        capabilities: PlatformCapabilities::MOUNTED_VOLUMES,
        result: Err(PlatformError::Io {
            message: "temporarily offline".to_owned(),
        }),
    };
    let server = spawn(Arc::new(adapter)).await;
    let response = reqwest::get(format!("{}/api/v1/volumes", server.base_url))
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
}
