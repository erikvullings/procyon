//! REST coverage for folder snapshots (task 0268).

mod common;

use common::TestServer;
use fm_domain::Location;
use reqwest::StatusCode;
use serde_json::{Value, json};

fn location_json(path: &std::path::Path) -> Value {
    let location = Location::from_native_path(path).expect("temp path must be representable");
    json!({ "providerId": location.provider_id.as_str(), "uri": location.uri })
}

async fn finish(client: &reqwest::Client, base: &str, job_id: &str) -> Value {
    for _ in 0..200 {
        let page: Value = client
            .get(format!("{base}/api/v1/snapshot-jobs/{job_id}"))
            .send()
            .await
            .expect("page")
            .json()
            .await
            .expect("json");
        if page["isComplete"] == true {
            return page;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("job did not finish");
}

#[tokio::test]
async fn creates_a_snapshot_and_reports_what_changed_since() {
    let root = tempfile::tempdir().expect("temp dir");
    let tree = root.path().join("tree");
    std::fs::create_dir_all(&tree).expect("tree");
    std::fs::write(tree.join("a.txt"), b"alpha").expect("a");
    let snapshot = root.path().join("tree.procyon-snapshot");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let created: Value = client
        .post(format!("{}/api/v1/snapshot-jobs", server.base_url))
        .json(&json!({
            "type": "create",
            "root": location_json(&tree),
            "destination": location_json(&snapshot),
            "withChecksums": true
        }))
        .send()
        .await
        .expect("create")
        .json()
        .await
        .expect("json");
    let page = finish(
        &client,
        &server.base_url,
        created["jobId"].as_str().expect("id"),
    )
    .await;
    assert!(page["error"].is_null(), "{page}");
    assert!(snapshot.exists());

    std::fs::write(tree.join("b.txt"), b"beta").expect("b");
    let compared: Value = client
        .post(format!("{}/api/v1/snapshot-jobs", server.base_url))
        .json(&json!({
            "type": "compare",
            "snapshot": location_json(&snapshot),
            "target": location_json(&tree)
        }))
        .send()
        .await
        .expect("compare")
        .json()
        .await
        .expect("json");
    let page = finish(
        &client,
        &server.base_url,
        compared["jobId"].as_str().expect("id"),
    )
    .await;
    assert_eq!(page["counts"]["added"], 1);
    assert_eq!(page["entries"][0]["path"], "b.txt");

    let missing = client
        .get(format!(
            "{}/api/v1/snapshot-jobs/{}",
            server.base_url,
            uuid::Uuid::new_v4()
        ))
        .send()
        .await
        .expect("missing");
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}
