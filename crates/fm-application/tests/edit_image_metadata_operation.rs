//! `editImageMetadata` (task 0274): lossless in-place edits, backups and undo, in temp roots only.

use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    ImageMetadataEditDto, LocationDto, OperationConflictPolicyDto, OperationDto, OperationKindDto,
    OperationStateDto, ReadImageMetadataRequestDto, RuntimeKindDto, StartOperationRequestDto,
};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

fn jpeg_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::new_rgb8(8, 6)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
        .expect("encode jpeg");
    bytes
}

fn location(path: &Path) -> LocationDto {
    LocationDto::from(Location::from_native_path(path).expect("location"))
}

async fn wait(service: &FileManagerService, id: uuid::Uuid) -> OperationDto {
    loop {
        let current = service.get_operation(id.into()).expect("operation");
        if matches!(
            current.state,
            OperationStateDto::Completed
                | OperationStateDto::CompletedWithWarnings
                | OperationStateDto::Failed
        ) {
            return current;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn edit(
    service: &FileManagerService,
    files: &[&Path],
    edit: ImageMetadataEditDto,
) -> OperationDto {
    let operation = service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::EditImageMetadata,
                sources: files.iter().map(|path| location(path)).collect(),
                destination: None,
                destinations: vec![],
                conflict_policy: OperationConflictPolicyDto::Ask,
                name: None,
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: false,
                initial_content: None,
                skip_space_check: false,
                image_metadata_edit: Some(edit),
                geo_transform: None,
                symlink_policy: Default::default(),
                permanent_delete_confirmed: false,
                override_read_only: false,
                link: None,
            },
            None,
        )
        .expect("operation accepted");
    wait(service, operation.id).await
}

async fn description_of(service: &FileManagerService, path: &Path) -> Option<String> {
    let metadata = service
        .read_image_metadata(ReadImageMetadataRequestDto {
            location: location(path),
        })
        .await
        .expect("metadata");
    metadata
        .groups
        .iter()
        .flat_map(|group| &group.tags)
        .find(|tag| tag.name == "ImageDescription")
        .map(|tag| tag.value.clone())
}

#[tokio::test]
async fn edits_in_place_keeps_a_backup_and_can_be_undone() {
    let root = tempfile::tempdir().expect("temporary directory");
    let photo = root.path().join("photo.jpg");
    let original = jpeg_bytes();
    std::fs::write(&photo, &original).expect("fixture");
    let service = service(&root);

    let done = edit(
        &service,
        &[&photo],
        ImageMetadataEditDto {
            description: Some("Sunset".into()),
            backup_original: true,
            ..ImageMetadataEditDto::default()
        },
    )
    .await;
    assert_eq!(done.state, OperationStateDto::Completed);
    assert_eq!(
        description_of(&service, &photo).await.as_deref(),
        Some("Sunset")
    );
    assert_eq!(
        std::fs::read(root.path().join("photo.jpg.orig")).expect("backup"),
        original
    );
    assert!(done.undo.available, "{:?}", done.undo.reason);

    let undo = service
        .undo_operation(done.id.into())
        .expect("undo accepted");
    let undone = wait(&service, undo.id).await;
    assert_eq!(undone.state, OperationStateDto::Completed);
    assert_eq!(std::fs::read(&photo).expect("restored"), original);
    assert!(!root.path().join("photo.jpg.orig").exists());
}

#[tokio::test]
async fn without_a_backup_undo_is_unavailable_and_no_backup_is_written() {
    let root = tempfile::tempdir().expect("temporary directory");
    let photo = root.path().join("photo.jpg");
    std::fs::write(&photo, jpeg_bytes()).expect("fixture");
    let service = service(&root);

    let done = edit(
        &service,
        &[&photo],
        ImageMetadataEditDto {
            description: Some("x".into()),
            ..ImageMetadataEditDto::default()
        },
    )
    .await;
    assert_eq!(done.state, OperationStateDto::Completed);
    assert!(!done.undo.available);
    assert!(!root.path().join("photo.jpg.orig").exists());
    let leftovers: Vec<_> = std::fs::read_dir(root.path())
        .expect("dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".fm-meta-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[tokio::test]
async fn undo_refuses_when_the_file_changed_afterwards() {
    let root = tempfile::tempdir().expect("temporary directory");
    let photo = root.path().join("photo.jpg");
    std::fs::write(&photo, jpeg_bytes()).expect("fixture");
    let service = service(&root);
    let done = edit(
        &service,
        &[&photo],
        ImageMetadataEditDto {
            description: Some("x".into()),
            backup_original: true,
            ..ImageMetadataEditDto::default()
        },
    )
    .await;
    std::fs::write(&photo, b"changed later").expect("later change");

    let undo = service
        .undo_operation(done.id.into())
        .expect("undo accepted");
    assert_eq!(
        wait(&service, undo.id).await.state,
        OperationStateDto::Failed
    );
    assert_eq!(std::fs::read(&photo).expect("untouched"), b"changed later");
}

#[tokio::test]
async fn unsupported_files_are_reported_and_left_untouched() {
    let root = tempfile::tempdir().expect("temporary directory");
    let photo = root.path().join("photo.jpg");
    let notes = root.path().join("notes.txt");
    std::fs::write(&photo, jpeg_bytes()).expect("fixture");
    std::fs::write(&notes, b"hello").expect("fixture");
    let service = service(&root);

    let done = edit(
        &service,
        &[&photo, &notes],
        ImageMetadataEditDto {
            description: Some("both".into()),
            ..ImageMetadataEditDto::default()
        },
    )
    .await;
    assert_eq!(done.state, OperationStateDto::CompletedWithWarnings);
    assert_eq!(done.errors.len(), 1);
    assert_eq!(std::fs::read(&notes).expect("untouched"), b"hello");
    assert_eq!(
        description_of(&service, &photo).await.as_deref(),
        Some("both")
    );
}

#[tokio::test]
async fn rejects_requests_without_an_edit_and_unknown_fields() {
    let root = tempfile::tempdir().expect("temporary directory");
    let photo = root.path().join("photo.jpg");
    std::fs::write(&photo, jpeg_bytes()).expect("fixture");
    let service = service(&root);

    let result = service.start_operation(
        StartOperationRequestDto {
            operation_type: OperationKindDto::EditImageMetadata,
            sources: vec![location(&photo)],
            destination: None,
            destinations: vec![],
            conflict_policy: OperationConflictPolicyDto::Ask,
            name: None,
            archive_format: None,
            archive_compression_level: None,
            create_intermediate_directories: false,
            initial_content: None,
            skip_space_check: false,
            image_metadata_edit: None,
            geo_transform: None,
            symlink_policy: Default::default(),
            permanent_delete_confirmed: false,
            override_read_only: false,
            link: None,
        },
        None,
    );
    assert!(result.is_err());

    let result = service.start_operation(
        StartOperationRequestDto {
            operation_type: OperationKindDto::EditImageMetadata,
            sources: vec![location(&photo)],
            destination: None,
            destinations: vec![],
            conflict_policy: OperationConflictPolicyDto::Ask,
            name: None,
            archive_format: None,
            archive_compression_level: None,
            create_intermediate_directories: false,
            initial_content: None,
            skip_space_check: false,
            image_metadata_edit: Some(ImageMetadataEditDto {
                clear_fields: vec!["nonsense".into()],
                ..ImageMetadataEditDto::default()
            }),
            geo_transform: None,
            symlink_policy: Default::default(),
            permanent_delete_confirmed: false,
            override_read_only: false,
            link: None,
        },
        None,
    );
    assert!(result.is_err());
}
