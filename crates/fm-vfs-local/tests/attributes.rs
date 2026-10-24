//! Permission and ownership editing contract for the local provider (task 0258).
//! Every test runs inside a temporary root and never follows symbolic links.

use fm_domain::{EntryId, Location};
use fm_vfs::{AttributeChange, EntryRef, FileSystemProvider, ProviderCapabilities, VfsError};
use fm_vfs_local::LocalFileSystemProvider;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

fn entry(path: &std::path::Path) -> EntryRef {
    EntryRef {
        id: EntryId::new(),
        location: Location::from_native_path(path).expect("local location"),
    }
}

#[test]
fn advertises_support_matching_the_platform() {
    let provider = LocalFileSystemProvider::new();
    let location =
        Location::from_native_path(std::env::temp_dir().as_path()).expect("test fixture");
    let support = provider.attribute_support(&location).expect("test fixture");
    assert!(
        provider
            .capabilities()
            .contains(ProviderCapabilities::SET_PERMISSIONS)
    );
    assert_eq!(support.mode, cfg!(unix));
    assert_eq!(support.ownership, cfg!(unix));
    assert_eq!(support.windows_attributes, cfg!(windows));
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

    #[tokio::test]
    async fn chmod_sets_all_twelve_editable_bits_and_reads_them_back() {
        let root = tempdir().expect("test fixture");
        let file = root.path().join("a.txt");
        std::fs::write(&file, b"x").expect("test fixture");
        let provider = LocalFileSystemProvider::new();
        let target = entry(&file);

        let before = provider
            .read_attributes(&target, CancellationToken::new())
            .await
            .expect("test fixture");
        assert_eq!(
            before.mode,
            Some(std::fs::metadata(&file).expect("test fixture").mode() & 0o7777)
        );

        provider
            .change_attributes(
                &target,
                AttributeChange {
                    mode: Some(0o2750),
                    ..AttributeChange::default()
                },
                CancellationToken::new(),
            )
            .await
            .expect("test fixture");
        let after = provider
            .read_attributes(&target, CancellationToken::new())
            .await
            .expect("test fixture");
        assert_eq!(after.mode, Some(0o2750));
        assert_eq!(
            std::fs::metadata(&file)
                .expect("test fixture")
                .permissions()
                .mode()
                & 0o7777,
            0o2750
        );
    }

    #[tokio::test]
    async fn chown_to_the_current_owner_succeeds_and_to_root_reports_permission_denied() {
        let root = tempdir().expect("test fixture");
        let file = root.path().join("a.txt");
        std::fs::write(&file, b"x").expect("test fixture");
        let meta = std::fs::metadata(&file).expect("test fixture");
        let provider = LocalFileSystemProvider::new();
        let target = entry(&file);

        provider
            .change_attributes(
                &target,
                AttributeChange {
                    uid: Some(meta.uid()),
                    gid: Some(meta.gid()),
                    ..AttributeChange::default()
                },
                CancellationToken::new(),
            )
            .await
            .expect("test fixture");

        // Only meaningful for an unprivileged user: handing a file to root must be refused.
        if meta.uid() != 0 {
            let error = provider
                .change_attributes(
                    &target,
                    AttributeChange {
                        uid: Some(0),
                        ..AttributeChange::default()
                    },
                    CancellationToken::new(),
                )
                .await
                .unwrap_err();
            assert!(
                matches!(error, VfsError::PermissionDenied { .. }),
                "{error:?}"
            );
        }
    }

    #[tokio::test]
    async fn symbolic_links_are_never_followed() {
        let root = tempdir().expect("test fixture");
        let file = root.path().join("real.txt");
        std::fs::write(&file, b"x").expect("test fixture");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))
            .expect("test fixture");
        let link = root.path().join("link");
        symlink(&file, &link).expect("test fixture");
        let provider = LocalFileSystemProvider::new();

        let error = provider
            .change_attributes(
                &entry(&link),
                AttributeChange {
                    mode: Some(0o000),
                    ..AttributeChange::default()
                },
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, VfsError::UnsupportedCapability { .. }));
        assert_eq!(
            std::fs::metadata(&file)
                .expect("test fixture")
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
    }

    #[tokio::test]
    async fn windows_only_fields_are_rejected_on_unix() {
        let root = tempdir().expect("test fixture");
        let file = root.path().join("a.txt");
        std::fs::write(&file, b"x").expect("test fixture");
        let error = LocalFileSystemProvider::new()
            .change_attributes(
                &entry(&file),
                AttributeChange {
                    hidden: Some(true),
                    ..AttributeChange::default()
                },
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, VfsError::UnsupportedCapability { .. }));
    }
}

#[cfg(windows)]
mod windows {
    use super::*;

    #[tokio::test]
    async fn hidden_read_only_and_archive_round_trip_and_unix_fields_are_rejected() {
        let root = tempdir().expect("test fixture");
        let file = root.path().join("a.txt");
        std::fs::write(&file, b"x").expect("test fixture");
        let provider = LocalFileSystemProvider::new();
        let target = entry(&file);

        provider
            .change_attributes(
                &target,
                AttributeChange {
                    hidden: Some(true),
                    read_only: Some(true),
                    archive: Some(false),
                    ..AttributeChange::default()
                },
                CancellationToken::new(),
            )
            .await
            .expect("set attributes");
        let snapshot = provider
            .read_attributes(&target, CancellationToken::new())
            .await
            .expect("read attributes");
        assert_eq!(snapshot.hidden, Some(true));
        assert_eq!(snapshot.read_only, Some(true));
        assert_eq!(snapshot.archive, Some(false));

        let error = provider
            .change_attributes(
                &target,
                AttributeChange {
                    mode: Some(0o600),
                    ..AttributeChange::default()
                },
                CancellationToken::new(),
            )
            .await
            .expect_err("unix bits are unsupported on Windows");
        assert!(matches!(error, VfsError::UnsupportedCapability { .. }));

        // Clear read-only so the temporary directory can be removed.
        provider
            .change_attributes(
                &target,
                AttributeChange {
                    read_only: Some(false),
                    ..AttributeChange::default()
                },
                CancellationToken::new(),
            )
            .await
            .expect("clear read-only");
    }
}
