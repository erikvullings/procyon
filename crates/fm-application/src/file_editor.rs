//! Whole-file editor service (task 0121).
//!
//! Provides load/save through a sibling temporary file and optimistic
//! revision-based conflict detection.

use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use fm_domain::{EntryId, EntryKind, Location};
use fm_transport_dto::{
    LoadEditableFileRequestDto, LoadEditableFileResponseDto, SaveEditableFileRequestDto,
    SaveEditableFileResponseDto,
};
use fm_vfs::{
    CopyCommitOptions, EntryRef, FileSystemProvider, ProviderCapabilities, ProviderRegistry,
    WriteOptions,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;

/// Whole-file editor ceiling. Large files remain on the ranged-viewer path.
pub(crate) const MAX_EDITABLE_FILE_BYTES: u64 = 3 * 1024 * 1024;

async fn local_save_target(location: &Location) -> Result<PathBuf, ApplicationError> {
    let path = location
        .to_native_path()
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    let parent = path
        .parent()
        .ok_or_else(|| ApplicationError::InvalidRequest("cannot edit a filesystem root".into()))?
        .to_path_buf();
    let name = path
        .file_name()
        .ok_or_else(|| ApplicationError::InvalidRequest("cannot edit a filesystem root".into()))?
        .to_os_string();
    tokio::task::spawn_blocking(move || {
        let canonical_parent = parent.canonicalize().map_err(local_lock_error)?;
        let target = canonical_parent.join(name);
        match target.canonicalize() {
            Ok(path) => Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(target),
            Err(error) => Err(local_lock_error(error)),
        }
    })
    .await
    .map_err(|error| ApplicationError::PlatformOperationFailed(error.to_string()))?
}

fn process_lock_for(target: &Path) -> Arc<AsyncMutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<AsyncMutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(lock) = locks.get(target).and_then(Weak::upgrade) {
        return lock;
    }
    locks.retain(|_, lock| lock.strong_count() > 0);
    let lock = Arc::new(AsyncMutex::new(()));
    locks.insert(target.to_path_buf(), Arc::downgrade(&lock));
    lock
}

async fn lock_local_save(target: PathBuf) -> Result<std::fs::File, ApplicationError> {
    tokio::task::spawn_blocking(move || {
        let key = Sha256::digest(target.to_string_lossy().as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let cache = dirs::cache_dir()
            .ok_or_else(|| {
                ApplicationError::PlatformOperationFailed(
                    "local editor lock cache unavailable".into(),
                )
            })?
            .join("procyon")
            .join("editor-locks");
        std::fs::create_dir_all(&cache).map_err(local_lock_error)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(cache.join(format!("{key}.lock")))
            .map_err(local_lock_error)?;
        fs2::FileExt::lock_exclusive(&file).map_err(local_lock_error)?;
        Ok(file)
    })
    .await
    .map_err(|error| ApplicationError::PlatformOperationFailed(error.to_string()))?
}

fn local_lock_error(error: std::io::Error) -> ApplicationError {
    ApplicationError::PlatformOperationFailed(format!("local editor lock: {error}"))
}

pub(crate) struct FileEditorService {
    providers: ProviderRegistry,
    audit_log_path: PathBuf,
}

struct TemporaryCopy {
    provider: Arc<dyn FileSystemProvider>,
    location: Location,
    cancellation: CancellationToken,
    pending: bool,
}

impl TemporaryCopy {
    fn new(
        provider: Arc<dyn FileSystemProvider>,
        location: Location,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            provider,
            location,
            cancellation,
            pending: true,
        }
    }

    async fn discard(&mut self) {
        self.cancellation.cancel();
        match self
            .provider
            .discard_copy(&self.location, CancellationToken::new())
            .await
        {
            Ok(()) => self.pending = false,
            Err(error) => {
                tracing::warn!(%error, uri = %self.location.uri, "could not discard editor temporary file")
            }
        }
    }
}

impl Drop for TemporaryCopy {
    fn drop(&mut self) {
        if !self.pending {
            return;
        }
        self.cancellation.cancel();
        let provider = Arc::clone(&self.provider);
        let location = self.location.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    if let Err(error) = provider
                        .discard_copy(&location, CancellationToken::new())
                        .await
                    {
                        tracing::warn!(%error, uri = %location.uri, "could not discard cancelled editor temporary file");
                    }
                });
            }
            Err(error) => {
                tracing::error!(%error, uri = %location.uri, "no runtime to discard editor temporary file");
            }
        }
    }
}

impl FileEditorService {
    pub(crate) fn new(providers: ProviderRegistry, audit_log_path: PathBuf) -> Self {
        Self {
            providers,
            audit_log_path,
        }
    }

    /// Loads a complete text file only when it fits the bounded editor budget.
    pub(crate) async fn load(
        &self,
        request: LoadEditableFileRequestDto,
    ) -> Result<LoadEditableFileResponseDto, ApplicationError> {
        let location: Location = request.location.into();
        let provider = self
            .providers
            .resolve(&location)
            .map_err(ApplicationError::from)?;
        let capabilities = provider
            .capabilities_for(&location)
            .map_err(ApplicationError::from)?;
        capabilities
            .require(ProviderCapabilities::READ)
            .map_err(ApplicationError::from)?;
        let entry = EntryRef {
            id: EntryId::new(),
            location,
        };
        let cancellation = CancellationToken::new();
        let summary = provider
            .inspect(&entry, cancellation.clone())
            .await
            .map_err(ApplicationError::from)?;
        if summary.kind != EntryKind::File {
            return Err(ApplicationError::InvalidRequest(
                "only regular files can be edited".to_owned(),
            ));
        }
        let size = provider
            .file_size(&entry, cancellation.clone())
            .await
            .map_err(ApplicationError::from)?;
        if size > MAX_EDITABLE_FILE_BYTES {
            return Err(ApplicationError::InvalidRequest(format!(
                "file is too large for the editor ({size} bytes; limit is {MAX_EDITABLE_FILE_BYTES}); use the large-file viewer or external editor"
            )));
        }
        let mut reader = provider
            .open_read(&entry, cancellation)
            .await
            .map_err(ApplicationError::from)?;
        let mut bytes = Vec::with_capacity(size as usize);
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(read_stream_error)?;
        if fm_vfs::looks_like_binary(&bytes) {
            return Err(ApplicationError::InvalidRequest(
                "binary files cannot be edited in-app; use the external editor".to_owned(),
            ));
        }
        let content = String::from_utf8(bytes.clone()).map_err(|_| {
            ApplicationError::InvalidRequest(
                "the file is not valid UTF-8; use the external editor".to_owned(),
            )
        })?;
        Ok(LoadEditableFileResponseDto {
            content,
            revision: content_revision(&bytes),
            size,
        })
    }

    /// Safely replaces editable content through a sibling temporary file and
    /// optimistic revision check.
    pub(crate) async fn save(
        &self,
        request: SaveEditableFileRequestDto,
    ) -> Result<SaveEditableFileResponseDto, ApplicationError> {
        let location: Location = request.location.into();
        let destination: Option<Location> = request.destination.map(Into::into);
        let provider = self
            .providers
            .resolve(&location)
            .map_err(ApplicationError::from)?;
        let capabilities = provider
            .capabilities_for(&location)
            .map_err(ApplicationError::from)?;
        capabilities
            .require(ProviderCapabilities::READ | ProviderCapabilities::WRITE)
            .map_err(ApplicationError::from)?;
        let local_target = if location.provider_id.as_str() == "local" {
            Some(local_save_target(&location).await?)
        } else {
            None
        };
        let process_lock = local_target.as_deref().map(process_lock_for);
        let _process_guard = if let Some(lock) = &process_lock {
            Some(lock.lock().await)
        } else {
            None
        };
        let _file_guard = if let Some(target) = local_target {
            Some(lock_local_save(target).await?)
        } else {
            None
        };
        let entry = EntryRef {
            id: EntryId::new(),
            location: location.clone(),
        };
        let cancellation = CancellationToken::new();
        let summary = provider
            .inspect(&entry, cancellation.clone())
            .await
            .map_err(ApplicationError::from)?;
        if summary.kind != EntryKind::File {
            return Err(ApplicationError::InvalidRequest(
                "symlinks and non-files cannot be replaced by the editor".to_owned(),
            ));
        }
        let existing_size = provider
            .file_size(&entry, cancellation.clone())
            .await
            .map_err(ApplicationError::from)?;
        if existing_size > MAX_EDITABLE_FILE_BYTES {
            return Err(ApplicationError::InvalidRequest(format!(
                "file is too large for the editor ({existing_size} bytes; limit is {MAX_EDITABLE_FILE_BYTES})"
            )));
        }
        let mut reader = provider
            .open_read(&entry, cancellation.clone())
            .await
            .map_err(ApplicationError::from)?;
        let mut existing = Vec::new();
        reader
            .read_to_end(&mut existing)
            .await
            .map_err(read_stream_error)?;
        let actual_revision = content_revision(&existing);
        let conflicted = actual_revision != request.expected_revision;
        if conflicted && !request.overwrite_conflict {
            return Err(ApplicationError::FileRevisionConflict {
                expected_revision: request.expected_revision,
                actual_revision,
            });
        }
        let bytes = request.content.into_bytes();
        if bytes.len() as u64 > MAX_EDITABLE_FILE_BYTES {
            return Err(ApplicationError::InvalidRequest(format!(
                "edited content exceeds the {MAX_EDITABLE_FILE_BYTES}-byte limit"
            )));
        }
        let save_location = destination.as_ref().unwrap_or(&location);
        let parent = save_location
            .parent()
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?
            .ok_or_else(|| {
                ApplicationError::InvalidRequest("cannot edit a filesystem root".to_owned())
            })?;
        let temporary = parent
            .join(&format!(".fm-edit-{}.tmp", Uuid::new_v4()))
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        let mut temporary_copy = TemporaryCopy::new(
            Arc::clone(&provider),
            temporary.clone(),
            cancellation.clone(),
        );
        let mut writer = match provider
            .open_write(
                &temporary,
                WriteOptions { overwrite: false },
                cancellation.clone(),
            )
            .await
        {
            Ok(writer) => writer,
            Err(error) => {
                if matches!(error, fm_vfs::VfsError::AlreadyExists { .. }) {
                    temporary_copy.pending = false;
                } else {
                    temporary_copy.discard().await;
                }
                return Err(ApplicationError::from(error));
            }
        };
        if let Err(error) = writer.write_all(&bytes).await {
            drop(writer);
            temporary_copy.discard().await;
            return Err(read_stream_error(error));
        }
        if let Err(error) = writer.shutdown().await {
            drop(writer);
            temporary_copy.discard().await;
            return Err(read_stream_error(error));
        }
        drop(writer);
        if let Err(error) = provider
            .commit_copy(
                &entry,
                &temporary,
                save_location,
                CopyCommitOptions {
                    overwrite: destination.is_none(),
                    preserve_metadata: true,
                },
                cancellation,
            )
            .await
        {
            temporary_copy.discard().await;
            return Err(ApplicationError::from(error));
        }
        temporary_copy.pending = false;
        if conflicted {
            if let Some(parent) = self.audit_log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.audit_log_path)
                .and_then(|mut file| {
                    writeln!(
                        file,
                        "explicit editable-file overwrite uri={}",
                        save_location.uri
                    )
                });
        }
        Ok(SaveEditableFileResponseDto {
            revision: content_revision(&bytes),
            size: bytes.len() as u64,
            overwrote_conflict: conflicted,
        })
    }
}

fn content_revision(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(crate) fn read_stream_error(error: std::io::Error) -> ApplicationError {
    ApplicationError::from(fm_vfs::VfsError::Io {
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_vfs_local::LocalFileSystemProvider;

    fn editor(root: &tempfile::TempDir) -> FileEditorService {
        let mut providers = ProviderRegistry::new();
        providers.register(std::sync::Arc::new(LocalFileSystemProvider));
        FileEditorService::new(providers, root.path().join("audit.jsonl"))
    }

    fn location_dto_for(path: &std::path::Path) -> fm_transport_dto::LocationDto {
        Location::from_native_path(path)
            .expect("path must convert to a location")
            .into()
    }

    #[tokio::test]
    async fn load_rejects_binary_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("binary.bin");
        std::fs::write(&target, [1, 0, 2]).expect("write binary fixture");

        let error = editor(&dir)
            .load(LoadEditableFileRequestDto {
                location: location_dto_for(&target),
            })
            .await
            .expect_err("binary file must be rejected");

        assert!(matches!(error, ApplicationError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn load_rejects_oversized_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("large.txt");
        std::fs::File::create(&target)
            .expect("create large fixture")
            .set_len(MAX_EDITABLE_FILE_BYTES + 1)
            .expect("size fixture");

        let error = editor(&dir)
            .load(LoadEditableFileRequestDto {
                location: location_dto_for(&target),
            })
            .await
            .expect_err("oversized file must be rejected");

        assert!(matches!(error, ApplicationError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn load_returns_content_and_revision_for_text_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("note.txt");
        std::fs::write(&target, b"hello world").expect("write fixture");

        let result = editor(&dir)
            .load(LoadEditableFileRequestDto {
                location: location_dto_for(&target),
            })
            .await
            .expect("load must succeed");

        assert_eq!(result.content, "hello world");
        assert_eq!(result.size, 11);
        assert!(!result.revision.is_empty());
    }

    #[tokio::test]
    async fn save_detects_revision_conflict() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("note.txt");
        std::fs::write(&target, b"original").expect("write fixture");
        let location = location_dto_for(&target);

        let editor = editor(&dir);
        let loaded = editor
            .load(LoadEditableFileRequestDto {
                location: location.clone(),
            })
            .await
            .expect("load must succeed");

        // Simulate external edit.
        std::fs::write(&target, b"external change").expect("external edit");

        let error = editor
            .save(SaveEditableFileRequestDto {
                location,
                destination: None,
                content: "editor content".to_owned(),
                expected_revision: loaded.revision,
                overwrite_conflict: false,
            })
            .await
            .expect_err("revision conflict must be detected");

        assert!(
            matches!(error, ApplicationError::FileRevisionConflict { .. }),
            "expected FileRevisionConflict, got {error}"
        );
        // File must be unchanged.
        assert_eq!(
            std::fs::read_to_string(&target).expect("read target"),
            "external change"
        );
    }

    #[tokio::test]
    async fn save_performs_explicit_overwrite_with_audit() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("note.txt");
        std::fs::write(&target, b"one").expect("write fixture");
        let location = location_dto_for(&target);

        let editor = editor(&dir);
        let loaded = editor
            .load(LoadEditableFileRequestDto {
                location: location.clone(),
            })
            .await
            .expect("load");

        std::fs::write(&target, b"two").expect("external edit");

        let saved = editor
            .save(SaveEditableFileRequestDto {
                location,
                destination: None,
                content: "three".to_owned(),
                expected_revision: loaded.revision,
                overwrite_conflict: true,
            })
            .await
            .expect("explicit overwrite");

        assert!(saved.overwrote_conflict);
        assert_eq!(
            std::fs::read_to_string(&target).expect("read target"),
            "three"
        );
        let audit = std::fs::read_to_string(dir.path().join("audit.jsonl")).expect("audit log");
        assert!(audit.contains("note.txt"));
    }

    #[tokio::test]
    async fn save_as_creates_sibling_without_changing_source() {
        let dir = tempfile::tempdir().expect("temp dir");
        let source = dir.path().join("source.txt");
        let destination = dir.path().join("copy.txt");
        std::fs::write(&source, b"source content").expect("write fixture");
        let location = location_dto_for(&source);

        let editor = editor(&dir);
        let loaded = editor
            .load(LoadEditableFileRequestDto {
                location: location.clone(),
            })
            .await
            .expect("load");

        editor
            .save(SaveEditableFileRequestDto {
                location,
                destination: Some(location_dto_for(&destination)),
                content: "copy content".to_owned(),
                expected_revision: loaded.revision,
                overwrite_conflict: false,
            })
            .await
            .expect("save as");

        assert_eq!(
            std::fs::read_to_string(&source).expect("source"),
            "source content"
        );
        assert_eq!(
            std::fs::read_to_string(&destination).expect("destination"),
            "copy content"
        );
    }

    #[tokio::test]
    async fn local_save_of_another_file_does_not_wait_on_unrelated_editor_lock() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = dir.path().join("first.svg");
        let second = dir.path().join("second.svg");
        std::fs::write(&first, "<svg/>").expect("first fixture");
        std::fs::write(&second, "<svg/>").expect("second fixture");
        let first_location = Location::from_native_path(&first).expect("first location");
        let first_target = local_save_target(&first_location)
            .await
            .expect("canonical target");
        let held_lock = process_lock_for(&first_target);
        let _guard = held_lock.lock().await;

        let location = location_dto_for(&second);
        let editor = editor(&dir);
        let loaded = editor
            .load(LoadEditableFileRequestDto {
                location: location.clone(),
            })
            .await
            .expect("load second file");
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            editor.save(SaveEditableFileRequestDto {
                location,
                destination: None,
                content: "<svg id=\"second\"/>".into(),
                expected_revision: loaded.revision,
                overwrite_conflict: false,
            }),
        )
        .await
        .expect("unrelated save must not wait")
        .expect("save second file");
        assert_eq!(
            std::fs::read_to_string(&first).expect("first unchanged"),
            "<svg/>"
        );
        assert_eq!(
            std::fs::read_to_string(&second).expect("second updated"),
            "<svg id=\"second\"/>"
        );
    }

    #[tokio::test]
    async fn failed_save_as_discards_temporary_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let source = dir.path().join("source.txt");
        let destination = dir.path().join("existing.txt");
        std::fs::write(&source, b"source").expect("write source");
        std::fs::write(&destination, b"keep").expect("write destination");
        let editor = editor(&dir);
        let location = location_dto_for(&source);
        let loaded = editor
            .load(LoadEditableFileRequestDto {
                location: location.clone(),
            })
            .await
            .expect("load");

        editor
            .save(SaveEditableFileRequestDto {
                location,
                destination: Some(location_dto_for(&destination)),
                content: "replacement".to_owned(),
                expected_revision: loaded.revision,
                overwrite_conflict: false,
            })
            .await
            .expect_err("save as cannot replace existing destination");

        assert_eq!(
            std::fs::read_to_string(&destination).expect("destination"),
            "keep"
        );
        assert!(
            !dir.path()
                .read_dir()
                .expect("list directory")
                .any(|entry| entry
                    .expect("directory entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".fm-edit-"))
        );
    }

    #[tokio::test]
    async fn dropping_temporary_copy_discards_file_after_cancellation() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(".fm-edit-test.tmp");
        std::fs::write(&path, b"partial edit").expect("write temporary");
        let provider: Arc<dyn FileSystemProvider> = Arc::new(LocalFileSystemProvider);
        let location = Location::from_native_path(&path).expect("temporary location");
        let cancellation = CancellationToken::new();
        drop(TemporaryCopy::new(provider, location, cancellation.clone()));
        assert!(cancellation.is_cancelled());

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while path.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("temporary file must be discarded");
    }
}
