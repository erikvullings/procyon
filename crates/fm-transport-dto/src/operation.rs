//! Shared wire types for semantic file operations (specification §7, §8).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::LocationDto;

/// A request to start one backend-owned semantic operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartOperationRequestDto {
    /// Semantic operation discriminator. `type` is the stable JSON field name.
    #[serde(rename = "type")]
    pub operation_type: OperationKindDto,
    /// Provider-neutral source locations.
    pub sources: Vec<LocationDto>,
    /// Optional target directory or entry.
    pub destination: Option<LocationDto>,
    /// Per-source destinations for a batch `rename` (task 0072 multi-rename), one entry per
    /// `sources` item in the same order. Empty for every other operation kind and for a
    /// single-entry rename, which keeps using `destination` instead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destinations: Vec<LocationDto>,
    /// Conflict behavior selected before execution.
    pub conflict_policy: OperationConflictPolicyDto,
    /// New child name for `createDirectory`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Archive format requested by a `createArchive` or `moveToArchive` operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_format: Option<ArchiveFormatDto>,
    /// ZIP compression level (0 through 9) requested for archive creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_compression_level: Option<i64>,
    /// Whether a multi-component create-directory name may create missing parents.
    #[serde(default)]
    pub create_intermediate_directories: bool,
    /// Policy for symbolic links encountered during recursive copying.
    #[serde(default)]
    pub symlink_policy: SymlinkPolicyDto,
    /// The user explicitly confirmed an irreversible permanent delete.
    #[serde(default)]
    pub permanent_delete_confirmed: bool,
    /// The user explicitly allowed deletion of read-only entries.
    #[serde(default)]
    pub override_read_only: bool,
}

/// Controls recursive-copy handling of symbolic links.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum SymlinkPolicyDto {
    #[default]
    CopyLink,
    CopyTarget,
}

/// Initial semantic operation kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum OperationKindDto {
    /// Package selected local entries into a new archive.
    CreateArchive,
    /// Package selected local entries into a new archive and remove the originals on success.
    MoveToArchive,
    CreateDirectory,
    CreateFile,
    Rename,
    Copy,
    Move,
    Duplicate,
    Trash,
    Delete,
    /// Search files.
    Search,
    /// Compare two directory trees (task 0075).
    Compare,
}

/// Supported formats for archive creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum ArchiveFormatDto {
    Zip,
    SevenZip,
}

/// Conflict policy carried by an operation request and snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum OperationConflictPolicyDto {
    Ask,
    Skip,
    Overwrite,
    RenameNew,
    KeepNewer,
}

/// Observable operation lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum OperationStateDto {
    Queued,
    Planning,
    Running,
    Paused,
    WaitingForConflictResolution,
    Cancelling,
    Cancelled,
    Completed,
    CompletedWithWarnings,
    Failed,
    /// Recovered after the backend stopped before a terminal transition.
    Interrupted,
}

/// Progress counters for an operation snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationProgressDto {
    /// Completed plan items.
    pub completed_items: u64,
    /// Planned item count.
    pub total_items: Option<u64>,
    /// Completed bytes.
    pub completed_bytes: u64,
    /// Planned bytes.
    pub total_bytes: Option<u64>,
    /// Entry currently processed.
    pub current_entry: Option<EntryRefDto>,
    /// Smoothed byte rate.
    pub bytes_per_second: Option<u64>,
}

/// Complete transport snapshot of an operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationDto {
    /// Stable operation identifier.
    pub id: Uuid,
    #[serde(rename = "type")]
    /// Semantic operation discriminator.
    pub operation_type: OperationKindDto,
    /// Current lifecycle state.
    pub state: OperationStateDto,
    /// Stable source references.
    pub sources: Vec<EntryRefDto>,
    /// Optional destination.
    pub destination: Option<LocationDto>,
    /// Latest progress.
    pub progress: OperationProgressDto,
    /// Selected conflict policy.
    pub conflict_policy: OperationConflictPolicyDto,
    /// Acceptance timestamp.
    pub created_at: DateTime<Utc>,
    /// Planning start timestamp.
    pub started_at: Option<DateTime<Utc>>,
    /// Terminal timestamp.
    pub completed_at: Option<DateTime<Utc>>,
    /// Entry-scoped failures that did not abort the operation.
    pub errors: Vec<OperationEntryErrorDto>,
    /// One-based FIFO position while waiting for a scheduler permit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_position: Option<u64>,
    /// Concise terminal outcome retained with the operation history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
}

/// A bounded page of active and historical operation snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationPageDto {
    /// Requested zero-based offset.
    pub offset: u64,
    /// Requested page size after server-side clamping.
    pub limit: u16,
    /// Number of active and retained history entries before paging.
    pub total: u64,
    /// Snapshots in descending creation order.
    pub operations: Vec<OperationDto>,
}

/// One non-fatal failure associated with a planned entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationEntryErrorDto {
    /// Entry that could not be processed.
    pub entry: EntryRefDto,
    /// Sanitized error message.
    pub message: String,
}

/// Stable provider-neutral reference included in operation snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryRefDto {
    /// Stable entry identifier assigned by the backend.
    pub id: Uuid,
    /// Provider-neutral entry location.
    pub location: LocationDto,
}

/// Reserved conflict-resolution request for the dialog introduced by task 0045.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveOperationConflictRequestDto {
    /// Decision for this conflict.
    pub resolution: ConflictResolutionDto,
    /// Whether the decision applies to subsequent similar conflicts.
    pub apply_to_all_similar: bool,
}

/// User decision for a pending conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum ConflictResolutionDto {
    Confirm,
    Skip,
    Overwrite,
    RenameNew,
    CancelOperation,
}
