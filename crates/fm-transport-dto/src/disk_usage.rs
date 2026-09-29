//! Wire types for local disk-usage analysis (task 0118).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::LocationDto;

/// Starts a recursive disk-usage scan rooted at one local directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScanDiskUsageRequestDto {
    /// Workspace that owns the scan and receives its progress events.
    pub workspace_id: Uuid,
    /// Caller-generated identifier used to correlate progress events.
    pub scan_id: Uuid,
    /// Local directory to scan.
    pub location: LocationDto,
    /// Exposes the immediate hierarchy when the scan root is normally collapsed.
    #[serde(default)]
    pub expand_root: bool,
}

/// Filesystem entry kind represented in a disk-usage tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiskUsageNodeKindDto {
    /// A directory, which may contain child nodes.
    Directory,
    /// A regular file.
    File,
    /// An unfollowed symbolic link.
    Symlink,
}

/// One node in the hierarchical disk-usage result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageNodeDto {
    /// Display name of the entry.
    pub name: String,
    /// Provider-neutral location used for navigation.
    pub location: LocationDto,
    /// Filesystem kind.
    pub kind: DiskUsageNodeKindDto,
    /// Apparent byte length, with hard-linked data counted once per scanned tree.
    pub logical_bytes: u64,
    /// Allocated bytes on Unix; equal to logical bytes on platforms without allocated-size data.
    pub physical_bytes: u64,
    /// Whether descendants were intentionally omitted from the response.
    #[serde(default)]
    pub collapsed: bool,
    /// Descendants retained by the backend depth cap.
    #[schema(no_recursion)]
    pub children: Vec<DiskUsageNodeDto>,
}

/// Why one filesystem entry could not be included in a disk-usage scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiskUsageUnreadableReasonDto {
    /// The entry exists but the scanning process lacked permission to read it.
    PermissionDenied,
    /// The entry was removed or renamed between being listed and being read.
    Disappeared,
    /// Any other I/O failure while reading metadata or directory contents.
    IoError,
    /// A directory on another volume (mount point); counted but not descended.
    OtherVolume,
    /// A directory whose contents are stored only in the cloud; not descended to avoid a download.
    CloudOnly,
}

/// One filesystem entry skipped during a disk-usage scan, with enough context to show the
/// caller which path was unreadable and why, without leaking raw OS error strings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageUnreadableEntryDto {
    /// The location that could not be read.
    pub location: LocationDto,
    /// Sanitized reason the entry was skipped.
    pub reason: DiskUsageUnreadableReasonDto,
}

/// Why a scanned folder is suggested as a clean-up candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiskUsageCleanupKindDto {
    /// A JavaScript `node_modules` dependency tree.
    NodeModules,
    /// A Python virtual environment (`.venv`, or `venv`/`env` with `pyvenv.cfg`).
    PythonVirtualEnvironment,
    /// A Rust `target` directory next to `Cargo.toml`.
    RustBuildOutput,
    /// A Next.js `.next` directory next to `package.json`.
    NextBuildOutput,
    /// Xcode `DerivedData` build products.
    XcodeDerivedData,
    /// Xcode device-support symbol caches.
    XcodeDeviceSupport,
    /// Application caches under `Library/Caches` (or simulator caches).
    ApplicationCaches,
    /// Package-manager or tool caches such as `~/.cache` or the npm cache.
    ToolCache,
}

/// A large folder that can usually be regenerated, found by structural heuristics only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageCleanupCandidateDto {
    /// The candidate folder.
    pub location: LocationDto,
    /// Rule that matched the folder.
    pub kind: DiskUsageCleanupKindDto,
    /// Apparent byte length of the folder's contents.
    pub logical_bytes: u64,
    /// Allocated bytes of the folder's contents.
    pub physical_bytes: u64,
}

/// Completed hierarchical disk-usage scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScanDiskUsageResponseDto {
    /// Root of the scanned hierarchy.
    pub root: DiskUsageNodeDto,
    /// Entries skipped because metadata or directory contents could not be read.
    pub unreadable_entries: u64,
    /// Bounded detail list (capped) for entries counted in `unreadable_entries`, stable-sorted
    /// by location.
    #[serde(default)]
    pub unreadable: Vec<DiskUsageUnreadableEntryDto>,
    /// Filesystem entries visited so far, so progress can advance visibly even while no
    /// top-level subtree has finished.
    #[serde(default)]
    pub scanned_entries: u64,
    /// Largest regenerable folders in the whole scanned tree, largest first and capped. Only
    /// populated once traversal has finished.
    #[serde(default)]
    pub cleanup_candidates: Vec<DiskUsageCleanupCandidateDto>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn disk_usage_request_defaults_expand_root_to_false() {
        let workspace_id = Uuid::new_v4();
        let scan_id = Uuid::new_v4();
        let request: ScanDiskUsageRequestDto = serde_json::from_value(json!({
            "workspaceId": workspace_id,
            "scanId": scan_id,
            "location": {
                "providerId": "local",
                "uri": "file:///fixture"
            }
        }))
        .expect("request must deserialize");

        assert_eq!(request.workspace_id, workspace_id);
        assert_eq!(request.scan_id, scan_id);
        assert!(!request.expand_root);
    }

    #[test]
    fn disk_usage_response_round_trips_with_camel_case_sizes() {
        let response = ScanDiskUsageResponseDto {
            root: DiskUsageNodeDto {
                name: "src".to_owned(),
                location: LocationDto {
                    provider_id: "local".to_owned(),
                    uri: "file:///tmp/src".to_owned(),
                },
                kind: DiskUsageNodeKindDto::Directory,
                logical_bytes: 12,
                physical_bytes: 4096,
                collapsed: false,
                children: Vec::new(),
            },
            unreadable_entries: 1,
            unreadable: vec![DiskUsageUnreadableEntryDto {
                location: LocationDto {
                    provider_id: "local".to_owned(),
                    uri: "file:///tmp/src/locked".to_owned(),
                },
                reason: DiskUsageUnreadableReasonDto::PermissionDenied,
            }],
            scanned_entries: 42,
            cleanup_candidates: vec![DiskUsageCleanupCandidateDto {
                location: LocationDto {
                    provider_id: "local".to_owned(),
                    uri: "file:///tmp/src/node_modules".to_owned(),
                },
                kind: DiskUsageCleanupKindDto::NodeModules,
                logical_bytes: 10,
                physical_bytes: 20,
            }],
        };

        let json = serde_json::to_string(&response).expect("serialization must succeed");
        assert!(json.contains("\"logicalBytes\":12"));
        assert!(json.contains("\"physicalBytes\":4096"));
        assert!(json.contains("\"unreadableEntries\":1"));
        assert!(json.contains("\"scannedEntries\":42"));
        assert!(json.contains("\"permissionDenied\""));
        assert!(json.contains("\"cleanupCandidates\":[{"));
        assert!(json.contains("\"nodeModules\""));
        assert_eq!(
            serde_json::from_str::<ScanDiskUsageResponseDto>(&json)
                .expect("deserialization must succeed"),
            response
        );
    }

    #[test]
    fn disk_usage_unreadable_reason_serializes_camel_case() {
        for (reason, expected) in [
            (
                DiskUsageUnreadableReasonDto::PermissionDenied,
                "\"permissionDenied\"",
            ),
            (DiskUsageUnreadableReasonDto::Disappeared, "\"disappeared\""),
            (DiskUsageUnreadableReasonDto::IoError, "\"ioError\""),
            (DiskUsageUnreadableReasonDto::OtherVolume, "\"otherVolume\""),
            (DiskUsageUnreadableReasonDto::CloudOnly, "\"cloudOnly\""),
        ] {
            assert_eq!(
                serde_json::to_string(&reason).expect("reason must serialize"),
                expected
            );
        }
    }

    #[test]
    fn disk_usage_response_defaults_new_fields_when_absent() {
        let response: ScanDiskUsageResponseDto = serde_json::from_value(json!({
            "root": {
                "name": "src",
                "location": {"providerId": "local", "uri": "file:///tmp/src"},
                "kind": "directory",
                "logicalBytes": 0,
                "physicalBytes": 0,
                "children": []
            },
            "unreadableEntries": 0
        }))
        .expect("response must deserialize without the new fields");

        assert!(response.unreadable.is_empty());
        assert!(response.cleanup_candidates.is_empty());
        assert_eq!(response.scanned_entries, 0);
    }
}
