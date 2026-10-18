//! Range semantics of the seekable media route (task 0260).

mod common;

use common::TestServer;
use fm_domain::Location;
use reqwest::StatusCode;
use serde_json::json;

fn media_url(server: &TestServer, path: &std::path::Path) -> String {
    let location = Location::from_native_path(path).expect("temp path must be representable");
    let json = json!({ "providerId": location.provider_id.as_str(), "uri": location.uri });
    let mut url = reqwest::Url::parse(&format!("{}/api/v1/media", server.base_url)).expect("url");
    url.query_pairs_mut()
        .append_pair("location", &json.to_string());
    url.to_string()
}

async fn fetch(url: &str, range: Option<&str>) -> reqwest::Response {
    let client = reqwest::Client::new();
    let mut request = client.get(url);
    if let Some(range) = range {
        request = request.header("Range", range);
    }
    request.send().await.expect("request must succeed")
}

fn header(response: &reqwest::Response, name: &str) -> String {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn serves_the_whole_file_and_advertises_range_support() {
    let root = tempfile::tempdir().expect("temp dir");
    let target = root.path().join("track.mp3");
    std::fs::write(&target, b"0123456789").expect("fixture");
    let server = TestServer::spawn().await;

    let response = fetch(&media_url(&server, &target), None).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "accept-ranges"), "bytes");
    assert_eq!(header(&response, "content-type"), "audio/mpeg");
    assert_eq!(
        response.bytes().await.expect("body").as_ref(),
        b"0123456789"
    );
}

#[tokio::test]
async fn serves_bounded_open_ended_and_suffix_ranges_with_content_range() {
    let root = tempfile::tempdir().expect("temp dir");
    let target = root.path().join("track.mp3");
    std::fs::write(&target, b"0123456789").expect("fixture");
    let server = TestServer::spawn().await;
    let url = media_url(&server, &target);

    for (range, content_range, body) in [
        ("bytes=2-4", "bytes 2-4/10", &b"234"[..]),
        ("bytes=7-", "bytes 7-9/10", &b"789"[..]),
        ("bytes=-3", "bytes 7-9/10", &b"789"[..]),
        ("bytes=8-99", "bytes 8-9/10", &b"89"[..]),
    ] {
        let response = fetch(&url, Some(range)).await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{range}");
        assert_eq!(header(&response, "content-range"), content_range, "{range}");
        assert_eq!(
            response.bytes().await.expect("body").as_ref(),
            body,
            "{range}"
        );
    }
}

#[tokio::test]
async fn out_of_range_requests_get_416_with_the_total_size() {
    let root = tempfile::tempdir().expect("temp dir");
    let target = root.path().join("track.mp3");
    std::fs::write(&target, b"0123456789").expect("fixture");
    let server = TestServer::spawn().await;

    let response = fetch(&media_url(&server, &target), Some("bytes=10-")).await;

    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(header(&response, "content-range"), "bytes */10");
}

#[tokio::test]
async fn missing_files_and_bad_locations_are_rejected() {
    let root = tempfile::tempdir().expect("temp dir");
    let server = TestServer::spawn().await;

    let missing = fetch(&media_url(&server, &root.path().join("nope.mp3")), None).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let bad = fetch(
        &format!("{}/api/v1/media?location=not-json", server.base_url),
        None,
    )
    .await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
}
