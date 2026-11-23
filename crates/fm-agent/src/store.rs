//! Where skill folders and instruction files are read from. The harness only needs to list a
//! folder, look at a file, read part of it and find a path's real location; [`LocalStore`] does
//! that on the local disk, and a host backs other locations (an SSH connection) with its own
//! implementation. Paths are strings joined with `/`; each store decides what they mean (the local
//! store takes absolute paths, a remote one may take paths relative to its working folder).

use std::fmt;
use std::fs;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;

/// What a path holds, after following links where the store follows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreEntryKind {
    /// A regular file.
    File,
    /// A folder.
    Folder,
    /// Anything else (a device, a link the store does not follow).
    Other,
}

/// One entry of a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreEntry {
    /// The entry's name in its folder.
    pub name: String,
    /// What it is.
    pub kind: StoreEntryKind,
}

/// What a store knows about one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreMetadata {
    /// What it is.
    pub kind: StoreEntryKind,
    /// Its size in bytes (0 when unknown).
    pub len: u64,
    /// When it last changed, when the store knows.
    pub modified: Option<SystemTime>,
}

/// Read access to the folders skills and instruction files live in.
#[async_trait]
pub trait SkillStore: Send + Sync {
    /// Tells this store apart from others (a cache key, and how a host recognises its own).
    fn id(&self) -> &str;
    /// The entries of a folder, unsorted; `None` when it is not a readable folder.
    async fn list(&self, folder: &str) -> Option<Vec<StoreEntry>>;
    /// What `path` is; `None` when it does not exist or cannot be read.
    async fn metadata(&self, path: &str) -> Option<StoreMetadata>;
    /// At most `max` bytes of the file at `path` from byte `offset`.
    async fn read(&self, path: &str, offset: u64, max: usize) -> Option<Vec<u8>>;
    /// The real location of `path`, links resolved, in the store's own path form. A store that
    /// cannot resolve links safely refuses any path that goes through one.
    async fn real_path(&self, path: &str) -> Option<String>;
}

/// A shared store, compared by [`SkillStore::id`].
#[derive(Clone)]
pub struct StoreHandle(pub Arc<dyn SkillStore>);

impl StoreHandle {
    /// A handle on `store`.
    pub fn new(store: impl SkillStore + 'static) -> Self {
        Self(Arc::new(store))
    }

    /// The local disk.
    pub fn local() -> Self {
        Self::new(LocalStore)
    }
}

impl std::ops::Deref for StoreHandle {
    type Target = dyn SkillStore;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl PartialEq for StoreHandle {
    fn eq(&self, other: &Self) -> bool {
        self.0.id() == other.0.id()
    }
}

impl Eq for StoreHandle {}

impl fmt::Debug for StoreHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "StoreHandle({})", self.0.id())
    }
}

/// `relative` under `base`, joined with `/` (an empty base is the store's root).
pub fn join_path(base: &str, relative: &str) -> String {
    let base = base.trim_end_matches(['/', '\\']);
    let relative = relative.trim_start_matches(['/', '\\']);
    match (base.is_empty(), relative.is_empty()) {
        (true, _) => relative.to_owned(),
        (false, true) => base.to_owned(),
        (false, false) => format!("{base}/{relative}"),
    }
}

/// The part of `path` below `base`, when `path` is `base` or lies inside it (`/` and `\` both
/// separate). An empty base contains every relative path.
pub fn relative_to<'a>(base: &str, path: &'a str) -> Option<&'a str> {
    let base = base.trim_end_matches(['/', '\\']);
    if base.is_empty() {
        return (!path.starts_with(['/', '\\'])).then_some(path);
    }
    let rest = path.strip_prefix(base)?;
    if rest.is_empty() {
        return Some("");
    }
    rest.strip_prefix(['/', '\\'])
}

/// The last part of a path.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The local disk, through `std::fs` on the blocking threads.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalStore;

/// The id of [`LocalStore`].
pub const LOCAL_STORE_ID: &str = "local";

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Option<T> + Send + 'static,
) -> Option<T> {
    tokio::task::spawn_blocking(work).await.ok().flatten()
}

fn kind_of(metadata: &fs::Metadata) -> StoreEntryKind {
    if metadata.is_file() {
        StoreEntryKind::File
    } else if metadata.is_dir() {
        StoreEntryKind::Folder
    } else {
        StoreEntryKind::Other
    }
}

/// A canonical path as a plain string: Windows' `\\?\` prefix is dropped so that it still starts
/// with the folders it was joined from.
fn plain(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

#[async_trait]
impl SkillStore for LocalStore {
    fn id(&self) -> &str {
        LOCAL_STORE_ID
    }

    async fn list(&self, folder: &str) -> Option<Vec<StoreEntry>> {
        let folder = folder.to_owned();
        blocking(move || {
            let entries = fs::read_dir(&folder).ok()?;
            Some(
                entries
                    .flatten()
                    .map(|entry| StoreEntry {
                        name: entry.file_name().to_string_lossy().into_owned(),
                        // Links are followed, as the skill folders often are links.
                        kind: fs::metadata(entry.path())
                            .map_or(StoreEntryKind::Other, |metadata| kind_of(&metadata)),
                    })
                    .collect(),
            )
        })
        .await
    }

    async fn metadata(&self, path: &str) -> Option<StoreMetadata> {
        let path = path.to_owned();
        blocking(move || {
            let metadata = fs::metadata(&path).ok()?;
            Some(StoreMetadata {
                kind: kind_of(&metadata),
                len: metadata.len(),
                modified: metadata.modified().ok(),
            })
        })
        .await
    }

    async fn read(&self, path: &str, offset: u64, max: usize) -> Option<Vec<u8>> {
        let path = path.to_owned();
        blocking(move || {
            let mut file = fs::File::open(&path).ok()?;
            file.seek(SeekFrom::Start(offset)).ok()?;
            let mut buffer = Vec::new();
            file.take(max as u64).read_to_end(&mut buffer).ok()?;
            Some(buffer)
        })
        .await
    }

    async fn real_path(&self, path: &str) -> Option<String> {
        let path = path.to_owned();
        blocking(move || {
            Path::new(&path)
                .canonicalize()
                .ok()
                .map(|real| plain(&real))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_join_and_compare_with_either_separator() {
        assert_eq!(join_path("/a/b/", "/c"), "/a/b/c");
        assert_eq!(join_path("", "c/d"), "c/d");
        assert_eq!(join_path("x", ""), "x");
        assert_eq!(relative_to("/a/b", "/a/b/c/d"), Some("c/d"));
        assert_eq!(relative_to(r"C:\a", r"C:\a\c"), Some("c"));
        assert_eq!(relative_to("/a/b", "/a/bc"), None);
        assert_eq!(relative_to("/a/b", "/a/b"), Some(""));
        assert_eq!(relative_to("", "c/d"), Some("c/d"));
        assert_eq!(relative_to("", "/etc"), None);
        assert_eq!(file_name("/a/b/SKILL.md"), "SKILL.md");
    }

    #[tokio::test]
    async fn the_local_store_lists_reads_and_resolves() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::create_dir(folder.path().join("sub")).unwrap();
        std::fs::write(folder.path().join("a.txt"), "hello world").unwrap();
        let base = folder.path().to_string_lossy().into_owned();
        let store = LocalStore;
        let mut entries = store.list(&base).await.unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(entries[0].kind, StoreEntryKind::File);
        assert_eq!(entries[1].kind, StoreEntryKind::Folder);
        let file = join_path(&base, "a.txt");
        assert_eq!(store.read(&file, 6, 3).await.unwrap(), b"wor");
        assert_eq!(store.metadata(&file).await.unwrap().len, 11);
        let real = store.real_path(&file).await.unwrap();
        assert!(real.ends_with("a.txt"));
        assert!(store.metadata(&join_path(&base, "missing")).await.is_none());
    }
}
