//! Recursive-copy integration tests confined to temporary roots.
#![expect(clippy::unwrap_used, reason = "temporary-root test setup")]

use std::{fs, time::Duration};

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    OperationConflictPolicyDto, OperationKindDto, OperationStateDto, RuntimeKindDto,
    StartOperationRequestDto, SymlinkPolicyDto,
};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

#[tokio::test]
async fn repeated_copy_reuses_active_job_and_move_of_same_source_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    let destination = root.path().join("destination");
    fs::write(&source, b"source").unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("source.txt"), b"existing").unwrap();
    let service = service(&root);
    let mut request = StartOperationRequestDto {
        operation_type: OperationKindDto::Copy,
        sources: vec![Location::from_native_path(&source).unwrap().into()],
        destination: Some(Location::from_native_path(&destination).unwrap().into()),
        destinations: vec![],
        conflict_policy: OperationConflictPolicyDto::Ask,
        name: None,
        archive_format: None,
        archive_compression_level: None,
        create_intermediate_directories: false,
        symlink_policy: SymlinkPolicyDto::CopyLink,
        permanent_delete_confirmed: false,
        override_read_only: false,
        link: None,
    };
    let first = service.start_operation(request.clone(), None).unwrap();
    let second = service.start_operation(request.clone(), None).unwrap();
    assert_eq!(first.id, second.id);
    request.operation_type = OperationKindDto::Move;
    assert!(service.start_operation(request, None).is_err());
    service.cancel_operation(first.id.into()).unwrap();
    assert_eq!(fs::read(&source).unwrap(), b"source");
}

async fn copy_directory(
    service: &FileManagerService,
    source: &std::path::Path,
    destination: &std::path::Path,
) -> fm_transport_dto::OperationDto {
    copy_directory_with_policy(service, source, destination, SymlinkPolicyDto::CopyLink).await
}

async fn copy_directory_with_policy(
    service: &FileManagerService,
    source: &std::path::Path,
    destination: &std::path::Path,
    symlink_policy: SymlinkPolicyDto,
) -> fm_transport_dto::OperationDto {
    let operation = service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::Copy,
                sources: vec![Location::from_native_path(source).unwrap().into()],
                destination: Some(Location::from_native_path(destination).unwrap().into()),
                destinations: vec![],
                conflict_policy: OperationConflictPolicyDto::Ask,
                name: None,
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: false,
                symlink_policy,
                permanent_delete_confirmed: false,
                override_read_only: false,
                link: None,
            },
            None,
        )
        .expect("accepted");
    // A fixed wall-clock budget here was flaky under CPU contention (a large copy genuinely
    // still advancing, just slower than usual, would get killed by an unrelated timer) - only
    // give up once the operation has made no progress at all for a while, so this stays fast in
    // the common case and robust under load.
    const STALL_TIMEOUT: Duration = Duration::from_secs(30);
    let mut last_progress = None;
    let mut last_progress_change = tokio::time::Instant::now();
    loop {
        let current = service.get_operation(operation.id.into()).unwrap();
        if current.state.is_terminal() {
            return current;
        }
        let progress = (
            current.progress.completed_items,
            current.progress.completed_bytes,
        );
        if last_progress != Some(progress) {
            last_progress = Some(progress);
            last_progress_change = tokio::time::Instant::now();
        }
        if last_progress_change.elapsed() > STALL_TIMEOUT {
            panic!("operation stalled without finishing: {current:?}");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn copies_symlink_cycles_as_links_without_following_them() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    symlink(".", source.join("loop")).unwrap();

    let result = copy_directory(&service(&root), &source, &destination).await;

    assert_eq!(result.state, OperationStateDto::Completed);
    assert_eq!(
        fs::read_link(destination.join("source/loop")).unwrap(),
        std::path::PathBuf::from(".")
    );
    assert_eq!(result.progress.total_items, None);
    assert_eq!(result.progress.completed_items, 2);
}

#[cfg(unix)]
#[tokio::test]
async fn explicit_copy_target_follows_a_link_but_cycle_protection_stops_reentry() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(source.join("target.txt"), b"target").unwrap();
    symlink("target.txt", source.join("followed.txt")).unwrap();
    symlink(".", source.join("loop")).unwrap();

    let result = copy_directory_with_policy(
        &service(&root),
        &source,
        &destination,
        SymlinkPolicyDto::CopyTarget,
    )
    .await;

    assert_eq!(result.state, OperationStateDto::Completed);
    assert_eq!(
        fs::read(destination.join("source/followed.txt")).unwrap(),
        b"target"
    );
    assert!(!destination.join("source/loop").exists());
}

#[tokio::test]
async fn iteratively_copies_a_deep_tree_without_stack_growth() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    let mut deepest = source.clone();
    for _ in 0..300 {
        deepest.push("d");
    }
    fs::create_dir_all(&deepest).unwrap();
    fs::write(deepest.join("leaf"), b"x").unwrap();
    fs::create_dir(&destination).unwrap();

    let result = copy_directory(&service(&root), &source, &destination).await;

    assert_eq!(result.state, OperationStateDto::Completed);
    assert_eq!(
        fs::read(
            destination
                .join("source")
                .join(deepest.strip_prefix(&source).unwrap())
                .join("leaf")
        )
        .unwrap(),
        b"x"
    );
}

#[tokio::test]
async fn plans_and_copies_ten_thousand_small_files() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    for index in 0..10_000 {
        fs::write(source.join(format!("f{index:05}")), b"x").unwrap();
    }

    let result = copy_directory(&service(&root), &source, &destination).await;

    assert_eq!(result.state, OperationStateDto::Completed);
    assert_eq!(result.progress.total_items, None);
    assert_eq!(result.progress.completed_items, 10_001);
    assert_eq!(result.progress.total_bytes, None);
    assert_eq!(result.progress.completed_bytes, 10_000);
    assert_eq!(
        fs::read_dir(destination.join("source")).unwrap().count(),
        10_000
    );
}

#[tokio::test]
async fn cancellation_during_large_tree_copy_stops_remaining_entries() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    for index in 0..10_000 {
        fs::write(source.join(format!("f{index:05}")), b"x").unwrap();
    }
    let service = service(&root);
    let operation = service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::Copy,
                sources: vec![Location::from_native_path(&source).unwrap().into()],
                destination: Some(Location::from_native_path(&destination).unwrap().into()),
                destinations: vec![],
                conflict_policy: OperationConflictPolicyDto::Ask,
                name: None,
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: false,
                symlink_policy: SymlinkPolicyDto::CopyLink,
                permanent_delete_confirmed: false,
                override_read_only: false,
                link: None,
            },
            None,
        )
        .unwrap();
    loop {
        let current = service.get_operation(operation.id.into()).unwrap();
        if matches!(
            current.state,
            OperationStateDto::Planning | OperationStateDto::Running
        ) {
            service.cancel_operation(operation.id.into()).unwrap();
            break;
        }
        assert!(
            !matches!(
                current.state,
                OperationStateDto::Cancelled
                    | OperationStateDto::Completed
                    | OperationStateDto::CompletedWithWarnings
                    | OperationStateDto::Failed
            ),
            "copy completed before cancellation could be requested"
        );
        tokio::task::yield_now().await;
    }
    loop {
        let current = service.get_operation(operation.id.into()).unwrap();
        if matches!(
            current.state,
            OperationStateDto::Cancelled
                | OperationStateDto::Completed
                | OperationStateDto::CompletedWithWarnings
                | OperationStateDto::Failed
        ) {
            assert_eq!(current.state, OperationStateDto::Cancelled);
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    assert!(
        !destination.join("source").exists()
            || fs::read_dir(destination.join("source")).unwrap().count() < 10_000
    );
}

#[tokio::test]
async fn copies_a_nested_unicode_tree_and_empty_directories() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir_all(source.join("子/empty")).unwrap();
    fs::write(source.join("子/hello.txt"), b"hello").unwrap();
    fs::create_dir(&destination).unwrap();

    let result = copy_directory(&service(&root), &source, &destination).await;

    assert_eq!(result.state, OperationStateDto::Completed);
    assert_eq!(
        fs::read(destination.join("source/子/hello.txt")).unwrap(),
        b"hello"
    );
    assert!(destination.join("source/子/empty").is_dir());
    assert_eq!(result.progress.total_bytes, None);
    assert_eq!(result.progress.completed_bytes, 5);
}

#[tokio::test]
async fn rejects_a_destination_inside_the_source_before_writing() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = source.join("inside");
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("data.txt"), b"safe").unwrap();

    let result = copy_directory(&service(&root), &source, &destination).await;

    assert_eq!(result.state, OperationStateDto::Failed);
    assert!(!destination.join("source").exists());
}

trait TerminalState {
    fn is_terminal(&self) -> bool;
}

impl TerminalState for OperationStateDto {
    fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::Completed | Self::CompletedWithWarnings | Self::Failed
        )
    }
}
