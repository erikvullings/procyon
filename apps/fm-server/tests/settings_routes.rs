//! Integration coverage for the settings REST surface.

mod common;

use reqwest::StatusCode;
use serde_json::{Value, json};

use common::TestServer;

#[tokio::test]
async fn get_and_put_settings_persist_the_complete_document() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let initial = client
        .get(format!("{}/api/v1/settings", server.base_url))
        .send()
        .await
        .expect("GET settings");
    assert_eq!(initial.status(), StatusCode::OK);

    let mut settings: Value = initial.json().await.expect("settings json");
    settings["theme"] = json!("dark");
    settings["rowHeight"] = json!(35);
    settings["dateFormat"] = json!("iso");
    settings["sizeFormat"] = json!("decimal");
    settings["showHiddenFiles"] = json!(true);

    let updated = client
        .put(format!("{}/api/v1/settings", server.base_url))
        .json(&settings)
        .send()
        .await
        .expect("PUT settings");
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(
        updated.json::<Value>().await.expect("updated json"),
        settings
    );

    let reloaded = client
        .get(format!("{}/api/v1/settings", server.base_url))
        .send()
        .await
        .expect("reload settings")
        .json::<Value>()
        .await
        .expect("reloaded json");
    assert_eq!(reloaded, settings);
}
