//! REST coverage for the music duplicate finder (task 0265).

mod common;

use common::TestServer;
use fm_domain::Location;
use reqwest::StatusCode;
use serde_json::{Value, json};

fn location_json(path: &std::path::Path) -> Value {
    let location = Location::from_native_path(path).expect("temp path must be representable");
    json!({ "providerId": location.provider_id.as_str(), "uri": location.uri })
}

fn wav() -> Vec<u8> {
    let samples = 800_u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&samples.to_le_bytes());
    bytes.extend(std::iter::repeat_n(7_u8, samples as usize));
    bytes
}

#[tokio::test]
async fn scans_pages_and_reports_unknown_ids() {
    let root = tempfile::tempdir().expect("temp dir");
    std::fs::write(root.path().join("one.wav"), wav()).expect("one");
    std::fs::write(root.path().join("two.wav"), wav()).expect("two");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let started = client
        .post(format!("{}/api/v1/music-duplicate-scans", server.base_url))
        .json(&json!({ "roots": [location_json(root.path())] }))
        .send()
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::CREATED);
    let scan_id = started.json::<Value>().await.expect("json")["scanId"]
        .as_str()
        .expect("scan id")
        .to_owned();

    let mut page = Value::Null;
    for _ in 0..200 {
        page = client
            .get(format!(
                "{}/api/v1/music-duplicate-scans/{scan_id}",
                server.base_url
            ))
            .send()
            .await
            .expect("page")
            .json()
            .await
            .expect("json");
        if page["isComplete"] == true {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(page["isComplete"], true);
    assert_eq!(page["groups"].as_array().map(Vec::len), Some(1));
    assert_eq!(page["groups"][0]["confidence"], "exact");

    let missing = client
        .get(format!(
            "{}/api/v1/music-duplicate-scans/{}",
            server.base_url,
            uuid::Uuid::new_v4()
        ))
        .send()
        .await
        .expect("missing");
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let cancel = client
        .post(format!(
            "{}/api/v1/music-duplicate-scans/{scan_id}/cancel",
            server.base_url
        ))
        .send()
        .await
        .expect("cancel");
    assert_eq!(cancel.status(), StatusCode::NO_CONTENT);
}
