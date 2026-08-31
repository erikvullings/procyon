//! End-to-end REST coverage for reading byte ranges and searching content
//! within a single file (task 0088).

mod common;

use common::TestServer;
use fm_domain::Location;
use fm_events::{BackendEventPayload, SessionId, SubscriptionEvent};
use serde_json::{Value, json};

fn location_json(path: &std::path::Path) -> Value {
    let location = Location::from_native_path(path).expect("temp path must be representable");
    json!({
        "providerId": location.provider_id.as_str(),
        "uri": location.uri,
    })
}

#[tokio::test]
async fn scans_disk_usage_with_progress_correlation_fields() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    std::fs::write(root.path().join("fixture.bin"), [1_u8; 9]).expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let workspace_id = uuid::Uuid::new_v4();
    let scan_id = uuid::Uuid::new_v4();
    let mut events = server
        .event_bus
        .subscribe_all_workspaces(SessionId::new("disk-usage-test"), None);

    let response = client
        .post(format!("{}/api/v1/directories/disk-usage", server.base_url))
        .json(&json!({
            "workspaceId": workspace_id,
            "scanId": scan_id,
            "location": location_json(root.path()),
            "expandRoot": false,
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let SubscriptionEvent::Event(event) =
                events.recv().await.expect("scan completion event")
            else {
                continue;
            };
            if let BackendEventPayload::DiskUsageProgress {
                scan_id: event_scan_id,
                root,
                is_complete: true,
                ..
            } = event.payload
                && event_scan_id == scan_id
            {
                assert_eq!(root.children[0].name, "fixture.bin");
                assert!(root.logical_bytes >= 9);
                break;
            }
        }
    })
    .await
    .expect("disk-usage scan must complete");
}

#[tokio::test]
async fn cancel_disk_usage_is_idempotent_before_scan_registration() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let scan_id = uuid::Uuid::new_v4();

    let response = client
        .delete(format!(
            "{}/api/v1/directories/disk-usage/{scan_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn cancel_disk_usage_interrupts_a_running_scan() {
    // Large enough that the scan is still running by the time the cancel request lands, without
    // relying on any artificial delay in the scan itself.
    let root = tempfile::tempdir().expect("must create a temp directory");
    for directory_index in 0..4 {
        let directory = root.path().join(format!("dir-{directory_index}"));
        std::fs::create_dir(&directory).expect("must create fixture subdirectory");
        for file_index in 0..5_000 {
            std::fs::write(directory.join(format!("file-{file_index:05}.txt")), b"x")
                .expect("must create fixture file");
        }
    }
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let workspace_id = uuid::Uuid::new_v4();
    let scan_id = uuid::Uuid::new_v4();
    let mut events = server
        .event_bus
        .subscribe_all_workspaces(SessionId::new("disk-usage-cancel-test"), None);

    let scan = {
        let client = client.clone();
        let base_url = server.base_url.clone();
        let root = root.path().to_path_buf();
        tokio::spawn(async move {
            client
                .post(format!("{base_url}/api/v1/directories/disk-usage"))
                .json(&json!({
                    "workspaceId": workspace_id,
                    "scanId": scan_id,
                    "location": location_json(&root),
                    "expandRoot": false,
                }))
                .send()
                .await
        })
    };

    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let cancelled = client
        .delete(format!(
            "{}/api/v1/directories/disk-usage/{scan_id}",
            server.base_url
        ))
        .send()
        .await
        .expect("cancel request must succeed");
    assert_eq!(cancelled.status(), reqwest::StatusCode::NO_CONTENT);

    let scan_response = scan
        .await
        .expect("scan task must not panic")
        .expect("scan request must succeed");
    assert_eq!(scan_response.status(), reqwest::StatusCode::ACCEPTED);
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let SubscriptionEvent::Event(event) = events.recv().await.expect("scan failure event")
            else {
                continue;
            };
            if let BackendEventPayload::DiskUsageFailed {
                scan_id: event_scan_id,
                code,
                ..
            } = event.payload
                && event_scan_id == scan_id
            {
                assert_eq!(code, "operationCancelled");
                break;
            }
        }
    })
    .await
    .expect("disk-usage cancellation must publish a terminal event");
}

#[tokio::test]
async fn reads_a_byte_range_from_a_file() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let target = root.path().join("report.txt");
    std::fs::write(&target, b"0123456789").expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/files/range", server.base_url))
        .json(&json!({
            "location": location_json(&target),
            "offset": 4,
            "length": 3,
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["data"], json!([52, 53, 54]));
    assert_eq!(body["offset"], 4);
    assert_eq!(body["length"], 3);
    assert_eq!(body["eof"], false);
}

/// A large (multi-megabyte) fixture file, created ad hoc since task 0065's
/// shared large-directory-fixture helper does not exist yet.
fn write_large_fixture(path: &std::path::Path) {
    let line = "the quick brown fox jumps over the lazy dog\n";
    let mut contents = String::with_capacity(3 * 1024 * 1024);
    while contents.len() < 3 * 1024 * 1024 {
        contents.push_str(line);
    }
    std::fs::write(path, contents).expect("must create large fixture");
}

#[tokio::test]
async fn reads_a_range_near_the_end_of_a_large_file() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let target = root.path().join("large.txt");
    write_large_fixture(&target);
    let file_size = std::fs::metadata(&target).expect("fixture metadata").len();
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/files/range", server.base_url))
        .json(&json!({
            "location": location_json(&target),
            "offset": file_size - 10,
            "length": 1000,
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["data"].as_array().unwrap().len(), 10);
    assert_eq!(body["eof"], true);
}

#[tokio::test]
async fn rejects_an_oversized_range_length() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let target = root.path().join("report.txt");
    std::fs::write(&target, b"contents").expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/files/range", server.base_url))
        .json(&json!({
            "location": location_json(&target),
            "offset": 0,
            "length": 10 * 1024 * 1024,
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn searches_a_file_for_a_case_insensitive_substring() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let target = root.path().join("log.txt");
    std::fs::write(
        &target,
        b"first line\nsecond ERROR line\nthird error line\n",
    )
    .expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/files/search", server.base_url))
        .json(&json!({
            "location": location_json(&target),
            "query": "error",
            "regex": false,
            "caseSensitive": false,
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    let matches = body["matches"].as_array().expect("matches array");
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0]["lineNumber"], 2);
    assert_eq!(matches[1]["lineNumber"], 3);
    assert_eq!(body["truncated"], false);
}

#[tokio::test]
async fn rejects_an_invalid_regex_query() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let target = root.path().join("log.txt");
    std::fs::write(&target, b"contents").expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/files/search", server.base_url))
        .json(&json!({
            "location": location_json(&target),
            "query": "(unclosed",
            "regex": true,
            "caseSensitive": false,
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn structured_view_routes_share_one_provider_neutral_session_contract() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let target = root.path().join("report.csv");
    std::fs::write(&target, b"name,notes\nAda,\"one\ntwo\"\n").expect("create CSV fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let opened = client
        .post(format!("{}/api/v1/files/structured/open", server.base_url))
        .json(&json!({
            "location": location_json(&target),
            "format": "csv",
            "headerMode": "firstRow",
        }))
        .send()
        .await
        .expect("open request")
        .error_for_status()
        .expect("open response");
    let opened: Value = opened.json().await.expect("open JSON");
    assert_eq!(opened["headers"], json!(["name", "notes"]));
    assert_eq!(opened["rows"][0]["cells"], json!(["Ada", "one\ntwo"]));
    let session_id = opened["sessionId"].clone();

    let rows = client
        .post(format!("{}/api/v1/files/structured/rows", server.base_url))
        .json(&json!({"sessionId": session_id, "startRow": 0, "count": 100}))
        .send()
        .await
        .expect("row request");
    assert_eq!(rows.status(), reqwest::StatusCode::OK);

    let closed = client
        .post(format!("{}/api/v1/files/structured/close", server.base_url))
        .json(&json!({"sessionId": session_id}))
        .send()
        .await
        .expect("close request");
    assert_eq!(closed.status(), reqwest::StatusCode::NO_CONTENT);
}
