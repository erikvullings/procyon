//! Clean-up candidate heuristics for disk-usage scans (task 0232).
//!
//! Recognises regenerable folders (dependency trees, build output, caches) purely from the scanned
//! structure. Rules are adapted from BlitzTree's `cleanup.rs`
//! (<https://github.com/ahmedkhaleel2004/blitztree>, MIT licence). Nothing here deletes anything:
//! candidates are only reported, and removal goes through the normal trash operation.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use crate::ApplicationError;

/// Folders smaller than this are not worth suggesting.
pub(crate) const MIN_CLEANUP_BYTES: u64 = 50 * 1024 * 1024;
/// Bounds the candidate list in responses and progress events.
pub(crate) const MAX_CLEANUP_CANDIDATES: usize = 100;

/// Why a folder is considered safe to regenerate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CleanupKind {
    NodeModules,
    PythonVirtualEnvironment,
    RustBuildOutput,
    NextBuildOutput,
    XcodeDerivedData,
    XcodeDeviceSupport,
    ApplicationCaches,
    ToolCache,
}

/// A scanned folder that matches one clean-up rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CleanupCandidate {
    pub(crate) path: PathBuf,
    pub(crate) kind: CleanupKind,
    pub(crate) logical_bytes: u64,
    pub(crate) physical_bytes: u64,
}

/// Read-only view of a scanned tree node, so the rules stay independent of the scanner's types.
pub(crate) trait CleanupTreeNode: Sized {
    fn entry_name(&self) -> &OsStr;
    fn is_directory(&self) -> bool;
    fn logical_bytes(&self) -> u64;
    fn physical_bytes(&self) -> u64;
    fn child_nodes(&self) -> &[Self];
}

/// Classifies the directory at `path` from its name, its ancestors' names, its siblings and its
/// own children.
pub(crate) fn classify_cleanup<T: CleanupTreeNode>(
    path: &Path,
    siblings: &[T],
    children: &[T],
) -> Option<CleanupKind> {
    let name = path.file_name()?.to_str()?;
    let parent = path.parent();
    let parent_name = parent.and_then(Path::file_name).and_then(OsStr::to_str);
    let grandparent_name = parent
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(OsStr::to_str);
    let has_sibling = |wanted: &str| siblings.iter().any(|node| node.entry_name() == wanted);
    let has_child = |wanted: &str| children.iter().any(|node| node.entry_name() == wanted);

    match name {
        "node_modules" => Some(CleanupKind::NodeModules),
        ".venv" => Some(CleanupKind::PythonVirtualEnvironment),
        "venv" | "env" if has_child("pyvenv.cfg") => Some(CleanupKind::PythonVirtualEnvironment),
        "target" if has_sibling("Cargo.toml") => Some(CleanupKind::RustBuildOutput),
        ".next" if has_sibling("package.json") => Some(CleanupKind::NextBuildOutput),
        "DerivedData" if parent_name == Some("Xcode") => Some(CleanupKind::XcodeDerivedData),
        _ if name.ends_with("DeviceSupport") && parent_name == Some("Xcode") => {
            Some(CleanupKind::XcodeDeviceSupport)
        }
        "Caches" if matches!(parent_name, Some("Library" | "CoreSimulator")) => {
            Some(CleanupKind::ApplicationCaches)
        }
        // `~/.cache` (XDG) only, recognised as `/home/<user>/.cache` or `/Users/<user>/.cache`.
        ".cache" if matches!(grandparent_name, Some("home" | "Users")) => {
            Some(CleanupKind::ToolCache)
        }
        "_cacache" if parent_name == Some(".npm") => Some(CleanupKind::ToolCache),
        "caches" if parent_name == Some(".gradle") => Some(CleanupKind::ToolCache),
        ".pnpm-store" => Some(CleanupKind::ToolCache),
        _ => None,
    }
}

/// Walks the full scanned tree below `root` and returns the largest candidates of at least
/// `min_bytes` (physical) first.
///
/// A matched folder is never descended into, so a `node_modules` nested in another
/// `node_modules` is not reported twice. The scan root itself is not a candidate.
pub(crate) fn find_cleanup_candidates<T: CleanupTreeNode>(
    root: &T,
    root_path: &Path,
    min_bytes: u64,
    cancellation: &CancellationToken,
) -> Result<Vec<CleanupCandidate>, ApplicationError> {
    let mut candidates = Vec::new();
    let mut pending = vec![(root, root_path.to_path_buf())];
    while let Some((node, path)) = pending.pop() {
        if cancellation.is_cancelled() {
            return Err(ApplicationError::OperationCancelled);
        }
        let siblings = node.child_nodes();
        for child in siblings.iter().filter(|child| child.is_directory()) {
            let child_path = path.join(child.entry_name());
            match classify_cleanup(&child_path, siblings, child.child_nodes()) {
                Some(kind) => {
                    if child.physical_bytes() >= min_bytes {
                        candidates.push(CleanupCandidate {
                            path: child_path,
                            kind,
                            logical_bytes: child.logical_bytes(),
                            physical_bytes: child.physical_bytes(),
                        });
                    }
                }
                None => pending.push((child, child_path)),
            }
        }
    }
    candidates.sort_by(|left, right| {
        right
            .physical_bytes
            .cmp(&left.physical_bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    candidates.truncate(MAX_CLEANUP_CANDIDATES);
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    const BIG: u64 = MIN_CLEANUP_BYTES;

    struct Node {
        name: OsString,
        directory: bool,
        bytes: u64,
        children: Vec<Node>,
    }

    impl CleanupTreeNode for Node {
        fn entry_name(&self) -> &OsStr {
            &self.name
        }
        fn is_directory(&self) -> bool {
            self.directory
        }
        fn logical_bytes(&self) -> u64 {
            self.bytes
        }
        fn physical_bytes(&self) -> u64 {
            self.bytes
        }
        fn child_nodes(&self) -> &[Self] {
            &self.children
        }
    }

    fn dir(name: &str, children: Vec<Node>) -> Node {
        let bytes = children.iter().map(|child| child.bytes).sum();
        Node {
            name: name.into(),
            directory: true,
            bytes,
            children,
        }
    }

    fn file(name: &str, bytes: u64) -> Node {
        Node {
            name: name.into(),
            directory: false,
            bytes,
            children: Vec::new(),
        }
    }

    fn found(root: &Node, root_path: &str) -> Vec<(String, CleanupKind)> {
        find_cleanup_candidates(
            root,
            Path::new(root_path),
            MIN_CLEANUP_BYTES,
            &CancellationToken::new(),
        )
        .expect("walk must succeed")
        .into_iter()
        .map(|candidate| {
            let relative = candidate
                .path
                .strip_prefix(root_path)
                .expect("candidates live below the root")
                .iter()
                .map(|part| part.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            (relative, candidate.kind)
        })
        .collect()
    }

    #[test]
    fn recognises_project_build_output_only_next_to_its_manifest() {
        let root = dir(
            "code",
            vec![
                dir(
                    "rust-app",
                    vec![file("Cargo.toml", 1), dir("target", vec![file("a", BIG)])],
                ),
                dir("maven-app", vec![dir("target", vec![file("a", BIG)])]),
                dir(
                    "web",
                    vec![
                        file("package.json", 1),
                        dir(".next", vec![file("a", BIG)]),
                        dir("node_modules", vec![file("a", BIG * 2)]),
                    ],
                ),
            ],
        );

        assert_eq!(
            found(&root, "/code"),
            vec![
                ("web/node_modules".to_owned(), CleanupKind::NodeModules),
                ("rust-app/target".to_owned(), CleanupKind::RustBuildOutput),
                ("web/.next".to_owned(), CleanupKind::NextBuildOutput),
            ]
        );
    }

    #[test]
    fn requires_a_pyvenv_marker_for_generic_virtual_environment_names() {
        let root = dir(
            "p",
            vec![
                dir("venv", vec![file("pyvenv.cfg", 1), file("lib", BIG)]),
                dir("env", vec![file("settings", BIG)]),
                dir(".venv", vec![file("lib", BIG)]),
            ],
        );

        let kinds = found(&root, "/p");
        assert_eq!(kinds.len(), 2);
        assert!(
            kinds
                .iter()
                .all(|(_, kind)| *kind == CleanupKind::PythonVirtualEnvironment)
        );
        assert!(!kinds.iter().any(|(path, _)| path == "env"));
    }

    #[test]
    fn recognises_caches_by_their_parent_folders() {
        let root = dir(
            "alice",
            vec![
                dir(
                    "Library",
                    vec![
                        dir("Caches", vec![file("a", BIG)]),
                        dir(
                            "Developer",
                            vec![dir(
                                "Xcode",
                                vec![
                                    dir("DerivedData", vec![file("a", BIG)]),
                                    dir("iOS DeviceSupport", vec![file("a", BIG)]),
                                ],
                            )],
                        ),
                    ],
                ),
                dir(".npm", vec![dir("_cacache", vec![file("a", BIG)])]),
                dir(".cache", vec![file("a", BIG)]),
                dir("Caches", vec![file("a", BIG)]),
            ],
        );

        let mut kinds = found(&root, "/Users/alice");
        kinds.sort_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(
            kinds,
            vec![
                (".cache".to_owned(), CleanupKind::ToolCache),
                (".npm/_cacache".to_owned(), CleanupKind::ToolCache),
                ("Library/Caches".to_owned(), CleanupKind::ApplicationCaches),
                (
                    "Library/Developer/Xcode/DerivedData".to_owned(),
                    CleanupKind::XcodeDerivedData
                ),
                (
                    "Library/Developer/Xcode/iOS DeviceSupport".to_owned(),
                    CleanupKind::XcodeDeviceSupport
                ),
            ]
        );
    }

    #[test]
    fn skips_small_and_nested_candidates() {
        let root = dir(
            "app",
            vec![
                dir(
                    "node_modules",
                    vec![dir(
                        "pkg",
                        vec![dir("node_modules", vec![file("a", BIG * 3)])],
                    )],
                ),
                dir("other", vec![dir("node_modules", vec![file("a", BIG - 1)])]),
            ],
        );

        assert_eq!(
            found(&root, "/app"),
            vec![("node_modules".to_owned(), CleanupKind::NodeModules)]
        );
    }

    #[test]
    fn stops_when_cancelled() {
        let root = dir("a", vec![dir("b", Vec::new())]);
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(matches!(
            find_cleanup_candidates(&root, Path::new("/a"), 0, &cancellation),
            Err(ApplicationError::OperationCancelled)
        ));
    }
}
