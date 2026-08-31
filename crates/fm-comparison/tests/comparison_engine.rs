//! Integration tests for [`fm_comparison::ComparisonEngine`] against a real
//! local-filesystem fixture pair (task 0075 acceptance criteria: comparison
//! correctness, cancellation).

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use fm_comparison::{
    ComparisonCriteria, ComparisonEngine, ComparisonEntry, ComparisonOptions,
    ComparisonResultsStore, ComparisonStatus, SyncAction, SyncMode, generate_sync_plan,
};
use fm_domain::Location;
use fm_events::{EventAudience, EventBus};
use fm_vfs::ProviderRegistry;
use fm_vfs_local::LocalFileSystemProvider;
use tempfile::tempdir;
use uuid::Uuid;

fn set_mtime(path: &Path, seconds_from_epoch: u64) {
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(seconds_from_epoch);
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("file must open for timestamp adjustment");
    file.set_modified(time).expect("setting mtime must succeed");
}

fn engine() -> (ComparisonEngine, Arc<ComparisonResultsStore>, EventBus) {
    let store = Arc::new(ComparisonResultsStore::new());
    let events = EventBus::new(256);
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(LocalFileSystemProvider::new()));
    let engine = ComparisonEngine::new(Arc::clone(&store), events.clone(), providers);
    (engine, store, events)
}

fn base_options(criteria: ComparisonCriteria) -> ComparisonOptions {
    ComparisonOptions {
        criteria,
        show_hidden: true,
        operation_id: None,
    }
}

/// Waits until the store reports the comparison complete, or panics after a
/// generous timeout. Every fixture below is small and entirely local, so
/// this should never actually wait long.
async fn wait_for_completion(store: &ComparisonResultsStore, id: Uuid) -> Vec<ComparisonEntry> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(page) = store.page(id, 0, 10_000, false)
                && page.is_complete
            {
                return store
                    .all_entries(id)
                    .expect("comparison must still be tracked");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("comparison must complete promptly")
}

fn status_by_path(entries: &[ComparisonEntry]) -> HashMap<String, ComparisonStatus> {
    entries
        .iter()
        .map(|entry| (entry.relative_path.clone(), entry.status))
        .collect()
}

#[tokio::test]
async fn compares_a_nested_tree_covering_every_status_under_size_and_timestamp() {
    let left = tempdir().unwrap();
    let right = tempdir().unwrap();

    // Present on both sides, identical.
    fs::write(left.path().join("same.txt"), b"same content").unwrap();
    fs::write(right.path().join("same.txt"), b"same content").unwrap();
    set_mtime(&left.path().join("same.txt"), 1_700_000_000);
    set_mtime(&right.path().join("same.txt"), 1_700_000_000);

    // Only on the left.
    fs::write(left.path().join("only-left.txt"), b"left only").unwrap();

    // Only on the right.
    fs::write(right.path().join("only-right.txt"), b"right only").unwrap();

    // Left is newer.
    fs::write(left.path().join("newer.txt"), b"left version").unwrap();
    fs::write(right.path().join("newer.txt"), b"right version").unwrap();
    set_mtime(&left.path().join("newer.txt"), 1_700_000_200);
    set_mtime(&right.path().join("newer.txt"), 1_700_000_000);

    // Left is older.
    fs::write(left.path().join("older.txt"), b"left version").unwrap();
    fs::write(right.path().join("older.txt"), b"right version").unwrap();
    set_mtime(&left.path().join("older.txt"), 1_700_000_000);
    set_mtime(&right.path().join("older.txt"), 1_700_000_200);

    // Same timestamp, different size.
    fs::write(left.path().join("size.txt"), b"short").unwrap();
    fs::write(right.path().join("size.txt"), b"a much longer body").unwrap();
    set_mtime(&left.path().join("size.txt"), 1_700_000_000);
    set_mtime(&right.path().join("size.txt"), 1_700_000_000);

    // A file on the left across from a directory of the same name on the right.
    fs::write(left.path().join("mismatch"), b"i am a file").unwrap();
    fs::create_dir(right.path().join("mismatch")).unwrap();

    // A nested subdirectory, matched on both sides, containing its own diff.
    fs::create_dir(left.path().join("sub")).unwrap();
    fs::create_dir(right.path().join("sub")).unwrap();
    fs::write(
        left.path().join("sub/nested.txt"),
        b"nested only on the left",
    )
    .unwrap();

    let (engine, store, _events) = engine();
    let comparison_id = Uuid::new_v4();
    engine
        .start(
            comparison_id,
            Location::from_native_path(left.path()).unwrap(),
            Location::from_native_path(right.path()).unwrap(),
            base_options(ComparisonCriteria::SizeAndTimestamp),
            EventAudience::Global,
        )
        .unwrap();

    let entries = wait_for_completion(&store, comparison_id).await;
    let status = status_by_path(&entries);

    assert_eq!(status["same.txt"], ComparisonStatus::Identical);
    assert_eq!(status["only-left.txt"], ComparisonStatus::OnlyLeft);
    assert_eq!(status["only-right.txt"], ComparisonStatus::OnlyRight);
    assert_eq!(status["newer.txt"], ComparisonStatus::Newer);
    assert_eq!(status["older.txt"], ComparisonStatus::Older);
    assert_eq!(status["size.txt"], ComparisonStatus::DifferentSize);
    assert_eq!(status["mismatch"], ComparisonStatus::TypeMismatch);
    // The matched `sub` directory pair is itself reported identical...
    assert_eq!(status["sub"], ComparisonStatus::Identical);
    // ...while its only-left child is still discovered by recursing into it.
    assert_eq!(status["sub/nested.txt"], ComparisonStatus::OnlyLeft);
    assert_eq!(entries.len(), 9);
}

#[tokio::test]
async fn name_only_criteria_reports_every_matched_pair_as_identical() {
    let left = tempdir().unwrap();
    let right = tempdir().unwrap();
    fs::write(left.path().join("a.txt"), b"left content").unwrap();
    fs::write(
        right.path().join("a.txt"),
        b"a completely different, longer body",
    )
    .unwrap();
    set_mtime(&left.path().join("a.txt"), 1_700_000_000);
    set_mtime(&right.path().join("a.txt"), 1_800_000_000);

    let (engine, store, _events) = engine();
    let comparison_id = Uuid::new_v4();
    engine
        .start(
            comparison_id,
            Location::from_native_path(left.path()).unwrap(),
            Location::from_native_path(right.path()).unwrap(),
            base_options(ComparisonCriteria::NameOnly),
            EventAudience::Global,
        )
        .unwrap();

    let entries = wait_for_completion(&store, comparison_id).await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].status, ComparisonStatus::Identical);
}

#[tokio::test]
async fn content_hash_criteria_distinguishes_identical_content_from_a_differing_timestamp() {
    let left = tempdir().unwrap();
    let right = tempdir().unwrap();

    // Same bytes, different mtimes: `SizeAndTimestamp` would call this
    // `Newer`, but content-hash mode must see through to equal content.
    fs::write(left.path().join("same-bytes.txt"), b"identical payload").unwrap();
    fs::write(right.path().join("same-bytes.txt"), b"identical payload").unwrap();
    set_mtime(&left.path().join("same-bytes.txt"), 1_700_000_500);
    set_mtime(&right.path().join("same-bytes.txt"), 1_700_000_000);

    fs::write(left.path().join("different.txt"), b"left payload").unwrap();
    fs::write(right.path().join("different.txt"), b"right payload!!").unwrap();

    let (engine, store, _events) = engine();
    let comparison_id = Uuid::new_v4();
    engine
        .start(
            comparison_id,
            Location::from_native_path(left.path()).unwrap(),
            Location::from_native_path(right.path()).unwrap(),
            base_options(ComparisonCriteria::ContentHash),
            EventAudience::Global,
        )
        .unwrap();

    let entries = wait_for_completion(&store, comparison_id).await;
    let status = status_by_path(&entries);
    assert_eq!(status["same-bytes.txt"], ComparisonStatus::Identical);
    assert_ne!(status["different.txt"], ComparisonStatus::Identical);

    let same_bytes = entries
        .iter()
        .find(|entry| entry.relative_path == "same-bytes.txt")
        .unwrap();
    let left_hash = same_bytes.left.as_ref().unwrap().content_hash.as_ref();
    let right_hash = same_bytes.right.as_ref().unwrap().content_hash.as_ref();
    assert!(left_hash.is_some(), "content hash must be computed");
    assert_eq!(left_hash, right_hash);
}

#[tokio::test]
async fn show_hidden_false_excludes_dotfiles_on_either_side() {
    let left = tempdir().unwrap();
    let right = tempdir().unwrap();
    fs::write(left.path().join(".hidden"), b"secret").unwrap();
    fs::write(right.path().join(".hidden"), b"secret").unwrap();
    fs::write(left.path().join("visible.txt"), b"v").unwrap();
    fs::write(right.path().join("visible.txt"), b"v").unwrap();

    let (engine, store, _events) = engine();
    let comparison_id = Uuid::new_v4();
    engine
        .start(
            comparison_id,
            Location::from_native_path(left.path()).unwrap(),
            Location::from_native_path(right.path()).unwrap(),
            ComparisonOptions {
                criteria: ComparisonCriteria::NameOnly,
                show_hidden: false,
                operation_id: None,
            },
            EventAudience::Global,
        )
        .unwrap();

    let entries = wait_for_completion(&store, comparison_id).await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].relative_path, "visible.txt");
}

#[tokio::test]
async fn cancellation_stops_traversal_before_every_directory_is_visited() {
    let left = tempdir().unwrap();
    let right = tempdir().unwrap();
    for index in 0..50 {
        let name = format!("dir-{index}");
        fs::create_dir(left.path().join(&name)).unwrap();
        fs::create_dir(right.path().join(&name)).unwrap();
        for file in 0..50 {
            fs::write(
                left.path().join(&name).join(format!("file-{file}.txt")),
                b"payload",
            )
            .unwrap();
            fs::write(
                right.path().join(&name).join(format!("file-{file}.txt")),
                b"payload",
            )
            .unwrap();
        }
    }

    let (engine, store, _events) = engine();
    let comparison_id = Uuid::new_v4();
    engine
        .start(
            comparison_id,
            Location::from_native_path(left.path()).unwrap(),
            Location::from_native_path(right.path()).unwrap(),
            base_options(ComparisonCriteria::NameOnly),
            EventAudience::Global,
        )
        .unwrap();

    // Cancel essentially immediately, racing the traversal.
    engine.cancel(comparison_id).unwrap();

    let entries = wait_for_completion(&store, comparison_id).await;
    assert!(
        entries.len() < 2_550,
        "cancellation must stop traversal before every entry is compared, found {}",
        entries.len()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_loop_on_one_side_does_not_hang_traversal() {
    use std::os::unix::fs::symlink;

    let left = tempdir().unwrap();
    let right = tempdir().unwrap();
    fs::create_dir(left.path().join("sub")).unwrap();
    fs::write(left.path().join("sub/real.txt"), b"payload").unwrap();
    symlink(left.path(), left.path().join("sub/cycle")).unwrap();
    fs::create_dir(right.path().join("sub")).unwrap();

    let (engine, store, _events) = engine();
    let comparison_id = Uuid::new_v4();
    engine
        .start(
            comparison_id,
            Location::from_native_path(left.path()).unwrap(),
            Location::from_native_path(right.path()).unwrap(),
            base_options(ComparisonCriteria::NameOnly),
            EventAudience::Global,
        )
        .unwrap();

    let entries = wait_for_completion(&store, comparison_id).await;
    let status = status_by_path(&entries);
    assert_eq!(status["sub/real.txt"], ComparisonStatus::OnlyLeft);
    // The symlink itself is a leaf entry (never followed), so it is
    // discovered but not recursed into.
    assert_eq!(status["sub/cycle"], ComparisonStatus::OnlyLeft);
}

#[tokio::test]
async fn an_unresolvable_root_is_rejected_synchronously() {
    let (engine, _store, _events) = engine();
    let error = engine
        .start(
            Uuid::new_v4(),
            Location::new(fm_domain::ProviderId::new("unknown"), "unknown://left"),
            Location::new(fm_domain::ProviderId::new("unknown"), "unknown://right"),
            base_options(ComparisonCriteria::NameOnly),
            EventAudience::Global,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        fm_comparison::ComparisonError::InvalidRoot(_)
    ));
}

#[tokio::test]
async fn cancelling_an_unknown_comparison_reports_not_found() {
    let (engine, _store, _events) = engine();
    let unknown = Uuid::new_v4();
    let error = engine.cancel(unknown).unwrap_err();
    assert!(matches!(error, fm_comparison::ComparisonError::NotFound(id) if id == unknown));
}

/// Snapshot of a directory tree's file names and contents, used to assert a
/// fixture pair is byte-for-byte unchanged.
fn snapshot_tree(root: &Path) -> HashMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut HashMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).expect("fixture directory must be readable") {
            let entry = entry.expect("fixture directory entry must be readable");
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("walked path must be under root")
                    .to_string_lossy()
                    .into_owned();
                out.insert(
                    relative,
                    fs::read(&path).expect("fixture file must be readable"),
                );
            }
        }
    }
    let mut out = HashMap::new();
    walk(root, root, &mut out);
    out
}

#[tokio::test]
async fn generating_a_sync_plan_from_a_real_comparison_never_changes_either_fixture() {
    let left = tempdir().unwrap();
    let right = tempdir().unwrap();
    fs::write(left.path().join("only-left.txt"), b"left only").unwrap();
    fs::write(right.path().join("only-right.txt"), b"right only").unwrap();
    fs::write(left.path().join("newer.txt"), b"left version").unwrap();
    fs::write(right.path().join("newer.txt"), b"right version").unwrap();
    set_mtime(&left.path().join("newer.txt"), 1_700_000_200);
    set_mtime(&right.path().join("newer.txt"), 1_700_000_000);

    let before_left = snapshot_tree(left.path());
    let before_right = snapshot_tree(right.path());

    let (engine, store, _events) = engine();
    let comparison_id = Uuid::new_v4();
    engine
        .start(
            comparison_id,
            Location::from_native_path(left.path()).unwrap(),
            Location::from_native_path(right.path()).unwrap(),
            base_options(ComparisonCriteria::SizeAndTimestamp),
            EventAudience::Global,
        )
        .unwrap();
    let entries = wait_for_completion(&store, comparison_id).await;

    let plan = generate_sync_plan(&entries, SyncMode::MirrorLeftToRight);
    assert_eq!(plan.len(), 3, "every non-identical entry gets a plan row");
    let action_for = |path: &str| {
        plan.iter()
            .find(|item| item.relative_path == path)
            .unwrap()
            .action
    };
    assert_eq!(action_for("only-left.txt"), SyncAction::CopyLeftToRight);
    assert_eq!(action_for("only-right.txt"), SyncAction::DeleteRight);
    assert_eq!(action_for("newer.txt"), SyncAction::CopyLeftToRight);

    // Neither comparing nor planning is allowed to mutate anything (spec
    // §35): the fixtures must be exactly as they were before either call.
    assert_eq!(snapshot_tree(left.path()), before_left);
    assert_eq!(snapshot_tree(right.path()), before_right);
}
