//! End-to-end REST coverage for directory comparison and basic
//! synchronization (spec §16 milestone 5, §37, task 0075).

mod common;

use common::TestServer;
use fm_domain::Location;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

fn location_json(path: &std::path::Path) -> Value {
    let location = Location::from_native_path(path).expect("temp path must be representable");
    json!({
        "providerId": location.provider_id.as_str(),
        "uri": location.uri,
    })
}

async fn poll_comparison_until_complete(
    client: &reqwest::Client,
    base_url: &str,
    comparison_id: &str,
) -> Value {
    for _ in 0..200 {
        let response = client
            .get(format!(
                "{base_url}/api/v1/comparisons/{comparison_id}?limit=500"
            ))
            .send()
            .await
            .expect("request must succeed");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: Value = response.json().await.expect("body must be JSON");
        if body["isComplete"] == json!(true) {
            return body;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("comparison did not complete in time");
}

async fn start_comparison(
    client: &reqwest::Client,
    base_url: &str,
    left: &std::path::Path,
    right: &std::path::Path,
    criteria: &str,
) -> Value {
    let response = client
        .post(format!("{base_url}/api/v1/comparisons"))
        .json(&json!({
            "workspaceId": Uuid::new_v4(),
            "left": location_json(left),
            "right": location_json(right),
            "criteria": criteria,
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    response.json().await.expect("body must be JSON")
}

#[tokio::test]
async fn comparing_two_directories_streams_entries_through_the_paged_endpoint() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let left = root.path().join("left");
    let right = root.path().join("right");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::create_dir_all(&right).unwrap();
    std::fs::write(left.join("same.txt"), b"same").unwrap();
    std::fs::write(right.join("same.txt"), b"same").unwrap();
    std::fs::write(left.join("only-left.txt"), b"left only").unwrap();

    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let started = start_comparison(&client, &server.base_url, &left, &right, "nameOnly").await;
    let comparison_id = started["comparisonId"].as_str().unwrap();

    let page = poll_comparison_until_complete(&client, &server.base_url, comparison_id).await;
    let entries = page["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(page["criteria"], "nameOnly");

    let filtered = client
        .get(format!(
            "{}/api/v1/comparisons/{comparison_id}?differencesOnly=true",
            server.base_url
        ))
        .send()
        .await
        .expect("request must succeed")
        .json::<Value>()
        .await
        .expect("body must be JSON");
    let filtered_entries = filtered["entries"].as_array().unwrap();
    assert_eq!(filtered_entries.len(), 1);
    assert_eq!(filtered_entries[0]["relativePath"], "only-left.txt");
    assert_eq!(filtered_entries[0]["status"], "onlyLeft");
}

#[tokio::test]
async fn cancelling_a_comparison_stops_it_and_unknown_comparisons_are_not_found() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let left = root.path().join("left");
    let right = root.path().join("right");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::create_dir_all(&right).unwrap();
    for index in 0..40 {
        let name = format!("dir-{index}");
        std::fs::create_dir(left.join(&name)).unwrap();
        std::fs::create_dir(right.join(&name)).unwrap();
        for file in 0..40 {
            std::fs::write(left.join(&name).join(format!("f{file}.txt")), b"x").unwrap();
            std::fs::write(right.join(&name).join(format!("f{file}.txt")), b"x").unwrap();
        }
    }
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let started = start_comparison(&client, &server.base_url, &left, &right, "nameOnly").await;
    let comparison_id = started["comparisonId"].as_str().unwrap();

    let response = client
        .post(format!(
            "{}/api/v1/comparisons/{comparison_id}/cancel",
            server.base_url
        ))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);

    let response = client
        .post(format!(
            "{}/api/v1/comparisons/{}/cancel",
            server.base_url,
            Uuid::new_v4()
        ))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);

    let page = poll_comparison_until_complete(&client, &server.base_url, comparison_id).await;
    let entries = page["entries"].as_array().unwrap();
    assert!(
        entries.len() < 1_600,
        "cancellation must stop traversal before every entry is compared, found {}",
        entries.len()
    );
}

#[tokio::test]
async fn generating_and_applying_a_sync_plan_runs_real_operations() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let left = root.path().join("left");
    let right = root.path().join("right");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::create_dir_all(&right).unwrap();
    std::fs::write(left.join("only-left.txt"), b"left only").unwrap();
    std::fs::write(right.join("only-right.txt"), b"right only").unwrap();

    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let started = start_comparison(&client, &server.base_url, &left, &right, "nameOnly").await;
    let comparison_id = started["comparisonId"].as_str().unwrap();
    poll_comparison_until_complete(&client, &server.base_url, comparison_id).await;

    let plan_response = client
        .post(format!(
            "{}/api/v1/comparisons/{comparison_id}/sync-plan",
            server.base_url
        ))
        .json(&json!({ "mode": "mirrorLeftToRight" }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(plan_response.status(), reqwest::StatusCode::OK);
    let plan: Value = plan_response.json().await.expect("body must be JSON");
    let items = plan["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);

    // No filesystem change yet: generating a plan is a dry run (spec §35).
    assert!(!right.join("only-left.txt").exists());
    assert!(right.join("only-right.txt").exists());

    let apply_response = client
        .post(format!(
            "{}/api/v1/comparisons/{comparison_id}/apply-sync-plan",
            server.base_url
        ))
        .json(&json!({ "items": items }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(apply_response.status(), reqwest::StatusCode::CREATED);
    let applied: Value = apply_response.json().await.expect("body must be JSON");
    let operation_ids = applied["operationIds"].as_array().unwrap();
    assert_eq!(operation_ids.len(), 2);

    for _ in 0..200 {
        if right.join("only-left.txt").exists() && !right.join("only-right.txt").exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("sync plan did not apply in time");
}

#[tokio::test]
async fn comparison_endpoints_report_not_found_for_an_unknown_id() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let unknown = Uuid::new_v4();

    let response = client
        .get(format!("{}/api/v1/comparisons/{unknown}", server.base_url))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);

    let response = client
        .post(format!(
            "{}/api/v1/comparisons/{unknown}/sync-plan",
            server.base_url
        ))
        .json(&json!({ "mode": "twoWayUpdate" }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);

    let response = client
        .post(format!(
            "{}/api/v1/comparisons/{unknown}/apply-sync-plan",
            server.base_url
        ))
        .json(&json!({ "items": [] }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}
