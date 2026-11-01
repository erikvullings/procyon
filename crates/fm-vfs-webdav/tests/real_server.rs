//! Integration coverage for the WebDAV provider against a **real, third-party
//! WebDAV server implementation** — Apache httpd with `mod_dav` — rather than
//! the in-process hand-rolled fixture (`tests/provider.rs`).
//!
//! This is the acceptance criterion that the sandbox could not satisfy back in
//! task 0147 (no Docker/`podman` there): *"tested against at least one real
//! server implementation (e.g. a Nextcloud test container) rather than a
//! hand-rolled fixture only."*
//!
//! The server is started out-of-band (a local `podman` container running the
//! `fm-webdav-test` image built from `httpd:2.4` + `mod_dav`, see the task's
//! Agent Notes). The URL/credentials are read from the environment; when they
//! are unset these tests are skipped so the ordinary `cargo test` suite never
//! requires a live server.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use fm_domain::{Location, ProviderId};
use fm_vfs::{
    EntryRef, FileSystemProvider, ListOptions, ProviderReadStream, RemoveOptions, VfsError,
    WriteOptions,
};
use fm_vfs_webdav::{
    WebDavAuthScheme, WebDavConnectionParameters, WebDavConnectionResolver,
    WebDavFileSystemProvider,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;

const CONNECTION_ID: &str = "33333333-3333-4333-8333-333333333333";

struct EnvResolver {
    base_url: String,
}

#[async_trait]
impl WebDavConnectionResolver for EnvResolver {
    async fn resolve(&self, _id: &str) -> Result<WebDavConnectionParameters, VfsError> {
        Ok(WebDavConnectionParameters {
            base_url: self.base_url.clone(),
            username: std::env::var("FM_WEBDAV_REAL_USER").unwrap_or_else(|_| "webdav".into()),
            password: std::env::var("FM_WEBDAV_REAL_PASS").unwrap_or_else(|_| "webdavpass".into()),
            auth_scheme: WebDavAuthScheme::Basic,
        })
    }
}

fn provider(base_url: &str) -> WebDavFileSystemProvider {
    WebDavFileSystemProvider::new(Arc::new(EnvResolver {
        base_url: base_url.to_owned(),
    }))
}

fn root() -> Location {
    Location::new(
        ProviderId::new("webdav"),
        format!("webdav://{CONNECTION_ID}/"),
    )
}

/// `require_env` returns `None` (and the test skips) when the real server is
/// not configured, so this file never breaks an ordinary `cargo test` run.
fn require_env() -> Option<String> {
    std::env::var("FM_WEBDAV_REAL_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
}

#[tokio::test]
async fn connectivity_and_listing_against_the_real_server() {
    let Some(base_url) = require_env() else {
        eprintln!("skipping: set FM_WEBDAV_REAL_BASE_URL to run against a real WebDAV server");
        return;
    };
    let provider = provider(&base_url);
    let cancellation = CancellationToken::new();

    // Verify connectivity (auth + transport) without retaining state.
    WebDavFileSystemProvider::verify_connectivity(&WebDavConnectionParameters {
        base_url: base_url.clone(),
        username: std::env::var("FM_WEBDAV_REAL_USER").unwrap_or_else(|_| "webdav".into()),
        password: std::env::var("FM_WEBDAV_REAL_PASS").unwrap_or_else(|_| "webdavpass".into()),
        auth_scheme: WebDavAuthScheme::Basic,
    })
    .await
    .expect("real WebDAV server should authenticate and respond to PROPFIND");

    let page = provider
        .list(&root(), ListOptions::default(), cancellation)
        .await
        .expect("PROPFIND depth 1 listing against the real server");
    // The collection lists itself with zero or more children; assert it does not error.
    let _ = page.total_known_entries;
}

async fn read_all(stream: &mut ProviderReadStream) -> Vec<u8> {
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    buf
}

/// Reads a file through the provider, returning `None` if it does not exist.
async fn try_read(provider: &WebDavFileSystemProvider, loc: Location) -> Option<Vec<u8>> {
    let mut reader = provider
        .open_read(
            &EntryRef {
                id: fm_domain::EntryId::new(),
                location: loc,
            },
            CancellationToken::new(),
        )
        .await
        .ok()?;
    Some(read_all(&mut reader).await)
}

/// Waits (bounded) for a just-written file to be visible on the server.
async fn wait_until_exists(provider: &WebDavFileSystemProvider, loc: Location) {
    let deadline = Duration::from_secs(10);
    let start = Instant::now();
    while start.elapsed() < deadline {
        if try_read(provider, loc.clone()).await.is_some() {
            return;
        }
        sleep(Duration::from_millis(50)).await;
    }
    panic!("file never appeared on the server: {loc:?}");
}

/// Removes an entry, tolerating it already being gone (best-effort cleanup).
async fn remove_ok(provider: &WebDavFileSystemProvider, location: Location) {
    let result = provider
        .remove(
            &EntryRef {
                id: fm_domain::EntryId::new(),
                location,
            },
            RemoveOptions::default(),
            CancellationToken::new(),
        )
        .await;
    assert!(result.is_ok() || matches!(result, Err(VfsError::NotFound { .. })));
}

/// A unique per-run working folder under the server root, so repeated runs
/// against the same live server never collide with leftovers from an earlier
/// run (the server is ephemeral but the store can persist across restarts).
fn unique_scope() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("dav-real-{nanos}-{}", std::process::id())
}

#[tokio::test]
async fn full_file_workflow_against_the_real_server() {
    let Some(base_url) = require_env() else {
        eprintln!("skipping: set FM_WEBDAV_REAL_BASE_URL to run against a real WebDAV server");
        return;
    };
    let provider = provider(&base_url);
    let cancellation = CancellationToken::new();
    let scope_name = unique_scope();
    let scope = root().join(&scope_name).unwrap();

    // Create a working folder.
    provider
        .create_directory(&root(), &scope_name, cancellation.clone())
        .await
        .expect("MKCOL against the real server");
    provider
        .list(&scope, ListOptions::default(), cancellation.clone())
        .await
        .expect("listing the newly created directory on the real server");

    // Upload a file inside the working folder.
    let upload = scope.join("upload.txt").unwrap();
    {
        let mut writer = provider
            .open_write(
                &upload,
                WriteOptions { overwrite: true },
                cancellation.clone(),
            )
            .await
            .expect("PUT (open_write) against the real server");
        writer
            .write_all(b"payload-from-fm-vfs-webdav")
            .await
            .unwrap();
        writer.shutdown().await.unwrap();
    }

    // `open_write` dispatches the PUT on a background task; poll until it has
    // landed on the real server before reading it back (same pattern the
    // fixture tests use).
    wait_until_exists(&provider, upload.clone()).await;

    // Download and verify it.
    let mut reader = provider
        .open_read(
            &EntryRef {
                id: fm_domain::EntryId::new(),
                location: upload.clone(),
            },
            cancellation.clone(),
        )
        .await
        .expect("GET (open_read) against the real server");
    let downloaded = read_all(&mut reader).await;
    assert_eq!(downloaded, b"payload-from-fm-vfs-webdav");

    // Native server-side copy (WebDAV COPY) — the real server must duplicate it.
    let copied = scope.join("copied.txt").unwrap();
    let used_native = provider
        .server_side_copy(
            &EntryRef {
                id: fm_domain::EntryId::new(),
                location: upload.clone(),
            },
            &copied,
            cancellation.clone(),
        )
        .await
        .expect("COPY against the real server");
    assert!(
        used_native,
        "real mod_dav advertises and performs native COPY"
    );
    assert_eq!(
        try_read(&provider, copied.clone()).await,
        Some(b"payload-from-fm-vfs-webdav".to_vec())
    );

    // Native rename (WebDAV MOVE) of the copy; the original `upload` is untouched.
    let moved = scope.join("moved.txt").unwrap();
    provider
        .rename(
            &EntryRef {
                id: fm_domain::EntryId::new(),
                location: copied,
            },
            &moved,
            cancellation.clone(),
        )
        .await
        .expect("MOVE against the real server");
    // After the MOVE, the copied path no longer exists.
    assert!(
        try_read(&provider, scope.join("copied.txt").unwrap())
            .await
            .is_none()
    );
    // The original upload still exists and is unchanged.
    assert_eq!(
        try_read(&provider, upload.clone()).await,
        Some(b"payload-from-fm-vfs-webdav".to_vec())
    );

    // Remove everything we created, then the working folder itself.
    remove_ok(&provider, moved).await;
    remove_ok(&provider, upload).await;
    remove_ok(&provider, scope).await;
}
