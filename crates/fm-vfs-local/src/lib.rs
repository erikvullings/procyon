//! Local filesystem implementation of the virtual filesystem provider.
//!
//! Directory entries are inspected without following symbolic links. macOS
//! Finder aliases are not detected yet and are treated as regular files.
//! macOS application bundles (`.app`) are reported as [`EntryKind::File`]
//! rather than [`EntryKind::Directory`] (specification §23), so they behave
//! as a single opaque item in listings; the underlying path is still a real
//! directory and can be listed by navigating into it directly.

use std::{
    collections::BTreeMap,
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fm_domain::{
    EntryId, EntryKind, EntryMetadata, EntrySummary, Location, OwnershipInfo, PermissionsInfo,
    ProviderId,
};
use fm_vfs::{
    CopyCommitOptions, DirectoryPage, EntryRef, FileSystemProvider, ListOptions,
    ProviderCapabilities, ProviderChange, ProviderChangeStream, ProviderReadStream,
    ProviderWriteStream, RemoveOptions, VfsError, WriteOptions,
};
use futures::stream;
use notify::{Event, RecursiveMode, Watcher};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const WATCH_DEBOUNCE: Duration = Duration::from_millis(75);
const WATCH_INPUT_CAPACITY: usize = 64;

/// Provider for the host's local filesystem.
#[derive(Debug, Default)]
pub struct LocalFileSystemProvider;

impl LocalFileSystemProvider {
    /// Creates a local filesystem provider.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl FileSystemProvider for LocalFileSystemProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new("local")
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["file"]
    }

    fn validate_location(&self, location: &Location) -> Result<(), VfsError> {
        location
            .validate_local_uri()
            .map_err(|_| invalid_location(location))
    }

    fn capabilities(&self) -> ProviderCapabilities {
        let capabilities = ProviderCapabilities::LIST
            | ProviderCapabilities::WATCH
            | ProviderCapabilities::CREATE_DIRECTORY
            | ProviderCapabilities::RENAME
            | ProviderCapabilities::READ
            | ProviderCapabilities::WRITE
            | ProviderCapabilities::SERVER_SIDE_COPY
            | ProviderCapabilities::SET_TIMESTAMPS
            | ProviderCapabilities::SET_PERMISSIONS
            | ProviderCapabilities::RANDOM_ACCESS
            // Checksums are computed by streaming `open_read`, so any
            // provider that can read can checksum (task 0077, spec §6).
            | ProviderCapabilities::CHECKSUM;
        capabilities | ProviderCapabilities::MOVE | ProviderCapabilities::DELETE
    }

    async fn list(
        &self,
        location: &Location,
        options: ListOptions,
        cancellation: CancellationToken,
    ) -> Result<DirectoryPage, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        if options.page_size == 0 {
            return Err(VfsError::InvalidLocation {
                location: location.uri.clone(),
            });
        }
        let path = location
            .to_native_path()
            .map_err(|_| invalid_location(location))?;
        let offset = decode_token(options.continuation_token.as_deref(), location)?;
        let page_size = options.page_size;
        let owned_location = location.clone();
        tokio::task::spawn_blocking(move || {
            list_directory_page_sync(&path, &owned_location, offset, page_size)
        })
        .await
        .map_err(|_| VfsError::Cancelled)?
    }

    async fn metadata(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntryMetadata, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let path = entry
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&entry.location))?;
        let metadata = tokio::fs::symlink_metadata(path)
            .await
            .map_err(|error| map_io_error(error, &entry.location.uri))?;
        Ok(EntryMetadata {
            entry_id: entry.id,
            permissions: Some(permissions(&metadata)),
            ownership: ownership(&metadata),
            extended_attributes: BTreeMap::new(),
            checksums: BTreeMap::new(),
            image_dimensions: None,
            media: None,
            archive: None,
            plugin_fields: BTreeMap::new(),
        })
    }

    async fn inspect(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntrySummary, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let path = entry
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&entry.location))?;
        summarize_path(&path, &entry.location).await
    }

    async fn file_size(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<u64, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let path = entry
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&entry.location))?;
        let metadata = tokio::fs::symlink_metadata(path)
            .await
            .map_err(|error| map_io_error(error, &entry.location.uri))?;
        if !metadata.is_file() {
            return Err(VfsError::IsADirectory {
                location: entry.location.uri.clone(),
            });
        }
        Ok(metadata.len())
    }

    async fn create_directory(
        &self,
        location: &Location,
        name: &str,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        validate_directory_name(name)?;
        let parent = location
            .to_native_path()
            .map_err(|_| invalid_location(location))?;
        let path = parent.join(name);
        tokio::fs::create_dir(&path)
            .await
            .map_err(|error| map_io_error(error, &location.uri))?;
        let child = location
            .join(name)
            .map_err(|_| invalid_location(location))?;
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|error| map_io_error(error, &child.uri))?;
        Ok(EntryRef {
            id: stable_entry_id(&metadata, &child),
            location: child,
        })
    }

    async fn rename(
        &self,
        source: &EntryRef,
        destination: &Location,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let destination_name = destination
            .name()
            .map_err(|_| invalid_location(destination))?;
        validate_directory_name(&destination_name)?;
        let source_path = source
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&source.location))?;
        let destination_path = destination
            .to_native_path()
            .map_err(|_| invalid_location(destination))?;
        let case_only = source_path != destination_path
            && source_path
                .to_string_lossy()
                .eq_ignore_ascii_case(&destination_path.to_string_lossy());
        if destination_path.exists() && !case_only {
            return Err(VfsError::AlreadyExists {
                location: destination.uri.clone(),
            });
        }
        if case_only {
            let parent = source_path
                .parent()
                .ok_or_else(|| invalid_location(&source.location))?;
            let temporary = parent.join(format!(".fm-rename-{}", Uuid::new_v4()));
            tokio::fs::rename(&source_path, &temporary)
                .await
                .map_err(|error| map_io_error(error, &source.location.uri))?;
            if let Err(error) = tokio::fs::rename(&temporary, &destination_path).await {
                let _ = tokio::fs::rename(&temporary, &source_path).await;
                return Err(map_io_error(error, &destination.uri));
            }
        } else {
            tokio::fs::rename(&source_path, &destination_path)
                .await
                .map_err(|error| map_io_error(error, &destination.uri))?;
        }
        Ok(EntryRef {
            id: source.id,
            location: destination.clone(),
        })
    }

    async fn remove(
        &self,
        entry: &EntryRef,
        options: RemoveOptions,
        cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        if options.use_trash {
            return unsupported(ProviderCapabilities::TRASH);
        }
        let path = entry
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&entry.location))?;
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|error| map_io_error(error, &entry.location.uri))?;
        if metadata.file_type().is_symlink() || metadata.is_file() {
            tokio::fs::remove_file(path)
                .await
                .map_err(|error| map_io_error(error, &entry.location.uri))
        } else if options.recursive {
            tokio::fs::remove_dir_all(path)
                .await
                .map_err(|error| map_io_error(error, &entry.location.uri))
        } else {
            tokio::fs::remove_dir(path)
                .await
                .map_err(|error| map_io_error(error, &entry.location.uri))
        }
    }

    async fn open_read(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let path = entry
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&entry.location))?;
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|error| map_io_error(error, &entry.location.uri))?;
        Ok(Box::pin(file))
    }

    async fn read_range(
        &self,
        entry: &EntryRef,
        offset: u64,
        length: Option<u64>,
        cancellation: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let path = entry
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&entry.location))?;
        let mut file = tokio::fs::File::open(path)
            .await
            .map_err(|error| map_io_error(error, &entry.location.uri))?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|error| map_io_error(error, &entry.location.uri))?;
        Ok(match length {
            Some(length) => Box::pin(file.take(length)),
            None => Box::pin(file),
        })
    }

    async fn open_write(
        &self,
        destination: &Location,
        options: WriteOptions,
        cancellation: CancellationToken,
    ) -> Result<ProviderWriteStream, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let path = destination
            .to_native_path()
            .map_err(|_| invalid_location(destination))?;
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(options.overwrite)
            .create_new(!options.overwrite)
            .open(path)
            .await
            .map_err(|error| map_io_error(error, &destination.uri))?;
        Ok(Box::pin(file))
    }

    async fn commit_copy(
        &self,
        source: &EntryRef,
        temporary: &Location,
        destination: &Location,
        options: CopyCommitOptions,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let temporary_path = temporary
            .to_native_path()
            .map_err(|_| invalid_location(temporary))?;
        let destination_path = destination
            .to_native_path()
            .map_err(|_| invalid_location(destination))?;
        if options.preserve_metadata {
            let source_path = source
                .location
                .to_native_path()
                .map_err(|_| invalid_location(&source.location))?;
            preserve_copy_metadata(&source_path, &temporary_path, &source.location.uri).await?;
        }
        if options.overwrite {
            tokio::fs::rename(&temporary_path, &destination_path)
                .await
                .map_err(|error| map_io_error(error, &destination.uri))?;
        } else {
            tokio::fs::hard_link(&temporary_path, &destination_path)
                .await
                .map_err(|error| map_io_error(error, &destination.uri))?;
            tokio::fs::remove_file(&temporary_path)
                .await
                .map_err(|error| map_io_error(error, &temporary.uri))?;
        }
        let metadata = tokio::fs::symlink_metadata(&destination_path)
            .await
            .map_err(|error| map_io_error(error, &destination.uri))?;
        Ok(EntryRef {
            id: stable_entry_id(&metadata, destination),
            location: destination.clone(),
        })
    }

    async fn server_side_copy(
        &self,
        source: &EntryRef,
        temporary: &Location,
        cancellation: CancellationToken,
    ) -> Result<bool, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let source_path = source
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&source.location))?;
        let temporary_path = temporary
            .to_native_path()
            .map_err(|_| invalid_location(temporary))?;
        let source_metadata = tokio::fs::metadata(&source_path)
            .await
            .map_err(|error| map_io_error(error, &source.location.uri))?;
        if source_metadata.permissions().readonly() {
            return Ok(false);
        }
        #[cfg(target_os = "macos")]
        if source_metadata.len() >= 1024 * 1024 {
            let clone_source = source_path.clone();
            let clone_temporary = temporary_path.clone();
            let result = tokio::task::spawn_blocking(move || {
                std::process::Command::new("cp")
                    .arg("-c")
                    .arg(clone_source)
                    .arg(&clone_temporary)
                    .status()
                    .map(|status| (status.success(), clone_temporary))
            })
            .await
            .map_err(|error| VfsError::Io {
                message: error.to_string(),
            })?
            .map_err(|error| map_io_error(error, &temporary.uri))?;
            if !result.0 {
                let _ = tokio::fs::remove_file(result.1).await;
            } else {
                return Ok(true);
            }
        }
        tokio::fs::copy(source_path, temporary_path)
            .await
            .map_err(|error| map_io_error(error, &temporary.uri))?;
        Ok(true)
    }

    async fn discard_copy(
        &self,
        temporary: &Location,
        _cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        let path = temporary
            .to_native_path()
            .map_err(|_| invalid_location(temporary))?;
        match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(map_io_error(error, &temporary.uri)),
        }
    }

    async fn copy_symlink(
        &self,
        source: &EntryRef,
        destination: &Location,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let source_path = source
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&source.location))?;
        let destination_path = destination
            .to_native_path()
            .map_err(|_| invalid_location(destination))?;
        let target = tokio::fs::read_link(&source_path)
            .await
            .map_err(|error| map_io_error(error, &source.location.uri))?;
        let destination_clone = destination.clone();
        let symlink_path = destination_path.clone();
        tokio::task::spawn_blocking(move || create_symlink(&target, &symlink_path))
            .await
            .map_err(|error| VfsError::Io {
                message: error.to_string(),
            })?
            .map_err(|error| map_io_error(error, &destination_clone.uri))?;
        let metadata = tokio::fs::symlink_metadata(destination_path)
            .await
            .map_err(|error| map_io_error(error, &destination.uri))?;
        Ok(EntryRef {
            id: stable_entry_id(&metadata, destination),
            location: destination.clone(),
        })
    }

    async fn resolve_symlink(
        &self,
        source: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntrySummary, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let source_path = source
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&source.location))?;
        let target_path = tokio::fs::canonicalize(source_path)
            .await
            .map_err(|error| map_io_error(error, &source.location.uri))?;
        let target_location = Location::from_native_path(&target_path)
            .map_err(|_| invalid_location(&source.location))?;
        summarize_path(&target_path, &target_location).await
    }

    async fn preserve_metadata(
        &self,
        source: &EntryRef,
        destination: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let source_path = source
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&source.location))?;
        let destination_path = destination
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&destination.location))?;
        preserve_entry_metadata(&source_path, &destination_path, &source.location.uri).await
    }

    async fn same_filesystem(
        &self,
        source: &EntryRef,
        destination_directory: &Location,
        cancellation: CancellationToken,
    ) -> Result<bool, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let source_path = source
            .location
            .to_native_path()
            .map_err(|_| invalid_location(&source.location))?;
        let destination_path = destination_directory
            .to_native_path()
            .map_err(|_| invalid_location(destination_directory))?;
        let source_metadata = tokio::fs::symlink_metadata(&source_path)
            .await
            .map_err(|error| map_io_error(error, &source.location.uri))?;
        let destination_metadata = tokio::fs::symlink_metadata(&destination_path)
            .await
            .map_err(|error| map_io_error(error, &destination_directory.uri))?;
        Ok(same_device(
            &source_metadata,
            &source_path,
            &destination_metadata,
            &destination_path,
        ))
    }

    async fn watch(
        &self,
        location: &Location,
        cancellation: CancellationToken,
    ) -> Result<ProviderChangeStream, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let path = location
            .to_native_path()
            .map_err(|_| invalid_location(location))?;
        let (input_tx, mut input_rx) = mpsc::channel(WATCH_INPUT_CAPACITY);
        let (output_tx, output_rx) = mpsc::channel(8);
        let reset_required = Arc::new(AtomicBool::new(false));
        let callback_reset = Arc::clone(&reset_required);
        let handler = move |result: notify::Result<Event>| {
            let reset = result.as_ref().map_or(true, |event| event.need_rescan());
            if reset {
                callback_reset.store(true, Ordering::Release);
            }
            if input_tx.try_send(()).is_err() {
                callback_reset.store(true, Ordering::Release);
            }
        };
        // `RecommendedWatcher` selects the platform-native backend (FSEvents on macOS, inotify
        // on Linux, ReadDirectoryChangesW on Windows) - push-based and idle-cost-free, unlike
        // `PollWatcher`, which re-stats the directory on a fixed timer for as long as any pane
        // displays it regardless of whether anything changed. A watch is only ever acquired for
        // a location whose listing already succeeded (see `DirectoryService::list`'s `?` on the
        // initial `list_all`), so this never runs against a location the OS hasn't already
        // granted access to.
        let mut watcher = notify::RecommendedWatcher::new(handler, notify::Config::default())
            .map_err(|error| watch_error(error, location))?;
        watcher
            .watch(&path, RecursiveMode::NonRecursive)
            .map_err(|error| watch_error(error, location))?;

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    signal = input_rx.recv() => {
                        if signal.is_none() {
                            break;
                        }
                        tokio::time::sleep(WATCH_DEBOUNCE).await;
                        while input_rx.try_recv().is_ok() {}
                        let change = if reset_required.swap(false, Ordering::AcqRel) {
                            ProviderChange::ResetRequired
                        } else {
                            ProviderChange::Changed
                        };
                        if output_tx.send(Ok(change)).await.is_err() {
                            break;
                        }
                    }
                }
            }
            drop(watcher);
        });

        Ok(Box::pin(stream::unfold(output_rx, |mut receiver| async {
            receiver.recv().await.map(|change| (change, receiver))
        })))
    }
}

async fn preserve_copy_metadata(
    source: &Path,
    temporary: &Path,
    location: &str,
) -> Result<(), VfsError> {
    let source = source.to_owned();
    let temporary = temporary.to_owned();
    let location = location.to_owned();
    tokio::task::spawn_blocking(move || {
        let metadata =
            std::fs::metadata(&source).map_err(|error| map_io_error(error, &location))?;
        let destination_file = std::fs::OpenOptions::new()
            .write(true)
            .open(&temporary)
            .map_err(|error| map_io_error(error, &location))?;
        destination_file
            .set_times(
                std::fs::FileTimes::new()
                    .set_accessed(
                        metadata
                            .accessed()
                            .map_err(|error| map_io_error(error, &location))?,
                    )
                    .set_modified(
                        metadata
                            .modified()
                            .map_err(|error| map_io_error(error, &location))?,
                    ),
            )
            .map_err(|error| map_io_error(error, &location))?;
        drop(destination_file);
        std::fs::set_permissions(&temporary, metadata.permissions())
            .map_err(|error| map_io_error(error, &location))
    })
    .await
    .map_err(|error| VfsError::Io {
        message: error.to_string(),
    })?
}

/// Opens a path for reading, tolerating directories.
///
/// `std::fs::File::open` issues a `CreateFileW` without `FILE_FLAG_BACKUP_SEMANTICS` on
/// Windows, which the OS rejects with access-denied for directory paths (unlike Unix, where
/// `open()` on a directory succeeds). Setting that flag lets us obtain a directory handle to
/// read/set its metadata, matching the Unix behavior `preserve_entry_metadata` relies on.
fn open_for_metadata(path: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ, FILE_WRITE_ATTRIBUTES,
        };
        // GENERIC_READ (from `.read(true)`) lacks FILE_WRITE_ATTRIBUTES, which
        // `File::set_times` needs; request it explicitly via `access_mode`.
        options
            .access_mode(FILE_GENERIC_READ | FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    options.open(path)
}

async fn preserve_entry_metadata(
    source: &Path,
    destination: &Path,
    location: &str,
) -> Result<(), VfsError> {
    let source = source.to_owned();
    let destination = destination.to_owned();
    let location = location.to_owned();
    tokio::task::spawn_blocking(move || {
        let metadata =
            std::fs::metadata(&source).map_err(|error| map_io_error(error, &location))?;
        let destination_file =
            open_for_metadata(&destination).map_err(|error| map_io_error(error, &location))?;
        destination_file
            .set_times(
                std::fs::FileTimes::new()
                    .set_accessed(
                        metadata
                            .accessed()
                            .map_err(|error| map_io_error(error, &location))?,
                    )
                    .set_modified(
                        metadata
                            .modified()
                            .map_err(|error| map_io_error(error, &location))?,
                    ),
            )
            .map_err(|error| map_io_error(error, &location))?;
        drop(destination_file);
        std::fs::set_permissions(&destination, metadata.permissions())
            .map_err(|error| map_io_error(error, &location))?;
        Ok(())
    })
    .await
    .map_err(|error| VfsError::Io {
        message: error.to_string(),
    })?
}

#[cfg(unix)]
fn create_symlink(target: &Path, destination: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, destination)
}

#[cfg(windows)]
fn create_symlink(target: &Path, destination: &Path) -> io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, destination)
    } else {
        std::os::windows::fs::symlink_file(target, destination)
    }
}

/// Synchronous core of [`LocalFileSystemProvider::list`], run inside one
/// `spawn_blocking` call instead of one `tokio::fs` async call per directory
/// entry. Each `tokio::fs` call (`next_entry`/`symlink_metadata`/`file_type`)
/// is its own blocking-thread-pool round trip with real scheduling overhead;
/// for a directory with tens of thousands of entries, thousands of
/// sequential per-entry round trips dominated listing time far more than the
/// separate round-trip-count fix in `fm-application::directory::list_all`
/// (task 0156) — even after that fix reduced a 20,000-entry directory to one
/// provider call, the per-entry async overhead alone still took ~37s in
/// testing. Batching the whole page into one blocking call using plain
/// `std::fs` brings that down to native `read_dir` speed.
///
/// Trade-off: cancellation can't be observed mid-scan any more (this
/// function has no way to check a `CancellationToken`) — acceptable since a
/// page this size now completes in well under a second; the caller still
/// checks cancellation before starting.
fn list_directory_page_sync(
    path: &Path,
    parent: &Location,
    offset: usize,
    page_size: usize,
) -> Result<DirectoryPage, VfsError> {
    let uri = &parent.uri;
    let mut directory = std::fs::read_dir(path).map_err(|error| map_io_error(error, uri))?;
    for _ in 0..offset {
        match directory.next() {
            Some(entry) => {
                entry.map_err(|error| map_io_error(error, uri))?;
            }
            None => return Ok(empty_page()),
        }
    }

    let mut entries = Vec::with_capacity(page_size.min(4096));
    while entries.len() < page_size {
        let Some(entry) = directory.next() else {
            break;
        };
        entries.push(summarize_entry_sync(
            entry.map_err(|error| map_io_error(error, uri))?,
            parent,
        )?);
    }
    let has_more = match directory.next() {
        Some(Ok(_)) => true,
        Some(Err(error)) => return Err(map_io_error(error, uri)),
        None => false,
    };
    let continuation_token = has_more.then(|| (offset + entries.len()).to_string());
    Ok(DirectoryPage {
        entries,
        total_known_entries: None,
        has_more,
        continuation_token,
    })
}

fn summarize_entry_sync(
    entry: std::fs::DirEntry,
    parent: &Location,
) -> Result<EntrySummary, VfsError> {
    let name = entry
        .file_name()
        .into_string()
        .map_err(|_| VfsError::InvalidLocation {
            location: parent.uri.clone(),
        })?;
    let location = parent.join(&name).map_err(|_| invalid_location(parent))?;
    let metadata = std::fs::symlink_metadata(entry.path())
        .map_err(|error| map_io_error(error, &location.uri))?;
    let file_type = metadata.file_type();
    let kind = if is_link(&file_type, &metadata) {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        if is_macos_app_bundle_sync(&name, &entry.path()) {
            EntryKind::File
        } else {
            EntryKind::Directory
        }
    } else {
        EntryKind::File
    };
    Ok(EntrySummary {
        id: stable_entry_id(&metadata, &location),
        location,
        extension: Path::new(&name)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_owned),
        hidden: is_hidden(&name, &metadata),
        name,
        kind,
        size: (kind == EntryKind::File).then_some(metadata.len()),
        modified_at: metadata.modified().ok().map(DateTime::<Utc>::from),
        created_at: metadata.created().ok().map(DateTime::<Utc>::from),
        read_only: is_read_only(&metadata),
        mime_type: None,
        icon_key: None,
        metadata_revision: 0,
        git_status: None,
    })
}

async fn summarize_path(path: &Path, location: &Location) -> Result<EntrySummary, VfsError> {
    let name = location.name().map_err(|_| invalid_location(location))?;
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| map_io_error(error, &location.uri))?;
    let file_type = metadata.file_type();
    let kind = if is_link(&file_type, &metadata) {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        if is_macos_app_bundle(&name, path).await {
            EntryKind::File
        } else {
            EntryKind::Directory
        }
    } else {
        EntryKind::File
    };
    Ok(EntrySummary {
        id: stable_entry_id(&metadata, location),
        location: location.clone(),
        extension: Path::new(&name)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_owned),
        hidden: is_hidden(&name, &metadata),
        name,
        kind,
        size: (kind == EntryKind::File).then_some(metadata.len()),
        modified_at: metadata.modified().ok().map(DateTime::<Utc>::from),
        created_at: metadata.created().ok().map(DateTime::<Utc>::from),
        read_only: is_read_only(&metadata),
        mime_type: None,
        icon_key: None,
        metadata_revision: 0,
        git_status: None,
    })
}

fn stable_entry_id(_metadata: &std::fs::Metadata, _location: &Location) -> EntryId {
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        format!("local:{}:{}", _metadata.dev(), _metadata.ino())
    };
    #[cfg(windows)]
    let identity = match _location
        .to_native_path()
        .ok()
        .and_then(|path| windows_file_identity(&path))
    {
        Some((volume, index)) => format!("local:{volume}:{index}"),
        None => format!("local:{}", _location.uri),
    };
    #[cfg(not(any(unix, windows)))]
    let identity = format!("local:{}", _location.uri);

    EntryId::from(Uuid::new_v5(&Uuid::NAMESPACE_URL, identity.as_bytes()))
}

#[cfg(unix)]
fn same_device(
    left: &std::fs::Metadata,
    _left_path: &Path,
    right: &std::fs::Metadata,
    _right_path: &Path,
) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev()
}

#[cfg(windows)]
fn same_device(
    _left: &std::fs::Metadata,
    left_path: &Path,
    _right: &std::fs::Metadata,
    right_path: &Path,
) -> bool {
    match (
        windows_file_identity(left_path),
        windows_file_identity(right_path),
    ) {
        (Some((left_volume, _)), Some((right_volume, _))) => left_volume == right_volume,
        _ => false,
    }
}

#[cfg(not(any(unix, windows)))]
fn same_device(
    _left: &std::fs::Metadata,
    _left_path: &Path,
    _right: &std::fs::Metadata,
    _right_path: &Path,
) -> bool {
    false
}

/// Queries the volume serial number and file index for a path via the raw Win32 API,
/// since `std::os::windows::fs::MetadataExt::{volume_serial_number,file_index}` are
/// gated behind the unstable `windows_by_handle` feature (rust-lang/rust#63010).
#[cfg(windows)]
#[allow(unsafe_code)]
fn windows_file_identity(path: &Path) -> Option<(u32, u64)> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let handle = CreateFileW(
            wide.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
        let succeeded = GetFileInformationByHandle(handle, &mut info) != 0;
        CloseHandle(handle);
        if !succeeded {
            return None;
        }
        let file_index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        Some((info.dwVolumeSerialNumber, file_index))
    }
}

fn watch_error(error: notify::Error, location: &Location) -> VfsError {
    VfsError::Io {
        message: format!("{}: {error}", location.uri),
    }
}

#[cfg(windows)]
fn is_link(file_type: &std::fs::FileType, metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    file_type.is_symlink() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link(file_type: &std::fs::FileType, _metadata: &std::fs::Metadata) -> bool {
    file_type.is_symlink()
}

/// Whether a directory is a macOS application bundle, shown as a single
/// opaque item rather than an enterable directory (specification §23).
///
/// A directory qualifies when its name ends in `.app` and it contains a
/// `Contents` subdirectory, matching Finder's own bundle heuristic closely
/// enough for listing purposes without needing `NSBundle` (kept as a plain
/// filesystem check here rather than routed through `fm-platform`: crate
/// layering places `fm-vfs-local` and `fm-platform-macos` in the same layer,
/// so `fm-vfs-local` may not depend on it).
#[cfg(target_os = "macos")]
async fn is_macos_app_bundle(name: &str, path: &Path) -> bool {
    name.to_ascii_lowercase().ends_with(".app")
        && tokio::fs::metadata(path.join("Contents"))
            .await
            .is_ok_and(|metadata| metadata.is_dir())
}

#[cfg(not(target_os = "macos"))]
async fn is_macos_app_bundle(_name: &str, _path: &Path) -> bool {
    false
}

/// Sync sibling of [`is_macos_app_bundle`] for use inside
/// [`list_directory_page_sync`]'s `spawn_blocking` closure.
#[cfg(target_os = "macos")]
fn is_macos_app_bundle_sync(name: &str, path: &Path) -> bool {
    name.to_ascii_lowercase().ends_with(".app")
        && std::fs::metadata(path.join("Contents")).is_ok_and(|metadata| metadata.is_dir())
}

#[cfg(not(target_os = "macos"))]
fn is_macos_app_bundle_sync(_name: &str, _path: &Path) -> bool {
    false
}

fn decode_token(token: Option<&str>, location: &Location) -> Result<usize, VfsError> {
    token.map_or(Ok(0), |value| {
        value.parse().map_err(|_| invalid_location(location))
    })
}

fn empty_page() -> DirectoryPage {
    DirectoryPage {
        entries: Vec::new(),
        total_known_entries: None,
        has_more: false,
        continuation_token: None,
    }
}

fn permissions(metadata: &std::fs::Metadata) -> PermissionsInfo {
    PermissionsInfo {
        readable: true,
        writable: !is_read_only(metadata),
        executable: executable(metadata),
        unix_mode: unix_mode(metadata),
    }
}

/// POSIX owner/group, reported as the raw numeric uid/gid (no `std`-only API resolves them to
/// names without an extra dependency, so this mirrors what `ls -n` shows).
#[cfg(unix)]
fn ownership(metadata: &std::fs::Metadata) -> Option<OwnershipInfo> {
    use std::os::unix::fs::MetadataExt;
    Some(OwnershipInfo {
        owner: Some(metadata.uid().to_string()),
        group: Some(metadata.gid().to_string()),
    })
}

#[cfg(not(unix))]
fn ownership(_metadata: &std::fs::Metadata) -> Option<OwnershipInfo> {
    None
}

/// Windows sets `FILE_ATTRIBUTE_READONLY` on directories to flag folder customisation rather than
/// write protection, so honouring it there would wrongly block renaming ordinary folders.
#[cfg(windows)]
fn is_read_only(metadata: &std::fs::Metadata) -> bool {
    !metadata.is_dir() && metadata.permissions().readonly()
}

#[cfg(not(windows))]
fn is_read_only(metadata: &std::fs::Metadata) -> bool {
    metadata.permissions().readonly()
}

#[cfg(unix)]
fn executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn unix_mode(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(metadata.permissions().mode())
}

#[cfg(not(unix))]
fn unix_mode(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(windows)]
fn is_hidden(name: &str, metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    // System entries are hidden by Explorer's own default too, so they follow
    // the hidden-file setting rather than always being listed (task 0060).
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    name.starts_with('.')
        || metadata.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
}

#[cfg(not(windows))]
fn is_hidden(name: &str, _metadata: &std::fs::Metadata) -> bool {
    name.starts_with('.')
}

fn invalid_location(location: &Location) -> VfsError {
    VfsError::InvalidLocation {
        location: location.uri.clone(),
    }
}

fn validate_directory_name(name: &str) -> Result<(), VfsError> {
    fm_domain::location::validate_name(name).map_err(|e| match e {
        fm_domain::LocationError::EmptySegment => VfsError::EmptyName,
        fm_domain::LocationError::InvalidName(msg) => {
            if msg.contains('/') || msg.contains('\\') || msg == "." || msg == ".." {
                VfsError::PathTraversalName
            } else {
                VfsError::InvalidNameCharacters
            }
        }
        fm_domain::LocationError::NullByte => VfsError::InvalidNameCharacters,
        fm_domain::LocationError::ReservedWindowsName(_) => VfsError::ReservedName,
        _ => VfsError::InvalidNameCharacters,
    })?;
    // Additional Windows-specific validation beyond what domain layer covers
    #[cfg(windows)]
    if name.chars().any(|c| "<>:\"|?*".contains(c)) {
        return Err(VfsError::InvalidNameCharacters);
    }
    Ok(())
}

fn map_io_error(error: io::Error, location: &str) -> VfsError {
    if is_locked_error(&error) {
        return VfsError::Locked {
            location: location.to_owned(),
        };
    }
    match error.kind() {
        io::ErrorKind::NotFound => VfsError::NotFound {
            location: location.to_owned(),
        },
        io::ErrorKind::PermissionDenied => VfsError::PermissionDenied {
            location: location.to_owned(),
        },
        io::ErrorKind::AlreadyExists => VfsError::AlreadyExists {
            location: location.to_owned(),
        },
        io::ErrorKind::NotADirectory => VfsError::NotADirectory {
            location: location.to_owned(),
        },
        _ => VfsError::Io {
            message: error.to_string(),
        },
    }
}

#[cfg(all(test, windows))]
mod windows_error_tests {
    use super::map_io_error;
    use fm_vfs::VfsError;
    use std::io;

    #[test]
    fn sharing_violation_maps_to_locked_error() {
        let error = map_io_error(io::Error::from_raw_os_error(32), "file:///locked.txt");
        assert!(matches!(error, VfsError::Locked { .. }));
        assert_eq!(error.code(), "locked");
    }
}

/// Windows reports a file another process holds open without sharing as
/// `ERROR_SHARING_VIOLATION`/`ERROR_LOCK_VIOLATION`, which `io::ErrorKind`
/// still flattens into `Uncategorized` (task 0060).
#[cfg(windows)]
fn is_locked_error(error: &io::Error) -> bool {
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_LOCK_VIOLATION: i32 = 33;
    matches!(
        error.raw_os_error(),
        Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)
    )
}

#[cfg(not(windows))]
fn is_locked_error(_error: &io::Error) -> bool {
    false
}

fn unsupported<T>(capability: ProviderCapabilities) -> Result<T, VfsError> {
    Err(VfsError::UnsupportedCapability { capability })
}
