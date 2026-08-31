//! End-to-end REST coverage for starting, listing, and cancelling a
//! recursive filesystem search (task 0068).

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

/// Polls `/api/v1/directories/list` until the expected streamed results are visible.
///
/// `hasMore` only describes pages buffered at request time; search completion is
/// signalled over the event stream and must not keep this request open.
async fn poll_until_entry_count(
    client: &reqwest::Client,
    base_url: &str,
    location: &Value,
    expected_count: usize,
) -> Value {
    for _ in 0..100 {
        let response = client
            .post(format!("{base_url}/api/v1/directories/list"))
            .json(&json!({
                "workspaceId": Uuid::new_v4(),
                "paneId": Uuid::new_v4(),
                "requestId": Uuid::new_v4(),
                "location": location,
                "sort": [],
                "showHidden": true,
                "foldersFirst": false,
            }))
            .send()
            .await
            .expect("request must succeed");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: Value = response.json().await.expect("body must be JSON");
        if body["entries"].as_array().map(Vec::len) == Some(expected_count) {
            return body;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("search results did not become visible in time");
}

#[tokio::test]
async fn starting_a_search_streams_matches_through_the_directory_listing_endpoint() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    std::fs::write(root.path().join("report.txt"), b"a").expect("must create fixture");
    std::fs::write(root.path().join("invoice.txt"), b"b").expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/search", server.base_url))
        .json(&json!({
            "workspaceId": Uuid::new_v4(),
            "roots": [location_json(root.path())],
            "query": "report",
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let started: Value = response.json().await.expect("body must be JSON");
    assert!(started["searchId"].is_string());
    assert_eq!(started["location"]["providerId"], "search");
    assert!(started["executionMode"].is_string());
    let search_location = started["location"].clone();

    let listing = poll_until_entry_count(&client, &server.base_url, &search_location, 1).await;
    assert_eq!(listing["entries"].as_array().unwrap().len(), 1);
    assert_eq!(listing["entries"][0]["name"], "report.txt");
}

#[tokio::test]
async fn cancelling_a_search_stops_it_and_unknown_searches_are_not_found() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    for index in 0..500 {
        std::fs::write(root.path().join(format!("match-{index}.txt")), b"x")
            .expect("must create fixture");
    }
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/search", server.base_url))
        .json(&json!({
            "workspaceId": Uuid::new_v4(),
            "roots": [location_json(root.path())],
            "query": "match",
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let started: Value = response.json().await.expect("body must be JSON");
    let search_id = started["searchId"].as_str().unwrap();

    let response = client
        .post(format!(
            "{}/api/v1/search/{search_id}/cancel",
            server.base_url
        ))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);

    let response = client
        .post(format!(
            "{}/api/v1/search/{}/cancel",
            server.base_url,
            Uuid::new_v4()
        ))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn starting_a_search_with_no_roots_is_a_bad_request() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/search", server.base_url))
        .json(&json!({
            "workspaceId": Uuid::new_v4(),
            "roots": [],
            "query": "report",
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn content_search_finds_text_across_files() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    std::fs::write(root.path().join("a.txt"), b"alpha\nneedle here\n")
        .expect("must create fixture");
    std::fs::write(root.path().join("b.txt"), b"no match\n").expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/search", server.base_url))
        .json(&json!({
            "workspaceId": Uuid::new_v4(),
            "roots": [location_json(root.path())],
            "query": "",
            "contentQuery": "needle",
            "contentRegex": false,
            "recurse": true,
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let started: Value = response.json().await.expect("body must be JSON");
    let search_location = started["location"].clone();

    let listing = poll_until_entry_count(&client, &server.base_url, &search_location, 1).await;
    let entries = listing["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "only a.txt should match content");
    assert_eq!(entries[0]["name"], "a.txt");
}

#[tokio::test]
async fn content_search_with_invalid_regex_is_rejected() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    std::fs::write(root.path().join("x.txt"), b"x").expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/search", server.base_url))
        .json(&json!({
            "workspaceId": Uuid::new_v4(),
            "roots": [location_json(root.path())],
            "query": "",
            "contentQuery": "[invalid",
            "contentRegex": true,
            "recurse": true,
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
}
