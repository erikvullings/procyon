//! `createFile` with initial content (task 0264), confined to temporary roots.

use std::time::Duration;

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    LocationDto, OperationConflictPolicyDto, OperationDto, OperationKindDto, OperationStateDto,
    RuntimeKindDto, StartOperationRequestDto,
};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

async fn create_file(
    service: &FileManagerService,
    parent: &Location,
    name: &str,
    content: Option<&str>,
) -> OperationDto {
    let operation = service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::CreateFile,
                sources: Vec::new(),
                destination: Some(LocationDto::from(parent.clone())),
                destinations: vec![],
                conflict_policy: OperationConflictPolicyDto::Ask,
                name: Some(name.to_owned()),
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: false,
                initial_content: content.map(str::to_owned),
                skip_space_check: false,
                image_metadata_edit: None,
                geo_transform: None,
                symlink_policy: Default::default(),
                permanent_delete_confirmed: false,
                override_read_only: false,
                link: None,
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
async fn writes_initial_content_and_never_overwrites_an_existing_file() {
    let root = tempfile::tempdir().expect("temporary directory");
    let location = Location::from_native_path(root.path()).expect("location");
    let service = service(&root);

    let created = create_file(&service, &location, "mix.m3u8", Some("#EXTM3U\n日本.mp3\n")).await;
    assert_eq!(created.state, OperationStateDto::Completed);
    assert_eq!(
        std::fs::read_to_string(root.path().join("mix.m3u8")).expect("file content"),
        "#EXTM3U\n日本.mp3\n"
    );

    let again = create_file(&service, &location, "mix.m3u8", Some("replacement")).await;
    assert_eq!(again.state, OperationStateDto::Failed);
    assert_eq!(
        std::fs::read_to_string(root.path().join("mix.m3u8")).expect("file content"),
        "#EXTM3U\n日本.mp3\n"
    );
}

#[tokio::test]
async fn without_content_still_creates_an_empty_file() {
    let root = tempfile::tempdir().expect("temporary directory");
    let location = Location::from_native_path(root.path()).expect("location");
    let service = service(&root);

    let created = create_file(&service, &location, "empty.txt", None).await;
    assert_eq!(created.state, OperationStateDto::Completed);
    assert_eq!(
        std::fs::metadata(root.path().join("empty.txt"))
            .expect("metadata")
            .len(),
        0
    );
}
