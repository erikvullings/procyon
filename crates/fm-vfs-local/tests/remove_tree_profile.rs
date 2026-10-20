//! Timing of recursive deletion of a large tree. Run with
//! `cargo test -p fm-vfs-local --release --test remove_tree_profile -- --ignored --nocapture`.

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use fm_domain::{EntryId, Location};
use fm_vfs::{EntryRef, FileSystemProvider};
use fm_vfs_local::LocalFileSystemProvider;
use tokio_util::sync::CancellationToken;

const DIRECTORIES: usize = 300;
const FILES_PER_DIRECTORY: usize = 200;

fn build(root: &std::path::Path) {
    for directory in 0..DIRECTORIES {
        let path = root.join(format!("dir-{directory:04}")).join("nested");
        fs::create_dir_all(&path).expect("create directory");
        for file in 0..FILES_PER_DIRECTORY {
            fs::write(path.join(format!("file-{file:05}.txt")), b"x").expect("create file");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "profiling"]
async fn profile_remove_tree() {
    let scratch = tempfile::tempdir().expect("temp dir");
    let root = scratch.path().join("tree");
    build(&root);
    let entry = EntryRef {
        id: EntryId::new(),
        location: Location::from_native_path(&root).expect("location"),
    };
    let removed = Arc::new(AtomicU64::new(0));
    let started = Instant::now();
    let report = LocalFileSystemProvider::new()
        .remove_tree(
            &entry,
            false,
            CancellationToken::new(),
            Arc::clone(&removed),
        )
        .await
        .expect("remove");
    println!(
        "remove_tree: {} items in {:?} ({} failures, counter {})",
        report.removed_items,
        started.elapsed(),
        report.failures.len(),
        removed.load(Ordering::Relaxed)
    );
    assert!(!root.exists());

    build(&root);
    let started = Instant::now();
    fs::remove_dir_all(&root).expect("remove_dir_all");
    println!("std remove_dir_all: {:?}", started.elapsed());
}
