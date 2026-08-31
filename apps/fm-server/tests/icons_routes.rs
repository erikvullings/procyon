//! End-to-end native file icon transport coverage (task 0091).

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::TestServer;
use fm_application::FileManagerService;
use fm_events::EventBus;
use fm_platform::{PlatformAdapter, PlatformCapabilities, PlatformError};
use fm_server::config::ServerConfig;
use fm_transport_dto::RuntimeKindDto;

#[derive(Default)]
struct ExtensionCachingAdapter {
    cache: Mutex<HashMap<String, Vec<u8>>>,
    lookups: Mutex<Vec<PathBuf>>,
}

impl PlatformAdapter for ExtensionCachingAdapter {
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::FILE_ICONS
    }

    fn file_icon(&self, path: &Path) -> Result<Vec<u8>, PlatformError> {
        let key = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mut cache = self.cache.lock().expect("cache lock must not be poisoned");
        if let Some(icon) = cache.get(&key) {
            return Ok(icon.clone());
        }
        self.lookups
            .lock()
            .expect("lookup lock must not be poisoned")
            .push(path.to_path_buf());
        let icon = format!("png:{key}").into_bytes();
        cache.insert(key, icon.clone());
        Ok(icon)
    }
}

async fn spawn_with_adapter(adapter: Arc<dyn PlatformAdapter>) -> (TestServer, tempfile::TempDir) {
    let workspace_directory = tempfile::tempdir().expect("must create workspace directory");
    let fixture_directory = tempfile::tempdir().expect("must create fixture directory");
    let config = ServerConfig {
        workspace_directory: workspace_directory.path().to_path_buf(),
        settings_directory: workspace_directory.path().join("config"),
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
    (
        TestServer::spawn_with_service(config, service, workspace_directory).await,
        fixture_directory,
    )
}

fn icon_url(base_url: &str, uri: &str) -> reqwest::Url {
    let mut url = reqwest::Url::parse(&format!("{base_url}/api/v1/icons"))
        .expect("icon endpoint must be a valid URL");
    url.query_pairs_mut().append_pair("uri", uri);
    url
}

#[tokio::test]
async fn icon_route_preserves_adapter_owned_extension_cache_for_interleaved_requests() {
    let adapter = Arc::new(ExtensionCachingAdapter::default());
    let (server, fixtures) = spawn_with_adapter(adapter.clone()).await;
    let client = reqwest::Client::new();

    for name in ["first.PDF", "photo.png", "second.pdf", "other.PNG"] {
        let path = fixtures.path().join(name);
        std::fs::write(&path, b"fixture").expect("must write fixture");
        let uri = fm_domain::Location::from_native_path(&path)
            .expect("path must become a location")
            .uri;
        let response = client
            .get(icon_url(&server.base_url, &uri))
            .send()
            .await
            .expect("request must succeed");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(
            response.headers().get(reqwest::header::CONTENT_TYPE),
            Some(&reqwest::header::HeaderValue::from_static("image/png"))
        );
    }

    let lookups = adapter
        .lookups
        .lock()
        .expect("lookup lock must not be poisoned");
    assert_eq!(lookups.len(), 2, "only one native lookup per extension");
}

#[tokio::test]
async fn icon_route_maps_an_unsupported_host_to_not_found() {
    let (server, fixtures) =
        spawn_with_adapter(Arc::new(fm_platform::FallbackPlatformAdapter)).await;
    let path = fixtures.path().join("report.pdf");
    std::fs::write(&path, b"fixture").expect("must write fixture");
    let uri = fm_domain::Location::from_native_path(&path)
        .expect("path must become a location")
        .uri;

    let response = reqwest::Client::new()
        .get(icon_url(&server.base_url, &uri))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: serde_json::Value = response.json().await.expect("error body must be JSON");
    assert_eq!(body["code"], "notFound");
}
