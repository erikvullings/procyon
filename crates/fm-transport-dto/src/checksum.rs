//! Wire types for checksum calculation, checksum-file verification and
//! duplicate detection (spec §16 milestone 5, §18, §37, task 0077).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::location::LocationDto;

/// A checksum algorithm the backend can compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum ChecksumAlgorithmDto {
    Sha256,
    Blake3,
    Crc32,
    Md5,
}

/// Starts a cancellable checksum job over a selection
/// (`POST /api/v1/checksums`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "workspaceId": "7136d9bc-90f1-4c67-8527-9d30683167ec",
    "entries": [{"providerId": "local", "uri": "file:///Users/erik/report.pdf"}],
    "algorithms": ["sha256", "blake3"]
}))]
pub struct StartChecksumRequestDto {
    /// Workspace that owns the job and receives its result-batch events.
    pub workspace_id: Uuid,
    /// Entries to hash. Directories are rejected: the caller expands a
    /// selection before starting the job.
    pub entries: Vec<LocationDto>,
    /// Algorithms to compute, all in a single pass over each file.
    pub algorithms: Vec<ChecksumAlgorithmDto>,
}

/// Identifies a started checksum job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartChecksumResponseDto {
    /// The started job's identifier, used to page, cancel, save and verify.
    pub job_id: Uuid,
}

/// One entry's computed checksums.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChecksumEntryDto {
    /// Location of the hashed entry.
    pub location: LocationDto,
    /// Path relative to the selection's common root.
    pub relative_path: String,
    /// Bytes hashed.
    pub size: u64,
    /// Digests keyed by lower-case algorithm name (`sha256`, `blake3`, …).
    pub checksums: BTreeMap<String, String>,
    /// Why this entry could not be hashed, when it could not be.
    pub error: Option<String>,
}

/// A bounded page of a checksum job's results
/// (`GET /api/v1/checksums/{jobId}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChecksumPageDto {
    /// The job this page belongs to.
    pub job_id: Uuid,
    /// Algorithms the job was started with.
    pub algorithms: Vec<ChecksumAlgorithmDto>,
    /// Requested zero-based offset.
    pub offset: u64,
    /// Requested page size after server-side clamping.
    pub limit: u16,
    /// Entries computed so far.
    pub total: u64,
    /// Entries the job was asked to hash.
    pub total_entries: u64,
    /// Entries in this page.
    pub entries: Vec<ChecksumEntryDto>,
    /// Whether the backend has stopped producing further entries.
    pub is_complete: bool,
    /// Whether the job stopped because it was cancelled. Distinguishes a
    /// short result set that is final from one that simply did not finish.
    pub is_cancelled: bool,
    /// Whether another page exists, or more entries may still arrive.
    pub has_more: bool,
}

/// Renders a finished job's results as a coreutils-compatible checksum file
/// (`POST /api/v1/checksums/{jobId}/checksum-file`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenderChecksumFileRequestDto {
    /// Which of the job's algorithms to write. A checksum file carries one
    /// algorithm, matching `sha256sum`/`md5sum`.
    pub algorithm: ChecksumAlgorithmDto,
}

/// The rendered checksum-file text, for the caller to copy to the clipboard.
///
/// Rendering deliberately does not write anything. Writing the same text to
/// disk is a separate, explicit step — [`SaveChecksumFileRequestDto`] — which
/// goes through the provider's audited `WRITE` path, so there is no second,
/// unaudited way to create a file (spec §35).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChecksumFileDto {
    /// Suggested filename, e.g. `checksums.sha256`.
    pub suggested_name: String,
    /// The complete file contents.
    pub content: String,
}

/// Writes a finished job's results to a checksum file on disk
/// (`POST /api/v1/checksums/{jobId}/save`).
///
/// The bytes go out through the provider's normal `WRITE` path, so saving a
/// checksum file is audited and capability-gated exactly like any other file
/// this application creates (spec §35) — there is no second, host-specific
/// write path smuggled in through a native save dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "destination": {"providerId": "local", "uri": "file:///Users/erik/checksums.sha256"},
    "algorithm": "sha256",
    "overwrite": false
}))]
pub struct SaveChecksumFileRequestDto {
    /// Where to write the file, including its filename.
    pub destination: LocationDto,
    /// Which of the job's algorithms to write.
    pub algorithm: ChecksumAlgorithmDto,
    /// Permit replacing an existing file. Defaults to `false`, so an
    /// accidental save never silently destroys an existing checksum file.
    #[serde(default)]
    pub overwrite: bool,
}

/// Confirms a checksum file was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveChecksumFileResponseDto {
    /// Where the file was written.
    pub location: LocationDto,
    /// Number of bytes written.
    pub bytes_written: u64,
}

/// Verifies a job's computed digests against an existing checksum file
/// (`POST /api/v1/checksums/{jobId}/verify`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VerifyChecksumFileRequestDto {
    /// The checksum file's complete text, as read by the caller.
    pub content: String,
}

/// Outcome of verifying one checksum-file entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum VerificationStatusDto {
    Match,
    Mismatch,
    Missing,
}

/// One verified path and its outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResultDto {
    /// Path exactly as recorded in the checksum file.
    pub path: String,
    /// What verifying that path produced.
    pub status: VerificationStatusDto,
    /// Digest recorded in the checksum file, when the status is a mismatch.
    pub expected: Option<String>,
    /// Digest computed from the entry, when the status is a mismatch.
    pub actual: Option<String>,
}

/// The full verification report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReportDto {
    /// The job whose digests were compared.
    pub job_id: Uuid,
    /// Per-entry outcomes, in checksum-file order.
    pub results: Vec<VerificationResultDto>,
    /// Number of entries that matched.
    pub matched: u64,
    /// Number of entries whose digest differed.
    pub mismatched: u64,
    /// Number of entries listed in the file but absent from the job.
    pub missing: u64,
}

/// Starts a cancellable duplicate scan across one or more roots
/// (`POST /api/v1/duplicate-scans`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "workspaceId": "7136d9bc-90f1-4c67-8527-9d30683167ec",
    "roots": [{"providerId": "local", "uri": "file:///Users/erik/Pictures"}],
    "showHidden": false,
    "includeEmptyFiles": false
}))]
pub struct StartDuplicateScanRequestDto {
    /// Workspace that owns the scan and receives its result event.
    pub workspace_id: Uuid,
    /// Roots to scan recursively.
    pub roots: Vec<LocationDto>,
    /// Include hidden entries. Defaults to `false`.
    #[serde(default)]
    pub show_hidden: bool,
    /// Include zero-byte files, which are all trivially identical.
    /// Defaults to `false`.
    #[serde(default)]
    pub include_empty_files: bool,
}

/// Identifies a started duplicate scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartDuplicateScanResponseDto {
    /// The started scan's identifier, used to page and cancel.
    pub scan_id: Uuid,
}

/// Two or more paths that are the same file through a hardlink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HardlinkClusterDto {
    /// Device number the cluster's paths share.
    pub device: u64,
    /// Inode number the cluster's paths share.
    pub inode: u64,
    /// The paths pointing at that one file.
    pub locations: Vec<LocationDto>,
}

/// A set of byte-identical files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroupDto {
    /// Full-content digest shared by every member.
    pub full_hash: String,
    /// Byte size shared by every member.
    pub size: u64,
    /// Paths that are the same file through a hardlink. Presented separately
    /// because deleting one of them reclaims nothing.
    pub hardlink_clusters: Vec<HardlinkClusterDto>,
    /// Byte-identical files with distinct identities — the true duplicates.
    pub distinct_locations: Vec<LocationDto>,
    /// Bytes reclaimable by keeping one copy of the content.
    pub reclaimable_bytes: u64,
}

/// Counters describing how much work each detection stage performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateStatsDto {
    /// Files considered.
    pub candidates: u64,
    /// Files whose size occurred more than once.
    pub size_survivors: u64,
    /// Files whose prefix was hashed.
    pub partially_hashed: u64,
    /// Files streamed in full.
    pub fully_hashed: u64,
    /// Bytes fed through a hasher.
    pub bytes_hashed: u64,
    /// Files skipped because they could not be read.
    pub failed: u64,
}

/// A bounded page of a duplicate scan's grouped results
/// (`GET /api/v1/duplicate-scans/{scanId}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatePageDto {
    /// The scan this page belongs to.
    pub scan_id: Uuid,
    /// Roots the scan covered.
    pub roots: Vec<LocationDto>,
    /// Requested zero-based offset.
    pub offset: u64,
    /// Requested page size after server-side clamping.
    pub limit: u16,
    /// Total groups found.
    pub total: u64,
    /// Groups in this page.
    pub groups: Vec<DuplicateGroupDto>,
    /// Whether the scan has finished.
    pub is_complete: bool,
    /// Whether the scan stopped because it was cancelled. A cancelled scan
    /// reports no groups, so this distinguishes "none found" from "did not
    /// finish looking".
    pub is_cancelled: bool,
    /// Whether another page exists.
    pub has_more: bool,
    /// How much work each stage performed.
    pub stats: DuplicateStatsDto,
    /// Number of files that had to be skipped.
    pub warnings_count: u32,
}
