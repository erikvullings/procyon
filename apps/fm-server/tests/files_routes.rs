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
            "[Content_Types].xml",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#,
        ),
        (
            "ppt/presentation.xml",
            r#"<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#,
        ),
        (
            "ppt/_rels/presentation.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
        ),
        (
            "ppt/slides/slide1.xml",
            r#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="914400" y="914400"/><a:ext cx="10000000" cy="1000000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US" sz="3200"/><a:t>Hello PPTX</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
        ),
    ] {
        archive
            .start_file(name, options)
            .expect("start PPTX fixture entry");
        archive
            .write_all(contents.as_bytes())
            .expect("write PPTX fixture entry");
    }
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
                tree,
                is_complete: true,
                ..
            } = event.payload
                && event_scan_id == scan_id
            {
                assert_eq!(tree.names[1], "fixture.bin");
                assert_eq!(tree.parents[1], 0);
                assert!(tree.logical_bytes[0] >= 9);
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
async fn pptx_preview_routes_open_read_rendered_pdf_and_close_one_session() {
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
    assert_eq!(
        &opened["firstPagePdf"]
            .as_array()
            .expect("first-page PDF byte array")[..5],
        &json!([37, 80, 68, 70, 45])
            .as_array()
            .expect("PDF signature array")[..]
    );
    let session_id = opened["sessionId"].clone();

    let range = client
        .post(format!("{}/api/v1/files/pptx/pdf", server.base_url))
        .json(&json!({ "sessionId": session_id, "offset": 0, "length": 5 }))
        .send()
        .await
        .expect("PDF range request")
        .error_for_status()
        .expect("PDF range response");
    let range: Value = range.json().await.expect("PDF range JSON");
    assert_eq!(range["data"], json!([37, 80, 68, 70, 45]));

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

#[tokio::test]
async fn resolves_metadata_tokens_for_files() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let target = root.path().join("notes.txt");
    std::fs::write(&target, b"hello").expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/api/v1/files/metadata-tokens", server.base_url))
        .json(&json!({
            "locations": [location_json(&target)],
            "tokens": ["artist", "nonsense"],
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("body must be JSON");
    assert_eq!(body["unknownTokens"], json!(["nonsense"]));
    assert_eq!(body["entries"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["entries"][0]["values"], json!({}));
}

#[tokio::test]
async fn reads_image_metadata_and_flags_unsupported_files() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let png = root.path().join("pic.png");
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&7_u32.to_be_bytes());
    ihdr.extend_from_slice(&3_u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    bytes.extend_from_slice(&13_u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&ihdr);
    bytes.extend_from_slice(&[0; 4]);
    std::fs::write(&png, bytes).expect("must create fixture");
    let text = root.path().join("notes.txt");
    std::fs::write(&text, b"hello").expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let url = format!("{}/api/v1/files/image-metadata", server.base_url);

    let body: Value = client
        .post(&url)
        .json(&json!({"location": location_json(&png)}))
        .send()
        .await
        .expect("request must succeed")
        .json()
        .await
        .expect("body must be JSON");
    assert_eq!(body["format"], "PNG");
    assert_eq!(body["supported"], true);
    assert_eq!(body["groups"][0]["namespace"], "File");

    let body: Value = client
        .post(&url)
        .json(&json!({"location": location_json(&text)}))
        .send()
        .await
        .expect("request must succeed")
        .json()
        .await
        .expect("body must be JSON");
    assert_eq!(body["supported"], false);
    assert_eq!(body["groups"], json!([]));
}

#[tokio::test]
async fn reads_image_gps_in_bulk_and_flags_unreadable_files() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let plain = root.path().join("plain.png");
    std::fs::write(&plain, b"\x89PNG\r\n\x1a\n").expect("must create fixture");
    let missing = root.path().join("missing.jpg");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let body: Value = client
        .post(format!("{}/api/v1/files/image-gps", server.base_url))
        .json(&json!({"locations": [location_json(&plain), location_json(&missing)]}))
        .send()
        .await
        .expect("request must succeed")
        .json()
        .await
        .expect("body must be JSON");
    assert_eq!(body["entries"].as_array().map(Vec::len), Some(2));
    assert_eq!(body["entries"][0]["unreadable"], false);
    assert!(body["entries"][0].get("gps").is_none());
    assert_eq!(body["entries"][1]["unreadable"], true);
}

#[tokio::test]
async fn reads_geojson_files_and_reports_invalid_ones_with_a_position() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let good = root.path().join("places.geojson");
    std::fs::write(
        &good,
        r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[4.9,52.3]},"properties":{"name":"A"}}]}"#,
    )
    .expect("must create fixture");
    let bad = root.path().join("bad.geojson");
    std::fs::write(
        &bad,
        r#"{"type":"Feature","geometry":{"type":"LineString","coordinates":[[0,0]]},"properties":{}}"#,
    )
    .expect("must create fixture");
    let plain = root.path().join("data.json");
    std::fs::write(&plain, r#"{"name":"x"}"#).expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let url = format!("{}/api/v1/files/geo/read", server.base_url);
    let read = |path: std::path::PathBuf| {
        let client = client.clone();
        let url = url.clone();
        async move {
            client
                .post(url)
                .json(&json!({"location": location_json(&path)}))
                .send()
                .await
                .expect("request must succeed")
                .json::<Value>()
                .await
                .expect("body must be JSON")
        }
    };

    let body = read(good).await;
    assert_eq!(body["format"], "geojson");
    assert_eq!(body["displayedFeatures"], 1);
    assert_eq!(body["bounds"], json!([4.9, 52.3, 4.9, 52.3]));
    assert!(body.get("error").is_none());

    let body = read(bad).await;
    assert_eq!(body["error"]["path"], "/geometry/coordinates");
    assert_eq!(body["displayedFeatures"], 0);

    let body = read(plain).await;
    assert_eq!(body["format"], "");
    assert!(body["error"]["message"].as_str().is_some());
}

#[tokio::test]
async fn validates_geojson_text_without_a_file() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let url = format!("{}/api/v1/geo/parse", server.base_url);
    let ok: Value = client
        .post(&url)
        .json(&json!({"text": r#"{"type":"Point","coordinates":[1,2]}"#}))
        .send()
        .await
        .expect("request must succeed")
        .json()
        .await
        .expect("body must be JSON");
    assert_eq!(ok["displayedFeatures"], 1);
    let bad: Value = client
        .post(&url)
        .json(&json!({"text": "{\n \"type\": }"}))
        .send()
        .await
        .expect("request must succeed")
        .json()
        .await
        .expect("body must be JSON");
    assert_eq!(bad["error"]["line"], 2);
}

#[tokio::test]
async fn reads_gpx_tracks_with_statistics_and_rejects_entity_declarations() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let track = root.path().join("run.gpx");
    std::fs::write(
        &track,
        r#"<gpx version="1.1"><trk><name>Run</name><trkseg>
        <trkpt lat="52.000" lon="4.0"><ele>1</ele><time>2024-05-05T08:00:00Z</time></trkpt>
        <trkpt lat="52.001" lon="4.0"><ele>9</ele><time>2024-05-05T08:00:30Z</time></trkpt>
        </trkseg></trk></gpx>"#,
    )
    .expect("must create fixture");
    let hostile = root.path().join("hostile.gpx");
    std::fs::write(
        &hostile,
        r#"<?xml version="1.0"?><!DOCTYPE gpx [<!ENTITY x SYSTEM "file:///etc/passwd">]><gpx version="1.1"/>"#,
    )
    .expect("must create fixture");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let url = format!("{}/api/v1/files/geo/read", server.base_url);
    let read = |path: std::path::PathBuf| {
        let client = client.clone();
        let url = url.clone();
        async move {
            client
                .post(url)
                .json(&json!({"location": location_json(&path)}))
                .send()
                .await
                .expect("request must succeed")
                .json::<Value>()
                .await
                .expect("body must be JSON")
        }
    };

    let body = read(track).await;
    assert_eq!(body["format"], "gpx");
    assert_eq!(body["tracks"][0]["name"], "Run");
    assert_eq!(body["tracks"][0]["elapsedSeconds"], 30.0);
    assert_eq!(body["tracks"][0]["ascentMeters"], 8.0);
    assert!(
        body["tracks"][0]["profile"]
            .as_array()
            .is_some_and(|p| p.len() == 2)
    );

    let body = read(hostile).await;
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("DOCTYPE"))
    );
}

#[tokio::test]
async fn reads_kml_and_kmz_files_with_styles_icons_and_skipped_elements() {
    use std::io::Write;
    let root = tempfile::tempdir().expect("must create a temp directory");
    let kml = br#"<kml><Document><name>Trip</name>
        <Style id="s"><LineStyle><color>ff0000ff</color><width>3</width></LineStyle><IconStyle><Icon><href>pin.png</href></Icon></IconStyle></Style>
        <Placemark><name>Line</name><styleUrl>#s</styleUrl><LineString><coordinates>1,2 3,4</coordinates></LineString></Placemark>
        <Placemark><name>Pin</name><styleUrl>#s</styleUrl><Point><coordinates>1,2</coordinates></Point></Placemark>
        <NetworkLink><name>Live</name><Link><href>http://x.example/f.kml</href></Link></NetworkLink>
        </Document></kml>"#;
    let plain = root.path().join("trip.kml");
    std::fs::write(&plain, kml).expect("must create fixture");
    let archive = root.path().join("trip.kmz");
    {
        let file = std::fs::File::create(&archive).expect("must create archive");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("doc.kml", options).expect("start");
        writer.write_all(kml).expect("write");
        writer.start_file("pin.png", options).expect("start");
        writer.write_all(b"\x89PNG\r\n\x1a\nxx").expect("write");
        writer.finish().expect("finish");
    }
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let url = format!("{}/api/v1/files/geo/read", server.base_url);
    let read = |path: std::path::PathBuf| {
        let client = client.clone();
        let url = url.clone();
        async move {
            client
                .post(url)
                .json(&json!({"location": location_json(&path)}))
                .send()
                .await
                .expect("request must succeed")
                .json::<Value>()
                .await
                .expect("body must be JSON")
        }
    };

    let body = read(plain).await;
    assert_eq!(body["format"], "kml");
    assert_eq!(body["displayedFeatures"], 2);
    assert_eq!(
        body["collection"]["features"][0]["properties"]["stroke"],
        "#ff0000"
    );
    assert!(
        body["skipped"][0]
            .as_str()
            .is_some_and(|s| s.contains("not fetched"))
    );
    assert_eq!(body["resources"], json!({}));

    let body = read(archive).await;
    assert_eq!(body["format"], "kmz");
    assert_eq!(
        body["collection"]["features"][1]["properties"]["_icon"],
        "pin.png"
    );
    assert!(
        body["resources"]["pin.png"]
            .as_str()
            .is_some_and(|s| s.starts_with("data:image/png"))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn inspects_and_previews_attributes_inside_accessible_roots() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().expect("must create a temp directory");
    let dir = root.path().join("tree");
    std::fs::create_dir(&dir).expect("must create fixture");
    let file = dir.join("a.txt");
    std::fs::write(&file, b"x").expect("must create fixture");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))
        .expect("must set fixture mode");
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let inspected: Value = client
        .post(format!("{}/api/v1/attributes/inspect", server.base_url))
        .json(&json!({"sources": [location_json(&file)]}))
        .send()
        .await
        .expect("request must succeed")
        .json()
        .await
        .expect("body must be JSON");
    assert_eq!(inspected["support"]["mode"], true);
    assert_eq!(inspected["entries"][0]["attributes"]["mode"], 0o644);

    let preview: Value = client
        .post(format!("{}/api/v1/attributes/preview", server.base_url))
        .json(&json!({
            "sources": [location_json(&dir)],
            "recursive": true,
            "scope": "filesOnly"
        }))
        .send()
        .await
        .expect("request must succeed")
        .json()
        .await
        .expect("body must be JSON");
    assert_eq!(preview["total"], 1);
    assert_eq!(preview["files"], 1);
    assert_eq!(preview["truncated"], false);
}
