//! Wire types for local disk-usage analysis (task 0118).

use std::collections::BTreeMap;

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

/// [`DiskUsageTreeDto::flags`] bits holding the node kind: `0` directory, `1` file, `2` symlink.
pub const DISK_USAGE_FLAG_KIND_MASK: u8 = 0b11;
/// [`DiskUsageTreeDto::flags`] bit set when a directory's descendants were omitted.
pub const DISK_USAGE_FLAG_COLLAPSED: u8 = 0b100;

/// A disk-usage hierarchy as parallel arrays in pre-order (every parent precedes its children,
/// and siblings keep their order), so a large tree doesn't repeat a full location per node.
///
/// Node `0` is the root. A node's URI is its parent's URI joined with `/` and its name, unless
/// `uri_overrides` holds an explicit URI for its index (names needing percent-encoding, or
/// synthetic nodes that share their parent's location).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageTreeDto {
    /// Provider shared by every node.
    pub provider_id: String,
    /// URI of the root node.
    pub root_uri: String,
    /// Index of each node's parent; the root's own entry is `0` and is ignored.
    pub parents: Vec<u32>,
    /// Display name of each node.
    pub names: Vec<String>,
    /// Kind (bits 0–1) and collapsed flag (bit 2) of each node.
    pub flags: Vec<u8>,
    /// Apparent byte length of each node.
    pub logical_bytes: Vec<u64>,
    /// Allocated byte length of each node.
    pub physical_bytes: Vec<u64>,
    /// Explicit URIs keyed by decimal node index, for nodes whose URI is not derivable.
    #[serde(default)]
    pub uri_overrides: BTreeMap<String, String>,
}

/// Joins a child name onto a parent URI the way [`DiskUsageTreeDto`] derives node URIs.
#[must_use]
pub fn join_disk_usage_uri(parent_uri: &str, name: &str) -> String {
    if parent_uri.ends_with('/') {
        format!("{parent_uri}{name}")
    } else {
        format!("{parent_uri}/{name}")
    }
}

impl DiskUsageTreeDto {
    /// Flattens a nested hierarchy.
    #[must_use]
    pub fn from_root(root: &DiskUsageNodeDto) -> Self {
        let mut tree = Self {
            provider_id: root.location.provider_id.clone(),
            root_uri: root.location.uri.clone(),
            parents: Vec::new(),
            names: Vec::new(),
            flags: Vec::new(),
            logical_bytes: Vec::new(),
            physical_bytes: Vec::new(),
            uri_overrides: BTreeMap::new(),
        };
        // (node, parent index, parent URI) — an explicit stack keeps deep trees off the call stack.
        let mut stack: Vec<(&DiskUsageNodeDto, u32, Option<&str>)> = vec![(root, 0, None)];
        while let Some((node, parent, parent_uri)) = stack.pop() {
            let index = tree.names.len();
            if let Some(parent_uri) = parent_uri
                && join_disk_usage_uri(parent_uri, &node.name) != node.location.uri
            {
                tree.uri_overrides
                    .insert(index.to_string(), node.location.uri.clone());
            }
            tree.parents.push(parent);
            tree.names.push(node.name.clone());
            let kind = match node.kind {
                DiskUsageNodeKindDto::Directory => 0,
                DiskUsageNodeKindDto::File => 1,
                DiskUsageNodeKindDto::Symlink => 2,
            };
            tree.flags.push(if node.collapsed {
                kind | DISK_USAGE_FLAG_COLLAPSED
            } else {
                kind
            });
            tree.logical_bytes.push(node.logical_bytes);
            tree.physical_bytes.push(node.physical_bytes);
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            for child in node.children.iter().rev() {
                stack.push((child, index, Some(node.location.uri.as_str())));
            }
        }
        tree
    }

    /// Rebuilds the nested hierarchy, or `None` when the arrays are inconsistent.
    #[must_use]
    pub fn to_root(&self) -> Option<DiskUsageNodeDto> {
        let count = self.names.len();
        if count == 0
            || self.parents.len() != count
            || self.flags.len() != count
            || self.logical_bytes.len() != count
            || self.physical_bytes.len() != count
        {
            return None;
        }
        let mut uris: Vec<String> = Vec::with_capacity(count);
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); count];
        for index in 0..count {
            let uri = if let Some(uri) = self.uri_overrides.get(&index.to_string()) {
                uri.clone()
            } else if index == 0 {
                self.root_uri.clone()
            } else {
                let parent = usize::try_from(self.parents[index]).ok()?;
                join_disk_usage_uri(uris.get(parent)?, &self.names[index])
            };
            if index > 0 {
                let parent = usize::try_from(self.parents[index]).ok()?;
                if parent >= index {
                    return None;
                }
                children[parent].push(index);
            }
            uris.push(uri);
        }
        let mut built: Vec<Option<DiskUsageNodeDto>> = vec![None; count];
        for index in (0..count).rev() {
            let kind = match self.flags[index] & DISK_USAGE_FLAG_KIND_MASK {
                0 => DiskUsageNodeKindDto::Directory,
                1 => DiskUsageNodeKindDto::File,
                2 => DiskUsageNodeKindDto::Symlink,
                _ => return None,
            };
            let node_children = children[index]
                .iter()
                .map(|child| built[*child].take())
                .collect::<Option<Vec<_>>>()?;
            built[index] = Some(DiskUsageNodeDto {
                name: self.names[index].clone(),
                location: LocationDto {
                    provider_id: self.provider_id.clone(),
                    uri: uris[index].clone(),
                },
                kind,
                logical_bytes: self.logical_bytes[index],
                physical_bytes: self.physical_bytes[index],
                collapsed: self.flags[index] & DISK_USAGE_FLAG_COLLAPSED != 0,
                children: node_children,
            });
        }
        built[0].take()
    }
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

    fn node(
        name: &str,
        uri: &str,
        kind: DiskUsageNodeKindDto,
        bytes: u64,
        children: Vec<DiskUsageNodeDto>,
    ) -> DiskUsageNodeDto {
        DiskUsageNodeDto {
            name: name.to_owned(),
            location: LocationDto {
                provider_id: "local".to_owned(),
                uri: uri.to_owned(),
            },
            kind,
            logical_bytes: bytes,
            physical_bytes: bytes + 1,
            collapsed: false,
            children,
        }
    }

    fn sample_tree() -> DiskUsageNodeDto {
        let mut collapsed = node(
            "node_modules",
            "file:///root/app/node_modules",
            DiskUsageNodeKindDto::Directory,
            40,
            Vec::new(),
        );
        collapsed.collapsed = true;
        node(
            "root",
            "file:///root",
            DiskUsageNodeKindDto::Directory,
            100,
            vec![
                node(
                    "app",
                    "file:///root/app",
                    DiskUsageNodeKindDto::Directory,
                    60,
                    vec![
                        collapsed,
                        node(
                            "my file.txt",
                            "file:///root/app/my%20file.txt",
                            DiskUsageNodeKindDto::File,
                            20,
                            Vec::new(),
                        ),
                    ],
                ),
                node(
                    "link",
                    "file:///root/link",
                    DiskUsageNodeKindDto::Symlink,
                    0,
                    Vec::new(),
                ),
                node(
                    "Small files (3)",
                    "file:///root",
                    DiskUsageNodeKindDto::File,
                    40,
                    Vec::new(),
                ),
            ],
        )
    }

    #[test]
    fn flat_tree_round_trips_in_pre_order_with_only_irregular_uris_overridden() {
        let root = sample_tree();
        let tree = DiskUsageTreeDto::from_root(&root);

        assert_eq!(
            tree.names,
            [
                "root",
                "app",
                "node_modules",
                "my file.txt",
                "link",
                "Small files (3)"
            ]
        );
        assert_eq!(tree.parents, [0, 0, 1, 1, 0, 0]);
        assert_eq!(tree.flags, [0, 0, DISK_USAGE_FLAG_COLLAPSED, 1, 2, 1]);
        assert_eq!(
            tree.uri_overrides.keys().collect::<Vec<_>>(),
            ["3", "5"],
            "only the percent-encoded name and the aggregate need explicit URIs"
        );
        let json = serde_json::to_string(&tree).expect("serialize tree");
        let decoded: DiskUsageTreeDto = serde_json::from_str(&json).expect("deserialize tree");
        assert_eq!(decoded.to_root(), Some(root));
    }

    #[test]
    fn flat_tree_rejects_inconsistent_arrays() {
        let mut tree = DiskUsageTreeDto::from_root(&sample_tree());
        tree.parents[2] = 4;
        assert_eq!(tree.to_root(), None, "a parent must precede its child");

        let mut tree = DiskUsageTreeDto::from_root(&sample_tree());
        tree.flags.pop();
        assert_eq!(tree.to_root(), None);
    }

    #[test]
    fn flat_tree_is_far_smaller_than_the_nested_json_for_a_deep_tree() {
        fn build(uri: &str, depth: u32) -> DiskUsageNodeDto {
            let children = if depth == 0 {
                Vec::new()
            } else {
                (0..6)
                    .map(|index| {
                        let name = format!("folder-{index}");
                        build(&join_disk_usage_uri(uri, &name), depth - 1)
                    })
                    .collect()
            };
            let name = uri.rsplit('/').next().unwrap_or_default();
            node(
                name,
                uri,
                DiskUsageNodeKindDto::Directory,
                1_234_567,
                children,
            )
        }
        let root = build("file:///Users/someone/Projects", 5);
        let nested = serde_json::to_vec(&root).expect("nested").len();
        let flat = serde_json::to_vec(&DiskUsageTreeDto::from_root(&root))
            .expect("flat")
            .len();

        assert!(
            flat * 3 < nested,
            "flat {flat} bytes should be under a third of nested {nested} bytes"
        );
    }

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
