//! Read-only two-file diff capability (task 0256).
//!
//! Text files (bounded by [`MAX_DIFF_FILE_BYTES`] and [`MAX_DIFF_LINES`]) are aligned into
//! side-by-side rows with intra-line highlights. Binary files are compared as streams and report
//! only identical/different plus the first differing offset. Oversized text files are reported
//! as such instead of being diffed. Nothing is written.

use crate::rejection::{Rejection, reason};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fm_comparison::{ByteComparison, ContentCompareError, compare_streams};
use fm_domain::{EntryId, EntryKind, Location};
use fm_transport_dto::{
    DiffFilesRequestDto, DiffFilesResponseDto, DiffRowDto, DiffRowKindDto, DiffSegmentDto,
    FileDiffOutcomeDto,
};
use fm_vfs::{EntryRef, FileSystemProvider, ProviderCapabilities, ProviderRegistry};
use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;

/// Largest text file, per side, that is diffed.
pub(crate) const MAX_DIFF_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// Largest line count, per side, that is diffed.
pub(crate) const MAX_DIFF_LINES: usize = 100_000;
/// Bytes sampled to decide whether a file that is too large to read whole is binary.
const SNIFF_BYTES: usize = 8 * 1024;
/// Upper bound on the time spent aligning one pair of files; beyond it the diff degrades to a
/// coarser (still correct) alignment.
const DIFF_DEADLINE: Duration = Duration::from_secs(5);

/// Computes read-only diffs between two files.
pub(crate) struct FileDiffService {
    providers: ProviderRegistry,
}

struct Side {
    provider: Arc<dyn FileSystemProvider>,
    entry: EntryRef,
    size: u64,
}

#[derive(Clone, Copy)]
struct DiffOptions {
    ignore_whitespace: bool,
    ignore_case: bool,
}

impl FileDiffService {
    pub(crate) fn new(providers: ProviderRegistry) -> Self {
        Self { providers }
    }

    pub(crate) async fn diff(
        &self,
        request: DiffFilesRequestDto,
    ) -> Result<DiffFilesResponseDto, ApplicationError> {
        let options = DiffOptions {
            ignore_whitespace: request.ignore_whitespace,
            ignore_case: request.ignore_case,
        };
        // Dropping this future (a disconnected client, a cancelled invoke) cancels the reads.
        let cancellation = CancellationToken::new();
        let _guard = cancellation.clone().drop_guard();
        let left = self.open_side(request.left.into(), &cancellation).await?;
        let right = self.open_side(request.right.into(), &cancellation).await?;

        let both_within_limit =
            left.size <= MAX_DIFF_FILE_BYTES && right.size <= MAX_DIFF_FILE_BYTES;
        if both_within_limit {
            let left_bytes = read_all(&left, &cancellation).await?;
            let right_bytes = read_all(&right, &cancellation).await?;
            if fm_vfs::looks_like_binary(&left_bytes) || fm_vfs::looks_like_binary(&right_bytes) {
                return Ok(binary_response(
                    &left,
                    &right,
                    first_difference(&left_bytes, &right_bytes),
                ));
            }
            return tokio::task::spawn_blocking(move || {
                text_response(left.size, right.size, &left_bytes, &right_bytes, options)
            })
            .await
            .map_err(|error| {
                ApplicationError::from(
                    Rejection::new(
                        reason::DIFF_FAILED,
                        format!("the diff task failed: {error}"),
                    )
                    .with("detail", &error),
                )
            })?;
        }

        let left_binary = fm_vfs::looks_like_binary(&sniff(&left, &cancellation).await?);
        let right_binary = fm_vfs::looks_like_binary(&sniff(&right, &cancellation).await?);
        if !(left_binary || right_binary) {
            return Ok(DiffFilesResponseDto {
                outcome: FileDiffOutcomeDto::TooLarge,
                left_size: left.size,
                right_size: right.size,
                identical: false,
                first_difference_offset: None,
                size_limit_bytes: MAX_DIFF_FILE_BYTES,
                line_limit: line_limit(),
                rows: Vec::new(),
                change_starts: Vec::new(),
            });
        }
        let left_stream = open_stream(&left, &cancellation).await?;
        let right_stream = open_stream(&right, &cancellation).await?;
        let offset = match compare_streams(left_stream, right_stream, &cancellation).await {
            Ok(ByteComparison::Identical) => None,
            Ok(ByteComparison::DiffersAt { offset }) => Some(offset),
            Err(ContentCompareError::Cancelled) => {
                return Err(ApplicationError::OperationCancelled);
            }
            Err(ContentCompareError::Io(error)) => {
                return Err(crate::file_editor::read_stream_error(error));
            }
        };
        Ok(binary_response(&left, &right, offset))
    }

    async fn open_side(
        &self,
        location: Location,
        cancellation: &CancellationToken,
    ) -> Result<Side, ApplicationError> {
        let provider = self.providers.resolve(&location)?;
        provider
            .capabilities_for(&location)?
            .require(ProviderCapabilities::READ)?;
        let entry = EntryRef {
            id: EntryId::new(),
            location,
        };
        let summary = provider.inspect(&entry, cancellation.clone()).await?;
        if summary.kind != EntryKind::File {
            return Err(ApplicationError::from(Rejection::new(
                reason::NOT_REGULAR_FILE,
                "only regular files can be compared",
            )));
        }
        let size = provider.file_size(&entry, cancellation.clone()).await?;
        Ok(Side {
            provider,
            entry,
            size,
        })
    }
}

fn line_limit() -> u32 {
    u32::try_from(MAX_DIFF_LINES).unwrap_or(u32::MAX)
}

async fn open_stream(
    side: &Side,
    cancellation: &CancellationToken,
) -> Result<fm_vfs::ProviderReadStream, ApplicationError> {
    Ok(side
        .provider
        .open_read(&side.entry, cancellation.clone())
        .await?)
}

async fn read_all(
    side: &Side,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, ApplicationError> {
    let mut reader = open_stream(side, cancellation).await?;
    let mut bytes = Vec::with_capacity(usize::try_from(side.size).unwrap_or(0));
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(crate::file_editor::read_stream_error)?;
    Ok(bytes)
}

async fn sniff(side: &Side, cancellation: &CancellationToken) -> Result<Vec<u8>, ApplicationError> {
    let reader = open_stream(side, cancellation).await?;
    let mut sample = Vec::with_capacity(SNIFF_BYTES);
    reader
        .take(SNIFF_BYTES as u64)
        .read_to_end(&mut sample)
        .await
        .map_err(crate::file_editor::read_stream_error)?;
    Ok(sample)
}

fn first_difference(left: &[u8], right: &[u8]) -> Option<u64> {
    let common = left.len().min(right.len());
    let offset = left[..common]
        .iter()
        .zip(&right[..common])
        .position(|(l, r)| l != r)
        .or((left.len() != right.len()).then_some(common))?;
    Some(offset as u64)
}

fn binary_response(left: &Side, right: &Side, offset: Option<u64>) -> DiffFilesResponseDto {
    DiffFilesResponseDto {
        outcome: FileDiffOutcomeDto::Binary,
        left_size: left.size,
        right_size: right.size,
        identical: offset.is_none(),
        first_difference_offset: offset,
        size_limit_bytes: MAX_DIFF_FILE_BYTES,
        line_limit: line_limit(),
        rows: Vec::new(),
        change_starts: Vec::new(),
    }
}

fn text_response(
    left_size: u64,
    right_size: u64,
    left_bytes: &[u8],
    right_bytes: &[u8],
    options: DiffOptions,
) -> Result<DiffFilesResponseDto, ApplicationError> {
    let left_text = String::from_utf8_lossy(left_bytes);
    let right_text = String::from_utf8_lossy(right_bytes);
    let left_lines: Vec<&str> = left_text.lines().collect();
    let right_lines: Vec<&str> = right_text.lines().collect();
    let mut response = DiffFilesResponseDto {
        outcome: FileDiffOutcomeDto::Text,
        left_size,
        right_size,
        identical: false,
        first_difference_offset: None,
        size_limit_bytes: MAX_DIFF_FILE_BYTES,
        line_limit: line_limit(),
        rows: Vec::new(),
        change_starts: Vec::new(),
    };
    if left_lines.len() > MAX_DIFF_LINES || right_lines.len() > MAX_DIFF_LINES {
        response.outcome = FileDiffOutcomeDto::TooLarge;
        return Ok(response);
    }
    let (rows, change_starts) = align_lines(&left_lines, &right_lines, options);
    response.identical = change_starts.is_empty();
    response.rows = rows;
    response.change_starts = change_starts;
    Ok(response)
}

/// Normalises a line into the key that decides equality under the ignore options.
fn line_key(line: &str, options: DiffOptions) -> String {
    let mut key = String::with_capacity(line.len());
    if options.ignore_whitespace {
        for word in line.split_whitespace() {
            if !key.is_empty() {
                key.push(' ');
            }
            key.push_str(word);
        }
    } else {
        key.push_str(line);
    }
    if options.ignore_case {
        key = key.to_lowercase();
    }
    key
}

fn align_lines(left: &[&str], right: &[&str], options: DiffOptions) -> (Vec<DiffRowDto>, Vec<u32>) {
    let left_keys: Vec<String> = left.iter().map(|line| line_key(line, options)).collect();
    let right_keys: Vec<String> = right.iter().map(|line| line_key(line, options)).collect();
    let ops = capture_diff_slices_deadline(
        Algorithm::Myers,
        &left_keys,
        &right_keys,
        Some(Instant::now() + DIFF_DEADLINE),
    );
    let mut rows: Vec<DiffRowDto> = Vec::new();
    let mut change_starts: Vec<u32> = Vec::new();
    let mut in_change = false;
    for op in ops {
        let is_change = !matches!(op, DiffOp::Equal { .. });
        if is_change && !in_change {
            change_starts.push(u32::try_from(rows.len()).unwrap_or(u32::MAX));
        }
        in_change = is_change;
        match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                for offset in 0..len {
                    rows.push(equal_row(
                        old_index + offset,
                        new_index + offset,
                        left[old_index + offset],
                        right[new_index + offset],
                    ));
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                for (index, line) in left.iter().enumerate().skip(old_index).take(old_len) {
                    rows.push(delete_row(index, line));
                }
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                for (index, line) in right.iter().enumerate().skip(new_index).take(new_len) {
                    rows.push(insert_row(index, line));
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let paired = old_len.min(new_len);
                for offset in 0..paired {
                    rows.push(replace_row(
                        old_index + offset,
                        new_index + offset,
                        left[old_index + offset],
                        right[new_index + offset],
                        options,
                    ));
                }
                for (index, line) in left
                    .iter()
                    .enumerate()
                    .skip(old_index + paired)
                    .take(old_len - paired)
                {
                    rows.push(delete_row(index, line));
                }
                for (index, line) in right
                    .iter()
                    .enumerate()
                    .skip(new_index + paired)
                    .take(new_len - paired)
                {
                    rows.push(insert_row(index, line));
                }
            }
        }
    }
    (rows, change_starts)
}

fn line_number(index: usize) -> Option<u32> {
    u32::try_from(index + 1).ok()
}

fn equal_row(left_index: usize, right_index: usize, left: &str, right: &str) -> DiffRowDto {
    DiffRowDto {
        kind: DiffRowKindDto::Equal,
        left_line: line_number(left_index),
        right_line: line_number(right_index),
        left_text: Some(left.to_owned()),
        right_text: Some(right.to_owned()),
        left_segments: Vec::new(),
        right_segments: Vec::new(),
    }
}

fn delete_row(index: usize, left: &str) -> DiffRowDto {
    DiffRowDto {
        kind: DiffRowKindDto::Delete,
        left_line: line_number(index),
        right_line: None,
        left_text: Some(left.to_owned()),
        right_text: None,
        left_segments: Vec::new(),
        right_segments: Vec::new(),
    }
}

fn insert_row(index: usize, right: &str) -> DiffRowDto {
    DiffRowDto {
        kind: DiffRowKindDto::Insert,
        left_line: None,
        right_line: line_number(index),
        left_text: None,
        right_text: Some(right.to_owned()),
        left_segments: Vec::new(),
        right_segments: Vec::new(),
    }
}

fn replace_row(
    left_index: usize,
    right_index: usize,
    left: &str,
    right: &str,
    options: DiffOptions,
) -> DiffRowDto {
    let (left_segments, right_segments) = intra_line_segments(left, right, options);
    DiffRowDto {
        kind: DiffRowKindDto::Replace,
        left_line: line_number(left_index),
        right_line: line_number(right_index),
        left_text: Some(left.to_owned()),
        right_text: Some(right.to_owned()),
        left_segments,
        right_segments,
    }
}

/// Characters of a line that take part in the intra-line alignment, with their index in the
/// original line. Ignored whitespace never participates, so it is never highlighted.
fn comparable_chars(line: &str, options: DiffOptions) -> (Vec<char>, Vec<usize>) {
    let mut keys = Vec::new();
    let mut origins = Vec::new();
    for (index, ch) in line.chars().enumerate() {
        if options.ignore_whitespace && ch.is_whitespace() {
            continue;
        }
        let key = if options.ignore_case {
            ch.to_lowercase().next().unwrap_or(ch)
        } else {
            ch
        };
        keys.push(key);
        origins.push(index);
    }
    (keys, origins)
}

fn intra_line_segments(
    left: &str,
    right: &str,
    options: DiffOptions,
) -> (Vec<DiffSegmentDto>, Vec<DiffSegmentDto>) {
    let (left_keys, left_origins) = comparable_chars(left, options);
    let (right_keys, right_origins) = comparable_chars(right, options);
    let ops = capture_diff_slices_deadline(
        Algorithm::Myers,
        &left_keys,
        &right_keys,
        Some(Instant::now() + DIFF_DEADLINE),
    );
    let mut left_changed = vec![false; left.chars().count()];
    let mut right_changed = vec![false; right.chars().count()];
    for op in ops {
        match op {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete {
                old_index, old_len, ..
            } => mark(&mut left_changed, &left_origins, old_index, old_len),
            DiffOp::Insert {
                new_index, new_len, ..
            } => mark(&mut right_changed, &right_origins, new_index, new_len),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                mark(&mut left_changed, &left_origins, old_index, old_len);
                mark(&mut right_changed, &right_origins, new_index, new_len);
            }
        }
    }
    (
        segments(left, &left_changed),
        segments(right, &right_changed),
    )
}

fn mark(changed: &mut [bool], origins: &[usize], start: usize, len: usize) {
    for origin in &origins[start..start + len] {
        changed[*origin] = true;
    }
}

fn segments(line: &str, changed: &[bool]) -> Vec<DiffSegmentDto> {
    let mut runs: Vec<DiffSegmentDto> = Vec::new();
    for (ch, &is_changed) in line.chars().zip(changed) {
        match runs.last_mut() {
            Some(run) if run.changed == is_changed => run.text.push(ch),
            _ => runs.push(DiffSegmentDto {
                text: ch.to_string(),
                changed: is_changed,
            }),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_vfs_local::LocalFileSystemProvider;

    fn service() -> FileDiffService {
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider::new()));
        FileDiffService::new(providers)
    }

    fn request(
        dir: &tempfile::TempDir,
        left: &[u8],
        right: &[u8],
        ignore_whitespace: bool,
        ignore_case: bool,
    ) -> DiffFilesRequestDto {
        std::fs::write(dir.path().join("left"), left).unwrap();
        std::fs::write(dir.path().join("right"), right).unwrap();
        DiffFilesRequestDto {
            left: Location::from_native_path(&dir.path().join("left"))
                .unwrap()
                .into(),
            right: Location::from_native_path(&dir.path().join("right"))
                .unwrap()
                .into(),
            ignore_whitespace,
            ignore_case,
        }
    }

    async fn diff(
        left: &[u8],
        right: &[u8],
        ignore_whitespace: bool,
        ignore_case: bool,
    ) -> DiffFilesResponseDto {
        let dir = tempfile::tempdir().unwrap();
        service()
            .diff(request(&dir, left, right, ignore_whitespace, ignore_case))
            .await
            .unwrap()
    }

    fn kinds(response: &DiffFilesResponseDto) -> Vec<DiffRowKindDto> {
        response.rows.iter().map(|row| row.kind).collect()
    }

    #[tokio::test]
    async fn aligns_inserts_deletes_and_replacements_with_line_numbers() {
        let response = diff(b"a\nb\nc\nd\n", b"a\nB\nc\nd\ne\n", false, false).await;
        assert_eq!(response.outcome, FileDiffOutcomeDto::Text);
        assert!(!response.identical);
        assert_eq!(
            kinds(&response),
            vec![
                DiffRowKindDto::Equal,
                DiffRowKindDto::Replace,
                DiffRowKindDto::Equal,
                DiffRowKindDto::Equal,
                DiffRowKindDto::Insert,
            ]
        );
        assert_eq!(response.change_starts, vec![1, 4]);
        let replaced = &response.rows[1];
        assert_eq!(
            (replaced.left_line, replaced.right_line),
            (Some(2), Some(2))
        );
        let inserted = &response.rows[4];
        assert_eq!((inserted.left_line, inserted.right_line), (None, Some(5)));
        assert_eq!(inserted.right_text.as_deref(), Some("e"));
    }

    #[tokio::test]
    async fn identical_text_has_no_change_blocks() {
        let response = diff(b"x\ny\n", b"x\ny\n", false, false).await;
        assert!(response.identical);
        assert!(response.change_starts.is_empty());
        assert_eq!(response.rows.len(), 2);
    }

    #[tokio::test]
    async fn deleted_lines_only_exist_on_the_left() {
        let response = diff(b"a\nb\nc\n", b"a\nc\n", false, false).await;
        assert_eq!(
            kinds(&response),
            vec![
                DiffRowKindDto::Equal,
                DiffRowKindDto::Delete,
                DiffRowKindDto::Equal
            ]
        );
        assert_eq!(response.rows[1].right_line, None);
    }

    #[tokio::test]
    async fn replaced_lines_carry_intra_line_segments() {
        let response = diff(b"let value = 1;\n", b"let valve = 1;\n", false, false).await;
        let row = &response.rows[0];
        assert_eq!(row.kind, DiffRowKindDto::Replace);
        let changed_left: String = row
            .left_segments
            .iter()
            .filter(|s| s.changed)
            .map(|s| s.text.as_str())
            .collect();
        let changed_right: String = row
            .right_segments
            .iter()
            .filter(|s| s.changed)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(changed_left, "u");
        assert_eq!(changed_right, "v");
        let rebuilt: String = row.left_segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(rebuilt, "let value = 1;");
    }

    #[tokio::test]
    async fn ignore_whitespace_treats_whitespace_only_changes_as_equal() {
        let strict = diff(b"a  b\n\tc\n", b"a b\nc\n", false, false).await;
        assert!(!strict.identical);
        let lenient = diff(b"a  b\n\tc\n", b"a b\nc\n", true, false).await;
        assert!(lenient.identical);
        assert_eq!(lenient.rows[0].left_text.as_deref(), Some("a  b"));
    }

    #[tokio::test]
    async fn ignore_case_treats_case_only_changes_as_equal_and_skips_their_highlight() {
        let strict = diff(b"Hello\n", b"hello\n", false, false).await;
        assert!(!strict.identical);
        let lenient = diff(b"Hello\n", b"hello\n", false, true).await;
        assert!(lenient.identical);
        // Mixed: a real change on a line that also differs in case highlights only the real change.
        let mixed = diff(b"Hello world\n", b"hello there\n", false, true).await;
        let row = &mixed.rows[0];
        assert_eq!(row.kind, DiffRowKindDto::Replace);
        assert!(
            row.left_segments
                .iter()
                .any(|s| !s.changed && s.text.starts_with('H'))
        );
    }

    #[tokio::test]
    async fn interleaved_changes_produce_ordered_change_blocks() {
        let left: Vec<String> = (0..20).map(|n| format!("line {n}")).collect();
        let mut right = left.clone();
        right[3] = "changed 3".to_owned();
        right.remove(10);
        right.insert(15, "inserted".to_owned());
        let response = diff(
            (left.join("\n") + "\n").as_bytes(),
            (right.join("\n") + "\n").as_bytes(),
            false,
            false,
        )
        .await;
        assert_eq!(response.change_starts.len(), 3);
        assert!(response.change_starts.windows(2).all(|w| w[0] < w[1]));
        for start in &response.change_starts {
            assert_ne!(response.rows[*start as usize].kind, DiffRowKindDto::Equal);
        }
    }

    #[tokio::test]
    async fn binary_files_report_the_first_differing_offset_and_no_rows() {
        let response = diff(b"ab\0cd", b"ab\0xd", false, false).await;
        assert_eq!(response.outcome, FileDiffOutcomeDto::Binary);
        assert!(!response.identical);
        assert_eq!(response.first_difference_offset, Some(3));
        assert!(response.rows.is_empty());
        let same = diff(b"ab\0cd", b"ab\0cd", false, false).await;
        assert!(same.identical);
        assert_eq!(same.first_difference_offset, None);
        let shorter = diff(b"ab\0", b"ab\0cd", false, false).await;
        assert_eq!(shorter.first_difference_offset, Some(3));
    }

    #[tokio::test]
    async fn oversized_text_is_reported_not_diffed() {
        let big = vec![b'a'; (MAX_DIFF_FILE_BYTES + 1) as usize];
        let response = diff(&big, b"small\n", false, false).await;
        assert_eq!(response.outcome, FileDiffOutcomeDto::TooLarge);
        assert_eq!(response.size_limit_bytes, MAX_DIFF_FILE_BYTES);
        assert_eq!(response.left_size, MAX_DIFF_FILE_BYTES + 1);
        assert!(response.rows.is_empty());
    }

    #[tokio::test]
    async fn too_many_lines_is_reported_not_diffed() {
        let many = "x\n".repeat(MAX_DIFF_LINES + 1);
        let response = diff(many.as_bytes(), b"x\n", false, false).await;
        assert_eq!(response.outcome, FileDiffOutcomeDto::TooLarge);
    }

    #[tokio::test]
    async fn oversized_binary_files_are_still_compared_by_streaming() {
        let mut left = vec![0_u8; (MAX_DIFF_FILE_BYTES + 100) as usize];
        let mut right = left.clone();
        right[(MAX_DIFF_FILE_BYTES + 50) as usize] = 1;
        left[0] = 0;
        let response = diff(&left, &right, false, false).await;
        assert_eq!(response.outcome, FileDiffOutcomeDto::Binary);
        assert_eq!(
            response.first_difference_offset,
            Some(MAX_DIFF_FILE_BYTES + 50)
        );
    }

    #[tokio::test]
    async fn directories_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut req = request(&dir, b"a", b"b", false, false);
        req.left = Location::from_native_path(dir.path()).unwrap().into();
        let error = service().diff(req).await.unwrap_err();
        assert!(matches!(
            error,
            ApplicationError::InvalidRequest(_) | ApplicationError::Rejected(_)
        ));
    }

    #[tokio::test]
    async fn a_missing_file_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let mut req = request(&dir, b"a", b"b", false, false);
        req.right = Location::from_native_path(&dir.path().join("missing"))
            .unwrap()
            .into();
        let error = service().diff(req).await.unwrap_err();
        assert_eq!(error, ApplicationError::NotFound);
    }
}
