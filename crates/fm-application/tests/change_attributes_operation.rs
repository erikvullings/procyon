//! `changeAttributes` (task 0258): chmod/chown through the operation engine, recursion scopes,
//! preview, undo and symlink safety. Every test works inside a temporary root.
#![cfg(unix)]

use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::Path;
use std::time::Duration;

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    AttributeChangeDto, AttributeScopeDto, AttributeUnsupportedReasonDto, ChangeAttributesDto,
    InspectAttributesRequestDto, LocationDto, OperationConflictPolicyDto, OperationDto,
    OperationKindDto, OperationStateDto, PreviewAttributeChangeRequestDto, RuntimeKindDto,
    StartOperationRequestDto,
};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

fn location(path: &Path) -> LocationDto {
    LocationDto::from(Location::from_native_path(path).expect("location"))
}

fn mode_of(path: &Path) -> u32 {
    std::fs::symlink_metadata(path).expect("metadata").mode() & 0o7777
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod fixture");
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

fn request(
    sources: &[&Path],
    change: AttributeChangeDto,
    recursive: bool,
    scope: AttributeScopeDto,
) -> StartOperationRequestDto {
    StartOperationRequestDto {
        operation_type: OperationKindDto::ChangeAttributes,
        sources: sources.iter().map(|path| location(path)).collect(),
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
        attribute_change: Some(ChangeAttributesDto {
            change,
            recursive,
            scope,
        }),
        geo_transform: None,
        symlink_policy: Default::default(),
        permanent_delete_confirmed: false,
        override_read_only: false,
        link: None,
    }
}

async fn run(
    service: &FileManagerService,
    sources: &[&Path],
    change: AttributeChangeDto,
    recursive: bool,
    scope: AttributeScopeDto,
) -> OperationDto {
    let operation = service
        .start_operation(request(sources, change, recursive, scope), None)
        .expect("operation accepted");
    wait(service, operation.id).await
}

fn chmod(mode: u32) -> AttributeChangeDto {
    AttributeChangeDto {
        mode: Some(mode),
        ..AttributeChangeDto::default()
    }
}

/// `tree/{a.txt, sub/{b.txt, deeper/c.txt}}` plus an outside file linked from inside the tree.
struct Tree {
    root: tempfile::TempDir,
}

impl Tree {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary directory");
        let tree = root.path().join("tree");
        std::fs::create_dir_all(tree.join("sub/deeper")).expect("test fixture");
        for file in ["a.txt", "sub/b.txt", "sub/deeper/c.txt"] {
            std::fs::write(tree.join(file), b"x").expect("test fixture");
            set_mode(&tree.join(file), 0o644);
        }
        for dir in ["", "sub", "sub/deeper"] {
            set_mode(&tree.join(dir), 0o755);
        }
        let outside = root.path().join("outside.txt");
        std::fs::write(&outside, b"x").expect("test fixture");
        set_mode(&outside, 0o644);
        symlink(&outside, tree.join("sub/link")).expect("test fixture");
        Self { root }
    }

    fn path(&self, relative: &str) -> std::path::PathBuf {
        self.root.path().join(relative)
    }
}

#[tokio::test]
async fn chmod_a_single_file_then_undo_restores_the_previous_mode() {
    let tree = Tree::new();
    let service = service(&tree.root);
    let file = tree.path("tree/a.txt");

    let done = run(
        &service,
        &[&file],
        chmod(0o2750),
        false,
        AttributeScopeDto::Both,
    )
    .await;
    assert_eq!(done.state, OperationStateDto::Completed);
    assert_eq!(mode_of(&file), 0o2750);
    assert!(done.undo.available, "{:?}", done.undo.reason);

    let undo = service
        .undo_operation(done.id.into())
        .expect("undo accepted");
    assert_eq!(
        wait(&service, undo.id).await.state,
        OperationStateDto::Completed
    );
    assert_eq!(mode_of(&file), 0o644);
}

#[tokio::test]
async fn undo_is_refused_once_the_mode_was_changed_by_someone_else() {
    let tree = Tree::new();
    let service = service(&tree.root);
    let file = tree.path("tree/a.txt");
    let done = run(
        &service,
        &[&file],
        chmod(0o600),
        false,
        AttributeScopeDto::Both,
    )
    .await;
    set_mode(&file, 0o666);

    let undo = service
        .undo_operation(done.id.into())
        .expect("undo accepted");
    let undone = wait(&service, undo.id).await;
    assert_ne!(undone.state, OperationStateDto::Completed);
    assert_eq!(mode_of(&file), 0o666, "the foreign change is left alone");
}

#[tokio::test]
async fn recursive_scopes_select_files_folders_or_both_and_never_follow_symlinks() {
    for (scope, files, folders) in [
        (AttributeScopeDto::Both, 0o700, 0o700),
        (AttributeScopeDto::FilesOnly, 0o700, 0o755),
        (AttributeScopeDto::FoldersOnly, 0o644, 0o700),
    ] {
        let tree = Tree::new();
        let service = service(&tree.root);
        let top = tree.path("tree");
        let done = run(&service, &[&top], chmod(0o700), true, scope).await;
        assert_eq!(done.state, OperationStateDto::Completed, "{scope:?}");
        for file in ["tree/a.txt", "tree/sub/b.txt", "tree/sub/deeper/c.txt"] {
            assert_eq!(mode_of(&tree.path(file)), files, "{scope:?} {file}");
        }
        for dir in ["tree", "tree/sub", "tree/sub/deeper"] {
            assert_eq!(mode_of(&tree.path(dir)), folders, "{scope:?} {dir}");
        }
        assert_eq!(
            mode_of(&tree.path("outside.txt")),
            0o644,
            "link target untouched"
        );
    }
}

#[tokio::test]
async fn multi_selection_mixes_a_folder_tree_and_a_loose_file() {
    let tree = Tree::new();
    let service = service(&tree.root);
    let sub = tree.path("tree/sub");
    let loose = tree.path("tree/a.txt");
    let done = run(
        &service,
        &[&loose, &sub],
        chmod(0o640),
        true,
        AttributeScopeDto::FilesOnly,
    )
    .await;
    assert_eq!(done.state, OperationStateDto::Completed);
    assert_eq!(mode_of(&loose), 0o640);
    assert_eq!(mode_of(&tree.path("tree/sub/b.txt")), 0o640);
    assert_eq!(mode_of(&tree.path("tree/sub/deeper/c.txt")), 0o640);
    assert_eq!(mode_of(&sub), 0o755);
}

#[tokio::test]
async fn a_selected_symlink_is_reported_and_left_alone() {
    let tree = Tree::new();
    let service = service(&tree.root);
    let link = tree.path("tree/sub/link");
    let done = run(
        &service,
        &[&link],
        chmod(0o000),
        false,
        AttributeScopeDto::Both,
    )
    .await;
    assert_eq!(done.state, OperationStateDto::CompletedWithWarnings);
    assert_eq!(done.errors.len(), 1);
    assert_eq!(mode_of(&tree.path("outside.txt")), 0o644);
}

#[tokio::test]
async fn refused_ownership_changes_are_reported_per_entry_and_the_rest_continues() {
    let tree = Tree::new();
    if tree
        .path("outside.txt")
        .metadata()
        .expect("test fixture")
        .uid()
        == 0
    {
        return; // root may chown freely, nothing to refuse
    }
    let service = service(&tree.root);
    let top = tree.path("tree");
    let done = run(
        &service,
        &[&top],
        AttributeChangeDto {
            uid: Some(0),
            mode: Some(0o700),
            ..AttributeChangeDto::default()
        },
        true,
        AttributeScopeDto::FilesOnly,
    )
    .await;
    assert_eq!(done.state, OperationStateDto::CompletedWithWarnings);
    assert_eq!(done.errors.len(), 3, "{:?}", done.errors);
    assert!(
        done.errors[0].message.contains("Not permitted"),
        "{}",
        done.errors[0].message
    );
}

#[tokio::test]
async fn invalid_requests_are_rejected_before_any_job_starts() {
    let tree = Tree::new();
    let service = service(&tree.root);
    let file = tree.path("tree/a.txt");
    for change in [AttributeChangeDto::default(), chmod(0o10000)] {
        let result = service.start_operation(
            request(&[&file], change, false, AttributeScopeDto::Both),
            None,
        );
        assert!(result.is_err(), "{change:?}");
    }
    let windows_only = AttributeChangeDto {
        hidden: Some(true),
        ..AttributeChangeDto::default()
    };
    assert!(
        service
            .start_operation(
                request(&[&file], windows_only, false, AttributeScopeDto::Both),
                None
            )
            .is_err()
    );
    assert_eq!(mode_of(&file), 0o644);
}

#[tokio::test]
async fn preview_counts_what_the_scope_would_change() {
    let tree = Tree::new();
    let service = service(&tree.root);
    let top = tree.path("tree");
    let preview = |scope, recursive| {
        let service = &service;
        let top = top.clone();
        async move {
            service
                .preview_attribute_change(PreviewAttributeChangeRequestDto {
                    sources: vec![location(&top)],
                    recursive,
                    scope,
                })
                .await
                .expect("preview")
        }
    };
    let both = preview(AttributeScopeDto::Both, true).await;
    assert_eq!((both.files, both.folders, both.total), (3, 3, 6));
    assert_eq!(both.skipped_symlinks, 1);
    let files = preview(AttributeScopeDto::FilesOnly, true).await;
    assert_eq!((files.files, files.folders, files.total), (3, 0, 3));
    let folders = preview(AttributeScopeDto::FoldersOnly, true).await;
    assert_eq!((folders.files, folders.folders, folders.total), (0, 3, 3));
    let shallow = preview(AttributeScopeDto::Both, false).await;
    assert_eq!(shallow.total, 1);
    assert!(!shallow.truncated);
}

#[tokio::test]
async fn inspect_reports_support_and_current_state_for_a_selection() {
    let tree = Tree::new();
    let service = service(&tree.root);
    let file = tree.path("tree/a.txt");
    let link = tree.path("tree/sub/link");

    let response = service
        .inspect_attributes(InspectAttributesRequestDto {
            sources: vec![location(&file), location(&link)],
        })
        .await
        .expect("inspect");
    assert!(response.support.mode && response.support.ownership);
    assert!(!response.support.windows_attributes);
    assert_eq!(response.unsupported_reason, None);
    assert_eq!(
        response.entries[0].attributes.expect("test fixture").mode,
        Some(0o644)
    );
    assert_eq!(response.entries[1].attributes, None, "links expose nothing");

    let only_link = service
        .inspect_attributes(InspectAttributesRequestDto {
            sources: vec![location(&link)],
        })
        .await
        .expect("inspect link");
    assert_eq!(
        only_link.unsupported_reason,
        Some(AttributeUnsupportedReasonDto::SymbolicLink)
    );
}

#[tokio::test]
async fn a_mode_mask_changes_only_the_touched_bits_of_each_entry_and_undoes_them() {
    let tree = Tree::new();
    let service = service(&tree.root);
    let mixed = tree.path("tree/sub/b.txt");
    let loose = tree.path("tree/a.txt");
    set_mode(&mixed, 0o600);
    let group_write = AttributeChangeDto {
        mode: Some(0o020),
        mode_mask: Some(0o020),
        ..AttributeChangeDto::default()
    };

    let done = run(
        &service,
        &[&loose, &mixed],
        group_write,
        false,
        AttributeScopeDto::Both,
    )
    .await;
    assert_eq!(done.state, OperationStateDto::Completed);
    assert_eq!(
        mode_of(&loose),
        0o664,
        "other bits of each entry stay as they were"
    );
    assert_eq!(mode_of(&mixed), 0o620);

    let undo = service
        .undo_operation(done.id.into())
        .expect("undo accepted");
    assert_eq!(
        wait(&service, undo.id).await.state,
        OperationStateDto::Completed
    );
    assert_eq!(mode_of(&loose), 0o644);
    assert_eq!(mode_of(&mixed), 0o600);
}

#[tokio::test]
async fn providers_without_permission_support_are_reported_and_refused() {
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};

    let tree = Tree::new();
    let service = service(&tree.root);
    let archive = tree.path("bundle.zip");
    let mut writer = ZipWriter::new(std::fs::File::create(&archive).expect("archive"));
    writer
        .start_file("inner.txt", SimpleFileOptions::default())
        .expect("entry");
    writer.write_all(b"x").expect("content");
    writer.finish().expect("finish");
    let file = Location::from_native_path(&archive).expect("location");
    let inner = LocationDto::from(
        Location::parse(&format!(
            "archive://{}!/inner.txt",
            &file.uri["file://".len()..]
        ))
        .expect("archive entry location"),
    );

    let response = service
        .inspect_attributes(InspectAttributesRequestDto {
            sources: vec![inner.clone()],
        })
        .await
        .expect("inspect");
    assert!(!response.support.mode && !response.support.ownership);
    assert_eq!(
        response.unsupported_reason,
        Some(AttributeUnsupportedReasonDto::ProviderUnsupported)
    );

    let mut refused = request(&[], chmod(0o600), false, AttributeScopeDto::Both);
    refused.sources = vec![inner];
    assert!(service.start_operation(refused, None).is_err());
}
