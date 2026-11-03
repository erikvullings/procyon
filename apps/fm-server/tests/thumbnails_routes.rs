//! End-to-end thumbnail transport coverage (task 0134).

mod common;

use std::io::Cursor;
use std::sync::Arc;

use common::TestServer;
use fm_application::FileManagerService;
use fm_events::EventBus;
use fm_server::config::ServerConfig;
use fm_transport_dto::RuntimeKindDto;

async fn spawn() -> (TestServer, tempfile::TempDir) {
    let workspace_directory = tempfile::tempdir().expect("must create workspace directory");
    let fixture_directory = tempfile::tempdir().expect("must create fixture directory");
    let config = ServerConfig {
        workspace_directory: workspace_directory.path().to_path_buf(),
        settings_directory: workspace_directory.path().join("config"),
        port: 0,
        dev_mode_auth_disabled: true,
        ..ServerConfig::default()
    };
    let service = Arc::new(FileManagerService::from_config(
        fm_application::ServiceConfig::new(
            RuntimeKindDto::BrowserServer,
            config.workspace_directory.clone(),
            config.settings_directory.clone(),
        )
        .events(EventBus::default())
        .platform(Arc::new(fm_platform::FallbackPlatformAdapter)),
    ));
    (
        TestServer::spawn_with_service(config, service, workspace_directory).await,
        fixture_directory,
    )
}

fn thumbnail_url(base_url: &str, uri: &str, size: &str) -> reqwest::Url {
    let mut url = reqwest::Url::parse(&format!("{base_url}/api/v1/thumbnails"))
        .expect("thumbnail endpoint must be a valid URL");
    url.query_pairs_mut()
        .append_pair("uri", uri)
        .append_pair("size", size);
    url
}

/// An opaque photo-like PNG (transparent images are served as PNG, see the test below).
fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        width,
        height,
        image::Rgba([10, 200, 30, 255]),
    ));
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("encode fixture png");
    bytes
}

#[tokio::test]
async fn thumbnail_route_returns_a_downscaled_jpeg_for_a_supported_image() {
    let (server, fixtures) = spawn().await;
    let path = fixtures.path().join("photo.png");
    std::fs::write(&path, png_bytes(200, 100)).expect("must write fixture");
    let uri = fm_domain::Location::from_native_path(&path)
        .expect("path must become a location")
        .uri;

    let response = reqwest::Client::new()
        .get(thumbnail_url(&server.base_url, &uri, "small"))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.headers().get(reqwest::header::CONTENT_TYPE),
        Some(&reqwest::header::HeaderValue::from_static("image/jpeg"))
    );
    let bytes = response.bytes().await.expect("body must read");
    let decoded = image::load_from_memory(&bytes).expect("body must decode as an image");
    assert!(decoded.width() <= 64, "small thumbnails are capped at 64px");
}

#[tokio::test]
async fn thumbnail_route_serves_a_transparent_image_as_png() {
    let (server, fixtures) = spawn().await;
    let path = fixtures.path().join("logo.png");
    // Fully transparent apart from one opaque block: JPEG would turn the rest black.
    let mut logo = image::RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 0]));
    for x in 20..44 {
        for y in 20..44 {
            logo.put_pixel(x, y, image::Rgba([250, 0, 0, 255]));
        }
    }
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(logo)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("encode fixture png");
    std::fs::write(&path, bytes).expect("must write fixture");
    let uri = fm_domain::Location::from_native_path(&path)
        .expect("path must become a location")
        .uri;

    let response = reqwest::Client::new()
        .get(thumbnail_url(&server.base_url, &uri, "small"))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.headers().get(reqwest::header::CONTENT_TYPE),
        Some(&reqwest::header::HeaderValue::from_static("image/png"))
    );
    let body = response.bytes().await.expect("body must read");
    let decoded = image::load_from_memory(&body)
        .expect("body must decode as an image")
        .to_rgba8();
    assert_eq!(decoded.get_pixel(0, 0).0[3], 0, "transparency is kept");
}

#[tokio::test]
async fn thumbnail_route_maps_an_unsupported_extension_to_not_found() {
    let (server, fixtures) = spawn().await;
    let path = fixtures.path().join("notes.txt");
    std::fs::write(&path, b"just text").expect("must write fixture");
    let uri = fm_domain::Location::from_native_path(&path)
        .expect("path must become a location")
        .uri;

    let response = reqwest::Client::new()
        .get(thumbnail_url(&server.base_url, &uri, "small"))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: serde_json::Value = response.json().await.expect("error body must be JSON");
    assert_eq!(body["code"], "notFound");
}

#[tokio::test]
async fn thumbnail_route_maps_an_invalid_size_to_bad_request() {
    let (server, fixtures) = spawn().await;
    let path = fixtures.path().join("photo.png");
    std::fs::write(&path, png_bytes(20, 20)).expect("must write fixture");
    let uri = fm_domain::Location::from_native_path(&path)
        .expect("path must become a location")
        .uri;

    let response = reqwest::Client::new()
        .get(thumbnail_url(&server.base_url, &uri, "huge"))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await.expect("error body must be JSON");
    assert_eq!(body["code"], "invalidRequest");
}
