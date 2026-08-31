//! Upload plumbing: bounded-memory streaming into Microsoft Graph, both a
//! plain single-request simple upload and a real resumable upload session
//! (task 0110's core write-path requirement).
//!
//! [`FileSystemProvider::open_write`](fm_vfs::FileSystemProvider::open_write)/
//! [`open_write_sized`](fm_vfs::FileSystemProvider::open_write_sized) hand
//! the caller one half of a [`tokio::io::duplex`] pipe immediately and drive
//! the actual Graph upload in a spawned task reading from the other half -
//! the same shape `fm_vfs_s3`/`fm_vfs_ftp` use. The difference that matters
//! here: [`ResultAwareWriter::poll_shutdown`] does not return success until
//! the background task's *real* outcome is known, so a failed remote upload
//! is reported to the caller as a failed `shutdown()`, never as a
//! success-shaped background failure nobody ever observes.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use fm_domain::Location;
use fm_vfs::{ProviderWriteStream, VfsError};
use reqwest::header::AUTHORIZATION;
use tokio::io::{AsyncReadExt, AsyncWrite, DuplexStream};
use tokio_util::sync::CancellationToken;

use crate::graph::{
    GraphConfig, Parsed, RetryClass, UploadSession, bearer_header_value, build_url, map_status,
    send_with_retry,
};
use crate::resolver::OneDriveConnectionResolver;

/// Byte capacity of the in-memory pipe between the caller's writes and the
/// background upload task - bounded regardless of the file's total size.
const DUPLEX_BUFFER_SIZE: usize = 256 * 1024;

/// Above this, [`fm_vfs::FileSystemProvider::open_write`] (unknown final
/// size) refuses rather than buffering an unbounded amount of memory (task
/// 0110: "without buffering arbitrary files in memory"). Matches
/// Microsoft's own "simple upload" guidance for files without a
/// pre-declared size. A caller that knows the size ahead of time should use
/// [`fm_vfs::FileSystemProvider::open_write_sized`] instead, which has no
/// such bound.
pub const SIMPLE_UPLOAD_THRESHOLD: u64 = 4 * 1024 * 1024;

/// Size of every non-final upload-session fragment: a multiple of 320 KiB
/// (Graph's hard requirement) and comfortably under the 60 MiB hard limit.
pub const UPLOAD_FRAGMENT_SIZE: u64 = 320 * 1024 * 10;

/// Spawns `task` against one half of a fresh duplex pipe and returns the
/// other half wrapped so that [`AsyncWrite::poll_shutdown`] waits for and
/// surfaces `task`'s real result.
fn spawn_writer<F, Fut>(task: F) -> ProviderWriteStream
where
    F: FnOnce(DuplexStream) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), VfsError>> + Send + 'static,
{
    let (writer, reader) = tokio::io::duplex(DUPLEX_BUFFER_SIZE);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let outcome = task(reader).await;
        let _ = result_tx.send(outcome);
    });
    Box::pin(ResultAwareWriter {
        inner: writer,
        outcome: Some(result_rx),
    })
}

/// Wraps a duplex pipe's write half so `shutdown()` reports the spawned
/// upload task's real outcome instead of only closing the local pipe.
struct ResultAwareWriter {
    inner: DuplexStream,
    /// `None` once a previous `poll_shutdown` has resolved with the real
    /// outcome; a further shutdown call is then a harmless no-op success,
    /// matching [`AsyncWrite`]'s convention that shutdown is idempotent.
    outcome: Option<tokio::sync::oneshot::Receiver<Result<(), VfsError>>>,
}

impl AsyncWrite for ResultAwareWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        // Closing the local pipe first is what lets the background task
        // observe EOF (or a short/oversized write) and finish.
        if Pin::new(&mut this.inner).poll_shutdown(cx).is_pending() {
            return Poll::Pending;
        }
        let Some(receiver) = this.outcome.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        match Pin::new(receiver).poll(cx) {
            Poll::Ready(Ok(Ok(()))) => {
                this.outcome = None;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Ok(Err(error))) => {
                this.outcome = None;
                Poll::Ready(Err(std::io::Error::other(error)))
            }
            Poll::Ready(Err(_)) => {
                this.outcome = None;
                Poll::Ready(Err(std::io::Error::other(
                    "the Microsoft Graph upload task ended without reporting a result",
                )))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Reads from `reader`, observing `cancellation` with equal priority so a
/// cancelled copy is reported as [`VfsError::Cancelled`] rather than a
/// misleadingly "clean" end-of-file (which would otherwise look
/// indistinguishable from the caller having legitimately finished writing).
async fn read_cancellably(
    reader: &mut DuplexStream,
    buf: &mut [u8],
    cancellation: &CancellationToken,
) -> Result<usize, VfsError> {
    tokio::select! {
        () = cancellation.cancelled() => Err(VfsError::Cancelled),
        result = reader.read(buf) => result.map_err(|error| VfsError::Io { message: error.to_string() }),
    }
}

/// Opens a destination for a write of unknown final size (task 0110's
/// honest fallback): buffers up to [`SIMPLE_UPLOAD_THRESHOLD`] bytes and, if
/// that turns out to be the whole payload, issues one simple upload.
/// Exceeding the bound without reaching end-of-file fails explicitly rather
/// than buffering further or silently truncating - callers that already
/// know the size should use [`open_write_sized`] instead, which has no such
/// limit.
pub(crate) fn open_write(
    http: reqwest::Client,
    config: GraphConfig,
    resolver: Arc<dyn OneDriveConnectionResolver>,
    destination: Location,
    cancellation: CancellationToken,
) -> ProviderWriteStream {
    spawn_writer(move |reader| {
        run_unbounded_upload(reader, http, config, resolver, destination, cancellation)
    })
}

/// Opens a destination for a write whose final size is already known
/// (task 0110's `open_write_sized` extension point). Below
/// [`SIMPLE_UPLOAD_THRESHOLD`] this still issues one simple upload (no
/// session overhead for a small file); at or above it, drives a real Graph
/// resumable upload session with bounded-memory, sequential chunks.
pub(crate) fn open_write_sized(
    http: reqwest::Client,
    config: GraphConfig,
    resolver: Arc<dyn OneDriveConnectionResolver>,
    destination: Location,
    expected_size: u64,
    cancellation: CancellationToken,
) -> ProviderWriteStream {
    if expected_size <= SIMPLE_UPLOAD_THRESHOLD {
        spawn_writer(move |reader| {
            run_simple_upload_sized(
                reader,
                http,
                config,
                resolver,
                destination,
                expected_size,
                cancellation,
            )
        })
    } else {
        spawn_writer(move |reader| {
            run_session_upload(
                reader,
                http,
                config,
                resolver,
                destination,
                expected_size,
                cancellation,
            )
        })
    }
}

async fn run_unbounded_upload(
    mut reader: DuplexStream,
    http: reqwest::Client,
    config: GraphConfig,
    resolver: Arc<dyn OneDriveConnectionResolver>,
    destination: Location,
    cancellation: CancellationToken,
) -> Result<(), VfsError> {
    let parsed = Parsed::parse(&destination)?;
    let mut buffer = Vec::new();
    let mut chunk = vec![0_u8; 64 * 1024];
    loop {
        let read = read_cancellably(&mut reader, &mut chunk, &cancellation).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() as u64 > SIMPLE_UPLOAD_THRESHOLD {
            return Err(VfsError::Io {
                message: format!(
                    "write exceeds the {SIMPLE_UPLOAD_THRESHOLD}-byte bound this provider allows for an upload of \
                     unknown size; a caller that knows the size ahead of time must use the sized write path instead"
                ),
            });
        }
    }
    put_simple_content(
        &http,
        &config,
        resolver.as_ref(),
        &parsed,
        &destination,
        buffer,
        &cancellation,
    )
    .await
}

async fn run_simple_upload_sized(
    mut reader: DuplexStream,
    http: reqwest::Client,
    config: GraphConfig,
    resolver: Arc<dyn OneDriveConnectionResolver>,
    destination: Location,
    expected_size: u64,
    cancellation: CancellationToken,
) -> Result<(), VfsError> {
    let parsed = Parsed::parse(&destination)?;
    let buffer = read_exact_bounded(&mut reader, expected_size, &cancellation).await?;
    put_simple_content(
        &http,
        &config,
        resolver.as_ref(),
        &parsed,
        &destination,
        buffer,
        &cancellation,
    )
    .await
}

/// Reads exactly `expected_size` bytes from `reader`, then confirms no
/// further bytes follow - an honest enforcement of the size contract
/// [`fm_vfs::FileSystemProvider::open_write_sized`] callers agree to.
async fn read_exact_bounded(
    reader: &mut DuplexStream,
    expected_size: u64,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, VfsError> {
    let mut buffer = Vec::with_capacity(expected_size.min(64 * 1024 * 1024) as usize);
    let mut remaining = expected_size;
    while remaining > 0 {
        let mut chunk = vec![0_u8; remaining.min(64 * 1024) as usize];
        let read = read_cancellably(reader, &mut chunk, cancellation).await?;
        if read == 0 {
            return Err(short_write_error());
        }
        buffer.extend_from_slice(&chunk[..read]);
        remaining -= read as u64;
    }
    ensure_no_further_bytes(reader, cancellation).await?;
    Ok(buffer)
}

async fn ensure_no_further_bytes(
    reader: &mut DuplexStream,
    cancellation: &CancellationToken,
) -> Result<(), VfsError> {
    let mut probe = [0_u8; 1];
    if read_cancellably(reader, &mut probe, cancellation).await? != 0 {
        return Err(oversized_write_error());
    }
    Ok(())
}

fn short_write_error() -> VfsError {
    VfsError::Io {
        message: "write ended before reaching the declared expected size".to_owned(),
    }
}

fn oversized_write_error() -> VfsError {
    VfsError::Io {
        message: "write exceeded the declared expected size".to_owned(),
    }
}

async fn put_simple_content(
    http: &reqwest::Client,
    config: &GraphConfig,
    resolver: &dyn OneDriveConnectionResolver,
    parsed: &Parsed,
    destination: &Location,
    content: Vec<u8>,
    cancellation: &CancellationToken,
) -> Result<(), VfsError> {
    let token = resolver.resolve(&parsed.connection_id).await?;
    let url = build_url(config, &parsed.content_relative_path())?;
    let response = send_with_retry(
        || {
            http.put(url.clone())
                .header(AUTHORIZATION, bearer_header_value(&token))
                .body(content.clone())
        },
        RetryClass::Idempotent,
        &config.retry,
        cancellation,
    )
    .await?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(map_status(status, &destination.uri))
    }
}

async fn run_session_upload(
    mut reader: DuplexStream,
    http: reqwest::Client,
    config: GraphConfig,
    resolver: Arc<dyn OneDriveConnectionResolver>,
    destination: Location,
    expected_size: u64,
    cancellation: CancellationToken,
) -> Result<(), VfsError> {
    let parsed = Parsed::parse(&destination)?;
    let token = resolver.resolve(&parsed.connection_id).await?;

    let create_url = build_url(&config, &parsed.create_upload_session_relative_path())?;
    let request_body = serde_json::json!({
        "item": { "@microsoft.graph.conflictBehavior": "replace" },
    });
    let response = send_with_retry(
        || {
            http.post(create_url.clone())
                .header(AUTHORIZATION, bearer_header_value(&token))
                .json(&request_body)
        },
        // A session-create failure is *not* resent after a bare transport
        // failure: this genuinely creates a new server-side resource
        // rather than replacing existing state, so whether the first
        // attempt secretly succeeded is unknown - only Graph's own
        // explicit 429/503 instruction is honoured here (an unused,
        // auto-expiring orphaned session is a harmless, bounded cost the
        // 429/503 case does not share, since that status means the
        // request was not processed at all).
        RetryClass::NonIdempotent,
        &config.retry,
        &cancellation,
    )
    .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(map_status(status, &destination.uri));
    }
    let session: UploadSession = response.json().await.map_err(|_| unparseable_response())?;

    let result = drive_chunks(
        &mut reader,
        &http,
        &session.upload_url,
        expected_size,
        &destination.uri,
        &config,
        &cancellation,
    )
    .await;

    if result.is_err() {
        // Best-effort: an unfinished upload session eventually expires on
        // its own, but proactively cancelling means no trace of a
        // cancelled/failed transfer lingers server-side in the meantime,
        // and the destination never appears to exist in a partial state
        // (task 0110). Its own failure is deliberately swallowed - there is
        // no further recovery action available, and surfacing it would
        // shadow the real error below.
        let _ = http.delete(&session.upload_url).send().await;
    }
    result
}

fn unparseable_response() -> VfsError {
    VfsError::Io {
        message: "Microsoft Graph returned a response this provider could not parse".to_owned(),
    }
}

/// Streams `reader` to `upload_url` (preauthenticated - **no** bearer is
/// ever attached here) as sequential, bounded-memory fragments, each a
/// multiple of 320 KiB except the final one, honouring `expected_size` as
/// the immutable declared total for every `Content-Range` header.
///
/// Before the *final* fragment is ever sent, this confirms the reader has
/// no more bytes waiting beyond `expected_size` - catching an oversized
/// write while the upload session is still incomplete, so a caller that
/// violated the declared size never gets a completed, wrongly-truncated
/// file published on the server (task 0110 review: the final chunk must
/// never be sent once excess data is detected, not merely reported as an
/// error after the fact).
async fn drive_chunks(
    reader: &mut DuplexStream,
    http: &reqwest::Client,
    upload_url: &str,
    expected_size: u64,
    location_text: &str,
    config: &GraphConfig,
    cancellation: &CancellationToken,
) -> Result<(), VfsError> {
    let mut sent: u64 = 0;
    loop {
        let remaining = expected_size - sent;
        if remaining == 0 {
            ensure_no_further_bytes(reader, cancellation).await?;
            return Ok(());
        }
        let chunk_len = remaining.min(UPLOAD_FRAGMENT_SIZE);
        let mut chunk = vec![0_u8; chunk_len as usize];
        let mut filled = 0_usize;
        while (filled as u64) < chunk_len {
            let read = read_cancellably(reader, &mut chunk[filled..], cancellation).await?;
            if read == 0 {
                return Err(short_write_error());
            }
            filled += read;
        }
        let start = sent;
        let end = sent + chunk_len - 1;
        let is_final = end + 1 == expected_size;
        if is_final {
            // Confirm end-of-file *before* this chunk (which would
            // otherwise complete the upload) is ever sent. Reading exactly
            // up to `expected_size` bytes does not by itself prove the
            // caller is done - only the absence of anything further does.
            ensure_no_further_bytes(reader, cancellation).await?;
        }
        let content_range = format!("bytes {start}-{end}/{expected_size}");
        // Deliberately **no** `Authorization` header: `upload_url` is
        // preauthenticated by Graph itself (task 0110's explicit
        // constraint - attaching the bearer here would be sending it to a
        // URL this provider does not control the ultimate destination of
        // once issued).
        let response = send_with_retry(
            || {
                http.put(upload_url)
                    .header(reqwest::header::CONTENT_RANGE, content_range.clone())
                    .body(chunk.clone())
            },
            RetryClass::Idempotent,
            &config.retry,
            cancellation,
        )
        .await?;
        let status = response.status();
        if is_final {
            if !status.is_success() {
                return Err(map_status(status, location_text));
            }
            return Ok(());
        }
        if status != reqwest::StatusCode::ACCEPTED {
            return Err(map_status(status, location_text));
        }
        sent += chunk_len;
    }
}
