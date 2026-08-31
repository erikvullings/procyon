//! Integration coverage for `DirectoryService::list_children` (task 0139's directory-tree
//! sidebar), which lists immediate child directories independently of any pane's own listing
//! session.

use std::sync::Arc;

use fm_application::DirectoryService;
use fm_archive::ArchiveFileSystemProvider;
use fm_domain::{EntryKind, Location};
use fm_vfs::ProviderRegistry;
use fm_vfs_local::LocalFileSystemProvider;
use zip::{ZipWriter, write::SimpleFileOptions};

fn service() -> DirectoryService {
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(LocalFileSystemProvider));
    providers.register(Arc::new(ArchiveFileSystemProvider::new()));
    DirectoryService::new(providers)
}

#[tokio::test]
async fn list_children_returns_only_directories_sorted_by_name() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    std::fs::create_dir(root.path().join("zeta")).expect("create zeta");
    std::fs::create_dir(root.path().join("alpha")).expect("create alpha");
    std::fs::write(root.path().join("file.txt"), b"not a directory").expect("create file");
    let location =
        Location::from_native_path(root.path()).expect("temp path must be representable");

    let children = service()
        .list_children(&location, false)
        .await
        .expect("listing succeeds");

    assert_eq!(children.len(), 2);
    assert_eq!(children[0].name, "alpha");
    assert_eq!(children[1].name, "zeta");
    assert!(
        children
            .iter()
            .all(|entry| entry.kind == EntryKind::Directory)
    );
}

#[tokio::test]
async fn list_children_hides_dotfiles_unless_show_hidden_is_set() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    std::fs::create_dir(root.path().join(".hidden")).expect("create hidden dir");
    std::fs::create_dir(root.path().join("visible")).expect("create visible dir");
    let location =
        Location::from_native_path(root.path()).expect("temp path must be representable");

    let hidden_excluded = service()
        .list_children(&location, false)
        .await
        .expect("listing succeeds");
    assert_eq!(hidden_excluded.len(), 1);
    assert_eq!(hidden_excluded[0].name, "visible");

    let hidden_included = service()
        .list_children(&location, true)
        .await
        .expect("listing succeeds");
    assert_eq!(hidden_included.len(), 2);
}

#[tokio::test]
async fn list_children_does_not_disturb_a_pane_s_own_in_flight_listing() {
    // Regression test for the pane-binding hazard documented on `DirectoryService::list`:
    // `list()` cancels a pane's previous in-flight request on every call, keyed by `PaneId`.
    // `list_children` must not go through that path, so calling it repeatedly must never
    // interfere with an actual pane's listing session.
    let root = tempfile::tempdir().expect("must create a temp directory");
    std::fs::create_dir(root.path().join("child")).expect("create child dir");
    let location =
        Location::from_native_path(root.path()).expect("temp path must be representable");
    let service = service();

    let pane_id = fm_domain::PaneId::new();
    let pane_request = fm_transport_dto::ListDirectoryRequest {
        workspace_id: uuid::Uuid::new_v4(),
        pane_id: pane_id.into(),
        request_id: uuid::Uuid::new_v4(),
        location: fm_transport_dto::LocationDto {
            provider_id: location.provider_id.as_str().to_owned(),
            uri: location.uri.clone(),
        },
        continuation_token: None,
        sort: Vec::new(),
        show_hidden: false,
        folders_first: false,
        show_git_status: false,
    };
    let pane_snapshot = service
        .list(pane_request.clone())
        .await
        .expect("pane listing succeeds");

    // Expanding a tree node for the same location must not cancel or alter the pane's cached
    // listing state.
    let _children = service
        .list_children(&location, false)
        .await
        .expect("tree listing succeeds");
    let _children_again = service
        .list_children(&location, false)
        .await
        .expect("tree listing succeeds");

    let refreshed = service
        .refresh(pane_request)
        .await
        .expect("pane refresh still succeeds");
    assert_eq!(refreshed.location, pane_snapshot.location);
}

#[tokio::test]
async fn list_children_works_for_the_archive_provider_not_just_local() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let archive_path = root.path().join("nested.zip");
    {
        let file = std::fs::File::create(&archive_path).expect("create archive file");
        let mut writer = ZipWriter::new(file);
        writer
            .add_directory("inner/", SimpleFileOptions::default())
            .expect("add inner directory");
        writer
            .start_file("inner/leaf.txt", SimpleFileOptions::default())
            .expect("start leaf file");
        std::io::Write::write_all(&mut writer, b"leaf").expect("write leaf contents");
        writer.finish().expect("finish archive");
    }
    let archive_file = Location::from_native_path(&archive_path).expect("valid archive path");
    let archive_root = Location::parse(&format!(
        "archive://{}!",
        &archive_file.uri["file://".len()..]
    ))
    .expect("valid archive root");

    let children = service()
        .list_children(&archive_root, false)
        .await
        .expect("listing the archive root succeeds");

    assert_eq!(children.len(), 1);
    assert_eq!(children[0].name, "inner");
    assert_eq!(children[0].kind, EntryKind::Directory);
}
