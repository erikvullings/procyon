use std::collections::BTreeMap;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{SemanticStateError, SemanticStateStore, Sha256Digest};

/// One isolated category within the semantic-data root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataCategory {
    /// Signed catalog state and resumable downloads.
    Catalog,
    /// Extracted text and structured content.
    Extracted,
    /// Zvec index data.
    Zvec,
    /// Reusable embedding cache.
    EmbeddingCache,
    /// Installed model packages.
    Models,
    /// Versioned worker and runtime bundles.
    Workers,
}

impl DataCategory {
    /// Returns all categories in stable reporting order.
    #[must_use]
    pub const fn all() -> &'static [Self; 6] {
        &[
            Self::Catalog,
            Self::Extracted,
            Self::Zvec,
            Self::EmbeddingCache,
            Self::Models,
            Self::Workers,
        ]
    }

    /// Returns the stable directory name.
    #[must_use]
    pub const fn directory_name(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Extracted => "extracted",
            Self::Zvec => "zvec",
            Self::EmbeddingCache => "embedding-cache",
            Self::Models => "models",
            Self::Workers => "workers",
        }
    }
}

/// Configurable root containing all semantic data categories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticDataRoot(PathBuf);

impl SemanticDataRoot {
    /// Derives the default root from an injected platform app-data path.
    #[must_use]
    pub fn from_app_data(app_data: &Path) -> Self {
        Self(app_data.join("semantic"))
    }

    /// Uses an explicit user-selected semantic-data root.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Returns the configured root path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Returns one known category path.
    #[must_use]
    pub fn category_path(&self, category: DataCategory) -> PathBuf {
        self.0.join(category.directory_name())
    }

    /// Creates the root and all separated category directories.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem failure.
    pub fn initialize(&self) -> Result<(), DataRootError> {
        ensure_no_symlink_components(&self.0)?;
        fs::create_dir_all(&self.0)?;
        ensure_directory(&self.0)?;
        for category in DataCategory::all() {
            let path = self.category_path(*category);
            fs::create_dir_all(&path)?;
            ensure_directory(&path)?;
        }
        Ok(())
    }
}

pub(crate) fn ensure_no_symlink_components(path: &Path) -> Result<(), DataRootError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(DataRootError::UnsafeLayout { path: current });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<(), DataRootError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(DataRootError::UnsafeLayout {
            path: path.to_owned(),
        });
    }
    Ok(())
}

/// Semantic-data filesystem failure.
#[derive(Debug, Error)]
pub enum DataRootError {
    /// A root or category path was a symlink or non-directory entry.
    #[error("semantic data layout contains an unsafe path: {}", path.display())]
    UnsafeLayout {
        /// Unsafe root or category path.
        path: PathBuf,
    },
    /// A filesystem operation failed.
    #[error("semantic data filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}

/// A host-owned RAII pause that resumes indexing when dropped.
///
/// Implementations must keep indexing paused for the guard's lifetime and
/// resume it from their `Drop` implementation.
pub trait IndexingPauseGuard: Send {}

/// Host boundary used to pause indexing before data movement.
pub trait IndexingController: Send + Sync {
    /// Pauses indexing until the returned guard is dropped.
    ///
    /// # Errors
    ///
    /// Returns a typed host pause failure.
    fn pause(&self) -> Result<Box<dyn IndexingPauseGuard>, PauseError>;
}

/// Indexing pause failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("indexing could not be paused: {message}")]
pub struct PauseError {
    message: String,
}

impl PauseError {
    /// Creates a host pause failure.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Cancellation boundary checked throughout a data-root migration.
pub trait DataMigrationCancellation: Send + Sync {
    /// Reports whether migration should stop before its next operation.
    fn is_cancelled(&self) -> bool;
}

/// Successful verified data-root switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRootMigrationReceipt {
    source: PathBuf,
    destination: PathBuf,
    verified_file_count: u64,
    verified_bytes: u64,
}

impl DataRootMigrationReceipt {
    /// Returns the retained source root.
    #[must_use]
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Returns the new active root.
    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Returns the number of files independently verified by size and SHA-256.
    #[must_use]
    pub const fn verified_file_count(&self) -> u64 {
        self.verified_file_count
    }

    /// Returns the verified byte count.
    #[must_use]
    pub const fn verified_bytes(&self) -> u64 {
        self.verified_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    bytes: u64,
    checksum: Sha256Digest,
}

pub(crate) fn migrate(
    store: &SemanticStateStore,
    app_data: &Path,
    destination: &Path,
    indexing: &dyn IndexingController,
    cancellation: &dyn DataMigrationCancellation,
) -> Result<DataRootMigrationReceipt, DataRootMigrationError> {
    if destination.as_os_str().is_empty()
        || destination.as_os_str().to_string_lossy().contains('\0')
        || destination.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err(DataRootMigrationError::UnsafeDestination);
    }
    let mut state = store.load_or_default_unlocked(app_data)?;
    let _pause = indexing.pause()?;
    state.data_root().initialize()?;
    let source = state.data_root().path().to_owned();
    let source_metadata = fs::symlink_metadata(&source)?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
        return Err(DataRootMigrationError::UnsafeEntry {
            path: source.clone(),
        });
    }
    let canonical_source = fs::canonicalize(&source)?;
    match fs::symlink_metadata(destination) {
        Ok(_) => {
            return Err(DataRootMigrationError::DestinationExists {
                path: destination.to_owned(),
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    check_migration_cancelled(cancellation)?;

    let destination_parent = destination
        .parent()
        .ok_or(DataRootMigrationError::DestinationHasNoParent)?;
    let destination_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(DataRootMigrationError::UnsafeDestination)?;
    if matches!(destination_name, "." | "..") {
        return Err(DataRootMigrationError::UnsafeDestination);
    }
    if destination_parent.exists() {
        let canonical_candidate = fs::canonicalize(destination_parent)?.join(destination_name);
        if canonical_source == canonical_candidate
            || canonical_source.starts_with(&canonical_candidate)
            || canonical_candidate.starts_with(&canonical_source)
        {
            return Err(DataRootMigrationError::OverlappingRoots);
        }
    }
    crate::data_root::ensure_no_symlink_components(destination_parent).map_err(
        |error| match error {
            DataRootError::UnsafeLayout { path } => DataRootMigrationError::UnsafeEntry { path },
            DataRootError::Io(error) => DataRootMigrationError::Io(error),
        },
    )?;
    if let Ok(metadata) = fs::symlink_metadata(destination_parent)
        && metadata.file_type().is_symlink()
    {
        return Err(DataRootMigrationError::UnsafeEntry {
            path: destination_parent.to_owned(),
        });
    }
    fs::create_dir_all(destination_parent)?;
    let destination_parent_metadata = fs::symlink_metadata(destination_parent)?;
    if destination_parent_metadata.file_type().is_symlink() || !destination_parent_metadata.is_dir()
    {
        return Err(DataRootMigrationError::UnsafeEntry {
            path: destination_parent.to_owned(),
        });
    }
    let canonical_destination_parent = fs::canonicalize(destination_parent)?;
    let canonical_destination = canonical_destination_parent.join(destination_name);
    if canonical_source == canonical_destination
        || canonical_source.starts_with(&canonical_destination)
        || canonical_destination.starts_with(&canonical_source)
    {
        return Err(DataRootMigrationError::OverlappingRoots);
    }
    let staging = canonical_destination_parent
        .join(format!(".{destination_name}.migration-{}", Uuid::new_v4()));
    let result = copy_and_verify_known_categories(&canonical_source, &staging, cancellation);
    let (verified_file_count, verified_bytes) = match result {
        Ok(verified) => verified,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    check_migration_cancelled(cancellation).inspect_err(|_| {
        let _ = fs::remove_dir_all(&staging);
    })?;
    if let Err(error) = fs::rename(&staging, &canonical_destination) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error.into());
    }
    if let Err(error) = sync_directory_tree(store, &canonical_destination)
        .and_then(|()| store.sync_directory(&canonical_destination_parent))
    {
        let _ = fs::remove_dir_all(&canonical_destination);
        return Err(error.into());
    }

    if let Err(error) =
        state.replace_data_root(SemanticDataRoot::new(canonical_destination.clone()))
    {
        let _ = fs::remove_dir_all(&canonical_destination);
        return Err(error.into());
    }
    if let Err(error) = store.save_unlocked(&state) {
        if !error.commit_outcome_unknown() {
            let _ = fs::remove_dir_all(&canonical_destination);
        }
        return Err(error.into());
    }
    Ok(DataRootMigrationReceipt {
        source,
        destination: canonical_destination,
        verified_file_count,
        verified_bytes,
    })
}

fn sync_directory_tree(store: &SemanticStateStore, directory: &Path) -> Result<(), std::io::Error> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::other(format!(
                "cannot synchronize symlinked semantic data entry {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            sync_directory_tree(store, &path)?;
        }
    }
    store.sync_directory(directory)
}

fn copy_and_verify_known_categories(
    source: &Path,
    staging: &Path,
    cancellation: &dyn DataMigrationCancellation,
) -> Result<(u64, u64), DataRootMigrationError> {
    fs::create_dir(staging)?;
    for category in DataCategory::all() {
        check_migration_cancelled(cancellation)?;
        let source_category = source.join(category.directory_name());
        let destination_category = staging.join(category.directory_name());
        fs::create_dir(&destination_category)?;
        match fs::symlink_metadata(&source_category) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(DataRootMigrationError::UnsafeEntry {
                    path: source_category,
                });
            }
            Ok(metadata) => {
                copy_tree(&source_category, &destination_category, cancellation)?;
                copy_and_verify_permissions(&metadata, &destination_category)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let source_root_metadata = fs::symlink_metadata(source)?;
        copy_and_verify_permissions(&source_root_metadata, staging)?;
    }

    let source_inventory = known_category_inventory(source, cancellation)?;
    let destination_inventory = known_category_inventory(staging, cancellation)?;
    if source_inventory != destination_inventory {
        return Err(DataRootMigrationError::VerificationFailed);
    }
    let files = u64::try_from(source_inventory.len()).unwrap_or(u64::MAX);
    let bytes = source_inventory.values().try_fold(0_u64, |total, file| {
        total
            .checked_add(file.bytes)
            .ok_or(DataRootMigrationError::SizeOverflow)
    })?;
    Ok((files, bytes))
}

fn copy_tree(
    source: &Path,
    destination: &Path,
    cancellation: &dyn DataMigrationCancellation,
) -> Result<(), DataRootMigrationError> {
    let mut entries: Vec<_> = fs::read_dir(source)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        check_migration_cancelled(cancellation)?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            return Err(DataRootMigrationError::UnsafeEntry { path: source_path });
        }
        if metadata.is_dir() {
            fs::create_dir(&destination_path)?;
            copy_tree(&source_path, &destination_path, cancellation)?;
            copy_and_verify_permissions(&metadata, &destination_path)?;
        } else if metadata.is_file() {
            copy_file(&source_path, &destination_path, cancellation)?;
        } else {
            return Err(DataRootMigrationError::UnsafeEntry { path: source_path });
        }
    }
    Ok(())
}

fn copy_file(
    source: &Path,
    destination: &Path,
    cancellation: &dyn DataMigrationCancellation,
) -> Result<(), DataRootMigrationError> {
    let mut input_options = OpenOptions::new();
    input_options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        input_options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut input = input_options.open(source)?;
    let mut output_options = OpenOptions::new();
    output_options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        output_options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut output = output_options.open(destination)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        check_migration_cancelled(cancellation)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
    }
    output.sync_all()?;
    copy_and_verify_permissions(&fs::symlink_metadata(source)?, destination)?;
    Ok(())
}

fn copy_and_verify_permissions(
    source: &fs::Metadata,
    destination: &Path,
) -> Result<(), DataRootMigrationError> {
    let permissions = safe_permissions(source);
    fs::set_permissions(destination, permissions)?;
    let copied = fs::symlink_metadata(destination)?;
    if permission_fingerprint(source) != permission_fingerprint(&copied) {
        return Err(DataRootMigrationError::VerificationFailed);
    }
    Ok(())
}

#[cfg(unix)]
fn safe_permissions(metadata: &fs::Metadata) -> fs::Permissions {
    use std::os::unix::fs::PermissionsExt;

    fs::Permissions::from_mode(metadata.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn safe_permissions(metadata: &fs::Metadata) -> fs::Permissions {
    metadata.permissions()
}

#[cfg(unix)]
fn permission_fingerprint(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn permission_fingerprint(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

fn known_category_inventory(
    root: &Path,
    cancellation: &dyn DataMigrationCancellation,
) -> Result<BTreeMap<PathBuf, FileFingerprint>, DataRootMigrationError> {
    let mut inventory = BTreeMap::new();
    for category in DataCategory::all() {
        let category_root = root.join(category.directory_name());
        match fs::symlink_metadata(&category_root) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(DataRootMigrationError::UnsafeEntry {
                    path: category_root,
                });
            }
            Ok(_) => inventory_tree(root, &category_root, cancellation, &mut inventory)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(inventory)
}

fn inventory_tree(
    root: &Path,
    directory: &Path,
    cancellation: &dyn DataMigrationCancellation,
    inventory: &mut BTreeMap<PathBuf, FileFingerprint>,
) -> Result<(), DataRootMigrationError> {
    let mut entries: Vec<_> = fs::read_dir(directory)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        check_migration_cancelled(cancellation)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(DataRootMigrationError::UnsafeEntry { path });
        }
        if metadata.is_dir() {
            inventory_tree(root, &path, cancellation, inventory)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| DataRootMigrationError::UnsafeEntry { path: path.clone() })?
                .to_owned();
            inventory.insert(
                relative,
                FileFingerprint {
                    bytes: metadata.len(),
                    checksum: hash_file(&path, cancellation)?,
                },
            );
        } else {
            return Err(DataRootMigrationError::UnsafeEntry { path });
        }
    }
    Ok(())
}

fn hash_file(
    path: &Path,
    cancellation: &dyn DataMigrationCancellation,
) -> Result<Sha256Digest, DataRootMigrationError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        check_migration_cancelled(cancellation)?;
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }

        digest.update(&buffer[..read]);
    }
    Ok(Sha256Digest::from_bytes(digest.finalize().into()))
}

fn check_migration_cancelled(
    cancellation: &dyn DataMigrationCancellation,
) -> Result<(), DataRootMigrationError> {
    if cancellation.is_cancelled() {
        Err(DataRootMigrationError::Cancelled)
    } else {
        Ok(())
    }
}

/// Pause-copy-verify-switch migration failure.
#[derive(Debug, Error)]
pub enum DataRootMigrationError {
    /// Source and destination overlap, which could recursively copy data.
    #[error("semantic data source and destination must not overlap")]
    OverlappingRoots,
    /// Destination already exists and cannot be replaced implicitly.
    #[error("semantic data destination already exists: {}", path.display())]
    DestinationExists {
        /// Existing destination.
        path: PathBuf,
    },
    /// Destination has no parent directory.
    #[error("semantic data destination has no parent directory")]
    DestinationHasNoParent,
    /// The destination used ambiguous dot segments or had no normal final component.
    #[error("semantic data destination path is not safely normalized")]
    UnsafeDestination,
    /// A symlink, device, socket, or other unsafe entry was encountered.
    #[error("semantic data contains an unsafe entry: {}", path.display())]
    UnsafeEntry {
        /// Rejected source path.
        path: PathBuf,
    },
    /// Source and copied size/SHA-256 inventories differed.
    #[error("copied semantic data failed size and SHA-256 verification")]
    VerificationFailed,
    /// Migration was cancelled before the verified switch.
    #[error("semantic data migration was cancelled")]
    Cancelled,
    /// Verified byte count overflowed.
    #[error("semantic data byte count overflowed")]
    SizeOverflow,
    /// Indexing could not be paused.
    #[error(transparent)]
    Pause(#[from] PauseError),
    /// Source data layout creation failed.
    #[error(transparent)]
    DataRoot(#[from] DataRootError),
    /// Durable semantic state failed.
    #[error(transparent)]
    State(#[from] SemanticStateError),
    /// Filesystem access failed.
    #[error("semantic data migration filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}
