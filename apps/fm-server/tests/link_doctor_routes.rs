//! REST coverage for the broken-link doctor (task 0266).

mod common;

use common::TestServer;
use fm_domain::Location;
use reqwest::StatusCode;
use serde_json::{Value, json};

fn location_json(path: &std::path::Path) -> Value {
    let location = Location::from_native_path(path).expect("temp path must be representable");
    json!({ "providerId": location.provider_id.as_str(), "uri": location.uri })
}

#[tokio::test]
async fn scans_for_broken_document_links_and_searches_replacements() {
    let root = tempfile::tempdir().expect("temp dir");
    std::fs::write(root.path().join("readme.md"), "[gone](missing.md)").expect("readme");
    std::fs::create_dir_all(root.path().join("moved")).expect("dir");
    std::fs::write(root.path().join("moved/missing.md"), "hello").expect("moved");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let started = client
        .post(format!("{}/api/v1/link-doctor-scans", server.base_url))
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
                "{}/api/v1/link-doctor-scans/{scan_id}",
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
    assert_eq!(page["problems"].as_array().map(Vec::len), Some(1));
    assert_eq!(page["problems"][0]["kind"], "missingDocumentLink");
    assert_eq!(page["problems"][0]["target"], "missing.md");

    let replacements: Value = client
        .post(format!(
            "{}/api/v1/link-doctor-replacements",
            server.base_url
        ))
        .json(&json!({ "roots": [location_json(root.path())], "targetName": "missing.md" }))
        .send()
        .await
        .expect("replacements")
        .json()
        .await
        .expect("json");
    assert_eq!(replacements["candidates"].as_array().map(Vec::len), Some(1));

    let missing = client
        .get(format!(
            "{}/api/v1/link-doctor-scans/{}",
            server.base_url,
            uuid::Uuid::new_v4()
        ))
        .send()
        .await
        .expect("missing");
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}
