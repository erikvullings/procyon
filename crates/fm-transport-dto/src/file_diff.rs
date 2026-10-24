//! Wire types for the read-only two-file diff (task 0256).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::LocationDto;

/// Asks for a diff between two files (`POST /api/v1/file-diff`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffFilesRequestDto {
    /// File shown on the left.
    pub left: LocationDto,
    /// File shown on the right.
    pub right: LocationDto,
    /// Treat lines that differ only in whitespace as equal.
    #[serde(default)]
    pub ignore_whitespace: bool,
    /// Treat lines that differ only in letter case as equal.
    #[serde(default)]
    pub ignore_case: bool,
}

/// How the pair was compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum FileDiffOutcomeDto {
    /// Both files are text; `rows` carries the line diff.
    Text,
    /// At least one file is binary; only identical/different and the first differing offset are
    /// reported.
    Binary,
    /// Both files are text but at least one exceeds `sizeLimitBytes`; no diff was computed.
    TooLarge,
}

/// Role of one aligned row of a side-by-side diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiffRowKindDto {
    /// The line is the same on both sides.
    Equal,
    /// The line exists only on the left.
    Delete,
    /// The line exists only on the right.
    Insert,
    /// The lines differ; `leftSegments`/`rightSegments` mark the changed parts.
    Replace,
}

/// A run of characters inside a changed line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffSegmentDto {
    /// The characters of the run.
    pub text: String,
    /// Whether the run differs from the other side.
    pub changed: bool,
}

/// One aligned row of the side-by-side diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffRowDto {
    /// Role of the row.
    pub kind: DiffRowKindDto,
    /// One-based left line number, absent for inserted rows.
    pub left_line: Option<u32>,
    /// One-based right line number, absent for deleted rows.
    pub right_line: Option<u32>,
    /// Left line text without its line terminator.
    pub left_text: Option<String>,
    /// Right line text without its line terminator.
    pub right_text: Option<String>,
    /// Intra-line runs of a replaced left line; empty for every other row.
    pub left_segments: Vec<DiffSegmentDto>,
    /// Intra-line runs of a replaced right line; empty for every other row.
    pub right_segments: Vec<DiffSegmentDto>,
}

/// The result of diffing two files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffFilesResponseDto {
    /// How the pair was compared.
    pub outcome: FileDiffOutcomeDto,
    /// Size of the left file in bytes.
    pub left_size: u64,
    /// Size of the right file in bytes.
    pub right_size: u64,
    /// Whether the files are identical under the requested ignore options (always byte-exact for
    /// binary files).
    pub identical: bool,
    /// Binary files only: zero-based offset of the first differing byte.
    pub first_difference_offset: Option<u64>,
    /// Per-file size limit of the text diff.
    pub size_limit_bytes: u64,
    /// Per-file line limit of the text diff.
    pub line_limit: u32,
    /// Text diff only: the aligned rows, in display order.
    pub rows: Vec<DiffRowDto>,
    /// Text diff only: index in `rows` of the first row of every change block.
    pub change_starts: Vec<u32>,
}
