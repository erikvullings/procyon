//! Wire types for folder snapshots and snapshot comparison (task 0268).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::LocationDto;

/// Which kind of snapshot job to start (`POST /api/v1/snapshot-jobs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StartSnapshotJobRequestDto {
    /// Record a folder tree into a snapshot file.
    #[serde(rename_all = "camelCase")]
    Create {
        /// Folder to record.
        root: LocationDto,
        /// Snapshot file to write; an existing file is never overwritten.
        destination: LocationDto,
        /// Also record a SHA-256 checksum for every file.
        #[serde(default)]
        with_checksums: bool,
        /// Include hidden entries.
        #[serde(default)]
        show_hidden: bool,
    },
    /// Compare a folder tree against a snapshot file.
    #[serde(rename_all = "camelCase")]
    Compare {
        /// Snapshot file to read.
        snapshot: LocationDto,
        /// Folder to compare; may differ from the recorded root (verify a backup copy).
        target: LocationDto,
        /// Re-hash files to detect silent corruption when the snapshot has checksums.
        #[serde(default = "default_true")]
        verify_checksums: bool,
        /// Include hidden entries.
        #[serde(default)]
        show_hidden: bool,
    },
}

fn default_true() -> bool {
    true
}

/// Identifies a started snapshot job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartSnapshotJobResponseDto {
    /// The started job's identifier, used to page and cancel.
    pub job_id: Uuid,
}

/// Kind of a snapshot job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotJobKindDto {
    /// Recording a snapshot.
    Create,
    /// Comparing against a snapshot.
    Compare,
}

/// Stage a snapshot job is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotJobPhaseDto {
    /// Reading the snapshot file.
    Loading,
    /// Walking the folder.
    Listing,
    /// Computing checksums.
    Hashing,
    /// Writing the snapshot file.
    Writing,
    /// Finished, failed or cancelled.
    Done,
}

/// How an entry differs from the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotDiffKindDto {
    /// Present now, absent from the snapshot.
    Added,
    /// In the snapshot, absent now.
    Removed,
    /// Size, modification time or content differs.
    Changed,
    /// Same size and modification time but a different checksum: silent corruption.
    Corrupted,
}

/// One difference between a folder and a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDiffEntryDto {
    /// How the entry differs.
    pub kind: SnapshotDiffKindDto,
    /// Path relative to the compared folder, with `/` separators.
    pub path: String,
    /// Where the entry exists now, absent for removed entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationDto>,
    /// Size recorded in the snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_size: Option<u64>,
    /// Size now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_size: Option<u64>,
    /// Modification time recorded in the snapshot, milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_modified_ms: Option<i64>,
    /// Modification time now, milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_modified_ms: Option<i64>,
    /// Whether the entry is a directory.
    pub is_directory: bool,
}

/// Counters of a comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDiffCountsDto {
    /// Entries added since the snapshot.
    pub added: u64,
    /// Entries removed since the snapshot.
    pub removed: u64,
    /// Entries changed since the snapshot.
    pub changed: u64,
    /// Files silently corrupted since the snapshot.
    pub corrupted: u64,
    /// Entries identical to the snapshot.
    pub unchanged: u64,
}

/// A bounded page of a snapshot job (`GET /api/v1/snapshot-jobs/{jobId}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotJobPageDto {
    /// The job this page belongs to.
    pub job_id: Uuid,
    /// Create or compare.
    pub kind: SnapshotJobKindDto,
    /// Current stage.
    pub phase: SnapshotJobPhaseDto,
    /// Entries found so far.
    pub entries_found: u64,
    /// Files processed in the current stage (hashed while hashing).
    pub files_processed: u64,
    /// Bytes fed through the hasher.
    pub bytes_hashed: u64,
    /// Whether the snapshot file contains checksums.
    pub has_checksums: bool,
    /// Where the snapshot was written (create jobs, once finished).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<LocationDto>,
    /// Folder that was recorded (create) or compared (compare).
    pub root: LocationDto,
    /// Differences found so far (compare jobs).
    pub counts: SnapshotDiffCountsDto,
    /// Requested zero-based offset.
    pub offset: u64,
    /// Page size after server-side clamping.
    pub limit: u16,
    /// Total differences found so far.
    pub total: u64,
    /// Differences in this page.
    pub entries: Vec<SnapshotDiffEntryDto>,
    /// Whether the job finished (successfully, with an error, or cancelled).
    pub is_complete: bool,
    /// Whether the job stopped because it was cancelled.
    pub is_cancelled: bool,
    /// Whether another page exists.
    pub has_more: bool,
    /// Why the job failed, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
