//! Wire representation of [`fm_domain::DirectorySnapshot`] (spec §5.4, §8).

use fm_domain::{DirectorySnapshot, LoadingState, PaneId};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::entry::EntrySummaryDto;
use crate::location::LocationDto;

/// The loading state of a pane's directory listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LoadingStateDto {
    /// Nothing has been requested yet.
    Idle,
    /// A request is in flight.
    Loading,
    /// The current page (or all pages) loaded successfully.
    Loaded,
    /// The request failed; never a raw OS error (spec §8).
    Error {
        /// A user-readable error message.
        message: String,
    },
}

impl From<LoadingState> for LoadingStateDto {
    fn from(state: LoadingState) -> Self {
        match state {
            LoadingState::Idle => Self::Idle,
            LoadingState::Loading => Self::Loading,
            LoadingState::Loaded => Self::Loaded,
            LoadingState::Error { message } => Self::Error { message },
        }
    }
}

impl From<LoadingStateDto> for LoadingState {
    fn from(state: LoadingStateDto) -> Self {
        match state {
            LoadingStateDto::Idle => Self::Idle,
            LoadingStateDto::Loading => Self::Loading,
            LoadingStateDto::Loaded => Self::Loaded,
            LoadingStateDto::Error { message } => Self::Error { message },
        }
    }
}

/// Total and available capacity for the volume backing a directory
/// snapshot's location (task 0096), when the platform adapter and provider
/// support reporting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VolumeCapacityDto {
    /// Total capacity of the volume, in bytes.
    pub total_bytes: u64,
    /// Currently available (free) capacity of the volume, in bytes.
    pub available_bytes: u64,
}

/// A batch of directory entries for one pane, at a specific revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "paneId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "requestId": "e1ce66cc-64a8-4ae7-9cc1-2882bc80de4e",
    "revision": 1,
    "location": {"providerId": "local", "uri": "file:///Users/erik"},
    "writable": true,
    "entries": [],
    "totalKnownEntries": 0,
    "totalKnownSize": 0,
    "totalKnownFileCount": 0,
    "hasMore": false,
    "continuationToken": null,
    "loadingState": {"type": "loaded"},
    "volumeCapacity": null
}))]
pub struct DirectorySnapshotDto {
    /// The pane this snapshot belongs to.
    pub pane_id: Uuid,
    /// Correlates this snapshot with the request that produced it.
    pub request_id: Uuid,
    /// Monotonic revision, used to reject responses to superseded requests.
    pub revision: u64,
    /// The location this snapshot lists.
    pub location: LocationDto,
    /// Whether the current user may create entries in this directory.
    pub writable: bool,
    /// The entries loaded so far.
    pub entries: Vec<EntrySummaryDto>,
    /// The total number of entries, when known in advance.
    pub total_known_entries: Option<u64>,
    /// The combined byte size of every file/symlink entry in the directory, when known in
    /// advance.
    pub total_known_size: Option<u64>,
    /// The number of file/symlink entries in the directory (directories excluded), when known
    /// in advance.
    pub total_known_file_count: Option<u64>,
    /// Whether more entries remain to be paged in.
    pub has_more: bool,
    /// An opaque token used to request the next page, when `hasMore`.
    pub continuation_token: Option<String>,
    /// The current loading state.
    pub loading_state: LoadingStateDto,
    /// Total/available capacity for the volume backing this location, when known
    /// (task 0096); absent for non-local providers or unsupported platforms.
    pub volume_capacity: Option<VolumeCapacityDto>,
}

impl From<DirectorySnapshot> for DirectorySnapshotDto {
    fn from(snapshot: DirectorySnapshot) -> Self {
        Self {
            pane_id: snapshot.pane_id.into(),
            request_id: snapshot.request_id,
            revision: snapshot.revision,
            location: snapshot.location.into(),
            writable: snapshot.writable,
            entries: snapshot.entries.into_iter().map(Into::into).collect(),
            total_known_entries: snapshot.total_known_entries,
            total_known_size: snapshot.total_known_size,
            total_known_file_count: snapshot.total_known_file_count,
            has_more: snapshot.has_more,
            continuation_token: snapshot.continuation_token,
            loading_state: snapshot.loading_state.into(),
            // Volume capacity is not part of the domain snapshot (it comes from
            // the platform adapter, which the domain layer has no dependency
            // on) - callers attach it afterward, e.g. `FileManagerService`.
            volume_capacity: None,
        }
    }
}

impl From<DirectorySnapshotDto> for DirectorySnapshot {
    fn from(dto: DirectorySnapshotDto) -> Self {
        Self {
            pane_id: PaneId::from(dto.pane_id),
            request_id: dto.request_id,
            revision: dto.revision,
            location: dto.location.into(),
            writable: dto.writable,
            entries: dto.entries.into_iter().map(Into::into).collect(),
            total_known_entries: dto.total_known_entries,
            total_known_size: dto.total_known_size,
            total_known_file_count: dto.total_known_file_count,
            has_more: dto.has_more,
            continuation_token: dto.continuation_token,
            loading_state: dto.loading_state.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use fm_domain::{Location, ProviderId};

    use super::*;

    fn sample_dto() -> DirectorySnapshotDto {
        DirectorySnapshotDto {
            pane_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
            revision: 1,
            location: LocationDto {
                provider_id: "local".to_owned(),
                uri: "file:///Users/erik".to_owned(),
            },
            writable: true,
            entries: vec![],
            total_known_entries: Some(0),
            total_known_size: Some(0),
            total_known_file_count: Some(0),
            has_more: false,
            continuation_token: None,
            loading_state: LoadingStateDto::Loaded,
            volume_capacity: Some(VolumeCapacityDto {
                total_bytes: 1_000_000_000_000,
                available_bytes: 616_040_000_000,
            }),
        }
    }

    #[test]
    fn directory_snapshot_dto_round_trips_through_serde_json() {
        let dto = sample_dto();
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        let parsed: DirectorySnapshotDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(dto, parsed);
    }

    #[test]
    fn directory_snapshot_dto_uses_camel_case_field_names() {
        let json = serde_json::to_string(&sample_dto()).expect("serialization must succeed");
        for field in [
            "\"paneId\"",
            "\"requestId\"",
            "\"totalKnownEntries\"",
            "\"totalKnownSize\"",
            "\"totalKnownFileCount\"",
            "\"hasMore\"",
            "\"continuationToken\"",
            "\"loadingState\"",
            "\"volumeCapacity\"",
        ] {
            assert!(json.contains(field), "expected {json} to contain {field}");
        }
    }

    #[test]
    fn loading_state_dto_uses_a_string_discriminator_for_every_variant() {
        let states = [
            LoadingStateDto::Idle,
            LoadingStateDto::Loading,
            LoadingStateDto::Loaded,
            LoadingStateDto::Error {
                message: "permission denied".to_owned(),
            },
        ];
        for state in states {
            let json = serde_json::to_string(&state).expect("serialization must succeed");
            assert!(json.contains("\"type\""));
            let parsed: LoadingStateDto =
                serde_json::from_str(&json).expect("deserialization must succeed");
            assert_eq!(state, parsed);
        }
    }

    #[test]
    fn directory_snapshot_dto_converts_to_and_from_the_domain_type() {
        let snapshot = DirectorySnapshot {
            pane_id: PaneId::new(),
            request_id: Uuid::new_v4(),
            revision: 1,
            location: Location::new(ProviderId::new("local"), "file:///Users/erik"),
            writable: true,
            entries: vec![],
            total_known_entries: Some(0),
            total_known_size: Some(0),
            total_known_file_count: Some(0),
            has_more: false,
            continuation_token: None,
            loading_state: LoadingState::Loaded,
        };

        let dto: DirectorySnapshotDto = snapshot.clone().into();
        let round_tripped: DirectorySnapshot = dto.into();
        assert_eq!(snapshot, round_tripped);
    }
}
