//! Wire types for reading byte ranges from a single file and searching its
//! content (task 0088).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::location::LocationDto;

/// Supplies a password for one archive to the current backend session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveCredentialRequestDto {
    /// Any location within the target archive; only the outer archive identity is cached.
    pub location: LocationDto,
    /// Password to cache in backend memory. Transport adapters must not log request bodies.
    #[schema(value_type = String, format = Password)]
    pub password: String,
}

/// Requests a byte range from a single file (`POST /api/v1/files/range`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "location": {"providerId": "local", "uri": "file:///Users/erik/report.txt"},
    "offset": 0,
    "length": 65536
}))]
pub struct ReadFileRangeRequestDto {
    /// The file to read from.
    pub location: LocationDto,
    /// The zero-based byte offset to start reading at.
    pub offset: u64,
    /// The number of bytes to read, capped server-side to a maximum chunk
    /// size; requesting more than remains in the file returns fewer bytes.
    pub length: u64,
}

/// One chunk of a file's content, starting at `offset`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "data": [72, 101, 108, 108, 111],
    "offset": 0,
    "length": 5,
    "eof": true,
    "probablyBinary": false
}))]
pub struct ReadFileRangeResponseDto {
    /// The chunk's raw bytes.
    pub data: Vec<u8>,
    /// The byte offset the chunk starts at (echoes the request).
    pub offset: u64,
    /// The number of bytes actually returned; may be less than requested if
    /// the file ends before `offset + length`.
    pub length: u64,
    /// Whether this chunk reached the end of the file.
    pub eof: bool,
    /// A NUL-byte sniff of the file's start, only populated when `offset`
    /// is `0`; `None` for later chunks of the same file.
    pub probably_binary: Option<bool>,
}

/// Loads one complete, bounded text file for editing (task 0099).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoadEditableFileRequestDto {
    /// File to load.
    pub location: LocationDto,
}

/// Complete UTF-8 content plus its optimistic revision token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoadEditableFileResponseDto {
    /// Editable UTF-8 text.
    pub content: String,
    /// Opaque token representing the exact bytes loaded.
    pub revision: String,
    /// Exact byte length at load time.
    pub size: u64,
}

/// Atomically replaces a text file after an optimistic revision check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveEditableFileRequestDto {
    /// File to replace.
    pub location: LocationDto,
    /// Optional new destination used by explicit Save As conflict resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<LocationDto>,
    /// New UTF-8 content.
    pub content: String,
    /// Revision returned by the last load/save.
    pub expected_revision: String,
    /// Explicitly permits replacing content that changed since load.
    #[serde(default)]
    pub overwrite_conflict: bool,
}

/// Result of an atomic editable-file save.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveEditableFileResponseDto {
    /// Revision of the saved bytes.
    pub revision: String,
    /// Saved UTF-8 byte length.
    pub size: u64,
    /// Whether an explicit stale-content overwrite occurred.
    pub overwrote_conflict: bool,
}

/// Searches for a substring or regex within a single file's content
/// (`POST /api/v1/files/search`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "location": {"providerId": "local", "uri": "file:///Users/erik/report.txt"},
    "query": "error",
    "regex": false,
    "caseSensitive": false,
    "wholeWord": false
}))]
pub struct SearchInFileRequestDto {
    /// The file to search within.
    pub location: LocationDto,
    /// The substring or regex pattern to search for.
    pub query: String,
    /// Whether `query` is a regular expression rather than a plain substring.
    pub regex: bool,
    /// Whether the match is case-sensitive.
    pub case_sensitive: bool,
    /// Whether a match must be flanked by non-word characters (or line start/end), like an
    /// editor's "whole word" search toggle. Defaults to `false` for older clients that omit it.
    #[serde(default)]
    pub whole_word: bool,
}

/// One match found by a [`SearchInFileRequestDto`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({"lineNumber": 12, "offset": 34, "length": 5}))]
pub struct SearchInFileMatchDto {
    /// The one-based line number the match starts on.
    pub line_number: u64,
    /// The byte offset within the file the match starts at.
    pub offset: u64,
    /// The match's length in bytes.
    pub length: u32,
}

/// The result of a content search within a single file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({"matches": [], "truncated": false}))]
pub struct SearchInFileResponseDto {
    /// Matches found, in file order, up to a server-side cap.
    pub matches: Vec<SearchInFileMatchDto>,
    /// Whether the result was cut off before scanning the whole file because
    /// the match cap was reached.
    pub truncated: bool,
}

/// Requested structured-data interpretation for a read-only viewer session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StructuredViewFormatDto {
    /// Comma-separated or detected-delimiter records.
    Csv,
    /// Tab-separated records.
    Tsv,
    /// Arbitrary JSON shown as bounded highlighted text.
    Json,
    /// One JSON value per physical line.
    Ndjson,
    /// Excel workbook requiring bounded handling or external fallback.
    Excel,
}

/// Whether the first logical delimited record is presented as column labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StructuredHeaderModeDto {
    /// Infer whether the first logical record is a header.
    Auto,
    /// Always use the first logical record as labels.
    FirstRow,
    /// Present the first logical record as data.
    None,
}

/// Renderer selected by the backend after applying bounded format checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StructuredViewKindDto {
    /// Virtualized logical-record table.
    Table,
    /// Bounded raw JSON text with token spans.
    JsonText,
    /// In-app materialization would exceed the memory budget.
    ExternalFallback,
}

/// Opens a provider-neutral, read-only structured-data viewer session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenStructuredViewRequestDto {
    /// Provider-neutral source location.
    pub location: LocationDto,
    /// Requested structured-data interpretation.
    pub format: StructuredViewFormatDto,
    /// A one-character CSV/TSV delimiter override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
    /// Initial header interpretation.
    #[serde(default = "default_header_mode")]
    pub header_mode: StructuredHeaderModeDto,
}

const fn default_header_mode() -> StructuredHeaderModeDto {
    StructuredHeaderModeDto::Auto
}

/// One bounded table row. No session response carries unbounded offset arrays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StructuredRowDto {
    /// Zero-based logical data-record index.
    pub index: u64,
    /// Decoded values for this one record only.
    pub cells: Vec<String>,
    /// Typed workbook metadata for non-empty cells. Empty for text formats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cell_details: Vec<StructuredCellDto>,
}

/// Useful spreadsheet cell semantics without claiming formula recalculation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StructuredCellDto {
    /// Zero-based column index within the worksheet.
    pub column: u32,
    /// Displayed or cached value returned by the workbook.
    pub display: String,
    /// Stable value category for host renderers.
    pub value_type: StructuredCellValueTypeDto,
    /// Formula source, when the workbook reader exposes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
}

/// Spreadsheet value categories preserved by the structured viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StructuredCellValueTypeDto {
    /// UTF-8 text.
    Text,
    /// Integer or floating-point number.
    Number,
    /// Boolean value.
    Boolean,
    /// Workbook cell error such as `#DIV/0!`.
    Error,
    /// Excel or ISO date/time value.
    DateTime,
    /// ISO duration value.
    Duration,
}

/// One worksheet tab and its bounded used dimensions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StructuredSheetDto {
    /// Worksheet tab label.
    pub name: String,
    /// Used row extent, including sparse gaps.
    pub row_count: u64,
    /// Used column extent, including sparse gaps.
    pub column_count: u32,
}

/// Initial session metadata and a bounded first page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenStructuredViewResponseDto {
    /// Opaque backend session identifier.
    pub session_id: Uuid,
    /// Safe renderer selected for the source.
    pub kind: StructuredViewKindDto,
    /// Revision recorded at session creation.
    pub source_revision: String,
    /// Source size at session creation.
    pub source_bytes: u64,
    /// Whether arbitrary bounded byte ranges are available.
    pub random_access: bool,
    /// Active one-byte table delimiter, when the session is tabular.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
    /// Active header interpretation.
    pub header_mode: StructuredHeaderModeDto,
    /// Bounded column labels.
    pub headers: Vec<String>,
    /// Bounded initial logical-record page.
    pub rows: Vec<StructuredRowDto>,
    /// Workbook sheets in source order. Empty for non-workbook formats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<StructuredSheetDto>,
    /// Selected worksheet name for workbook sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_sheet: Option<String>,
    /// Bytes scanned by the incremental indexer.
    pub indexed_bytes: u64,
    /// Logical data records indexed so far.
    pub indexed_rows: u64,
    /// Exact logical-record total once EOF has been reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_rows: Option<u64>,
    /// Whether the indexer reached EOF.
    pub indexing_complete: bool,
    /// Explicit provider or format limitation shown to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Identifies a structured-view session for status or close operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StructuredViewSessionRequestDto {
    /// Opaque backend session identifier.
    pub session_id: Uuid,
}

/// Current bounded indexing progress for a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StructuredViewStatusDto {
    /// Bytes scanned by the incremental indexer.
    pub indexed_bytes: u64,
    /// Logical data records indexed so far.
    pub indexed_rows: u64,
    /// Exact logical-record total once EOF has been reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_rows: Option<u64>,
    /// Whether the indexer reached EOF.
    pub indexing_complete: bool,
    /// Explicit provider or format limitation shown to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Reads a bounded page of logical CSV/TSV/NDJSON records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadStructuredRowsRequestDto {
    /// Opaque backend session identifier.
    pub session_id: Uuid,
    /// Zero-based first logical data record.
    pub start_row: u64,
    /// Bounded number of records requested.
    pub count: u16,
}

/// Bounded logical-record page plus current progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadStructuredRowsResponseDto {
    /// Requested bounded logical-record page.
    pub rows: Vec<StructuredRowDto>,
    /// Logical records indexed so far.
    pub indexed_rows: u64,
    /// Exact logical-record total once EOF has been reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_rows: Option<u64>,
    /// Whether the indexer reached EOF.
    pub indexing_complete: bool,
}

/// Changes table interpretation without reopening the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStructuredViewRequestDto {
    /// Opaque backend session identifier.
    pub session_id: Uuid,
    /// Optional one-byte delimiter override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<String>,
    /// Optional header interpretation override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_mode: Option<StructuredHeaderModeDto>,
    /// Worksheet to select without reopening the workbook.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_sheet: Option<String>,
}

/// Reads a bounded raw JSON byte window and chunk-safe lexical token spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadStructuredJsonWindowRequestDto {
    /// Opaque backend session identifier.
    pub session_id: Uuid,
    /// Requested source byte offset.
    pub offset: u64,
    /// Bounded maximum byte length.
    pub length: u32,
}

/// JSON lexical category used by the raw-window renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum JsonTokenKindDto {
    /// Object property string.
    Property,
    /// JSON string value or a chunk-spanning string segment.
    String,
    /// JSON number.
    Number,
    /// `true` or `false`.
    Boolean,
    /// `null`.
    Null,
    /// Container or separator punctuation.
    Punctuation,
    /// Byte not recognized by the incremental lexer.
    Invalid,
}

/// Byte-relative token span inside a JSON window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JsonTokenSpanDto {
    /// Token category.
    pub kind: JsonTokenKindDto,
    /// Byte offset relative to the returned window.
    pub start: u32,
    /// Token length in bytes.
    pub length: u32,
}

/// Bounded raw JSON bytes and byte-relative lexical token spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadStructuredJsonWindowResponseDto {
    /// UTF-8-boundary-aligned raw bytes.
    pub data: Vec<u8>,
    /// Actual source byte offset after UTF-8 alignment.
    pub offset: u64,
    /// Whether this window reaches EOF.
    pub eof: bool,
    /// Bounded token spans for `data`.
    pub tokens: Vec<JsonTokenSpanDto>,
}

/// Cursor-paged, cancellable table search. Sorting is deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchStructuredRowsRequestDto {
    /// Opaque backend session identifier.
    pub session_id: Uuid,
    /// Case-insensitive substring query.
    pub query: String,
    /// Zero-based logical-record continuation cursor.
    #[serde(default)]
    pub cursor: u64,
    /// Maximum matches to return.
    pub limit: u16,
}

/// One bounded search page and an optional continuation cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchStructuredRowsResponseDto {
    /// Matching logical records.
    pub matches: Vec<StructuredRowDto>,
    /// Continuation row for the next bounded search page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
    /// Whether the source indexer reached EOF.
    pub indexing_complete: bool,
}

/// Requests a directory's recursive total size (`POST /api/v1/directories/size`), for the Total
/// Commander-style "press a key on a folder to see how much space it consumes" behaviour
/// (task 0071).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "location": {"providerId": "local", "uri": "file:///Users/erik/Documents"}
}))]
pub struct CalculateFolderSizeRequestDto {
    /// The directory to sum.
    pub location: LocationDto,
}

/// The recursive total size of a directory and how many files were counted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({"totalBytes": 104857600, "fileCount": 42}))]
pub struct CalculateFolderSizeResponseDto {
    /// Sum of every descendant file's (and unfollowed symlink's) size, in bytes.
    pub total_bytes: u64,
    /// Number of files (and symlinks) counted.
    pub file_count: u64,
}

/// Requests an archive's recursively computed summary for the F3 viewer (task 0141).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSummaryRequestDto {
    /// The outer local archive file; the application derives its `archive://...!/` root.
    pub location: LocationDto,
}

/// Content-derived archive format and provider-neutral recursive entry totals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSummaryResponseDto {
    /// Canonical format label derived by the archive provider from content.
    pub format: String,
    /// Number of file entries.
    pub file_count: u64,
    /// Number of directory entries.
    pub directory_count: u64,
    /// Sum of every file entry's uncompressed size.
    pub uncompressed_size: u64,
    /// Packed payload bytes when the format exposes them cheaply.
    pub compressed_size: Option<u64>,
}

/// Opens a bounded, provider-neutral DOCX content-preview session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocxPreviewRequestDto {
    /// Provider-neutral DOCX source location.
    pub location: LocationDto,
}

/// One embedded image retained by a bounded DOCX preview session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocxPreviewResourceDto {
    /// Opaque resource identifier scoped to the preview session.
    pub resource_id: Uuid,
    /// Package-relative source used by the generated semantic HTML.
    pub source: String,
    /// Browser media type for the bounded image bytes.
    pub media_type: String,
    /// Exact retained byte count.
    pub byte_length: u64,
}

/// Initial semantic DOCX content and bounded resource descriptors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocxPreviewResponseDto {
    /// Opaque backend session identifier.
    pub session_id: Uuid,
    /// Revision recorded before parsing.
    pub source_revision: String,
    /// Source package bytes at session creation.
    pub source_bytes: u64,
    /// Semantic HTML fragment. Hosts must sanitize it before DOM insertion.
    pub html: String,
    /// Embedded images referenced by the HTML, fetched separately by id.
    pub resources: Vec<DocxPreviewResourceDto>,
    /// Word layout features deliberately omitted from this content view.
    pub omitted_features: Vec<String>,
}

/// Identifies a DOCX preview session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocxPreviewSessionRequestDto {
    /// Opaque backend session identifier.
    pub session_id: Uuid,
}

/// Requests one bounded embedded DOCX resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadDocxPreviewResourceRequestDto {
    /// Opaque backend session identifier.
    pub session_id: Uuid,
    /// Opaque resource identifier from the open response.
    pub resource_id: Uuid,
}

/// Embedded image bytes and their browser media type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadDocxPreviewResourceResponseDto {
    /// Bounded raw image bytes.
    pub data: Vec<u8>,
    /// Browser media type for the bytes.
    pub media_type: String,
}

/// Opens a bounded, provider-neutral PowerPoint-to-PDF preview session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenPptxPreviewRequestDto {
    /// Provider-neutral PPTX source location.
    pub location: LocationDto,
}

/// Rendered PDF session metadata with an immediately displayable first slide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenPptxPreviewResponseDto {
    /// Opaque backend session identifier.
    pub session_id: Uuid,
    /// Revision recorded before parsing.
    pub source_revision: String,
    /// Source package bytes at session creation.
    pub source_bytes: u64,
    /// Bounded single-page PDF shown while the complete presentation renders.
    pub first_page_pdf: Vec<u8>,
}

/// Identifies a PPTX preview session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PptxPreviewSessionRequestDto {
    /// Opaque backend session identifier.
    pub session_id: Uuid,
}

/// Requests one bounded byte range from a rendered PowerPoint PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadPptxPreviewPdfRequestDto {
    /// Opaque backend session identifier.
    pub session_id: Uuid,
    /// Zero-based byte offset in the rendered PDF.
    pub offset: u64,
    /// Requested byte count, capped server-side.
    pub length: u64,
}

/// Requests a file's git commit history (`POST /api/v1/files/git-history`), for the Alt+Space
/// metadata panel's history section (task 0135).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "location": {"providerId": "local", "uri": "file:///Users/erik/report.txt"}
}))]
pub struct GetFileGitHistoryRequestDto {
    /// The file to look up history for.
    pub location: LocationDto,
}

/// One commit touching the requested file, newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "commitId": "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0",
    "shortId": "a1b2c3d",
    "authorName": "Ada Lovelace",
    "authorEmail": "ada@example.com",
    "committedAt": "2026-01-15T09:30:00Z",
    "summary": "fix(app): handle empty selection"
}))]
pub struct GitLogEntryDto {
    /// The commit's full SHA.
    pub commit_id: String,
    /// The commit's abbreviated id, as `git log --oneline` would show it.
    pub short_id: String,
    /// The commit author's display name.
    pub author_name: String,
    /// The commit author's email address.
    pub author_email: String,
    /// When the commit was authored.
    pub committed_at: chrono::DateTime<chrono::Utc>,
    /// The commit message's first line.
    pub summary: String,
}

/// A file's git history: empty when the file is outside a git working tree, on a non-local
/// provider, or not yet tracked by any commit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({"commits": []}))]
pub struct GetFileGitHistoryResponseDto {
    /// Commits touching the file, newest first, up to a server-side cap.
    pub commits: Vec<GitLogEntryDto>,
}

impl From<fm_domain::GitLogEntry> for GitLogEntryDto {
    fn from(entry: fm_domain::GitLogEntry) -> Self {
        Self {
            commit_id: entry.commit_id,
            short_id: entry.short_id,
            author_name: entry.author_name,
            author_email: entry.author_email,
            committed_at: entry.committed_at,
            summary: entry.summary,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_location() -> LocationDto {
        LocationDto {
            provider_id: "local".to_owned(),
            uri: "file:///Users/erik/report.txt".to_owned(),
        }
    }

    #[test]
    fn read_file_range_request_round_trips_and_uses_camel_case_field_names() {
        let request = ReadFileRangeRequestDto {
            location: sample_location(),
            offset: 128,
            length: 65536,
        };
        let json = serde_json::to_string(&request).expect("serialization must succeed");
        assert!(json.contains("\"location\""));
        assert!(json.contains("\"offset\""));
        assert!(json.contains("\"length\""));
        let parsed: ReadFileRangeRequestDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(request, parsed);
    }

    #[test]
    fn calculate_folder_size_request_round_trips_and_uses_camel_case_field_names() {
        let request = CalculateFolderSizeRequestDto {
            location: sample_location(),
        };
        let json = serde_json::to_string(&request).expect("serialization must succeed");
        assert!(json.contains("\"location\""));
        let parsed: CalculateFolderSizeRequestDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(request, parsed);
    }

    #[test]
    fn calculate_folder_size_response_round_trips_and_uses_camel_case_field_names() {
        let response = CalculateFolderSizeResponseDto {
            total_bytes: 104_857_600,
            file_count: 42,
        };
        let json = serde_json::to_string(&response).expect("serialization must succeed");
        assert!(json.contains("\"totalBytes\""));
        assert!(json.contains("\"fileCount\""));
        let parsed: CalculateFolderSizeResponseDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(response, parsed);
    }

    #[test]
    fn archive_summary_response_round_trips_and_uses_camel_case_field_names() {
        let response = ArchiveSummaryResponseDto {
            format: "zip".to_owned(),
            file_count: 3,
            directory_count: 2,
            uncompressed_size: 4_096,
            compressed_size: Some(512),
        };
        let json = serde_json::to_string(&response).expect("serialization must succeed");
        assert!(json.contains("\"fileCount\""));
        assert!(json.contains("\"directoryCount\""));
        assert!(json.contains("\"uncompressedSize\""));
        assert!(json.contains("\"compressedSize\""));
        let parsed: ArchiveSummaryResponseDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(response, parsed);
    }

    #[test]
    fn docx_preview_contract_round_trips_without_a_native_path_or_inline_image_bytes() {
        let session_id = Uuid::new_v4();
        let resource_id = Uuid::new_v4();
        let response = OpenDocxPreviewResponseDto {
            session_id,
            source_revision: "42:7".to_owned(),
            source_bytes: 42,
            html: r#"<p><img src="media/image1.png"></p>"#.to_owned(),
            resources: vec![DocxPreviewResourceDto {
                resource_id,
                source: "media/image1.png".to_owned(),
                media_type: "image/png".to_owned(),
                byte_length: 8,
            }],
            omitted_features: vec!["exact pagination".to_owned()],
        };

        let json = serde_json::to_value(&response).expect("serialize DOCX preview");
        assert_eq!(json["sessionId"], session_id.to_string());
        assert_eq!(json["resources"][0]["resourceId"], resource_id.to_string());
        assert!(json["resources"][0].get("data").is_none());
        assert!(json.get("path").is_none());
        let parsed: OpenDocxPreviewResponseDto =
            serde_json::from_value(json).expect("deserialize DOCX preview");
        assert_eq!(parsed, response);
    }

    #[test]
    fn read_file_range_response_round_trips_and_uses_camel_case_field_names() {
        let response = ReadFileRangeResponseDto {
            data: vec![72, 101, 108, 108, 111],
            offset: 0,
            length: 5,
            eof: true,
            probably_binary: Some(false),
        };
        let json = serde_json::to_string(&response).expect("serialization must succeed");
        assert!(json.contains("\"data\""));
        assert!(json.contains("\"eof\""));
        assert!(json.contains("\"probablyBinary\""));
        let parsed: ReadFileRangeResponseDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(response, parsed);
    }

    #[test]
    fn read_file_range_response_serializes_data_as_a_plain_byte_array() {
        let response = ReadFileRangeResponseDto {
            data: vec![1, 2, 3],
            offset: 0,
            length: 3,
            eof: true,
            probably_binary: None,
        };
        let value = serde_json::to_value(&response).expect("serialization must succeed");
        assert_eq!(value["data"], serde_json::json!([1, 2, 3]));
        assert_eq!(value["probablyBinary"], serde_json::Value::Null);
    }

    #[test]
    fn search_in_file_request_round_trips_and_uses_camel_case_field_names() {
        let request = SearchInFileRequestDto {
            location: sample_location(),
            query: "error".to_owned(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
        };
        let json = serde_json::to_string(&request).expect("serialization must succeed");
        assert!(json.contains("\"query\""));
        assert!(json.contains("\"regex\""));
        assert!(json.contains("\"caseSensitive\""));
        assert!(json.contains("\"wholeWord\""));
        let parsed: SearchInFileRequestDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(request, parsed);
    }

    #[test]
    fn search_in_file_request_defaults_whole_word_to_false_when_omitted() {
        let json = serde_json::json!({
            "location": sample_location(),
            "query": "error",
            "regex": false,
            "caseSensitive": false,
        });
        let parsed: SearchInFileRequestDto =
            serde_json::from_value(json).expect("deserialization must succeed");
        assert!(!parsed.whole_word);
    }

    #[test]
    fn search_in_file_response_round_trips_and_uses_camel_case_field_names() {
        let response = SearchInFileResponseDto {
            matches: vec![SearchInFileMatchDto {
                line_number: 12,
                offset: 34,
                length: 5,
            }],
            truncated: false,
        };
        let json = serde_json::to_string(&response).expect("serialization must succeed");
        assert!(json.contains("\"lineNumber\""));
        assert!(json.contains("\"truncated\""));
        let parsed: SearchInFileResponseDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(response, parsed);
    }

    #[test]
    fn get_file_git_history_request_round_trips_and_uses_camel_case_field_names() {
        let request = GetFileGitHistoryRequestDto {
            location: sample_location(),
        };
        let json = serde_json::to_string(&request).expect("serialization must succeed");
        assert!(json.contains("\"location\""));
        let parsed: GetFileGitHistoryRequestDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(request, parsed);
    }

    #[test]
    fn get_file_git_history_response_round_trips_and_uses_camel_case_field_names() {
        let response = GetFileGitHistoryResponseDto {
            commits: vec![GitLogEntryDto {
                commit_id: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0".to_owned(),
                short_id: "a1b2c3d".to_owned(),
                author_name: "Ada Lovelace".to_owned(),
                author_email: "ada@example.com".to_owned(),
                committed_at: "2026-01-15T09:30:00Z".parse().expect("valid timestamp"),
                summary: "fix(app): handle empty selection".to_owned(),
            }],
        };
        let json = serde_json::to_string(&response).expect("serialization must succeed");
        assert!(json.contains("\"commitId\""));
        assert!(json.contains("\"shortId\""));
        assert!(json.contains("\"authorName\""));
        assert!(json.contains("\"authorEmail\""));
        assert!(json.contains("\"committedAt\""));
        let parsed: GetFileGitHistoryResponseDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(response, parsed);
    }

    #[test]
    fn structured_view_open_request_round_trips_without_native_paths() {
        let request = OpenStructuredViewRequestDto {
            location: sample_location(),
            format: StructuredViewFormatDto::Csv,
            delimiter: Some(";".to_owned()),
            header_mode: StructuredHeaderModeDto::FirstRow,
        };

        let json = serde_json::to_value(&request).expect("serialization must succeed");
        assert_eq!(json["delimiter"], ";");
        assert_eq!(json["headerMode"], "firstRow");
        assert!(json.get("path").is_none());
        let parsed: OpenStructuredViewRequestDto =
            serde_json::from_value(json).expect("deserialization must succeed");
        assert_eq!(request, parsed);
    }
}
