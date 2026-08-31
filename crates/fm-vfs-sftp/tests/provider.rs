//! Integration tests for [`fm_vfs_sftp::SftpFileSystemProvider`] against the
//! real in-process `fm-ssh` fixture (task 0104, spec §18 "SFTP": "list,
//! upload, download, rename, mkdir, delete, cancellation, ... Unicode
//! paths").

use std::sync::Arc;

use async_trait::async_trait;
use fm_domain::{EntryKind, Location, ProviderId};
use fm_ssh::fixture::{FIXTURE_PASSWORD, FIXTURE_USERNAME, SshFixture};
use fm_ssh::{
    InMemoryKnownHostsStore, KnownHostsStore, SshConnectTarget, SshConnectionManager,
    SshConnectionParameters, SshCredential, SshHostKeyPolicy,
};
use fm_vfs::{
    CONSERVATIVE_POLL_INTERVAL, ChangeTracking, EntryRef, FileSystemProvider, ListOptions,
    ProviderCapabilities, RemoveOptions, VfsError, WriteOptions,
};
use fm_vfs_sftp::{SftpFileSystemProvider, SshConnectionResolver};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const CONNECTION_ID: &str = "11111111-1111-4111-8111-111111111111";

struct FixtureResolver {
    fixture_addr: std::net::SocketAddr,
}

#[async_trait]
impl SshConnectionResolver for FixtureResolver {
    async fn resolve(&self, connection_id: &str) -> Result<SshConnectionParameters, VfsError> {
        assert_eq!(connection_id, CONNECTION_ID);
        Ok(SshConnectionParameters {
            target: SshConnectTarget {
                host: self.fixture_addr.ip().to_string(),
                port: self.fixture_addr.port(),
                username: FIXTURE_USERNAME.to_owned(),
            },
            credential: SshCredential::Password(FIXTURE_PASSWORD.to_owned().into()),
            host_key_policy: SshHostKeyPolicy::PromptOnFirstUse,
            keepalive: None,
        })
    }
}

/// Starts a fixture, pre-trusts its host key, and returns a provider wired
/// to browse it under [`CONNECTION_ID`], plus the fixture itself (which
/// callers use as the "remote root" for building test paths).
async fn provider_and_fixture() -> (SftpFileSystemProvider, SshFixture) {
    let (provider, fixture, _connections) = provider_fixture_and_connections().await;
    (provider, fixture)
}

/// Same as [`provider_and_fixture`], but also returns the
/// [`SshConnectionManager`] the provider was built with, for tests that need
/// to directly manipulate the session cache (e.g. simulating a dropped
/// connection).
async fn provider_fixture_and_connections() -> (
    SftpFileSystemProvider,
    SshFixture,
    Arc<SshConnectionManager>,
) {
    let fixture = SshFixture::start().await;
    let known_hosts = Arc::new(InMemoryKnownHostsStore::new());
    known_hosts
        .accept(CONNECTION_ID, fixture.host_key_fingerprint.clone())
        .await
        .expect("seeding the trusted fingerprint must succeed");
    let connections = Arc::new(SshConnectionManager::new(known_hosts));
    let resolver = Arc::new(FixtureResolver {
        fixture_addr: fixture.addr,
    });
    let provider = SftpFileSystemProvider::new(connections.clone(), resolver);
    (provider, fixture, connections)
}

/// Builds an `sftp://` location from a root-relative Unix-style path. The
/// fixture's wire protocol is always Unix-style, independent of the host
/// OS - see `fm_ssh::fixture`'s module doc for why this must not be built
/// from `fixture.path()`'s native OS path.
fn location(_fixture: &SshFixture, relative: &str) -> Location {
    let uri = format!("sftp://{CONNECTION_ID}/{relative}");
    Location::parse(&uri).expect("test location must parse")
}

fn root_location(_fixture: &SshFixture) -> Location {
    let uri = format!("sftp://{CONNECTION_ID}/");
    Location::parse(&uri).expect("test root location must parse")
}

fn entry_ref(location: Location) -> EntryRef {
    EntryRef {
        id: fm_domain::EntryId::from(Uuid::new_v4()),
        location,
    }
}

fn cancellation() -> CancellationToken {
    CancellationToken::new()
}

/// Like `Result::expect_err`, but does not require the `Ok` type to
/// implement `Debug` - several provider methods return a boxed
/// `AsyncRead`/`AsyncWrite`/stream trait object on success, which cannot.
fn expect_err<T, E>(result: Result<T, E>, message: &str) -> E {
    match result {
        Ok(_) => panic!("{message}"),
        Err(error) => error,
    }
}

#[tokio::test]
async fn id_and_capabilities_are_reported_accurately() {
    let (provider, _fixture) = provider_and_fixture().await;
    assert_eq!(provider.id(), ProviderId::new("sftp"));

    let capabilities = provider.capabilities();
    for expected in [
        ProviderCapabilities::LIST,
        ProviderCapabilities::READ,
        ProviderCapabilities::WRITE,
        ProviderCapabilities::CREATE_DIRECTORY,
        ProviderCapabilities::RENAME,
        ProviderCapabilities::MOVE,
        ProviderCapabilities::DELETE,
    ] {
        assert!(capabilities.contains(expected), "missing {expected:?}");
    }
    for unexpected in [
        ProviderCapabilities::SERVER_SIDE_COPY,
        ProviderCapabilities::TRASH,
        ProviderCapabilities::WATCH,
        ProviderCapabilities::RANDOM_ACCESS,
    ] {
        assert!(
            !capabilities.contains(unexpected),
            "unexpectedly advertised {unexpected:?}"
        );
    }
}

/// Task 0108: the endpoint must identify the *connection*, not the provider
/// type, so that a copy between two different SFTP hosts never takes a
/// same-backend fast path.
#[tokio::test]
async fn transfer_capabilities_identify_the_connection_rather_than_the_provider_type() {
    let (provider, fixture) = provider_and_fixture().await;

    let here = provider
        .transfer_capabilities(&location(&fixture, "a.txt"))
        .expect("transfer capabilities must resolve");
    let same_connection = provider
        .transfer_capabilities(&location(&fixture, "nested/b.txt"))
        .expect("transfer capabilities must resolve");
    let other_connection = provider
        .transfer_capabilities(
            &Location::parse("sftp://22222222-2222-4222-8222-222222222222/a.txt")
                .expect("test location must parse"),
        )
        .expect("transfer capabilities must resolve");

    assert!(here.shares_endpoint_with(&same_connection));
    assert!(!here.shares_endpoint_with(&other_connection));
    // SFTPv3 has no portable server-side clone, but `rename` within one
    // connection is a genuine server-side move.
    assert!(!here.server_side_copy);
    assert!(here.server_side_move);
    // Honestly under-advertised: this provider implements neither offset
    // reads/writes nor resumable transfers (see the module documentation).
    assert!(!here.random_read);
    assert!(!here.random_write);
    assert!(!here.resumable_upload);
    assert!(!here.resumable_download);
}

#[tokio::test]
async fn transfer_capabilities_reject_a_location_belonging_to_another_provider() {
    let (provider, _fixture) = provider_and_fixture().await;

    let result = provider
        .transfer_capabilities(&Location::new(ProviderId::new("file"), "file:///tmp/a.txt"));

    assert!(matches!(result, Err(VfsError::InvalidLocation { .. })));
}

#[tokio::test]
async fn change_tracking_reports_conservative_polling_rather_than_the_native_watch_default() {
    let (provider, _fixture) = provider_and_fixture().await;

    assert_eq!(
        provider.change_tracking(),
        ChangeTracking::Poll {
            interval: CONSERVATIVE_POLL_INTERVAL
        }
    );
}

#[tokio::test]
async fn watch_reports_unsupported_rather_than_a_default_no_op() {
    let (provider, fixture) = provider_and_fixture().await;
    let error = expect_err(
        provider
            .watch(&root_location(&fixture), cancellation())
            .await,
        "watch must be unsupported",
    );
    assert!(matches!(
        error,
        VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::WATCH
        }
    ));
}

#[tokio::test]
async fn mkdir_list_and_metadata_round_trip() {
    let (provider, fixture) = provider_and_fixture().await;
    let root = root_location(&fixture);

    let created = provider
        .create_directory(&root, "docs", cancellation())
        .await
        .expect("mkdir must succeed");
    assert!(fixture.path("docs").is_dir());

    let page = provider
        .list(&root, ListOptions::default(), cancellation())
        .await
        .expect("list must succeed");
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "docs");
    assert_eq!(page.entries[0].kind, EntryKind::Directory);
    assert!(!page.has_more);

    let metadata = provider
        .metadata(&created, cancellation())
        .await
        .expect("metadata must succeed");
    assert_eq!(metadata.entry_id, created.id);
    assert!(metadata.permissions.expect("permissions").readable);
}

#[tokio::test]
async fn list_paginates_across_multiple_pages() {
    // Note: each `list()` call re-lists the whole remote directory fresh (no
    // server-side cursor survives between calls - SFTPv3's own stable
    // cursor, a single `opendir` handle's sequential `readdir`s, cannot
    // outlive one `list()` call the way `ListOptions::continuation_token`
    // requires). Real filesystems/SFTP servers do not guarantee enumeration
    // order is identical across two independent listings of the same
    // directory, so this test only asserts the paging *mechanics*
    // (`has_more`/`continuation_token`/`total_known_entries`), not that a
    // specific item lands on a specific page - documented as a known
    // limitation in this task's Agent Notes.
    let (provider, fixture) = provider_and_fixture().await;
    let root = root_location(&fixture);
    for index in 0..5 {
        std::fs::write(fixture.path(&format!("file-{index}.txt")), b"x").unwrap();
    }

    let first_page = provider
        .list(
            &root,
            ListOptions {
                page_size: 2,
                continuation_token: None,
            },
            cancellation(),
        )
        .await
        .expect("first page must succeed");
    assert_eq!(first_page.entries.len(), 2);
    assert!(first_page.has_more);
    assert_eq!(first_page.total_known_entries, Some(5));
    assert!(first_page.continuation_token.is_some());

    let full_page = provider
        .list(
            &root,
            ListOptions {
                page_size: 10,
                continuation_token: None,
            },
            cancellation(),
        )
        .await
        .expect("a page large enough for everything must succeed");
    assert!(!full_page.has_more);
    assert_eq!(full_page.continuation_token, None);
    let mut names: Vec<String> = full_page
        .entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "file-0.txt",
            "file-1.txt",
            "file-2.txt",
            "file-3.txt",
            "file-4.txt"
        ]
    );
}

#[tokio::test]
async fn upload_and_download_round_trip_real_bytes() {
    let (provider, fixture) = provider_and_fixture().await;
    let destination = location(&fixture, "uploaded.bin");
    let payload = b"the quick brown fox jumps over the lazy dog".to_vec();

    let mut writer = provider
        .open_write(
            &destination,
            WriteOptions { overwrite: false },
            cancellation(),
        )
        .await
        .expect("open_write must succeed");
    writer
        .write_all(&payload)
        .await
        .expect("write must succeed");
    writer.shutdown().await.expect("shutdown must succeed");

    // shutdown() already drains every pending SFTP write ack and the close ack
    // before returning, so the server has processed the write - but under a
    // loaded CI runner the written bytes can take a moment to become visible
    // to a bystander `std::fs::read` on this same file. Poll briefly rather
    // than assuming the very first read observes them.
    let mut on_disk = Vec::new();
    for _ in 0..100 {
        on_disk = std::fs::read(fixture.path("uploaded.bin")).unwrap();
        if on_disk == payload {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(on_disk, payload);

    let mut reader = provider
        .open_read(&entry_ref(destination), cancellation())
        .await
        .expect("open_read must succeed");
    let mut downloaded = Vec::new();
    reader
        .read_to_end(&mut downloaded)
        .await
        .expect("read must succeed");
    assert_eq!(downloaded, payload);
}

#[tokio::test]
async fn open_write_without_overwrite_refuses_an_existing_file() {
    let (provider, fixture) = provider_and_fixture().await;
    std::fs::write(fixture.path("exists.txt"), b"original").unwrap();
    let destination = location(&fixture, "exists.txt");

    let error = expect_err(
        provider
            .open_write(
                &destination,
                WriteOptions { overwrite: false },
                cancellation(),
            )
            .await,
        "must refuse to silently overwrite",
    );
    assert!(matches!(
        error,
        VfsError::AlreadyExists { .. } | VfsError::Io { .. }
    ));
    assert_eq!(
        std::fs::read(fixture.path("exists.txt")).unwrap(),
        b"original"
    );
}

#[tokio::test]
async fn rename_moves_a_file_within_the_same_connection() {
    let (provider, fixture) = provider_and_fixture().await;
    std::fs::write(fixture.path("old-name.txt"), b"content").unwrap();
    let source = entry_ref(location(&fixture, "old-name.txt"));
    let destination = location(&fixture, "new-name.txt");

    let renamed = provider
        .rename(&source, &destination, cancellation())
        .await
        .expect("rename must succeed");
    assert_eq!(renamed.location, destination);
    assert!(!fixture.path("old-name.txt").exists());
    assert_eq!(
        std::fs::read(fixture.path("new-name.txt")).unwrap(),
        b"content"
    );
}

#[tokio::test]
async fn remove_deletes_a_file() {
    let (provider, fixture) = provider_and_fixture().await;
    std::fs::write(fixture.path("doomed.txt"), b"x").unwrap();
    let entry = entry_ref(location(&fixture, "doomed.txt"));

    provider
        .remove(&entry, RemoveOptions::default(), cancellation())
        .await
        .expect("remove must succeed");
    assert!(!fixture.path("doomed.txt").exists());
}

#[tokio::test]
async fn remove_recursive_deletes_a_populated_directory_tree() {
    let (provider, fixture) = provider_and_fixture().await;
    std::fs::create_dir_all(fixture.path("tree/nested")).unwrap();
    std::fs::write(fixture.path("tree/file.txt"), b"x").unwrap();
    std::fs::write(fixture.path("tree/nested/inner.txt"), b"y").unwrap();
    let entry = entry_ref(location(&fixture, "tree"));

    provider
        .remove(
            &entry,
            RemoveOptions {
                recursive: true,
                use_trash: false,
            },
            cancellation(),
        )
        .await
        .expect("recursive remove must succeed");
    assert!(!fixture.path("tree").exists());
}

#[tokio::test]
async fn remove_non_recursive_on_a_non_empty_directory_fails_without_deleting_it() {
    let (provider, fixture) = provider_and_fixture().await;
    std::fs::create_dir_all(fixture.path("populated")).unwrap();
    std::fs::write(fixture.path("populated/child.txt"), b"x").unwrap();
    let entry = entry_ref(location(&fixture, "populated"));

    let result = provider
        .remove(&entry, RemoveOptions::default(), cancellation())
        .await;
    assert!(result.is_err());
    assert!(fixture.path("populated").exists());
    assert!(fixture.path("populated/child.txt").exists());
}

#[tokio::test]
async fn trash_removal_reports_unsupported() {
    let (provider, fixture) = provider_and_fixture().await;
    std::fs::write(fixture.path("trashme.txt"), b"x").unwrap();
    let entry = entry_ref(location(&fixture, "trashme.txt"));

    let error = provider
        .remove(
            &entry,
            RemoveOptions {
                recursive: false,
                use_trash: true,
            },
            cancellation(),
        )
        .await
        .expect_err("trash must be unsupported");
    assert!(matches!(
        error,
        VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::TRASH
        }
    ));
}

#[tokio::test]
async fn unicode_file_and_directory_names_round_trip() {
    let (provider, fixture) = provider_and_fixture().await;
    let root = root_location(&fixture);

    let directory = provider
        .create_directory(&root, "café \u{1F600}", cancellation())
        .await
        .expect("mkdir with a unicode name must succeed");
    assert_eq!(directory.location.name().unwrap(), "café \u{1F600}");

    let file_destination = directory.location.join("naïve résumé.txt").unwrap();
    let mut writer = provider
        .open_write(
            &file_destination,
            WriteOptions { overwrite: false },
            cancellation(),
        )
        .await
        .expect("open_write with a unicode name must succeed");
    writer.write_all(b"unicode payload").await.unwrap();
    writer.shutdown().await.unwrap();

    let page = provider
        .list(&directory.location, ListOptions::default(), cancellation())
        .await
        .expect("listing a unicode directory must succeed");
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "naïve résumé.txt");

    let mut reader = provider
        .open_read(&entry_ref(file_destination), cancellation())
        .await
        .expect("reading a unicode-named file must succeed");
    let mut downloaded = Vec::new();
    reader.read_to_end(&mut downloaded).await.unwrap();
    assert_eq!(downloaded, b"unicode payload");
}

#[tokio::test]
async fn same_filesystem_is_true_within_one_connection_and_false_across_connections() {
    let (provider, fixture) = provider_and_fixture().await;
    let source = entry_ref(location(&fixture, "a.txt"));
    let same_connection_dir = root_location(&fixture);
    assert!(
        provider
            .same_filesystem(&source, &same_connection_dir, cancellation())
            .await
            .expect("same_filesystem must succeed")
    );

    let other_connection =
        Location::parse("sftp://22222222-2222-4222-8222-222222222222/tmp").unwrap();
    assert!(
        !provider
            .same_filesystem(&source, &other_connection, cancellation())
            .await
            .expect("same_filesystem across connections must succeed")
    );
}

#[tokio::test]
async fn commit_copy_publishes_a_remote_temporary_file_without_local_staging() {
    let (provider, fixture) = provider_and_fixture().await;
    let temporary = location(&fixture, ".fm-copy-test");
    let destination = location(&fixture, "published.txt");

    let mut writer = provider
        .open_write(
            &temporary,
            WriteOptions { overwrite: false },
            cancellation(),
        )
        .await
        .expect("open_write on the temporary must succeed");
    writer.write_all(b"staged remotely").await.unwrap();
    writer.shutdown().await.unwrap();
    // The temporary file is a REAL file on the fixture's remote root - not a
    // local file anywhere in this test process's own temp directory.
    assert!(fixture.path(".fm-copy-test").exists());

    let published = provider
        .commit_copy(
            &entry_ref(temporary.clone()),
            &temporary,
            &destination,
            fm_vfs::CopyCommitOptions::default(),
            cancellation(),
        )
        .await
        .expect("commit_copy must succeed");
    assert_eq!(published.location, destination);
    assert!(!fixture.path(".fm-copy-test").exists());
    assert_eq!(
        std::fs::read(fixture.path("published.txt")).unwrap(),
        b"staged remotely"
    );
}

#[tokio::test]
async fn commit_copy_without_overwrite_refuses_an_existing_destination() {
    let (provider, fixture) = provider_and_fixture().await;
    std::fs::write(fixture.path("final.txt"), b"already here").unwrap();
    let temporary = location(&fixture, ".fm-copy-conflict");
    std::fs::write(fixture.path(".fm-copy-conflict"), b"new content").unwrap();
    let destination = location(&fixture, "final.txt");

    let error = provider
        .commit_copy(
            &entry_ref(temporary.clone()),
            &temporary,
            &destination,
            fm_vfs::CopyCommitOptions::default(),
            cancellation(),
        )
        .await
        .expect_err("must refuse to overwrite");
    assert!(matches!(error, VfsError::AlreadyExists { .. }));
    assert_eq!(
        std::fs::read(fixture.path("final.txt")).unwrap(),
        b"already here"
    );
}

#[tokio::test]
async fn discard_copy_removes_the_temporary_and_tolerates_it_already_being_gone() {
    let (provider, fixture) = provider_and_fixture().await;
    let temporary = location(&fixture, ".fm-copy-abandoned");
    std::fs::write(fixture.path(".fm-copy-abandoned"), b"partial").unwrap();

    provider
        .discard_copy(&temporary, cancellation())
        .await
        .expect("discard_copy must succeed");
    assert!(!fixture.path(".fm-copy-abandoned").exists());

    // Calling it again (already gone) must not error.
    provider
        .discard_copy(&temporary, cancellation())
        .await
        .expect("discard_copy must tolerate an already-missing temporary");
}

#[tokio::test]
async fn cancellation_before_any_io_reports_cancelled_without_creating_anything() {
    let (provider, fixture) = provider_and_fixture().await;
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let destination = location(&fixture, "never-created.txt");

    let error = expect_err(
        provider
            .open_write(
                &destination,
                WriteOptions { overwrite: false },
                cancellation,
            )
            .await,
        "a pre-cancelled call must be rejected",
    );
    assert!(matches!(error, VfsError::Cancelled));
    assert!(!fixture.path("never-created.txt").exists());
}

#[tokio::test]
async fn a_dropped_session_reconnects_transparently_for_a_subsequent_operation() {
    let (provider, fixture, connections) = provider_fixture_and_connections().await;
    let root = root_location(&fixture);

    provider
        .list(&root, ListOptions::default(), cancellation())
        .await
        .expect("first list must succeed");

    // Simulate "the session died" the way a real dropped connection would
    // surface to the manager: invalidate the cached session directly. The
    // very next call must transparently redial and still succeed, rather
    // than requiring the caller to explicitly reconnect first (spec §6.8
    // "reconnect for browsing").
    connections.invalidate(CONNECTION_ID).await;

    provider
        .list(&root, ListOptions::default(), cancellation())
        .await
        .expect("a subsequent list must transparently reconnect and succeed");
}
