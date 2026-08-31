//! Integration tests for task 0009: the deterministic `export-openapi`
//! command shares one document with the served `/api/v1/openapi.json`
//! (spec §9).

use std::net::{IpAddr, Ipv4Addr};
use std::process::Command;

use fm_server::config::ServerConfig;
use utoipa::openapi::OpenApi;

async fn spawn_server() -> (String, tokio::task::JoinHandle<()>) {
    let config = ServerConfig {
        bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: 0,
        ..ServerConfig::default()
    };
    let router = fm_server::build_router(&config);

    let listener = tokio::net::TcpListener::bind((config.bind_address, config.port))
        .await
        .expect("failed to bind an ephemeral port");
    let addr = listener
        .local_addr()
        .expect("bound listener must have a local address");

    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("test server exited unexpectedly");
    });

    (format!("http://{addr}"), handle)
}

#[test]
fn canonical_json_is_byte_for_byte_stable_across_runs() {
    let first = fm_server::openapi_export::canonical_json().expect("first export must succeed");
    let second = fm_server::openapi_export::canonical_json().expect("second export must succeed");

    assert_eq!(first, second, "re-running the export must produce no diff");
}

#[test]
fn canonical_json_has_sorted_keys_fixed_indentation_and_trailing_newline() {
    let bytes = fm_server::openapi_export::canonical_json().expect("export must succeed");
    let text = String::from_utf8(bytes).expect("export must be valid UTF-8");

    assert!(text.ends_with('\n'), "output must end with a newline");
    assert!(
        !text.ends_with("\n\n"),
        "output must have exactly one trailing newline"
    );
    assert!(
        text.contains("\n  \""),
        "top-level fields must use two-space indentation"
    );

    // The `paths` object's keys are sorted alphabetically because
    // `serde_json::Value`'s object map is `BTreeMap`-backed in this
    // workspace (the `preserve_order` feature is never enabled).
    let health_index = text
        .find("\"/api/v1/health\"")
        .expect("health path must be present");
    let runtime_index = text
        .find("\"/api/v1/runtime\"")
        .expect("runtime path must be present");
    assert!(
        health_index < runtime_index,
        "paths must be sorted alphabetically"
    );
}

#[test]
fn write_to_file_creates_parent_directories_and_matches_canonical_json() {
    let dir = std::env::temp_dir().join(format!("fm-server-export-test-{}", uuid::Uuid::new_v4()));
    let path = dir.join("nested").join("openapi.json");

    fm_server::openapi_export::write_to_file(&path).expect("write must succeed");

    let written = std::fs::read(&path).expect("file must have been written");
    let expected = fm_server::openapi_export::canonical_json().expect("canonical json");
    assert_eq!(written, expected);

    std::fs::remove_dir_all(&dir).expect("cleanup must succeed");
}

#[tokio::test]
async fn exported_document_matches_the_document_served_by_the_running_router() {
    let (base_url, handle) = spawn_server().await;

    let response = reqwest::get(format!("{base_url}/api/v1/openapi.json"))
        .await
        .expect("request must succeed");
    let served: serde_json::Value = response.json().await.expect("body must be JSON");

    let exported_bytes =
        fm_server::openapi_export::canonical_json().expect("canonical json must serialize");
    let exported: serde_json::Value =
        serde_json::from_slice(&exported_bytes).expect("export must be valid JSON");

    assert_eq!(
        served, exported,
        "every route registered on the router must appear in the exported document"
    );

    handle.abort();
}

#[test]
fn export_openapi_subcommand_exits_zero_without_binding_a_port() {
    // Reserve an arbitrary free port and pass it explicitly, so the
    // subprocess would fail to bind it if `export-openapi` ever tried to
    // start the web server on top of it.
    let guard = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("must be able to reserve a port for this test");
    let port = guard
        .local_addr()
        .expect("bound listener must have a local address")
        .port();

    let dir = std::env::temp_dir().join(format!(
        "fm-server-export-cli-test-{}",
        uuid::Uuid::new_v4()
    ));
    let path = dir.join("openapi.json");

    let output = Command::new(env!("CARGO_BIN_EXE_fm-server"))
        .args(["--port", &port.to_string(), "export-openapi"])
        .arg(&path)
        .output()
        .expect("failed to run the fm-server binary");

    drop(guard);

    assert!(
        output.status.success(),
        "export-openapi must exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let text = std::fs::read_to_string(&path).expect("output file must have been written");
    let document: OpenApi =
        serde_json::from_str(&text).expect("output must be a valid OpenAPI document");
    assert!(document.paths.paths.contains_key("/api/v1/health"));
    assert!(document.paths.paths.contains_key("/api/v1/runtime"));

    std::fs::remove_dir_all(&dir).expect("cleanup must succeed");
}
