use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use fm_domain::{EntryId, EntryKind, EntryMetadata, EntrySummary, Location, ProviderId};
use fm_vfs::{
    CONSERVATIVE_POLL_INTERVAL, ChangeTracking, CopyCommitOptions, DirectoryPage, EntryRef,
    FileSystemProvider, ListOptions, ProviderCapabilities, ProviderChangeStream,
    ProviderReadStream, ProviderWriteStream, RemoveOptions, TransferCapabilities, TransferEndpoint,
    VfsError, WriteOptions,
};
use futures::TryStreamExt;
use rusty_s3::actions::{CreateMultipartUpload, DeleteObject, GetObject, ListObjectsV2, PutObject};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

use crate::resolver::{S3ConnectionParameters, S3ConnectionResolver};

/// Provider id used by every `s3://` [`Location`].
const S3_PROVIDER: &str = "s3";

/// Default byte threshold above which [`S3FileSystemProvider::open_write`]
/// switches from a single `PutObject` to a multipart upload.
///
/// Kept well under S3's 5 GiB single-`PUT` limit so an upload of unknown
/// total length never risks buffering the whole file in memory: once this
/// many bytes have arrived without seeing end-of-stream, the buffered prefix
/// becomes multipart part 1 and every later chunk streams straight through
/// as its own part instead of accumulating further.
pub const DEFAULT_MULTIPART_THRESHOLD: u64 = 64 * 1024 * 1024;

/// S3's own minimum size for every multipart part except the last one (AWS
/// API reference: `UploadPart`). The buffered prefix that becomes part 1
/// once the threshold is reached is never the last part by construction (a
/// full buffer means more data is still coming), so the threshold itself
/// must never be set below this - a real S3-compatible endpoint rejects a
/// smaller non-final part with `EntityTooSmall` (caught by this crate's real
/// endpoint smoke test against local MinIO; the in-process mock fixture does
/// not enforce this, having no reason to invent a limit its own tests don't
/// need). [`S3FileSystemProvider::with_multipart_threshold`] clamps to this.
const MINIMUM_MULTIPART_PART_SIZE: u64 = 5 * 1024 * 1024;

/// Maximum attempts for each part before the multipart upload is aborted.
const MAX_UPLOAD_PART_ATTEMPTS: usize = 3;

/// Duration a presigned request stays valid for. Requests are signed and
/// sent immediately, so this only needs to comfortably exceed one round trip.
const SIGNED_URL_TTL: std::time::Duration = std::time::Duration::from_secs(300);

/// VFS provider for S3-compatible object storage (task 0146): AWS S3 and any
/// endpoint speaking the same API (MinIO, Cloudflare R2, Backblaze B2, ...).
///
/// Buckets have no real directories. `mkdir` therefore creates a zero-byte
/// marker object whose key ends in `/`, matching the convention used by the
/// AWS console and most other S3 clients (documented choice, task 0146's
/// implementation notes: "decide and document" rather than silently no-op).
/// A "directory" is otherwise recognized purely by `ListObjectsV2`'s
/// delimiter/prefix semantics - there is no other server-side concept of one.
pub struct S3FileSystemProvider {
    resolver: Arc<dyn S3ConnectionResolver>,
    http: reqwest::Client,
    multipart_threshold: u64,
}

impl S3FileSystemProvider {
    /// Creates a provider using [`DEFAULT_MULTIPART_THRESHOLD`].
    #[must_use]
    pub fn new(resolver: Arc<dyn S3ConnectionResolver>) -> Self {
        Self::with_multipart_threshold(resolver, DEFAULT_MULTIPART_THRESHOLD)
    }

    /// Creates a provider with an explicit multipart threshold, for example
    /// in tests that want to exercise the multipart path without a huge
    /// payload. Clamped up to [`MINIMUM_MULTIPART_PART_SIZE`] - a smaller
    /// value would make the first (non-final) part too small for a real
    /// S3-compatible endpoint to accept.
    #[must_use]
    pub fn with_multipart_threshold(
        resolver: Arc<dyn S3ConnectionResolver>,
        multipart_threshold: u64,
    ) -> Self {
        Self {
            resolver,
            http: reqwest::Client::new(),
            multipart_threshold: multipart_threshold.max(MINIMUM_MULTIPART_PART_SIZE),
        }
    }

    /// Verifies the bucket is reachable and the credentials are accepted,
    /// using a `HeadBucket` request, without retaining a session.
    pub async fn verify_connectivity(params: &S3ConnectionParameters) -> Result<(), VfsError> {
        let bucket = build_bucket(params)?;
        let credentials = Credentials::new(
            params.access_key_id.clone(),
            params.secret_access_key.clone(),
        );
        let action = bucket.head_bucket(Some(&credentials));
        let url = action.sign(SIGNED_URL_TTL);
        let response = reqwest::Client::new()
            .head(url)
            .send()
            .await
            .map_err(map_reqwest)?;
        ensure_success(response).await?;
        Ok(())
    }

    async fn bucket_and_credentials(
        &self,
        connection_id: &str,
    ) -> Result<(Bucket, Credentials), VfsError> {
        let params = self.resolver.resolve(connection_id).await?;
        let bucket = build_bucket(&params)?;
        let credentials = Credentials::new(params.access_key_id, params.secret_access_key);
        Ok((bucket, credentials))
    }
}

fn build_bucket(params: &S3ConnectionParameters) -> Result<Bucket, VfsError> {
    let endpoint_text = params
        .endpoint
        .clone()
        .unwrap_or_else(|| format!("https://s3.{}.amazonaws.com", params.region));
    let endpoint: reqwest::Url = endpoint_text.parse().map_err(|_| VfsError::Io {
        message: format!("invalid S3 endpoint URL: {endpoint_text}"),
    })?;
    // Path-style addressing works uniformly against AWS S3 and every
    // S3-compatible endpoint (MinIO, R2, B2); virtual-host style requires a
    // bucket-specific DNS entry that self-hosted endpoints rarely have.
    Bucket::new(
        endpoint,
        UrlStyle::Path,
        params.bucket.clone(),
        params.region.clone(),
    )
    .map_err(|error| VfsError::Io {
        message: format!("invalid S3 bucket configuration: {error}"),
    })
}

#[async_trait]
impl FileSystemProvider for S3FileSystemProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new(S3_PROVIDER)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::LIST
            | ProviderCapabilities::READ
            | ProviderCapabilities::WRITE
            | ProviderCapabilities::CREATE_DIRECTORY
            | ProviderCapabilities::RENAME
            | ProviderCapabilities::MOVE
            | ProviderCapabilities::DELETE
            | ProviderCapabilities::SERVER_SIDE_COPY
            | ProviderCapabilities::RANDOM_ACCESS
    }

    /// Task 0108. The endpoint identifies the concrete connection, which
    /// maps one-to-one onto one bucket in one region - so "same endpoint"
    /// and "same bucket/region" are the same condition here.
    ///
    /// `server_side_move` is `false`: S3 has no native rename, only
    /// `CopyObject` + `DeleteObject` (task 0146's acceptance criteria call
    /// this out explicitly, so the planner never expects an atomic
    /// same-backend move). `server_side_copy` is `true`: `CopyObject` is a
    /// real, implemented fast path within one bucket. `random_read` is
    /// `true` (ranged `GetObject`, see [`Self::read_range`]). Resumable
    /// upload/download and random writes are not implemented, so they stay
    /// `false` rather than advertising a fast path this provider cannot
    /// honour.
    fn transfer_capabilities(&self, location: &Location) -> Result<TransferCapabilities, VfsError> {
        let parsed = Parsed::parse(location)?;
        Ok(TransferCapabilities {
            endpoint: TransferEndpoint::new(format!("s3:{}", parsed.connection_id)),
            server_side_copy: true,
            server_side_move: false,
            resumable_upload: false,
            resumable_download: false,
            random_read: true,
            random_write: false,
        })
    }

    /// S3 has no native change-notification mechanism; `fm-application`'s
    /// directory service polls `list` conservatively instead (task 0109).
    fn change_tracking(&self) -> ChangeTracking {
        ChangeTracking::Poll {
            interval: CONSERVATIVE_POLL_INTERVAL,
        }
    }

    async fn list(
        &self,
        location: &Location,
        options: ListOptions,
        cancellation: CancellationToken,
    ) -> Result<DirectoryPage, VfsError> {
        cancelled(&cancellation)?;
        if options.page_size == 0 {
            return Err(invalid(&location.uri));
        }
        let parsed = Parsed::parse(location)?;
        let (bucket, credentials) = self.bucket_and_credentials(&parsed.connection_id).await?;

        let prefix = parsed.list_prefix();
        let mut action = bucket.list_objects_v2(Some(&credentials));
        action.with_delimiter("/");
        if !prefix.is_empty() {
            action.with_prefix(prefix.clone());
        }
        action.with_max_keys(options.page_size);
        if let Some(token) = &options.continuation_token {
            action.with_continuation_token(token.clone());
        }

        let url = action.sign(SIGNED_URL_TTL);
        let response = self.http.get(url).send().await.map_err(map_reqwest)?;
        let response = ensure_success(response).await?;
        let body = response.text().await.map_err(map_reqwest)?;
        let parsed_response =
            ListObjectsV2::parse_response(&body).map_err(|error| VfsError::Io {
                message: format!("invalid ListObjectsV2 response: {error}"),
            })?;
        cancelled(&cancellation)?;

        let mut entries = Vec::new();
        for common_prefix in &parsed_response.common_prefixes {
            let Some(name) = directory_name(&common_prefix.prefix, &prefix) else {
                continue;
            };
            let child = location.join(&name).map_err(|_| invalid(&location.uri))?;
            entries.push(directory_summary(child, &name));
        }
        for object in &parsed_response.contents {
            // The zero-byte marker object representing this directory itself
            // is not one of its own children.
            if object.key == prefix {
                continue;
            }
            let Some(name) = object.key.strip_prefix(&prefix) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let child = location.join(name).map_err(|_| invalid(&location.uri))?;
            entries.push(file_summary(
                child,
                name,
                object.size,
                &object.last_modified,
            ));
        }

        let has_more = parsed_response.next_continuation_token.is_some();
        Ok(DirectoryPage {
            entries,
            total_known_entries: None,
            has_more,
            continuation_token: parsed_response.next_continuation_token,
        })
    }

    async fn metadata(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntryMetadata, VfsError> {
        cancelled(&cancellation)?;
        Ok(EntryMetadata {
            entry_id: entry.id,
            permissions: None,
            ownership: None,
            extended_attributes: BTreeMap::new(),
            checksums: BTreeMap::new(),
            image_dimensions: None,
            media: None,
            archive: None,
            plugin_fields: BTreeMap::new(),
        })
    }

    async fn inspect(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntrySummary, VfsError> {
        let parent = entry
            .location
            .parent()
            .map_err(|_| invalid(&entry.location.uri))?
            .ok_or_else(|| invalid(&entry.location.uri))?;
        let name = entry
            .location
            .name()
            .map_err(|_| invalid(&entry.location.uri))?;
        self.list(&parent, ListOptions::default(), cancellation)
            .await?
            .entries
            .into_iter()
            .find(|found| found.name == name)
            .ok_or_else(|| VfsError::NotFound {
                location: entry.location.uri.clone(),
            })
    }

    async fn file_size(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<u64, VfsError> {
        self.inspect(entry, cancellation)
            .await?
            .size
            .ok_or_else(|| VfsError::IsADirectory {
                location: entry.location.uri.clone(),
            })
    }

    async fn create_directory(
        &self,
        location: &Location,
        name: &str,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        cancelled(&cancellation)?;
        let directory = location.join(name).map_err(|_| invalid(&location.uri))?;
        let parsed = Parsed::parse(&directory)?;
        let (bucket, credentials) = self.bucket_and_credentials(&parsed.connection_id).await?;
        let marker_key = format!("{}/", parsed.key);
        let action = bucket.put_object(Some(&credentials), &marker_key);
        let url = action.sign(SIGNED_URL_TTL);
        let response = self
            .http
            .put(url)
            .body(Vec::new())
            .send()
            .await
            .map_err(map_reqwest)?;
        ensure_success(response).await?;
        Ok(entry_ref(directory))
    }

    async fn rename(
        &self,
        source: &EntryRef,
        destination: &Location,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        cancelled(&cancellation)?;
        let from = Parsed::parse(&source.location)?;
        let to = Parsed::parse(destination)?;
        if from.connection_id != to.connection_id {
            return Err(invalid(&destination.uri));
        }
        let (bucket, credentials) = self.bucket_and_credentials(&from.connection_id).await?;
        copy_object(&self.http, &bucket, &credentials, &from.key, &to.key).await?;
        delete_object(&self.http, &bucket, &credentials, &from.key).await?;
        Ok(entry_ref(destination.clone()))
    }

    /// `DeleteObject` is idempotent on real S3 - it returns success whether
    /// or not the key existed, and never reports which. That means a target
    /// can be a file (`key`), a directory marker (`key/`), or both never
    /// having existed at all, and this provider cannot tell those apart
    /// before deleting. Both candidate keys are therefore always deleted
    /// unconditionally rather than trying one and inspecting the result.
    async fn remove(
        &self,
        entry: &EntryRef,
        options: RemoveOptions,
        cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        cancelled(&cancellation)?;
        if options.use_trash {
            return Err(VfsError::UnsupportedCapability {
                capability: ProviderCapabilities::DELETE,
            });
        }
        let parsed = Parsed::parse(&entry.location)?;
        let (bucket, credentials) = self.bucket_and_credentials(&parsed.connection_id).await?;

        if options.recursive {
            let child_prefix = format!("{}/", parsed.key);
            let keys = list_all_keys(&self.http, &bucket, &credentials, &child_prefix).await?;
            for key in keys {
                delete_object_ignoring_not_found(&self.http, &bucket, &credentials, &key).await?;
            }
        }
        delete_object_ignoring_not_found(&self.http, &bucket, &credentials, &parsed.key).await?;
        delete_object_ignoring_not_found(
            &self.http,
            &bucket,
            &credentials,
            &format!("{}/", parsed.key),
        )
        .await
    }

    async fn open_read(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError> {
        self.read_range(entry, 0, None, cancellation).await
    }

    async fn read_range(
        &self,
        entry: &EntryRef,
        offset: u64,
        length: Option<u64>,
        cancellation: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError> {
        cancelled(&cancellation)?;
        let parsed = Parsed::parse(&entry.location)?;
        let (bucket, credentials) = self.bucket_and_credentials(&parsed.connection_id).await?;
        let mut action = GetObject::new(&bucket, Some(&credentials), &parsed.key);
        let range = match length {
            Some(length) if length > 0 => {
                format!("bytes={offset}-{}", offset + length - 1)
            }
            Some(_) => {
                // A zero-length request still needs a well-formed stream;
                // read a single byte range and let the caller stop after 0
                // bytes rather than sending a malformed `Range` header.
                format!("bytes={offset}-{offset}")
            }
            None => format!("bytes={offset}-"),
        };
        // SigV4 canonicalization requires signed header *names* to be
        // lowercase (a capitalized "Range" produced a signature a real
        // signature-verifying endpoint - unlike the fixture - rejects with
        // 403; caught by the real-endpoint smoke test against local MinIO).
        action.headers_mut().insert("range", range.clone());
        let url = action.sign(SIGNED_URL_TTL);
        let response = self
            .http
            .get(url)
            .header("range", range)
            .send()
            .await
            .map_err(map_reqwest)?;
        let response = ensure_success(response).await?;
        let stream = response.bytes_stream().map_err(std::io::Error::other);
        Ok(Box::pin(StreamReader::new(stream)))
    }

    async fn open_write(
        &self,
        destination: &Location,
        options: WriteOptions,
        cancellation: CancellationToken,
    ) -> Result<ProviderWriteStream, VfsError> {
        cancelled(&cancellation)?;
        if !options.overwrite
            && self
                .inspect(&entry_ref(destination.clone()), CancellationToken::new())
                .await
                .is_ok()
        {
            return Err(VfsError::AlreadyExists {
                location: destination.uri.clone(),
            });
        }
        let parsed = Parsed::parse(destination)?;
        let (bucket, credentials) = self.bucket_and_credentials(&parsed.connection_id).await?;
        let http = self.http.clone();
        let key = parsed.key;
        let threshold = self.multipart_threshold;

        let (writer, mut reader) = tokio::io::duplex(64 * 1024);
        tokio::spawn(async move {
            let client = ReqwestMultipartUploadClient {
                http: &http,
                bucket: &bucket,
                credentials: &credentials,
                key: &key,
            };
            let upload = tokio::select! {
                result = drive_upload(&client, threshold, &mut reader) => result,
                () = cancellation.cancelled() => Err(VfsError::Cancelled),
            };
            // A failed upload has no caller left to report to by this point
            // (the returned `AsyncWrite` has already been dropped once
            // `shutdown` returns): matches `fm-vfs-ftp`'s `open_write`, which
            // has the same limitation for the same reason - the duplex-pipe
            // pattern can only signal local flush completion, not remote
            // durability. A later reader of the destination will observe it
            // as missing or short rather than silently "succeeding".
            let _ = upload;
        });
        Ok(Box::pin(writer))
    }

    /// Attempts a real `CopyObject` into `temporary`. Only possible within
    /// one connection (S3 has no cross-bucket/cross-region atomic copy);
    /// callers outside that scope get `Ok(false)` and fall back to
    /// streaming, never an error.
    async fn server_side_copy(
        &self,
        source: &EntryRef,
        temporary: &Location,
        cancellation: CancellationToken,
    ) -> Result<bool, VfsError> {
        cancelled(&cancellation)?;
        let from = Parsed::parse(&source.location)?;
        let to = match Parsed::parse(temporary) {
            Ok(parsed) if parsed.connection_id == from.connection_id => parsed,
            _ => return Ok(false),
        };
        let (bucket, credentials) = self.bucket_and_credentials(&from.connection_id).await?;
        copy_object(&self.http, &bucket, &credentials, &from.key, &to.key).await?;
        Ok(true)
    }

    async fn commit_copy(
        &self,
        _source: &EntryRef,
        temporary: &Location,
        destination: &Location,
        options: CopyCommitOptions,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        if !options.overwrite
            && self
                .inspect(&entry_ref(destination.clone()), CancellationToken::new())
                .await
                .is_ok()
        {
            return Err(VfsError::AlreadyExists {
                location: destination.uri.clone(),
            });
        }
        self.rename(&entry_ref(temporary.clone()), destination, cancellation)
            .await
    }

    /// Discarding a temporary that was never created must succeed: the
    /// operation engine calls this on every cancellation and failure path
    /// (task 0108), matching the local, FTP and SFTP providers, which all
    /// swallow `NotFound` here too.
    async fn discard_copy(
        &self,
        temporary: &Location,
        cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        match self
            .remove(
                &entry_ref(temporary.clone()),
                RemoveOptions::default(),
                cancellation,
            )
            .await
        {
            Ok(()) | Err(VfsError::NotFound { .. }) => Ok(()),
            Err(other) => Err(other),
        }
    }

    async fn same_filesystem(
        &self,
        source: &EntryRef,
        destination_directory: &Location,
        cancellation: CancellationToken,
    ) -> Result<bool, VfsError> {
        cancelled(&cancellation)?;
        let from = Parsed::parse(&source.location)?;
        let to = Parsed::parse(destination_directory)?;
        Ok(from.connection_id == to.connection_id)
    }

    async fn watch(
        &self,
        _location: &Location,
        _cancellation: CancellationToken,
    ) -> Result<ProviderChangeStream, VfsError> {
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::WATCH,
        })
    }
}

#[async_trait]
trait MultipartUploadClient {
    async fn put_object(&self, body: Vec<u8>) -> Result<(), VfsError>;

    async fn create_multipart_upload(&self) -> Result<String, VfsError>;

    async fn upload_part(
        &self,
        upload_id: &str,
        part_number: u16,
        body: Bytes,
    ) -> Result<String, UploadPartError>;

    async fn complete_multipart_upload(
        &self,
        upload_id: &str,
        etags: &[String],
    ) -> Result<(), VfsError>;

    async fn abort_multipart_upload(&self, upload_id: &str) -> Result<(), VfsError>;
}

struct ReqwestMultipartUploadClient<'a> {
    http: &'a reqwest::Client,
    bucket: &'a Bucket,
    credentials: &'a Credentials,
    key: &'a str,
}

#[async_trait]
impl MultipartUploadClient for ReqwestMultipartUploadClient<'_> {
    async fn put_object(&self, body: Vec<u8>) -> Result<(), VfsError> {
        let action = PutObject::new(self.bucket, Some(self.credentials), self.key);
        let url = action.sign(SIGNED_URL_TTL);
        let response = self
            .http
            .put(url)
            .body(body)
            .send()
            .await
            .map_err(map_reqwest)?;
        ensure_success(response).await?;
        Ok(())
    }

    async fn create_multipart_upload(&self) -> Result<String, VfsError> {
        let action = self
            .bucket
            .create_multipart_upload(Some(self.credentials), self.key);
        let url = action.sign(SIGNED_URL_TTL);
        let response = self.http.post(url).send().await.map_err(map_reqwest)?;
        let response = ensure_success(response).await?;
        let body = response.text().await.map_err(map_reqwest)?;
        let created =
            CreateMultipartUpload::parse_response(&body).map_err(|error| VfsError::Io {
                message: format!("invalid CreateMultipartUpload response: {error}"),
            })?;
        Ok(created.upload_id().to_owned())
    }

    async fn upload_part(
        &self,
        upload_id: &str,
        part_number: u16,
        body: Bytes,
    ) -> Result<String, UploadPartError> {
        let action =
            self.bucket
                .upload_part(Some(self.credentials), self.key, part_number, upload_id);
        let url = action.sign(SIGNED_URL_TTL);
        let response = self
            .http
            .put(url)
            .body(body)
            .send()
            .await
            .map_err(|error| UploadPartError::retryable(map_reqwest(error)))?;
        let retryable = response.status().is_server_error()
            || matches!(
                response.status(),
                reqwest::StatusCode::REQUEST_TIMEOUT | reqwest::StatusCode::TOO_MANY_REQUESTS
            );
        let response = ensure_success(response).await.map_err(|error| {
            if retryable {
                UploadPartError::retryable(error)
            } else {
                UploadPartError::permanent(error)
            }
        })?;
        response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| {
                UploadPartError::permanent(VfsError::Io {
                    message: "S3-compatible endpoint did not return an ETag for an uploaded part"
                        .to_owned(),
                })
            })
    }

    async fn complete_multipart_upload(
        &self,
        upload_id: &str,
        etags: &[String],
    ) -> Result<(), VfsError> {
        let action = self.bucket.complete_multipart_upload(
            Some(self.credentials),
            self.key,
            upload_id,
            etags.iter().map(String::as_str),
        );
        let url = action.sign(SIGNED_URL_TTL);
        let body = action.body();
        let response = self
            .http
            .post(url)
            .body(body)
            .send()
            .await
            .map_err(map_reqwest)?;
        ensure_success(response).await?;
        Ok(())
    }

    async fn abort_multipart_upload(&self, upload_id: &str) -> Result<(), VfsError> {
        let action =
            self.bucket
                .abort_multipart_upload(Some(self.credentials), self.key, upload_id);
        let url = action.sign(SIGNED_URL_TTL);
        let response = self.http.delete(url).send().await.map_err(map_reqwest)?;
        ensure_success(response).await?;
        Ok(())
    }
}

struct UploadPartError {
    source: VfsError,
    retryable: bool,
}

impl UploadPartError {
    fn retryable(source: VfsError) -> Self {
        Self {
            source,
            retryable: true,
        }
    }

    fn permanent(source: VfsError) -> Self {
        Self {
            source,
            retryable: false,
        }
    }
}

/// Reads from `reader` and uploads to `key`, buffering up to `threshold`
/// bytes before deciding between a single `PutObject` and a multipart
/// upload - so a stream of unknown total length never buffers more than
/// `threshold` bytes at once (task 0146's memory requirement).
async fn drive_upload<C, R>(client: &C, threshold: u64, reader: &mut R) -> Result<(), VfsError>
where
    C: MultipartUploadClient + ?Sized,
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    let mut buffer = Vec::new();
    let mut chunk = vec![0_u8; 64 * 1024];
    let mut eof = false;
    while (buffer.len() as u64) < threshold {
        let read = reader
            .read(&mut chunk)
            .await
            .map_err(|error| VfsError::Io {
                message: error.to_string(),
            })?;
        if read == 0 {
            eof = true;
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }

    if eof {
        client.put_object(buffer).await
    } else {
        multipart_upload(client, buffer, reader).await
    }
}

async fn multipart_upload<C, R>(
    client: &C,
    first_part: Vec<u8>,
    reader: &mut R,
) -> Result<(), VfsError>
where
    C: MultipartUploadClient + ?Sized,
    R: tokio::io::AsyncRead + Unpin,
{
    let upload_id = client.create_multipart_upload().await?;
    let result = match upload_parts(client, &upload_id, first_part, reader).await {
        Ok(etags) => client.complete_multipart_upload(&upload_id, &etags).await,
        Err(error) => Err(error),
    };
    if result.is_err() {
        let _ = client.abort_multipart_upload(&upload_id).await;
    }
    result
}

async fn upload_parts<C, R>(
    client: &C,
    upload_id: &str,
    first_part: Vec<u8>,
    reader: &mut R,
) -> Result<Vec<String>, VfsError>
where
    C: MultipartUploadClient + ?Sized,
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    let mut etags = Vec::new();
    let mut part_number: u16 = 1;
    let mut part = Bytes::from(first_part);
    loop {
        let etag = upload_part_with_retry(client, upload_id, part_number, &part).await?;
        etags.push(etag);

        let mut next = vec![0_u8; 8 * 1024 * 1024];
        let mut filled = 0;
        loop {
            let read = reader
                .read(&mut next[filled..])
                .await
                .map_err(|error| VfsError::Io {
                    message: error.to_string(),
                })?;
            if read == 0 {
                break;
            }
            filled += read;
            if filled == next.len() {
                break;
            }
        }
        if filled == 0 {
            break;
        }
        next.truncate(filled);
        part = Bytes::from(next);
        part_number += 1;
    }
    Ok(etags)
}

async fn upload_part_with_retry<C>(
    client: &C,
    upload_id: &str,
    part_number: u16,
    body: &Bytes,
) -> Result<String, VfsError>
where
    C: MultipartUploadClient + ?Sized,
{
    for attempt in 1..=MAX_UPLOAD_PART_ATTEMPTS {
        match client
            .upload_part(upload_id, part_number, body.clone())
            .await
        {
            Ok(etag) => return Ok(etag),
            Err(error) if error.retryable && attempt < MAX_UPLOAD_PART_ATTEMPTS => {}
            Err(error) => return Err(error.source),
        }
    }
    unreachable!("the bounded retry loop always returns on its final attempt")
}

async fn copy_object(
    http: &reqwest::Client,
    bucket: &Bucket,
    credentials: &Credentials,
    source_key: &str,
    destination_key: &str,
) -> Result<(), VfsError> {
    let mut action = PutObject::new(bucket, Some(credentials), destination_key);
    let copy_source = format!("/{}/{}", bucket.name(), encode_copy_source(source_key));
    action
        .headers_mut()
        .insert("x-amz-copy-source", copy_source.clone());
    let url = action.sign(SIGNED_URL_TTL);
    let response = http
        .put(url)
        .header("x-amz-copy-source", copy_source)
        .body(Vec::new())
        .send()
        .await
        .map_err(map_reqwest)?;
    ensure_success(response).await?;
    Ok(())
}

async fn delete_object(
    http: &reqwest::Client,
    bucket: &Bucket,
    credentials: &Credentials,
    key: &str,
) -> Result<(), VfsError> {
    let action = DeleteObject::new(bucket, Some(credentials), key);
    let url = action.sign(SIGNED_URL_TTL);
    let response = http.delete(url).send().await.map_err(map_reqwest)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(VfsError::NotFound {
            location: key.to_owned(),
        });
    }
    ensure_success(response).await?;
    Ok(())
}

async fn delete_object_ignoring_not_found(
    http: &reqwest::Client,
    bucket: &Bucket,
    credentials: &Credentials,
    key: &str,
) -> Result<(), VfsError> {
    match delete_object(http, bucket, credentials, key).await {
        Ok(()) | Err(VfsError::NotFound { .. }) => Ok(()),
        Err(other) => Err(other),
    }
}

async fn list_all_keys(
    http: &reqwest::Client,
    bucket: &Bucket,
    credentials: &Credentials,
    prefix: &str,
) -> Result<Vec<String>, VfsError> {
    let mut keys = Vec::new();
    let mut continuation: Option<String> = None;
    loop {
        let mut action = bucket.list_objects_v2(Some(credentials));
        action.with_prefix(prefix.to_owned());
        if let Some(token) = &continuation {
            action.with_continuation_token(token.clone());
        }
        let url = action.sign(SIGNED_URL_TTL);
        let response = http.get(url).send().await.map_err(map_reqwest)?;
        let response = ensure_success(response).await?;
        let body = response.text().await.map_err(map_reqwest)?;
        let parsed = ListObjectsV2::parse_response(&body).map_err(|error| VfsError::Io {
            message: format!("invalid ListObjectsV2 response: {error}"),
        })?;
        keys.extend(parsed.contents.into_iter().map(|object| object.key));
        continuation = parsed.next_continuation_token;
        if continuation.is_none() {
            break;
        }
    }
    Ok(keys)
}

fn encode_copy_source(key: &str) -> String {
    key.split('/')
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_encode_segment(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, VfsError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(VfsError::NotFound {
            location: response.url().to_string(),
        });
    }
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(VfsError::PermissionDenied {
            location: response.url().to_string(),
        });
    }
    let body = response.text().await.unwrap_or_default();
    Err(VfsError::Io {
        message: format!("S3-compatible endpoint returned {status}: {body}"),
    })
}

fn map_reqwest(error: reqwest::Error) -> VfsError {
    VfsError::Io {
        message: error.to_string(),
    }
}

fn cancelled(cancellation: &CancellationToken) -> Result<(), VfsError> {
    if cancellation.is_cancelled() {
        Err(VfsError::Cancelled)
    } else {
        Ok(())
    }
}

fn invalid(uri: &str) -> VfsError {
    VfsError::InvalidLocation {
        location: uri.to_owned(),
    }
}

fn entry_ref(location: Location) -> EntryRef {
    EntryRef {
        id: EntryId::new(),
        location,
    }
}

fn directory_name(prefix: &str, parent_prefix: &str) -> Option<String> {
    let rest = prefix.strip_prefix(parent_prefix)?;
    let name = rest.strip_suffix('/')?;
    (!name.is_empty()).then(|| name.to_owned())
}

fn directory_summary(location: Location, name: &str) -> EntrySummary {
    EntrySummary {
        id: EntryId::new(),
        location,
        name: name.to_owned(),
        kind: EntryKind::Directory,
        size: None,
        modified_at: None,
        created_at: None,
        hidden: name.starts_with('.'),
        read_only: false,
        extension: None,
        mime_type: None,
        icon_key: None,
        metadata_revision: 0,
        git_status: None,
    }
}

fn file_summary(location: Location, name: &str, size: u64, last_modified: &str) -> EntrySummary {
    EntrySummary {
        id: EntryId::new(),
        location,
        name: name.to_owned(),
        kind: EntryKind::File,
        size: Some(size),
        modified_at: chrono::DateTime::parse_from_rfc3339(last_modified)
            .ok()
            .map(|value| value.with_timezone(&chrono::Utc)),
        created_at: None,
        hidden: name.starts_with('.'),
        read_only: false,
        extension: std::path::Path::new(name)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_owned),
        mime_type: None,
        icon_key: None,
        metadata_revision: 0,
        git_status: None,
    }
}

/// A parsed `s3://<connection-id>/<key>` location.
struct Parsed {
    connection_id: String,
    /// Object key relative to the bucket root, without a leading slash.
    /// Empty for the bucket root.
    key: String,
}

impl Parsed {
    fn parse(location: &Location) -> Result<Self, VfsError> {
        if location.provider_id.as_str() != S3_PROVIDER {
            return Err(invalid(&location.uri));
        }
        let remainder = location
            .uri
            .strip_prefix("s3://")
            .ok_or_else(|| invalid(&location.uri))?;
        let (connection_id, path) = remainder
            .split_once('/')
            .ok_or_else(|| invalid(&location.uri))?;
        if connection_id.is_empty() {
            return Err(invalid(&location.uri));
        }
        let key = if path.is_empty() {
            String::new()
        } else {
            let segments = path
                .split('/')
                .map(|segment| decode_percent(segment, location))
                .collect::<Result<Vec<_>, _>>()?;
            segments.join("/")
        };
        Ok(Self {
            connection_id: connection_id.to_owned(),
            key,
        })
    }

    /// The prefix `ListObjectsV2` should filter on: the bucket root lists
    /// with an empty prefix, any other "directory" lists with its key plus
    /// a trailing slash so only its own children match.
    fn list_prefix(&self) -> String {
        if self.key.is_empty() {
            String::new()
        } else {
            format!("{}/", self.key)
        }
    }
}

fn decode_percent(segment: &str, location: &Location) -> Result<String, VfsError> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(value) = hex.and_then(|value| u8::from_str_radix(value, 16).ok()) {
                decoded.push(value);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).map_err(|_| invalid(&location.uri))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use bytes::Bytes;

    use super::{
        MultipartUploadClient, UploadPartError, drive_upload, multipart_upload, upload_parts,
    };
    use fm_vfs::VfsError;

    #[derive(Default)]
    struct MockUploadClient {
        put_bodies: Mutex<Vec<Vec<u8>>>,
        upload_results: Mutex<VecDeque<Result<String, UploadPartError>>>,
        uploaded_parts: Mutex<Vec<(u16, Vec<u8>)>>,
        completions: Mutex<Vec<Vec<String>>>,
        completion_error: Mutex<Option<VfsError>>,
        aborts: Mutex<usize>,
    }

    impl MockUploadClient {
        fn with_upload_results(results: Vec<Result<String, UploadPartError>>) -> Self {
            Self {
                upload_results: Mutex::new(results.into()),
                ..Self::default()
            }
        }
    }

    #[async_trait]
    impl MultipartUploadClient for MockUploadClient {
        async fn put_object(&self, body: Vec<u8>) -> Result<(), VfsError> {
            self.put_bodies.lock().unwrap().push(body);
            Ok(())
        }

        async fn create_multipart_upload(&self) -> Result<String, VfsError> {
            Ok("upload-1".to_owned())
        }

        async fn upload_part(
            &self,
            _upload_id: &str,
            part_number: u16,
            body: Bytes,
        ) -> Result<String, UploadPartError> {
            self.uploaded_parts
                .lock()
                .unwrap()
                .push((part_number, body.to_vec()));
            self.upload_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(format!("etag-{part_number}")))
        }

        async fn complete_multipart_upload(
            &self,
            _upload_id: &str,
            etags: &[String],
        ) -> Result<(), VfsError> {
            self.completions.lock().unwrap().push(etags.to_vec());
            match self.completion_error.lock().unwrap().take() {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }

        async fn abort_multipart_upload(&self, _upload_id: &str) -> Result<(), VfsError> {
            *self.aborts.lock().unwrap() += 1;
            Ok(())
        }
    }

    fn io_error(message: &str) -> VfsError {
        VfsError::Io {
            message: message.to_owned(),
        }
    }

    #[tokio::test]
    async fn drive_upload_uses_single_put_when_input_ends_below_threshold() {
        let client = MockUploadClient::default();
        let mut reader = &b"abc"[..];

        drive_upload(&client, 4, &mut reader).await.unwrap();

        assert_eq!(*client.put_bodies.lock().unwrap(), vec![b"abc".to_vec()]);
        assert!(client.uploaded_parts.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn drive_upload_uses_multipart_when_input_reaches_threshold() {
        let client = MockUploadClient::default();
        let mut reader = &b"abcde"[..];

        drive_upload(&client, 4, &mut reader).await.unwrap();

        assert!(client.put_bodies.lock().unwrap().is_empty());
        assert_eq!(
            *client.uploaded_parts.lock().unwrap(),
            vec![(1, b"abcde".to_vec())]
        );
        assert_eq!(
            *client.completions.lock().unwrap(),
            vec![vec!["etag-1".to_owned()]]
        );
    }

    #[tokio::test]
    async fn upload_parts_retries_a_transient_part_failure() {
        let client = MockUploadClient::with_upload_results(vec![
            Err(UploadPartError::retryable(io_error("temporary"))),
            Ok("etag-1".to_owned()),
        ]);
        let mut reader = &b""[..];

        let etags = upload_parts(&client, "upload-1", b"first".to_vec(), &mut reader)
            .await
            .unwrap();

        assert_eq!(etags, vec!["etag-1"]);
        assert_eq!(client.uploaded_parts.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn upload_parts_stops_after_a_permanent_part_failure() {
        let client = MockUploadClient::with_upload_results(vec![Err(UploadPartError::permanent(
            io_error("missing ETag"),
        ))]);
        let mut reader = &b""[..];

        let result = upload_parts(&client, "upload-1", b"first".to_vec(), &mut reader).await;

        assert!(matches!(result, Err(VfsError::Io { .. })));
        assert_eq!(client.uploaded_parts.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn multipart_upload_aborts_after_part_retries_are_exhausted() {
        let client = MockUploadClient::with_upload_results(vec![
            Err(UploadPartError::retryable(io_error("temporary 1"))),
            Err(UploadPartError::retryable(io_error("temporary 2"))),
            Err(UploadPartError::retryable(io_error("temporary 3"))),
        ]);
        let mut reader = &b""[..];

        let result = multipart_upload(&client, b"first".to_vec(), &mut reader).await;

        assert!(matches!(result, Err(VfsError::Io { .. })));
        assert_eq!(client.uploaded_parts.lock().unwrap().len(), 3);
        assert_eq!(*client.aborts.lock().unwrap(), 1);
        assert!(client.completions.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn multipart_upload_aborts_after_a_later_part_fails() {
        let client = MockUploadClient::with_upload_results(vec![
            Ok("etag-1".to_owned()),
            Err(UploadPartError::permanent(io_error("part 2 failed"))),
        ]);
        let mut reader = &b"second"[..];

        let result = multipart_upload(&client, b"first".to_vec(), &mut reader).await;

        assert!(matches!(result, Err(VfsError::Io { .. })));
        assert_eq!(
            *client.uploaded_parts.lock().unwrap(),
            vec![(1, b"first".to_vec()), (2, b"second".to_vec())]
        );
        assert_eq!(*client.aborts.lock().unwrap(), 1);
        assert!(client.completions.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn multipart_upload_aborts_when_completion_fails() {
        let client = MockUploadClient {
            completion_error: Mutex::new(Some(io_error("completion failed"))),
            ..MockUploadClient::default()
        };
        let mut reader = &b""[..];

        let result = multipart_upload(&client, b"first".to_vec(), &mut reader).await;

        assert!(matches!(result, Err(VfsError::Io { .. })));
        assert_eq!(
            *client.completions.lock().unwrap(),
            vec![vec!["etag-1".to_owned()]]
        );
        assert_eq!(*client.aborts.lock().unwrap(), 1);
    }
}
