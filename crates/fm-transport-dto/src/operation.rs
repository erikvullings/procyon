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
    /// Link kind and target form for a `createLink` operation (task 0168).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkRequestDto>,
}

/// Filesystem link or shell shortcut requested by `createLink`.
///
/// The three kinds are deliberately distinct: a symbolic link stores target text, an NTFS
/// junction is a directory mount point with an absolute target, and a `.lnk` shortcut is an
/// ordinary file that only the Windows shell interprets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum LinkKindDto {
    /// POSIX or Windows symbolic link.
    SymbolicLink,
    /// Windows NTFS directory junction.
    Junction,
    /// Windows shell `.lnk` shortcut file.
    Shortcut,
}

/// How a symbolic link records its target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum LinkTargetStyleDto {
    /// Path relative to the directory containing the link.
    #[default]
    Relative,
    /// Absolute native path.
    Absolute,
}

/// Link parameters carried by a `createLink` start request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkRequestDto {
    /// Link kind to create.
    pub kind: LinkKindDto,
    /// Target form; only symbolic links honour `relative`.
    #[serde(default)]
    pub target_style: LinkTargetStyleDto,
}

/// A precondition the user should know about before creating a link kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum LinkRequirementDto {
    /// Windows symbolic links need Developer Mode or an elevated process.
    DeveloperModeOrAdministrator,
    /// Junctions can only point at local directories.
    LocalDirectoryTarget,
    /// Shortcuts are only interpreted by the Windows shell, not by other programs.
    ShellOnly,
}

/// One link kind available for a target/destination pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkKindOptionDto {
    /// Link kind.
    pub kind: LinkKindDto,
    /// Whether a relative target may be requested.
    pub supports_relative: bool,
    /// Preconditions to explain before execution.
    pub requirements: Vec<LinkRequirementDto>,
    /// Default file name for the new link inside the destination.
    pub suggested_name: String,
}

/// Asks which link kinds can point at `target` from inside `destination`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkOptionsRequestDto {
    /// Entry the link will point at.
    pub target: LocationDto,
    /// Directory that will contain the new link.
    pub destination: LocationDto,
}

/// Asks which entry a symbolic link ultimately points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveLinkTargetRequestDto {
    /// The link (or any other entry, which resolves to itself).
    pub location: LocationDto,
}

/// Link kinds supported for a target placed in a destination directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkOptionsDto {
    /// Supported kinds, most portable first. Empty when links cannot be created here.
    pub kinds: Vec<LinkKindOptionDto>,
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
    /// Create a symbolic link, junction, or shell shortcut (task 0168).
    CreateLink,
    Rename,
    Copy,
    Move,
    Duplicate,
    Trash,
    Delete,
    Undo,
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
    /// Whether this completed operation can currently be undone and, if not, why.
    pub undo: OperationUndoDto,
    /// Original operation reversed by this undo job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_of: Option<Uuid>,
}

/// User-facing undo availability for an operation history row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationUndoDto {
    /// A safe undo job may currently be submitted.
    pub available: bool,
    /// Explanation when undo is unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Undo job already running or completed for this operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<Uuid>,
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
