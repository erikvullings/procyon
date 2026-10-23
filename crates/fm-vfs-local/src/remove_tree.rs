//! Parallel recursive deletion for the local provider.
//!
//! Deleting tens of thousands of small files is bound by per-file filesystem latency (and, on
//! Windows, by antivirus scanning each handle), not by CPU. Worker threads therefore scan and
//! unlink different directories concurrently. File types and attributes come from the directory
//! enumeration itself (`DirEntry::metadata` costs no extra handle on Windows) instead of a
//! separate `symlink_metadata` open per entry.

use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{
        Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use fm_domain::{EntryId, Location};
use fm_vfs::{EntryRef, RemoveReport};
use tokio_util::sync::CancellationToken;

use crate::is_directory_link;

const MAX_WORKERS: usize = 16;

/// On Windows a directory often cannot be removed for a moment after its last file went: a
/// watcher, the indexer or antivirus still holds a handle, or a deleted file is pending close.
#[cfg(windows)]
fn remove_dir_retrying(path: &Path) -> std::io::Result<()> {
    const ATTEMPTS: u32 = 8;
    let mut attempt = 0;
    loop {
        match fs::remove_dir(path) {
            Err(error)
                if attempt + 1 < ATTEMPTS
                    && (error.kind() == std::io::ErrorKind::PermissionDenied
                        || matches!(error.raw_os_error(), Some(5 | 32 | 145))) =>
            {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(25 << attempt));
            }
            other => return other,
        }
    }
}

#[cfg(not(windows))]
fn remove_dir_retrying(path: &Path) -> std::io::Result<()> {
    fs::remove_dir(path)
}

/// A directory without owner read/write/search permission (mode 000, say) can be neither listed
/// nor emptied, yet its owner may always restore access. Only called once the user has
/// overridden read-only protection; failures are left for the real operation to report.
#[cfg(unix)]
fn grant_owner_access(directory: &Path) {
    use std::os::unix::fs::PermissionsExt;
    const OWNER_ALL: u32 = 0o700;
    if let Ok(metadata) = fs::symlink_metadata(directory) {
        let mode = metadata.permissions().mode();
        if metadata.is_dir() && mode & OWNER_ALL != OWNER_ALL {
            let _ = fs::set_permissions(directory, fs::Permissions::from_mode(mode | OWNER_ALL));
        }
    }
}

#[cfg(not(unix))]
fn grant_owner_access(_directory: &Path) {}

struct Queue {
    directories: VecDeque<PathBuf>,
    /// Directories currently being scanned by a worker.
    active: usize,
}

struct Shared<'a> {
    root_entry: &'a EntryRef,
    override_read_only: bool,
    cancellation: &'a CancellationToken,
    removed: &'a AtomicU64,
    queue: Mutex<Queue>,
    wake: Condvar,
    /// Every directory found, parents before children, so that reverse order empties them.
    found: Mutex<Vec<PathBuf>>,
    failures: Mutex<Vec<(EntryRef, String)>>,
}

impl Shared<'_> {
    fn fail(&self, path: &Path, error: impl std::fmt::Display) {
        let location =
            Location::from_native_path(path).unwrap_or_else(|_| self.root_entry.location.clone());
        let entry = EntryRef {
            id: EntryId::new(),
            location,
        };
        self.failures
            .lock()
            .expect("failures lock")
            .push((entry, format!("{}: {error}", path.display())));
    }

    fn count_removed(&self) {
        self.removed.fetch_add(1, Ordering::Relaxed);
    }

    fn denied_by_read_only(&self, metadata: Option<&fs::Metadata>) -> bool {
        !self.override_read_only
            && metadata.is_some_and(|metadata| metadata.permissions().readonly())
    }

    /// Scans one directory: unlinks its files and queues its subdirectories.
    fn scan(&self, directory: &Path) {
        if self.override_read_only {
            grant_owner_access(directory);
        }
        let children = match fs::read_dir(directory) {
            Ok(children) => children,
            Err(error) => return self.fail(directory, error),
        };
        let mut subdirectories = Vec::new();
        for child in children {
            if self.cancellation.is_cancelled() {
                return;
            }
            let child = match child {
                Ok(child) => child,
                Err(error) => {
                    self.fail(directory, error);
                    continue;
                }
            };
            let path = child.path();
            let file_type = child.file_type();
            // Unix needs the attributes only to honour the read-only guard.
            let metadata = if cfg!(windows) || !self.override_read_only {
                match child.metadata() {
                    Ok(metadata) => Some(metadata),
                    Err(error) => {
                        self.fail(&path, error);
                        continue;
                    }
                }
            } else {
                None
            };
            if self.denied_by_read_only(metadata.as_ref()) {
                self.fail(&path, "read-only entry requires explicit override");
                continue;
            }
            let directory_link = metadata.as_ref().is_some_and(is_directory_link);
            let is_directory = match (&file_type, &metadata) {
                (Ok(file_type), _) => file_type.is_dir(),
                (Err(_), Some(metadata)) => metadata.is_dir(),
                (Err(error), None) => {
                    self.fail(&path, error);
                    continue;
                }
            };
            if is_directory && !directory_link {
                subdirectories.push(path);
                continue;
            }
            let result = if directory_link {
                fs::remove_dir(&path)
            } else {
                fs::remove_file(&path)
            };
            match result {
                Ok(()) => self.count_removed(),
                Err(error) => self.fail(&path, error),
            }
        }
        if subdirectories.is_empty() {
            return;
        }
        self.found
            .lock()
            .expect("found lock")
            .extend(subdirectories.iter().cloned());
        let mut queue = self.queue.lock().expect("queue lock");
        queue.directories.extend(subdirectories);
        self.wake.notify_all();
    }

    fn work(&self) {
        loop {
            let directory = {
                let mut queue = self.queue.lock().expect("queue lock");
                loop {
                    if self.cancellation.is_cancelled() {
                        self.wake.notify_all();
                        return;
                    }
                    if let Some(directory) = queue.directories.pop_front() {
                        queue.active += 1;
                        break directory;
                    }
                    if queue.active == 0 {
                        self.wake.notify_all();
                        return;
                    }
                    queue = self.wake.wait(queue).expect("queue wait");
                }
            };
            self.scan(&directory);
            let mut queue = self.queue.lock().expect("queue lock");
            queue.active -= 1;
            self.wake.notify_all();
        }
    }
}

/// Removes `root` and everything below it, reporting per-entry failures instead of stopping.
pub(crate) fn remove_tree(
    entry: &EntryRef,
    root: PathBuf,
    override_read_only: bool,
    cancellation: &CancellationToken,
    removed: &AtomicU64,
) -> RemoveReport {
    let mut report = RemoveReport {
        removed_items: 0,
        failures: Vec::new(),
        cancelled: false,
    };
    let shared = Shared {
        root_entry: entry,
        override_read_only,
        cancellation,
        removed,
        queue: Mutex::new(Queue {
            directories: VecDeque::new(),
            active: 0,
        }),
        wake: Condvar::new(),
        found: Mutex::new(Vec::new()),
        failures: Mutex::new(Vec::new()),
    };
    let before = removed.load(Ordering::Relaxed);
    let finish = |shared: Shared<'_>, mut report: RemoveReport| {
        report.removed_items = removed.load(Ordering::Relaxed) - before;
        report.failures = shared.failures.into_inner().expect("failures lock");
        report.cancelled = cancellation.is_cancelled();
        report
    };

    let metadata = match fs::symlink_metadata(&root) {
        Ok(metadata) => metadata,
        Err(error) => {
            shared.fail(&root, error);
            return finish(shared, report);
        }
    };
    if shared.denied_by_read_only(Some(&metadata)) {
        shared.fail(&root, "read-only entry requires explicit override");
        return finish(shared, report);
    }
    let directory_link = is_directory_link(&metadata);
    if !metadata.is_dir() || directory_link {
        let result = if directory_link {
            fs::remove_dir(&root)
        } else {
            fs::remove_file(&root)
        };
        match result {
            Ok(()) => shared.count_removed(),
            Err(error) => shared.fail(&root, error),
        }
        return finish(shared, report);
    }

    shared.found.lock().expect("found lock").push(root.clone());
    shared
        .queue
        .lock()
        .expect("queue lock")
        .directories
        .push_back(root);
    let workers = std::thread::available_parallelism()
        .map_or(4, usize::from)
        .clamp(4, MAX_WORKERS);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| shared.work());
        }
    });

    if !cancellation.is_cancelled() {
        // Children come after their parents in `found`, so reverse order empties bottom-up.
        let found = std::mem::take(&mut *shared.found.lock().expect("found lock"));
        for directory in found.iter().rev() {
            if cancellation.is_cancelled() {
                break;
            }
            match remove_dir_retrying(directory) {
                Ok(()) => shared.count_removed(),
                Err(error) => shared.fail(directory, error),
            }
        }
    }
    report.cancelled = cancellation.is_cancelled();
    finish(shared, report)
}
