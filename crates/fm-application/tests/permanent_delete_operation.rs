//! Destructive integration tests confined to temporary roots.
#![expect(clippy::unwrap_used, reason = "temporary-root test setup")]

use std::{fs, time::Duration};

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    ConflictResolutionDto, OperationConflictPolicyDto, OperationKindDto, OperationStateDto,
    ResolveOperationConflictRequestDto, RuntimeKindDto, StartOperationRequestDto,
};

fn request(
    source: &std::path::Path,
    confirmed: bool,
    override_read_only: bool,
) -> StartOperationRequestDto {
    StartOperationRequestDto {
        operation_type: OperationKindDto::Delete,
        sources: vec![Location::from_native_path(source).unwrap().into()],
        destination: None,
        destinations: vec![],
        conflict_policy: OperationConflictPolicyDto::Ask,
        name: None,
        archive_format: None,
        archive_compression_level: None,
        create_intermediate_directories: false,
        symlink_policy: Default::default(),
        permanent_delete_confirmed: confirmed,
        override_read_only,
    }
}

#[tokio::test]
async fn requires_confirmation_then_deletes_a_planned_tree_and_audits_it() {
    // Distinctive enough that it cannot coincidentally appear in the audit log for an unrelated
    // reason (a byte/item count, a timestamp, a temp-directory path segment, ...) - unlike a
    // short numeric payload such as "1234", which a random tempdir name can contain by sheer
    // chance (observed on Windows CI, where `!audit.contains("1234")` failed with no actual
    // content leak).
    const FILE_CONTENT: &[u8] = b"audit-log-must-not-leak-this-file-content-b3f8c2";
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("delete-me");
    fs::create_dir_all(source.join("empty")).unwrap();
    fs::write(source.join("data.bin"), FILE_CONTENT).unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    );

    let awaiting = service
        .start_operation(request(&source, false, false), None)
        .unwrap();
    loop {
        let operation = service.get_operation(awaiting.id.into()).unwrap();
        if operation.state == OperationStateDto::WaitingForConflictResolution {
            assert_eq!(operation.progress.total_items, Some(3));
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    service.cancel_operation(awaiting.id.into()).unwrap();
    assert!(source.exists());

    let started = service
        .start_operation(request(&source, true, false), None)
        .unwrap();
    let result = loop {
        let operation = service.get_operation(started.id.into()).unwrap();
        if matches!(
            operation.state,
            OperationStateDto::Completed | OperationStateDto::Failed
        ) {
            break operation;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };

    assert_eq!(result.state, OperationStateDto::Completed);
    assert_eq!(result.progress.total_items, Some(3));
    assert_eq!(result.progress.total_bytes, Some(FILE_CONTENT.len() as u64));
    assert!(!source.exists());
    let audit = fs::read_to_string(root.path().join("settings/audit.jsonl")).unwrap();
    assert!(audit.contains("permanentDelete"));
    assert!(!audit.contains(std::str::from_utf8(FILE_CONTENT).unwrap()));
}

#[cfg(unix)]
#[tokio::test]
async fn read_only_entries_require_an_explicit_override() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("readonly.txt");
    fs::write(&source, b"safe").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o444)).unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    );

    let rejected = service
        .start_operation(request(&source, true, false), None)
        .unwrap();
    loop {
        let operation = service.get_operation(rejected.id.into()).unwrap();
        if operation.state == OperationStateDto::Failed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(source.exists());

    let accepted = service
        .start_operation(request(&source, true, true), None)
        .unwrap();
    loop {
        let operation = service.get_operation(accepted.id.into()).unwrap();
        if operation.state == OperationStateDto::Completed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!source.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn read_only_descendants_do_not_prevent_the_confirmation_prompt() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("delete-me");
    let read_only = source.join(".git/objects/pack/index.idx");
    fs::create_dir_all(read_only.parent().unwrap()).unwrap();
    fs::write(&read_only, b"safe").unwrap();
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o444)).unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    );

    let started = service
        .start_operation(request(&source, false, false), None)
        .unwrap();
    loop {
        let operation = service.get_operation(started.id.into()).unwrap();
        if operation.state == OperationStateDto::WaitingForConflictResolution {
            assert_eq!(operation.progress.total_items, Some(5));
            break;
        }
        assert_ne!(operation.state, OperationStateDto::Failed);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert!(source.exists());
    assert!(read_only.exists());
    service
        .resolve_operation_conflict(
            started.id.into(),
            ResolveOperationConflictRequestDto {
                resolution: ConflictResolutionDto::Confirm,
                apply_to_all_similar: false,
            },
        )
        .unwrap();
    loop {
        let operation = service.get_operation(started.id.into()).unwrap();
        if operation.state == OperationStateDto::Completed {
            break;
        }
        assert_ne!(operation.state, OperationStateDto::Failed);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!source.exists());
}

#[tokio::test]
async fn cancellation_reports_and_audits_the_exact_deleted_count() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("delete-many");
    fs::create_dir(&source).unwrap();
    for index in 0..2_000 {
        fs::write(source.join(format!("f{index:04}")), b"x").unwrap();
    }
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    );
    let started = service
        .start_operation(request(&source, true, false), None)
        .unwrap();
    loop {
        let operation = service.get_operation(started.id.into()).unwrap();
        if operation.state == OperationStateDto::Running {
            service.cancel_operation(started.id.into()).unwrap();
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let cancelled = loop {
        let operation = service.get_operation(started.id.into()).unwrap();
        if operation.state == OperationStateDto::Cancelled {
            break operation;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    };

    let remaining = fs::read_dir(&source).unwrap().count() as u64;
    assert_eq!(cancelled.progress.completed_items, 2_000 - remaining);
    let audit = fs::read_to_string(root.path().join("settings/audit.jsonl")).unwrap();
    let record: serde_json::Value = serde_json::from_str(audit.lines().last().unwrap()).unwrap();
    assert_eq!(
        record["deletedItems"].as_u64(),
        Some(cancelled.progress.completed_items)
    );
}
