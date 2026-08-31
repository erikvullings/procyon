//! Archive copy/delete integration tests through the ordinary operation engine.

use std::{fs, io::Write, time::Duration};

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    OperationConflictPolicyDto, OperationKindDto, OperationStateDto, RuntimeKindDto,
    StartOperationRequestDto,
};
use fm_vfs::FileSystemProvider;
use zip::{ZipWriter, write::SimpleFileOptions};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

fn archive_root(path: &std::path::Path) -> Location {
    let file = Location::from_native_path(path).expect("absolute temporary path");
    Location::parse(&format!("archive://{}!", &file.uri["file://".len()..]))
        .expect("valid archive root")
}

async fn run(
    service: &FileManagerService,
    kind: OperationKindDto,
    sources: Vec<Location>,
    destination: Option<Location>,
) -> fm_transport_dto::OperationDto {
    let started = service
        .start_operation(
            StartOperationRequestDto {
                operation_type: kind,
                sources: sources.into_iter().map(Into::into).collect(),
                destination: destination.map(Into::into),
                destinations: vec![],
                conflict_policy: OperationConflictPolicyDto::Ask,
                name: None,
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: false,
                symlink_policy: Default::default(),
                permanent_delete_confirmed: true,
                override_read_only: false,
            },
            None,
        )
        .expect("operation accepted");
    for _ in 0..1_000 {
        let operation = service
            .get_operation(started.id.into())
            .expect("operation exists");
        if matches!(
            operation.state,
            OperationStateDto::Completed
                | OperationStateDto::CompletedWithWarnings
                | OperationStateDto::Failed
                | OperationStateDto::Cancelled
                | OperationStateDto::WaitingForConflictResolution
        ) {
            return operation;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("archive operation did not finish")
}

#[tokio::test]
async fn ordinary_engine_copies_into_within_and_out_of_zip_then_deletes() {
    let root = tempfile::tempdir().expect("temporary root");
    let archive_path = root.path().join("sample.zip");
    let file = fs::File::create(&archive_path).expect("create ZIP");
    let mut writer = ZipWriter::new(file);
    writer
        .start_file("existing.txt", SimpleFileOptions::default())
        .expect("start entry");
    writer.write_all(b"existing").expect("write entry");
    writer
        .add_directory("folder/", SimpleFileOptions::default())
        .expect("add directory");
    writer.finish().expect("finish ZIP");
    let input = root.path().join("input.txt");
    fs::write(&input, b"archive engine").expect("write source");
    let tree = root.path().join("tree");
    fs::create_dir_all(tree.join("nested")).expect("create source tree");
    fs::write(tree.join("nested/leaf.txt"), b"tree leaf").expect("write tree leaf");
    let output = root.path().join("output");
    fs::create_dir(&output).expect("create output directory");
    let service = service(&root);
    let archive = archive_root(&archive_path);

    let copied_in = run(
        &service,
        OperationKindDto::Copy,
        vec![Location::from_native_path(&input).expect("source location")],
        Some(archive.clone()),
    )
    .await;
    assert_eq!(
        copied_in.state,
        OperationStateDto::Completed,
        "{copied_in:?}"
    );

    let tree_in = run(
        &service,
        OperationKindDto::Copy,
        vec![Location::from_native_path(&tree).expect("tree location")],
        Some(archive.clone()),
    )
    .await;
    assert_eq!(tree_in.state, OperationStateDto::Completed, "{tree_in:?}");

    let tree_out = run(
        &service,
        OperationKindDto::Copy,
        vec![archive.join("tree").expect("archived tree")],
        Some(Location::from_native_path(&output).expect("output location")),
    )
    .await;
    assert_eq!(tree_out.state, OperationStateDto::Completed, "{tree_out:?}");
    assert_eq!(
        fs::read(output.join("tree/nested/leaf.txt")).expect("read extracted tree leaf"),
        b"tree leaf"
    );

    let copied_within = run(
        &service,
        OperationKindDto::Copy,
        vec![archive.join("input.txt").expect("archive entry")],
        Some(archive.join("folder").expect("archive directory")),
    )
    .await;
    assert_eq!(
        copied_within.state,
        OperationStateDto::Completed,
        "{copied_within:?}"
    );

    let copied_out = run(
        &service,
        OperationKindDto::Copy,
        vec![
            archive
                .join("folder")
                .expect("archive directory")
                .join("input.txt")
                .expect("nested archive entry"),
        ],
        Some(Location::from_native_path(&output).expect("output location")),
    )
    .await;
    assert_eq!(
        copied_out.state,
        OperationStateDto::Completed,
        "{copied_out:?}"
    );
    assert_eq!(
        fs::read(output.join("input.txt")).expect("read extracted copy"),
        b"archive engine"
    );

    let deleted = run(
        &service,
        OperationKindDto::Delete,
        vec![archive.join("folder").expect("archive directory")],
        None,
    )
    .await;
    assert_eq!(deleted.state, OperationStateDto::Completed);
}

#[tokio::test]
async fn selected_files_can_be_packaged_as_zip_or_7z_operations() {
    let root = tempfile::tempdir().expect("temporary root");
    let source = root.path().join("report.txt");
    fs::write(&source, b"packed content").expect("write source");
    let service = service(&root);

    for name in ["reports.zip", "reports.7z"] {
        let archive = root.path().join(name);
        let operation = run(
            &service,
            OperationKindDto::CreateArchive,
            vec![Location::from_native_path(&source).expect("source location")],
            Some(Location::from_native_path(&archive).expect("archive location")),
        )
        .await;
        assert_eq!(
            operation.state,
            OperationStateDto::Completed,
            "{operation:?}"
        );
        let provider = fm_archive::ArchiveFileSystemProvider::new();
        let entries = provider
            .list(
                &archive_root(&archive),
                Default::default(),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("created archive is readable");
        assert_eq!(entries.entries[0].name, "report.txt");
    }
}

#[tokio::test]
async fn archive_creation_validates_format_and_compression_options() {
    let root = tempfile::tempdir().expect("temporary root");
    let source = root.path().join("report.txt");
    fs::write(&source, b"packed content").expect("write source");
    let service = service(&root);

    let invalid = service.start_operation(
        StartOperationRequestDto {
            operation_type: OperationKindDto::CreateArchive,
            sources: vec![
                Location::from_native_path(&source)
                    .expect("source location")
                    .into(),
            ],
            destination: Some(
                Location::from_native_path(&root.path().join("reports.7z"))
                    .expect("destination")
                    .into(),
            ),
            destinations: vec![],
            conflict_policy: OperationConflictPolicyDto::Ask,
            name: None,
            archive_format: Some(fm_transport_dto::ArchiveFormatDto::Zip),
            archive_compression_level: Some(9),
            create_intermediate_directories: false,
            symlink_policy: Default::default(),
            permanent_delete_confirmed: false,
            override_read_only: false,
        },
        None,
    );
    assert!(invalid.is_err(), "mismatched format must be rejected");

    let invalid_level = service.start_operation(
        StartOperationRequestDto {
            operation_type: OperationKindDto::CreateArchive,
            sources: vec![
                Location::from_native_path(&source)
                    .expect("source location")
                    .into(),
            ],
            destination: Some(
                Location::from_native_path(&root.path().join("reports.zip"))
                    .expect("destination")
                    .into(),
            ),
            destinations: vec![],
            conflict_policy: OperationConflictPolicyDto::Ask,
            name: None,
            archive_format: Some(fm_transport_dto::ArchiveFormatDto::Zip),
            archive_compression_level: Some(10),
            create_intermediate_directories: false,
            symlink_policy: Default::default(),
            permanent_delete_confirmed: false,
            override_read_only: false,
        },
        None,
    );
    assert!(
        invalid_level.is_err(),
        "invalid ZIP compression must be rejected"
    );
}

#[tokio::test]
async fn move_to_archive_removes_sources_only_after_a_readable_archive_is_created() {
    let root = tempfile::tempdir().expect("temporary root");
    let source = root.path().join("report.txt");
    fs::write(&source, b"packed content").expect("write source");
    let archive = root.path().join("reports.zip");
    let service = service(&root);

    let operation = run(
        &service,
        OperationKindDto::MoveToArchive,
        vec![Location::from_native_path(&source).expect("source location")],
        Some(Location::from_native_path(&archive).expect("archive location")),
    )
    .await;

    assert_eq!(
        operation.state,
        OperationStateDto::Completed,
        "{operation:?}"
    );
    assert!(
        !source.exists(),
        "source is removed after the archive succeeds"
    );
    let provider = fm_archive::ArchiveFileSystemProvider::new();
    let entries = provider
        .list(
            &archive_root(&archive),
            Default::default(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("created archive is readable");
    assert_eq!(entries.entries[0].name, "report.txt");
}

#[tokio::test]
async fn archive_creation_never_overwrites_an_existing_archive() {
    let root = tempfile::tempdir().expect("temporary root");
    let source = root.path().join("report.txt");
    let archive = root.path().join("reports.zip");
    fs::write(&source, b"new content").expect("write source");
    fs::write(&archive, b"existing archive bytes").expect("write destination");
    let service = service(&root);

    let operation = run(
        &service,
        OperationKindDto::CreateArchive,
        vec![Location::from_native_path(&source).expect("source location")],
        Some(Location::from_native_path(&archive).expect("archive location")),
    )
    .await;

    assert_eq!(operation.state, OperationStateDto::Failed, "{operation:?}");
    assert_eq!(
        fs::read(&archive).expect("read existing archive"),
        b"existing archive bytes"
    );
}
