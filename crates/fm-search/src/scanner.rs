//! Bounded, per-file content scanning for recursive search (task 0089).
//!
//! Reuses [`fm_vfs::content`] for the actual line-by-line scan and
//! binary sniff. Adds per-file byte and time limits so one huge file
//! cannot stall a multi-root search.

use fm_vfs::{ContentMatch, ContentQuery, looks_like_binary, search_content};
use std::io;
use std::path::Path;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::io::BufReader;
use tokio_util::sync::CancellationToken;

/// Maximum bytes to scan per file during recursive content search.
const MAX_SCAN_BYTES: u64 = 10 * 1024 * 1024; // 10 MiB
/// Maximum time to spend scanning a single file.
const MAX_SCAN_DURATION: Duration = Duration::from_millis(200);
/// Bytes to read for binary sniff.
const SNIFF_BYTES: usize = 8192;

/// Outcome of scanning one file's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileScanResult {
    /// Ordered by line, then offset within line.
    pub matches: Vec<ContentMatch>,
}

/// Errors during a single-file content scan.
#[derive(Debug, thiserror::Error)]
pub enum FileScanError {
    /// The file appeared binary (NUL bytes in first chunk).
    #[error("file is binary")]
    BinaryFile,
    /// The file exceeds the per-file scan budget.
    #[error("file too large ({0} bytes > {MAX_SCAN_BYTES} limit)")]
    FileTooLarge(u64),
    /// I/O error during file access.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    /// The scan was cancelled before completion.
    #[error("scan cancelled")]
    Cancelled,
}

/// Scans a single local file's contents for `query`, bounded by
/// `MAX_SCAN_BYTES` and `MAX_SCAN_DURATION`.
///
/// Returns `Err(FileScanError::BinaryFile)` when the file appears binary,
/// `Err(FileScanError::FileTooLarge)` when the file exceeds the byte budget.
pub async fn scan_file(
    path: &Path,
    query: &ContentQuery,
    cancellation: &CancellationToken,
) -> Result<FileScanResult, FileScanError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.len() > MAX_SCAN_BYTES {
        return Err(FileScanError::FileTooLarge(metadata.len()));
    }

    // Sniff first chunk for binary.
    let file = File::open(path).await?;
    let mut buf_reader = BufReader::new(file);
    let mut sniff_buf = vec![0_u8; SNIFF_BYTES];
    let sniffed = buf_reader.read(&mut sniff_buf).await?;
    let sniff = &sniff_buf[..sniffed];
    if sniff.is_empty() {
        return Ok(FileScanResult {
            matches: Vec::new(),
        });
    }
    if looks_like_binary(sniff) {
        return Err(FileScanError::BinaryFile);
    }

    // Re-open and scan the full file (already bounded by size check above).
    let file = File::open(path).await?;
    let scan_result = tokio::time::timeout(
        MAX_SCAN_DURATION,
        search_content(file, query, 500, cancellation),
    )
    .await
    .map_err(|_| FileScanError::Cancelled)?
    .map_err(|_| FileScanError::Io(io::Error::other("vfs error")))?;

    Ok(FileScanResult {
        matches: scan_result.matches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn finds_plain_substring_matches() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("test.txt"),
            b"alpha\nneedle\nbeta\nneedle again\n",
        )
        .unwrap();

        let query = ContentQuery::new("needle", false, false, false).unwrap();
        let result = scan_file(
            dir.path().join("test.txt").as_ref(),
            &query,
            &CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].line_number, 2);
        assert_eq!(result.matches[1].line_number, 4);
    }

    #[tokio::test]
    async fn finds_case_insensitive_matches() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.txt"), b"NEEDLE\nneedle\nNeedLe\n").unwrap();

        let query = ContentQuery::new("needle", false, false, false).unwrap();
        let result = scan_file(
            dir.path().join("test.txt").as_ref(),
            &query,
            &CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(result.matches.len(), 3);
    }

    #[tokio::test]
    async fn finds_regex_matches() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("test.txt"),
            b"error: foo\ninfo: bar\nerror: baz\n",
        )
        .unwrap();

        let query = ContentQuery::new("error.*", true, false, false).unwrap();
        let result = scan_file(
            dir.path().join("test.txt").as_ref(),
            &query,
            &CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].line_number, 1);
        assert_eq!(result.matches[1].line_number, 3);
    }

    #[tokio::test]
    async fn empty_file_returns_no_matches() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("empty.txt"), b"").unwrap();

        let query = ContentQuery::new("anything", false, false, false).unwrap();
        let result = scan_file(
            dir.path().join("empty.txt").as_ref(),
            &query,
            &CancellationToken::new(),
        )
        .await
        .unwrap();

        assert!(result.matches.is_empty());
    }

    #[tokio::test]
    async fn binary_file_with_nul_bytes_is_skipped() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("binary.bin"), b"\x00\x01\x02needle\x03\x04").unwrap();

        let query = ContentQuery::new("needle", false, false, false).unwrap();
        let result = scan_file(
            dir.path().join("binary.bin").as_ref(),
            &query,
            &CancellationToken::new(),
        )
        .await;

        assert!(matches!(result, Err(FileScanError::BinaryFile)));
    }

    #[tokio::test]
    async fn utf16_file_is_not_treated_as_binary() {
        let dir = tempdir().unwrap();
        // UTF-16 LE BOM
        fs::write(dir.path().join("utf16.txt"), [0xFF, 0xFE, b'n', 0, b'e', 0]).unwrap();

        let query = ContentQuery::new("needle", false, false, false).unwrap();
        // Should not be flagged as binary (BOM detected), but may return no matches since
        // the scanner treats it as a byte stream.
        let result = scan_file(
            dir.path().join("utf16.txt").as_ref(),
            &query,
            &CancellationToken::new(),
        )
        .await;

        assert!(!matches!(result, Err(FileScanError::BinaryFile)));
    }

    #[tokio::test]
    async fn file_exceeding_size_limit_is_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("huge.bin");
        // Create a sparse file larger than MAX_SCAN_BYTES
        let f = fs::File::create(&path).unwrap();
        f.set_len(MAX_SCAN_BYTES + 1).unwrap();
        drop(f);

        let query = ContentQuery::new("x", false, false, false).unwrap();
        let result = scan_file(&path, &query, &CancellationToken::new()).await;

        assert!(matches!(result, Err(FileScanError::FileTooLarge(_))));
    }

    #[tokio::test]
    async fn cancellation_stops_scan() {
        let dir = tempdir().unwrap();
        // Write many lines so scan takes time
        let mut content = String::new();
        for i in 0..10_000 {
            content.push_str(&format!("line {} needle\n", i));
        }
        fs::write(dir.path().join("large.txt"), content.into_bytes()).unwrap();

        let query = ContentQuery::new("needle", false, false, false).unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let result = scan_file(dir.path().join("large.txt").as_ref(), &query, &cancellation).await;

        // Should either cancel or return partial results
        assert!(
            matches!(result, Err(FileScanError::Cancelled))
                || result
                    .as_ref()
                    .map(|r| r.matches.len() < 10_000)
                    .unwrap_or(false),
            "scan must respond to cancellation"
        );
    }

    #[tokio::test]
    async fn multiple_matches_on_same_line_are_returned() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("test.txt"),
            b"needle needle needle\nother\n",
        )
        .unwrap();

        let query = ContentQuery::new("needle", false, false, false).unwrap();
        let result = scan_file(
            dir.path().join("test.txt").as_ref(),
            &query,
            &CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(
            result.matches.len(),
            3,
            "must find all three occurrences on the same line"
        );
        assert_eq!(
            result
                .matches
                .iter()
                .map(|m| m.line_number)
                .collect::<Vec<_>>(),
            vec![1, 1, 1],
            "all should be on line 1"
        );
    }

    #[tokio::test]
    async fn non_ascii_content_matches_correctly() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("test.txt"),
            "line A\ncafé au lait\nline C\n\",\nline E\n".as_bytes(),
        )
        .unwrap();

        let query = ContentQuery::new("café", false, false, false).unwrap();
        let result = scan_file(
            dir.path().join("test.txt").as_ref(),
            &query,
            &CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].line_number, 2);
    }
}
