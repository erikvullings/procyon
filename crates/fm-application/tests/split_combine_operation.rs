//! `splitFile` / `combineFiles` (task 0257): streaming parts, manifest verification, conflicts,
//! gaps, cancellation and undo, in temp roots only.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    InspectSplitSetRequestDto, LocationDto, OperationConflictPolicyDto, OperationDto,
    OperationKindDto, OperationStateDto, RuntimeKindDto, SplitFileDto, StartOperationRequestDto,
};
use uuid::Uuid;

struct Fixture {
    _root: tempfile::TempDir,
    data: PathBuf,
    service: FileManagerService,
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().expect("temporary directory");
    let data = root.path().join("data");
    std::fs::create_dir(&data).expect("data dir");
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    );
    Fixture {
        _root: root,
        data,
        service,
    }
}

fn location(path: &Path) -> LocationDto {
    LocationDto::from(Location::from_native_path(path).expect("location"))
}

/// Deterministic, non-repeating-looking content so a misordered join is detected.
fn content(len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| ((index * 31 + index / 251) % 256) as u8)
        .collect()
}

async fn wait(service: &FileManagerService, id: Uuid) -> OperationDto {
    loop {
        let current = service.get_operation(id.into()).expect("operation");
        if matches!(
            current.state,
            OperationStateDto::Completed
                | OperationStateDto::CompletedWithWarnings
                | OperationStateDto::Failed
                | OperationStateDto::Cancelled
                | OperationStateDto::WaitingForConflictResolution
        ) {
            return current;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn request(
    kind: OperationKindDto,
    sources: &[&Path],
    destination: Option<&Path>,
    policy: OperationConflictPolicyDto,
) -> StartOperationRequestDto {
    StartOperationRequestDto {
        operation_type: kind,
        sources: sources.iter().map(|path| location(path)).collect(),
        destination: destination.map(location),
        destinations: vec![],
        conflict_policy: policy,
        name: None,
        archive_format: None,
        archive_compression_level: None,
        create_intermediate_directories: false,
        initial_content: None,
        skip_space_check: false,
        image_metadata_edit: None,
        attribute_change: None,
        split_file: None,
        geo_transform: None,
        symlink_policy: Default::default(),
        permanent_delete_confirmed: false,
        override_read_only: false,
        link: None,
    }
}

async fn split(
    f: &Fixture,
    file: &Path,
    part: u64,
    manifest: bool,
    destination: Option<&Path>,
    policy: OperationConflictPolicyDto,
) -> OperationDto {
    let mut start = request(OperationKindDto::SplitFile, &[file], destination, policy);
    start.split_file = Some(SplitFileDto {
        part_size_bytes: part,
        write_manifest: manifest,
    });
    let operation = f.service.start_operation(start, None).expect("accepted");
    wait(&f.service, operation.id).await
}

async fn combine(
    f: &Fixture,
    parts: &[&Path],
    destination: Option<&Path>,
    policy: OperationConflictPolicyDto,
) -> OperationDto {
    let start = request(OperationKindDto::CombineFiles, parts, destination, policy);
    let operation = f.service.start_operation(start, None).expect("accepted");
    wait(&f.service, operation.id).await
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn splits_into_numbered_parts_and_joins_them_back_byte_for_byte() {
    let f = fixture();
    let original = content(2_500_000);
    let source = f.data.join("movie.bin");
    std::fs::write(&source, &original).expect("fixture");

    let done = split(
        &f,
        &source,
        1_000_000,
        true,
        None,
        OperationConflictPolicyDto::Ask,
    )
    .await;
    assert_eq!(
        done.state,
        OperationStateDto::Completed,
        "{:?}",
        done.errors
    );
    assert_eq!(
        names(&f.data),
        [
            "movie.bin",
            "movie.bin.001",
            "movie.bin.002",
            "movie.bin.003",
            "movie.bin.blake3"
        ]
    );
    assert_eq!(
        std::fs::metadata(f.data.join("movie.bin.003"))
            .expect("part")
            .len(),
        500_000
    );
    let manifest = std::fs::read_to_string(f.data.join("movie.bin.blake3")).expect("manifest");
    assert!(manifest.contains("# parts: 3"), "{manifest}");
    assert_eq!(
        std::fs::read(&source).expect("source untouched"),
        original,
        "the source is never touched"
    );

    // Join into another folder from just the middle part.
    let target = f.data.join("out");
    std::fs::create_dir(&target).expect("target");
    let joined = combine(
        &f,
        &[&f.data.join("movie.bin.002")],
        Some(&target),
        OperationConflictPolicyDto::Ask,
    )
    .await;
    assert_eq!(
        joined.state,
        OperationStateDto::Completed,
        "{:?}",
        joined.errors
    );
    assert_eq!(
        std::fs::read(target.join("movie.bin")).expect("joined"),
        original
    );
}

#[tokio::test]
async fn a_file_that_fits_one_part_and_an_empty_file_still_split() {
    let f = fixture();
    let small = f.data.join("small.txt");
    std::fs::write(&small, b"abc").expect("fixture");
    let empty = f.data.join("empty.txt");
    std::fs::write(&empty, b"").expect("fixture");
    for file in [&small, &empty] {
        let done = split(&f, file, 1024, false, None, OperationConflictPolicyDto::Ask).await;
        assert_eq!(
            done.state,
            OperationStateDto::Completed,
            "{:?}",
            done.errors
        );
    }
    assert_eq!(
        std::fs::read(f.data.join("small.txt.001")).expect("part"),
        b"abc"
    );
    assert_eq!(
        std::fs::read(f.data.join("empty.txt.001")).expect("part"),
        b""
    );
}

#[tokio::test]
async fn split_undo_removes_exactly_the_parts_and_manifest() {
    let f = fixture();
    let source = f.data.join("a.bin");
    std::fs::write(&source, content(2_500)).expect("fixture");
    let done = split(
        &f,
        &source,
        1_000,
        true,
        None,
        OperationConflictPolicyDto::Ask,
    )
    .await;
    assert!(done.undo.available, "{:?}", done.undo.reason);
    let undo = f.service.undo_operation(done.id.into()).expect("undo");
    assert_eq!(
        wait(&f.service, undo.id).await.state,
        OperationStateDto::Completed
    );
    assert_eq!(names(&f.data), ["a.bin"]);
}

#[tokio::test]
async fn existing_parts_ask_then_skip_rename_or_overwrite_and_never_replace_silently() {
    let f = fixture();
    let source = f.data.join("a.bin");
    let original = content(2_500);
    std::fs::write(&source, &original).expect("fixture");
    std::fs::write(f.data.join("a.bin.001"), b"precious").expect("existing part");

    let asked = split(
        &f,
        &source,
        1_000,
        false,
        None,
        OperationConflictPolicyDto::Ask,
    )
    .await;
    assert_eq!(asked.state, OperationStateDto::WaitingForConflictResolution);
    assert_eq!(
        std::fs::read(f.data.join("a.bin.001")).expect("kept"),
        b"precious"
    );
    f.service.cancel_operation(asked.id.into()).expect("cancel");

    let skipped = split(
        &f,
        &source,
        1_000,
        false,
        None,
        OperationConflictPolicyDto::Skip,
    )
    .await;
    assert_ne!(skipped.state, OperationStateDto::Failed);
    assert_eq!(
        std::fs::read(f.data.join("a.bin.001")).expect("kept"),
        b"precious"
    );
    assert!(!f.data.join("a.bin.002").exists());

    let renamed = split(
        &f,
        &source,
        1_000,
        false,
        None,
        OperationConflictPolicyDto::RenameNew,
    )
    .await;
    assert_eq!(
        renamed.state,
        OperationStateDto::Completed,
        "{:?}",
        renamed.errors
    );
    assert!(f.data.join("a (copy 1).bin.003").exists());
    assert_eq!(
        std::fs::read(f.data.join("a.bin.001")).expect("kept"),
        b"precious"
    );

    let replaced = split(
        &f,
        &source,
        1_000,
        false,
        None,
        OperationConflictPolicyDto::Overwrite,
    )
    .await;
    assert_eq!(
        replaced.state,
        OperationStateDto::Completed,
        "{:?}",
        replaced.errors
    );
    assert_eq!(
        std::fs::read(f.data.join("a.bin.001")).expect("replaced"),
        original[..1_000]
    );
    assert!(
        !replaced.undo.available,
        "replaced files cannot be restored"
    );
}

#[tokio::test]
async fn overwriting_a_larger_split_removes_its_stale_trailing_parts() {
    let f = fixture();
    let source = f.data.join("a.bin");
    std::fs::write(&source, content(3_000)).expect("fixture");
    let first = split(
        &f,
        &source,
        1_000,
        false,
        None,
        OperationConflictPolicyDto::Ask,
    )
    .await;
    assert_eq!(first.state, OperationStateDto::Completed);
    let again = split(
        &f,
        &source,
        2_000,
        false,
        None,
        OperationConflictPolicyDto::Overwrite,
    )
    .await;
    assert_eq!(
        again.state,
        OperationStateDto::Completed,
        "{:?}",
        again.errors
    );
    assert_eq!(names(&f.data), ["a.bin", "a.bin.001", "a.bin.002"]);
}

#[tokio::test]
async fn combine_rejects_gaps_and_writes_nothing() {
    let f = fixture();
    let source = f.data.join("a.bin");
    std::fs::write(&source, content(3_000)).expect("fixture");
    split(
        &f,
        &source,
        1_000,
        false,
        None,
        OperationConflictPolicyDto::Ask,
    )
    .await;
    std::fs::remove_file(f.data.join("a.bin.002")).expect("gap");
    std::fs::remove_file(&source).expect("source gone");

    let inspected = f
        .service
        .inspect_split_set(InspectSplitSetRequestDto {
            locations: vec![location(&f.data.join("a.bin.001"))],
        })
        .await
        .expect("inspect");
    assert!(
        inspected
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("a.bin.002"),
        "{inspected:?}"
    );

    let done = combine(
        &f,
        &[&f.data.join("a.bin.001")],
        None,
        OperationConflictPolicyDto::Ask,
    )
    .await;
    assert_eq!(done.state, OperationStateDto::Failed);
    assert!(!f.data.join("a.bin").exists());
    assert_eq!(names(&f.data), ["a.bin.001", "a.bin.003"]);
}

#[tokio::test]
async fn a_manifest_catches_a_damaged_part_and_a_missing_last_part() {
    let f = fixture();
    let source = f.data.join("a.bin");
    let original = content(3_000);
    std::fs::write(&source, &original).expect("fixture");
    split(
        &f,
        &source,
        1_000,
        true,
        None,
        OperationConflictPolicyDto::Ask,
    )
    .await;
    std::fs::remove_file(&source).expect("source gone");

    // Flip one byte in the second part: same size, wrong content.
    let second = f.data.join("a.bin.002");
    let mut bytes = std::fs::read(&second).expect("part");
    bytes[10] ^= 0xFF;
    std::fs::write(&second, &bytes).expect("damage");
    let damaged = combine(&f, &[&second], None, OperationConflictPolicyDto::Ask).await;
    assert_eq!(damaged.state, OperationStateDto::CompletedWithWarnings);
    assert!(
        damaged
            .errors
            .iter()
            .any(|e| e.message.contains("manifest")),
        "{damaged:?}"
    );
    assert!(!f.data.join("a.bin").exists(), "nothing is published");
    assert!(
        names(&f.data).iter().all(|name| !name.contains(".tmp")),
        "{:?}",
        names(&f.data)
    );

    // Restore, then drop the last part: only the manifest knows it is missing.
    bytes[10] ^= 0xFF;
    std::fs::write(&second, &bytes).expect("restore");
    std::fs::remove_file(f.data.join("a.bin.003")).expect("truncate set");
    let inspected = f
        .service
        .inspect_split_set(InspectSplitSetRequestDto {
            locations: vec![location(&second)],
        })
        .await
        .expect("inspect");
    assert!(inspected.error.is_some(), "{inspected:?}");
}

#[tokio::test]
async fn inspect_reports_the_set_with_interleaved_sets_in_one_folder() {
    let f = fixture();
    let a = f.data.join("a.bin");
    let b = f.data.join("b.bin");
    std::fs::write(&a, content(2_500)).expect("fixture");
    std::fs::write(&b, content(1_500)).expect("fixture");
    split(&f, &a, 1_000, true, None, OperationConflictPolicyDto::Ask).await;
    split(&f, &b, 1_000, false, None, OperationConflictPolicyDto::Ask).await;

    let set = f
        .service
        .inspect_split_set(InspectSplitSetRequestDto {
            locations: vec![
                location(&f.data.join("a.bin.003")),
                location(&f.data.join("a.bin.001")),
            ],
        })
        .await
        .expect("inspect");
    assert_eq!(set.error, None);
    assert_eq!(set.output_name, "a.bin");
    assert_eq!(set.parts.len(), 3);
    assert_eq!(set.total_size, 2_500);
    assert!(set.has_manifest);

    let other = f
        .service
        .inspect_split_set(InspectSplitSetRequestDto {
            locations: vec![location(&f.data.join("b.bin.002"))],
        })
        .await
        .expect("inspect");
    assert_eq!(other.parts.len(), 2);
    assert!(!other.has_manifest);

    let mixed = f
        .service
        .inspect_split_set(InspectSplitSetRequestDto {
            locations: vec![
                location(&f.data.join("a.bin.001")),
                location(&f.data.join("b.bin.001")),
            ],
        })
        .await
        .expect("inspect");
    assert!(mixed.error.is_some());
}

#[tokio::test]
async fn combine_conflicts_never_overwrite_silently_and_undo_removes_the_output() {
    let f = fixture();
    let source = f.data.join("a.bin");
    let original = content(2_000);
    std::fs::write(&source, &original).expect("fixture");
    split(
        &f,
        &source,
        1_000,
        false,
        None,
        OperationConflictPolicyDto::Ask,
    )
    .await;
    let part = f.data.join("a.bin.001");

    let asked = combine(&f, &[&part], None, OperationConflictPolicyDto::Ask).await;
    assert_eq!(asked.state, OperationStateDto::WaitingForConflictResolution);
    f.service.cancel_operation(asked.id.into()).expect("cancel");
    assert_eq!(std::fs::read(&source).expect("kept"), original);

    std::fs::remove_file(&source).expect("remove");
    let done = combine(&f, &[&part], None, OperationConflictPolicyDto::Ask).await;
    assert_eq!(
        done.state,
        OperationStateDto::Completed,
        "{:?}",
        done.errors
    );
    assert_eq!(std::fs::read(&source).expect("joined"), original);
    assert!(done.undo.available, "{:?}", done.undo.reason);
    let undo = f.service.undo_operation(done.id.into()).expect("undo");
    assert_eq!(
        wait(&f.service, undo.id).await.state,
        OperationStateDto::Completed
    );
    assert!(!source.exists());
    assert!(f.data.join("a.bin.002").exists(), "the parts stay");
}

#[tokio::test]
async fn invalid_requests_are_rejected() {
    let f = fixture();
    let source = f.data.join("a.bin");
    std::fs::write(&source, b"x").expect("fixture");
    let mut zero = request(
        OperationKindDto::SplitFile,
        &[&source],
        None,
        OperationConflictPolicyDto::Ask,
    );
    assert!(
        f.service.start_operation(zero.clone(), None).is_err(),
        "no options"
    );
    zero.split_file = Some(SplitFileDto {
        part_size_bytes: 0,
        write_manifest: false,
    });
    assert!(
        f.service.start_operation(zero, None).is_err(),
        "zero part size"
    );

    // A split that would exceed the part limit fails in planning and writes nothing.
    let done = split(&f, &source, 1, false, None, OperationConflictPolicyDto::Ask).await;
    assert_eq!(
        done.state,
        OperationStateDto::Completed,
        "1 byte -> 1 part is fine"
    );
    let big = f.data.join("big.bin");
    std::fs::write(&big, content(10_100)).expect("fixture");
    let too_many = split(&f, &big, 1, false, None, OperationConflictPolicyDto::Ask).await;
    assert_eq!(too_many.state, OperationStateDto::Failed);
    assert_eq!(
        names(&f.data)
            .iter()
            .filter(|n| n.starts_with("big.bin."))
            .count(),
        0
    );
}

#[tokio::test]
async fn cancelling_a_split_removes_the_partial_output() {
    let f = fixture();
    let source = f.data.join("huge.bin");
    std::fs::write(&source, vec![7_u8; 96 * 1024 * 1024]).expect("fixture");
    let mut start = request(
        OperationKindDto::SplitFile,
        &[&source],
        None,
        OperationConflictPolicyDto::Ask,
    );
    start.split_file = Some(SplitFileDto {
        part_size_bytes: 1024 * 1024,
        write_manifest: true,
    });
    let operation = f.service.start_operation(start, None).expect("accepted");
    for _ in 0..2_000 {
        if f.data.join("huge.bin.002").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    f.service
        .cancel_operation(operation.id.into())
        .expect("cancel");
    let finished = wait(&f.service, operation.id).await;
    // The job may have finished before the cancel landed on a fast disk; only assert the cleanup
    // when it was really cancelled.
    if finished.state == OperationStateDto::Cancelled {
        assert_eq!(names(&f.data), ["huge.bin"]);
    }
}
