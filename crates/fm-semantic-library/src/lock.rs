//! Exclusive cross-process serialization of semantic-library mutations.
//!
//! Two independently persisted documents plus a write-ahead journal are only
//! consistent if exactly one writer at a time may recover the journal, read the
//! current documents, decide, and commit. An in-process mutex alone cannot do
//! that: a second Procyon process — or a desktop app running beside a
//! server — would recover the same journal concurrently and lose the other's
//! update.
//!
//! The lock therefore has two layers, mirroring the managed-component state
//! lock (`fm-semantic-components`): a process-wide registry keyed by the
//! canonical semantic-data root, because advisory `flock` is per-open-file and
//! does not exclude threads of the same process, and an `fs2` exclusive file
//! lock beneath that root for other processes.
//!
//! Acquiring the file lock materialises the semantic-data root and the lock
//! file when they do not exist yet. Skipping the file lock while the root is
//! absent would leave the first-use window — exactly when two processes start
//! together against a fresh profile — unserialized, and journal recovery runs
//! on the *read* path, so even a read must not proceed without it.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};

use crate::StoreError;

pub(crate) const LOCK_FILE_NAME: &str = "library.lock";

/// Guard proving the caller owns the semantic library for both layers.
///
/// Held across recover, reload, optimistic revision check, mutation, durable
/// commit, and cache update, so no other thread or process can interleave.
///
/// Acquisition always materialises the cross-process lock, including the very
/// first use of a library whose semantic-data root does not exist yet. A read
/// that skipped the file lock could otherwise recover the journal — installing
/// or discarding another process's in-flight commit — while that process was
/// still committing. Creating one directory and one empty lock file is the
/// price of that guarantee; nothing else about the library is created, and
/// *construction* stays completely inert.
#[derive(Debug)]
pub struct LibraryLockGuard<'registry> {
    _process: MutexGuard<'registry, ()>,
    file: fs::File,
}

impl Drop for LibraryLockGuard<'_> {
    fn drop(&mut self) {
        // Closing the file releases the advisory lock; unlocking explicitly
        // keeps the intent obvious and surfaces nothing to the caller because
        // a failed unlock is indistinguishable from a closed descriptor.
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

/// Per-path exclusive lock over one semantic-data root.
#[derive(Debug, Clone)]
pub(crate) struct LibraryLock {
    directory: PathBuf,
    process_lock: Arc<Mutex<()>>,
}

impl LibraryLock {
    pub(crate) fn new(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        let process_lock = process_lock_for(&directory);
        Self {
            directory,
            process_lock,
        }
    }

    /// Acquires the process-wide lock and then the cross-process file lock,
    /// creating the semantic-data root and the lock file when they are absent.
    ///
    /// The file lock is never skipped: journal recovery runs on the read path
    /// too, so a lock-free read of a library that is being written for the
    /// first time would race a concurrent committer.
    ///
    /// # Errors
    ///
    /// Returns a filesystem failure, or [`StoreError::UnsafePath`] when the
    /// directory or lock file is not a regular directory/file.
    pub(crate) fn acquire(&self) -> Result<LibraryLockGuard<'_>, StoreError> {
        let process = self
            .process_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match fs::symlink_metadata(&self.directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(StoreError::UnsafePath(self.directory.clone()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(&self.directory)?;
            }
            Err(error) => return Err(error.into()),
        }
        let file = open_locked(&self.directory)?;
        Ok(LibraryLockGuard {
            _process: process,
            file,
        })
    }
}

fn open_locked(directory: &Path) -> Result<fs::File, StoreError> {
    let metadata = fs::symlink_metadata(directory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(StoreError::UnsafePath(directory.to_path_buf()));
    }
    let path = directory.join(LOCK_FILE_NAME);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(StoreError::UnsafePath(path));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(&path)?;
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(StoreError::UnsafePath(path));
    }
    fs2::FileExt::lock_exclusive(&file).map_err(StoreError::Lock)?;
    Ok(file)
}

fn process_lock_for(directory: &Path) -> Arc<Mutex<()>> {
    static PROCESS_LOCKS: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

    let key = if directory.is_absolute() {
        directory.to_owned()
    } else {
        std::env::current_dir()
            .map(|current| current.join(directory))
            .unwrap_or_else(|_| directory.to_owned())
    };
    let mut locks = PROCESS_LOCKS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}
