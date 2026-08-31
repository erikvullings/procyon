//! Wire types for directory comparison and basic synchronization (spec §16
//! milestone 5, §37, task 0075).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::entry::EntryKindDto;
use crate::location::LocationDto;

/// How two directory trees are compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum ComparisonCriteriaDto {
    NameOnly,
    SizeAndTimestamp,
    ContentHash,
}

/// Per-entry comparison outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum ComparisonStatusDto {
    OnlyLeft,
    OnlyRight,
    Newer,
    Older,
    DifferentSize,
    Identical,
    TypeMismatch,
}

/// Starts a new recursive, cancellable directory comparison
/// (`POST /api/v1/comparisons`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "workspaceId": "7136d9bc-90f1-4c67-8527-9d30683167ec",
    "left": {"providerId": "local", "uri": "file:///Users/erik/left"},
    "right": {"providerId": "local", "uri": "file:///Users/erik/right"},
    "criteria": "sizeAndTimestamp",
    "showHidden": false
}))]
pub struct StartComparisonRequestDto {
    /// Workspace that owns the comparison and receives its result-batch events.
    pub workspace_id: Uuid,
    /// Root compared as "left".
    pub left: LocationDto,
    /// Root compared as "right".
    pub right: LocationDto,
    /// How entries are classified.
    pub criteria: ComparisonCriteriaDto,
    /// Include hidden entries (dotfiles, or the platform hidden attribute)
    /// on either side. Defaults to `false`.
    #[serde(default)]
    pub show_hidden: bool,
}

/// Identifies a started comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartComparisonResponseDto {
    /// The started comparison's identifier, used to page, cancel, and
    /// generate a sync plan from its results.
    pub comparison_id: Uuid,
}

/// One side's metadata for a compared entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonEntrySideDto {
    /// File, directory or symlink.
    pub kind: EntryKindDto,
    /// Size in bytes, when known.
    pub size: Option<u64>,
    /// Last modification time, when known.
    pub modified_at: Option<DateTime<Utc>>,
    /// Streamed content hash, present only under content-hash criteria.
    pub content_hash: Option<String>,
}

/// One compared path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonEntryDto {
    /// Path relative to both roots, using `/` separators.
    pub relative_path: String,
    /// Left-side metadata, absent when [`ComparisonStatusDto::OnlyRight`].
    pub left: Option<ComparisonEntrySideDto>,
    /// Right-side metadata, absent when [`ComparisonStatusDto::OnlyLeft`].
    pub right: Option<ComparisonEntrySideDto>,
    /// The computed outcome for this path.
    pub status: ComparisonStatusDto,
}

/// A bounded, optionally differences-only page of a comparison's results
/// (`GET /api/v1/comparisons/{comparisonId}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonPageDto {
    /// The comparison this page belongs to.
    pub comparison_id: Uuid,
    /// Root compared as "left".
    pub left: LocationDto,
    /// Root compared as "right".
    pub right: LocationDto,
    /// How entries were classified.
    pub criteria: ComparisonCriteriaDto,
    /// Requested zero-based offset, into the (possibly filtered) result set.
    pub offset: u64,
    /// Requested page size after server-side clamping.
    pub limit: u16,
    /// Number of entries matching the requested filter, known so far.
    pub total: u64,
    /// Entries in this page.
    pub entries: Vec<ComparisonEntryDto>,
    /// Whether the backend has stopped producing further entries (either
    /// finished or cancelled).
    pub is_complete: bool,
    /// Cumulative count of unreadable directories skipped so far.
    pub warnings_count: u32,
}

/// Which side is authoritative when a sync plan proposes actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum SyncModeDto {
    MirrorLeftToRight,
    MirrorRightToLeft,
    TwoWayUpdate,
}

/// A proposed (and, before applying, user-editable) action for one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[allow(missing_docs)]
pub enum SyncActionDto {
    CopyLeftToRight,
    CopyRightToLeft,
    DeleteLeft,
    DeleteRight,
    Skip,
}

/// One row of a sync plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlanItemDto {
    /// Path relative to both roots, using `/` separators.
    pub relative_path: String,
    /// The comparison outcome this action was proposed from.
    pub status: ComparisonStatusDto,
    /// The proposed (or, on the way back to `applySyncPlan`, user-edited)
    /// action.
    pub action: SyncActionDto,
    /// Left-side metadata, for display.
    pub left: Option<ComparisonEntrySideDto>,
    /// Right-side metadata, for display.
    pub right: Option<ComparisonEntrySideDto>,
}

/// Proposes a sync plan from a comparison's current results
/// (`POST /api/v1/comparisons/{comparisonId}/sync-plan`).
///
/// Generating a plan never touches a filesystem (spec §35): it only reads
/// the comparison's already-computed results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateSyncPlanRequestDto {
    /// Which side is authoritative.
    pub mode: SyncModeDto,
}

/// A proposed sync plan, ready for review and per-row edits before applying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlanDto {
    /// The comparison this plan was generated from.
    pub comparison_id: Uuid,
    /// Proposed rows, one per non-identical compared entry.
    pub items: Vec<SyncPlanItemDto>,
}

/// Applies a (possibly user-edited) sync plan
/// (`POST /api/v1/comparisons/{comparisonId}/apply-sync-plan`).
///
/// Every non-`skip` row starts one ordinary `copy` or (confirmed, permanent)
/// `delete` operation through the existing operation engine, with the
/// normal conflict, progress and cancellation semantics (spec §35: nothing
/// runs without this explicit, reviewed call).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplySyncPlanRequestDto {
    /// The (possibly edited) rows to apply. Rows with action `skip` start no
    /// operation.
    pub items: Vec<SyncPlanItemDto>,
}

/// The operations started by applying a sync plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplySyncPlanResponseDto {
    /// One operation id per applied (non-`skip`) row, in the same order as
    /// the request's `items`.
    pub operation_ids: Vec<Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_location() -> LocationDto {
        LocationDto {
            provider_id: "local".to_owned(),
            uri: "file:///left".to_owned(),
        }
    }

    #[test]
    fn start_comparison_request_round_trips_and_uses_camel_case_field_names() {
        let request = StartComparisonRequestDto {
            workspace_id: Uuid::new_v4(),
            left: sample_location(),
            right: LocationDto {
                provider_id: "local".to_owned(),
                uri: "file:///right".to_owned(),
            },
            criteria: ComparisonCriteriaDto::ContentHash,
            show_hidden: true,
        };
        let json = serde_json::to_string(&request).expect("serialization must succeed");
        assert!(json.contains("\"workspaceId\""));
        assert!(json.contains("\"showHidden\""));
        assert!(json.contains("\"contentHash\""));
        let parsed: StartComparisonRequestDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(request, parsed);
    }

    #[test]
    fn start_comparison_request_show_hidden_defaults_to_false() {
        let json = format!(
            r#"{{"workspaceId":"00000000-0000-0000-0000-000000000000","left":{},"right":{},"criteria":"nameOnly"}}"#,
            serde_json::to_string(&sample_location()).unwrap(),
            serde_json::to_string(&sample_location()).unwrap(),
        );
        let parsed: StartComparisonRequestDto =
            serde_json::from_str(&json).expect("defaults must fill in missing fields");
        assert!(!parsed.show_hidden);
    }

    #[test]
    fn comparison_entry_dto_round_trips_with_both_sides_present() {
        let entry = ComparisonEntryDto {
            relative_path: "sub/file.txt".to_owned(),
            left: Some(ComparisonEntrySideDto {
                kind: EntryKindDto::File,
                size: Some(10),
                modified_at: None,
                content_hash: Some("abc".to_owned()),
            }),
            right: None,
            status: ComparisonStatusDto::OnlyLeft,
        };
        let json = serde_json::to_string(&entry).expect("serialization must succeed");
        let parsed: ComparisonEntryDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(entry, parsed);
    }

    #[test]
    fn sync_plan_dto_round_trips_through_serde_json() {
        let plan = SyncPlanDto {
            comparison_id: Uuid::new_v4(),
            items: vec![SyncPlanItemDto {
                relative_path: "a.txt".to_owned(),
                status: ComparisonStatusDto::Newer,
                action: SyncActionDto::CopyLeftToRight,
                left: None,
                right: None,
            }],
        };
        let json = serde_json::to_string(&plan).expect("serialization must succeed");
        assert!(json.contains("\"comparisonId\""));
        assert!(json.contains("\"copyLeftToRight\""));
        let parsed: SyncPlanDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(plan, parsed);
    }

    #[test]
    fn apply_sync_plan_response_round_trips_through_serde_json() {
        let response = ApplySyncPlanResponseDto {
            operation_ids: vec![Uuid::new_v4(), Uuid::new_v4()],
        };
        let json = serde_json::to_string(&response).expect("serialization must succeed");
        let parsed: ApplySyncPlanResponseDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(response, parsed);
    }
}
