//! Link creation through the local provider (task 0168).

use std::fs;

use fm_domain::Location;
use fm_vfs::{FileSystemProvider, ProviderCapabilities, RemoveOptions, VfsError};
use fm_vfs_local::LocalFileSystemProvider;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

fn location(path: &std::path::Path) -> Location {
    Location::from_native_path(path).expect("local location")
}

#[test]
fn local_provider_advertises_symlink_creation() {
    let capabilities = LocalFileSystemProvider::new().capabilities();
    assert!(capabilities.contains(ProviderCapabilities::CREATE_SYMLINK));
    assert_eq!(
        capabilities.contains(ProviderCapabilities::CREATE_JUNCTION),
        cfg!(windows)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn stores_the_relative_target_text_verbatim_for_unicode_names() {
    let root = tempdir().expect("temporary directory");
    fs::create_dir(root.path().join("dossiers")).unwrap();
    fs::write(root.path().join("dossiers").join("überzicht 📁.txt"), b"x").unwrap();
    let links = root.path().join("links");
    fs::create_dir(&links).unwrap();
    let link = links.join("snelkoppeling ✓");

    let created = LocalFileSystemProvider::new()
        .create_symlink(
            &location(&link),
            "../dossiers/überzicht 📁.txt",
            false,
            CancellationToken::new(),
        )
        .await
        .expect("create symlink");

    assert_eq!(created.location, location(&link));
    assert_eq!(
        fs::read_link(&link).unwrap(),
        std::path::Path::new("../dossiers/überzicht 📁.txt")
    );
    assert_eq!(fs::read(&link).unwrap(), b"x");
    let summary = LocalFileSystemProvider::new()
        .inspect(&created, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(summary.kind, fm_domain::EntryKind::Symlink);
}

#[cfg(unix)]
#[tokio::test]
async fn directory_links_are_removed_without_touching_the_target() {
    let root = tempdir().expect("temporary directory");
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep.txt"), b"keep").unwrap();
    let link = root.path().join("link");
    let provider = LocalFileSystemProvider::new();
    let created = provider
        .create_symlink(
            &location(&link),
            target.to_str().unwrap(),
            true,
            CancellationToken::new(),
        )
        .await
        .expect("create directory symlink");

    provider
        .remove(
            &created,
            RemoveOptions {
                recursive: true,
                use_trash: false,
            },
            CancellationToken::new(),
        )
        .await
        .expect("remove link");

    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"keep");
}

#[cfg(unix)]
#[tokio::test]
async fn an_existing_destination_is_never_replaced() {
    let root = tempdir().expect("temporary directory");
    let link = root.path().join("occupied");
    fs::write(&link, b"original").unwrap();

    let error = LocalFileSystemProvider::new()
        .create_symlink(
            &location(&link),
            "elsewhere",
            false,
            CancellationToken::new(),
        )
        .await
        .expect_err("destination exists");

    assert!(matches!(error, VfsError::AlreadyExists { .. }));
    assert_eq!(fs::read(&link).unwrap(), b"original");
}

#[cfg(unix)]
#[tokio::test]
async fn a_read_only_parent_reports_permission_denied() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempdir().expect("temporary directory");
    let locked = root.path().join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    let probe = locked.join("probe");
    if fs::write(&probe, b"").is_ok() {
        // Running as root: permissions are not enforced, so the denial cannot be observed.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }

    let error = LocalFileSystemProvider::new()
        .create_symlink(
            &location(&locked.join("link")),
            "target",
            false,
            CancellationToken::new(),
        )
        .await
        .expect_err("parent is read-only");

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(error, VfsError::PermissionDenied { .. }));
}

#[tokio::test]
async fn cancelled_requests_create_nothing() {
    let root = tempdir().expect("temporary directory");
    let link = root.path().join("link");
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = LocalFileSystemProvider::new()
        .create_symlink(&location(&link), "target", false, cancellation)
        .await
        .expect_err("cancelled");

    assert!(matches!(error, VfsError::Cancelled));
    assert!(fs::symlink_metadata(&link).is_err());
}

#[cfg(windows)]
#[tokio::test]
async fn junctions_point_at_directories_and_remove_only_the_link() {
    let root = tempdir().expect("temporary directory");
    let target = root.path().join("doel");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep.txt"), b"keep").unwrap();
    let link = root.path().join("verbinding ✓");
    let provider = LocalFileSystemProvider::new();

    let created = provider
        .create_junction(
            &location(&link),
            &location(&target),
            CancellationToken::new(),
        )
        .await
        .expect("create junction");
    assert_eq!(fs::read(link.join("keep.txt")).unwrap(), b"keep");

    provider
        .remove(
            &created,
            RemoveOptions {
                recursive: true,
                use_trash: false,
            },
            CancellationToken::new(),
        )
        .await
        .expect("remove junction");
    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"keep");
}
