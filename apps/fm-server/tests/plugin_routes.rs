//! Integration coverage for the plugin discovery and enablement REST surface.

mod common;

use reqwest::StatusCode;

#[tokio::test]
async fn list_plugins_reports_only_the_bundled_plugins_and_unknown_enablement_is_not_found() {
    let server = common::TestServer::spawn().await;
    let client = reqwest::Client::new();

    // `FileManagerService` always discovers this repo's own `plugins/` directory as its
    // bundled plugin source (`service.rs`: `.with_bundled_directory(CARGO_MANIFEST_DIR/../../plugins)`),
    // independent of the test's isolated workspace/settings directories. A fresh server therefore
    // starts with exactly the plugins currently committed under `plugins/`, not zero.
    let plugins = client
        .get(format!("{}/api/v1/plugins", server.base_url))
        .send()
        .await
        .expect("list plugins");
    assert_eq!(plugins.status(), StatusCode::OK);
    let mut plugin_ids: Vec<String> = plugins
        .json::<Vec<serde_json::Value>>()
        .await
        .expect("plugin JSON")
        .into_iter()
        .map(|plugin| {
            plugin
                .get("id")
                .and_then(serde_json::Value::as_str)
                .expect("plugin descriptor has an id")
                .to_owned()
        })
        .collect();
    plugin_ids.sort();
    assert_eq!(
        plugin_ids,
        vec![
            "catppuccin.icons",
            "sample.copy-markdown-path",
            "sample.file-age"
        ]
    );

    let enable = client
        .post(format!("{}/api/v1/plugins/missing/enable", server.base_url))
        .send()
        .await
        .expect("enable missing plugin");
    assert_eq!(enable.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn plugin_logs_are_not_found_for_an_unknown_plugin() {
    let server = common::TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/api/v1/plugins/missing/logs", server.base_url))
        .send()
        .await
        .expect("get plugin logs");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
