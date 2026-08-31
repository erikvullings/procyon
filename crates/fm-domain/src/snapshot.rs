//! Directory snapshots and incremental deltas (spec §5.4).
//!
//! Snapshots and deltas both carry a monotonic `revision`; a stale response
//! (an earlier request that resolves after a later one) must never overwrite
//! a newer view — see the `Reset` variant and the revision comparison used
//! by callers in the application layer (task 0027).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entry::EntrySummary;
use crate::ids::{EntryId, PaneId};
use crate::location::Location;

/// The loading state of a pane's directory listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadingState {
    /// Nothing has been requested yet.
    Idle,
    /// A request is in flight.
    Loading,
    /// The current page (or all pages) loaded successfully.
    Loaded,
    /// The request failed; `message` is a user-readable description, never a
    /// raw OS error (spec §8).
    Error {
        /// A user-readable error message.
        message: String,
    },
}

/// A batch of directory entries for one pane, at a specific revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectorySnapshot {
    /// The pane this snapshot belongs to.
    pub pane_id: PaneId,
    /// Correlates this snapshot with the request that produced it, so a
    /// superseded request's late response can be recognised and dropped.
    pub request_id: Uuid,
    /// Monotonic revision, used to reject responses to superseded requests.
    pub revision: u64,
    /// The location this snapshot lists.
    pub location: Location,
    /// Whether the current user may create entries in this directory.
    ///
    /// A false value includes providers which cannot establish write access;
    /// callers must not start a mutating operation against such a target.
    pub writable: bool,
    /// The entries loaded so far.
    pub entries: Vec<EntrySummary>,
    /// The total number of entries, when known in advance.
    pub total_known_entries: Option<u64>,
    /// The combined byte size of every file/symlink entry in the directory, when known in
    /// advance (mirrors `total_known_entries`'s eager-enumeration availability).
    pub total_known_size: Option<u64>,
    /// The number of file/symlink entries in the directory (directories excluded), when known
    /// in advance; `total_known_entries - total_known_file_count` gives the folder count.
    pub total_known_file_count: Option<u64>,
    /// Whether more entries remain to be paged in.
    pub has_more: bool,
    /// An opaque token used to request the next page, when `has_more`.
    pub continuation_token: Option<String>,
    /// The current loading state.
    pub loading_state: LoadingState,
}

/// An incremental change to a previously delivered [`DirectorySnapshot`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DirectoryDelta {
    /// New entries appeared.
    EntriesAdded {
        /// The revision this delta advances the view to.
        revision: u64,
        /// The entries that were added.
        entries: Vec<EntrySummary>,
    },
    /// Existing entries changed.
    EntriesUpdated {
        /// The revision this delta advances the view to.
        revision: u64,
        /// The entries that changed, in their new state.
        entries: Vec<EntrySummary>,
    },
    /// Existing entries disappeared.
    EntriesRemoved {
        /// The revision this delta advances the view to.
        revision: u64,
        /// The identifiers of the entries that were removed.
        entry_ids: Vec<EntryId>,
    },
    /// The view could not be updated incrementally and must be replaced
    /// wholesale by the carried snapshot.
    Reset {
        /// The replacement snapshot.
        snapshot: DirectorySnapshot,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ProviderId;

    fn sample_snapshot() -> DirectorySnapshot {
        DirectorySnapshot {
            pane_id: PaneId::new(),
            request_id: Uuid::new_v4(),
            revision: 1,
            location: Location::new(ProviderId::new("file"), "file:///Users/erik"),
            writable: true,
            entries: vec![],
            total_known_entries: Some(0),
            total_known_size: Some(0),
            total_known_file_count: Some(0),
            has_more: false,
            continuation_token: None,
            loading_state: LoadingState::Loaded,
        }
    }

    #[test]
    fn directory_snapshot_round_trips_through_serde_json() {
        let snapshot = sample_snapshot();
        let json = serde_json::to_string(&snapshot).expect("serialization must succeed");
        let parsed: DirectorySnapshot =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(snapshot, parsed);
    }

    #[test]
    fn loading_state_round_trips_for_every_variant() {
        let states = [
            LoadingState::Idle,
            LoadingState::Loading,
            LoadingState::Loaded,
            LoadingState::Error {
                message: "permission denied".to_owned(),
            },
        ];
        for state in states {
            let json = serde_json::to_string(&state).expect("serialization must succeed");
            let parsed: LoadingState =
                serde_json::from_str(&json).expect("deserialization must succeed");
            assert_eq!(state, parsed);
        }
    }

    #[test]
    fn directory_delta_entries_added_round_trips_through_serde_json() {
        let delta = DirectoryDelta::EntriesAdded {
            revision: 2,
            entries: vec![],
        };
        let json = serde_json::to_string(&delta).expect("serialization must succeed");
        let parsed: DirectoryDelta =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(delta, parsed);
    }

    #[test]
    fn directory_delta_entries_removed_round_trips_through_serde_json() {
        let delta = DirectoryDelta::EntriesRemoved {
            revision: 3,
            entry_ids: vec![EntryId::new()],
        };
        let json = serde_json::to_string(&delta).expect("serialization must succeed");
        let parsed: DirectoryDelta =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(delta, parsed);
    }

    #[test]
    fn directory_delta_reset_carries_a_full_snapshot() {
        let delta = DirectoryDelta::Reset {
            snapshot: sample_snapshot(),
        };
        let json = serde_json::to_string(&delta).expect("serialization must succeed");
        let parsed: DirectoryDelta =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(delta, parsed);
    }
}
