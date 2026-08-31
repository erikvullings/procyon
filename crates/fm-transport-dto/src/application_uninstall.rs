//! Wire types for discovering a macOS application bundle's related files
//! before uninstalling it (task 0148). Discovery is read-only; deletion goes
//! through the existing `StartOperationRequestDto { operationType: "trash" }`
//! path once the user has reviewed and picked which candidates to remove.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::location::LocationDto;

/// Requests discovery of a `.app` bundle's related files
/// (`POST /api/v1/applications/uninstall/discover`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "location": {"providerId": "local", "uri": "file:///Applications/Widget.app"}
}))]
pub struct DiscoverApplicationUninstallCandidatesRequestDto {
    /// The `.app` bundle to uninstall.
    pub location: LocationDto,
}

/// One file or folder discovered under a well-known macOS location that
/// appears to belong to the application being uninstalled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationUninstallCandidateDto {
    /// The candidate's location.
    pub location: LocationDto,
    /// Total size in bytes (recursive for a directory).
    pub size_bytes: u64,
    /// Whether this candidate can actually be moved to the Trash. `false`
    /// for matches under `/Library`, which require elevation this feature
    /// does not implement - such candidates are reported so the user can see
    /// them, but must never be offered a way to remove them.
    pub removable: bool,
}

/// The result of scanning for an application's related files, for the user
/// to review before anything is deleted (task 0148). Nothing outside the
/// fixed set of well-known locations is ever touched, and nothing is
/// deleted by discovery itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverApplicationUninstallCandidatesResponseDto {
    /// The bundle's `CFBundleIdentifier`, when its `Info.plist` declared one.
    pub bundle_identifier: Option<String>,
    /// The bundle's product name, shown in the review checklist.
    pub product_name: String,
    /// Related files discovered outside the bundle itself.
    pub related_files: Vec<ApplicationUninstallCandidateDto>,
}

/// Requests removal of a `.app` bundle's pinned Dock icon, if it has one
/// (`POST /api/v1/applications/uninstall/remove-dock-icon`, task 0148
/// follow-up) - called once the user confirms an uninstall, so a Dock icon
/// left over from a trashed bundle doesn't dangle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "location": {"providerId": "local", "uri": "file:///Applications/Widget.app"}
}))]
pub struct RemoveApplicationDockIconRequestDto {
    /// The `.app` bundle being uninstalled.
    pub location: LocationDto,
}

/// Whether a pinned Dock icon was found and removed. `false` means there
/// simply was none to remove - a normal, non-error outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({"removed": true}))]
pub struct RemoveApplicationDockIconResponseDto {
    /// `true` when a matching Dock icon was found and removed.
    pub removed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_location() -> LocationDto {
        LocationDto {
            provider_id: "local".to_owned(),
            uri: "file:///Applications/Widget.app".to_owned(),
        }
    }

    #[test]
    fn discover_request_round_trips_through_json() {
        let request = DiscoverApplicationUninstallCandidatesRequestDto {
            location: sample_location(),
        };

        let json = serde_json::to_value(&request).expect("serialize");
        assert_eq!(json["location"]["uri"], "file:///Applications/Widget.app");
        let parsed: DiscoverApplicationUninstallCandidatesRequestDto =
            serde_json::from_value(json).expect("deserialize");
        assert_eq!(parsed, request);
    }

    #[test]
    fn discover_response_round_trips_through_json_with_camel_case_fields() {
        let response = DiscoverApplicationUninstallCandidatesResponseDto {
            bundle_identifier: Some("com.example.Widget".to_owned()),
            product_name: "Widget".to_owned(),
            related_files: vec![ApplicationUninstallCandidateDto {
                location: LocationDto {
                    provider_id: "local".to_owned(),
                    uri: "file:///Users/erik/Library/Caches/com.example.Widget".to_owned(),
                },
                size_bytes: 1024,
                removable: true,
            }],
        };

        let json = serde_json::to_value(&response).expect("serialize");
        assert_eq!(json["bundleIdentifier"], "com.example.Widget");
        assert_eq!(json["relatedFiles"][0]["sizeBytes"], 1024);
        assert_eq!(json["relatedFiles"][0]["removable"], true);
        let parsed: DiscoverApplicationUninstallCandidatesResponseDto =
            serde_json::from_value(json).expect("deserialize");
        assert_eq!(parsed, response);
    }

    #[test]
    fn remove_dock_icon_request_round_trips_through_json() {
        let request = RemoveApplicationDockIconRequestDto {
            location: sample_location(),
        };

        let json = serde_json::to_value(&request).expect("serialize");
        assert_eq!(json["location"]["uri"], "file:///Applications/Widget.app");
        let parsed: RemoveApplicationDockIconRequestDto =
            serde_json::from_value(json).expect("deserialize");
        assert_eq!(parsed, request);
    }

    #[test]
    fn remove_dock_icon_response_round_trips_through_json() {
        let response = RemoveApplicationDockIconResponseDto { removed: true };

        let json = serde_json::to_value(response).expect("serialize");
        assert_eq!(json["removed"], true);
        let parsed: RemoveApplicationDockIconResponseDto =
            serde_json::from_value(json).expect("deserialize");
        assert_eq!(parsed, response);
    }
}
