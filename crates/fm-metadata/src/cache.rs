//! Disk-backed thumbnail cache, content-addressed so a change to the source
//! bytes is automatically a cache miss (task 0134).

use std::io;
use std::path::PathBuf;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::thumbnail::ThumbnailSize;

/// Errors reading or writing the on-disk thumbnail cache.
#[derive(Debug, thiserror::Error)]
pub enum ThumbnailCacheError {
    /// The cache directory could not be created, written to, or read.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

/// A disk-backed store of generated thumbnails, keyed by content hash and
/// requested size, bounded to `max_total_bytes` via oldest-first eviction.
pub struct ThumbnailCache {
    root: PathBuf,
    max_total_bytes: u64,
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl ThumbnailCache {
    /// Creates a cache rooted at `root` (created lazily on first [`Self::put`]),
    /// bounded to `max_total_bytes` on disk.
    pub fn new(root: impl Into<PathBuf>, max_total_bytes: u64) -> Self {
        Self {
            root: root.into(),
            max_total_bytes,
        }
    }

    /// A change to `source_bytes` (even by one byte) or a different
    /// requested `size` produces a different key, so a cache "hit" is only
    /// ever a hit for bytes identical to what's cached now - no separate
    /// invalidation step is needed (task 0134: "cached on disk keyed by
    /// content hash + size, invalidated when the source file changes").
    pub fn cache_key(source_bytes: &[u8], size: ThumbnailSize) -> String {
        let mut hasher = Sha256::new();
        hasher.update(source_bytes);
        format!("{}-{}", to_hex(&hasher.finalize()), size.as_str())
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.jpg"))
    }

    /// Reads a previously cached thumbnail, or `None` on a cache miss (also
    /// used for any I/O error - a miss just re-triggers generation).
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        std::fs::read(self.path_for(key)).ok()
    }

    /// Writes `bytes` under `key`, then evicts the oldest-written entries
    /// until the cache directory is back under `max_total_bytes` - bounds
    /// disk usage when a directory with thousands of images is thumbnailed
    /// (task 0134 acceptance criteria: "...doesn't stall the UI or exhaust
    /// disk cache space").
    pub fn put(&self, key: &str, bytes: &[u8]) -> Result<(), ThumbnailCacheError> {
        std::fs::create_dir_all(&self.root)?;
        let final_path = self.path_for(key);
        let temp_path = self
            .root
            .join(format!("{key}.jpg.tmp-{}", std::process::id()));
        std::fs::write(&temp_path, bytes)?;
        std::fs::rename(&temp_path, &final_path)?;
        self.enforce_budget()
    }

    fn enforce_budget(&self) -> Result<(), ThumbnailCacheError> {
        let mut entries: Vec<(PathBuf, u64, SystemTime)> = std::fs::read_dir(&self.root)?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                if !metadata.is_file() {
                    return None;
                }
                let modified = metadata.modified().ok()?;
                Some((entry.path(), metadata.len(), modified))
            })
            .collect();
        let mut total: u64 = entries.iter().map(|(_, size, _)| *size).sum();
        if total <= self.max_total_bytes {
            return Ok(());
        }
        entries.sort_by_key(|(_, _, modified)| *modified);
        for (path, size, _) in entries {
            if total <= self.max_total_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn cache_key_differs_for_different_content() {
        let a = ThumbnailCache::cache_key(b"one", ThumbnailSize::Small);
        let b = ThumbnailCache::cache_key(b"two", ThumbnailSize::Small);
        assert_ne!(a, b);
    }

    #[test]
    fn cache_key_differs_for_different_size() {
        let small = ThumbnailCache::cache_key(b"same bytes", ThumbnailSize::Small);
        let large = ThumbnailCache::cache_key(b"same bytes", ThumbnailSize::Large);
        assert_ne!(small, large);
    }

    #[test]
    fn cache_key_is_stable_for_identical_input() {
        let first = ThumbnailCache::cache_key(b"same bytes", ThumbnailSize::Medium);
        let second = ThumbnailCache::cache_key(b"same bytes", ThumbnailSize::Medium);
        assert_eq!(first, second);
    }

    #[test]
    fn put_then_get_round_trips() {
        let directory = tempfile::tempdir().expect("temp dir");
        let cache = ThumbnailCache::new(directory.path(), 1024 * 1024);
        let key = ThumbnailCache::cache_key(b"source bytes", ThumbnailSize::Small);
        cache.put(&key, b"thumbnail bytes").expect("put");
        assert_eq!(cache.get(&key), Some(b"thumbnail bytes".to_vec()));
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        let directory = tempfile::tempdir().expect("temp dir");
        let cache = ThumbnailCache::new(directory.path(), 1024 * 1024);
        assert_eq!(cache.get("never-written"), None);
    }

    #[test]
    fn a_changed_source_produces_a_different_key_so_the_old_entry_is_simply_unused() {
        let directory = tempfile::tempdir().expect("temp dir");
        let cache = ThumbnailCache::new(directory.path(), 1024 * 1024);
        let original_key = ThumbnailCache::cache_key(b"original file bytes", ThumbnailSize::Small);
        cache
            .put(&original_key, b"original thumbnail")
            .expect("put original");

        let changed_key = ThumbnailCache::cache_key(b"changed file bytes", ThumbnailSize::Small);
        assert_ne!(original_key, changed_key);
        assert_eq!(
            cache.get(&changed_key),
            None,
            "changed content must miss the cache"
        );
        assert_eq!(
            cache.get(&original_key),
            Some(b"original thumbnail".to_vec()),
            "the stale entry is merely unused, not corrupted"
        );
    }

    #[test]
    fn put_evicts_oldest_entries_once_the_total_size_budget_is_exceeded() {
        let directory = tempfile::tempdir().expect("temp dir");
        // Budget for roughly 2.5 entries of 10 bytes each.
        let cache = ThumbnailCache::new(directory.path(), 25);

        cache.put("oldest", &[0_u8; 10]).expect("put oldest");
        sleep(Duration::from_millis(5));
        cache.put("middle", &[0_u8; 10]).expect("put middle");
        sleep(Duration::from_millis(5));
        cache.put("newest", &[0_u8; 10]).expect("put newest");

        assert_eq!(
            cache.get("oldest"),
            None,
            "oldest entry must be evicted first"
        );
        assert_eq!(cache.get("middle"), Some(vec![0_u8; 10]));
        assert_eq!(cache.get("newest"), Some(vec![0_u8; 10]));

        let total_on_disk: u64 = std::fs::read_dir(directory.path())
            .expect("read cache dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.metadata().expect("metadata").len())
            .sum();
        assert!(
            total_on_disk <= 25,
            "cache directory must stay within budget"
        );
    }
}
