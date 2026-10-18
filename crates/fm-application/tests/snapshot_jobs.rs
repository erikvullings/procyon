//! Folder snapshot create/compare jobs over temporary trees (task 0268).

use std::time::{Duration, SystemTime};

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    LocationDto, SnapshotDiffKindDto, SnapshotJobPageDto, StartSnapshotJobRequestDto,
};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        fm_transport_dto::RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

fn location(path: &std::path::Path) -> LocationDto {
    LocationDto::from(Location::from_native_path(path).expect("location"))
}

async fn finished(service: &FileManagerService, job_id: uuid::Uuid) -> SnapshotJobPageDto {
    for _ in 0..500 {
        let page = service.get_snapshot_job_page(job_id, 0, 100).expect("page");
        if page.is_complete {
            return page;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("job did not finish");
}

fn set_modified(path: &std::path::Path, time: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open")
        .set_modified(time)
        .expect("set mtime");
}

async fn snapshot(
    service: &FileManagerService,
    tree: &std::path::Path,
    file: &std::path::Path,
    with_checksums: bool,
) -> SnapshotJobPageDto {
    let started = service.start_snapshot_job(StartSnapshotJobRequestDto::Create {
        root: location(tree),
        destination: location(file),
        with_checksums,
        show_hidden: false,
    });
    finished(service, started.job_id).await
}

async fn compare(
    service: &FileManagerService,
    snapshot: &std::path::Path,
    target: &std::path::Path,
) -> SnapshotJobPageDto {
    let started = service.start_snapshot_job(StartSnapshotJobRequestDto::Compare {
        snapshot: location(snapshot),
        target: location(target),
        verify_checksums: true,
        show_hidden: false,
    });
    finished(service, started.job_id).await
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().expect("temp dir");
    let tree = root.path().join("data");
    std::fs::create_dir_all(tree.join("docs")).expect("dirs");
    std::fs::write(tree.join("docs/keep.txt"), b"keep me").expect("keep");
    std::fs::write(tree.join("docs/change.txt"), b"before").expect("change");
    std::fs::write(tree.join("docs/rot.bin"), b"AAAAAAAA").expect("rot");
    std::fs::write(tree.join("gone.txt"), b"bye").expect("gone");
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    for name in [
        "docs/keep.txt",
        "docs/change.txt",
        "docs/rot.bin",
        "gone.txt",
    ] {
        set_modified(&tree.join(name), old);
    }
    (root, tree)
}

#[tokio::test]
async fn detects_added_removed_changed_and_silently_corrupted_files() {
    let (root, tree) = fixture();
    let service = service(&root);
    let file = root.path().join("data.procyon-snapshot");

    let created = snapshot(&service, &tree, &file, true).await;
    assert!(created.error.is_none(), "{:?}", created.error);
    assert!(created.has_checksums);
    assert_eq!(created.entries_found, 5);
    let text = std::fs::read_to_string(&file).expect("snapshot file");
    assert!(
        text.lines()
            .next()
            .expect("header")
            .contains("procyon-snapshot")
    );

    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    std::fs::write(tree.join("docs/change.txt"), b"after!!").expect("change");
    std::fs::write(tree.join("docs/rot.bin"), b"AAAABAAA").expect("rot");
    set_modified(&tree.join("docs/rot.bin"), old);
    std::fs::remove_file(tree.join("gone.txt")).expect("remove");
    std::fs::write(tree.join("new.txt"), b"fresh").expect("new");

    let result = compare(&service, &file, &tree).await;

    assert!(result.error.is_none(), "{:?}", result.error);
    let by_path = |kind: SnapshotDiffKindDto| -> Vec<String> {
        result
            .entries
            .iter()
            .filter(|entry| entry.kind == kind)
            .map(|entry| entry.path.clone())
            .collect()
    };
    assert_eq!(
        by_path(SnapshotDiffKindDto::Corrupted),
        vec!["docs/rot.bin"]
    );
    assert_eq!(
        by_path(SnapshotDiffKindDto::Changed),
        vec!["docs/change.txt"]
    );
    assert_eq!(by_path(SnapshotDiffKindDto::Removed), vec!["gone.txt"]);
    assert_eq!(by_path(SnapshotDiffKindDto::Added), vec!["new.txt"]);
    assert_eq!(result.counts.unchanged, 2); // docs/ and docs/keep.txt
    assert!(result.counts.corrupted == 1 && result.counts.added == 1);
}

#[tokio::test]
async fn a_backup_copy_can_be_verified_against_the_original_snapshot() {
    let (root, tree) = fixture();
    let service = service(&root);
    let file = root.path().join("data.procyon-snapshot");
    snapshot(&service, &tree, &file, true).await;

    let backup = root.path().join("backup");
    std::fs::create_dir_all(backup.join("docs")).expect("backup dirs");
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    for name in [
        "docs/keep.txt",
        "docs/change.txt",
        "docs/rot.bin",
        "gone.txt",
    ] {
        std::fs::copy(tree.join(name), backup.join(name)).expect("copy");
        set_modified(&backup.join(name), old);
    }

    let clean = compare(&service, &file, &backup).await;
    assert!(clean.entries.is_empty(), "{:?}", clean.entries);

    std::fs::write(backup.join("docs/keep.txt"), b"keep ME").expect("corrupt backup");
    set_modified(&backup.join("docs/keep.txt"), old);
    let dirty = compare(&service, &file, &backup).await;
    assert_eq!(dirty.entries.len(), 1);
    assert_eq!(dirty.entries[0].kind, SnapshotDiffKindDto::Corrupted);
}

#[tokio::test]
async fn never_overwrites_an_existing_snapshot_and_rejects_foreign_files() {
    let (root, tree) = fixture();
    let service = service(&root);
    let file = root.path().join("data.procyon-snapshot");
    assert!(
        snapshot(&service, &tree, &file, false)
            .await
            .error
            .is_none()
    );

    let again = snapshot(&service, &tree, &file, false).await;
    assert!(again.error.is_some());

    let foreign = root.path().join("notes.txt");
    std::fs::write(&foreign, b"just text").expect("foreign");
    let result = compare(&service, &foreign, &tree).await;
    assert!(
        result
            .error
            .expect("error")
            .contains("not a Procyon snapshot")
    );
}

#[test]
fn unknown_jobs_are_not_found() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);
    assert!(service.cancel_snapshot_job(uuid::Uuid::new_v4()).is_err());
    assert!(
        service
            .get_snapshot_job_page(uuid::Uuid::new_v4(), 0, 10)
            .is_err()
    );
}
