//! Resume against REAL servers (task 0303). Ignored by default: it needs an OpenSSH server on
//! 127.0.0.1:2222 (user `tester`), an S3-compatible server on 127.0.0.1:8333 with bucket
//! `livebucket`, and a WebDAV server on 127.0.0.1:6060, started by hand (podman) and removed
//! afterwards. Run with `cargo test -p fm-application live_ -- --ignored --nocapture`.

use std::sync::Arc;

use async_trait::async_trait;
use fm_domain::{EntryId, Location};
use fm_ssh::{
    HostKeyVerification, InMemoryKnownHostsStore, KnownHostsStore, SshConnectTarget,
    SshConnectionManager, SshConnectionParameters, SshCredential, SshHostKeyPolicy,
};
use fm_vfs::{CopyCommitOptions, FileSystemProvider, VfsError, WriteOptions};
use fm_vfs_s3::{S3ConnectionParameters, S3ConnectionResolver, S3FileSystemProvider};
use fm_vfs_sftp::{SftpFileSystemProvider, SshConnectionResolver};
use fm_vfs_webdav::{
    WebDavAuthScheme, WebDavConnectionParameters, WebDavConnectionResolver,
    WebDavFileSystemProvider,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

const CONNECTION: &str = "11111111-1111-4111-8111-111111111111";
const SIZE: usize = 3 * 1024 * 1024 + 123;
const PREFIX: usize = 1_000_000;

fn payload() -> Vec<u8> {
    (0..SIZE)
        .map(|n| ((n * 31 + n / 251) % 256) as u8)
        .collect()
}

struct SftpResolver;

#[async_trait]
impl SshConnectionResolver for SftpResolver {
    async fn resolve(&self, _connection_id: &str) -> Result<SshConnectionParameters, VfsError> {
        Ok(SshConnectionParameters {
            target: SshConnectTarget {
                host: "127.0.0.1".into(),
                port: 2222,
                username: "tester".into(),
            },
            credential: SshCredential::Password("testerpw".to_owned().into()),
            host_key_policy: SshHostKeyPolicy::PromptOnFirstUse,
            keepalive: None,
        })
    }
}

async fn sftp_provider() -> Arc<dyn FileSystemProvider> {
    let known = Arc::new(InMemoryKnownHostsStore::new());
    let manager = Arc::new(SshConnectionManager::new(known.clone()));
    let target = SshConnectTarget {
        host: "127.0.0.1".into(),
        port: 2222,
        username: "tester".into(),
    };
    if let HostKeyVerification::Unverified { fingerprint } = manager
        .probe_host_key(CONNECTION, &target)
        .await
        .expect("probe the SFTP server")
    {
        known
            .accept(CONNECTION, fingerprint)
            .await
            .expect("accept key");
    }
    Arc::new(SftpFileSystemProvider::new(manager, Arc::new(SftpResolver)))
}

struct S3Resolver;

#[async_trait]
impl S3ConnectionResolver for S3Resolver {
    async fn resolve(&self, _id: &str) -> Result<S3ConnectionParameters, VfsError> {
        Ok(S3ConnectionParameters {
            endpoint: Some("http://127.0.0.1:8333".into()),
            region: "us-east-1".into(),
            bucket: "livebucket".into(),
            access_key_id: "testkey".into(),
            secret_access_key: "testsecretkey".into(),
        })
    }
}

struct DavResolver;

#[async_trait]
impl WebDavConnectionResolver for DavResolver {
    async fn resolve(&self, _id: &str) -> Result<WebDavConnectionParameters, VfsError> {
        Ok(WebDavConnectionParameters {
            base_url: "http://127.0.0.1:6060".into(),
            username: "tester".into(),
            password: "testerpw".into(),
            auth_scheme: WebDavAuthScheme::Basic,
        })
    }
}

fn entry(location: &Location) -> EntryRef {
    EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    }
}

async fn put(
    provider: &dyn FileSystemProvider,
    temporary: &Location,
    destination: &Location,
    bytes: &[u8],
) {
    let mut writer = provider
        .open_write(temporary, WriteOptions::default(), CancellationToken::new())
        .await
        .expect("open_write");
    writer.write_all(bytes).await.expect("write");
    writer.shutdown().await.expect("shutdown");
    drop(writer);
    provider
        .commit_copy(
            &entry(temporary),
            temporary,
            destination,
            CopyCommitOptions {
                overwrite: true,
                preserve_metadata: false,
            },
            CancellationToken::new(),
        )
        .await
        .expect("commit_copy");
}

/// Writes `bytes` as a private partial object (no publish step).
async fn put_partial(provider: &dyn FileSystemProvider, partial: &Location, bytes: &[u8]) {
    let mut writer = provider
        .open_write(partial, WriteOptions::default(), CancellationToken::new())
        .await
        .expect("open partial");
    writer.write_all(bytes).await.expect("write partial");
    writer.shutdown().await.expect("close partial");
}

async fn read_all(provider: &dyn FileSystemProvider, location: &Location) -> Vec<u8> {
    let mut reader = provider
        .open_read(&entry(location), CancellationToken::new())
        .await
        .expect("open_read");
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.expect("read");
    bytes
}

async fn read_slice(
    provider: &dyn FileSystemProvider,
    location: &Location,
    offset: u64,
    length: Option<u64>,
) -> Result<Vec<u8>, VfsError> {
    let mut reader = provider
        .read_range(&entry(location), offset, length, CancellationToken::new())
        .await?;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.expect("read range");
    Ok(bytes)
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
}

/// Runs one copy of `source` into `destination_directory` with `partial` hinted for resume.
async fn run_resume(
    source_provider: Arc<dyn FileSystemProvider>,
    destination_provider: Arc<dyn FileSystemProvider>,
    source: &Location,
    destination_directory: &Location,
    partial: &Location,
) -> (Result<ExecutionOutcome, ExecutionError>, JournalingReporter) {
    let mut executor = copy_only_executor(
        source_provider,
        destination_provider,
        source.clone(),
        destination_directory.clone(),
        false,
    );
    executor.resume_hints = Arc::new(HashMap::from([(source.clone(), partial.clone())]));
    let operation = Operation::new(
        fm_operations::OperationKind::Copy,
        Vec::new(),
        Some(destination_directory.clone()),
        fm_operations::ConflictPolicy::Ask,
    )
    .with_verification(true);
    let reporter = JournalingReporter::default();
    let plan = executor
        .plan(&operation, &CancellationToken::new())
        .await
        .expect("plan");
    let item = plan.items.first().cloned().expect("one item");
    let result = executor
        .execute(
            &operation,
            &item,
            None,
            &reporter,
            &PauseToken::default(),
            &CancellationToken::new(),
        )
        .await;
    (result, reporter)
}

fn local_dir() -> (std::path::PathBuf, Location) {
    let directory = tempfile::tempdir().expect("temp").keep();
    let location = Location::from_native_path(&directory).expect("location");
    (directory, location)
}

/// Download resume from a remote source into a local partial.
async fn download_resume(provider: Arc<dyn FileSystemProvider>, remote_dir: &str, scheme: &str) {
    let data = payload();
    let name = unique("live");
    let remote = |file: &str| {
        Location::parse(&format!("{scheme}://{CONNECTION}/{remote_dir}{file}"))
            .expect("remote location")
    };
    let source = remote(&name);
    put(
        provider.as_ref(),
        &remote(&format!(".fm-copy-{name}")),
        &source,
        &data,
    )
    .await;
    assert_eq!(
        read_all(provider.as_ref(), &source).await,
        data,
        "upload round-trips"
    );

    // Ranged reads against the real server.
    match read_slice(provider.as_ref(), &source, PREFIX as u64, None).await {
        Ok(tail) => assert_eq!(tail, data[PREFIX..], "{scheme}: read_range tail"),
        Err(VfsError::UnsupportedCapability { .. }) => {
            println!("{scheme}: read_range unsupported (resume will skip-read)");
        }
        Err(error) => panic!("{scheme}: read_range failed: {error:?}"),
    }
    if let Ok(slice) = read_slice(provider.as_ref(), &source, 10, Some(100)).await {
        assert_eq!(slice, data[10..110], "{scheme}: bounded read_range");
    }

    // An interrupted download: a local partial with the first PREFIX bytes.
    let (directory, destination_dir) = local_dir();
    let partial_path = directory.join(".fm-copy-interrupted");
    std::fs::write(&partial_path, &data[..PREFIX]).expect("write partial");
    let partial = Location::from_native_path(&partial_path).expect("partial location");
    let local: Arc<dyn FileSystemProvider> = Arc::new(fm_vfs_local::LocalFileSystemProvider::new());
    let (result, reporter) =
        run_resume(provider.clone(), local, &source, &destination_dir, &partial).await;
    result.unwrap_or_else(|error| panic!("{scheme}: resumed download failed: {error:?}"));
    assert_eq!(
        reporter.resumes.lock().unwrap().clone(),
        vec![fm_operations::ResumeOutcome::Resumed {
            bytes: PREFIX as u64
        }],
        "{scheme}: the download continued from the partial"
    );
    assert_eq!(
        std::fs::read(directory.join(&name)).expect("published"),
        data
    );
    assert_eq!(
        reporter.verifications.lock().unwrap().clone(),
        vec![fm_operations::VerificationOutcome::Verified]
    );
    let _ = provider
        .remove(
            &entry(&source),
            fm_vfs::RemoveOptions::default(),
            CancellationToken::new(),
        )
        .await;
    println!("{scheme}: download resume OK ({PREFIX} of {SIZE} bytes not fetched again)");
}

#[tokio::test]
#[ignore = "needs the podman SFTP server"]
async fn live_sftp_resumes_downloads_and_uploads() {
    let provider = sftp_provider().await;
    download_resume(provider.clone(), "config/", "sftp").await;

    // Upload resume: a local source into an SFTP partial that already holds the first bytes.
    let data = payload();
    let name = unique("live-up");
    let (directory, _) = local_dir();
    let source_path = directory.join(&name);
    std::fs::write(&source_path, &data).expect("write source");
    let source = Location::from_native_path(&source_path).expect("source location");
    let remote = |file: &str| {
        Location::parse(&format!("sftp://{CONNECTION}/config/{file}")).expect("location")
    };
    let partial = remote(&format!(".fm-copy-{name}"));
    let mut writer = provider
        .open_write(&partial, WriteOptions::default(), CancellationToken::new())
        .await
        .expect("open partial");
    writer
        .write_all(&data[..PREFIX])
        .await
        .expect("write prefix");
    writer.shutdown().await.expect("close partial");
    drop(writer);

    let local: Arc<dyn FileSystemProvider> = Arc::new(fm_vfs_local::LocalFileSystemProvider::new());
    let destination_dir =
        Location::parse(&format!("sftp://{CONNECTION}/config")).expect("directory location");
    let (result, reporter) =
        run_resume(local, provider.clone(), &source, &destination_dir, &partial).await;
    result.unwrap_or_else(|error| panic!("sftp: resumed upload failed: {error:?}"));
    assert_eq!(
        reporter.resumes.lock().unwrap().clone(),
        vec![fm_operations::ResumeOutcome::Resumed {
            bytes: PREFIX as u64
        }],
        "sftp: the upload continued from the partial"
    );
    assert_eq!(read_all(provider.as_ref(), &remote(&name)).await, data);
    assert_eq!(
        reporter.verifications.lock().unwrap().clone(),
        vec![fm_operations::VerificationOutcome::Verified]
    );
    let _ = provider
        .remove(
            &entry(&remote(&name)),
            fm_vfs::RemoveOptions::default(),
            CancellationToken::new(),
        )
        .await;
    println!("sftp: upload resume OK ({PREFIX} of {SIZE} bytes not sent again)");
}

#[tokio::test]
#[ignore = "needs the podman S3 server"]
async fn live_s3_resumes_downloads_and_never_claims_upload_resume() {
    let provider: Arc<dyn FileSystemProvider> = Arc::new(
        S3FileSystemProvider::with_multipart_threshold(Arc::new(S3Resolver), 5 * 1024 * 1024),
    );
    download_resume(provider.clone(), "", "s3").await;

    // Uploading to S3: a partial object cannot be appended to, so a hint must fall back to a
    // full copy that still produces the right object.
    let data = payload();
    let name = unique("live-s3-up");
    let (directory, _) = local_dir();
    let source_path = directory.join(&name);
    std::fs::write(&source_path, &data).expect("write source");
    let source = Location::from_native_path(&source_path).expect("source location");
    let remote =
        |file: &str| Location::parse(&format!("s3://{CONNECTION}/{file}")).expect("location");
    let partial = remote(&format!(".fm-copy-{name}"));
    put_partial(provider.as_ref(), &partial, &data[..PREFIX]).await;
    let local: Arc<dyn FileSystemProvider> = Arc::new(fm_vfs_local::LocalFileSystemProvider::new());
    let (result, reporter) =
        run_resume(local, provider.clone(), &source, &remote(""), &partial).await;
    result.unwrap_or_else(|error| panic!("s3: upload with a stale hint failed: {error:?}"));
    assert_eq!(
        reporter.resumes.lock().unwrap().clone(),
        vec![fm_operations::ResumeOutcome::FellBack(
            fm_operations::ResumeFallback::Unverifiable
        )]
    );
    assert_eq!(read_all(provider.as_ref(), &remote(&name)).await, data);
    let _ = provider
        .remove(
            &entry(&remote(&name)),
            fm_vfs::RemoveOptions::default(),
            CancellationToken::new(),
        )
        .await;
    println!("s3: upload falls back to a full copy as designed");
}

#[tokio::test]
#[ignore = "needs the podman WebDAV server"]
async fn live_webdav_resumes_downloads_and_never_claims_upload_resume() {
    let provider: Arc<dyn FileSystemProvider> =
        Arc::new(WebDavFileSystemProvider::new(Arc::new(DavResolver)));
    download_resume(provider.clone(), "", "webdav").await;

    let data = payload();
    let name = unique("live-dav-up");
    let (directory, _) = local_dir();
    let source_path = directory.join(&name);
    std::fs::write(&source_path, &data).expect("write source");
    let source = Location::from_native_path(&source_path).expect("source location");
    let remote =
        |file: &str| Location::parse(&format!("webdav://{CONNECTION}/{file}")).expect("location");
    let partial = remote(&format!(".fm-copy-{name}"));
    put_partial(provider.as_ref(), &partial, &data[..PREFIX]).await;
    let local: Arc<dyn FileSystemProvider> = Arc::new(fm_vfs_local::LocalFileSystemProvider::new());
    let (result, reporter) =
        run_resume(local, provider.clone(), &source, &remote(""), &partial).await;
    result.unwrap_or_else(|error| panic!("webdav: upload with a stale hint failed: {error:?}"));
    assert_eq!(
        reporter.resumes.lock().unwrap().clone(),
        vec![fm_operations::ResumeOutcome::FellBack(
            fm_operations::ResumeFallback::Unverifiable
        )]
    );
    assert_eq!(read_all(provider.as_ref(), &remote(&name)).await, data);
    let _ = provider
        .remove(
            &entry(&remote(&name)),
            fm_vfs::RemoveOptions::default(),
            CancellationToken::new(),
        )
        .await;
    println!("webdav: upload falls back to a full copy as designed");
}
