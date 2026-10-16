//! General-pasteboard file references, so Procyon copies paste in Finder and
//! Finder copies paste in Procyon.
//!
//! Entries are written as `public.file-url` objects via `-writeObjects:`, the
//! same representation Finder itself produces. macOS has no "cut" flag on the
//! pasteboard, so a Procyon cut is exposed to other applications as a plain
//! copy; move semantics stay inside Procyon's own clipboard.

use std::path::{Path, PathBuf};

use fm_platform::PlatformError;
use objc2::ClassType;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{NSPasteboard, NSPasteboardURLReadingFileURLsOnlyKey, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};

/// Replaces `pasteboard`'s contents with file URLs for `paths`.
///
/// Each URL's directory flag comes from `symlink_metadata`, so a symbolic link
/// is published as the link itself rather than as the directory it points to.
pub(crate) fn write_file_references(
    pasteboard: &NSPasteboard,
    paths: &[PathBuf],
) -> Result<(), PlatformError> {
    if paths.is_empty() {
        return Err(PlatformError::Io {
            message: "at least one file is required".to_owned(),
        });
    }
    let entries = paths
        .iter()
        .map(|path| {
            let text = path.to_str().ok_or_else(|| PlatformError::Io {
                message: "file path is not valid UTF-8".to_owned(),
            })?;
            Ok((text, is_real_directory(path)))
        })
        .collect::<Result<Vec<_>, PlatformError>>()?;
    autoreleasepool(|_| {
        let urls = entries
            .iter()
            .map(|(path, is_directory)| {
                ProtocolObject::<dyn NSPasteboardWriting>::from_retained(
                    NSURL::fileURLWithPath_isDirectory(&NSString::from_str(path), *is_directory),
                )
            })
            .collect::<Vec<_>>();
        let urls = NSArray::from_retained_slice(&urls);
        pasteboard.clearContents();
        if pasteboard.writeObjects(&urls) {
            Ok(())
        } else {
            Err(PlatformError::Io {
                message: "the pasteboard rejected the file references".to_owned(),
            })
        }
    })
}

/// Reads the local file paths referenced on `pasteboard`, in pasteboard order.
///
/// Only file URLs are requested; Finder's file-reference URLs
/// (`file:///.file/id=...`) are resolved to ordinary paths. An empty result
/// means the pasteboard holds no file references.
pub(crate) fn read_file_references(pasteboard: &NSPasteboard) -> Vec<PathBuf> {
    autoreleasepool(|_| {
        let classes = NSArray::from_slice(&[NSURL::class()]);
        let file_urls_only = NSNumber::new_bool(true);
        let file_urls_only: &AnyObject = &file_urls_only;
        let options = NSDictionary::from_slices(
            &[unsafe { NSPasteboardURLReadingFileURLsOnlyKey }],
            &[file_urls_only],
        );
        let Some(objects) =
            (unsafe { pasteboard.readObjectsForClasses_options(&classes, Some(&options)) })
        else {
            return Vec::new();
        };
        objects
            .iter()
            .filter_map(|object| object.downcast::<NSURL>().ok())
            .filter(|url| url.isFileURL())
            .filter_map(|url| url.filePathURL())
            .filter_map(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()))
            .filter(|path| path.is_absolute())
            .collect()
    })
}

/// The application-wide general pasteboard Finder reads and writes.
pub(crate) fn general_pasteboard() -> Retained<NSPasteboard> {
    NSPasteboard::generalPasteboard()
}

fn is_real_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

/// A private, uniquely named pasteboard released when dropped, so tests never
/// clobber the user's clipboard.
#[cfg(test)]
pub(crate) struct ScratchPasteboard(pub(crate) Retained<NSPasteboard>);

#[cfg(test)]
impl ScratchPasteboard {
    pub(crate) fn new() -> Self {
        Self(NSPasteboard::pasteboardWithUniqueName())
    }
}

#[cfg(test)]
impl Drop for ScratchPasteboard {
    fn drop(&mut self) {
        let () = unsafe { objc2::msg_send![&*self.0, releaseGlobally] };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_file_references_read_back_as_the_same_paths() {
        let directory = tempfile::tempdir().expect("temp directory");
        let root = directory
            .path()
            .canonicalize()
            .expect("canonical temp root");
        let file = root.join("report with spaces & 100%.txt");
        let folder = root.join("Folder");
        std::fs::write(&file, b"x").expect("write file");
        std::fs::create_dir(&folder).expect("create folder");
        let pasteboard = ScratchPasteboard::new();

        write_file_references(&pasteboard.0, &[file.clone(), folder.clone()])
            .expect("write references");

        assert_eq!(read_file_references(&pasteboard.0), vec![file, folder]);
    }

    #[test]
    fn a_symlink_to_a_directory_is_published_as_the_link_itself() {
        let directory = tempfile::tempdir().expect("temp directory");
        let root = directory
            .path()
            .canonicalize()
            .expect("canonical temp root");
        let target = root.join("target");
        let link = root.join("link");
        std::fs::create_dir(&target).expect("create target");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");
        let pasteboard = ScratchPasteboard::new();

        write_file_references(&pasteboard.0, std::slice::from_ref(&link)).expect("write link");

        assert_eq!(read_file_references(&pasteboard.0), vec![link]);
    }

    #[test]
    fn a_pasteboard_holding_only_text_has_no_file_references() {
        let pasteboard = ScratchPasteboard::new();
        pasteboard.0.clearContents();
        let text = NSString::from_str("/etc/hosts");
        let objects = NSArray::from_retained_slice(&[
            ProtocolObject::<dyn NSPasteboardWriting>::from_retained(text),
        ]);
        assert!(pasteboard.0.writeObjects(&objects));

        assert!(read_file_references(&pasteboard.0).is_empty());
    }

    #[test]
    fn writing_nothing_is_rejected_without_clearing_the_pasteboard() {
        let pasteboard = ScratchPasteboard::new();
        let directory = tempfile::tempdir().expect("temp directory");
        let file = directory
            .path()
            .canonicalize()
            .expect("root")
            .join("kept.txt");
        std::fs::write(&file, b"x").expect("write file");
        write_file_references(&pasteboard.0, std::slice::from_ref(&file)).expect("write");

        assert!(write_file_references(&pasteboard.0, &[]).is_err());
        assert_eq!(read_file_references(&pasteboard.0), vec![file]);
    }
}
