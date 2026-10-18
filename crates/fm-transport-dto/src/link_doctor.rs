//! Wire types for the broken-link doctor (task 0266).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::LocationDto;

/// Starts a cancellable broken-link scan (`POST /api/v1/link-doctor-scans`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "roots": [{"providerId": "local", "uri": "file:///Users/erik/Notes"}],
    "showHidden": false
}))]
pub struct StartLinkDoctorScanRequestDto {
    /// Roots to scan recursively. Symbolic links are inspected but never followed.
    pub roots: Vec<LocationDto>,
    /// Include hidden entries. Defaults to `false`.
    #[serde(default)]
    pub show_hidden: bool,
}

/// Identifies a started broken-link scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartLinkDoctorScanResponseDto {
    /// The started scan's identifier, used to page and cancel.
    pub scan_id: Uuid,
}

/// Where a broken link was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum LinkProblemKindDto {
    /// A symbolic link whose target does not exist.
    BrokenSymlink,
    /// A Windows `.lnk` shortcut whose target does not exist.
    BrokenShortcut,
    /// A playlist entry that points at a missing file.
    MissingPlaylistEntry,
    /// A relative Markdown or HTML link to a missing file.
    MissingDocumentLink,
}

/// One broken link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkProblemDto {
    /// What kind of link is broken.
    pub kind: LinkProblemKindDto,
    /// The file or symlink that contains the link.
    pub source: LocationDto,
    /// The target exactly as recorded in the link.
    pub target: String,
    /// Where the target would be, when it could be resolved to a location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<LocationDto>,
    /// One-based line of the link inside a text file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// A bounded page of a broken-link scan (`GET /api/v1/link-doctor-scans/{scanId}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkDoctorPageDto {
    /// The scan this page belongs to.
    pub scan_id: Uuid,
    /// Roots the scan covers.
    pub roots: Vec<LocationDto>,
    /// Files and links inspected so far.
    pub entries_scanned: u64,
    /// Individual links whose targets were checked.
    pub links_checked: u64,
    /// Requested zero-based offset.
    pub offset: u64,
    /// Page size after server-side clamping.
    pub limit: u16,
    /// Total problems found so far.
    pub total: u64,
    /// Problems in this page.
    pub problems: Vec<LinkProblemDto>,
    /// Whether the scan finished.
    pub is_complete: bool,
    /// Whether the scan stopped because it was cancelled.
    pub is_cancelled: bool,
    /// Whether another page exists.
    pub has_more: bool,
}

/// Searches for files that could replace a missing link target
/// (`POST /api/v1/link-doctor-replacements`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FindLinkReplacementsRequestDto {
    /// Roots to search.
    pub roots: Vec<LocationDto>,
    /// File name of the missing target.
    pub target_name: String,
    /// Size of the missing target when known, to rank same-sized files first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_size: Option<u64>,
    /// Include hidden entries. Defaults to `false`.
    #[serde(default)]
    pub show_hidden: bool,
}

/// A file that might replace a missing target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkReplacementCandidateDto {
    /// Where the candidate lives.
    pub location: LocationDto,
    /// Size in bytes.
    pub size: u64,
    /// Whether the size equals the expected size.
    pub same_size: bool,
    /// Whether the name matches case-sensitively (otherwise only ignoring case).
    pub exact_name: bool,
}

/// Replacement candidates, best first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FindLinkReplacementsResponseDto {
    /// Candidates ordered by confidence.
    pub candidates: Vec<LinkReplacementCandidateDto>,
    /// Whether the search stopped early because of its entry budget.
    pub truncated: bool,
}
