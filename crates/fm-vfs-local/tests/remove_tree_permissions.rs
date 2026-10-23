//! Deleting a tree that contains directories the owner cannot list or modify.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use fm_domain::{EntryId, Location};
use fm_vfs::{EntryRef, FileSystemProvider};
use fm_vfs_local::LocalFileSystemProvider;
use tokio_util::sync::CancellationToken;

async fn remove(root: &Path, override_read_only: bool) -> fm_vfs::RemoveReport {
    let entry = EntryRef {
        id: EntryId::new(),
        location: Location::from_native_path(root).expect("location"),
    };
    LocalFileSystemProvider::new()
        .remove_tree(
            &entry,
            override_read_only,
            CancellationToken::new(),
            Arc::new(AtomicU64::new(0)),
        )
        .await
        .expect("remove")
}

fn build_blocked_tree(scratch: &Path) -> std::path::PathBuf {
    let root = scratch.join("tree");
    let blocked = root.join("home").join("Blocked folder");
    fs::create_dir_all(blocked.join("inner")).expect("create directories");
    fs::write(blocked.join("file.txt"), b"x").expect("create file");
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).expect("block folder");
    root
}

#[tokio::test(flavor = "multi_thread")]
async fn override_removes_directories_without_owner_permissions() {
    let scratch = tempfile::tempdir().expect("temp dir");
    let root = build_blocked_tree(scratch.path());

    let report = remove(&root, true).await;

    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(!root.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn without_override_blocked_directories_stay_and_are_reported() {
    let scratch = tempfile::tempdir().expect("temp dir");
    let root = build_blocked_tree(scratch.path());

    let report = remove(&root, false).await;

    assert!(!report.failures.is_empty());
    assert!(root.exists());
    // Restore access so the temp dir can be cleaned up.
    let blocked = root.join("home").join("Blocked folder");
    fs::set_permissions(blocked, fs::Permissions::from_mode(0o700)).expect("restore");
}
