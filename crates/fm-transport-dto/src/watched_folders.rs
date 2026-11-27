//! Shared watched-folder organizer commands and views.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::LocationDto;

/// One ordered rule. Every supplied criterion must match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilingRule {
    /// Extension without the leading dot.
    pub extension: Option<String>,
    /// Exact MIME type.
    pub content_type: Option<String>,
    /// Minimum file size.
    pub minimum_bytes: Option<u64>,
    /// Immediate destination folder name.
    pub folder: String,
}

/// A reviewed assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApprovedMove {
    /// Original file.
    pub source: LocationDto,
    /// File version captured during preview.
    pub version: Option<FileVersion>,
    /// Immediate destination folder name.
    pub folder: String,
}

/// Listing evidence used to detect changes between scan and approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileVersion {
    /// Provider entry identifier.
    pub entry_id: Uuid,
    /// Size seen during scan.
    pub size: Option<u64>,
    /// Modification timestamp seen during scan.
    pub modified_at: Option<DateTime<Utc>>,
}

/// Requests handled by the watched-folder capability.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[allow(missing_docs)]
pub enum OrganizerRequest {
    List,
    Preview {
        root: LocationDto,
    },
    Collect {
        #[serde(rename = "jobId")]
        job_id: Uuid,
    },
    Cancel {
        #[serde(rename = "jobId")]
        job_id: Uuid,
    },
    Save {
        root: LocationDto,
        #[serde(rename = "delayHours")]
        delay_hours: u16,
        paused: bool,
        rules: Vec<FilingRule>,
        approved: Vec<ApprovedMove>,
    },
    Remove {
        root: LocationDto,
    },
}

/// A pending or failed assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilingStatus {
    /// Original file and destination.
    pub assignment: ApprovedMove,
    /// Earliest eligible filing time.
    pub due_at: DateTime<Utc>,
    /// Operation identifier while the assignment is being filed.
    pub operation_id: Option<Uuid>,
    /// Filing failure, when applicable.
    pub error: Option<String>,
}

/// A persisted watched folder and its pending work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolder {
    /// Watched directory.
    pub root: LocationDto,
    /// Grace period before moving files.
    pub delay_hours: u16,
    /// Whether automatic submissions are suspended.
    pub paused: bool,
    /// Ordered rules for new arrivals.
    pub rules: Vec<FilingRule>,
    /// Pending and failed assignments.
    pub pending: Vec<FilingStatus>,
    /// Current source or listing error, if any.
    pub error: Option<String>,
}

/// The result of a read-only model scan.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerPreview {
    /// Proposed assignments.
    pub moves: Vec<ApprovedMove>,
    /// Files that received no assignment.
    pub unmatched: Vec<LocationDto>,
    /// Eligible files beyond the current model budget.
    pub unprocessed: Vec<LocationDto>,
}

/// Result of an organizer command. A collect response with no preview is still running.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerResponse {
    /// The current watched-folder state.
    pub folders: Vec<WatchedFolder>,
    /// Newly started preview.
    pub job_id: Option<Uuid>,
    /// Finished preview.
    pub preview: Option<OrganizerPreview>,
}
