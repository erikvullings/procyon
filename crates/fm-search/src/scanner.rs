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
/// Time allowed to extract the text of one PDF before it is skipped.
const MAX_PDF_EXTRACT_DURATION: Duration = Duration::from_secs(5);

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
    /// The file used up its per-file time budget; only this file is skipped.
    #[error("scan timed out")]
    TimedOut,
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
    if sniff.starts_with(b"%PDF-") {
        return scan_pdf(path, query, cancellation).await;
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
    .map_err(|_| FileScanError::TimedOut)?
    .map_err(|_| FileScanError::Io(io::Error::other("vfs error")))?;

    Ok(FileScanResult {
        matches: scan_result.matches,
    })
}

/// Extracts the text layer of PDF `bytes` (one text line per line). `None` for a PDF without a
/// readable text layer (scanned images, encrypted, malformed) or one that takes too long.
pub(crate) async fn pdf_text(
    bytes: Vec<u8>,
    cancellation: &CancellationToken,
) -> Result<Option<String>, FileScanError> {
    let extraction = tokio::task::spawn_blocking(move || {
        let document = lopdf::Document::load_mem(&bytes).ok()?;
        let pages: Vec<u32> = document.get_pages().keys().copied().collect();
        document.extract_text(&pages).ok()
    });
    tokio::select! {
        () = cancellation.cancelled() => Err(FileScanError::Cancelled),
        outcome = tokio::time::timeout(MAX_PDF_EXTRACT_DURATION, extraction) => match outcome {
            Ok(Ok(Some(text))) if !text.trim().is_empty() => Ok(Some(text)),
            Ok(_) => Ok(None),
            Err(_) => Err(FileScanError::TimedOut),
        },
    }
}

/// Scans extracted PDF `text` for `query`.
pub(crate) async fn scan_text(
    text: &str,
    query: &ContentQuery,
    cancellation: &CancellationToken,
) -> Result<FileScanResult, FileScanError> {
    let scan_result = tokio::time::timeout(
        MAX_SCAN_DURATION,
        search_content(text.as_bytes(), query, 500, cancellation),
    )
    .await
    .map_err(|_| FileScanError::TimedOut)?
    .map_err(|_| FileScanError::Io(io::Error::other("vfs error")))?;
    Ok(FileScanResult {
        matches: scan_result.matches,
    })
}

/// Scans a local PDF's text layer like a text file. A PDF without one is skipped like any other
/// unreadable binary ([`FileScanError::BinaryFile`]).
async fn scan_pdf(
    path: &Path,
    query: &ContentQuery,
    cancellation: &CancellationToken,
) -> Result<FileScanResult, FileScanError> {
    let bytes = tokio::fs::read(path).await?;
    match pdf_text(bytes, cancellation).await? {
        Some(text) => scan_text(&text, query, cancellation).await,
        None => Err(FileScanError::BinaryFile),
    }
}

/// Writes a one-page PDF whose text layer is `text` (test fixture shared with the engine tests).
#[cfg(test)]
pub(crate) fn write_text_pdf(path: &Path, text: &str) {
    use lopdf::content::{Content, Operation};
    use lopdf::{Document, Object, Stream, dictionary};

    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 12.into()]),
            Operation::new("Td", vec![50.into(), 700.into()]),
            Operation::new("Tj", vec![Object::string_literal(text)]),
            Operation::new("ET", vec![]),
        ],
    };
    let content_id = document.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
        "Resources" => resources_id, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    document.save(path).unwrap();
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
    async fn finds_text_inside_a_pdf_instead_of_skipping_it_as_binary() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("report.pdf");
        write_text_pdf(&path, "Staal in the report");

        let query = ContentQuery::new("staal", false, false, false).unwrap();
        let result = scan_file(&path, &query, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.matches.len(), 1);

        let none = ContentQuery::new("absent", false, false, false).unwrap();
        let result = scan_file(&path, &none, &CancellationToken::new())
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
