//! OS file clipboard bridge (task 0247): mirrors Procyon's file clipboard to
//! the system clipboard as file references, and reads file references copied
//! in Finder or Explorer so a paste can start an ordinary copy job.
//!
//! Only the desktop host has a system clipboard to share; the browser host
//! never reports the capability. Only local entries are published: remote and
//! archive entries stay internal to Procyon rather than being downloaded
//! implicitly. Publishing never carries "cut" semantics; see the platform
//! adapters.

use std::collections::HashSet;
use std::sync::Arc;

use fm_domain::Location;
use fm_platform::{PlatformAdapter, PlatformCapabilities};
use fm_transport_dto::RuntimeKindDto;

use crate::ApplicationError;

/// Whether this host can share file references with the operating system.
pub(crate) fn is_supported(runtime: RuntimeKindDto, platform: &dyn PlatformAdapter) -> bool {
    runtime == RuntimeKindDto::Tauri
        && platform
            .capabilities()
            .contains(PlatformCapabilities::CLIPBOARD_FILE_REFERENCES)
}

/// Publishes `locations` on the system clipboard.
///
/// Returns `false` without touching the system clipboard when the host is
/// unsupported, the list is empty, or any entry is not a local file; Procyon's
/// own clipboard then remains the only copy of the selection.
pub(crate) async fn write(
    runtime: RuntimeKindDto,
    platform: &Arc<dyn PlatformAdapter>,
    locations: &[Location],
) -> Result<bool, ApplicationError> {
    if locations.is_empty() || !is_supported(runtime, platform.as_ref()) {
        return Ok(false);
    }
    let Ok(paths) = locations
        .iter()
        .map(Location::to_native_path)
        .collect::<Result<Vec<_>, _>>()
    else {
        return Ok(false);
    };
    let platform = Arc::clone(platform);
    tokio::task::spawn_blocking(move || platform.write_clipboard_file_references(&paths))
        .await
        .map_err(|_| ApplicationError::Internal)?
        .map_err(crate::platform_mapping::platform_failure)?;
    Ok(true)
}

/// Reads the local files referenced on the system clipboard, in clipboard
/// order and without duplicates. An unsupported host reads as empty.
pub(crate) async fn read(
    runtime: RuntimeKindDto,
    platform: &Arc<dyn PlatformAdapter>,
) -> Result<Vec<Location>, ApplicationError> {
    if !is_supported(runtime, platform.as_ref()) {
        return Ok(Vec::new());
    }
    let platform = Arc::clone(platform);
    let paths = tokio::task::spawn_blocking(move || platform.read_clipboard_file_references())
        .await
        .map_err(|_| ApplicationError::Internal)?
        .map_err(crate::platform_mapping::platform_failure)?;
    let mut seen = HashSet::new();
    Ok(paths
        .iter()
        .filter_map(|path| Location::from_native_path(path).ok())
        .filter(|location| seen.insert(location.uri.clone()))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use fm_domain::ProviderId;
    use fm_platform::PlatformError;

    use super::*;

    #[derive(Default)]
    struct FakeClipboard {
        capable: bool,
        files: Mutex<Vec<PathBuf>>,
        fail: bool,
    }

    impl PlatformAdapter for FakeClipboard {
        fn capabilities(&self) -> PlatformCapabilities {
            if self.capable {
                PlatformCapabilities::CLIPBOARD_FILE_REFERENCES
            } else {
                PlatformCapabilities::empty()
            }
        }

        fn read_clipboard_file_references(&self) -> Result<Vec<PathBuf>, PlatformError> {
            Ok(self.files.lock().unwrap().clone())
        }

        fn write_clipboard_file_references(&self, paths: &[PathBuf]) -> Result<(), PlatformError> {
            if self.fail {
                return Err(PlatformError::Io {
                    message: "clipboard busy".to_owned(),
                });
            }
            *self.files.lock().unwrap() = paths.to_vec();
            Ok(())
        }
    }

    fn capable() -> Arc<FakeClipboard> {
        Arc::new(FakeClipboard {
            capable: true,
            ..FakeClipboard::default()
        })
    }

    fn local(path: &str) -> Location {
        Location::from_native_path(&std::env::temp_dir().join(path)).unwrap()
    }

    #[tokio::test]
    async fn local_files_are_published_and_read_back() {
        let fake = capable();
        let platform: Arc<dyn PlatformAdapter> = fake.clone();
        let locations = vec![local("a.txt"), local("b folder")];

        assert!(
            write(RuntimeKindDto::Tauri, &platform, &locations)
                .await
                .unwrap()
        );

        assert_eq!(
            read(RuntimeKindDto::Tauri, &platform).await.unwrap(),
            locations
        );
    }

    #[tokio::test]
    async fn a_selection_with_any_remote_entry_stays_internal() {
        let fake = capable();
        let platform: Arc<dyn PlatformAdapter> = fake.clone();
        let remote = Location::new(ProviderId::new("sftp"), "sftp://host/home/me/a.txt");

        let written = write(RuntimeKindDto::Tauri, &platform, &[local("a.txt"), remote])
            .await
            .unwrap();

        assert!(!written);
        assert!(fake.files.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_browser_host_never_touches_the_system_clipboard() {
        let fake = capable();
        *fake.files.lock().unwrap() = vec![std::env::temp_dir().join("x")];
        let platform: Arc<dyn PlatformAdapter> = fake.clone();

        assert!(!is_supported(
            RuntimeKindDto::BrowserServer,
            platform.as_ref()
        ));
        assert!(
            !write(RuntimeKindDto::BrowserServer, &platform, &[local("a.txt")])
                .await
                .unwrap()
        );
        assert!(
            read(RuntimeKindDto::BrowserServer, &platform)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(fake.files.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn an_adapter_without_the_capability_is_unsupported() {
        let platform: Arc<dyn PlatformAdapter> = Arc::new(FakeClipboard::default());

        assert!(
            !write(RuntimeKindDto::Tauri, &platform, &[local("a.txt")])
                .await
                .unwrap()
        );
        assert!(
            read(RuntimeKindDto::Tauri, &platform)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn reading_skips_relative_paths_and_duplicates() {
        let fake = capable();
        let absolute = std::env::temp_dir().join("same.txt");
        *fake.files.lock().unwrap() = vec![
            absolute.clone(),
            PathBuf::from("relative.txt"),
            absolute.clone(),
        ];
        let platform: Arc<dyn PlatformAdapter> = fake;

        assert_eq!(
            read(RuntimeKindDto::Tauri, &platform).await.unwrap(),
            vec![Location::from_native_path(&absolute).unwrap()]
        );
    }

    #[tokio::test]
    async fn a_platform_failure_is_reported() {
        let platform: Arc<dyn PlatformAdapter> = Arc::new(FakeClipboard {
            capable: true,
            fail: true,
            ..FakeClipboard::default()
        });

        let error = write(RuntimeKindDto::Tauri, &platform, &[local("a.txt")])
            .await
            .unwrap_err();

        assert!(
            matches!(error, ApplicationError::PlatformFailed(ref rejection) if rejection.message.contains("busy"))
        );
    }
}
