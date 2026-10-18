//! Wire types for the duplicate and near-duplicate music finder (task 0265).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::LocationDto;

/// Starts a cancellable music duplicate scan (`POST /api/v1/music-duplicate-scans`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "roots": [{"providerId": "local", "uri": "file:///Users/erik/Music"}],
    "showHidden": false
}))]
pub struct StartMusicDuplicateScanRequestDto {
    /// Roots to scan recursively.
    pub roots: Vec<LocationDto>,
    /// Include hidden entries. Defaults to `false`.
    #[serde(default)]
    pub show_hidden: bool,
}

/// Identifies a started music duplicate scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartMusicDuplicateScanResponseDto {
    /// The started scan's identifier, used to page and cancel.
    pub scan_id: Uuid,
}

/// How sure the finder is that a group's files are the same recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum MusicDuplicateConfidenceDto {
    /// Byte-identical content (matching checksum).
    Exact,
    /// Same normalised artist and title with a duration within tolerance.
    Likely,
}

/// Stage a music duplicate scan is currently in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum MusicDuplicateScanPhaseDto {
    /// Walking the roots for audio files.
    Listing,
    /// Hashing same-sized files to find byte-identical copies.
    Hashing,
    /// Reading tags and stream properties.
    Tagging,
    /// Grouping likely duplicates.
    Grouping,
    /// Finished (or cancelled).
    Done,
}

/// One audio file inside a duplicate group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MusicDuplicateFileDto {
    /// Where the file lives.
    pub location: LocationDto,
    /// Size in bytes.
    pub size: u64,
    /// Container/codec family, when the file could be parsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Whether the codec is lossless.
    pub lossless: bool,
    /// Overall bitrate in kilobits per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bitrate_kbps: Option<u32>,
    /// Playing time in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    /// Track artist tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// Track title tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Album tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
}

/// Files believed to be the same recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MusicDuplicateGroupDto {
    /// How sure the finder is.
    pub confidence: MusicDuplicateConfidenceDto,
    /// Group members; the suggested keeper is `files[keep_index]`.
    pub files: Vec<MusicDuplicateFileDto>,
    /// Index of the file to keep: lossless first, then the highest bitrate, then the largest.
    pub keep_index: u32,
    /// Bytes freed by removing every file except the suggested keeper.
    pub reclaimable_bytes: u64,
}

/// A bounded page of a music duplicate scan (`GET /api/v1/music-duplicate-scans/{scanId}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MusicDuplicatePageDto {
    /// The scan this page belongs to.
    pub scan_id: Uuid,
    /// Roots the scan covers.
    pub roots: Vec<LocationDto>,
    /// Current stage.
    pub phase: MusicDuplicateScanPhaseDto,
    /// Audio files found so far.
    pub files_found: u64,
    /// Files handled in the current stage (tags read while tagging).
    pub files_processed: u64,
    /// Files whose tags could not be read; they still take part in exact matching.
    pub unreadable_tags: u64,
    /// Requested zero-based offset.
    pub offset: u64,
    /// Page size after server-side clamping.
    pub limit: u16,
    /// Total groups found (grows until the scan completes).
    pub total: u64,
    /// Groups in this page.
    pub groups: Vec<MusicDuplicateGroupDto>,
    /// Whether the scan finished.
    pub is_complete: bool,
    /// Whether the scan stopped because it was cancelled.
    pub is_cancelled: bool,
    /// Whether another page exists.
    pub has_more: bool,
}
