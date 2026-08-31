//! Contract tests for [`fm_vfs_s3::S3FileSystemProvider`], run against the
//! in-process mock endpoint in [`fm_vfs_s3::fixture`] rather than real AWS
//! credentials (task 0146's acceptance criteria).

use std::sync::Arc;

use async_trait::async_trait;
use fm_domain::{Location, ProviderId};
use fm_vfs::{
    FileSystemProvider, ListOptions, ProviderCapabilities, RemoveOptions, VfsError, WriteOptions,
};
use fm_vfs_s3::fixture::{
    FIXTURE_ACCESS_KEY_ID, FIXTURE_BUCKET, FIXTURE_REGION, FIXTURE_SECRET_ACCESS_KEY, S3Fixture,
};
use fm_vfs_s3::{S3ConnectionParameters, S3ConnectionResolver, S3FileSystemProvider};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

const CONNECTION_ID: &str = "11111111-1111-4111-8111-111111111111";
const OTHER_CONNECTION_ID: &str = "22222222-2222-4222-8222-222222222222";

struct FixedResolver {
    endpoint: String,
}

#[async_trait]
impl S3ConnectionResolver for FixedResolver {
    async fn resolve(&self, _connection_id: &str) -> Result<S3ConnectionParameters, VfsError> {
        Ok(S3ConnectionParameters {
            endpoint: Some(self.endpoint.clone()),
            region: FIXTURE_REGION.to_owned(),
            bucket: FIXTURE_BUCKET.to_owned(),
            access_key_id: FIXTURE_ACCESS_KEY_ID.to_owned(),
            secret_access_key: FIXTURE_SECRET_ACCESS_KEY.to_owned(),
        })
    }
}

async fn provider() -> (S3Fixture, S3FileSystemProvider) {
    let fixture = S3Fixture::start().await;
    let resolver = Arc::new(FixedResolver {
        endpoint: fixture.endpoint.clone(),
    });
    (fixture, S3FileSystemProvider::new(resolver))
}

fn root() -> Location {
    Location::parse(&format!("s3://{CONNECTION_ID}/")).expect("valid s3 root")
}

/// Uploads `contents`, then waits for the upload to become visible.
///
/// [`S3FileSystemProvider::open_write`] streams into a `tokio::io::duplex`
/// consumed by a background task (mirroring `fm-vfs-ftp`'s own
/// `open_write`); `AsyncWrite::shutdown` only closes the local pipe, it does
/// not wait for that task's upload to finish. `fm-vfs-ftp`'s own contract
/// tests poll the fixture the same way after `shutdown` for the same reason.
async fn write_all(
    provider: &S3FileSystemProvider,
    location: &Location,
    contents: &[u8],
    overwrite: bool,
) {
    let mut writer = provider
        .open_write(
            location,
            WriteOptions { overwrite },
            CancellationToken::new(),
        )
        .await
        .expect("open_write must succeed");
    writer
        .write_all(contents)
        .await
        .expect("write must succeed");
    writer.shutdown().await.expect("shutdown must succeed");

    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            match provider
                .open_read(&entry(location.clone()), CancellationToken::new())
                .await
            {
                Ok(_) => break,
                Err(e) if std::env::var_os("FM_S3_DEBUG").is_some() => {
                    eprintln!("DEBUG open_read retry error: {e}");
                }
                Err(_) => {}
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("upload must become visible within the timeout");
}

async fn read_all(provider: &S3FileSystemProvider, entry: &fm_vfs::EntryRef) -> Vec<u8> {
    let mut reader = provider
        .open_read(entry, CancellationToken::new())
        .await
        .expect("open_read must succeed");
    let mut buffer = Vec::new();
    reader
        .read_to_end(&mut buffer)
        .await
        .expect("read must succeed");
    buffer
}

fn entry(location: Location) -> fm_vfs::EntryRef {
    fm_vfs::EntryRef {
        id: fm_domain::EntryId::new(),
        location,
    }
}

#[tokio::test]
async fn provider_id_and_capabilities_report_no_directories_and_no_native_rename() {
    let (_fixture, provider) = provider().await;
    assert_eq!(provider.id(), ProviderId::new("s3"));
    let capabilities = provider.capabilities();
    assert!(capabilities.contains(ProviderCapabilities::LIST));
    assert!(capabilities.contains(ProviderCapabilities::READ));
    assert!(capabilities.contains(ProviderCapabilities::WRITE));
    assert!(capabilities.contains(ProviderCapabilities::DELETE));
    assert!(capabilities.contains(ProviderCapabilities::SERVER_SIDE_COPY));
    assert!(capabilities.contains(ProviderCapabilities::RANDOM_ACCESS));
    assert!(!capabilities.contains(ProviderCapabilities::WATCH));
    assert!(!capabilities.contains(ProviderCapabilities::TRASH));
}

#[tokio::test]
async fn transfer_capabilities_report_no_native_move_but_native_copy_and_ranged_reads() {
    let (_fixture, provider) = provider().await;
    let capabilities = provider
        .transfer_capabilities(&root())
        .expect("transfer_capabilities must succeed");
    assert!(!capabilities.server_side_move);
    assert!(capabilities.server_side_copy);
    assert!(capabilities.random_read);
    assert!(!capabilities.random_write);
}

#[tokio::test]
async fn transfer_capabilities_differ_between_distinct_connections() {
    let (_fixture, provider) = provider().await;
    let a = provider.transfer_capabilities(&root()).unwrap();
    let other_root = Location::parse(&format!("s3://{OTHER_CONNECTION_ID}/")).unwrap();
    let b = provider.transfer_capabilities(&other_root).unwrap();
    assert!(!a.shares_endpoint_with(&b));
}

#[tokio::test]
async fn upload_then_list_then_download_round_trips_bytes() {
    let (_fixture, provider) = provider().await;
    let file = root().join("hello.txt").unwrap();
    write_all(&provider, &file, b"hello world", false).await;

    let page = provider
        .list(&root(), ListOptions::default(), CancellationToken::new())
        .await
        .expect("list must succeed");
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "hello.txt");
    assert_eq!(page.entries[0].size, Some(11));

    let downloaded = read_all(&provider, &entry(file)).await;
    assert_eq!(downloaded, b"hello world");
}

#[tokio::test]
async fn create_directory_writes_a_zero_byte_marker_and_lists_as_a_prefix() {
    let (_fixture, provider) = provider().await;
    provider
        .create_directory(&root(), "photos", CancellationToken::new())
        .await
        .expect("create_directory must succeed");

    let page = provider
        .list(&root(), ListOptions::default(), CancellationToken::new())
        .await
        .expect("list must succeed");
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "photos");
    assert_eq!(page.entries[0].kind, fm_domain::EntryKind::Directory);
}

#[tokio::test]
async fn nested_uploads_are_grouped_under_their_directory_prefix() {
    let (_fixture, provider) = provider().await;
    let nested = root().join("photos").unwrap().join("2026.jpg").unwrap();
    write_all(&provider, &nested, b"jpeg-bytes", false).await;

    let page = provider
        .list(&root(), ListOptions::default(), CancellationToken::new())
        .await
        .expect("list must succeed");
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "photos");
    assert_eq!(page.entries[0].kind, fm_domain::EntryKind::Directory);

    let inner = provider
        .list(
            &root().join("photos").unwrap(),
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("list must succeed");
    assert_eq!(inner.entries.len(), 1);
    assert_eq!(inner.entries[0].name, "2026.jpg");
}

#[tokio::test]
async fn upload_without_overwrite_rejects_an_existing_destination() {
    let (_fixture, provider) = provider().await;
    let file = root().join("hello.txt").unwrap();
    write_all(&provider, &file, b"first", false).await;

    let result = provider
        .open_write(
            &file,
            WriteOptions { overwrite: false },
            CancellationToken::new(),
        )
        .await;
    assert!(matches!(result, Err(VfsError::AlreadyExists { .. })));
}

#[tokio::test]
async fn upload_with_overwrite_replaces_an_existing_destination() {
    let (_fixture, provider) = provider().await;
    let file = root().join("hello.txt").unwrap();
    write_all(&provider, &file, b"first", false).await;
    write_all(&provider, &file, b"second", true).await;

    let downloaded = read_all(&provider, &entry(file)).await;
    assert_eq!(downloaded, b"second");
}

#[tokio::test]
async fn read_range_returns_only_the_requested_bytes() {
    let (_fixture, provider) = provider().await;
    let file = root().join("data.bin").unwrap();
    write_all(&provider, &file, b"0123456789", false).await;

    let mut reader = provider
        .read_range(&entry(file), 2, Some(3), CancellationToken::new())
        .await
        .expect("read_range must succeed");
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer).await.unwrap();
    assert_eq!(buffer, b"234");
}

#[tokio::test]
async fn rename_moves_bytes_via_copy_then_delete() {
    let (_fixture, provider) = provider().await;
    let source = root().join("old.txt").unwrap();
    let destination = root().join("new.txt").unwrap();
    write_all(&provider, &source, b"payload", false).await;

    provider
        .rename(
            &entry(source.clone()),
            &destination,
            CancellationToken::new(),
        )
        .await
        .expect("rename must succeed");

    let downloaded = read_all(&provider, &entry(destination)).await;
    assert_eq!(downloaded, b"payload");

    let page = provider
        .list(&root(), ListOptions::default(), CancellationToken::new())
        .await
        .expect("list must succeed");
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "new.txt");
}

#[tokio::test]
async fn remove_deletes_an_uploaded_file() {
    let (_fixture, provider) = provider().await;
    let file = root().join("throwaway.txt").unwrap();
    write_all(&provider, &file, b"gone soon", false).await;

    provider
        .remove(
            &entry(file),
            RemoveOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("remove must succeed");

    let page = provider
        .list(&root(), ListOptions::default(), CancellationToken::new())
        .await
        .expect("list must succeed");
    assert!(page.entries.is_empty());
}

#[tokio::test]
async fn remove_use_trash_is_rejected_as_unsupported() {
    let (_fixture, provider) = provider().await;
    let file = root().join("throwaway.txt").unwrap();
    write_all(&provider, &file, b"gone soon", false).await;

    let result = provider
        .remove(
            &entry(file),
            RemoveOptions {
                recursive: false,
                use_trash: true,
            },
            CancellationToken::new(),
        )
        .await;
    assert!(matches!(
        result,
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::DELETE
        })
    ));
}

#[tokio::test]
async fn recursive_remove_deletes_every_object_under_a_prefix() {
    let (_fixture, provider) = provider().await;
    let dir = root().join("photos").unwrap();
    write_all(&provider, &dir.join("a.jpg").unwrap(), b"a", false).await;
    write_all(&provider, &dir.join("b.jpg").unwrap(), b"b", false).await;

    provider
        .remove(
            &entry(dir),
            RemoveOptions {
                recursive: true,
                use_trash: false,
            },
            CancellationToken::new(),
        )
        .await
        .expect("recursive remove must succeed");

    let page = provider
        .list(&root(), ListOptions::default(), CancellationToken::new())
        .await
        .expect("list must succeed");
    assert!(page.entries.is_empty());
}

#[tokio::test]
async fn large_upload_above_the_multipart_threshold_round_trips_intact() {
    let fixture = S3Fixture::start().await;
    let resolver = Arc::new(FixedResolver {
        endpoint: fixture.endpoint.clone(),
    });
    // A threshold below `DEFAULT_MULTIPART_THRESHOLD` forces the multipart
    // path without an enormous test payload, but it's still clamped up to
    // `MINIMUM_MULTIPART_PART_SIZE` (5 MiB) - a real S3-compatible endpoint
    // rejects a smaller non-final part with `EntityTooSmall` (caught by the
    // real-endpoint smoke test below), so the payload must exceed 5 MiB too
    // for the multipart path to actually trigger.
    let provider = S3FileSystemProvider::with_multipart_threshold(resolver, 1);

    let file = root().join("large.bin").unwrap();
    let contents: Vec<u8> = (0_u8..=255).cycle().take(6 * 1024 * 1024).collect();
    write_all(&provider, &file, &contents, false).await;

    let downloaded = read_all(&provider, &entry(file)).await;
    assert_eq!(downloaded, contents);
}

#[tokio::test]
async fn same_filesystem_is_true_only_within_one_connection() {
    let (_fixture, provider) = provider().await;
    let source = entry(root().join("a.txt").unwrap());
    let same_connection_dir = root();
    let other_connection_dir = Location::parse(&format!("s3://{OTHER_CONNECTION_ID}/")).unwrap();

    assert!(
        provider
            .same_filesystem(&source, &same_connection_dir, CancellationToken::new())
            .await
            .unwrap()
    );
    assert!(
        !provider
            .same_filesystem(&source, &other_connection_dir, CancellationToken::new())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn watch_is_unsupported_and_change_tracking_is_conservative_polling() {
    let (_fixture, provider) = provider().await;
    let result = provider.watch(&root(), CancellationToken::new()).await;
    assert!(matches!(
        result,
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::WATCH
        })
    ));
    assert!(matches!(
        provider.change_tracking(),
        fm_vfs::ChangeTracking::Poll { .. }
    ));
}

#[tokio::test]
async fn server_side_copy_then_commit_publishes_the_destination() {
    let (_fixture, provider) = provider().await;
    let source = root().join("source.txt").unwrap();
    write_all(&provider, &source, b"copy me", false).await;

    let temporary = root().join(".tmp-copy").unwrap();
    let copied = provider
        .server_side_copy(&entry(source.clone()), &temporary, CancellationToken::new())
        .await
        .expect("server_side_copy must succeed");
    assert!(copied);

    let destination = root().join("destination.txt").unwrap();
    provider
        .commit_copy(
            &entry(source),
            &temporary,
            &destination,
            fm_vfs::CopyCommitOptions {
                overwrite: false,
                preserve_metadata: false,
            },
            CancellationToken::new(),
        )
        .await
        .expect("commit_copy must succeed");

    let downloaded = read_all(&provider, &entry(destination)).await;
    assert_eq!(downloaded, b"copy me");
}

#[tokio::test]
async fn discard_copy_of_a_never_created_temporary_succeeds() {
    let (_fixture, provider) = provider().await;
    let temporary = root().join(".tmp-never-created").unwrap();
    provider
        .discard_copy(&temporary, CancellationToken::new())
        .await
        .expect("discard_copy of a missing temporary must not error");
}

/// Real-endpoint smoke test, run only on demand (`cargo test -p fm-vfs-s3 --
/// --ignored real_endpoint_smoke_test`), against a real S3-compatible server
/// rather than [`S3Fixture`] - defaults match the local MinIO instance
/// started for this task (`brew install minio/stable/minio minio/stable/mc`,
/// then `MINIO_ROOT_USER=fm-test-access MINIO_ROOT_PASSWORD=fm-test-secret-key
/// minio server --address 127.0.0.1:9000 .minio-data`, a bucket named
/// `fm-test-bucket` with a 10 MiB hard quota). Override any of
/// `FM_S3_SMOKE_ENDPOINT`/`FM_S3_SMOKE_BUCKET`/`FM_S3_SMOKE_REGION`/
/// `FM_S3_SMOKE_ACCESS_KEY_ID`/`FM_S3_SMOKE_SECRET_ACCESS_KEY` to point at a
/// different real endpoint (AWS S3, R2, B2, ...) instead. Unlike
/// `fm-vfs-ftp`'s public-server smoke test, this deliberately hard-fails
/// rather than soft-skipping: it targets a server the caller just started on
/// purpose, so a failure is worth seeing immediately. Cleans up every object
/// it creates, so repeat runs never grow the bucket.
#[tokio::test]
#[ignore = "real S3-compatible endpoint smoke test; see doc comment for setup"]
async fn real_endpoint_smoke_test() {
    struct EnvResolver;

    #[async_trait]
    impl S3ConnectionResolver for EnvResolver {
        async fn resolve(&self, _connection_id: &str) -> Result<S3ConnectionParameters, VfsError> {
            Ok(S3ConnectionParameters {
                endpoint: Some(env_or("FM_S3_SMOKE_ENDPOINT", "http://127.0.0.1:9000")),
                region: env_or("FM_S3_SMOKE_REGION", "us-east-1"),
                bucket: env_or("FM_S3_SMOKE_BUCKET", "fm-test-bucket"),
                access_key_id: env_or("FM_S3_SMOKE_ACCESS_KEY_ID", "fm-test-access"),
                secret_access_key: env_or("FM_S3_SMOKE_SECRET_ACCESS_KEY", "fm-test-secret-key"),
            })
        }
    }

    fn env_or(name: &str, default: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| default.to_owned())
    }

    let provider = S3FileSystemProvider::new(Arc::new(EnvResolver));
    let smoke_root = root();

    // Plain upload/download/list round trip.
    let file = smoke_root.join("fm-smoke-test.txt").unwrap();
    write_all(&provider, &file, b"fm smoke test payload", true).await;

    let downloaded = read_all(&provider, &entry(file.clone())).await;
    assert_eq!(downloaded, b"fm smoke test payload");

    let page = provider
        .list(
            &smoke_root,
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("list against the real endpoint must succeed");
    assert!(
        page.entries
            .iter()
            .any(|found| found.name == "fm-smoke-test.txt")
    );

    // Rename via CopyObject + DeleteObject (a real SigV4-signed CopyObject
    // request, exercising the `x-amz-copy-source` signed-header path).
    let renamed = smoke_root.join("fm-smoke-test-renamed.txt").unwrap();
    provider
        .rename(&entry(file), &renamed, CancellationToken::new())
        .await
        .expect("rename against the real endpoint must succeed");
    let downloaded = read_all(&provider, &entry(renamed.clone())).await;
    assert_eq!(downloaded, b"fm smoke test payload");

    // A forced multipart upload (real CreateMultipartUpload/UploadPart/
    // CompleteMultipartUpload against the real endpoint): the threshold is
    // clamped up to S3's 5 MiB minimum non-final part size, so the payload
    // must exceed that too for the multipart path to actually trigger - 6
    // MiB total (a ~5 MiB first part plus a ~1 MiB final part) comfortably
    // fits the 10 MiB bucket quota alongside this test's other small objects.
    let multipart_provider =
        S3FileSystemProvider::with_multipart_threshold(Arc::new(EnvResolver), 1);
    let multipart_file = smoke_root.join("fm-smoke-test-multipart.bin").unwrap();
    let multipart_contents: Vec<u8> = (0_u8..=255).cycle().take(6 * 1024 * 1024).collect();
    write_all(
        &multipart_provider,
        &multipart_file,
        &multipart_contents,
        true,
    )
    .await;
    let downloaded = read_all(&multipart_provider, &entry(multipart_file.clone())).await;
    assert_eq!(downloaded, multipart_contents);

    provider
        .remove(
            &entry(renamed),
            RemoveOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("cleanup delete against the real endpoint must succeed");
    provider
        .remove(
            &entry(multipart_file),
            RemoveOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("cleanup delete against the real endpoint must succeed");
}
