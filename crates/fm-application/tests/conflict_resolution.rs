//! End-to-end conflict policy and decision tests confined to temporary roots.
#![expect(clippy::unwrap_used, reason = "temporary-root test setup")]

use std::{fs, time::Duration};

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_events::{BackendEventPayload, SessionId, SubscriptionEvent};
use fm_transport_dto::{
    ConflictResolutionDto, OperationConflictPolicyDto, OperationKindDto, OperationStateDto,
    ResolveOperationConflictRequestDto, RuntimeKindDto, StartOperationRequestDto,
};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

fn start_copy(
    service: &FileManagerService,
    sources: &[&std::path::Path],
    destination: &std::path::Path,
    policy: OperationConflictPolicyDto,
) -> fm_transport_dto::OperationDto {
    service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::Copy,
                sources: sources
                    .iter()
                    .map(|path| Location::from_native_path(path).unwrap().into())
                    .collect(),
                destination: Some(Location::from_native_path(destination).unwrap().into()),
                destinations: vec![],
                conflict_policy: policy,
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

/// How long to tolerate *no progress at all* before giving up. A fixed wall-clock budget for the
/// whole wait was flaky under CPU contention (a 2,000-item operation genuinely still advancing,
/// just slower than usual, would get killed by an unrelated timer) - this only fails when the
/// operation has truly stalled, so it stays fast in the common case and robust under load.
const STALL_TIMEOUT: Duration = Duration::from_secs(20);

async fn wait_for_state(
    service: &FileManagerService,
    id: uuid::Uuid,
    expected: OperationStateDto,
) -> fm_transport_dto::OperationDto {
    let mut last;
    let mut last_progress = None;
    let mut last_progress_change = tokio::time::Instant::now();
    loop {
        let operation = service.get_operation(id.into()).unwrap();
        if operation.state == expected {
            return operation;
        }
        let reached_other_terminal_state = matches!(
            operation.state,
            OperationStateDto::Completed
                | OperationStateDto::CompletedWithWarnings
                | OperationStateDto::Failed
                | OperationStateDto::Cancelled
        );
        let progress = (
            operation.progress.completed_items,
            operation.progress.completed_bytes,
        );
        if last_progress != Some(progress) {
            last_progress = Some(progress);
            last_progress_change = tokio::time::Instant::now();
        }
        last = Some(operation);
        if reached_other_terminal_state || last_progress_change.elapsed() > STALL_TIMEOUT {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mut sub = service
        .event_bus()
        .subscribe_all_workspaces(SessionId::new("diagnostic"), Some(0));
    let mut failure_message = None;
    while let Ok(Ok(SubscriptionEvent::Event(envelope))) =
        tokio::time::timeout(Duration::from_millis(200), sub.recv()).await
    {
        if let BackendEventPayload::OperationFailed { message, .. } = &envelope.payload {
            failure_message = Some(message.clone());
            break;
        }
    }
    panic!(
        "operation did not reach {expected:?}; last observed: {last:?}; failure_message={failure_message:?}"
    )
}

#[tokio::test]
async fn ask_emits_both_entries_and_applies_the_requested_one_shot_decision() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let source = root.path().join("same.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(destination.join("same.txt"), b"destination").unwrap();
    let service = service(&root);
    let mut events = service
        .event_bus()
        .subscribe_all_workspaces(SessionId::new("test"), None);

    let operation = start_copy(
        &service,
        &[&source],
        &destination,
        OperationConflictPolicyDto::Ask,
    );
    wait_for_state(
        &service,
        operation.id,
        OperationStateDto::WaitingForConflictResolution,
    )
    .await;

    let conflict = loop {
        if let SubscriptionEvent::Event(event) = events.recv().await.unwrap()
            && let BackendEventPayload::OperationConflict { conflict } = event.payload
        {
            break conflict;
        }
    };
    assert_eq!(conflict.source.name, "same.txt");
    assert_eq!(conflict.source.size, Some(6));
    assert!(conflict.source.modified_at.is_some());
    assert_eq!(conflict.destination.name, "same.txt");
    assert_eq!(conflict.destination.size, Some(11));
    assert!(conflict.destination.modified_at.is_some());

    service
        .resolve_operation_conflict(
            operation.id.into(),
            ResolveOperationConflictRequestDto {
                resolution: ConflictResolutionDto::RenameNew,
                apply_to_all_similar: false,
            },
        )
        .unwrap();
    wait_for_state(&service, operation.id, OperationStateDto::Completed).await;
    assert_eq!(
        fs::read(destination.join("same.txt")).unwrap(),
        b"destination"
    );
    assert_eq!(
        fs::read(destination.join("same (copy 1).txt")).unwrap(),
        b"source"
    );
}

#[tokio::test]
async fn skip_and_apply_to_all_leave_every_existing_file_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::create_dir(destination.join("source")).unwrap();
    for name in ["first.txt", "second.txt"] {
        fs::write(source.join(name), b"source").unwrap();
        fs::write(destination.join("source").join(name), b"existing").unwrap();
    }
    let service = service(&root);
    let operation = start_copy(
        &service,
        &[&source],
        &destination,
        OperationConflictPolicyDto::Ask,
    );
    wait_for_state(
        &service,
        operation.id,
        OperationStateDto::WaitingForConflictResolution,
    )
    .await;
    service
        .resolve_operation_conflict(
            operation.id.into(),
            ResolveOperationConflictRequestDto {
                resolution: ConflictResolutionDto::Skip,
                apply_to_all_similar: true,
            },
        )
        .unwrap();

    let completed = wait_for_state(&service, operation.id, OperationStateDto::Completed).await;
    assert_eq!(completed.progress.completed_items, 3);
    assert_eq!(completed.progress.completed_bytes, 0);
    assert_eq!(
        fs::read(destination.join("source/first.txt")).unwrap(),
        b"existing"
    );
    assert_eq!(
        fs::read(destination.join("source/second.txt")).unwrap(),
        b"existing"
    );
}

#[tokio::test]
async fn a_file_never_replaces_a_directory_even_with_overwrite() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("destination");
    fs::create_dir_all(destination.join("same")).unwrap();
    let source = root.path().join("same");
    fs::write(&source, b"file").unwrap();
    let service = service(&root);

    let operation = start_copy(
        &service,
        &[&source],
        &destination,
        OperationConflictPolicyDto::Overwrite,
    );
    wait_for_state(&service, operation.id, OperationStateDto::Failed).await;
    assert!(destination.join("same").is_dir());
}

#[tokio::test]
async fn reconnect_republishes_a_pending_conflict_and_cancel_keeps_the_operation_safe() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let source = root.path().join("same.txt");
    fs::write(&source, b"source").unwrap();
    fs::write(destination.join("same.txt"), b"existing").unwrap();
    let service = service(&root);
    let operation = start_copy(
        &service,
        &[&source],
        &destination,
        OperationConflictPolicyDto::Ask,
    );
    wait_for_state(
        &service,
        operation.id,
        OperationStateDto::WaitingForConflictResolution,
    )
    .await;
    let mut reconnected = service
        .event_bus()
        .subscribe_all_workspaces(SessionId::new("reconnected"), None);
    service.republish_pending_operation_conflicts();
    let event = reconnected.recv().await.unwrap();
    assert!(matches!(
        event,
        SubscriptionEvent::Event(event)
            if matches!(event.payload, BackendEventPayload::OperationConflict { .. })
    ));
    service
        .resolve_operation_conflict(
            operation.id.into(),
            ResolveOperationConflictRequestDto {
                resolution: ConflictResolutionDto::CancelOperation,
                apply_to_all_similar: false,
            },
        )
        .unwrap();
    wait_for_state(&service, operation.id, OperationStateDto::Cancelled).await;
    assert_eq!(fs::read(destination.join("same.txt")).unwrap(), b"existing");
}

#[tokio::test]
async fn a_destination_appearing_after_planning_is_resolved_like_an_initial_conflict() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let source = root.path().join("late");
    fs::create_dir(&source).unwrap();
    for index in 0..2_000 {
        fs::write(source.join(format!("{index:04}.txt")), b"data").unwrap();
    }
    let service = service(&root);
    let operation = start_copy(
        &service,
        &[&source],
        &destination,
        OperationConflictPolicyDto::Ask,
    );
    for _ in 0..10_000 {
        if service.get_operation(operation.id.into()).unwrap().state == OperationStateDto::Planning
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        service.get_operation(operation.id.into()).unwrap().state,
        OperationStateDto::Planning
    );
    fs::create_dir(destination.join("late")).unwrap();
    fs::write(destination.join("late/appeared.txt"), b"appeared late").unwrap();

    wait_for_state(
        &service,
        operation.id,
        OperationStateDto::WaitingForConflictResolution,
    )
    .await;
    service
        .resolve_operation_conflict(
            operation.id.into(),
            ResolveOperationConflictRequestDto {
                resolution: ConflictResolutionDto::Overwrite,
                apply_to_all_similar: false,
            },
        )
        .unwrap();
    wait_for_state(&service, operation.id, OperationStateDto::Completed).await;
    assert_eq!(
        fs::read_dir(destination.join("late")).unwrap().count(),
        2_001
    );
}
