//! End-to-end REST coverage for reading byte ranges and searching content
//! within a single file (task 0088).

mod common;

use common::TestServer;
use fm_domain::Location;
use fm_events::{BackendEventPayload, SessionId, SubscriptionEvent};
use serde_json::{Value, json};
use std::io::Write;
use zip::write::SimpleFileOptions;

fn location_json(path: &std::path::Path) -> Value {
    let location = Location::from_native_path(path).expect("temp path must be representable");
    json!({
        "providerId": location.provider_id.as_str(),
        "uri": location.uri,
    })
}

fn write_docx_fixture(path: &std::path::Path) {
    let file = std::fs::File::create(path).expect("create DOCX fixture");
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for (name, contents) in [
        (
            "[Content_Types].xml",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body><w:p><w:r><w:t>Hello DOCX</w:t></w:r></w:p><w:p><w:r><w:drawing><wp:inline><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rImage"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/></Relationships>"#,
        ),
    ] {
        archive
            .start_file(name, options)
            .expect("start DOCX fixture entry");
        archive
            .write_all(contents.as_bytes())
            .expect("write DOCX fixture entry");
    }
    archive
        .start_file("word/media/image1.png", options)
        .expect("start DOCX image");
    archive
        .write_all(b"\x89PNG\r\n\x1a\nfixture")
        .expect("write DOCX image");
    archive.finish().expect("finish DOCX fixture");
}

fn write_pptx_fixture(path: &std::path::Path) {
    let file = std::fs::File::create(path).expect("create PPTX fixture");
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for (name, contents) in [
        (
            "ppt/presentation.xml",
            r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst></p:presentation>"#,
        ),
        (
            "ppt/_rels/presentation.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
        ),
        (
            "ppt/slides/slide1.xml",
            r#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="1" name="Title"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Hello PPTX</a:t></a:r></a:p></p:txBody></p:sp><p:pic><p:nvPicPr><p:cNvPr id="2" name="Picture"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rImage"/></p:blipFill><p:spPr/></p:pic></p:spTree></p:cSld></p:sld>"#,
        ),
        (
            "ppt/slides/_rels/slide1.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#,
        ),
    ] {
        archive
            .start_file(name, options)
            .expect("start PPTX fixture entry");
        archive
            .write_all(contents.as_bytes())
            .expect("write PPTX fixture entry");
    }
    archive
        .start_file("ppt/media/image1.png", options)
        .expect("start PPTX image");
    archive
        .write_all(b"\x89PNG\r\n\x1a\nfixture")
        .expect("write PPTX image");
    archive.finish().expect("finish PPTX fixture");
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

#[tokio::test]
async fn docx_preview_routes_open_read_a_bounded_resource_and_close_one_session() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let target = root.path().join("report.docx");
    write_docx_fixture(&target);
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let opened = client
        .post(format!("{}/api/v1/files/docx/open", server.base_url))
        .json(&json!({ "location": location_json(&target) }))
        .send()
        .await
        .expect("open request")
        .error_for_status()
        .expect("open response");
    let opened: Value = opened.json().await.expect("open JSON");
    assert!(
        opened["html"]
            .as_str()
            .unwrap_or_default()
            .contains("Hello DOCX")
    );
    assert_eq!(opened["resources"].as_array().map(Vec::len), Some(1));
    let session_id = opened["sessionId"].clone();
    let resource_id = opened["resources"][0]["resourceId"].clone();

    let resource = client
        .post(format!("{}/api/v1/files/docx/resource", server.base_url))
        .json(&json!({ "sessionId": session_id, "resourceId": resource_id }))
        .send()
        .await
        .expect("resource request")
        .error_for_status()
        .expect("resource response");
    let resource: Value = resource.json().await.expect("resource JSON");
    assert_eq!(resource["mediaType"], "image/png");
    assert_eq!(resource["data"][0], 137);

    let closed = client
        .post(format!("{}/api/v1/files/docx/close", server.base_url))
        .json(&json!({ "sessionId": opened["sessionId"] }))
        .send()
        .await
        .expect("close request");
    assert_eq!(closed.status(), reqwest::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn pptx_preview_routes_open_read_a_bounded_resource_and_close_one_session() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let target = root.path().join("briefing.pptx");
    write_pptx_fixture(&target);
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let opened = client
        .post(format!("{}/api/v1/files/pptx/open", server.base_url))
        .json(&json!({ "location": location_json(&target) }))
        .send()
        .await
        .expect("open request")
        .error_for_status()
        .expect("open response");
    let opened: Value = opened.json().await.expect("open JSON");
    assert!(
        opened["slides"][0]["markdown"]
            .as_str()
            .unwrap_or_default()
            .contains("Hello PPTX")
    );
    assert_eq!(opened["resources"].as_array().map(Vec::len), Some(1));
    let session_id = opened["sessionId"].clone();
    let resource_id = opened["resources"][0]["resourceId"].clone();

    let resource = client
        .post(format!("{}/api/v1/files/pptx/resource", server.base_url))
        .json(&json!({ "sessionId": session_id, "resourceId": resource_id }))
        .send()
        .await
        .expect("resource request")
        .error_for_status()
        .expect("resource response");
    let resource: Value = resource.json().await.expect("resource JSON");
    assert_eq!(resource["mediaType"], "image/png");
    assert_eq!(resource["data"][0], 137);

    let closed = client
        .post(format!("{}/api/v1/files/pptx/close", server.base_url))
        .json(&json!({ "sessionId": opened["sessionId"] }))
        .send()
        .await
        .expect("close request");
    assert_eq!(closed.status(), reqwest::StatusCode::NO_CONTENT);
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
