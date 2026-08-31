//! Wire-level contracts for the WebDAV provider, exercised against the
//! in-process fixture (`fm_vfs_webdav::fixture`) rather than a mocked
//! provider (task 0147).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use async_trait::async_trait;
use fm_domain::{EntryId, Location, ProviderId};
use fm_vfs::{
    CONSERVATIVE_POLL_INTERVAL, ChangeTracking, EntryRef, FileSystemProvider, ListOptions,
    ProviderCapabilities, RemoveOptions, VfsError, WriteOptions,
};
use fm_vfs_webdav::fixture::{FIXTURE_PASSWORD, FIXTURE_USERNAME, FixtureAuth, WebDavFixture};
use fm_vfs_webdav::{
    WebDavAuthScheme, WebDavConnectionParameters, WebDavConnectionResolver,
    WebDavFileSystemProvider,
};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

const CONNECTION_ID: &str = "11111111-1111-4111-8111-111111111111";

struct Resolver {
    base_url: String,
    auth_scheme: WebDavAuthScheme,
}

#[async_trait]
impl WebDavConnectionResolver for Resolver {
    async fn resolve(&self, _id: &str) -> Result<WebDavConnectionParameters, VfsError> {
        Ok(WebDavConnectionParameters {
            base_url: self.base_url.clone(),
            username: FIXTURE_USERNAME.to_owned(),
            password: FIXTURE_PASSWORD.to_owned(),
            auth_scheme: self.auth_scheme,
        })
    }
}

fn provider(base_url: &str, auth_scheme: WebDavAuthScheme) -> WebDavFileSystemProvider {
    WebDavFileSystemProvider::new(Arc::new(Resolver {
        base_url: base_url.to_owned(),
        auth_scheme,
    }))
}

fn root() -> Location {
    Location::parse(&format!("webdav://{CONNECTION_ID}/")).unwrap()
}

#[test]
fn reports_only_implemented_webdav_capabilities() {
    let capabilities = provider("http://127.0.0.1:1/dav", WebDavAuthScheme::Basic).capabilities();
    for supported in [
        ProviderCapabilities::LIST,
        ProviderCapabilities::READ,
        ProviderCapabilities::WRITE,
        ProviderCapabilities::CREATE_DIRECTORY,
        ProviderCapabilities::RENAME,
        ProviderCapabilities::MOVE,
        ProviderCapabilities::SERVER_SIDE_COPY,
        ProviderCapabilities::DELETE,
    ] {
        assert!(capabilities.contains(supported));
    }
    for unsupported in [
        ProviderCapabilities::WATCH,
        ProviderCapabilities::CHECKSUM,
        ProviderCapabilities::SET_TIMESTAMPS,
        ProviderCapabilities::SET_PERMISSIONS,
        ProviderCapabilities::TRASH,
        ProviderCapabilities::RANDOM_ACCESS,
    ] {
        assert!(!capabilities.contains(unsupported));
    }
}

#[test]
fn transfer_capabilities_identify_the_connection_rather_than_the_provider_type() {
    let provider = provider("http://127.0.0.1:1/dav", WebDavAuthScheme::Basic);
    let first = provider
        .transfer_capabilities(
            &Location::parse(&format!("webdav://{CONNECTION_ID}/a.txt")).unwrap(),
        )
        .unwrap();
    let same_connection = provider
        .transfer_capabilities(
            &Location::parse(&format!("webdav://{CONNECTION_ID}/nested/b.txt")).unwrap(),
        )
        .unwrap();
    let other_connection = provider
        .transfer_capabilities(
            &Location::parse("webdav://22222222-2222-4222-8222-222222222222/a.txt").unwrap(),
        )
        .unwrap();

    assert!(first.shares_endpoint_with(&same_connection));
    assert!(!first.shares_endpoint_with(&other_connection));
    // WebDAV has native MOVE/COPY.
    assert!(first.server_side_copy);
    assert!(first.server_side_move);
    // Never probed against this endpoint yet: under-advertised, not assumed.
    assert!(!first.random_read);
    assert!(!first.random_write);
    assert!(!first.resumable_upload);
    assert!(!first.resumable_download);
}

#[test]
fn transfer_capabilities_reject_a_malformed_location() {
    let result = provider("http://127.0.0.1:1/dav", WebDavAuthScheme::Basic).transfer_capabilities(
        &Location::new(ProviderId::new("webdav"), "webdav://not-a-uuid/a.txt"),
    );
    assert!(matches!(result, Err(VfsError::InvalidLocation { .. })));
}

#[test]
fn change_tracking_reports_conservative_polling_rather_than_the_native_watch_default() {
    assert_eq!(
        provider("http://127.0.0.1:1/dav", WebDavAuthScheme::Basic).change_tracking(),
        ChangeTracking::Poll {
            interval: CONSERVATIVE_POLL_INTERVAL
        }
    );
}

#[tokio::test]
async fn cancellation_prevents_a_network_operation() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let result = provider("http://127.0.0.1:1/dav", WebDavAuthScheme::Basic)
        .list(&root(), ListOptions::default(), cancellation)
        .await;
    assert!(matches!(result, Err(VfsError::Cancelled)));
}

async fn run_file_workflow(auth_scheme: WebDavAuthScheme) {
    let fixture_auth = match auth_scheme {
        WebDavAuthScheme::Basic => FixtureAuth::Basic,
        WebDavAuthScheme::Digest => FixtureAuth::Digest,
    };
    let fixture = WebDavFixture::start(fixture_auth).await;
    fixture.put("/hello.txt", b"hello").await;
    fixture.create_directory("/downloads").await;
    fixture.put("/downloads/inner.txt", b"nested").await;

    let provider = provider(&fixture.base_url, auth_scheme);
    let cancellation = CancellationToken::new();

    let listed = provider
        .list(&root(), ListOptions::default(), cancellation.clone())
        .await
        .unwrap();
    assert!(listed.entries.iter().any(|entry| entry.name == "hello.txt"));
    assert!(listed.entries.iter().any(|entry| entry.name == "downloads"));

    let nested = root().join("downloads").unwrap();
    let nested_listing = provider
        .list(&nested, ListOptions::default(), cancellation.clone())
        .await
        .unwrap();
    assert!(
        nested_listing
            .entries
            .iter()
            .any(|entry| entry.name == "inner.txt")
    );

    let upload = root().join("upload.txt").unwrap();
    let mut writer = provider
        .open_write(
            &upload,
            WriteOptions { overwrite: true },
            cancellation.clone(),
        )
        .await
        .unwrap();
    writer.write_all(b"uploaded").await.unwrap();
    writer.shutdown().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while fixture.get("/upload.txt").await.is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fixture.get("/upload.txt").await.unwrap(), b"uploaded");

    let mut reader = provider
        .open_read(
            &EntryRef {
                id: EntryId::new(),
                location: upload.clone(),
            },
            cancellation.clone(),
        )
        .await
        .unwrap();
    let mut downloaded = Vec::new();
    reader.read_to_end(&mut downloaded).await.unwrap();
    assert_eq!(downloaded, b"uploaded");

    let moved = root().join("moved.txt").unwrap();
    provider
        .rename(
            &EntryRef {
                id: EntryId::new(),
                location: upload,
            },
            &moved,
            cancellation.clone(),
        )
        .await
        .unwrap();
    assert!(fixture.get("/upload.txt").await.is_none());
    assert_eq!(fixture.get("/moved.txt").await.unwrap(), b"uploaded");

    provider
        .remove(
            &EntryRef {
                id: EntryId::new(),
                location: moved,
            },
            RemoveOptions::default(),
            cancellation,
        )
        .await
        .unwrap();
    assert!(fixture.get("/moved.txt").await.is_none());
}

#[tokio::test]
async fn basic_auth_fixture_supports_the_file_workflow() {
    run_file_workflow(WebDavAuthScheme::Basic).await;
}

#[tokio::test]
async fn digest_auth_fixture_supports_the_file_workflow() {
    run_file_workflow(WebDavAuthScheme::Digest).await;
}

#[tokio::test]
async fn server_side_copy_uses_the_native_copy_method() {
    let fixture = WebDavFixture::start(FixtureAuth::Basic).await;
    fixture.put("/original.txt", b"payload").await;
    let provider = provider(&fixture.base_url, WebDavAuthScheme::Basic);
    let source = EntryRef {
        id: EntryId::new(),
        location: root().join("original.txt").unwrap(),
    };
    let temporary = root().join(".fm-copy-test").unwrap();

    let used_native = provider
        .server_side_copy(&source, &temporary, CancellationToken::new())
        .await
        .unwrap();

    assert!(used_native);
    assert_eq!(
        fixture.get("/.fm-copy-test").await.unwrap(),
        b"payload".to_vec()
    );
    // The original must still exist: COPY, unlike MOVE, does not remove it.
    assert_eq!(fixture.get("/original.txt").await.unwrap(), b"payload");
}

#[tokio::test]
async fn a_locked_resource_reports_a_conflict_rather_than_a_generic_failure() {
    let fixture = WebDavFixture::start(FixtureAuth::Basic).await;
    fixture.put("/locked.txt", b"payload").await;
    fixture.lock("/locked.txt").await;
    let provider = provider(&fixture.base_url, WebDavAuthScheme::Basic);

    let result = provider
        .remove(
            &EntryRef {
                id: EntryId::new(),
                location: root().join("locked.txt").unwrap(),
            },
            RemoveOptions::default(),
            CancellationToken::new(),
        )
        .await;

    assert!(matches!(result, Err(VfsError::Locked { .. })));
}

#[tokio::test]
async fn https_rejects_an_untrusted_server_certificate() {
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let fixture = WebDavFixture::start_tls(
        certificate.cert.der().to_vec(),
        certificate.signing_key.serialize_der(),
    )
    .await;

    let parameters = WebDavConnectionParameters {
        base_url: format!("https://localhost:{}/dav", fixture.addr.port()),
        username: FIXTURE_USERNAME.to_owned(),
        password: FIXTURE_PASSWORD.to_owned(),
        auth_scheme: WebDavAuthScheme::Basic,
    };

    let result = WebDavFileSystemProvider::verify_connectivity(&parameters).await;

    assert!(matches!(result, Err(VfsError::Io { .. })));
}

#[tokio::test]
async fn wrong_credentials_report_permission_denied() {
    let fixture = WebDavFixture::start(FixtureAuth::Basic).await;
    let parameters = WebDavConnectionParameters {
        base_url: fixture.base_url.clone(),
        username: FIXTURE_USERNAME.to_owned(),
        password: "wrong".to_owned(),
        auth_scheme: WebDavAuthScheme::Basic,
    };
    let result = WebDavFileSystemProvider::verify_connectivity(&parameters).await;
    assert!(matches!(result, Err(VfsError::PermissionDenied { .. })));
}
