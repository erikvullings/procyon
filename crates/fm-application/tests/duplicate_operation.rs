//! Duplicate integration tests confined to temporary roots.

use std::{fs, time::Duration};

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    OperationConflictPolicyDto, OperationKindDto, OperationStateDto, RuntimeKindDto,
    StartOperationRequestDto,
};

#[tokio::test]
async fn duplicates_files_and_directory_trees_with_collision_safe_names() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("folder");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("child.txt"), b"child").unwrap();
    let file = root.path().join("archive.tar.gz");
    fs::write(&file, b"archive").unwrap();
    fs::write(root.path().join("archive copy.tar.gz"), b"existing").unwrap();
    let dotfile = root.path().join(".env");
    fs::write(&dotfile, b"readonly").unwrap();
    let mut permissions = fs::metadata(&dotfile).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&dotfile, permissions).unwrap();
    let unicode = root.path().join("résumé.txt");
    fs::write(&unicode, b"unicode").unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    );

    let started = service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::Duplicate,
                sources: vec![
                    Location::from_native_path(&file).unwrap().into(),
                    Location::from_native_path(&directory).unwrap().into(),
                    Location::from_native_path(&dotfile).unwrap().into(),
                    Location::from_native_path(&unicode).unwrap().into(),
                ],
                destination: None,
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
    assert_eq!(result.state, OperationStateDto::Completed, "{result:?}");
    assert_eq!(
        fs::read(root.path().join("archive copy 2.tar.gz")).unwrap(),
        b"archive"
    );
    assert_eq!(
        fs::read(root.path().join("folder copy/child.txt")).unwrap(),
        b"child"
    );
    assert_eq!(
        fs::read(root.path().join(".env copy")).unwrap(),
        b"readonly"
    );
    assert_eq!(
        fs::read(root.path().join("résumé copy.txt")).unwrap(),
        b"unicode"
    );
    assert_eq!(result.progress.total_bytes, Some(27));
}
