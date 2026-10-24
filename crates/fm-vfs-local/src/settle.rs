//! Waiting for a file that another process is still writing.
//!
//! Remote-desktop clients (e.g. Microsoft Remote Desktop on macOS) put pasted files on the
//! pasteboard as soon as the transfer starts: the files already exist at their final size but
//! are filled in over the following minutes. Copying such a file straight away - above all by
//! APFS clone, which shares the extents without reading them - captures the unfinished (zeroed)
//! state. Finder avoids this by waiting for the writer; here such a source is waited on until
//! its modification time has been quiet for [`QUIET_PERIOD`]. Only files inside another app's
//! sandbox container qualify ([`needs_settling`]): waiting for every recently saved file would
//! slow ordinary copies down.

use std::path::Path;
use std::time::{Duration, SystemTime};

use fm_vfs::VfsError;
use tokio_util::sync::CancellationToken;

/// How long a source must have gone unmodified before it is considered complete.
pub(crate) const QUIET_PERIOD: Duration = Duration::from_secs(3);

/// True for files inside an app sandbox container (`~/Library/Containers/<app>/...` or
/// `~/Library/Group Containers/...`), where apps such as remote-desktop clients stage files
/// they are still filling in.
pub(crate) fn needs_settling(path: &Path) -> bool {
    let mut previous_was_library = false;
    for component in path.components() {
        let name = component.as_os_str();
        if previous_was_library && (name == "Containers" || name == "Group Containers") {
            return true;
        }
        previous_was_library = name == "Library";
    }
    false
}

/// Returns once `path` has not been modified for `quiet`, or `Cancelled` if `cancellation`
/// fires first. Files untouched for longer than `quiet` (the normal case) return immediately.
pub(crate) async fn wait_until_quiet(
    path: &Path,
    quiet: Duration,
    cancellation: &CancellationToken,
) -> Result<(), VfsError> {
    loop {
        let modified = tokio::fs::metadata(path)
            .await
            .and_then(|metadata| metadata.modified())
            .map_err(|error| VfsError::Io {
                message: error.to_string(),
            })?;
        // A modification time in the future (clock skew) can never become "old": don't wait.
        let Ok(age) = SystemTime::now().duration_since(modified) else {
            return Ok(());
        };
        if age >= quiet {
            return Ok(());
        }
        let remaining = quiet - age + Duration::from_millis(50);
        tokio::select! {
            () = cancellation.cancelled() => return Err(VfsError::Cancelled),
            () = tokio::time::sleep(remaining) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn only_app_container_paths_need_settling() {
        let container =
            Path::new("/Users/me/Library/Containers/com.microsoft.rdc.macos/Data/tmp/x");
        assert!(needs_settling(container));
        assert!(needs_settling(Path::new(
            "/Users/me/Library/Group Containers/g/f"
        )));
        assert!(!needs_settling(Path::new(
            "/Users/me/Downloads/Containers/f"
        )));
        assert!(!needs_settling(Path::new(
            "/Users/me/Library/Application Support/f"
        )));
    }

    #[tokio::test]
    async fn an_old_file_does_not_wait() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old");
        std::fs::write(&path, "x").unwrap();
        let started = Instant::now();

        wait_until_quiet(&path, Duration::ZERO, &CancellationToken::new())
            .await
            .unwrap();

        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[tokio::test]
    async fn a_file_still_being_written_is_waited_on() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("growing");
        std::fs::write(&path, "x").unwrap();
        let started = Instant::now();

        wait_until_quiet(&path, Duration::from_millis(400), &CancellationToken::new())
            .await
            .unwrap();

        assert!(started.elapsed() >= Duration::from_millis(300));
    }

    #[tokio::test]
    async fn cancellation_ends_the_wait() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("growing");
        std::fs::write(&path, "x").unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let result = wait_until_quiet(&path, Duration::from_secs(60), &cancellation).await;

        assert!(matches!(result, Err(VfsError::Cancelled)));
    }
}
