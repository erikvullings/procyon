//! Move integration tests confined to temporary roots.
#![expect(clippy::unwrap_used, reason = "temporary-root test setup")]

use std::{fs, time::Duration};

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    OperationConflictPolicyDto, OperationKindDto, OperationStateDto, RuntimeKindDto,
    StartOperationRequestDto,
};

async fn wait(service: &FileManagerService, id: uuid::Uuid) -> fm_transport_dto::OperationDto {
    for _ in 0..2000 {
        let operation = service.get_operation(id.into()).unwrap();
        if matches!(
            operation.state,
            OperationStateDto::Cancelled | OperationStateDto::Completed | OperationStateDto::Failed
        ) {
            return operation;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("move did not finish")
}

#[tokio::test]
async fn failed_cross_volume_fallback_never_deletes_the_source() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    let destination = root.path().join("destination");
    fs::write(&source, b"source").unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("source.txt"), b"existing").unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    );
    service.force_cross_volume_moves_for_tests(true);

    let started = start_move(&service, &source, &destination);
    for _ in 0..2000 {
        if service.get_operation(started.id.into()).unwrap().state
            == OperationStateDto::WaitingForConflictResolution
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        service.get_operation(started.id.into()).unwrap().state,
        OperationStateDto::WaitingForConflictResolution
    );
    service.cancel_operation(started.id.into()).unwrap();
    let result = wait(&service, started.id).await;

    assert_eq!(result.state, OperationStateDto::Cancelled);
    assert_eq!(fs::read(&source).unwrap(), b"source");
    assert_eq!(
        fs::read(destination.join("source.txt")).unwrap(),
        b"existing"
    );
}

#[tokio::test]
async fn cancelling_cross_volume_fallback_leaves_the_source_tree() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    for index in 0..2_000 {
        fs::write(source.join(format!("f{index:04}")), b"data").unwrap();
    }
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    );
    service.force_cross_volume_moves_for_tests(true);

    let started = start_move(&service, &source, &destination);
    loop {
        let operation = service.get_operation(started.id.into()).unwrap();
        if operation.state == OperationStateDto::Running {
            service.cancel_operation(started.id.into()).unwrap();
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let result = wait(&service, started.id).await;

    assert_eq!(result.state, OperationStateDto::Cancelled);
    assert!(source.is_dir());
    assert_eq!(fs::read_dir(&source).unwrap().count(), 2_000);
}

fn start_move(
    service: &FileManagerService,
    source: &std::path::Path,
    destination: &std::path::Path,
) -> fm_transport_dto::OperationDto {
    service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::Move,
                sources: vec![Location::from_native_path(source).unwrap().into()],
                destination: Some(Location::from_native_path(destination).unwrap().into()),
                destinations: vec![],
                conflict_policy: OperationConflictPolicyDto::Ask,
                name: None,
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: false,
                symlink_policy: Default::default(),
                permanent_delete_confirmed: false,
                override_read_only: false,
            },
            None,
        )
        .unwrap()
}

#[tokio::test]
async fn moves_a_unicode_directory_to_another_directory() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("資料");
    let destination = root.path().join("destination");
    fs::create_dir_all(source.join("child")).unwrap();
    fs::write(source.join("child/file.txt"), b"data").unwrap();
    fs::create_dir(&destination).unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    );

    let started = service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::Move,
                sources: vec![Location::from_native_path(&source).unwrap().into()],
                destination: Some(Location::from_native_path(&destination).unwrap().into()),
                destinations: vec![],
                conflict_policy: OperationConflictPolicyDto::Ask,
                name: None,
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: false,
                symlink_policy: Default::default(),
                permanent_delete_confirmed: false,
                override_read_only: false,
            },
            None,
        )
        .unwrap();

    let result = wait(&service, started.id).await;
    assert_eq!(result.state, OperationStateDto::Completed);
    assert!(!source.exists());
    assert_eq!(
        fs::read(destination.join("資料/child/file.txt")).unwrap(),
        b"data"
    );
}

#[tokio::test]
async fn forced_cross_volume_move_copies_then_deletes_the_source() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    let destination = root.path().join("destination");
    fs::write(&source, b"cross-volume").unwrap();
    fs::create_dir(&destination).unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    );
    service.force_cross_volume_moves_for_tests(true);

    let started = service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::Move,
                sources: vec![Location::from_native_path(&source).unwrap().into()],
                destination: Some(Location::from_native_path(&destination).unwrap().into()),
                destinations: vec![],
                conflict_policy: OperationConflictPolicyDto::Ask,
                name: None,
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: false,
                symlink_policy: Default::default(),
                permanent_delete_confirmed: false,
                override_read_only: false,
            },
            None,
        )
        .unwrap();

    let result = wait(&service, started.id).await;
    assert_eq!(result.state, OperationStateDto::Completed);
    assert!(!source.exists());
    assert_eq!(
        fs::read(destination.join("source.txt")).unwrap(),
        b"cross-volume"
    );
}
