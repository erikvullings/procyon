//! Backend-to-frontend events.
//!
//! The envelope and payload types are defined in task 0014 and the bus itself
//! in task 0031. The SSE endpoint (task 0032) and the Tauri channel
//! (task 0034) are both thin adapters over this crate, which is what keeps the
//! two transports at parity.
//!
//! Event projections intentionally mirror the OpenAPI-facing DTOs rather than
//! depending on `fm-transport-dto`: both crates are independent layer-one
//! contracts. The shared JSON fixture below cross-checks this manual mapping
//! against the frontend union.

mod bus;

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use fm_domain::{
    DirectorySnapshot, EntryId, EntryKind, EntrySummary, LoadingState, Location, OperationId,
    PaneId, PluginId, ProviderId, TabId, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use bus::{
    DEFAULT_EVENT_BUS_CAPACITY, EventAudience, EventBus, EventSubscription, SessionId,
    SubscriptionClosed, SubscriptionEvent,
};

/// Transport-neutral wrapper shared by SSE and Tauri event delivery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope<T> {
    /// Stable, monotonically increasing identifier allocated by the event bus.
    pub event_id: u64,
    /// UTC time at which the event was created.
    pub timestamp: DateTime<Utc>,
    /// Workspace scope, omitted for application-wide events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    /// Typed event data.
    pub payload: T,
}

/// Serializes one backend envelope at the shared SSE/Tauri wire boundary.
///
/// Both hosts call this function so field omission, casing and JSON bytes
/// cannot drift between transports.
pub fn serialize_event_envelope(
    envelope: &EventEnvelope<BackendEventPayload>,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(envelope)
}

/// Provider-neutral location in event payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationPayload {
    /// Provider handling this location.
    pub provider_id: ProviderId,
    /// Canonical provider-specific URI.
    pub uri: String,
}

/// Filesystem-entry kind in a progressive disk-usage result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskUsageNodeKindPayload {
    /// Directory or filesystem root.
    Directory,
    /// Regular file.
    File,
    /// Unfollowed symbolic link.
    Symlink,
}

/// One node in a progressive disk-usage result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageNodePayload {
    /// Display name of the entry.
    pub name: String,
    /// Provider-neutral location used for navigation.
    pub location: LocationPayload,
    /// Filesystem kind.
    pub kind: DiskUsageNodeKindPayload,
    /// Apparent byte length, with hard-linked data counted once.
    pub logical_bytes: u64,
    /// Allocated bytes on platforms that expose them.
    pub physical_bytes: u64,
    /// Whether descendants were intentionally omitted.
    #[serde(default)]
    pub collapsed: bool,
    /// Currently known descendants.
    pub children: Vec<DiskUsageNodePayload>,
}

/// Why one filesystem entry could not be included in a disk-usage scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskUsageUnreadableReasonPayload {
    /// The entry exists but the scanning process lacked permission to read it.
    PermissionDenied,
    /// The entry was removed or renamed between being listed and being read.
    Disappeared,
    /// Any other I/O failure while reading metadata or directory contents.
    IoError,
}

/// One filesystem entry skipped during a disk-usage scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageUnreadableEntryPayload {
    /// The location that could not be read.
    pub location: LocationPayload,
    /// Sanitized reason the entry was skipped.
    pub reason: DiskUsageUnreadableReasonPayload,
}

/// Provider-neutral entry reference used by operation events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryRefPayload {
    /// Stable entry identifier.
    pub id: EntryId,
    /// Provider location of the entry.
    pub location: LocationPayload,
}

/// Directory-entry kind in snapshot and delta payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKindPayload {
    /// Regular file.
    File,
    /// Directory or provider-specific container.
    Directory,
    /// Symbolic link or platform equivalent.
    Symlink,
}

/// Compact directory entry carried by snapshot and delta events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummaryPayload {
    /// Stable entry identifier.
    pub id: EntryId,
    /// Provider location.
    pub location: LocationPayload,
    /// Display name.
    pub name: String,
    /// Entry kind.
    pub kind: EntryKindPayload,
    /// Size in bytes, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Modification timestamp, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<DateTime<Utc>>,
    /// Creation timestamp, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    /// Whether the entry is hidden.
    pub hidden: bool,
    /// Whether the entry is read-only.
    pub read_only: bool,
    /// Extension without a leading dot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    /// Detected media type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Display icon lookup key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_key: Option<String>,
    /// Monotonic metadata revision.
    pub metadata_revision: u64,
    /// Content matches found during recursive content search (task 0089).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_matches: Option<Vec<ContentMatchSummary>>,
}

/// Single content match within a file (task 0089).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMatchSummary {
    /// 1-based line number of the match.
    pub line_number: u64,
    /// Byte offset of the match within the file.
    pub offset: u64,
    /// Length of the matched text in bytes.
    pub length: u32,
}

/// Per-entry comparison outcome (task 0075).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ComparisonStatusPayload {
    /// Present only under the left root.
    OnlyLeft,
    /// Present only under the right root.
    OnlyRight,
    /// Present on both sides; the left entry is the more recently modified.
    Newer,
    /// Present on both sides; the left entry is the less recently modified.
    Older,
    /// Present on both sides with the same kind, but they differ without a
    /// determinable newer/older direction.
    DifferentSize,
    /// Present on both sides and considered equal under the active criteria.
    Identical,
    /// Present on both sides at the same relative path but as different
    /// kinds.
    TypeMismatch,
}

/// One side's metadata for a compared entry (task 0075).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonEntrySidePayload {
    /// File, directory or symlink.
    pub kind: EntryKindPayload,
    /// Size in bytes, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Last modification time, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<DateTime<Utc>>,
    /// Streamed content hash, present only under content-hash criteria.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

/// One compared path carried by a comparison results batch (task 0075).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonEntryPayload {
    /// Path relative to both roots, using `/` separators.
    pub relative_path: String,
    /// Left-side metadata, absent when [`ComparisonStatusPayload::OnlyRight`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<ComparisonEntrySidePayload>,
    /// Right-side metadata, absent when [`ComparisonStatusPayload::OnlyLeft`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<ComparisonEntrySidePayload>,
    /// The computed outcome for this path.
    pub status: ComparisonStatusPayload,
}

/// One entry's computed checksums, carried by a checksum results batch
/// (task 0077).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecksumEntryPayload {
    /// Location of the hashed entry.
    pub location: LocationPayload,
    /// Path relative to the request's common root, for display.
    pub relative_path: String,
    /// Bytes hashed.
    pub size: u64,
    /// Digests keyed by lower-case algorithm name (`sha256`, `blake3`, …).
    pub checksums: BTreeMap<String, String>,
    /// Why this entry could not be hashed, when it could not be.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Two or more paths that are the same file through a hardlink (task 0077).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardlinkClusterPayload {
    /// Device number the cluster's paths share.
    pub device: u64,
    /// Inode number the cluster's paths share.
    pub inode: u64,
    /// The paths pointing at that one file.
    pub locations: Vec<LocationPayload>,
}

/// A set of byte-identical files (task 0077).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroupPayload {
    /// Full-content digest shared by every member.
    pub full_hash: String,
    /// Byte size shared by every member.
    pub size: u64,
    /// Paths that are the same file through a hardlink. Reported separately
    /// because deleting one reclaims nothing.
    pub hardlink_clusters: Vec<HardlinkClusterPayload>,
    /// Byte-identical files with distinct identities — the true duplicates.
    pub distinct_locations: Vec<LocationPayload>,
    /// Bytes reclaimable by keeping one copy of the content.
    pub reclaimable_bytes: u64,
}

/// Directory loading state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LoadingStatePayload {
    /// Nothing requested.
    Idle,
    /// Request in flight.
    Loading,
    /// Current page loaded.
    Loaded,
    /// Request failed.
    Error {
        /// Safe user-readable message.
        message: String,
    },
}

/// Full directory state carried by snapshot and reset events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorySnapshotPayload {
    /// Pane receiving the listing.
    pub pane_id: PaneId,
    /// Request correlation identifier.
    pub request_id: String,
    /// Monotonic directory revision.
    pub revision: u64,
    /// Listed location.
    pub location: LocationPayload,
    /// Whether the current user may create entries in the listed directory.
    pub writable: bool,
    /// Loaded entries.
    pub entries: Vec<EntrySummaryPayload>,
    /// Known total, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_known_entries: Option<u64>,
    /// Combined byte size of every file/symlink entry, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_known_size: Option<u64>,
    /// Number of file/symlink entries (directories excluded), when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_known_file_count: Option<u64>,
    /// Whether another page exists.
    pub has_more: bool,
    /// Opaque paging token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_token: Option<String>,
    /// Current loading state.
    pub loading_state: LoadingStatePayload,
}

impl From<Location> for LocationPayload {
    fn from(location: Location) -> Self {
        Self {
            provider_id: location.provider_id,
            uri: location.uri,
        }
    }
}

impl From<EntrySummary> for EntrySummaryPayload {
    fn from(entry: EntrySummary) -> Self {
        Self {
            id: entry.id,
            location: entry.location.into(),
            name: entry.name,
            kind: match entry.kind {
                EntryKind::File => EntryKindPayload::File,
                EntryKind::Directory => EntryKindPayload::Directory,
                EntryKind::Symlink => EntryKindPayload::Symlink,
            },
            size: entry.size,
            modified_at: entry.modified_at,
            created_at: entry.created_at,
            hidden: entry.hidden,
            read_only: entry.read_only,
            extension: entry.extension,
            mime_type: entry.mime_type,
            icon_key: entry.icon_key,
            metadata_revision: entry.metadata_revision,
            content_matches: None,
        }
    }
}

impl EntrySummaryPayload {
    /// Creates a payload from an [`EntrySummary`] with content-search
    /// matches (task 0089).
    pub fn with_matches(entry: &EntrySummary, content_matches: Vec<ContentMatchSummary>) -> Self {
        Self {
            id: entry.id,
            location: entry.location.clone().into(),
            name: entry.name.clone(),
            kind: match entry.kind {
                EntryKind::File => EntryKindPayload::File,
                EntryKind::Directory => EntryKindPayload::Directory,
                EntryKind::Symlink => EntryKindPayload::Symlink,
            },
            size: entry.size,
            modified_at: entry.modified_at,
            created_at: entry.created_at,
            hidden: entry.hidden,
            read_only: entry.read_only,
            extension: entry.extension.clone(),
            mime_type: entry.mime_type.clone(),
            icon_key: entry.icon_key.clone(),
            metadata_revision: entry.metadata_revision,
            content_matches: Some(content_matches),
        }
    }
}

impl From<DirectorySnapshot> for DirectorySnapshotPayload {
    fn from(snapshot: DirectorySnapshot) -> Self {
        Self {
            pane_id: snapshot.pane_id,
            request_id: snapshot.request_id.to_string(),
            revision: snapshot.revision,
            location: snapshot.location.into(),
            writable: snapshot.writable,
            entries: snapshot.entries.into_iter().map(Into::into).collect(),
            total_known_entries: snapshot.total_known_entries,
            total_known_size: snapshot.total_known_size,
            total_known_file_count: snapshot.total_known_file_count,
            has_more: snapshot.has_more,
            continuation_token: snapshot.continuation_token,
            loading_state: match snapshot.loading_state {
                LoadingState::Idle => LoadingStatePayload::Idle,
                LoadingState::Loading => LoadingStatePayload::Loading,
                LoadingState::Loaded => LoadingStatePayload::Loaded,
                LoadingState::Error { message } => LoadingStatePayload::Error { message },
            },
        }
    }
}

/// Workspace sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortDirectionPayload {
    /// Smallest first.
    Ascending,
    /// Largest first.
    Descending,
}

/// A single sort descriptor: an open column id and a direction, mirroring
/// the persisted `fm_domain::SortDescriptor` (spec §5.3.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortDescriptorPayload {
    /// The sorted column, e.g. `"core.name"` or a plugin-provided column.
    pub column_id: String,
    /// Sort direction.
    pub direction: SortDirectionPayload,
}

/// Persisted width and visibility for a single directory-table column,
/// mirroring `fm_domain::ColumnConfiguration`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnConfigurationPayload {
    /// The column's identifier.
    pub column_id: String,
    /// The column's width in pixels.
    pub width: u32,
    /// Whether the column is currently visible.
    pub visible: bool,
}

/// A persisted quick-filter query, mirroring `fm_domain::PersistedFilter`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedFilterPayload {
    /// The filter's plain-text query.
    pub query: String,
}

/// Persisted directory-view configuration (sort, columns, filters) carried by
/// `workspace.tabViewChanged` (spec §5.3.9's `UpdateView`, §5.3.11), mirroring
/// `fm_domain::DirectoryViewConfiguration`. Contains no frontend-only
/// selection/cursor state, matching the domain type it projects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryViewConfigurationPayload {
    /// The active sort descriptors, in priority order.
    pub sort: Vec<SortDescriptorPayload>,
    /// Per-column width and visibility.
    pub columns: Vec<ColumnConfigurationPayload>,
    /// Whether hidden entries are shown.
    pub show_hidden: bool,
    /// Whether directories are grouped before files.
    pub folders_first: bool,
    /// A persisted quick-filter query, if one is saved with the tab.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quick_filter: Option<PersistedFilterPayload>,
}

/// Workspace split direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SplitDirectionPayload {
    /// Side by side.
    Horizontal,
    /// Stacked.
    Vertical,
}

/// Recursive workspace layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WorkspaceLayoutPayload {
    /// Pane leaf.
    Pane {
        /// Pane occupying the leaf.
        pane_id: PaneId,
    },
    /// Two regions separated by a splitter.
    Split {
        /// Split axis.
        direction: SplitDirectionPayload,
        /// Fraction assigned to the first region.
        ratio: f32,
        /// First region.
        first: Box<WorkspaceLayoutPayload>,
        /// Second region.
        second: Box<WorkspaceLayoutPayload>,
    },
}

/// Initial operation kinds from specification §17.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationKindPayload {
    /// Package selected entries into an archive.
    CreateArchive,
    /// Package selected entries into an archive and remove the originals on success.
    MoveToArchive,
    /// Create a directory.
    CreateDirectory,
    /// Create an empty file.
    CreateFile,
    /// Rename one entry.
    Rename,
    /// Copy entries.
    Copy,
    /// Move entries.
    Move,
    /// Duplicate entries.
    Duplicate,
    /// Send entries to trash.
    Trash,
    /// Permanently delete entries.
    Delete,
    /// Safely reverse a completed operation.
    Undo,
    /// Search files.
    Search,
    /// Compare two directory trees (task 0075).
    Compare,
    /// Calculate checksums for a selection (task 0077).
    Checksum,
    /// Scan one or more roots for duplicate files (task 0077).
    FindDuplicates,
}

/// Operation lifecycle states from specification §17.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationStatePayload {
    /// Waiting to start.
    Queued,
    /// Enumerating work and totals.
    Planning,
    /// Performing filesystem mutations.
    Running,
    /// Paused by the user.
    Paused,
    /// Awaiting conflict resolution.
    WaitingForConflictResolution,
    /// Cancellation was requested.
    Cancelling,
    /// Cancelled at a safe point.
    Cancelled,
    /// Completed successfully.
    Completed,
    /// Completed with non-fatal warnings.
    CompletedWithWarnings,
    /// Failed.
    Failed,
    /// Recovered after the backend stopped before a terminal transition.
    Interrupted,
}

/// Conflict policies from specification §17.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicyPayload {
    /// Ask the user.
    Ask,
    /// Skip the conflicting entry.
    Skip,
    /// Replace the destination.
    Overwrite,
    /// Generate a new destination name.
    RenameNew,
    /// Keep the newer entry.
    KeepNewer,
}

/// Progress counters for an operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationProgressDetails {
    /// Number of completed entries.
    pub completed_items: u64,
    /// Total entries, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_items: Option<u64>,
    /// Number of completed bytes.
    pub completed_bytes: u64,
    /// Total bytes, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    /// Entry currently being processed, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_entry: Option<EntryRefPayload>,
    /// Current transfer rate, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_per_second: Option<u64>,
}

/// Progress counters associated with a running operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationProgressPayload {
    /// Operation being measured.
    pub operation_id: OperationId,
    /// Current counters.
    pub progress: OperationProgressDetails,
}

/// Undo availability attached to an operation lifecycle snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationUndoPayload {
    /// Whether a guarded inverse operation can currently be submitted.
    pub available: bool,
    /// Explanation when undo is unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Active or completed undo operation associated with this row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<OperationId>,
}

/// A file operation carried by operation lifecycle events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationPayload {
    /// Stable operation identifier.
    pub id: OperationId,
    /// Operation kind.
    pub kind: OperationKindPayload,
    /// Current lifecycle state.
    pub state: OperationStatePayload,
    /// Source entries.
    pub sources: Vec<EntryRefPayload>,
    /// Destination location, when the kind has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<LocationPayload>,
    /// Current progress.
    pub progress: OperationProgressDetails,
    /// Active conflict policy.
    pub conflict_policy: ConflictPolicyPayload,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Start timestamp, when started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    /// Completion timestamp, when finished.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    /// Current guarded undo availability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo: Option<OperationUndoPayload>,
    /// Original operation reversed by this undo job.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo_of: Option<OperationId>,
}

/// Incremental directory changes in the frontend wire format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DirectoryDeltaPayload {
    /// Entries were added.
    EntriesAdded {
        /// New directory revision.
        revision: u64,
        /// Added entries.
        entries: Vec<EntrySummaryPayload>,
    },
    /// Entries were updated.
    EntriesUpdated {
        /// New directory revision.
        revision: u64,
        /// Updated entries.
        entries: Vec<EntrySummaryPayload>,
    },
    /// Entries were removed.
    EntriesRemoved {
        /// New directory revision.
        revision: u64,
        /// Removed entry identifiers.
        entry_ids: Vec<EntryId>,
    },
    /// Incremental state is invalid and must be replaced.
    Reset {
        /// Replacement snapshot.
        snapshot: DirectorySnapshotPayload,
    },
}

/// Conflict requiring an explicit request/response resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationConflictPayload {
    /// Operation waiting for a decision.
    pub operation_id: OperationId,
    /// Stable conflict identifier.
    pub conflict_id: String,
    /// Human-readable conflict summary.
    pub message: String,
    /// Source entry that would be applied.
    pub source: OperationConflictEntryPayload,
    /// Existing destination entry.
    pub destination: OperationConflictEntryPayload,
}

/// Compact entry metadata carried by a conflict event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationConflictEntryPayload {
    /// Display name.
    pub name: String,
    /// Entry kind.
    pub kind: EntryKindPayload,
    /// Logical size, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Last modification time, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<DateTime<Utc>>,
}

/// Plugin state delivered when discovery or enablement changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPayload {
    /// Plugin identifier.
    pub id: PluginId,
    /// Display name.
    pub name: String,
    /// Manifest version.
    pub version: String,
    /// Whether the plugin is enabled.
    pub enabled: bool,
}

/// User-visible notification delivered by the backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPayload {
    /// Stable notification identifier.
    pub id: String,
    /// Severity (`info`, `warning`, or `error`).
    pub level: NotificationLevelPayload,
    /// Human-readable message.
    pub message: String,
}

/// A connection's runtime state (task 0103, spec §5.4).
///
/// Mirrors `fm_connections::ConnectionStatus`; defined here rather than
/// depended on directly so `fm-events` never needs a dependency on the
/// connections feature crate (matching how `search.resultsBatch` above
/// carries a plain `Uuid` rather than a feature-crate-specific id type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionStatusPayload {
    /// No active or attempted session.
    Disconnected,
    /// A connection attempt is in progress.
    Connecting,
    /// The connection is usable.
    Connected,
    /// A previously connected session is being re-established.
    Reconnecting,
    /// The connection could not authenticate; a valid credential is needed.
    AuthenticationRequired,
    /// The remote host presented a key that has never been verified before
    /// (task 0104, spec §6.4); an explicit accept action is required.
    HostKeyUnverified,
    /// The remote host's key changed since it was last accepted (task 0104,
    /// spec §6.4); never silently accepted.
    HostKeyMismatch,
    /// The last connection attempt failed for a reason other than
    /// authentication or a host-key state.
    Failed,
}

/// User-visible notification severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationLevelPayload {
    /// Informational message.
    Info,
    /// Warning that may require attention.
    Warning,
    /// Error requiring attention.
    Error,
}

/// How a search result set is being populated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchExecutionModePayload {
    /// Results come exclusively from a platform-maintained local index.
    Indexed,
    /// Results come from the live provider-neutral recursive traversal.
    LiveRecursive,
    /// Different roots, or a native-index fallback, use both execution paths.
    Mixed,
}

/// All backend-to-frontend event payloads named by specification §10.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum BackendEventPayload {
    /// Runtime startup completed.
    #[serde(rename = "runtime.ready")]
    RuntimeReady,
    /// A new workspace was created (spec §5.3.11).
    #[serde(rename = "workspace.created")]
    WorkspaceCreated {
        /// The workspace's revision at creation.
        revision: u64,
    },
    /// The workspace was renamed.
    #[serde(rename = "workspace.renamed")]
    WorkspaceRenamed {
        /// The workspace's new revision.
        revision: u64,
        /// The new name.
        name: String,
    },
    /// The workspace became the active workspace (spec §5.3.7 startup and
    /// workspace-switching lifecycle transitions, not only explicit
    /// `openWorkspace` requests).
    #[serde(rename = "workspace.opened")]
    WorkspaceOpened {
        /// The workspace's current revision.
        revision: u64,
    },
    /// The workspace stopped being the active workspace (spec §5.3.7's
    /// workspace-switching lifecycle transition).
    #[serde(rename = "workspace.closed")]
    WorkspaceClosed {
        /// The workspace's revision when it was closed.
        revision: u64,
    },
    /// The workspace was deleted.
    #[serde(rename = "workspace.deleted")]
    WorkspaceDeleted {
        /// The workspace's revision immediately before deletion.
        revision: u64,
    },
    /// The workspace's pane layout tree changed.
    #[serde(rename = "workspace.layoutChanged")]
    WorkspaceLayoutChanged {
        /// The workspace's new revision.
        revision: u64,
        /// The new layout.
        layout: WorkspaceLayoutPayload,
    },
    /// The workspace's operation-centre presentation preferences changed.
    #[serde(rename = "workspace.operationCentreChanged")]
    WorkspaceOperationCentreChanged {
        /// The workspace's new revision.
        revision: u64,
        /// Whether the operation centre is visible.
        visible: bool,
        /// The operation centre's height in pixels.
        height: u32,
    },
    /// The focused pane changed.
    #[serde(rename = "workspace.activePaneChanged")]
    WorkspaceActivePaneChanged {
        /// The workspace's new revision.
        revision: u64,
        /// The newly focused pane.
        pane_id: PaneId,
    },
    /// A new tab was opened in a pane.
    #[serde(rename = "workspace.tabAdded")]
    WorkspaceTabAdded {
        /// The workspace's new revision.
        revision: u64,
        /// The pane the tab was added to.
        pane_id: PaneId,
        /// The new tab's identifier.
        tab_id: TabId,
        /// The new tab's initial location.
        location: LocationPayload,
    },
    /// A tab was closed.
    #[serde(rename = "workspace.tabClosed")]
    WorkspaceTabClosed {
        /// The workspace's new revision.
        revision: u64,
        /// The pane the tab belonged to.
        pane_id: PaneId,
        /// The closed tab's identifier.
        tab_id: TabId,
    },
    /// A pane's active tab changed.
    #[serde(rename = "workspace.tabActivated")]
    WorkspaceTabActivated {
        /// The workspace's new revision.
        revision: u64,
        /// The pane the tab belongs to.
        pane_id: PaneId,
        /// The newly activated tab.
        tab_id: TabId,
    },
    /// A tab navigated to a new location.
    #[serde(rename = "workspace.tabNavigated")]
    WorkspaceTabNavigated {
        /// The workspace's new revision.
        revision: u64,
        /// The pane the tab belongs to.
        pane_id: PaneId,
        /// The navigated tab's identifier.
        tab_id: TabId,
        /// The tab's new location.
        location: LocationPayload,
    },
    /// A tab's persisted view configuration changed.
    #[serde(rename = "workspace.tabViewChanged")]
    WorkspaceTabViewChanged {
        /// The workspace's new revision.
        revision: u64,
        /// The pane the tab belongs to.
        pane_id: PaneId,
        /// The updated tab's identifier.
        tab_id: TabId,
        /// The tab's new persisted view configuration.
        view: DirectoryViewConfigurationPayload,
    },
    /// Full directory state is available.
    #[serde(rename = "directory.snapshot")]
    DirectorySnapshot {
        /// Current directory snapshot.
        snapshot: DirectorySnapshotPayload,
    },
    /// Incremental directory state is available.
    #[serde(rename = "directory.delta")]
    DirectoryDelta {
        /// Pane whose current snapshot this delta advances.
        pane_id: PaneId,
        /// Incremental change.
        delta: DirectoryDeltaPayload,
    },
    /// An operation was accepted.
    #[serde(rename = "operation.created")]
    OperationCreated {
        /// Created operation.
        operation: OperationPayload,
    },
    /// Operation progress changed.
    #[serde(rename = "operation.progress")]
    OperationProgress {
        /// Current progress.
        #[serde(flatten)]
        progress: OperationProgressPayload,
    },
    /// Operation lifecycle state changed.
    #[serde(rename = "operation.stateChanged")]
    OperationStateChanged {
        /// Operation identifier.
        operation_id: OperationId,
        /// New lifecycle state.
        state: OperationStatePayload,
    },
    /// An operation requires conflict resolution.
    #[serde(rename = "operation.conflict")]
    OperationConflict {
        /// Conflict details.
        #[serde(flatten)]
        conflict: OperationConflictPayload,
    },
    /// An operation completed successfully.
    #[serde(rename = "operation.completed")]
    OperationCompleted {
        /// Completed operation.
        operation: OperationPayload,
    },
    /// An operation failed.
    #[serde(rename = "operation.failed")]
    OperationFailed {
        /// Failed operation identifier.
        operation_id: OperationId,
        /// Stable error code.
        code: String,
        /// User-readable error message.
        message: String,
    },
    /// Plugin discovery or enablement changed.
    #[serde(rename = "plugin.changed")]
    PluginChanged {
        /// Current plugin projection.
        plugin: PluginPayload,
    },
    /// A user-visible notification was created.
    #[serde(rename = "notification.created")]
    NotificationCreated {
        /// Created notification.
        notification: NotificationPayload,
    },
    /// A complete snapshot of currently known disk-usage scan results.
    #[serde(rename = "diskUsage.progress")]
    DiskUsageProgress {
        /// The scan this snapshot belongs to.
        scan_id: Uuid,
        /// Currently known hierarchy.
        root: DiskUsageNodePayload,
        /// Cumulative entries skipped because they could not be read.
        unreadable_entries: u64,
        /// Bounded detail list (capped) for entries counted in `unreadable_entries`.
        #[serde(default)]
        unreadable: Vec<DiskUsageUnreadableEntryPayload>,
        /// Filesystem entries visited so far, so progress can advance visibly even while no
        /// top-level subtree has finished.
        #[serde(default)]
        scanned_entries: u64,
        /// Whether no further snapshots will be emitted.
        is_complete: bool,
    },
    /// Filesystem traversal has finished and the final bounded tree is being assembled.
    #[serde(rename = "diskUsage.finalizing")]
    DiskUsageFinalizing {
        /// The scan being finalized.
        scan_id: Uuid,
        /// Total filesystem entries visited during traversal.
        scanned_entries: u64,
    },
    /// A disk-usage scan stopped without producing a complete result.
    #[serde(rename = "diskUsage.failed")]
    DiskUsageFailed {
        /// The scan that failed.
        scan_id: Uuid,
        /// Stable application error code.
        code: String,
        /// Safe user-readable error message.
        message: String,
    },
    /// A batch of streamed recursive-search results is available (spec §24,
    /// §28, task 0068).
    ///
    /// Scoped by `search_id` rather than `pane_id`: a search runs
    /// independently of whether any pane currently has its
    /// `search://local/{searchId}` location open.
    #[serde(rename = "search.resultsBatch")]
    SearchResultsBatch {
        /// The search this batch belongs to.
        search_id: Uuid,
        /// Newly discovered entries since the previous batch.
        entries: Vec<EntrySummaryPayload>,
        /// Whether the backend has stopped producing further batches for
        /// this search (either finished or cancelled).
        is_complete: bool,
        /// Cumulative count of unreadable directories skipped so far.
        warnings_count: u32,
        /// How this result set is currently being populated.
        execution_mode: SearchExecutionModePayload,
    },
    /// A batch of streamed directory-comparison results is available (spec
    /// §16 milestone 5, task 0075).
    ///
    /// Scoped by `comparison_id` rather than `pane_id`, matching
    /// `search.resultsBatch` above.
    #[serde(rename = "comparison.resultsBatch")]
    ComparisonResultsBatch {
        /// The comparison this batch belongs to.
        comparison_id: Uuid,
        /// Newly compared entries since the previous batch.
        entries: Vec<ComparisonEntryPayload>,
        /// Whether the backend has stopped producing further batches for
        /// this comparison (either finished or cancelled).
        is_complete: bool,
        /// Cumulative count of unreadable directories skipped so far.
        warnings_count: u32,
    },
    /// Newly computed checksums for a running checksum job (task 0077).
    ///
    /// Scoped by `job_id` rather than `pane_id`, matching
    /// `comparison.resultsBatch` above.
    #[serde(rename = "checksum.resultsBatch")]
    ChecksumResultsBatch {
        /// The checksum job this batch belongs to.
        job_id: Uuid,
        /// Entries hashed since the previous batch.
        entries: Vec<ChecksumEntryPayload>,
        /// Whether the backend has stopped producing further batches.
        is_complete: bool,
        /// Whether the job stopped because it was cancelled.
        is_cancelled: bool,
    },
    /// The final grouped result of a duplicate scan (task 0077).
    ///
    /// Unlike the streaming batches above this arrives once, at the end: a
    /// duplicate group is only knowable after its whole size/prefix/full-hash
    /// funnel has run, so there is nothing meaningful to stream before then.
    #[serde(rename = "duplicates.resultsReady")]
    DuplicateResultsReady {
        /// The scan these groups belong to.
        scan_id: Uuid,
        /// Groups found, empty when the scan was cancelled.
        groups: Vec<DuplicateGroupPayload>,
        /// Whether the scan stopped because it was cancelled. A cancelled
        /// scan never reports groups, so this distinguishes "none found"
        /// from "did not finish looking".
        is_cancelled: bool,
        /// Files that had to be skipped.
        warnings_count: u32,
    },
    /// A new connection profile was created (task 0103).
    #[serde(rename = "connection.created")]
    ConnectionCreated {
        /// The created connection's identifier.
        connection_id: Uuid,
    },
    /// A connection profile was updated.
    #[serde(rename = "connection.updated")]
    ConnectionUpdated {
        /// The updated connection's identifier.
        connection_id: Uuid,
    },
    /// A connection's runtime status changed.
    #[serde(rename = "connection.statusChanged")]
    ConnectionStatusChanged {
        /// The connection whose status changed.
        connection_id: Uuid,
        /// The connection's new status.
        status: ConnectionStatusPayload,
    },
    /// A connection profile was deleted.
    #[serde(rename = "connection.deleted")]
    ConnectionDeleted {
        /// The deleted connection's identifier.
        connection_id: Uuid,
    },
}

impl BackendEventPayload {
    /// Stable transport event name used by SSE and Tauri adapters.
    #[must_use]
    pub const fn event_name(&self) -> &'static str {
        match self {
            Self::RuntimeReady => "runtime.ready",
            Self::WorkspaceCreated { .. } => "workspace.created",
            Self::WorkspaceRenamed { .. } => "workspace.renamed",
            Self::WorkspaceOpened { .. } => "workspace.opened",
            Self::WorkspaceClosed { .. } => "workspace.closed",
            Self::WorkspaceDeleted { .. } => "workspace.deleted",
            Self::WorkspaceLayoutChanged { .. } => "workspace.layoutChanged",
            Self::WorkspaceOperationCentreChanged { .. } => "workspace.operationCentreChanged",
            Self::WorkspaceActivePaneChanged { .. } => "workspace.activePaneChanged",
            Self::WorkspaceTabAdded { .. } => "workspace.tabAdded",
            Self::WorkspaceTabClosed { .. } => "workspace.tabClosed",
            Self::WorkspaceTabActivated { .. } => "workspace.tabActivated",
            Self::WorkspaceTabNavigated { .. } => "workspace.tabNavigated",
            Self::WorkspaceTabViewChanged { .. } => "workspace.tabViewChanged",
            Self::DirectorySnapshot { .. } => "directory.snapshot",
            Self::DirectoryDelta { .. } => "directory.delta",
            Self::OperationCreated { .. } => "operation.created",
            Self::OperationProgress { .. } => "operation.progress",
            Self::OperationStateChanged { .. } => "operation.stateChanged",
            Self::OperationConflict { .. } => "operation.conflict",
            Self::OperationCompleted { .. } => "operation.completed",
            Self::OperationFailed { .. } => "operation.failed",
            Self::PluginChanged { .. } => "plugin.changed",
            Self::NotificationCreated { .. } => "notification.created",
            Self::DiskUsageProgress { .. } => "diskUsage.progress",
            Self::DiskUsageFinalizing { .. } => "diskUsage.finalizing",
            Self::DiskUsageFailed { .. } => "diskUsage.failed",
            Self::SearchResultsBatch { .. } => "search.resultsBatch",
            Self::ComparisonResultsBatch { .. } => "comparison.resultsBatch",
            Self::ChecksumResultsBatch { .. } => "checksum.resultsBatch",
            Self::DuplicateResultsReady { .. } => "duplicates.resultsReady",
            Self::ConnectionCreated { .. } => "connection.created",
            Self::ConnectionUpdated { .. } => "connection.updated",
            Self::ConnectionStatusChanged { .. } => "connection.statusChanged",
            Self::ConnectionDeleted { .. } => "connection.deleted",
        }
    }

    /// Indicates payloads that transports may replace with a newer value
    /// while preserving the latest observable state.
    #[must_use]
    pub const fn should_coalesce(&self) -> bool {
        matches!(
            self,
            Self::DirectoryDelta { .. }
                | Self::OperationProgress { .. }
                | Self::DiskUsageProgress { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{TimeZone, Utc};
    use fm_domain::{OperationId, PaneId, PluginId, ProviderId, TabId, WorkspaceId};
    use serde_json::json;
    use uuid::Uuid;

    use super::{
        BackendEventPayload, ColumnConfigurationPayload, ConflictPolicyPayload,
        ConnectionStatusPayload, DirectoryDeltaPayload, DirectorySnapshotPayload,
        DirectoryViewConfigurationPayload, DiskUsageNodeKindPayload, DiskUsageNodePayload,
        DiskUsageUnreadableEntryPayload, DiskUsageUnreadableReasonPayload, EntryKindPayload,
        EventEnvelope, LoadingStatePayload, LocationPayload, NotificationLevelPayload,
        NotificationPayload, OperationConflictEntryPayload, OperationConflictPayload,
        OperationKindPayload, OperationPayload, OperationProgressDetails, OperationProgressPayload,
        OperationStatePayload, PersistedFilterPayload, PluginPayload, SearchExecutionModePayload,
        SortDescriptorPayload, SortDirectionPayload, WorkspaceLayoutPayload,
    };

    const WORKSPACE_ID: &str = "11111111-1111-4111-8111-111111111111";
    const OPERATION_ID: &str = "22222222-2222-4222-8222-222222222222";
    const PANE_ID: &str = "33333333-3333-4333-8333-333333333333";
    const TAB_ID: &str = "44444444-4444-4444-8444-444444444444";

    fn fixture_envelope() -> EventEnvelope<BackendEventPayload> {
        EventEnvelope {
            event_id: 1042,
            timestamp: Utc
                .with_ymd_and_hms(2026, 7, 29, 12, 34, 56)
                .single()
                .expect("fixture timestamp must be valid"),
            workspace_id: Some(
                WorkspaceId::from_str(WORKSPACE_ID).expect("fixture workspace id must be valid"),
            ),
            payload: BackendEventPayload::OperationProgress {
                progress: OperationProgressPayload {
                    operation_id: OperationId::from_str(OPERATION_ID)
                        .expect("fixture operation id must be valid"),
                    progress: OperationProgressDetails {
                        completed_items: 3,
                        total_items: Some(10),
                        completed_bytes: 1_048_576,
                        total_bytes: Some(5_242_880),
                        current_entry: None,
                        bytes_per_second: Some(262_144),
                    },
                },
            },
        }
    }

    fn sample_snapshot() -> DirectorySnapshotPayload {
        DirectorySnapshotPayload {
            pane_id: "33333333-3333-4333-8333-333333333333"
                .parse()
                .expect("fixture pane id must be valid"),
            request_id: "55555555-5555-4555-8555-555555555555".to_owned(),
            revision: 1,
            location: LocationPayload {
                provider_id: ProviderId::new("file"),
                uri: "file:///fixture".to_owned(),
            },
            writable: true,
            entries: vec![],
            total_known_entries: Some(0),
            total_known_size: Some(0),
            total_known_file_count: Some(0),
            has_more: false,
            continuation_token: None,
            loading_state: LoadingStatePayload::Loaded,
        }
    }

    #[test]
    fn envelope_serializes_to_the_shared_frontend_fixture() {
        let actual = serde_json::to_value(fixture_envelope()).expect("serialization must succeed");
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/events/operation-progress.json"
        ))
        .expect("shared fixture must be valid JSON");

        assert_eq!(actual, expected);
    }

    #[test]
    fn every_named_workspace_event_matches_its_shared_fixture() {
        let workspace_id =
            WorkspaceId::from_str(WORKSPACE_ID).expect("fixture workspace id must be valid");
        let pane_id = PaneId::from_str(PANE_ID).expect("fixture pane id must be valid");
        let tab_id = TabId::from_str(TAB_ID).expect("fixture tab id must be valid");
        let timestamp = Utc
            .with_ymd_and_hms(2026, 7, 29, 12, 34, 56)
            .single()
            .expect("fixture timestamp must be valid");
        let location = LocationPayload {
            provider_id: ProviderId::new("file"),
            uri: "file:///fixture".to_owned(),
        };

        macro_rules! assert_matches_fixture {
            ($fixture:literal, $payload:expr) => {{
                let envelope = EventEnvelope {
                    event_id: 2000,
                    timestamp,
                    workspace_id: Some(workspace_id),
                    payload: $payload,
                };
                let actual = serde_json::to_value(envelope).expect("serialization must succeed");
                let expected: serde_json::Value = serde_json::from_str(include_str!(concat!(
                    "../../../fixtures/events/",
                    $fixture
                )))
                .expect("shared fixture must be valid JSON");
                assert_eq!(actual, expected, "fixture {} mismatch", $fixture);
            }};
        }

        assert_matches_fixture!(
            "workspace-created.json",
            BackendEventPayload::WorkspaceCreated { revision: 1 }
        );
        assert_matches_fixture!(
            "workspace-renamed.json",
            BackendEventPayload::WorkspaceRenamed {
                revision: 2,
                name: "Photos".to_owned(),
            }
        );
        assert_matches_fixture!(
            "workspace-opened.json",
            BackendEventPayload::WorkspaceOpened { revision: 1 }
        );
        assert_matches_fixture!(
            "workspace-closed.json",
            BackendEventPayload::WorkspaceClosed { revision: 1 }
        );
        assert_matches_fixture!(
            "workspace-deleted.json",
            BackendEventPayload::WorkspaceDeleted { revision: 3 }
        );
        assert_matches_fixture!(
            "workspace-layout-changed.json",
            BackendEventPayload::WorkspaceLayoutChanged {
                revision: 2,
                layout: WorkspaceLayoutPayload::Pane { pane_id },
            }
        );
        assert_matches_fixture!(
            "workspace-active-pane-changed.json",
            BackendEventPayload::WorkspaceActivePaneChanged {
                revision: 2,
                pane_id,
            }
        );
        assert_matches_fixture!(
            "workspace-tab-added.json",
            BackendEventPayload::WorkspaceTabAdded {
                revision: 2,
                pane_id,
                tab_id,
                location: location.clone(),
            }
        );
        assert_matches_fixture!(
            "workspace-tab-closed.json",
            BackendEventPayload::WorkspaceTabClosed {
                revision: 2,
                pane_id,
                tab_id,
            }
        );
        assert_matches_fixture!(
            "workspace-tab-activated.json",
            BackendEventPayload::WorkspaceTabActivated {
                revision: 2,
                pane_id,
                tab_id,
            }
        );
        assert_matches_fixture!(
            "workspace-tab-navigated.json",
            BackendEventPayload::WorkspaceTabNavigated {
                revision: 2,
                pane_id,
                tab_id,
                location: location.clone(),
            }
        );
        assert_matches_fixture!(
            "workspace-tab-view-changed.json",
            BackendEventPayload::WorkspaceTabViewChanged {
                revision: 2,
                pane_id,
                tab_id,
                view: DirectoryViewConfigurationPayload {
                    sort: vec![SortDescriptorPayload {
                        column_id: "core.name".to_owned(),
                        direction: SortDirectionPayload::Ascending,
                    }],
                    columns: vec![ColumnConfigurationPayload {
                        column_id: "core.name".to_owned(),
                        width: 240,
                        visible: true,
                    }],
                    show_hidden: false,
                    folders_first: true,
                    quick_filter: Some(PersistedFilterPayload {
                        query: "report".to_owned(),
                    }),
                },
            }
        );
    }

    #[test]
    fn every_backend_event_uses_the_named_type_discriminator() {
        let events = BackendEventPayload::fixture_variants();
        let actual = events
            .into_iter()
            .map(|event| {
                serde_json::to_value(event)
                    .expect("serialization must succeed")
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .expect("event must have a string type")
                    .to_owned()
            })
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            [
                "runtime.ready",
                "workspace.created",
                "workspace.renamed",
                "workspace.opened",
                "workspace.closed",
                "workspace.deleted",
                "workspace.layoutChanged",
                "workspace.operationCentreChanged",
                "workspace.activePaneChanged",
                "workspace.tabAdded",
                "workspace.tabClosed",
                "workspace.tabActivated",
                "workspace.tabNavigated",
                "workspace.tabViewChanged",
                "directory.snapshot",
                "directory.delta",
                "operation.created",
                "operation.progress",
                "operation.stateChanged",
                "operation.conflict",
                "operation.completed",
                "operation.failed",
                "plugin.changed",
                "notification.created",
                "diskUsage.progress",
                "diskUsage.finalizing",
                "diskUsage.failed",
                "search.resultsBatch",
                "comparison.resultsBatch",
                "connection.created",
                "connection.updated",
                "connection.statusChanged",
                "connection.deleted",
            ]
        );
    }

    #[test]
    fn connection_status_changed_carries_the_connection_id_and_new_status() {
        let connection_id =
            Uuid::from_str(OPERATION_ID).expect("fixture operation id must be valid");
        let event = BackendEventPayload::ConnectionStatusChanged {
            connection_id,
            status: ConnectionStatusPayload::AuthenticationRequired,
        };

        let json = serde_json::to_value(event).expect("serialization must succeed");
        assert_eq!(json["type"], "connection.statusChanged");
        assert_eq!(json["connectionId"], connection_id.to_string());
        assert_eq!(json["status"], "authenticationRequired");
    }

    #[test]
    fn disk_usage_progress_serializes_recursive_camel_case_payload() {
        let scan_id = Uuid::from_str(OPERATION_ID).expect("fixture scan id must be valid");
        let event = BackendEventPayload::DiskUsageProgress {
            scan_id,
            root: DiskUsageNodePayload {
                name: "root".to_owned(),
                location: LocationPayload {
                    provider_id: ProviderId::new("local"),
                    uri: "file:///root".to_owned(),
                },
                kind: DiskUsageNodeKindPayload::Directory,
                logical_bytes: 12,
                physical_bytes: 4096,
                collapsed: false,
                children: vec![DiskUsageNodePayload {
                    name: "node_modules".to_owned(),
                    location: LocationPayload {
                        provider_id: ProviderId::new("local"),
                        uri: "file:///root/node_modules".to_owned(),
                    },
                    kind: DiskUsageNodeKindPayload::Directory,
                    logical_bytes: 12,
                    physical_bytes: 4096,
                    collapsed: true,
                    children: vec![],
                }],
            },
            unreadable_entries: 2,
            unreadable: vec![DiskUsageUnreadableEntryPayload {
                location: LocationPayload {
                    provider_id: ProviderId::new("local"),
                    uri: "file:///root/locked".to_owned(),
                },
                reason: DiskUsageUnreadableReasonPayload::PermissionDenied,
            }],
            scanned_entries: 7,
            is_complete: false,
        };

        assert_eq!(event.event_name(), "diskUsage.progress");
        assert_eq!(
            serde_json::to_value(event).expect("serialization must succeed"),
            json!({
                "type": "diskUsage.progress",
                "scanId": scan_id,
                "root": {
                    "name": "root",
                    "location": {"providerId": "local", "uri": "file:///root"},
                    "kind": "directory",
                    "logicalBytes": 12,
                    "physicalBytes": 4096,
                    "collapsed": false,
                    "children": [{
                        "name": "node_modules",
                        "location": {
                            "providerId": "local",
                            "uri": "file:///root/node_modules"
                        },
                        "kind": "directory",
                        "logicalBytes": 12,
                        "physicalBytes": 4096,
                        "collapsed": true,
                        "children": []
                    }]
                },
                "unreadableEntries": 2,
                "unreadable": [{
                    "location": {"providerId": "local", "uri": "file:///root/locked"},
                    "reason": "permissionDenied"
                }],
                "scannedEntries": 7,
                "isComplete": false
            })
        );
    }

    #[test]
    fn absent_workspace_id_is_omitted_from_json() {
        let envelope = EventEnvelope {
            event_id: 1,
            timestamp: Utc
                .timestamp_opt(0, 0)
                .single()
                .expect("Unix epoch is valid"),
            workspace_id: None,
            payload: BackendEventPayload::RuntimeReady,
        };

        assert_eq!(
            serde_json::to_value(envelope).expect("serialization must succeed"),
            json!({
                "eventId": 1,
                "timestamp": "1970-01-01T00:00:00Z",
                "payload": {"type": "runtime.ready"}
            })
        );
    }

    impl BackendEventPayload {
        fn fixture_variants() -> Vec<Self> {
            let operation_id =
                OperationId::from_str(OPERATION_ID).expect("fixture operation id must be valid");
            let operation = OperationPayload {
                id: operation_id,
                kind: OperationKindPayload::Copy,
                state: OperationStatePayload::Running,
                sources: vec![],
                destination: None,
                progress: OperationProgressDetails {
                    completed_items: 0,
                    total_items: None,
                    completed_bytes: 0,
                    total_bytes: None,
                    current_entry: None,
                    bytes_per_second: None,
                },
                conflict_policy: ConflictPolicyPayload::Ask,
                created_at: Utc
                    .timestamp_opt(0, 0)
                    .single()
                    .expect("Unix epoch is valid"),
                started_at: None,
                completed_at: None,
                undo: None,
                undo_of: None,
            };
            let pane_id = PaneId::from_str(PANE_ID).expect("fixture pane id must be valid");
            let tab_id = TabId::from_str(TAB_ID).expect("fixture tab id must be valid");
            let location = LocationPayload {
                provider_id: ProviderId::new("file"),
                uri: "file:///fixture".to_owned(),
            };
            vec![
                Self::RuntimeReady,
                Self::WorkspaceCreated { revision: 1 },
                Self::WorkspaceRenamed {
                    revision: 2,
                    name: "Photos".to_owned(),
                },
                Self::WorkspaceOpened { revision: 1 },
                Self::WorkspaceClosed { revision: 1 },
                Self::WorkspaceDeleted { revision: 3 },
                Self::WorkspaceLayoutChanged {
                    revision: 2,
                    layout: WorkspaceLayoutPayload::Pane { pane_id },
                },
                Self::WorkspaceOperationCentreChanged {
                    revision: 2,
                    visible: true,
                    height: 240,
                },
                Self::WorkspaceActivePaneChanged {
                    revision: 2,
                    pane_id,
                },
                Self::WorkspaceTabAdded {
                    revision: 2,
                    pane_id,
                    tab_id,
                    location: location.clone(),
                },
                Self::WorkspaceTabClosed {
                    revision: 2,
                    pane_id,
                    tab_id,
                },
                Self::WorkspaceTabActivated {
                    revision: 2,
                    pane_id,
                    tab_id,
                },
                Self::WorkspaceTabNavigated {
                    revision: 2,
                    pane_id,
                    tab_id,
                    location: location.clone(),
                },
                Self::WorkspaceTabViewChanged {
                    revision: 2,
                    pane_id,
                    tab_id,
                    view: DirectoryViewConfigurationPayload {
                        sort: vec![],
                        columns: vec![],
                        show_hidden: false,
                        folders_first: true,
                        quick_filter: None,
                    },
                },
                Self::DirectorySnapshot {
                    snapshot: sample_snapshot(),
                },
                Self::DirectoryDelta {
                    pane_id,
                    delta: DirectoryDeltaPayload::EntriesRemoved {
                        revision: 2,
                        entry_ids: vec![],
                    },
                },
                Self::OperationCreated {
                    operation: operation.clone(),
                },
                fixture_envelope().payload,
                Self::OperationStateChanged {
                    operation_id,
                    state: OperationStatePayload::Paused,
                },
                Self::OperationConflict {
                    conflict: OperationConflictPayload {
                        operation_id,
                        conflict_id: "conflict-1".to_owned(),
                        message: "Destination exists".to_owned(),
                        source: OperationConflictEntryPayload {
                            name: "source.txt".to_owned(),
                            kind: EntryKindPayload::File,
                            size: Some(6),
                            modified_at: None,
                        },
                        destination: OperationConflictEntryPayload {
                            name: "source.txt".to_owned(),
                            kind: EntryKindPayload::File,
                            size: Some(8),
                            modified_at: None,
                        },
                    },
                },
                Self::OperationCompleted {
                    operation: operation.clone(),
                },
                Self::OperationFailed {
                    operation_id,
                    code: "permissionDenied".to_owned(),
                    message: "Permission denied".to_owned(),
                },
                Self::PluginChanged {
                    plugin: PluginPayload {
                        id: PluginId::new("fixture"),
                        name: "Fixture".to_owned(),
                        version: "1.0.0".to_owned(),
                        enabled: true,
                    },
                },
                Self::NotificationCreated {
                    notification: NotificationPayload {
                        id: "notification-1".to_owned(),
                        level: NotificationLevelPayload::Info,
                        message: "Done".to_owned(),
                    },
                },
                Self::DiskUsageProgress {
                    scan_id: Uuid::from_str(OPERATION_ID).expect("fixture scan id must be valid"),
                    root: DiskUsageNodePayload {
                        name: "fixture".to_owned(),
                        location: location.clone(),
                        kind: DiskUsageNodeKindPayload::Directory,
                        logical_bytes: 12,
                        physical_bytes: 4096,
                        collapsed: false,
                        children: vec![DiskUsageNodePayload {
                            name: "file.txt".to_owned(),
                            location: LocationPayload {
                                provider_id: ProviderId::new("file"),
                                uri: "file:///fixture/file.txt".to_owned(),
                            },
                            kind: DiskUsageNodeKindPayload::File,
                            logical_bytes: 12,
                            physical_bytes: 4096,
                            collapsed: false,
                            children: vec![],
                        }],
                    },
                    unreadable_entries: 1,
                    unreadable: vec![DiskUsageUnreadableEntryPayload {
                        location: location.clone(),
                        reason: DiskUsageUnreadableReasonPayload::IoError,
                    }],
                    scanned_entries: 2,
                    is_complete: false,
                },
                Self::DiskUsageFinalizing {
                    scan_id: Uuid::from_str(OPERATION_ID).expect("fixture scan id must be valid"),
                    scanned_entries: 2,
                },
                Self::DiskUsageFailed {
                    scan_id: Uuid::from_str(OPERATION_ID).expect("fixture scan id must be valid"),
                    code: "internal".to_owned(),
                    message: "Disk-usage scan failed".to_owned(),
                },
                Self::SearchResultsBatch {
                    search_id: Uuid::from_str(OPERATION_ID)
                        .expect("fixture operation id must be valid"),
                    entries: vec![],
                    is_complete: true,
                    warnings_count: 0,
                    execution_mode: SearchExecutionModePayload::LiveRecursive,
                },
                Self::ComparisonResultsBatch {
                    comparison_id: Uuid::from_str(OPERATION_ID)
                        .expect("fixture operation id must be valid"),
                    entries: vec![],
                    is_complete: true,
                    warnings_count: 0,
                },
                Self::ConnectionCreated {
                    connection_id: Uuid::from_str(OPERATION_ID)
                        .expect("fixture operation id must be valid"),
                },
                Self::ConnectionUpdated {
                    connection_id: Uuid::from_str(OPERATION_ID)
                        .expect("fixture operation id must be valid"),
                },
                Self::ConnectionStatusChanged {
                    connection_id: Uuid::from_str(OPERATION_ID)
                        .expect("fixture operation id must be valid"),
                    status: ConnectionStatusPayload::Connected,
                },
                Self::ConnectionDeleted {
                    connection_id: Uuid::from_str(OPERATION_ID)
                        .expect("fixture operation id must be valid"),
                },
            ]
        }
    }
}
