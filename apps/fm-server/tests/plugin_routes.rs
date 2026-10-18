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
            "procyon.preview-eml",
            "procyon.preview-torrent",
            "procyon.preview-vcard-ical",
            "procyon.svgo",
            "sample.copy-markdown-path",
            "sample.file-age",
            "sample.js-svg-uri"
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

#[tokio::test]
async fn bundled_preview_plugins_are_enabled_by_default_and_render_a_local_file() {
    let server = common::TestServer::spawn().await;
    let client = reqwest::Client::new();
    let directory = tempfile::tempdir().expect("temp directory");
    let file = directory.path().join("contact.vcf");
    std::fs::write(&file, "BEGIN:VCARD\nFN:Ada <Lovelace>\nEND:VCARD\n").expect("fixture");
    let location = serde_json::json!({
        "providerId": "local",
        "uri": common::file_uri(&file),
    });
    let preview = |plugin: &str| {
        client
            .post(format!(
                "{}/api/v1/plugins/{plugin}/preview",
                server.base_url
            ))
            .json(&serde_json::json!({ "location": location }))
            .send()
    };

    let response = preview("procyon.preview-vcard-ical")
        .await
        .expect("preview");
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response.json().await.expect("preview JSON");
    assert_eq!(body["kind"], "html");
    assert!(
        body["body"]
            .as_str()
            .is_some_and(|html| html.contains("Ada &lt;Lovelace&gt;")),
        "{body}"
    );

    // A plugin that does not claim `.vcf` is a client error, and an unknown one is not found.
    assert_eq!(
        preview("procyon.preview-torrent")
            .await
            .expect("unclaimed")
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        preview("missing.plugin").await.expect("unknown").status(),
        StatusCode::NOT_FOUND
    );
}
