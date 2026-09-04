//! Safe operation undo integration tests confined to temporary roots.
#![expect(clippy::unwrap_used, reason = "temporary-root test setup")]

use std::{fs, path::Path, time::Duration};

use fm_application::{ApplicationError, FileManagerService};
use fm_domain::Location;
use fm_transport_dto::{
    OperationConflictPolicyDto, OperationKindDto, OperationStateDto, RuntimeKindDto,
    StartOperationRequestDto,
};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

fn request(
    kind: OperationKindDto,
    sources: &[&Path],
    destination: Option<&Path>,
) -> StartOperationRequestDto {
    StartOperationRequestDto {
        operation_type: kind,
        sources: sources
            .iter()
            .map(|path| Location::from_native_path(path).unwrap().into())
            .collect(),
        destination: destination.map(|path| Location::from_native_path(path).unwrap().into()),
        destinations: vec![],
        conflict_policy: OperationConflictPolicyDto::Ask,
        name: None,
        archive_format: None,
        archive_compression_level: None,
        create_intermediate_directories: false,
        symlink_policy: Default::default(),
        permanent_delete_confirmed: false,
        override_read_only: false,
    }
}

async fn wait(service: &FileManagerService, id: uuid::Uuid) -> fm_transport_dto::OperationDto {
    let completed = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let operation = service.get_operation(id.into()).unwrap();
            if matches!(
                operation.state,
                OperationStateDto::Cancelled
                    | OperationStateDto::Completed
                    | OperationStateDto::CompletedWithWarnings
                    | OperationStateDto::Failed
                    | OperationStateDto::Interrupted
            ) {
                return operation;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    match completed {
        Ok(operation) => operation,
        Err(_) => {
            let operation = service.get_operation(id.into()).unwrap();
            panic!(
                "operation {id} did not finish within 60 seconds; last state: {:?}",
                operation.state
            )
        }
    }
}

#[tokio::test]
async fn rename_undo_restores_the_original_location_once() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("before.txt");
    let renamed = root.path().join("after.txt");
    fs::write(&original, b"original").unwrap();
    let service = service(&root);

    let rename = service
        .start_operation(
            request(OperationKindDto::Rename, &[&original], Some(&renamed)),
            None,
        )
        .unwrap();
    let completed = wait(&service, rename.id).await;
    assert!(completed.undo.available);
    assert_eq!(completed.undo.reason, None);

    let undo = service.undo_operation(rename.id.into()).unwrap();
    let undone = wait(&service, undo.id).await;

    assert_eq!(undone.operation_type, OperationKindDto::Undo);
    assert_eq!(undone.undo_of, Some(rename.id));
    assert_eq!(undone.state, OperationStateDto::Completed);
    assert_eq!(fs::read(&original).unwrap(), b"original");
    assert!(!renamed.exists());

    let original_history = service.get_operation(rename.id.into()).unwrap();
    assert!(!original_history.undo.available);
    assert_eq!(
        original_history.undo.reason.as_deref(),
        Some("This operation has already been undone.")
    );
    assert!(matches!(
        service.undo_operation(rename.id.into()),
        Err(ApplicationError::InvalidRequest(message))
            if message == "This operation has already been undone."
    ));
}

#[tokio::test]
async fn rename_undo_refuses_changed_missing_and_reused_destinations() {
    for scenario in ["changed", "missing", "reused"] {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("before.txt");
        let renamed = root.path().join("after.txt");
        fs::write(&original, b"original").unwrap();
        let service = service(&root);
        let rename = service
            .start_operation(
                request(OperationKindDto::Rename, &[&original], Some(&renamed)),
                None,
            )
            .unwrap();
        assert_eq!(
            wait(&service, rename.id).await.state,
            OperationStateDto::Completed
        );

        match scenario {
            "changed" => fs::write(&renamed, b"changed").unwrap(),
            "missing" => fs::remove_file(&renamed).unwrap(),
            "reused" => {
                fs::remove_file(&renamed).unwrap();
                fs::write(&renamed, b"replacement").unwrap();
            }
            _ => unreachable!(),
        }

        let undo = service.undo_operation(rename.id.into()).unwrap();
        assert_eq!(
            wait(&service, undo.id).await.state,
            OperationStateDto::Failed
        );
        assert!(!original.exists());
    }
}

#[tokio::test]
async fn rename_undo_never_overwrites_a_reused_original_path() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("before.txt");
    let renamed = root.path().join("after.txt");
    fs::write(&original, b"original").unwrap();
    let service = service(&root);
    let rename = service
        .start_operation(
            request(OperationKindDto::Rename, &[&original], Some(&renamed)),
            None,
        )
        .unwrap();
    assert_eq!(
        wait(&service, rename.id).await.state,
        OperationStateDto::Completed
    );
    fs::write(&original, b"later").unwrap();

    let undo = service.undo_operation(rename.id.into()).unwrap();
    assert_eq!(
        wait(&service, undo.id).await.state,
        OperationStateDto::Failed
    );
    assert_eq!(fs::read(&original).unwrap(), b"later");
    assert_eq!(fs::read(&renamed).unwrap(), b"original");
}

#[tokio::test]
async fn copy_and_duplicate_undo_remove_only_unchanged_created_entries() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    let destination = root.path().join("destination");
    fs::write(&source, b"source").unwrap();
    fs::create_dir(&destination).unwrap();
    let service = service(&root);

    let copy = service
        .start_operation(
            request(OperationKindDto::Copy, &[&source], Some(&destination)),
            None,
        )
        .unwrap();
    assert_eq!(
        wait(&service, copy.id).await.state,
        OperationStateDto::Completed
    );
    let copied = destination.join("source.txt");
    let undo_copy = service.undo_operation(copy.id.into()).unwrap();
    assert_eq!(
        wait(&service, undo_copy.id).await.state,
        OperationStateDto::Completed
    );
    assert!(!copied.exists());
    assert_eq!(fs::read(&source).unwrap(), b"source");

    let duplicate = service
        .start_operation(request(OperationKindDto::Duplicate, &[&source], None), None)
        .unwrap();
    assert_eq!(
        wait(&service, duplicate.id).await.state,
        OperationStateDto::Completed
    );
    let duplicated = root.path().join("source copy.txt");
    assert!(duplicated.exists());
    let undo_duplicate = service.undo_operation(duplicate.id.into()).unwrap();
    assert_eq!(
        wait(&service, undo_duplicate.id).await.state,
        OperationStateDto::Completed
    );
    assert!(!duplicated.exists());
}

#[tokio::test]
async fn copy_undo_refuses_to_delete_a_modified_output() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    let destination = root.path().join("destination");
    fs::write(&source, b"source").unwrap();
    fs::create_dir(&destination).unwrap();
    let service = service(&root);
    let copy = service
        .start_operation(
            request(OperationKindDto::Copy, &[&source], Some(&destination)),
            None,
        )
        .unwrap();
    assert_eq!(
        wait(&service, copy.id).await.state,
        OperationStateDto::Completed
    );
    let copied = destination.join("source.txt");
    fs::write(&copied, b"later edit").unwrap();

    let undo = service.undo_operation(copy.id.into()).unwrap();
    assert_eq!(
        wait(&service, undo.id).await.state,
        OperationStateDto::Failed
    );
    assert_eq!(fs::read(&copied).unwrap(), b"later edit");
}

#[tokio::test]
async fn permanent_delete_explains_that_it_is_not_undoable() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("delete.txt");
    fs::write(&source, b"gone").unwrap();
    let service = service(&root);
    let mut delete = request(OperationKindDto::Delete, &[&source], None);
    delete.permanent_delete_confirmed = true;
    let operation = service.start_operation(delete, None).unwrap();
    let completed = wait(&service, operation.id).await;

    assert!(!completed.undo.available);
    assert_eq!(
        completed.undo.reason.as_deref(),
        Some("This operation does not retain enough evidence to be undone safely.")
    );
}

#[tokio::test]
async fn move_undo_restores_the_original_location() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("move.txt");
    let destination = root.path().join("destination");
    fs::write(&source, b"move").unwrap();
    fs::create_dir(&destination).unwrap();
    let service = service(&root);
    let operation = service
        .start_operation(
            request(OperationKindDto::Move, &[&source], Some(&destination)),
            None,
        )
        .unwrap();
    let completed = wait(&service, operation.id).await;
    assert!(completed.undo.available);
    assert!(!source.exists());

    let undo = service.undo_operation(operation.id.into()).unwrap();
    assert_eq!(
        wait(&service, undo.id).await.state,
        OperationStateDto::Completed
    );
    assert_eq!(fs::read(&source).unwrap(), b"move");
    assert!(!destination.join("move.txt").exists());
}

#[tokio::test]
async fn partial_copy_failure_retains_undo_for_outputs_that_were_created() {
    let root = tempfile::tempdir().unwrap();
    let present = root.path().join("present.txt");
    let missing = root.path().join("missing.txt");
    let destination = root.path().join("destination");
    fs::write(&present, b"present").unwrap();
    fs::create_dir(&destination).unwrap();
    let service = service(&root);
    let copy = service
        .start_operation(
            request(
                OperationKindDto::Copy,
                &[&present, &missing],
                Some(&destination),
            ),
            None,
        )
        .unwrap();
    let completed = wait(&service, copy.id).await;
    assert_eq!(completed.state, OperationStateDto::CompletedWithWarnings);
    assert!(completed.undo.available);

    let undo = service.undo_operation(copy.id.into()).unwrap();
    assert_eq!(
        wait(&service, undo.id).await.state,
        OperationStateDto::Completed
    );
    assert!(!destination.join("present.txt").exists());
}

#[tokio::test]
async fn restart_preserves_the_evidence_needed_for_undo() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    let destination = root.path().join("destination");
    fs::write(&source, b"source").unwrap();
    fs::create_dir(&destination).unwrap();
    let operation_id = {
        let service = service(&root);
        let operation = service
            .start_operation(
                request(OperationKindDto::Copy, &[&source], Some(&destination)),
                None,
            )
            .unwrap();
        let completed = wait(&service, operation.id).await;
        assert!(completed.undo.available);
        operation.id
    };

    let restarted = service(&root);
    let restored = restarted.get_operation(operation_id.into()).unwrap();
    assert!(restored.undo.available);
    let undo = restarted.undo_operation(operation_id.into()).unwrap();
    assert_eq!(
        wait(&restarted, undo.id).await.state,
        OperationStateDto::Completed
    );
    assert!(!destination.join("source.txt").exists());
}

#[tokio::test]
async fn cancelling_an_undo_job_leaves_a_cancelled_audit_history_entry() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    for index in 0..1_000 {
        fs::write(source.join(format!("{index:04}.txt")), b"data").unwrap();
    }
    let service = service(&root);
    let copy = service
        .start_operation(
            request(OperationKindDto::Copy, &[&source], Some(&destination)),
            None,
        )
        .unwrap();
    assert_eq!(
        wait(&service, copy.id).await.state,
        OperationStateDto::Completed
    );

    let undo = service.undo_operation(copy.id.into()).unwrap();
    service.cancel_operation(undo.id.into()).unwrap();
    assert_eq!(
        wait(&service, undo.id).await.state,
        OperationStateDto::Cancelled
    );
    let retained = service.get_operation(copy.id.into()).unwrap();
    assert!(retained.undo.available);
}
