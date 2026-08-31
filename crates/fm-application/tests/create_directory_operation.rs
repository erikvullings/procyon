//! End-to-end application operation tests confined to temporary roots.

use std::collections::HashSet;
use std::time::Duration;

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_events::{
    BackendEventPayload, DirectoryDeltaPayload, EventBus, SessionId, SubscriptionEvent,
};
use fm_transport_dto::{
    ListDirectoryRequest, LocationDto, OperationConflictPolicyDto, OperationKindDto,
    OperationStateDto, RuntimeKindDto, StartOperationRequestDto,
};
use uuid::Uuid;

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

async fn create(
    service: &FileManagerService,
    parent: &Location,
    name: &str,
    create_intermediates: bool,
) -> fm_transport_dto::OperationDto {
    let operation = service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::CreateDirectory,
                sources: Vec::new(),
                destination: Some(LocationDto::from(parent.clone())),
                destinations: vec![],
                conflict_policy: OperationConflictPolicyDto::Ask,
                name: Some(name.to_owned()),
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: create_intermediates,
                symlink_policy: Default::default(),
                permanent_delete_confirmed: false,
                override_read_only: false,
            },
            None,
        )
        .expect("operation accepted");
    loop {
        let current = service
            .get_operation(operation.id.into())
            .expect("operation");
        if matches!(
            current.state,
            OperationStateDto::Completed | OperationStateDto::Failed
        ) {
            return current;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn creates_unicode_directory_and_only_explicit_intermediates() {
    let root = tempfile::tempdir().expect("temporary directory");
    let location = Location::from_native_path(root.path()).expect("location");
    let service = service(&root);

    assert_eq!(
        create(&service, &location, "資料", false).await.state,
        OperationStateDto::Completed
    );
    assert!(root.path().join("資料").is_dir());

    assert_eq!(
        create(&service, &location, "one/two", false).await.state,
        OperationStateDto::Failed
    );
    assert!(!root.path().join("one").exists());
    assert_eq!(
        create(&service, &location, "one/two", true).await.state,
        OperationStateDto::Completed
    );
    assert!(root.path().join("one/two").is_dir());
}

#[tokio::test]
async fn rejects_collision_and_invalid_names_without_mutating_outside_temp_root() {
    let root = tempfile::tempdir().expect("temporary directory");
    std::fs::create_dir(root.path().join("existing")).expect("fixture");
    let location = Location::from_native_path(root.path()).expect("location");
    let service = service(&root);

    for name in ["existing", "", "../escape", "CON"] {
        assert_eq!(
            create(&service, &location, name, false).await.state,
            OperationStateDto::Failed,
            "{name:?} must fail"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn reports_permission_denied_where_the_platform_enforces_it() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().expect("temporary directory");
    let blocked = root.path().join("blocked");
    std::fs::create_dir(&blocked).expect("fixture");
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o500))
        .expect("remove write permission");
    let location = Location::from_native_path(&blocked).expect("location");
    let service = service(&root);
    let result = create(&service, &location, "child", false).await;
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700))
        .expect("restore permissions");

    if result.state == OperationStateDto::Completed {
        eprintln!("filesystem/user does not enforce mode-based write denial");
    } else {
        assert_eq!(result.state, OperationStateDto::Failed);
    }
}

#[tokio::test]
async fn creation_refreshes_an_open_directory_through_an_added_delta() {
    let root = tempfile::tempdir().expect("temporary directory");
    let location = Location::from_native_path(root.path()).expect("location");
    let events = EventBus::new(32);
    let service = FileManagerService::with_event_bus(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
        events.clone(),
    );
    let workspace_id = fm_domain::WorkspaceId::new();
    let pane_id = fm_domain::PaneId::new();
    service
        .list_directory(ListDirectoryRequest {
            workspace_id: workspace_id.into(),
            pane_id: pane_id.into(),
            request_id: Uuid::new_v4(),
            location: location.clone().into(),
            continuation_token: None,
            sort: Vec::new(),
            show_hidden: false,
            folders_first: true,
            show_git_status: false,
        })
        .await
        .expect("open directory");
    let second_pane_id = fm_domain::PaneId::new();
    service
        .list_directory(ListDirectoryRequest {
            workspace_id: workspace_id.into(),
            pane_id: second_pane_id.into(),
            request_id: Uuid::new_v4(),
            location: location.clone().into(),
            continuation_token: None,
            sort: Vec::new(),
            show_hidden: false,
            folders_first: true,
            show_git_status: false,
        })
        .await
        .expect("open second directory pane");
    let mut subscription = events.subscribe(SessionId::new("create-test"), [workspace_id], None);
    tokio::time::sleep(Duration::from_millis(250)).await;

    assert_eq!(
        create(&service, &location, "from-operation", false)
            .await
            .state,
        OperationStateDto::Completed
    );
    let expected_panes = HashSet::from([pane_id, second_pane_id]);
    let mut refreshed_panes = HashSet::new();
    while refreshed_panes != expected_panes {
        let event = tokio::time::timeout(Duration::from_secs(3), subscription.recv())
            .await
            .expect("delta timeout")
            .expect("event bus open");
        if let SubscriptionEvent::Event(event) = event
            && let BackendEventPayload::DirectoryDelta {
                pane_id: event_pane,
                delta,
            } = event.payload
        {
            assert!(expected_panes.contains(&event_pane));
            match delta {
                DirectoryDeltaPayload::EntriesAdded { entries, .. } => {
                    if !entries.iter().any(|entry| entry.name == "from-operation") {
                        continue;
                    }
                }
                DirectoryDeltaPayload::Reset { snapshot } => {
                    assert!(
                        snapshot
                            .entries
                            .iter()
                            .any(|entry| entry.name == "from-operation")
                    );
                }
                _ => continue,
            }
            refreshed_panes.insert(event_pane);
        }
    }
}
