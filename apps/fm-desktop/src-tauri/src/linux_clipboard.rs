//! Linux clipboard file references, best effort (X11 selections; Wayland
//! sessions work through XWayland only).
//!
//! Copied files are published as `text/uri-list` and GNOME's
//! `x-special/gnome-copied-files` (always marked `copy`, never `cut`, so another
//! application cannot delete Procyon's source files), plus the plain paths as
//! text. Reading accepts `text/uri-list`, which Nautilus, Dolphin and most other
//! file managers provide.

use std::path::PathBuf;

use fm_domain::Location;
use fm_platform::PlatformError;

/// Formats `paths` as percent-encoded `file://` URIs, one per line.
fn file_uris(paths: &[PathBuf]) -> Result<Vec<String>, PlatformError> {
    if paths.is_empty() {
        return Err(io("at least one file is required".to_owned()));
    }
    paths
        .iter()
        .map(|path| {
            Location::from_native_path(path)
                .map(|location| location.uri)
                .map_err(|error| io(format!("{}: {error}", path.display())))
        })
        .collect()
}

/// The `text/uri-list` body (RFC 2483: CRLF-separated).
pub(crate) fn uri_list(paths: &[PathBuf]) -> Result<String, PlatformError> {
    Ok(file_uris(paths)?
        .into_iter()
        .map(|uri| uri + "\r\n")
        .collect())
}

/// The `x-special/gnome-copied-files` body: an operation line, then URIs.
pub(crate) fn gnome_copied_files(paths: &[PathBuf]) -> Result<String, PlatformError> {
    Ok(std::iter::once("copy".to_owned())
        .chain(file_uris(paths)?)
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Parses a `text/uri-list` body into local paths, skipping comments, blank
/// lines and anything that is not a local `file:` URI.
pub(crate) fn parse_uri_list(body: &str) -> Vec<PathBuf> {
    body.lines()
        .map(|line| line.trim_end_matches('\r').trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| Location::parse(line).ok()?.to_native_path().ok())
        .collect()
}

fn io(message: String) -> PlatformError {
    PlatformError::Io { message }
}

#[cfg(target_os = "linux")]
pub(crate) use adapter::LinuxDesktopPlatformAdapter;

#[cfg(target_os = "linux")]
mod adapter {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext};
    use fm_platform::{PlatformAdapter, PlatformCapabilities, PlatformError};

    use super::{gnome_copied_files, io, parse_uri_list, uri_list};

    /// The Linux desktop adapter: clipboard file references only; every other
    /// integration keeps the default "unsupported" behaviour.
    ///
    /// The X11 clipboard context lives as long as the adapter because an X11
    /// selection is served by its owner: dropping it would empty the clipboard.
    #[derive(Default)]
    pub(crate) struct LinuxDesktopPlatformAdapter {
        clipboard: Mutex<Option<ClipboardContext>>,
    }

    impl LinuxDesktopPlatformAdapter {
        fn with_clipboard<T>(
            &self,
            action: impl FnOnce(&ClipboardContext) -> Result<T, PlatformError>,
        ) -> Result<T, PlatformError> {
            let mut slot = self
                .clipboard
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if slot.is_none() {
                *slot = Some(ClipboardContext::new().map_err(|error| {
                    io(format!("the system clipboard is unavailable: {error}"))
                })?);
            }
            action(slot.as_ref().expect("clipboard context initialised"))
        }
    }

    impl PlatformAdapter for LinuxDesktopPlatformAdapter {
        fn capabilities(&self) -> PlatformCapabilities {
            PlatformCapabilities::CLIPBOARD_FILE_REFERENCES
        }

        fn read_clipboard_file_references(&self) -> Result<Vec<PathBuf>, PlatformError> {
            self.with_clipboard(|clipboard| {
                // A missing `text/uri-list` target means "no files", not a failure.
                Ok(clipboard
                    .get_buffer("text/uri-list")
                    .map(|bytes| parse_uri_list(&String::from_utf8_lossy(&bytes)))
                    .unwrap_or_default())
            })
        }

        fn write_clipboard_file_references(&self, paths: &[PathBuf]) -> Result<(), PlatformError> {
            let uris = uri_list(paths)?;
            let gnome = gnome_copied_files(paths)?;
            let text = paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            self.with_clipboard(|clipboard| {
                clipboard
                    .set(vec![
                        ClipboardContent::Other("text/uri-list".to_owned(), uris.into_bytes()),
                        ClipboardContent::Other(
                            "x-special/gnome-copied-files".to_owned(),
                            gnome.into_bytes(),
                        ),
                        ClipboardContent::Text(text),
                    ])
                    .map_err(|error| io(format!("the clipboard rejected the files: {error}")))
            })
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn uri_list_percent_encodes_every_path_and_round_trips() {
        let paths = vec![
            PathBuf::from("/home/me/Report 100%.txt"),
            PathBuf::from("/home/me/Ünïcode#1"),
        ];

        let body = uri_list(&paths).expect("uri list");

        assert!(body.lines().all(|line| line.starts_with("file:///")));
        assert!(!body.contains(' '), "{body}");
        assert_eq!(parse_uri_list(&body), paths);
    }

    #[test]
    fn gnome_copied_files_is_always_a_copy() {
        let body = gnome_copied_files(&[PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")])
            .expect("gnome body");

        assert_eq!(body, "copy\nfile:///tmp/a\nfile:///tmp/b");
    }

    #[test]
    fn parsing_skips_comments_and_non_local_uris() {
        let body =
            "# comment\r\nfile:///tmp/a%20b\r\nsftp://host/tmp/c\r\n\r\nhttps://example.com/\n";

        assert_eq!(parse_uri_list(body), vec![PathBuf::from("/tmp/a b")]);
    }

    #[test]
    fn relative_paths_and_empty_selections_are_rejected() {
        assert!(uri_list(&[]).is_err());
        assert!(uri_list(&[PathBuf::from("relative/file")]).is_err());
    }
}
