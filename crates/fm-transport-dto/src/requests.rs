//! Request DTOs for the milestone-1 navigation and metadata endpoints
//! (spec §8, §12).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::location::LocationDto;
use crate::workspace::SortDescriptorDto;

/// Requests the entries of a directory (`POST /api/v1/directories/list`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "workspaceId": "7136d9bc-90f1-4c67-8527-9d30683167ec",
    "paneId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "requestId": "e1ce66cc-64a8-4ae7-9cc1-2882bc80de4e",
    "location": {"providerId": "local", "uri": "file:///Users/erik"},
    "continuationToken": null,
    "sort": [{"columnId": "core.name", "direction": "ascending"}],
    "showHidden": false,
    "foldersFirst": true,
    "showGitStatus": false
}))]
pub struct ListDirectoryRequest {
    /// Workspace that owns the pane and receives its events.
    pub workspace_id: Uuid,
    /// The pane the resulting snapshot will be shown in.
    pub pane_id: Uuid,
    /// Client-generated identifier, echoed back so a superseded request's
    /// late response can be recognised and dropped.
    pub request_id: Uuid,
    /// The location to list.
    pub location: LocationDto,
    /// An opaque token requesting the next page of a prior listing.
    pub continuation_token: Option<String>,
    /// Sort descriptors applied by the backend to the returned page.
    #[serde(default)]
    pub sort: Vec<SortDescriptorDto>,
    /// Whether hidden entries should be included.
    #[serde(default)]
    pub show_hidden: bool,
    /// Whether directories should sort before non-directories.
    #[serde(default)]
    pub folders_first: bool,
    /// Whether the pane's git-status column is visible, carried over from
    /// the requesting tab's table configuration. The backend never computes
    /// git working-tree status (a per-repository `git2` walk) when this is
    /// `false` — most panes never show the column, so this keeps every
    /// ordinary listing free of git2 work entirely rather than relying on
    /// caching alone to hide its cost.
    #[serde(default)]
    pub show_git_status: bool,
}

/// Requests the immediate child directories of a location, for the directory-tree sidebar
/// (`POST /api/v1/directories/children`, task 0139). Unlike [`ListDirectoryRequest`], this is not
/// bound to a pane or workspace: expanding a tree node is independent of any pane's own listing
/// session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "location": {"providerId": "local", "uri": "file:///Users/erik"},
    "showHidden": false
}))]
pub struct ListDirectoryChildrenRequest {
    /// The location whose immediate child directories should be listed.
    pub location: LocationDto,
    /// Whether hidden directories should be included.
    #[serde(default)]
    pub show_hidden: bool,
}

/// Requests navigation to a new location (`POST /api/v1/navigation/open`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "workspaceId": "7136d9bc-90f1-4c67-8527-9d30683167ec",
    "paneId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "requestId": "e1ce66cc-64a8-4ae7-9cc1-2882bc80de4e",
    "location": {"providerId": "local", "uri": "file:///Users/erik/Documents"},
    "sort": [{"columnId": "core.name", "direction": "ascending"}],
    "showHidden": false,
    "foldersFirst": true,
    "showGitStatus": false
}))]
pub struct NavigateRequest {
    /// Workspace that owns the pane and receives its events.
    pub workspace_id: Uuid,
    /// The pane to navigate.
    pub pane_id: Uuid,
    /// Client-generated identifier, echoed back so a superseded request's
    /// late response can be recognised and dropped.
    pub request_id: Uuid,
    /// The location to navigate to.
    pub location: LocationDto,
    /// Sort descriptors applied by the backend to the returned page, carried
    /// over from the navigating tab's current view so navigation doesn't
    /// silently reset it.
    #[serde(default)]
    pub sort: Vec<SortDescriptorDto>,
    /// Whether hidden entries should be included, carried over from the
    /// navigating tab's current view so navigation doesn't silently reset it.
    #[serde(default)]
    pub show_hidden: bool,
    /// Whether directories should sort before non-directories, carried over
    /// from the navigating tab's current view so navigation doesn't silently
    /// reset it.
    #[serde(default)]
    pub folders_first: bool,
    /// Whether the pane's git-status column is visible, carried over from
    /// the navigating tab's table configuration so the backend can skip the
    /// git2 status walk entirely when nothing will show it.
    #[serde(default)]
    pub show_git_status: bool,
}

/// Requests detailed metadata for a single entry
/// (`POST /api/v1/entries/metadata`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "entryId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "location": {"providerId": "local", "uri": "file:///Users/erik/report.pdf"}
}))]
pub struct EntryMetadataRequest {
    /// The entry to fetch metadata for.
    pub entry_id: Uuid,
    /// The entry's location, so the request can be dispatched to the owning
    /// provider without a prior lookup.
    pub location: LocationDto,
}

/// Marks whether a pane is currently in the foreground, so a poll-tracked
/// directory watch (SFTP, FTP, ...) can poll less often while backgrounded
/// (`POST /api/v1/directories/activity`, task 0109). Has no effect on a
/// native/delta-API watch, which is push-based rather than polled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "paneId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "active": false
}))]
pub struct SetPaneActivityRequest {
    /// The pane whose foreground/background state changed.
    pub pane_id: Uuid,
    /// Whether the pane is currently in the foreground.
    pub active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_location() -> LocationDto {
        LocationDto {
            provider_id: "local".to_owned(),
            uri: "file:///Users/erik".to_owned(),
        }
    }

    #[test]
    fn list_directory_request_round_trips_and_uses_camel_case_field_names() {
        let request = ListDirectoryRequest {
            workspace_id: Uuid::new_v4(),
            pane_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
            location: sample_location(),
            continuation_token: Some("page-2".to_owned()),
            sort: Vec::new(),
            show_hidden: false,
            folders_first: true,
            show_git_status: true,
        };
        let json = serde_json::to_string(&request).expect("serialization must succeed");
        assert!(json.contains("\"paneId\""));
        assert!(json.contains("\"workspaceId\""));
        assert!(json.contains("\"requestId\""));
        assert!(json.contains("\"continuationToken\""));
        assert!(json.contains("\"showGitStatus\""));
        let parsed: ListDirectoryRequest =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(request, parsed);
    }

    #[test]
    fn navigate_request_round_trips_and_uses_camel_case_field_names() {
        let request = NavigateRequest {
            workspace_id: Uuid::new_v4(),
            pane_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
            location: sample_location(),
            sort: Vec::new(),
            show_hidden: true,
            folders_first: true,
            show_git_status: true,
        };
        let json = serde_json::to_string(&request).expect("serialization must succeed");
        assert!(json.contains("\"paneId\""));
        assert!(json.contains("\"workspaceId\""));
        assert!(json.contains("\"requestId\""));
        assert!(json.contains("\"showHidden\""));
        assert!(json.contains("\"foldersFirst\""));
        assert!(json.contains("\"showGitStatus\""));
        let parsed: NavigateRequest =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(request, parsed);
    }

    #[test]
    fn navigate_request_defaults_missing_view_fields_for_backward_compatibility() {
        let json = serde_json::json!({
            "workspaceId": Uuid::new_v4(),
            "paneId": Uuid::new_v4(),
            "requestId": Uuid::new_v4(),
            "location": sample_location(),
        })
        .to_string();
        let parsed: NavigateRequest =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(parsed.sort, Vec::new());
        assert!(!parsed.show_hidden);
        assert!(!parsed.folders_first);
        assert!(!parsed.show_git_status);
    }

    #[test]
    fn entry_metadata_request_round_trips_and_uses_camel_case_field_names() {
        let request = EntryMetadataRequest {
            entry_id: Uuid::new_v4(),
            location: sample_location(),
        };
        let json = serde_json::to_string(&request).expect("serialization must succeed");
        assert!(json.contains("\"entryId\""));
        let parsed: EntryMetadataRequest =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(request, parsed);
    }

    #[test]
    fn set_pane_activity_request_round_trips_and_uses_camel_case_field_names() {
        let request = SetPaneActivityRequest {
            pane_id: Uuid::new_v4(),
            active: false,
        };
        let json = serde_json::to_string(&request).expect("serialization must succeed");
        assert!(json.contains("\"paneId\""));
        assert!(json.contains("\"active\""));
        let parsed: SetPaneActivityRequest =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(request, parsed);
    }
}
