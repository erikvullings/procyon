//! Streamed, cancellable byte-for-byte comparison of two readers (task 0256).
//!
//! Shared by the directory-comparison "by content" criterion and by the
//! application's two-file diff service, so both agree on what "identical" means and on how the
//! first differing offset is reported. Neither side is ever loaded into memory beyond one chunk.

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

/// Bytes read from each side per iteration.
const CHUNK_BYTES: usize = 64 * 1024;

/// Outcome of comparing two byte streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteComparison {
    /// Both streams yielded exactly the same bytes.
    Identical,
    /// The streams differ; `offset` is the zero-based offset of the first differing byte (or of
    /// the end of the shorter stream when one is a strict prefix of the other).
    DiffersAt {
        /// Zero-based offset of the first difference.
        offset: u64,
    },
}

/// Failure comparing two streams.
#[derive(Debug, thiserror::Error)]
pub enum ContentCompareError {
    /// The cancellation token fired before a verdict was reached.
    #[error("content comparison was cancelled")]
    Cancelled,
    /// Reading one of the streams failed.
    #[error("content comparison could not read a stream: {0}")]
    Io(#[from] std::io::Error),
}

/// Compares two readers byte for byte, stopping at the first difference.
///
/// # Errors
///
/// [`ContentCompareError::Cancelled`] when `cancellation` fires, [`ContentCompareError::Io`] on a
/// read failure.
pub async fn compare_streams<L, R>(
    mut left: L,
    mut right: R,
    cancellation: &CancellationToken,
) -> Result<ByteComparison, ContentCompareError>
where
    L: AsyncRead + Unpin,
    R: AsyncRead + Unpin,
{
    let mut left_buffer = vec![0_u8; CHUNK_BYTES];
    let mut right_buffer = vec![0_u8; CHUNK_BYTES];
    let mut offset: u64 = 0;
    loop {
        if cancellation.is_cancelled() {
            return Err(ContentCompareError::Cancelled);
        }
        let left_len = fill(&mut left, &mut left_buffer).await?;
        let right_len = fill(&mut right, &mut right_buffer).await?;
        let common = left_len.min(right_len);
        if let Some(index) = left_buffer[..common]
            .iter()
            .zip(&right_buffer[..common])
            .position(|(l, r)| l != r)
        {
            return Ok(ByteComparison::DiffersAt {
                offset: offset + index as u64,
            });
        }
        if left_len != right_len {
            return Ok(ByteComparison::DiffersAt {
                offset: offset + common as u64,
            });
        }
        if left_len == 0 {
            return Ok(ByteComparison::Identical);
        }
        offset += left_len as u64;
    }
}

/// Reads until `buffer` is full or the stream ends, so two readers with different chunking still
/// line up byte for byte.
async fn fill<T: AsyncRead + Unpin>(
    reader: &mut T,
    buffer: &mut [u8],
) -> Result<usize, std::io::Error> {
    let mut filled = 0;
    while filled < buffer.len() {
        let read = reader.read(&mut buffer[filled..]).await?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn compare(left: &[u8], right: &[u8]) -> ByteComparison {
        compare_streams(left, right, &CancellationToken::new())
            .await
            .expect("in-memory comparison must succeed")
    }

    #[tokio::test]
    async fn identical_streams_are_identical_including_empty_ones() {
        assert_eq!(compare(b"", b"").await, ByteComparison::Identical);
        assert_eq!(compare(b"abc", b"abc").await, ByteComparison::Identical);
    }

    #[tokio::test]
    async fn reports_the_first_differing_offset() {
        assert_eq!(
            compare(b"abcdef", b"abXdeY").await,
            ByteComparison::DiffersAt { offset: 2 }
        );
    }

    #[tokio::test]
    async fn a_strict_prefix_differs_at_the_shorter_length() {
        assert_eq!(
            compare(b"abc", b"abcde").await,
            ByteComparison::DiffersAt { offset: 3 }
        );
        assert_eq!(
            compare(b"abcde", b"").await,
            ByteComparison::DiffersAt { offset: 0 }
        );
    }

    #[tokio::test]
    async fn offsets_are_correct_across_chunk_boundaries() {
        let mut left = vec![7_u8; CHUNK_BYTES * 2 + 10];
        let mut right = left.clone();
        right[CHUNK_BYTES + 5] = 9;
        assert_eq!(
            compare(&left, &right).await,
            ByteComparison::DiffersAt {
                offset: (CHUNK_BYTES + 5) as u64
            }
        );
        left.truncate(CHUNK_BYTES);
        let right_prefix = vec![7_u8; CHUNK_BYTES];
        assert_eq!(
            compare(&left, &right_prefix).await,
            ByteComparison::Identical
        );
    }

    #[tokio::test]
    async fn a_cancelled_token_stops_the_comparison() {
        let token = CancellationToken::new();
        token.cancel();
        let result = compare_streams(&b"abc"[..], &b"abc"[..], &token).await;
        assert!(matches!(result, Err(ContentCompareError::Cancelled)));
    }
}
