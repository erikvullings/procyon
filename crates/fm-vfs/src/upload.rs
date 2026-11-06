//! A writer whose `shutdown` reports whether the upload behind it actually succeeded.
//!
//! Remote providers (S3, WebDAV, ...) turn an `AsyncWrite` into an HTTP request by piping the
//! written bytes into a task that performs the upload. A bare duplex pipe can only say "the
//! bytes left my buffer": `shutdown` returned before the server had accepted anything, an upload
//! the server refused was never reported, and the operation engine's next step (publishing the
//! temporary) raced the upload. [`upload_pipe`] closes that gap: the returned writer's
//! `shutdown` waits for the task's verdict and fails with it, and a write that hits a dead pipe
//! fails with the upload's own error instead of a bare "broken pipe". A writer dropped WITHOUT
//! `shutdown` (a cancelled or failed copy) is not a clean end of the body: the reader the upload
//! consumes fails instead of reporting end-of-file, so a half-written body is never completed as
//! if it were whole.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use futures::channel::oneshot;
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};

use crate::{ProviderWriteStream, VfsError};

/// Handed to the upload task so it can report how the upload ended.
pub struct UploadCompletion(oneshot::Sender<Result<(), VfsError>>);

impl UploadCompletion {
    /// Reports the upload's outcome to the writer. Call exactly once, when the task is done.
    pub fn finish(self, result: Result<(), VfsError>) {
        // The writer may already be gone (a cancelled copy drops it); nobody is waiting then.
        let _ = self.0.send(result);
    }
}

/// The body the upload task reads: the bytes written, ending cleanly only when the writer was
/// shut down. If the writer is dropped first, the read fails with `Interrupted` at the point
/// where end-of-file would have been.
pub struct UploadBody {
    inner: DuplexStream,
    finished: Arc<AtomicBool>,
}

impl AsyncRead for UploadBody {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buffer.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buffer) {
            Poll::Ready(Ok(())) if buffer.filled().len() == before => {
                if self.finished.load(Ordering::Acquire) {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "the upload was abandoned before its body was complete",
                    )))
                }
            }
            other => other,
        }
    }
}

/// Creates the write half callers hand out, the body the upload task consumes, and the
/// completion handle the task reports through.
#[must_use]
pub fn upload_pipe(buffer: usize) -> (ProviderWriteStream, UploadBody, UploadCompletion) {
    let (writer, reader) = tokio::io::duplex(buffer);
    let (sender, receiver) = oneshot::channel();
    let finished = Arc::new(AtomicBool::new(false));
    (
        Box::pin(AwaitedWriter {
            inner: writer,
            outcome: Some(receiver),
            finished: Arc::clone(&finished),
        }),
        UploadBody {
            inner: reader,
            finished,
        },
        UploadCompletion(sender),
    )
}

struct AwaitedWriter {
    inner: DuplexStream,
    outcome: Option<oneshot::Receiver<Result<(), VfsError>>>,
    finished: Arc<AtomicBool>,
}

fn io_error(error: &VfsError) -> io::Error {
    let kind = match error {
        VfsError::PermissionDenied { .. } | VfsError::AuthenticationFailed { .. } => {
            io::ErrorKind::PermissionDenied
        }
        VfsError::NotFound { .. } => io::ErrorKind::NotFound,
        VfsError::Cancelled => io::ErrorKind::Interrupted,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error.to_string())
}

impl AwaitedWriter {
    /// A dead pipe means the upload task stopped reading. Wait for the reason the task reports
    /// (a server that rejects a request early drops the body before it says why), and return the
    /// original error only when the task reports nothing useful.
    fn explain(&mut self, error: io::Error, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        if error.kind() == io::ErrorKind::BrokenPipe
            && let Some(outcome) = self.outcome.as_mut()
        {
            match Pin::new(outcome).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(Err(reason))) => {
                    self.outcome = None;
                    return Poll::Ready(Err(io_error(&reason)));
                }
                Poll::Ready(_) => self.outcome = None,
            }
        }
        Poll::Ready(Err(error))
    }
}

impl AsyncWrite for AwaitedWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_write(cx, buffer) {
            Poll::Ready(Err(error)) => self.explain(error, cx),
            other => other,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match Pin::new(&mut self.inner).poll_flush(cx) {
            Poll::Ready(Err(error)) => self.explain(error, cx).map(|result| result.map(|_| ())),
            other => other,
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Close the pipe so the upload sees end-of-body (a clean one: only a shutdown makes the
        // body end without an error), then wait for its verdict.
        self.finished.store(true, Ordering::Release);
        match Pin::new(&mut self.inner).poll_shutdown(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(error)) => {
                return self.explain(error, cx).map(|result| result.map(|_| ()));
            }
            Poll::Pending => return Poll::Pending,
        }
        let Some(outcome) = self.outcome.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        match Pin::new(outcome).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(Ok(()))) => {
                self.outcome = None;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Ok(Err(reason))) => {
                self.outcome = None;
                Poll::Ready(Err(io_error(&reason)))
            }
            Poll::Ready(Err(_canceled)) => {
                self.outcome = None;
                Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "the upload ended without reporting a result",
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn shutdown_waits_for_the_upload_to_finish() {
        let (mut writer, mut reader, completion) = upload_pipe(8);
        let uploader = tokio::spawn(async move {
            let mut body = Vec::new();
            reader.read_to_end(&mut body).await.expect("read body");
            // The server takes a moment to answer after the body ends.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            completion.finish(Ok(()));
            body
        });
        writer.write_all(b"payload").await.expect("write");
        let started = std::time::Instant::now();
        writer.shutdown().await.expect("shutdown reports success");
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(40),
            "shutdown must not return before the upload's verdict"
        );
        assert_eq!(uploader.await.expect("uploader"), b"payload");
    }

    #[tokio::test]
    async fn a_refused_upload_fails_shutdown_with_its_own_error() {
        let (mut writer, mut reader, completion) = upload_pipe(8);
        tokio::spawn(async move {
            let mut sink = Vec::new();
            let _ = reader.read_to_end(&mut sink).await;
            completion.finish(Err(VfsError::PermissionDenied {
                location: "dav://x/file".into(),
            }));
        });
        writer.write_all(b"data").await.expect("write");
        let error = writer.shutdown().await.expect_err("the server refused it");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("permission denied"));
    }

    #[tokio::test]
    async fn an_upload_that_dies_early_explains_itself_on_the_next_write() {
        let (mut writer, reader, completion) = upload_pipe(4);
        drop(reader);
        completion.finish(Err(VfsError::Io {
            message: "connection reset".into(),
        }));
        let error = loop {
            match writer.write_all(&[0_u8; 64]).await {
                Ok(()) => tokio::task::yield_now().await,
                Err(error) => break error,
            }
        };
        assert!(
            error.to_string().contains("connection reset"),
            "not a bare broken pipe: {error}"
        );
    }

    #[tokio::test]
    async fn a_writer_dropped_without_shutdown_aborts_the_body_instead_of_ending_it() {
        let (mut writer, mut body, _completion) = upload_pipe(8);
        writer.write_all(b"half").await.expect("write");
        drop(writer);
        let mut read = Vec::new();
        let error = body
            .read_to_end(&mut read)
            .await
            .expect_err("a body that was never finished must not look complete");
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(
            read, b"half",
            "what arrived is still delivered before the failure"
        );
    }

    #[tokio::test]
    async fn a_clean_shutdown_ends_the_body_normally() {
        let (mut writer, mut body, completion) = upload_pipe(8);
        let uploader = tokio::spawn(async move {
            let mut read = Vec::new();
            let result = body.read_to_end(&mut read).await;
            completion.finish(Ok(()));
            (result.is_ok(), read)
        });
        writer.write_all(b"whole").await.expect("write");
        writer.shutdown().await.expect("shutdown");
        assert_eq!(uploader.await.expect("uploader"), (true, b"whole".to_vec()));
    }

    #[tokio::test]
    async fn a_server_that_rejects_early_still_explains_itself_to_the_writer() {
        let (mut writer, body, completion) = upload_pipe(4);
        tokio::spawn(async move {
            // The request is refused before the body is read: the body is dropped first and
            // the reason follows a moment later.
            drop(body);
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            completion.finish(Err(VfsError::PermissionDenied {
                location: "dav://x/file".into(),
            }));
        });
        let error = loop {
            match writer.write_all(&[0_u8; 64]).await {
                Ok(()) => tokio::task::yield_now().await,
                Err(error) => break error,
            }
        };
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "got {error}");
    }

    #[tokio::test]
    async fn a_task_that_vanishes_without_a_result_is_an_error_not_a_success() {
        let (mut writer, mut reader, completion) = upload_pipe(8);
        tokio::spawn(async move {
            let mut sink = Vec::new();
            let _ = reader.read_to_end(&mut sink).await;
            drop(completion);
        });
        writer.write_all(b"data").await.expect("write");
        let error = writer
            .shutdown()
            .await
            .expect_err("no verdict means no success");
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}
