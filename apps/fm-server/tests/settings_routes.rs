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

#[tokio::test]
async fn map_settings_are_validated_and_gate_the_tile_route() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let settings_url = format!("{}/api/v1/settings", server.base_url);
    let mut settings: Value = client
        .get(&settings_url)
        .send()
        .await
        .expect("GET settings")
        .json()
        .await
        .expect("settings json");
    assert_eq!(settings["map"]["basemapKind"], "raster");
    assert_eq!(settings["map"]["remoteHost"], "tile.openstreetmap.org");

    let mut invalid = settings.clone();
    invalid["map"]["urlTemplate"] = json!("https://tiles.example/{z}/{x}.png");
    let rejected = client
        .put(&settings_url)
        .json(&invalid)
        .send()
        .await
        .expect("PUT invalid map");
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);

    settings["map"]["onlineTiles"] = json!(false);
    let saved = client
        .put(&settings_url)
        .json(&settings)
        .send()
        .await
        .expect("PUT offline map");
    assert_eq!(saved.status(), StatusCode::OK);

    let tile = client
        .get(format!("{}/api/v1/map/tiles/3/2/1", server.base_url))
        .send()
        .await
        .expect("tile request");
    assert_eq!(tile.status(), StatusCode::FORBIDDEN);
    let out_of_range = client
        .get(format!("{}/api/v1/map/tiles/40/0/0", server.base_url))
        .send()
        .await
        .expect("tile request");
    assert!(
        out_of_range.status() == StatusCode::BAD_REQUEST
            || out_of_range.status() == StatusCode::FORBIDDEN
    );
}
