//! Application-level behavior for watched-folder setup.

#![allow(clippy::unwrap_used)]

use fm_agent::{SideModel, SideModelError};
use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::RuntimeKindDto;
use tokio_util::sync::CancellationToken;

struct CleanupModel;

#[async_trait::async_trait]
impl SideModel for CleanupModel {
    fn input_bytes(&self) -> usize {
        16_384
    }

    async fn complete(&self, _system: &str, prompt: &str) -> Result<String, SideModelError> {
        assert!(prompt.contains("invoice.pdf"));
        assert!(!prompt.contains("transfer.crdownload"));
        assert!(!prompt.contains(".private"));
        Ok(r#"{"assignments":[{"file":0,"folder":"Receipts"}]}"#.to_owned())
    }
}

#[tokio::test]
async fn initial_cleanup_proposes_a_local_folder_without_moving_files() {
    let root = tempfile::tempdir().unwrap();
    let downloads = root.path().join("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    for name in ["invoice.pdf", "misc.pdf", "transfer.crdownload", ".private"] {
        std::fs::write(downloads.join(name), b"test fixture").unwrap();
    }
    let service = FileManagerService::new(
        RuntimeKindDto::Mock,
        root.path().join("workspaces"),
        root.path().join("settings"),
    );
    let location = Location::from_native_path(&downloads.canonicalize().unwrap()).unwrap();
    let plan = service
        .preview_watched_folder_cleanup(&location, &CleanupModel, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.moves[0].source, location.join("invoice.pdf").unwrap());
    assert_eq!(
        plan.moves[0].destination,
        location.join("Receipts").unwrap()
    );
    assert_eq!(plan.unmatched, vec![location.join("misc.pdf").unwrap()]);
    assert!(downloads.join("invoice.pdf").is_file());
    assert!(!downloads.join("Receipts").exists());
}

struct TraversalModel;

#[async_trait::async_trait]
impl SideModel for TraversalModel {
    fn input_bytes(&self) -> usize {
        16_384
    }

    async fn complete(&self, _system: &str, _prompt: &str) -> Result<String, SideModelError> {
        Ok(r#"{"assignments":[{"file":0,"folder":"../Receipts"}]}"#.to_owned())
    }
}

#[tokio::test]
async fn initial_cleanup_rejects_a_destination_outside_the_watched_folder() {
    let root = tempfile::tempdir().unwrap();
    let downloads = root.path().join("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    std::fs::write(downloads.join("invoice.pdf"), b"test fixture").unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::Mock,
        root.path().join("workspaces"),
        root.path().join("settings"),
    );
    let location = Location::from_native_path(&downloads.canonicalize().unwrap()).unwrap();

    let result = service
        .preview_watched_folder_cleanup(&location, &TraversalModel, CancellationToken::new())
        .await;

    assert!(matches!(
        result,
        Err(fm_application::ApplicationError::InvalidRequest(_))
    ));
    assert!(downloads.join("invoice.pdf").is_file());
    assert!(!root.path().join("Receipts").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn initial_cleanup_rejects_a_symlink_to_another_folder() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let receipts = root_path.join("Receipts");
    std::fs::create_dir(&receipts).unwrap();
    let downloads = root_path.join("Downloads");
    std::os::unix::fs::symlink(&receipts, &downloads).unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::Mock,
        root_path.join("workspaces"),
        root_path.join("settings"),
    );
    let location = Location::from_native_path(&downloads).unwrap();

    let result = service
        .preview_watched_folder_cleanup(&location, &CleanupModel, CancellationToken::new())
        .await;

    assert!(matches!(
        result,
        Err(fm_application::ApplicationError::InvalidRequest(_))
    ));
}

#[tokio::test]
async fn cleanup_preview_job_for_an_empty_folder_completes_without_a_configured_model() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let downloads = root_path.join("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::Mock,
        root_path.join("workspaces"),
        root_path.join("settings"),
    );
    let location = Location::from_native_path(&downloads).unwrap();
    let job = service.start_watched_folder_cleanup(location).unwrap();

    let preview = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(preview) = service.take_watched_folder_cleanup(job).unwrap() {
                break preview;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(preview, fm_application::CleanupPreview::default());
    assert!(matches!(
        service.take_watched_folder_cleanup(job),
        Err(fm_application::ApplicationError::NotFound)
    ));
}

#[tokio::test]
async fn approved_recent_file_is_retained_and_the_queue_survives_restart() {
    use fm_transport_dto::watched_folders::{ApprovedMove, OrganizerRequest};

    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let downloads = root.join("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    std::fs::write(downloads.join("invoice.pdf"), b"invoice").unwrap();
    std::fs::write(downloads.join("other.pdf"), b"other").unwrap();
    let location = Location::from_native_path(&downloads).unwrap();
    let service = || {
        FileManagerService::new(
            RuntimeKindDto::Mock,
            root.join("workspaces"),
            root.join("settings"),
        )
    };
    let result = service()
        .watched_folder(OrganizerRequest::Save {
            root: location.clone().into(),
            delay_hours: 48,
            paused: false,
            rules: vec![],
            approved: vec![ApprovedMove {
                source: location.join("invoice.pdf").unwrap().into(),
                folder: "Receipts".into(),
                version: None,
            }],
        })
        .await
        .unwrap();
    assert_eq!(result.folders[0].pending.len(), 1);
    assert!(downloads.join("invoice.pdf").is_file());
    let restored = service()
        .watched_folder(OrganizerRequest::List)
        .await
        .unwrap();
    assert_eq!(restored.folders, result.folders);
    assert!(downloads.join("other.pdf").is_file());
}

#[tokio::test]
async fn paused_filing_waits_then_moves_and_records_undo_on_resume() {
    use fm_transport_dto::watched_folders::{ApprovedMove, OrganizerRequest};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let downloads = root.join("Downloads");
    let receipts = downloads.join("Receipts");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::write(downloads.join("invoice.pdf"), b"invoice").unwrap();
    let location = Location::from_native_path(&downloads).unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::Mock,
        root.join("workspaces"),
        root.join("settings"),
    );
    let save = |paused, approved| OrganizerRequest::Save {
        root: location.clone().into(),
        delay_hours: 0,
        paused,
        rules: vec![],
        approved,
    };
    let preview = ApprovedMove {
        source: location.join("invoice.pdf").unwrap().into(),
        folder: "Receipts".into(),
        version: None,
    };
    service
        .watched_folder(save(true, vec![preview]))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(11)).await;
    assert!(downloads.join("invoice.pdf").is_file());
    assert!(!receipts.join("invoice.pdf").exists());

    service.watched_folder(save(false, vec![])).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let status = service
                .watched_folder(OrganizerRequest::List)
                .await
                .unwrap();
            if status.folders[0].pending.is_empty() {
                break;
            }
            assert!(
                status.folders[0].pending[0].error.is_none(),
                "{:?}",
                status.folders[0].pending
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(
        std::fs::read(receipts.join("invoice.pdf")).unwrap(),
        b"invoice"
    );
    assert!(!downloads.join("invoice.pdf").exists());
    assert!(
        service
            .list_operations()
            .iter()
            .any(
                |operation| operation.operation_type == fm_transport_dto::OperationKindDto::Move
                    && operation.state == fm_transport_dto::OperationStateDto::Completed
                    && operation.undo.available
            )
    );
}

#[tokio::test]
async fn standing_rule_queues_new_files_without_sweeping_existing_files() {
    use fm_transport_dto::watched_folders::{FilingRule, OrganizerRequest};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let downloads = root.join("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    std::fs::write(downloads.join("existing.pdf"), b"already there").unwrap();
    let location = Location::from_native_path(&downloads).unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::Mock,
        root.join("workspaces"),
        root.join("settings"),
    );
    service
        .watched_folder(OrganizerRequest::Save {
            root: location.clone().into(),
            delay_hours: 48,
            paused: false,
            approved: vec![],
            rules: vec![FilingRule {
                extension: Some("pdf".into()),
                content_type: None,
                minimum_bytes: None,
                folder: "PDFs".into(),
            }],
        })
        .await
        .unwrap();
    std::fs::write(downloads.join("new.pdf"), b"new arrival").unwrap();
    std::fs::write(downloads.join("other.txt"), b"no match").unwrap();

    let queued = tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            let response = service
                .watched_folder(OrganizerRequest::List)
                .await
                .unwrap();
            if !response.folders[0].pending.is_empty() {
                break response.folders[0].pending.clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(
        queued[0].assignment.source,
        location.join("new.pdf").unwrap().into()
    );
    assert_eq!(queued[0].assignment.folder, "PDFs");
    assert!(downloads.join("existing.pdf").is_file());
    assert!(downloads.join("new.pdf").is_file());
    assert!(!downloads.join("PDFs").exists());
}

#[tokio::test]
async fn collision_leaves_both_files_in_place_and_reports_the_failure() {
    use fm_transport_dto::watched_folders::{ApprovedMove, OrganizerRequest};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let downloads = root.join("Downloads");
    let receipts = downloads.join("Receipts");
    std::fs::create_dir_all(&receipts).unwrap();
    std::fs::write(downloads.join("invoice.pdf"), b"source").unwrap();
    std::fs::write(receipts.join("invoice.pdf"), b"original").unwrap();
    let location = Location::from_native_path(&downloads).unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::Mock,
        root.join("workspaces"),
        root.join("settings"),
    );
    service
        .watched_folder(OrganizerRequest::Save {
            root: location.clone().into(),
            delay_hours: 0,
            paused: false,
            rules: vec![],
            approved: vec![ApprovedMove {
                source: location.join("invoice.pdf").unwrap().into(),
                folder: "Receipts".into(),
                version: None,
            }],
        })
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let status = service
                .watched_folder(OrganizerRequest::List)
                .await
                .unwrap();
            if status.folders[0]
                .pending
                .first()
                .is_some_and(|item| item.error.is_some())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(
        std::fs::read(downloads.join("invoice.pdf")).unwrap(),
        b"source"
    );
    assert_eq!(
        std::fs::read(receipts.join("invoice.pdf")).unwrap(),
        b"original"
    );
}

#[tokio::test]
async fn approval_rejects_a_file_edited_since_the_preview() {
    use fm_transport_dto::watched_folders::{ApprovedMove, OrganizerRequest};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let downloads = root.join("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    std::fs::write(downloads.join("invoice.pdf"), b"original").unwrap();
    let location = Location::from_native_path(&downloads).unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::Mock,
        root.join("workspaces"),
        root.join("settings"),
    );
    let plan = service
        .preview_watched_folder_cleanup(&location, &CleanupModel, CancellationToken::new())
        .await
        .unwrap();
    let proposed = &plan.moves[0];
    std::fs::write(downloads.join("invoice.pdf"), b"edited after preview").unwrap();
    let result = service
        .watched_folder(OrganizerRequest::Save {
            root: location.into(),
            delay_hours: 48,
            paused: false,
            rules: vec![],
            approved: vec![ApprovedMove {
                source: proposed.source.clone().into(),
                folder: "Receipts".into(),
                version: Some(proposed.version.clone()),
            }],
        })
        .await;
    assert!(matches!(
        result,
        Err(fm_application::ApplicationError::InvalidRequest(_))
    ));
    let response = service
        .watched_folder(OrganizerRequest::List)
        .await
        .unwrap();
    assert!(response.folders.is_empty());
}
