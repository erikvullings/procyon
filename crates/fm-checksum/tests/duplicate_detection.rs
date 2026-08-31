//! Staged duplicate detection over a real local fixture tree (task 0077
//! acceptance criteria: duplicate grouping including same-size-different-
//! content and hardlinked files, and cancellation).

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use fm_checksum::{
    DuplicateCandidate, DuplicateObserver, DuplicateOptions, DuplicateProgress, DuplicateScan,
    DuplicateStage, ScanOutcome, find_duplicates, find_duplicates_observed,
};
use fm_domain::{EntryId, Location};
use fm_vfs::{EntryRef, ProviderRegistry};
use fm_vfs_local::LocalFileSystemProvider;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

fn registry() -> ProviderRegistry {
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(LocalFileSystemProvider::new()));
    providers
}

fn candidate_for(path: &Path) -> DuplicateCandidate {
    let size = fs::metadata(path).map_or(0, |metadata| metadata.len());
    candidate_of_size(path, size)
}

/// Builds a candidate for a path that need not exist, with a declared size.
///
/// Used to plant a file the detector must never open.
fn candidate_of_size(path: &Path, size: u64) -> DuplicateCandidate {
    let location = Location::from_native_path(path).expect("path must be a valid location");
    DuplicateCandidate::new(
        EntryRef {
            id: EntryId::new(),
            location,
        },
        size,
    )
}

fn write(directory: &Path, name: &str, contents: &[u8]) -> std::path::PathBuf {
    let path = directory.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("fixture directory must be created");
    }
    fs::write(&path, contents).expect("fixture file must be written");
    path
}

struct Fixture {
    _directory: TempDir,
    candidates: Vec<DuplicateCandidate>,
    identical: (std::path::PathBuf, std::path::PathBuf),
    hardlinks: (std::path::PathBuf, std::path::PathBuf),
}

/// A tree containing every case the acceptance criteria name.
fn fixture() -> Fixture {
    let directory = tempfile::tempdir().expect("temp dir must be created");
    let root = directory.path();

    // Two byte-identical files with separate inodes: true duplicates.
    let one = write(root, "left/one.txt", b"identical payload");
    let two = write(root, "right/two.txt", b"identical payload");

    // Same size, different content: must survive stage 1 but be rejected by
    // the partial hash, and must never be reported.
    write(root, "left/block.bin", &[b'A'; 128]);
    write(root, "right/block.bin", &[b'B'; 128]);

    // A hardlinked pair: one file reachable through two paths.
    let source = write(root, "left/linked.dat", b"linked payload!!");
    let alias = root.join("right/linked-alias.dat");
    fs::create_dir_all(alias.parent().expect("parent must exist"))
        .expect("directory must be created");
    fs::hard_link(&source, &alias).expect("hardlink must be created");

    // A file whose size is unique: stage 1 must discard it untouched.
    write(root, "left/unique.txt", b"xyz");

    let mut candidates: Vec<DuplicateCandidate> = [
        "left/one.txt",
        "right/two.txt",
        "left/block.bin",
        "right/block.bin",
        "left/linked.dat",
        "right/linked-alias.dat",
        "left/unique.txt",
    ]
    .iter()
    .map(|relative| candidate_for(&root.join(relative)))
    .collect();

    // A candidate that does not exist on disk, with a size shared by nothing.
    // If stage 1 ever leaked a singleton into a hashing stage, opening this
    // would fail and show up as a warning — so `warnings.is_empty()` is a
    // direct assertion that singletons are never hashed.
    candidates.push(candidate_of_size(
        &root.join("left/never-opened.bin"),
        7_777,
    ));

    Fixture {
        _directory: directory,
        candidates,
        identical: (one, two),
        hardlinks: (source, alias),
    }
}

async fn scan(fixture: &Fixture) -> DuplicateScan {
    find_duplicates(
        &registry(),
        fixture.candidates.clone(),
        &DuplicateOptions::default(),
        &CancellationToken::new(),
    )
    .await
}

fn uris(paths: &[&Path]) -> Vec<String> {
    let mut uris: Vec<String> = paths
        .iter()
        .map(|path| {
            Location::from_native_path(path)
                .expect("path must be a valid location")
                .uri
        })
        .collect();
    uris.sort();
    uris
}

#[tokio::test]
async fn groups_byte_identical_files_and_ignores_same_size_different_content() {
    let fixture = fixture();
    let result = scan(&fixture).await;

    assert_eq!(result.outcome, ScanOutcome::Completed);
    assert!(result.is_complete());
    assert_eq!(result.groups.len(), 2, "groups: {:#?}", result.groups);

    let identical = result
        .groups
        .iter()
        .find(|group| group.distinct_files.len() == 2)
        .expect("the true-duplicate group must be reported");
    let found = {
        let mut found: Vec<String> = identical
            .distinct_files
            .iter()
            .map(|file| file.entry.location.uri.clone())
            .collect();
        found.sort();
        found
    };
    assert_eq!(found, uris(&[&fixture.identical.0, &fixture.identical.1]));
    assert_eq!(identical.size, 17);
    assert_eq!(identical.reclaimable_bytes(), 17);

    // The same-size-different-content pair must appear nowhere.
    let all_uris: Vec<String> = result
        .groups
        .iter()
        .flat_map(|group| {
            group
                .distinct_files
                .iter()
                .chain(group.hardlink_clusters.iter().flat_map(|c| c.files.iter()))
        })
        .map(|file| file.entry.location.uri.clone())
        .collect();
    assert!(
        all_uris.iter().all(|uri| !uri.ends_with("block.bin")),
        "same-size-different-content files must not be reported: {all_uris:?}"
    );
}

#[tokio::test]
async fn reports_hardlinks_as_a_distinct_category() {
    let fixture = fixture();
    let result = scan(&fixture).await;

    let linked = result
        .groups
        .iter()
        .find(|group| !group.hardlink_clusters.is_empty())
        .expect("the hardlinked pair must be reported");

    assert_eq!(linked.hardlink_clusters.len(), 1);
    let cluster = &linked.hardlink_clusters[0];
    assert_eq!(cluster.files.len(), 2);
    let mut found: Vec<String> = cluster
        .files
        .iter()
        .map(|file| file.entry.location.uri.clone())
        .collect();
    found.sort();
    assert_eq!(found, uris(&[&fixture.hardlinks.0, &fixture.hardlinks.1]));

    // The whole point of the distinction: these are one file, not two copies,
    // so they are not listed as distinct duplicates and free nothing.
    assert!(linked.distinct_files.is_empty());
    assert_eq!(linked.reclaimable_bytes(), 0);
    assert_eq!(linked.path_count(), 2);
}

#[tokio::test]
async fn never_hashes_a_file_whose_size_or_prefix_is_unique() {
    let fixture = fixture();
    let result = scan(&fixture).await;

    assert!(
        result.warnings.is_empty(),
        "the never-opened candidate must not have been read: {:?}",
        result.warnings
    );
    assert_eq!(result.stats.failed, 0);
    assert_eq!(result.stats.candidates, 8);
    // Six files share a size with another (2 identical, 2 blocks, 2 links);
    // the unique-size file and the planted non-existent one are dropped.
    assert_eq!(result.stats.size_survivors, 6);
    // The hardlinked pair is one file, so it is hashed once, not twice.
    assert_eq!(result.stats.partially_hashed, 5);
    // Only the identical pair and the hardlinked file reach a full hash: the
    // two same-size blocks were rejected by their prefix.
    assert_eq!(result.stats.fully_hashed, 3);
}

/// Cancels the scan the moment the third stage has hashed two files, so the
/// test does not depend on timing.
struct CancelDuringFullHash {
    cancellation: CancellationToken,
    full_hash_reports: AtomicUsize,
}

impl DuplicateObserver for CancelDuringFullHash {
    fn on_progress(&self, progress: DuplicateProgress) {
        if progress.stage == DuplicateStage::FullHash {
            let seen = self.full_hash_reports.fetch_add(1, Ordering::SeqCst) + 1;
            if seen >= 2 {
                self.cancellation.cancel();
            }
        }
    }
}

#[tokio::test]
async fn cancellation_during_the_full_hash_stage_reports_no_false_success() {
    let directory = tempfile::tempdir().expect("temp dir must be created");
    let root = directory.path();
    // Many identical files, so stage 3 has plenty of work left when the
    // observer cancels it.
    let payload = vec![b'z'; 512 * 1024];
    let candidates: Vec<DuplicateCandidate> = (0..40)
        .map(|index| {
            let path = write(root, &format!("copy-{index:02}.bin"), &payload);
            candidate_for(&path)
        })
        .collect();

    let cancellation = CancellationToken::new();
    let observer = CancelDuringFullHash {
        cancellation: cancellation.clone(),
        full_hash_reports: AtomicUsize::new(0),
    };

    let started = Instant::now();
    let result = find_duplicates_observed(
        &registry(),
        candidates,
        &DuplicateOptions::default(),
        Some(&observer),
        &cancellation,
    )
    .await;
    let elapsed = started.elapsed();

    assert_eq!(
        result.outcome,
        ScanOutcome::Cancelled,
        "a cancelled scan must never claim completion"
    );
    assert!(!result.is_complete());
    assert!(
        result.groups.is_empty(),
        "a cancelled scan must not present partial groups as results"
    );
    assert!(
        result.stats.fully_hashed < 40,
        "cancellation must stop before every file is hashed, got {}",
        result.stats.fully_hashed
    );
    assert!(
        elapsed < Duration::from_secs(20),
        "cancellation must be prompt, took {elapsed:?}"
    );
}

#[tokio::test]
async fn cancelling_before_the_scan_starts_produces_no_results() {
    let fixture = fixture();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let result = find_duplicates(
        &registry(),
        fixture.candidates.clone(),
        &DuplicateOptions::default(),
        &cancellation,
    )
    .await;
    assert_eq!(result.outcome, ScanOutcome::Cancelled);
    assert!(result.groups.is_empty());
}
