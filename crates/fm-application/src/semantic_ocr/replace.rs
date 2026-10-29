//! Host-side verification and replacement of an original PDF with its
//! OCRmyPDF-processed version (task 0286).
//!
//! The semantic worker only ever *retains* OCR output in a host-private
//! directory, named by the SHA-256 of the exact bytes it OCR'd. This module is
//! the trusted host half: it verifies that output against the source on disk,
//! moves the original to the system trash, and puts the OCR'd file at the
//! original path. Every failed check leaves the original untouched and is
//! reported as a typed [`OcrReplacementSkip`].

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use fm_domain::Location;
use fm_platform::{PlatformAdapter, PlatformCapabilities};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Result of the replace-original step for one successfully OCR'd file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OcrReplacementOutcome {
    /// The original is in the trash and the OCR'd file is at its path.
    Replaced,
    /// The original was left untouched.
    Kept {
        /// Why the original was not replaced.
        reason: OcrReplacementSkip,
    },
}

/// Typed reason the original PDF was kept instead of replaced.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
pub enum OcrReplacementSkip {
    /// Only local files are replaced; remote providers and archive members
    /// keep using the temporary OCR copy for the index only.
    #[error("only files on a local disk are replaced")]
    NotLocal,
    /// The file or its path is not a plain regular file.
    #[error("the original is not a regular file")]
    NotRegularFile,
    /// The original is read-only.
    #[error("the original is read-only")]
    ReadOnly,
    /// The original could not be read for verification.
    #[error("the original could not be read: {detail}")]
    SourceUnreadable {
        /// Sanitized I/O diagnostic.
        detail: String,
    },
    /// The original changed after it was read for OCR.
    #[error("the original changed after it was read")]
    SourceChanged,
    /// The worker did not retain output for exactly this content.
    #[error("no OCR output was retained for the current content of the original")]
    RetainedOutputMissing,
    /// The retained output is not a readable PDF.
    #[error("the OCR output is not a readable PDF: {detail}")]
    OutputUnreadable {
        /// Sanitized parser diagnostic.
        detail: String,
    },
    /// The OCR output has a different number of pages.
    #[error("the OCR output has {output_pages} pages but the original has {original_pages}")]
    PageCountMismatch {
        /// Pages in the original.
        original_pages: u32,
        /// Pages in the OCR output.
        output_pages: u32,
    },
    /// This host has no system trash, so the original cannot be preserved.
    #[error("this system has no trash to preserve the original in")]
    TrashUnavailable,
    /// Moving the original to the trash failed.
    #[error("the original could not be moved to the trash: {detail}")]
    TrashFailed {
        /// Sanitized platform diagnostic.
        detail: String,
    },
    /// The OCR output could not be staged or put in place.
    #[error("the OCR output could not be put in place: {detail}")]
    ReplaceFailed {
        /// Sanitized I/O diagnostic, including recovery state.
        detail: String,
    },
}

/// Failure moving an original to the system trash.
#[derive(Debug, Error)]
pub(crate) enum OcrTrashError {
    /// The host has no trash capability.
    #[error("trash is unavailable")]
    Unavailable,
    /// The platform refused or failed the move.
    #[error("{0}")]
    Failed(String),
}

/// The system trash used to preserve replaced originals.
pub(crate) trait OcrOriginalTrash: Send + Sync {
    /// Moves `path` to the trash, returning where it can be restored from.
    fn trash(&self, path: &Path) -> Result<Option<PathBuf>, OcrTrashError>;

    /// Puts a just-trashed original back at its vacant `original` path.
    fn restore(&self, trashed: &Path, original: &Path) -> Result<(), String>;
}

/// Trash backed by the host's [`PlatformAdapter`].
pub(crate) struct PlatformOriginalTrash(pub(crate) Arc<dyn PlatformAdapter>);

impl OcrOriginalTrash for PlatformOriginalTrash {
    fn trash(&self, path: &Path) -> Result<Option<PathBuf>, OcrTrashError> {
        if !self.0.capabilities().contains(PlatformCapabilities::TRASH) {
            return Err(OcrTrashError::Unavailable);
        }
        self.0
            .trash_with_restore_location(path)
            .map_err(|error| OcrTrashError::Failed(error.to_string()))
    }

    fn restore(&self, trashed: &Path, original: &Path) -> Result<(), String> {
        self.0
            .restore_from_trash(trashed, original)
            .map_err(|error| error.to_string())
    }
}

/// Native path of a replaceable original, or why it is not replaceable.
pub(crate) fn local_original_path(location: &Location) -> Result<PathBuf, OcrReplacementSkip> {
    if location.provider_id.as_str() != "local" {
        return Err(OcrReplacementSkip::NotLocal);
    }
    location
        .to_native_path()
        .map_err(|_| OcrReplacementSkip::NotLocal)
}

/// File state captured before the worker reads the original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceSnapshot {
    length: u64,
    modified: Option<SystemTime>,
}

impl SourceSnapshot {
    /// Captures a plain, writable, regular file's size and modification time.
    pub(crate) fn capture(path: &Path) -> Result<Self, OcrReplacementSkip> {
        let metadata =
            fs::symlink_metadata(path).map_err(|error| OcrReplacementSkip::SourceUnreadable {
                detail: error.to_string(),
            })?;
        if !metadata.file_type().is_file() {
            return Err(OcrReplacementSkip::NotRegularFile);
        }
        if metadata.permissions().readonly() {
            return Err(OcrReplacementSkip::ReadOnly);
        }
        Ok(Self::of(&metadata))
    }

    fn of(metadata: &fs::Metadata) -> Self {
        Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }
}

/// Verifies retained OCR output and swaps it in for the original.
pub(crate) struct OcrOriginalReplacer {
    retention_directory: PathBuf,
    trash: Arc<dyn OcrOriginalTrash>,
}

/// Removes the retained output on every exit path: it is single-use.
struct RetainedOutput(PathBuf);

impl Drop for RetainedOutput {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl OcrOriginalReplacer {
    pub(crate) fn new(retention_directory: PathBuf, trash: Arc<dyn OcrOriginalTrash>) -> Self {
        Self {
            retention_directory,
            trash,
        }
    }

    /// Replaces `original` with its retained OCR output when every check holds.
    pub(crate) fn replace(
        &self,
        original: &Path,
        snapshot: SourceSnapshot,
    ) -> OcrReplacementOutcome {
        match self.try_replace(original, snapshot) {
            Ok(()) => OcrReplacementOutcome::Replaced,
            Err(reason) => OcrReplacementOutcome::Kept { reason },
        }
    }

    fn try_replace(
        &self,
        original: &Path,
        snapshot: SourceSnapshot,
    ) -> Result<(), OcrReplacementSkip> {
        let current = fs::symlink_metadata(original).map_err(unreadable)?;
        if !current.file_type().is_file() {
            return Err(OcrReplacementSkip::NotRegularFile);
        }
        if SourceSnapshot::of(&current) != snapshot {
            return Err(OcrReplacementSkip::SourceChanged);
        }
        let source = fs::read(original).map_err(unreadable)?;
        let retained = RetainedOutput(fm_semantic_docling::retained_ocr_output_path(
            &self.retention_directory,
            &fm_semantic_docling::ocr_source_digest(&source),
        ));
        let output = match fs::read(&retained.0) {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(OcrReplacementSkip::RetainedOutputMissing);
            }
            Err(error) => return Err(unreadable(error)),
        };

        let source_pages = page_count(&source)
            .map_err(|detail| OcrReplacementSkip::SourceUnreadable { detail })?;
        let output_pages = page_count(&output)
            .map_err(|detail| OcrReplacementSkip::OutputUnreadable { detail })?;
        if source_pages != output_pages {
            return Err(OcrReplacementSkip::PageCountMismatch {
                original_pages: source_pages,
                output_pages,
            });
        }

        let parent = original
            .parent()
            .ok_or(OcrReplacementSkip::NotRegularFile)?;
        let staged = stage(parent, &output, &current)?;

        // Last check immediately before the swap: nothing may have touched
        // the original since it was verified.
        let latest = fs::symlink_metadata(original).map_err(unreadable)?;
        if SourceSnapshot::of(&latest) != snapshot {
            return Err(OcrReplacementSkip::SourceChanged);
        }
        let restore_from = match self.trash.trash(original) {
            Ok(location) => location,
            Err(OcrTrashError::Unavailable) => return Err(OcrReplacementSkip::TrashUnavailable),
            Err(OcrTrashError::Failed(detail)) => {
                return Err(OcrReplacementSkip::TrashFailed { detail });
            }
        };
        if let Err(error) = staged.persist(original) {
            return Err(self.recover(original, restore_from.as_deref(), &error.error));
        }
        Ok(())
    }

    /// The swap failed after the original was trashed: put it back.
    fn recover(
        &self,
        original: &Path,
        trashed: Option<&Path>,
        cause: &std::io::Error,
    ) -> OcrReplacementSkip {
        let detail = match trashed {
            Some(trashed) => match self.trash.restore(trashed, original) {
                Ok(()) => format!("{cause}; the original was restored"),
                Err(restore) => format!(
                    "{cause}; restoring the original from {} failed: {restore}",
                    trashed.display()
                ),
            },
            None => format!("{cause}; the original remains in the trash"),
        };
        OcrReplacementSkip::ReplaceFailed { detail }
    }
}

fn unreadable(error: std::io::Error) -> OcrReplacementSkip {
    OcrReplacementSkip::SourceUnreadable {
        detail: error.to_string(),
    }
}

fn page_count(bytes: &[u8]) -> Result<u32, String> {
    let document = lopdf::Document::load_mem(bytes).map_err(|error| error.to_string())?;
    u32::try_from(document.get_pages().len()).map_err(|error| error.to_string())
}

/// Writes the OCR output next to the original (same volume, so the final
/// rename is atomic), carrying the original's permissions.
fn stage(
    parent: &Path,
    output: &[u8],
    original: &fs::Metadata,
) -> Result<tempfile::TempPath, OcrReplacementSkip> {
    let failed = |error: std::io::Error| OcrReplacementSkip::ReplaceFailed {
        detail: error.to_string(),
    };
    let mut staged = tempfile::Builder::new()
        .prefix(".procyon-ocr-")
        .suffix(".pdf")
        .tempfile_in(parent)
        .map_err(failed)?;
    staged.write_all(output).map_err(failed)?;
    staged.flush().map_err(failed)?;
    staged
        .as_file()
        .set_permissions(original.permissions())
        .map_err(failed)?;
    staged.as_file().sync_all().map_err(failed)?;
    Ok(staged.into_temp_path())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use lopdf::{Document, Object, Stream, dictionary};

    use super::*;

    /// Temp roots live under the project `target/`; nothing here ever
    /// touches a real user file.
    fn temp_root() -> tempfile::TempDir {
        let parent =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/application-ocr-tests");
        fs::create_dir_all(&parent).unwrap();
        tempfile::Builder::new()
            .prefix("ocr-replace-")
            .tempdir_in(fs::canonicalize(parent).unwrap())
            .unwrap()
    }

    fn pdf(pages: usize, marker: &str) -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let mut kids = Vec::new();
        for page in 0..pages {
            let content = document.add_object(Stream::new(
                dictionary! {},
                format!("% {marker} {page}").into_bytes(),
            ));
            let id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            });
            kids.push(Object::Reference(id));
        }
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Count" => i64::try_from(pages).unwrap(),
                "Kids" => kids,
            }),
        );
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    #[derive(Clone, Copy)]
    enum TrashMode {
        Works,
        Unavailable,
        Fails,
    }

    /// A trash that moves files into a folder inside the temp root.
    struct FolderTrash {
        folder: PathBuf,
        mode: TrashMode,
        trashed: Mutex<Vec<PathBuf>>,
    }

    impl FolderTrash {
        fn new(folder: PathBuf, mode: TrashMode) -> Arc<Self> {
            fs::create_dir_all(&folder).unwrap();
            Arc::new(Self {
                folder,
                mode,
                trashed: Mutex::new(Vec::new()),
            })
        }

        fn items(&self) -> Vec<PathBuf> {
            fs::read_dir(&self.folder)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect()
        }
    }

    impl OcrOriginalTrash for FolderTrash {
        fn trash(&self, path: &Path) -> Result<Option<PathBuf>, OcrTrashError> {
            match self.mode {
                TrashMode::Unavailable => Err(OcrTrashError::Unavailable),
                TrashMode::Fails => Err(OcrTrashError::Failed("denied".into())),
                TrashMode::Works => {
                    let destination = self.folder.join(path.file_name().unwrap());
                    fs::rename(path, &destination)
                        .map_err(|error| OcrTrashError::Failed(error.to_string()))?;
                    self.trashed.lock().unwrap().push(destination.clone());
                    Ok(Some(destination))
                }
            }
        }

        fn restore(&self, trashed: &Path, original: &Path) -> Result<(), String> {
            fs::rename(trashed, original).map_err(|error| error.to_string())
        }
    }

    struct Fixture {
        root: tempfile::TempDir,
        original: PathBuf,
        source: Vec<u8>,
        trash: Arc<FolderTrash>,
        replacer: OcrOriginalReplacer,
    }

    impl Fixture {
        fn new(mode: TrashMode) -> Self {
            let root = temp_root();
            let documents = root.path().join("documents");
            let retention = root.path().join("retention");
            fs::create_dir_all(&documents).unwrap();
            fs::create_dir_all(&retention).unwrap();
            let source = pdf(2, "scan");
            let original = documents.join("scan.pdf");
            fs::write(&original, &source).unwrap();
            let trash = FolderTrash::new(root.path().join("trash"), mode);
            let replacer = OcrOriginalReplacer::new(retention, trash.clone());
            Self {
                root,
                original,
                source,
                trash,
                replacer,
            }
        }

        fn retain(&self, output: &[u8]) {
            fs::write(
                fm_semantic_docling::retained_ocr_output_path(
                    &self.root.path().join("retention"),
                    &fm_semantic_docling::ocr_source_digest(&self.source),
                ),
                output,
            )
            .unwrap();
        }

        fn retained_files(&self) -> usize {
            fs::read_dir(self.root.path().join("retention"))
                .unwrap()
                .count()
        }

        fn documents(&self) -> Vec<PathBuf> {
            fs::read_dir(self.original.parent().unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect()
        }

        fn assert_original_untouched(&self) {
            assert_eq!(fs::read(&self.original).unwrap(), self.source);
            assert_eq!(
                self.documents(),
                vec![self.original.clone()],
                "no stray files"
            );
            assert!(self.trash.items().is_empty(), "nothing was trashed");
        }
    }

    fn kept(outcome: OcrReplacementOutcome) -> OcrReplacementSkip {
        match outcome {
            OcrReplacementOutcome::Kept { reason } => reason,
            OcrReplacementOutcome::Replaced => panic!("expected the original to be kept"),
        }
    }

    #[test]
    fn verified_output_replaces_the_original_and_trashes_it() {
        let fixture = Fixture::new(TrashMode::Works);
        let ocr = pdf(2, "ocr-layer");
        fixture.retain(&ocr);
        let snapshot = SourceSnapshot::capture(&fixture.original).unwrap();

        let outcome = fixture.replacer.replace(&fixture.original, snapshot);

        assert_eq!(outcome, OcrReplacementOutcome::Replaced);
        assert_eq!(fs::read(&fixture.original).unwrap(), ocr);
        let trashed = fixture.trash.items();
        assert_eq!(trashed.len(), 1);
        assert_eq!(fs::read(&trashed[0]).unwrap(), fixture.source);
        assert_eq!(fixture.documents(), vec![fixture.original.clone()]);
        assert_eq!(fixture.retained_files(), 0, "retained output is single-use");
    }

    #[test]
    fn page_count_mismatch_keeps_the_original() {
        let fixture = Fixture::new(TrashMode::Works);
        fixture.retain(&pdf(3, "ocr-layer"));
        let snapshot = SourceSnapshot::capture(&fixture.original).unwrap();

        let reason = kept(fixture.replacer.replace(&fixture.original, snapshot));

        assert_eq!(
            reason,
            OcrReplacementSkip::PageCountMismatch {
                original_pages: 2,
                output_pages: 3
            }
        );
        fixture.assert_original_untouched();
        assert_eq!(fixture.retained_files(), 0);
    }

    #[test]
    fn unreadable_output_keeps_the_original() {
        let fixture = Fixture::new(TrashMode::Works);
        fixture.retain(b"this is not a pdf");
        let snapshot = SourceSnapshot::capture(&fixture.original).unwrap();

        let reason = kept(fixture.replacer.replace(&fixture.original, snapshot));

        assert!(matches!(
            reason,
            OcrReplacementSkip::OutputUnreadable { .. }
        ));
        fixture.assert_original_untouched();
    }

    #[test]
    fn missing_retained_output_keeps_the_original() {
        let fixture = Fixture::new(TrashMode::Works);
        let snapshot = SourceSnapshot::capture(&fixture.original).unwrap();

        let reason = kept(fixture.replacer.replace(&fixture.original, snapshot));

        assert_eq!(reason, OcrReplacementSkip::RetainedOutputMissing);
        fixture.assert_original_untouched();
    }

    #[test]
    fn output_retained_for_different_content_is_never_applied() {
        let fixture = Fixture::new(TrashMode::Works);
        // Same size and mtime, different bytes: only the content digest can tell.
        let snapshot = SourceSnapshot::capture(&fixture.original).unwrap();
        fixture.retain(&pdf(2, "ocr-layer"));
        let mut altered = fixture.source.clone();
        let last = altered.len() - 1;
        altered[last] ^= 0x01;
        let modified = fs::metadata(&fixture.original).unwrap().modified().unwrap();
        fs::write(&fixture.original, &altered).unwrap();
        fs::File::options()
            .write(true)
            .open(&fixture.original)
            .unwrap()
            .set_modified(modified)
            .unwrap();

        let reason = kept(fixture.replacer.replace(&fixture.original, snapshot));

        assert_eq!(reason, OcrReplacementSkip::RetainedOutputMissing);
        assert_eq!(fs::read(&fixture.original).unwrap(), altered);
        assert!(fixture.trash.items().is_empty());
    }

    #[test]
    fn a_modified_original_is_not_replaced() {
        let fixture = Fixture::new(TrashMode::Works);
        fixture.retain(&pdf(2, "ocr-layer"));
        let snapshot = SourceSnapshot::capture(&fixture.original).unwrap();
        let edited = pdf(2, "edited while OCR ran");
        fs::write(&fixture.original, &edited).unwrap();

        let reason = kept(fixture.replacer.replace(&fixture.original, snapshot));

        assert_eq!(reason, OcrReplacementSkip::SourceChanged);
        assert_eq!(fs::read(&fixture.original).unwrap(), edited);
        assert!(fixture.trash.items().is_empty());
    }

    #[test]
    fn a_touched_original_is_not_replaced() {
        let fixture = Fixture::new(TrashMode::Works);
        fixture.retain(&pdf(2, "ocr-layer"));
        let snapshot = SourceSnapshot::capture(&fixture.original).unwrap();
        let later = SystemTime::now() + std::time::Duration::from_secs(120);
        fs::File::options()
            .write(true)
            .open(&fixture.original)
            .unwrap()
            .set_modified(later)
            .unwrap();

        let reason = kept(fixture.replacer.replace(&fixture.original, snapshot));

        assert_eq!(reason, OcrReplacementSkip::SourceChanged);
        assert!(fixture.trash.items().is_empty());
    }

    #[test]
    fn a_read_only_original_is_never_captured_for_replacement() {
        let fixture = Fixture::new(TrashMode::Works);
        let mut permissions = fs::metadata(&fixture.original).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&fixture.original, permissions).unwrap();

        let result = SourceSnapshot::capture(&fixture.original);

        assert_eq!(result.unwrap_err(), OcrReplacementSkip::ReadOnly);
        // Restore so the temp root can be removed on every platform.
        let mut permissions = fs::metadata(&fixture.original).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&fixture.original, permissions).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_not_a_regular_file() {
        let fixture = Fixture::new(TrashMode::Works);
        let link = fixture.root.path().join("documents").join("link.pdf");
        std::os::unix::fs::symlink(&fixture.original, &link).unwrap();

        assert_eq!(
            SourceSnapshot::capture(&link).unwrap_err(),
            OcrReplacementSkip::NotRegularFile
        );
    }

    #[test]
    fn a_trash_failure_keeps_the_original_and_removes_the_staged_copy() {
        let fixture = Fixture::new(TrashMode::Fails);
        fixture.retain(&pdf(2, "ocr-layer"));
        let snapshot = SourceSnapshot::capture(&fixture.original).unwrap();

        let reason = kept(fixture.replacer.replace(&fixture.original, snapshot));

        assert_eq!(
            reason,
            OcrReplacementSkip::TrashFailed {
                detail: "denied".into()
            }
        );
        fixture.assert_original_untouched();
        assert_eq!(fixture.retained_files(), 0);
    }

    #[test]
    fn a_host_without_trash_keeps_the_original() {
        let fixture = Fixture::new(TrashMode::Unavailable);
        fixture.retain(&pdf(2, "ocr-layer"));
        let snapshot = SourceSnapshot::capture(&fixture.original).unwrap();

        let reason = kept(fixture.replacer.replace(&fixture.original, snapshot));

        assert_eq!(reason, OcrReplacementSkip::TrashUnavailable);
        fixture.assert_original_untouched();
    }

    #[test]
    fn only_local_provider_locations_resolve_to_a_replaceable_path() {
        let root = temp_root();
        let local = Location::from_native_path(&root.path().join("a.pdf")).unwrap();
        assert_eq!(
            local_original_path(&local).unwrap(),
            root.path().join("a.pdf")
        );

        let remote = Location::parse("sftp://host/docs/a.pdf").unwrap();
        assert_eq!(
            local_original_path(&remote).unwrap_err(),
            OcrReplacementSkip::NotLocal
        );
    }
}
