//! End-to-end Finder-tags/Spotlight-comment transport coverage (task 0136).

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::TestServer;
use fm_application::FileManagerService;
use fm_events::EventBus;
use fm_platform::{FinderTag, PlatformAdapter, PlatformCapabilities, PlatformError};
use fm_server::config::ServerConfig;
use fm_transport_dto::RuntimeKindDto;

/// A platform adapter storing Finder tags/comments in memory, keyed by
/// path, so route tests can exercise a real write-then-read round trip
/// without touching a real filesystem's extended attributes.
#[derive(Default)]
struct InMemoryExtendedAttributeAdapter {
    tags: Mutex<HashMap<PathBuf, Vec<FinderTag>>>,
    comments: Mutex<HashMap<PathBuf, String>>,
}

impl PlatformAdapter for InMemoryExtendedAttributeAdapter {
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::FINDER_TAGS | PlatformCapabilities::EXTENDED_ATTRIBUTES
    }

    fn finder_tags(&self, path: &Path) -> Result<Vec<FinderTag>, PlatformError> {
        Ok(self
            .tags
            .lock()
            .expect("tags lock must not be poisoned")
            .get(path)
            .cloned()
            .unwrap_or_default())
    }

    fn set_finder_tags(&self, path: &Path, tags: &[FinderTag]) -> Result<(), PlatformError> {
        let mut store = self.tags.lock().expect("tags lock must not be poisoned");
        if tags.is_empty() {
            store.remove(path);
        } else {
            store.insert(path.to_path_buf(), tags.to_vec());
        }
        Ok(())
    }

    fn spotlight_comment(&self, path: &Path) -> Result<Option<String>, PlatformError> {
        Ok(self
            .comments
            .lock()
            .expect("comments lock must not be poisoned")
            .get(path)
            .cloned())
    }

    fn set_spotlight_comment(
        &self,
        path: &Path,
        comment: Option<&str>,
    ) -> Result<(), PlatformError> {
        let mut store = self
            .comments
            .lock()
            .expect("comments lock must not be poisoned");
        match comment {
            Some(comment) => {
                store.insert(path.to_path_buf(), comment.to_owned());
            }
            None => {
                store.remove(path);
            }
        }
        Ok(())
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

fn entry_url(base_url: &str, path: &str, uri: &str) -> reqwest::Url {
    let mut url =
        reqwest::Url::parse(&format!("{base_url}{path}")).expect("endpoint must be a valid URL");
    url.query_pairs_mut().append_pair("uri", uri);
    url
}

fn fixture_uri(fixtures: &tempfile::TempDir, name: &str) -> String {
    let path = fixtures.path().join(name);
    std::fs::write(&path, b"fixture").expect("must write fixture");
    fm_domain::Location::from_native_path(&path)
        .expect("path must become a location")
        .uri
}

#[tokio::test]
async fn finder_tags_route_round_trips_a_write_through_a_read() {
    let (server, fixtures) =
        spawn_with_adapter(Arc::new(InMemoryExtendedAttributeAdapter::default())).await;
    let uri = fixture_uri(&fixtures, "tagged.txt");
    let client = reqwest::Client::new();

    let empty = client
        .get(entry_url(&server.base_url, "/api/v1/finder-tags", &uri))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(empty.status(), reqwest::StatusCode::OK);
    let empty_body: serde_json::Value = empty.json().await.expect("JSON body");
    assert_eq!(empty_body["tags"], serde_json::json!([]));

    let put = client
        .put(entry_url(&server.base_url, "/api/v1/finder-tags", &uri))
        .json(&serde_json::json!({
            "tags": [
                { "name": "Work", "color": "blue" },
                { "name": "Plain", "color": "none" },
            ]
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(put.status(), reqwest::StatusCode::OK);
    let put_body: serde_json::Value = put.json().await.expect("JSON body");
    assert_eq!(put_body["tags"][0]["name"], "Work");
    assert_eq!(put_body["tags"][0]["color"], "blue");

    let get = client
        .get(entry_url(&server.base_url, "/api/v1/finder-tags", &uri))
        .send()
        .await
        .expect("request must succeed");
    let get_body: serde_json::Value = get.json().await.expect("JSON body");
    assert_eq!(get_body, put_body, "GET must reflect the just-written PUT");
}

#[tokio::test]
async fn finder_tags_route_maps_an_unsupported_host_to_not_found() {
    let (server, fixtures) =
        spawn_with_adapter(Arc::new(fm_platform::FallbackPlatformAdapter)).await;
    let uri = fixture_uri(&fixtures, "report.txt");

    let response = reqwest::Client::new()
        .get(entry_url(&server.base_url, "/api/v1/finder-tags", &uri))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: serde_json::Value = response.json().await.expect("error body must be JSON");
    assert_eq!(body["code"], "notFound");
}

#[tokio::test]
async fn finder_tags_route_rejects_an_invalid_location_uri() {
    let (server, _fixtures) =
        spawn_with_adapter(Arc::new(InMemoryExtendedAttributeAdapter::default())).await;

    let response = reqwest::Client::new()
        .get(entry_url(
            &server.base_url,
            "/api/v1/finder-tags",
            "not a location",
        ))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn spotlight_comment_route_round_trips_a_write_through_a_read_and_clears_it() {
    let (server, fixtures) =
        spawn_with_adapter(Arc::new(InMemoryExtendedAttributeAdapter::default())).await;
    let uri = fixture_uri(&fixtures, "commented.txt");
    let client = reqwest::Client::new();

    let empty = client
        .get(entry_url(
            &server.base_url,
            "/api/v1/spotlight-comment",
            &uri,
        ))
        .send()
        .await
        .expect("request must succeed");
    let empty_body: serde_json::Value = empty.json().await.expect("JSON body");
    assert_eq!(empty_body["comment"], serde_json::Value::Null);

    let put = client
        .put(entry_url(
            &server.base_url,
            "/api/v1/spotlight-comment",
            &uri,
        ))
        .json(&serde_json::json!({ "comment": "Reviewed 2026-08-17" }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(put.status(), reqwest::StatusCode::OK);
    let put_body: serde_json::Value = put.json().await.expect("JSON body");
    assert_eq!(put_body["comment"], "Reviewed 2026-08-17");

    let cleared = client
        .put(entry_url(
            &server.base_url,
            "/api/v1/spotlight-comment",
            &uri,
        ))
        .json(&serde_json::json!({ "comment": null }))
        .send()
        .await
        .expect("request must succeed");
    let cleared_body: serde_json::Value = cleared.json().await.expect("JSON body");
    assert_eq!(cleared_body["comment"], serde_json::Value::Null);
}

#[tokio::test]
async fn spotlight_comment_route_maps_an_unsupported_host_to_not_found() {
    let (server, fixtures) =
        spawn_with_adapter(Arc::new(fm_platform::FallbackPlatformAdapter)).await;
    let uri = fixture_uri(&fixtures, "report.txt");

    let response = reqwest::Client::new()
        .get(entry_url(
            &server.base_url,
            "/api/v1/spotlight-comment",
            &uri,
        ))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}
