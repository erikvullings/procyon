//! Virtual filesystem provider for archive files (task 0076).
//!
//! Archive paths use `archive:///absolute/file.zip!/inner/path`. Codec work is delegated to
//! permissively licensed libraries; this crate owns provider semantics and safety policy only.

use std::{
    collections::{BTreeMap, HashMap},
    fs::{File, OpenOptions},
    io::{Cursor, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use fm_domain::{
    ArchiveInfo, EntryId, EntryKind, EntryMetadata, EntrySummary, Location, ProviderId,
};
use fm_vfs::{
    CopyCommitOptions, DirectoryPage, EntryRef, FileSystemProvider, ListOptions,
    ProviderCapabilities, ProviderChangeStream, ProviderReadStream, ProviderWriteStream,
    RemoveOptions, VfsError, WriteOptions,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

const ARCHIVE_PREFIX: &str = "archive://";
const FILE_PREFIX: &str = "file://";
const ZIP_MAGIC: &[u8] = b"PK";
const SEVEN_Z_MAGIC: &[u8] = b"7z\xBC\xAF\x27\x1C";
const GZIP_MAGIC: &[u8] = b"\x1f\x8b";
const BZIP2_MAGIC: &[u8] = b"BZh";
const XZ_MAGIC: &[u8] = b"\xfd7zXZ\0";
const RAR4_MAGIC: &[u8] = b"Rar!\x1a\x07\0";
const RAR5_MAGIC: &[u8] = b"Rar!\x1a\x07\x01\0";
const CACHE_ROOT_NAME: &str = "procyon-archive-cache";
const CACHE_SESSION_PREFIX: &str = "session-";
const CACHE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const CACHE_MAX_ENTRIES: usize = 512;

/// Resource policy applied before an archive entry is expanded.
#[derive(Clone, Copy, Debug)]
pub struct ArchiveLimits {
    /// Largest permitted expanded entry.
    pub max_uncompressed_entry_bytes: u64,
    /// Largest permitted expansion ratio (`uncompressed / compressed`).
    pub max_expansion_ratio: u64,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_uncompressed_entry_bytes: 8 * 1024 * 1024 * 1024,
            max_expansion_ratio: 1_000,
        }
    }
}

/// Provider that exposes supported archive entries as virtual directories.
#[derive(Debug)]
pub struct ArchiveFileSystemProvider {
    staged_writes: Mutex<HashMap<String, PathBuf>>,
    passwords: Mutex<HashMap<PathBuf, Zeroizing<String>>>,
    extraction_cache: Arc<Mutex<Option<SessionExtractionCache>>>,
    limits: ArchiveLimits,
}

impl Default for ArchiveFileSystemProvider {
    fn default() -> Self {
        Self::with_limits(ArchiveLimits::default())
    }
}

impl ArchiveFileSystemProvider {
    /// Creates an archive provider with an empty backend-session credential cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a provider with explicit decompression-bomb limits.
    #[must_use]
    pub fn with_limits(limits: ArchiveLimits) -> Self {
        Self {
            staged_writes: Mutex::new(HashMap::new()),
            passwords: Mutex::new(HashMap::new()),
            extraction_cache: Arc::new(Mutex::new(SessionExtractionCache::new().ok())),
            limits,
        }
    }

    /// Caches an archive password in owned, zeroizing memory for this provider session.
    pub fn cache_password(&self, location: &Location, password: String) -> Result<(), VfsError> {
        let archive_path = ParsedArchiveLocation::parse(location)?.archive_path;
        self.passwords
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(archive_path, Zeroizing::new(password));
        Ok(())
    }

    fn password_for(&self, archive_path: &Path) -> Option<Zeroizing<String>> {
        self.passwords
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(archive_path)
            .map(|password| Zeroizing::new(password.to_string()))
    }
}

/// Creates a ZIP archive from local filesystem entries.
///
/// The archive is written to a sibling temporary file and renamed only after
/// `finish` and `sync_all` succeed.  Callers retain ownership of the selected
/// paths; this helper never removes them.
pub fn create_zip_archive(
    destination: &Path,
    sources: &[PathBuf],
    compression_level: Option<i64>,
    cancellation: &CancellationToken,
) -> Result<(), VfsError> {
    if destination.exists() {
        return Err(VfsError::AlreadyExists {
            location: destination.display().to_string(),
        });
    }
    let parent = destination
        .parent()
        .ok_or_else(|| VfsError::InvalidLocation {
            location: destination.display().to_string(),
        })?;
    let temporary = parent.join(format!(".fm-archive-create-{}.tmp", Uuid::new_v4()));
    let mut guard = TemporaryFileGuard::new(temporary.clone());
    let file = File::create(&temporary).map_err(|error| io_error(error, &temporary))?;
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(compression_level);
    let mut writer = zip::ZipWriter::new(file);
    for source in sources {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| VfsError::InvalidLocation {
                location: source.display().to_string(),
            })?;
        append_zip_path(&mut writer, source, Path::new(name), options, cancellation)?;
    }
    let file = writer.finish().map_err(zip_error)?;
    file.sync_all()
        .map_err(|error| io_error(error, &temporary))?;
    if cancellation.is_cancelled() {
        return Err(VfsError::Cancelled);
    }
    std::fs::rename(&temporary, destination).map_err(|error| io_error(error, destination))?;
    guard.disarm();
    Ok(())
}

/// Creates a 7z archive from local filesystem entries using the maintained
/// `sevenz-rust2` backend.  Like ZIP creation, publication is transactional.
pub fn create_7z_archive(
    destination: &Path,
    sources: &[PathBuf],
    cancellation: &CancellationToken,
) -> Result<(), VfsError> {
    if destination.exists() {
        return Err(VfsError::AlreadyExists {
            location: destination.display().to_string(),
        });
    }
    let parent = destination
        .parent()
        .ok_or_else(|| VfsError::InvalidLocation {
            location: destination.display().to_string(),
        })?;
    let temporary = parent.join(format!(".fm-archive-create-{}.tmp", Uuid::new_v4()));
    let mut guard = TemporaryFileGuard::new(temporary.clone());
    let mut writer = sevenz_rust2::ArchiveWriter::create(&temporary).map_err(seven_zip_error)?;
    for source in sources {
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| VfsError::InvalidLocation {
                location: source.display().to_string(),
            })?;
        append_7z_path(&mut writer, source, Path::new(name), cancellation)?;
    }
    if cancellation.is_cancelled() {
        return Err(VfsError::Cancelled);
    }
    let file = writer.finish().map_err(|error| VfsError::Io {
        message: error.to_string(),
    })?;
    file.sync_all()
        .map_err(|error| io_error(error, &temporary))?;
    drop(file);
    std::fs::rename(&temporary, destination).map_err(|error| io_error(error, destination))?;
    guard.disarm();
    Ok(())
}

fn append_7z_path(
    writer: &mut sevenz_rust2::ArchiveWriter<File>,
    source: &Path,
    name: &Path,
    cancellation: &CancellationToken,
) -> Result<(), VfsError> {
    if cancellation.is_cancelled() {
        return Err(VfsError::Cancelled);
    }
    let entry_name = name.to_string_lossy().replace('\\', "/");
    let metadata = std::fs::symlink_metadata(source).map_err(|error| io_error(error, source))?;
    if metadata.file_type().is_symlink() {
        return Err(VfsError::Io {
            message: format!("refusing to archive symbolic link {}", source.display()),
        });
    }
    if metadata.is_dir() {
        writer
            .push_archive_entry(
                sevenz_rust2::ArchiveEntry::new_directory(&entry_name),
                None::<&[u8]>,
            )
            .map_err(seven_zip_error)?;
        for child in std::fs::read_dir(source).map_err(|error| io_error(error, source))? {
            let child = child.map_err(|error| io_error(error, source))?;
            append_7z_path(
                writer,
                &child.path(),
                &name.join(child.file_name()),
                cancellation,
            )?;
        }
    } else if metadata.is_file() {
        let contents = std::fs::read(source).map_err(|error| io_error(error, source))?;
        writer
            .push_archive_entry(
                sevenz_rust2::ArchiveEntry::new_file(&entry_name),
                Some(contents.as_slice()),
            )
            .map_err(seven_zip_error)?;
    }
    Ok(())
}

fn append_zip_path(
    writer: &mut zip::ZipWriter<File>,
    source: &Path,
    name: &Path,
    options: zip::write::SimpleFileOptions,
    cancellation: &CancellationToken,
) -> Result<(), VfsError> {
    if cancellation.is_cancelled() {
        return Err(VfsError::Cancelled);
    }
    let entry_name = name.to_string_lossy().replace('\\', "/");
    let metadata = std::fs::symlink_metadata(source).map_err(|error| io_error(error, source))?;
    if metadata.file_type().is_symlink() {
        return Err(VfsError::Io {
            message: format!("refusing to archive symbolic link {}", source.display()),
        });
    }
    if metadata.is_dir() {
        writer
            .add_directory(format!("{entry_name}/"), options)
            .map_err(zip_error)?;
        for child in std::fs::read_dir(source).map_err(|error| io_error(error, source))? {
            let child = child.map_err(|error| io_error(error, source))?;
            append_zip_path(
                writer,
                &child.path(),
                &name.join(child.file_name()),
                options,
                cancellation,
            )?;
        }
    } else if metadata.is_file() {
        writer.start_file(entry_name, options).map_err(zip_error)?;
        let mut input = File::open(source).map_err(|error| io_error(error, source))?;
        std::io::copy(&mut input, writer).map_err(|error| VfsError::Io {
            message: error.to_string(),
        })?;
    }
    Ok(())
}

#[async_trait]
impl FileSystemProvider for ArchiveFileSystemProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new("archive")
    }

    fn capabilities(&self) -> ProviderCapabilities {
        // The scheme-level baseline is intentionally read-only. Callers with a location use
        // `capabilities_for`, which adds mutation only for ZIP.
        // `CHECKSUM` rides along with `READ`: a checksum is just a streamed
        // read of the decompressed entry (task 0077, spec §6).
        ProviderCapabilities::LIST | ProviderCapabilities::READ | ProviderCapabilities::CHECKSUM
    }

    fn capabilities_for(&self, location: &Location) -> Result<ProviderCapabilities, VfsError> {
        let archive_path = ParsedArchiveLocation::parse(location)?.archive_path;
        let format = detect_format(&archive_path)?;
        let mut capabilities = self.capabilities();
        if format == ArchiveFormat::Zip {
            capabilities |= ProviderCapabilities::WRITE
                | ProviderCapabilities::CREATE_DIRECTORY
                | ProviderCapabilities::DELETE;
        }
        Ok(capabilities)
    }

    async fn list(
        &self,
        location: &Location,
        options: ListOptions,
        cancellation: CancellationToken,
    ) -> Result<DirectoryPage, VfsError> {
        check_request(location, options.page_size, &cancellation)?;
        let parsed = ParsedArchiveLocation::parse(location)?;
        let requested = parsed.inner.clone();
        let archive_path = parsed.archive_path.clone();
        let password = self.password_for(&archive_path);
        let (entries, writable) = tokio::task::spawn_blocking(move || {
            let entries = list_archive(
                &archive_path,
                &requested,
                password.as_ref().map(|value| value.as_str()),
            )?;
            Ok::<_, VfsError>((entries, detect_format(&archive_path)? == ArchiveFormat::Zip))
        })
        .await
        .map_err(join_error)??;
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        paginate(entries, options, location, writable)
    }

    async fn metadata(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntryMetadata, VfsError> {
        let summary = self.inspect(entry, cancellation).await?;
        let archive = if summary.kind == EntryKind::File {
            let parsed = ParsedArchiveLocation::parse(&entry.location)?;
            tokio::task::spawn_blocking(move || {
                zip_entry_archive_info(&parsed.archive_path, &parsed.inner)
            })
            .await
            .map_err(join_error)??
        } else {
            None
        };
        Ok(EntryMetadata {
            entry_id: summary.id,
            permissions: None,
            ownership: None,
            extended_attributes: BTreeMap::new(),
            checksums: BTreeMap::new(),
            image_dimensions: None,
            media: None,
            archive,
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
        let parent = entry
            .location
            .parent()
            .map_err(|_| invalid(&entry.location))?
            .ok_or_else(|| VfsError::IsADirectory {
                location: entry.location.uri.clone(),
            })?;
        let name = entry
            .location
            .name()
            .map_err(|_| invalid(&entry.location))?;
        self.list(
            &parent,
            ListOptions {
                page_size: usize::MAX,
                continuation_token: None,
            },
            cancellation,
        )
        .await?
        .entries
        .into_iter()
        .find(|candidate| candidate.name == name)
        .ok_or_else(|| VfsError::NotFound {
            location: entry.location.uri.clone(),
        })
    }

    async fn file_size(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<u64, VfsError> {
        let summary = self.inspect(entry, cancellation).await?;
        summary.size.ok_or_else(|| VfsError::IsADirectory {
            location: entry.location.uri.clone(),
        })
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
        let destination = location.join(name).map_err(|_| invalid(location))?;
        let parsed = ParsedArchiveLocation::parse(&destination)?;
        let archive_path = parsed.archive_path;
        let inner = format!("{}/", parsed.inner);
        let rewrite_cancellation = cancellation.clone();
        tokio::task::spawn_blocking(move || {
            require_zip_mutation(&archive_path, ProviderCapabilities::CREATE_DIRECTORY)?;
            rewrite_zip(
                &archive_path,
                Rewrite::AddDirectory(&inner),
                &rewrite_cancellation,
            )
        })
        .await
        .map_err(join_error)??;
        Ok(EntryRef {
            id: stable_id(&destination),
            location: destination,
        })
    }

    async fn rename(
        &self,
        _source: &EntryRef,
        _destination: &Location,
        _cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        unsupported(ProviderCapabilities::RENAME)
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
        let parsed = ParsedArchiveLocation::parse(&entry.location)?;
        if parsed.inner.is_empty() {
            return Err(invalid(&entry.location));
        }
        let archive_path = parsed.archive_path;
        let inner = parsed.inner;
        let rewrite_cancellation = cancellation.clone();
        tokio::task::spawn_blocking(move || {
            require_zip_mutation(&archive_path, ProviderCapabilities::DELETE)?;
            rewrite_zip(
                &archive_path,
                Rewrite::Remove {
                    inner: &inner,
                    recursive: options.recursive,
                },
                &rewrite_cancellation,
            )
        })
        .await
        .map_err(join_error)??;
        Ok(())
    }

    async fn open_read(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let parsed = ParsedArchiveLocation::parse(&entry.location)?;
        if parsed.inner.is_empty() {
            return Err(VfsError::IsADirectory {
                location: entry.location.uri.clone(),
            });
        }
        let archive_path = parsed.archive_path;
        let inner = parsed.inner;
        let password = self.password_for(&archive_path);
        let limits = self.limits;
        let extraction_cache = self.extraction_cache.clone();
        let bytes = tokio::task::spawn_blocking(move || {
            if detect_format(&archive_path)? == ArchiveFormat::Rar {
                read_cached_rar_entry(
                    &extraction_cache,
                    &archive_path,
                    &inner,
                    limits,
                    password.as_ref().map(|value| value.as_str()),
                )
            } else {
                read_archive_entry(
                    &archive_path,
                    &inner,
                    limits,
                    password.as_ref().map(|value| value.as_str()),
                )
            }
        })
        .await
        .map_err(join_error)??;
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        Ok(Box::pin(Cursor::new(bytes)))
    }

    async fn open_write(
        &self,
        destination: &Location,
        _options: WriteOptions,
        cancellation: CancellationToken,
    ) -> Result<ProviderWriteStream, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let parsed = ParsedArchiveLocation::parse(destination)?;
        let archive_path = parsed.archive_path.clone();
        tokio::task::spawn_blocking(move || {
            require_zip_mutation(&archive_path, ProviderCapabilities::WRITE)
        })
        .await
        .map_err(join_error)??;
        let parent = parsed
            .archive_path
            .parent()
            .ok_or_else(|| invalid(destination))?;
        let staging = parent.join(format!(".fm-archive-entry-{}.tmp", Uuid::new_v4()));
        let file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staging)
            .await
            .map_err(|error| io_error(error, &staging))?;
        self.staged_writes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(destination.uri.clone(), staging);
        Ok(Box::pin(file))
    }

    async fn commit_copy(
        &self,
        _source: &EntryRef,
        temporary: &Location,
        destination: &Location,
        options: CopyCommitOptions,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let staging = self
            .staged_writes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&temporary.uri)
            .ok_or_else(|| invalid(temporary))?;
        let parsed = ParsedArchiveLocation::parse(destination)?;
        let archive_path = parsed.archive_path;
        let inner = parsed.inner;
        let staging_for_worker = staging.clone();
        let rewrite_cancellation = cancellation.clone();
        let result = tokio::task::spawn_blocking(move || {
            require_zip_mutation(&archive_path, ProviderCapabilities::WRITE)?;
            rewrite_zip(
                &archive_path,
                Rewrite::AddFile {
                    inner: &inner,
                    staging: &staging_for_worker,
                    overwrite: options.overwrite,
                },
                &rewrite_cancellation,
            )
        })
        .await
        .map_err(join_error)?;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&staging).await;
        }
        result?;
        Ok(EntryRef {
            id: stable_id(destination),
            location: destination.clone(),
        })
    }

    async fn discard_copy(
        &self,
        temporary: &Location,
        _cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        let staging = self
            .staged_writes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&temporary.uri);
        if let Some(staging) = staging {
            match tokio::fs::remove_file(&staging).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(error, &staging)),
            }
        }
        Ok(())
    }

    async fn watch(
        &self,
        _location: &Location,
        _cancellation: CancellationToken,
    ) -> Result<ProviderChangeStream, VfsError> {
        unsupported(ProviderCapabilities::WATCH)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CacheKey {
    archive_path: PathBuf,
    archive_len: u64,
    archive_modified_nanos: u128,
    inner: String,
}

impl CacheKey {
    fn new(archive_path: &Path, inner: &str) -> Result<Self, VfsError> {
        let metadata =
            std::fs::metadata(archive_path).map_err(|error| io_error(error, archive_path))?;
        let modified = metadata
            .modified()
            .map_err(|error| io_error(error, archive_path))?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| VfsError::Io {
                message: format!(
                    "archive modification time is invalid: {}",
                    archive_path.display()
                ),
            })?;
        Ok(Self {
            archive_path: archive_path.to_path_buf(),
            archive_len: metadata.len(),
            archive_modified_nanos: modified.as_nanos(),
            inner: inner.to_owned(),
        })
    }
}

#[derive(Debug)]
struct CachedEntry {
    path: PathBuf,
    size: u64,
    last_used: u64,
}

#[derive(Debug)]
struct SessionExtractionCache {
    _lock: File,
    directory: tempfile::TempDir,
    entries: HashMap<CacheKey, CachedEntry>,
    total_bytes: u64,
    clock: u64,
}

impl SessionExtractionCache {
    fn new() -> std::io::Result<Self> {
        let root = std::env::temp_dir().join(CACHE_ROOT_NAME);
        Self::new_in(&root)
    }

    fn new_in(root: &Path) -> std::io::Result<Self> {
        create_private_directory(root)?;
        cleanup_abandoned_cache_sessions(root);
        let directory = tempfile::Builder::new()
            .prefix(CACHE_SESSION_PREFIX)
            .tempdir_in(root)?;
        set_private_permissions(directory.path())?;
        let lock_path = directory.path().join("session.lock");
        let lock = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(lock_path)?;
        fs2::FileExt::lock_exclusive(&lock)?;
        Ok(Self {
            _lock: lock,
            directory,
            entries: HashMap::new(),
            total_bytes: 0,
            clock: 0,
        })
    }

    fn read(&mut self, key: &CacheKey) -> Option<Vec<u8>> {
        let entry = self.entries.get_mut(key)?;
        match std::fs::read(&entry.path) {
            Ok(bytes) => {
                self.clock = self.clock.saturating_add(1);
                entry.last_used = self.clock;
                Some(bytes)
            }
            Err(_) => {
                let removed = self.entries.remove(key)?;
                self.total_bytes = self.total_bytes.saturating_sub(removed.size);
                None
            }
        }
    }

    fn insert(&mut self, key: CacheKey, bytes: &[u8]) -> std::io::Result<()> {
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if size > CACHE_MAX_BYTES {
            return Ok(());
        }
        if let Some(previous) = self.entries.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(previous.size);
            let _ = std::fs::remove_file(previous.path);
        }
        while self.entries.len() >= CACHE_MAX_ENTRIES
            || self.total_bytes.saturating_add(size) > CACHE_MAX_BYTES
        {
            let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest_key) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.size);
                let _ = std::fs::remove_file(entry.path);
            }
        }
        self.clock = self.clock.saturating_add(1);
        let path = self.directory.path().join(Uuid::new_v4().to_string());
        std::fs::write(&path, bytes)?;
        self.total_bytes = self.total_bytes.saturating_add(size);
        self.entries.insert(
            key,
            CachedEntry {
                path,
                size,
                last_used: self.clock,
            },
        );
        Ok(())
    }
}

fn read_cached_rar_entry(
    cache: &Mutex<Option<SessionExtractionCache>>,
    archive_path: &Path,
    inner: &str,
    limits: ArchiveLimits,
    password: Option<&str>,
) -> Result<Vec<u8>, VfsError> {
    let key = CacheKey::new(archive_path, inner)?;
    if let Some(bytes) = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_mut()
        .and_then(|cache| cache.read(&key))
    {
        return Ok(bytes);
    }
    let bytes = read_rar_entry(archive_path, inner, limits, password)?;
    if let Some(cache) = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_mut()
    {
        let _ = cache.insert(key, &bytes);
    }
    Ok(bytes)
}

fn cleanup_abandoned_cache_sessions(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(CACHE_SESSION_PREFIX)
            || !path.is_dir()
        {
            continue;
        }
        let Ok(lock) = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join("session.lock"))
        else {
            continue;
        };
        if fs2::FileExt::try_lock_exclusive(&lock).is_ok() {
            drop(lock);
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

fn create_private_directory(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    set_private_permissions(path)
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[derive(Debug)]
struct ParsedArchiveLocation {
    archive_path: PathBuf,
    inner: String,
}

impl ParsedArchiveLocation {
    fn parse(location: &Location) -> Result<Self, VfsError> {
        if location.provider_id.as_str() != "archive" {
            return Err(invalid(location));
        }
        let remainder = location
            .uri
            .strip_prefix(ARCHIVE_PREFIX)
            .ok_or_else(|| invalid(location))?;
        let (outer, inner) = remainder.split_once('!').ok_or_else(|| invalid(location))?;
        let local =
            Location::parse(&format!("{FILE_PREFIX}{outer}")).map_err(|_| invalid(location))?;
        let archive_path = local.to_native_path().map_err(|_| invalid(location))?;
        let inner = decode_archive_inner_path(inner.strip_prefix('/').unwrap_or(inner))
            .map_err(|_| invalid(location))?;
        Ok(Self {
            archive_path,
            inner,
        })
    }
}

fn decode_archive_inner_path(inner: &str) -> Result<String, ()> {
    if inner.is_empty() {
        return Ok(String::new());
    }
    inner
        .split('/')
        .map(percent_decode_segment)
        .collect::<Result<Vec<_>, _>>()
        .map(|segments| segments.join("/"))
}

fn percent_decode_segment(segment: &str) -> Result<String, ()> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(());
            }
            let high = hex_nibble(bytes[index + 1]).ok_or(())?;
            let low = hex_nibble(bytes[index + 2]).ok_or(())?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| ())
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn zip_datetime_to_utc(timestamp: Option<zip::DateTime>) -> Option<DateTime<Utc>> {
    let timestamp = timestamp?;
    Utc.with_ymd_and_hms(
        i32::from(timestamp.year()),
        u32::from(timestamp.month()),
        u32::from(timestamp.day()),
        u32::from(timestamp.hour()),
        u32::from(timestamp.minute()),
        u32::from(timestamp.second()),
    )
    .single()
}

fn unix_seconds_to_utc(seconds: u64) -> Option<DateTime<Utc>> {
    i64::try_from(seconds)
        .ok()
        .and_then(|value| DateTime::from_timestamp(value, 0))
}

fn list_zip(archive_path: &Path, requested: &str) -> Result<Vec<RawEntry>, VfsError> {
    let file = File::open(archive_path).map_err(|error| io_error(error, archive_path))?;
    let mut archive = zip::ZipArchive::new(file).map_err(zip_error)?;
    let prefix = if requested.is_empty() {
        String::new()
    } else {
        format!("{requested}/")
    };
    let mut children: HashMap<String, RawEntry> = HashMap::new();
    for index in 0..archive.len() {
        let item = archive.by_index_raw(index).map_err(zip_error)?;
        let path = safe_entry_path(&item)?;
        let Some(remainder) = path.strip_prefix(&prefix) else {
            continue;
        };
        if remainder.is_empty() {
            continue;
        }
        let mut parts = remainder.split('/');
        let name = parts.next().unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let has_descendants = parts.next().is_some();
        let is_directory = has_descendants || item.is_dir();
        let candidate = RawEntry {
            name: name.to_owned(),
            kind: if is_directory {
                EntryKind::Directory
            } else {
                EntryKind::File
            },
            size: (!is_directory).then_some(item.size()),
            modified_at: (!is_directory)
                .then(|| zip_datetime_to_utc(item.last_modified()))
                .flatten(),
        };
        match children.get(name) {
            Some(existing) if existing.kind != candidate.kind => {
                return Err(VfsError::Io {
                    message: "archive contains conflicting duplicate entry names".into(),
                });
            }
            Some(_) => {}
            None => {
                children.insert(name.to_owned(), candidate);
            }
        }
    }
    let mut values: Vec<_> = children.into_values().collect();
    values.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(values)
}

/// Looks up a single file entry's compressed size and compression method within a ZIP archive.
///
/// Returns `Ok(None)` when `archive_path` isn't a ZIP file, or when `inner_path` can't be found
/// (for example a directory entry, which ZIP doesn't record per-entry compression for) — this is
/// best-effort metadata, not required for an entry to be browsable.
fn zip_entry_archive_info(
    archive_path: &Path,
    inner_path: &str,
) -> Result<Option<ArchiveInfo>, VfsError> {
    if detect_format(archive_path)? != ArchiveFormat::Zip {
        return Ok(None);
    }
    let file = File::open(archive_path).map_err(|error| io_error(error, archive_path))?;
    let mut archive = zip::ZipArchive::new(file).map_err(zip_error)?;
    for index in 0..archive.len() {
        let item = archive.by_index_raw(index).map_err(zip_error)?;
        if item.is_dir() {
            continue;
        }
        if safe_entry_path(&item)? == inner_path {
            return Ok(Some(ArchiveInfo {
                entry_count: None,
                uncompressed_size: Some(item.size()),
                compressed_size: Some(item.compressed_size()),
                compression_method: Some(format!("{:?}", item.compression())),
            }));
        }
    }
    Ok(None)
}

fn open_rar(archive_path: &Path, password: Option<&str>) -> Result<rars::Archive, VfsError> {
    rars::ArchiveReader::read_path_with_options(
        archive_path,
        rars::ArchiveReadOptions::with_optional_password(password.map(str::as_bytes)),
    )
    .map_err(rar_error)
}

fn list_rar(
    archive_path: &Path,
    requested: &str,
    password: Option<&str>,
) -> Result<Vec<RawEntry>, VfsError> {
    let archive = open_rar(archive_path, password)?;
    let prefix = if requested.is_empty() {
        String::new()
    } else {
        format!("{requested}/")
    };
    let mut children: HashMap<String, RawEntry> = HashMap::new();
    for member in archive.members() {
        let name = std::str::from_utf8(member.meta.name_bytes()).map_err(|_| VfsError::Io {
            message: "RAR entry name is not valid UTF-8".into(),
        })?;
        let path = safe_stored_path(name, member.meta.is_directory)?;
        let Some(remainder) = path.strip_prefix(&prefix) else {
            continue;
        };
        if remainder.is_empty() {
            continue;
        }
        let mut parts = remainder.split('/');
        let child_name = parts.next().unwrap_or_default();
        if child_name.is_empty() {
            continue;
        }
        let is_directory = parts.next().is_some() || member.meta.is_directory;
        let candidate = RawEntry {
            name: child_name.to_owned(),
            kind: if is_directory {
                EntryKind::Directory
            } else {
                EntryKind::File
            },
            size: (!is_directory).then_some(member.meta.unpacked_size),
            modified_at: None,
        };
        match children.get(child_name) {
            Some(existing) if existing.kind != candidate.kind => {
                return Err(VfsError::Io {
                    message: "archive contains conflicting duplicate entry names".into(),
                });
            }
            Some(_) => {}
            None => {
                children.insert(child_name.to_owned(), candidate);
            }
        }
    }
    let mut values: Vec<_> = children.into_values().collect();
    values.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(values)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArchiveFormat {
    Zip,
    SevenZip,
    Gzip,
    Tar(TarCompression),
    Rar,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TarCompression {
    None,
    Gzip,
    Bzip2,
    Xz,
}

fn detect_format(archive_path: &Path) -> Result<ArchiveFormat, VfsError> {
    let mut file = File::open(archive_path).map_err(|error| io_error(error, archive_path))?;
    let mut magic = [0_u8; 512];
    let count = file
        .read(&mut magic)
        .map_err(|error| io_error(error, archive_path))?;
    if magic[..count].starts_with(SEVEN_Z_MAGIC) {
        Ok(ArchiveFormat::SevenZip)
    } else if magic[..count].starts_with(ZIP_MAGIC) {
        Ok(ArchiveFormat::Zip)
    } else if magic[..count].starts_with(GZIP_MAGIC) {
        let file = File::open(archive_path).map_err(|error| io_error(error, archive_path))?;
        let mut decoder = flate2::read::GzDecoder::new(file).take(262);
        let mut decompressed_magic = Vec::with_capacity(262);
        decoder
            .read_to_end(&mut decompressed_magic)
            .map_err(|error| io_error(error, archive_path))?;
        if decompressed_magic.len() >= 262 && &decompressed_magic[257..262] == b"ustar" {
            Ok(ArchiveFormat::Tar(TarCompression::Gzip))
        } else {
            Ok(ArchiveFormat::Gzip)
        }
    } else if magic[..count].starts_with(BZIP2_MAGIC) {
        Ok(ArchiveFormat::Tar(TarCompression::Bzip2))
    } else if magic[..count].starts_with(XZ_MAGIC) {
        Ok(ArchiveFormat::Tar(TarCompression::Xz))
    } else if magic[..count].starts_with(RAR4_MAGIC) || magic[..count].starts_with(RAR5_MAGIC) {
        Ok(ArchiveFormat::Rar)
    } else if count >= 262 && &magic[257..262] == b"ustar" {
        Ok(ArchiveFormat::Tar(TarCompression::None))
    } else if File::open(archive_path)
        .map_err(|error| io_error(error, archive_path))
        .and_then(|file| zip::ZipArchive::new(file).map_err(zip_error))
        .is_ok()
    {
        Ok(ArchiveFormat::Zip)
    } else if rars::ArchiveReader::read_path(archive_path).is_ok() {
        Ok(ArchiveFormat::Rar)
    } else {
        Err(VfsError::Io {
            message: "unsupported or unrecognized archive format".into(),
        })
    }
}

fn require_zip_mutation(
    archive_path: &Path,
    capability: ProviderCapabilities,
) -> Result<(), VfsError> {
    if detect_format(archive_path)? == ArchiveFormat::Zip {
        Ok(())
    } else {
        unsupported(capability)
    }
}

fn list_archive(
    archive_path: &Path,
    requested: &str,
    password: Option<&str>,
) -> Result<Vec<RawEntry>, VfsError> {
    match detect_format(archive_path)? {
        ArchiveFormat::Zip => list_zip(archive_path, requested),
        ArchiveFormat::SevenZip => list_seven_zip(archive_path, requested, password),
        ArchiveFormat::Gzip => list_gzip(archive_path, requested),
        ArchiveFormat::Tar(compression) => list_tar(archive_path, requested, compression),
        ArchiveFormat::Rar => list_rar(archive_path, requested, password),
    }
}

fn gzip_entry_name(archive_path: &Path) -> Result<String, VfsError> {
    let file_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| VfsError::Io {
            message: "gzip filename is not valid Unicode".into(),
        })?;
    let name = file_name
        .get(..file_name.len().saturating_sub(3))
        .filter(|_| file_name.to_ascii_lowercase().ends_with(".gz"))
        .filter(|name| !name.is_empty())
        .unwrap_or("content");
    safe_stored_path(name, false)
}

fn gzip_uncompressed_size(archive_path: &Path) -> Result<u64, VfsError> {
    let mut file = File::open(archive_path).map_err(|error| io_error(error, archive_path))?;
    file.seek(SeekFrom::End(-4))
        .map_err(|error| io_error(error, archive_path))?;
    let mut trailer = [0_u8; 4];
    file.read_exact(&mut trailer)
        .map_err(|error| io_error(error, archive_path))?;
    Ok(u64::from(u32::from_le_bytes(trailer)))
}

fn list_gzip(archive_path: &Path, requested: &str) -> Result<Vec<RawEntry>, VfsError> {
    if !requested.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![RawEntry {
        name: gzip_entry_name(archive_path)?,
        kind: EntryKind::File,
        size: Some(gzip_uncompressed_size(archive_path)?),
        modified_at: None,
    }])
}

fn tar_reader(archive_path: &Path, compression: TarCompression) -> Result<Box<dyn Read>, VfsError> {
    let file = File::open(archive_path).map_err(|error| io_error(error, archive_path))?;
    Ok(match compression {
        TarCompression::None => Box::new(file),
        TarCompression::Gzip => Box::new(flate2::read::GzDecoder::new(file)),
        TarCompression::Bzip2 => Box::new(bzip2::read::BzDecoder::new(file)),
        TarCompression::Xz => Box::new(xz2::read::XzDecoder::new(file)),
    })
}

fn list_tar(
    archive_path: &Path,
    requested: &str,
    compression: TarCompression,
) -> Result<Vec<RawEntry>, VfsError> {
    let mut archive = tar::Archive::new(tar_reader(archive_path, compression)?);
    let entries = archive
        .entries()
        .map_err(|error| io_error(error, archive_path))?
        .map(|entry| {
            let entry = entry.map_err(|error| io_error(error, archive_path))?;
            let entry_type = entry.header().entry_type();
            if !(entry_type.is_file() || entry_type.is_dir()) {
                return Err(unsafe_entry_error());
            }
            let path = entry
                .path()
                .map_err(|error| io_error(error, archive_path))?;
            let name = safe_stored_path(&path.to_string_lossy(), entry_type.is_dir())?;
            Ok((
                name,
                entry_type.is_dir(),
                entry
                    .header()
                    .size()
                    .map_err(|error| io_error(error, archive_path))?,
                entry.header().mtime().ok().and_then(unix_seconds_to_utc),
            ))
        })
        .collect::<Result<Vec<_>, VfsError>>()?;
    collect_children(entries, requested)
}

fn list_seven_zip(
    archive_path: &Path,
    requested: &str,
    password: Option<&str>,
) -> Result<Vec<RawEntry>, VfsError> {
    let password = password
        .map(sevenz_rust2::Password::from)
        .unwrap_or_else(sevenz_rust2::Password::empty);
    let archive = sevenz_rust2::Archive::open_with_password(archive_path, &password)
        .map_err(seven_zip_error)?;
    let entries = archive
        .files
        .iter()
        .map(|entry| {
            let name = safe_stored_path(&entry.name, entry.is_directory)?;
            Ok((name, entry.is_directory, entry.size, None))
        })
        .collect::<Result<Vec<_>, VfsError>>()?;
    collect_children(entries, requested)
}

fn collect_children(
    entries: impl IntoIterator<Item = (String, bool, u64, Option<DateTime<Utc>>)>,
    requested: &str,
) -> Result<Vec<RawEntry>, VfsError> {
    let prefix = if requested.is_empty() {
        String::new()
    } else {
        format!("{requested}/")
    };
    let mut children: HashMap<String, RawEntry> = HashMap::new();
    for (path, stored_directory, size, modified_at) in entries {
        let Some(remainder) = path.strip_prefix(&prefix) else {
            continue;
        };
        if remainder.is_empty() {
            continue;
        }
        let mut parts = remainder.split('/');
        let name = parts.next().unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let is_directory = parts.next().is_some() || stored_directory;
        let candidate = RawEntry {
            name: name.to_owned(),
            kind: if is_directory {
                EntryKind::Directory
            } else {
                EntryKind::File
            },
            size: (!is_directory).then_some(size),
            modified_at: (!is_directory).then_some(modified_at).flatten(),
        };
        match children.get(name) {
            Some(existing) if existing.kind != candidate.kind => {
                return Err(VfsError::Io {
                    message: "archive contains conflicting duplicate entry names".into(),
                });
            }
            Some(_) => {}
            None => {
                children.insert(name.to_owned(), candidate);
            }
        }
    }
    let mut values: Vec<_> = children.into_values().collect();
    values.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(values)
}

fn read_archive_entry(
    archive_path: &Path,
    inner: &str,
    limits: ArchiveLimits,
    password: Option<&str>,
) -> Result<Vec<u8>, VfsError> {
    match detect_format(archive_path)? {
        ArchiveFormat::Zip => read_zip_entry(archive_path, inner, limits, password),
        ArchiveFormat::SevenZip => {
            let password = password
                .map(sevenz_rust2::Password::from)
                .unwrap_or_else(sevenz_rust2::Password::empty);
            let mut reader = sevenz_rust2::ArchiveReader::open(archive_path, password)
                .map_err(seven_zip_error)?;
            let entry = reader
                .archive()
                .files
                .iter()
                .find(|entry| entry.name == inner)
                .ok_or_else(|| VfsError::NotFound {
                    location: inner.to_owned(),
                })?;
            check_limits(entry.size, entry.compressed_size, limits)?;
            reader.read_file(inner).map_err(seven_zip_error)
        }
        ArchiveFormat::Gzip => read_gzip_entry(archive_path, inner, limits),
        ArchiveFormat::Tar(compression) => read_tar_entry(archive_path, inner, limits, compression),
        ArchiveFormat::Rar => read_rar_entry(archive_path, inner, limits, password),
    }
}

#[derive(Clone)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn read_rar_entry(
    archive_path: &Path,
    inner: &str,
    limits: ArchiveLimits,
    password: Option<&str>,
) -> Result<Vec<u8>, VfsError> {
    let archive = open_rar(archive_path, password)?;
    let mut found = false;
    for member in archive.members() {
        let name = std::str::from_utf8(member.meta.name_bytes()).map_err(|_| VfsError::Io {
            message: "RAR entry name is not valid UTF-8".into(),
        })?;
        let path = safe_stored_path(name, member.meta.is_directory)?;
        check_limits(member.meta.unpacked_size, member.meta.packed_size, limits)?;
        if path == inner {
            if member.meta.is_directory {
                return Err(VfsError::IsADirectory {
                    location: inner.to_owned(),
                });
            }
            found = true;
        }
    }
    if !found {
        return Err(VfsError::NotFound {
            location: inner.to_owned(),
        });
    }

    let output = Arc::new(Mutex::new(Vec::new()));
    let selected = output.clone();
    let mut selected_complete = false;
    let result = archive.extract_to(password.map(str::as_bytes), |meta| {
        if selected_complete {
            return Err(rars::Error::Cancelled);
        }
        let name = String::from_utf8_lossy(meta.name_bytes()).replace('\\', "/");
        if name.trim_end_matches('/') == inner {
            selected_complete = true;
            Ok(Box::new(SharedBuffer(selected.clone())))
        } else {
            Ok(Box::new(std::io::sink()))
        }
    });
    if let Err(error) = result
        && !(selected_complete && rar_cancelled(&error))
    {
        return Err(rar_error(error));
    }
    let bytes = output
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    Ok(bytes)
}

fn read_gzip_entry(
    archive_path: &Path,
    inner: &str,
    limits: ArchiveLimits,
) -> Result<Vec<u8>, VfsError> {
    if inner != gzip_entry_name(archive_path)? {
        return Err(VfsError::NotFound {
            location: inner.to_owned(),
        });
    }
    let compressed = std::fs::metadata(archive_path)
        .map_err(|error| io_error(error, archive_path))?
        .len();
    let file = File::open(archive_path).map_err(|error| io_error(error, archive_path))?;
    let limit = limits.max_uncompressed_entry_bytes.saturating_add(1);
    let mut decoder = flate2::read::GzDecoder::new(file).take(limit);
    let mut bytes = Vec::new();
    decoder
        .read_to_end(&mut bytes)
        .map_err(|error| io_error(error, archive_path))?;
    let uncompressed = u64::try_from(bytes.len()).map_err(|_| VfsError::ArchiveResourceLimit {
        kind: "uncompressedEntryBytes",
    })?;
    check_limits(uncompressed, compressed, limits)?;
    Ok(bytes)
}

fn read_tar_entry(
    archive_path: &Path,
    inner: &str,
    limits: ArchiveLimits,
    compression: TarCompression,
) -> Result<Vec<u8>, VfsError> {
    let mut archive = tar::Archive::new(tar_reader(archive_path, compression)?);
    for entry in archive
        .entries()
        .map_err(|error| io_error(error, archive_path))?
    {
        let mut entry = entry.map_err(|error| io_error(error, archive_path))?;
        let entry_type = entry.header().entry_type();
        if !(entry_type.is_file() || entry_type.is_dir()) {
            return Err(unsafe_entry_error());
        }
        let path = entry
            .path()
            .map_err(|error| io_error(error, archive_path))?;
        let name = safe_stored_path(&path.to_string_lossy(), entry_type.is_dir())?;
        if name != inner {
            continue;
        }
        if entry_type.is_dir() {
            return Err(VfsError::IsADirectory {
                location: inner.to_owned(),
            });
        }
        let size = entry
            .header()
            .size()
            .map_err(|error| io_error(error, archive_path))?;
        if size > limits.max_uncompressed_entry_bytes {
            return Err(VfsError::ArchiveResourceLimit {
                kind: "uncompressedEntryBytes",
            });
        }
        let capacity = usize::try_from(size).map_err(|_| VfsError::ArchiveResourceLimit {
            kind: "uncompressedEntryBytes",
        })?;
        let mut bytes = Vec::with_capacity(capacity);
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| io_error(error, archive_path))?;
        return Ok(bytes);
    }
    Err(VfsError::NotFound {
        location: inner.to_owned(),
    })
}

fn read_zip_entry(
    archive_path: &Path,
    inner: &str,
    limits: ArchiveLimits,
    password: Option<&str>,
) -> Result<Vec<u8>, VfsError> {
    let file = File::open(archive_path).map_err(|error| io_error(error, archive_path))?;
    let mut archive = zip::ZipArchive::new(file).map_err(zip_error)?;
    let mut item = match password {
        Some(password) => archive.by_name_decrypt(inner, password.as_bytes()),
        None => archive.by_name(inner),
    }
    .map_err(|error| match error {
        zip::result::ZipError::FileNotFound => VfsError::NotFound {
            location: inner.to_owned(),
        },
        zip::result::ZipError::UnsupportedArchive(message)
            if message == zip::result::ZipError::PASSWORD_REQUIRED =>
        {
            VfsError::CredentialRequired
        }
        zip::result::ZipError::InvalidPassword if password.is_none() => {
            VfsError::CredentialRequired
        }
        zip::result::ZipError::InvalidPassword => VfsError::InvalidCredential,
        other => zip_error(other),
    })?;
    if item.is_dir() {
        return Err(VfsError::IsADirectory {
            location: inner.to_owned(),
        });
    }
    check_limits(item.size(), item.compressed_size(), limits)?;
    let capacity = usize::try_from(item.size()).map_err(|_| VfsError::Io {
        message: "archive entry is too large".into(),
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    item.read_to_end(&mut bytes).map_err(|error| VfsError::Io {
        message: error.to_string(),
    })?;
    Ok(bytes)
}

fn check_limits(uncompressed: u64, compressed: u64, limits: ArchiveLimits) -> Result<(), VfsError> {
    if uncompressed > limits.max_uncompressed_entry_bytes {
        return Err(VfsError::ArchiveResourceLimit {
            kind: "uncompressedEntryBytes",
        });
    }
    if uncompressed > 0
        && (compressed == 0 || uncompressed / compressed.max(1) > limits.max_expansion_ratio)
    {
        return Err(VfsError::ArchiveResourceLimit {
            kind: "expansionRatio",
        });
    }
    Ok(())
}

fn safe_entry_path<R: Read>(entry: &zip::read::ZipFile<'_, R>) -> Result<String, VfsError> {
    let path = entry.enclosed_name().ok_or_else(unsafe_entry_error)?;
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(unsafe_entry_error());
    }
    let mut text = path.to_string_lossy().replace('\\', "/");
    if entry.is_dir() {
        text = text.trim_end_matches('/').to_owned();
    }
    Ok(text)
}

fn safe_stored_path(name: &str, is_directory: bool) -> Result<String, VfsError> {
    let normalized = name.replace('\\', "/");
    let path = Path::new(&normalized);
    if normalized.starts_with('/')
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(unsafe_entry_error());
    }
    Ok(if is_directory {
        normalized.trim_end_matches('/').to_owned()
    } else {
        normalized
    })
}

fn unsafe_entry_error() -> VfsError {
    VfsError::UnsafeArchiveEntry
}

#[derive(Debug)]
struct RawEntry {
    name: String,
    kind: EntryKind,
    size: Option<u64>,
    modified_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy)]
enum Rewrite<'a> {
    AddFile {
        inner: &'a str,
        staging: &'a Path,
        overwrite: bool,
    },
    AddDirectory(&'a str),
    Remove {
        inner: &'a str,
        recursive: bool,
    },
}

fn rewrite_zip(
    archive_path: &Path,
    rewrite: Rewrite<'_>,
    cancellation: &CancellationToken,
) -> Result<(), VfsError> {
    let source_file = File::open(archive_path).map_err(|error| io_error(error, archive_path))?;
    let mut source = zip::ZipArchive::new(source_file).map_err(zip_error)?;
    let parent = archive_path
        .parent()
        .ok_or_else(|| VfsError::InvalidLocation {
            location: archive_path.display().to_string(),
        })?;
    let replacement = parent.join(format!(".fm-archive-rewrite-{}.tmp", Uuid::new_v4()));
    let mut replacement_guard = TemporaryFileGuard::new(replacement.clone());
    let replacement_file =
        File::create(&replacement).map_err(|error| io_error(error, &replacement))?;
    let mut writer = zip::ZipWriter::new(replacement_file);
    let mut matched = false;
    let remove_prefix = match &rewrite {
        Rewrite::Remove { inner, .. } => Some(format!("{}/", inner.trim_end_matches('/'))),
        _ => None,
    };
    for index in 0..source.len() {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        let item = source.by_index_raw(index).map_err(zip_error)?;
        let name = safe_entry_path(&item)?;
        let skip = match &rewrite {
            Rewrite::AddFile {
                inner, overwrite, ..
            } if name == *inner => {
                if !overwrite {
                    return Err(VfsError::AlreadyExists {
                        location: (*inner).to_owned(),
                    });
                }
                matched = true;
                true
            }
            Rewrite::AddDirectory(inner)
                if name.trim_end_matches('/') == inner.trim_end_matches('/') =>
            {
                return Err(VfsError::AlreadyExists {
                    location: (*inner).to_owned(),
                });
            }
            Rewrite::Remove { inner, recursive } => {
                let exact = name.trim_end_matches('/') == inner.trim_end_matches('/');
                let descendant = remove_prefix
                    .as_ref()
                    .is_some_and(|prefix| name.starts_with(prefix));
                if descendant && !recursive {
                    return Err(VfsError::Io {
                        message: "archive directory is not empty".into(),
                    });
                }
                matched |= exact || descendant;
                exact || descendant
            }
            _ => false,
        };
        if !skip {
            writer.raw_copy_file(item).map_err(zip_error)?;
        }
    }
    match rewrite {
        Rewrite::AddFile { inner, staging, .. } => {
            if cancellation.is_cancelled() {
                return Err(VfsError::Cancelled);
            }
            writer
                .start_file(inner, zip::write::SimpleFileOptions::default())
                .map_err(zip_error)?;
            let mut input = File::open(staging).map_err(|error| io_error(error, staging))?;
            std::io::copy(&mut input, &mut writer).map_err(|error| VfsError::Io {
                message: error.to_string(),
            })?;
        }
        Rewrite::AddDirectory(inner) => {
            writer
                .add_directory(inner, zip::write::SimpleFileOptions::default())
                .map_err(zip_error)?;
        }
        Rewrite::Remove { inner, .. } if !matched => {
            return Err(VfsError::NotFound {
                location: inner.to_owned(),
            });
        }
        Rewrite::Remove { .. } => {}
    }
    let replacement_file = writer.finish().map_err(zip_error)?;
    replacement_file
        .sync_all()
        .map_err(|error| io_error(error, &replacement))?;
    if cancellation.is_cancelled() {
        return Err(VfsError::Cancelled);
    }
    std::fs::rename(&replacement, archive_path).map_err(|error| {
        let _ = std::fs::remove_file(&replacement);
        io_error(error, archive_path)
    })?;
    replacement_guard.disarm();
    if let Rewrite::AddFile { staging, .. } = rewrite {
        let _ = std::fs::remove_file(staging);
    }
    Ok(())
}

struct TemporaryFileGuard {
    path: PathBuf,
    armed: bool,
}

impl TemporaryFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TemporaryFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn paginate(
    entries: Vec<RawEntry>,
    options: ListOptions,
    location: &Location,
    writable: bool,
) -> Result<DirectoryPage, VfsError> {
    let offset = match options.continuation_token {
        Some(token) => token.parse::<usize>().map_err(|_| invalid(location))?,
        None => 0,
    };
    if offset > entries.len() {
        return Err(invalid(location));
    }
    let total = entries.len();
    let page: Vec<_> = entries
        .into_iter()
        .skip(offset)
        .take(options.page_size)
        .map(|entry| {
            let child = location.join(&entry.name).map_err(|_| invalid(location))?;
            let extension = (entry.kind == EntryKind::File)
                .then(|| extension(&entry.name))
                .flatten();
            Ok(EntrySummary {
                id: stable_id(&child),
                location: child,
                name: entry.name,
                kind: entry.kind,
                size: entry.size,
                modified_at: entry.modified_at,
                created_at: None,
                hidden: false,
                read_only: !writable,
                extension,
                mime_type: None,
                icon_key: None,
                metadata_revision: 0,
                git_status: None,
            })
        })
        .collect::<Result<_, VfsError>>()?;
    let next = offset + page.len();
    Ok(DirectoryPage {
        entries: page,
        total_known_entries: Some(total as u64),
        has_more: next < total,
        continuation_token: (next < total).then(|| next.to_string()),
    })
}

fn stable_id(location: &Location) -> EntryId {
    EntryId::from(Uuid::new_v5(&Uuid::NAMESPACE_URL, location.uri.as_bytes()))
}

fn extension(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
}

fn check_request(
    location: &Location,
    page_size: usize,
    cancellation: &CancellationToken,
) -> Result<(), VfsError> {
    if cancellation.is_cancelled() {
        return Err(VfsError::Cancelled);
    }
    if page_size == 0 {
        return Err(invalid(location));
    }
    Ok(())
}

fn unsupported<T>(capability: ProviderCapabilities) -> Result<T, VfsError> {
    Err(VfsError::UnsupportedCapability { capability })
}

fn invalid(location: &Location) -> VfsError {
    VfsError::InvalidLocation {
        location: location.uri.clone(),
    }
}
fn join_error(error: tokio::task::JoinError) -> VfsError {
    VfsError::Io {
        message: format!("archive worker failed: {error}"),
    }
}
fn zip_error(error: zip::result::ZipError) -> VfsError {
    VfsError::Io {
        message: format!("invalid ZIP archive: {error}"),
    }
}
fn seven_zip_error(error: sevenz_rust2::Error) -> VfsError {
    match error {
        sevenz_rust2::Error::PasswordRequired => VfsError::CredentialRequired,
        sevenz_rust2::Error::MaybeBadPassword(_) => VfsError::InvalidCredential,
        other => VfsError::Io {
            message: format!("invalid 7z archive: {other}"),
        },
    }
}
fn rar_error(error: rars::Error) -> VfsError {
    match error {
        rars::Error::NeedPassword => VfsError::CredentialRequired,
        rars::Error::WrongPasswordOrCorruptData => VfsError::InvalidCredential,
        rars::Error::AtArchiveOffset { source, .. } | rars::Error::AtEntry { source, .. } => {
            rar_error(*source)
        }
        other => VfsError::Io {
            message: format!("invalid RAR archive: {other}"),
        },
    }
}

fn rar_cancelled(error: &rars::Error) -> bool {
    match error {
        rars::Error::Cancelled => true,
        rars::Error::AtArchiveOffset { source, .. } | rars::Error::AtEntry { source, .. } => {
            rar_cancelled(source)
        }
        _ => false,
    }
}
fn io_error(error: std::io::Error, path: &Path) -> VfsError {
    match error.kind() {
        std::io::ErrorKind::NotFound => VfsError::NotFound {
            location: path.display().to_string(),
        },
        std::io::ErrorKind::PermissionDenied => VfsError::PermissionDenied {
            location: path.display().to_string(),
        },
        _ => VfsError::Io {
            message: error.to_string(),
        },
    }
}

#[cfg(test)]
mod unit_tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn session_cache_reuses_entries_and_invalidates_changed_archives() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let cache_root = directory.path().join("cache");
        let archive_path = directory.path().join("comic.rar");
        std::fs::write(&archive_path, b"first archive").expect("write archive fixture");
        let first_key = CacheKey::new(&archive_path, "001.jpg").expect("first fingerprint");
        let mut cache = SessionExtractionCache::new_in(&cache_root).expect("create cache");

        cache
            .insert(first_key.clone(), b"page one")
            .expect("cache page");
        assert_eq!(cache.read(&first_key), Some(b"page one".to_vec()));

        std::fs::write(&archive_path, b"changed archive contents").expect("change archive fixture");
        let changed_key = CacheKey::new(&archive_path, "001.jpg").expect("changed fingerprint");
        assert_ne!(changed_key, first_key);
        assert_eq!(cache.read(&changed_key), None);
    }

    #[test]
    fn startup_cleanup_removes_only_abandoned_sessions() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let cache_root = directory.path().join("cache");
        let active = SessionExtractionCache::new_in(&cache_root).expect("create active cache");
        let active_path = active.directory.path().to_path_buf();
        let abandoned = cache_root.join(format!("{CACHE_SESSION_PREFIX}abandoned"));
        create_private_directory(&abandoned).expect("create abandoned cache");
        File::create(abandoned.join("session.lock")).expect("create abandoned lock");
        std::fs::write(abandoned.join("plaintext"), b"cached page")
            .expect("write abandoned cache entry");

        let second = SessionExtractionCache::new_in(&cache_root).expect("create second cache");

        assert!(active_path.exists(), "locked active cache must remain");
        assert!(
            !abandoned.exists(),
            "unlocked crashed cache must be removed"
        );
        drop(second);
        drop(active);
        assert!(
            !active_path.exists(),
            "normal shutdown removes session cache"
        );
    }

    #[test]
    fn cancelled_rewrite_preserves_archive_and_removes_temporary_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let archive_path = directory.path().join("sample.zip");
        let file = File::create(&archive_path).expect("create ZIP");
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("first.txt", zip::write::SimpleFileOptions::default())
            .expect("start entry");
        writer.write_all(b"first").expect("write entry");
        writer
            .start_file("second.txt", zip::write::SimpleFileOptions::default())
            .expect("start entry");
        writer.write_all(b"second").expect("write entry");
        writer.finish().expect("finish ZIP");
        let before = std::fs::read(&archive_path).expect("read original ZIP");
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let result = rewrite_zip(
            &archive_path,
            Rewrite::Remove {
                inner: "first.txt",
                recursive: false,
            },
            &cancellation,
        );

        assert!(matches!(result, Err(VfsError::Cancelled)));
        assert_eq!(
            std::fs::read(&archive_path).expect("read preserved ZIP"),
            before
        );
        let leftovers = std::fs::read_dir(directory.path())
            .expect("list temporary directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".fm-archive-rewrite-")
            })
            .count();
        assert_eq!(leftovers, 0);
    }
}
