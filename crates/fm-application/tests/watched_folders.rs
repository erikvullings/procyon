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
