//! Staged duplicate detection (task 0077).
//!
//! Duplicates are found in three stages, each of which is strictly cheaper
//! than the next, so the expensive one only ever sees the files that survived
//! the cheap ones:
//!
//! 1. **Group by size.** Two files of different length cannot have the same
//!    content, so a size that occurs exactly once is discarded and *never
//!    hashed at all*.
//! 2. **Partial hash.** Within a surviving size group, the first
//!    [`DEFAULT_PARTIAL_HASH_BYTES`] of each file are hashed. Files that
//!    disagree on their prefix cannot be identical, so they are discarded
//!    *without ever reading the rest of the file*.
//! 3. **Full hash.** Only the files still sharing both a size and a prefix
//!    hash are streamed in full, through
//!    [`crate::hash::hash_entry`]'s bounded-chunk loop.
//!
//! Hardlinks are reported separately from true duplicates: two paths that
//! resolve to the same `(device, inode)` are the *same* file rather than two
//! copies wasting space, and deleting one of them frees nothing. Each
//! [`DuplicateGroup`] therefore separates [`DuplicateGroup::hardlink_clusters`]
//! from [`DuplicateGroup::distinct_files`]. Because a hardlink cluster's
//! members are literally one file, the full hash is computed once per
//! identity and reused across the cluster.
//!
//! Hashing is provider-neutral (it goes through
//! [`fm_vfs::ProviderRegistry`]), but identity is not: it is obtained
//! through [`std::os::unix::fs::MetadataExt`] on Unix and
//! [`std::os::windows::fs::MetadataExt`] on Windows. On any other platform,
//! and for any non-local provider, identity is reported as unknown and
//! every file is treated as a distinct file — the detector never *claims*
//! a hardlink it cannot prove.

use std::collections::HashMap;

use fm_vfs::{EntryRef, ProviderRegistry};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::error::ChecksumError;
use crate::hash::{ChecksumAlgorithm, hash_entry, hash_entry_prefix};

/// Bytes of each file's prefix hashed in stage 2.
///
/// 64 KiB matches [`crate::hash::HASH_CHUNK_BYTES`], so a partial hash costs
/// exactly one read of one buffer, and is wide enough to separate files that
/// merely share a common header (archive magic bytes, media containers,
/// office documents) which a 4 KiB prefix would not.
pub const DEFAULT_PARTIAL_HASH_BYTES: u64 = 64 * 1024;

/// A file offered to the detector.
///
/// The caller supplies the size it already knows from listing the tree, so
/// stage 1 costs no I/O at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateCandidate {
    /// The entry to hash, addressed through its provider.
    pub entry: EntryRef,
    /// Exact byte size of the entry.
    pub size: u64,
}

impl DuplicateCandidate {
    /// Creates a candidate for `entry` of `size` bytes.
    #[must_use]
    pub const fn new(entry: EntryRef, size: u64) -> Self {
        Self { entry, size }
    }
}

/// A file's on-disk identity: two paths sharing one are the same file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileIdentity {
    /// Device the file lives on.
    pub device: u64,
    /// Inode number within that device.
    pub inode: u64,
}

impl FileIdentity {
    /// Resolves the identity of a local entry, or `None` when it cannot be
    /// determined (a non-local provider, a stat failure, or a platform
    /// without inode semantics).
    #[must_use]
    pub fn of(entry: &EntryRef) -> Option<Self> {
        let path = entry.location.to_native_path().ok()?;
        Self::of_path(&path)
    }

    /// Resolves a local path's stable filesystem identity.
    #[cfg(unix)]
    #[must_use]
    pub fn of_path(path: &std::path::Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    /// On Windows, the NTFS file ID plays the role of `(device, inode)`:
    /// the volume serial number identifies the volume and the file index
    /// identifies the file on it, and both survive hardlinking. Queried via
    /// the raw Win32 API rather than
    /// `std::os::windows::fs::MetadataExt::{volume_serial_number,file_index}`,
    /// which are gated behind the unstable `windows_by_handle` feature
    /// (rust-lang/rust#63010).
    #[cfg(windows)]
    #[must_use]
    #[allow(unsafe_code)]
    pub fn of_path(path: &std::path::Path) -> Option<Self> {
        use std::os::windows::ffi::OsStrExt as _;

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
            let inode = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
            Some(Self {
                device: u64::from(info.dwVolumeSerialNumber),
                inode,
            })
        }
    }

    /// Platforms other than Unix and Windows expose no portable inode number
    /// through `std`, so identity is reported as unknown rather than
    /// guessed. Every file is then treated as distinct, which is the safe
    /// direction: the detector under-reports hardlinks instead of falsely
    /// claiming them.
    #[cfg(not(any(unix, windows)))]
    #[must_use]
    pub fn of_path(_path: &std::path::Path) -> Option<Self> {
        None
    }
}

/// One file within a reported duplicate group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// The entry itself.
    pub entry: EntryRef,
    /// Byte size shared by the whole group.
    pub size: u64,
    /// On-disk identity, when it could be determined.
    pub identity: Option<FileIdentity>,
}

/// Two or more paths that are the same file through a hardlink.
///
/// Deleting one member of a cluster reclaims no space; the UI presents these
/// distinctly from true duplicates for exactly that reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardlinkCluster {
    /// The `(device, inode)` every member shares.
    pub identity: FileIdentity,
    /// The paths pointing at that one file — always two or more.
    pub files: Vec<FileEntry>,
}

/// A set of byte-identical files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGroup {
    /// Full-content digest shared by every member.
    pub full_hash: String,
    /// Byte size shared by every member.
    pub size: u64,
    /// Groups of paths that are the same file through a hardlink.
    pub hardlink_clusters: Vec<HardlinkCluster>,
    /// Files with distinct identities (or unknown identity) whose content is
    /// nevertheless identical — the true, space-wasting duplicates.
    pub distinct_files: Vec<FileEntry>,
}

impl DuplicateGroup {
    /// Total number of paths in the group, across both categories.
    #[must_use]
    pub fn path_count(&self) -> usize {
        self.distinct_files.len()
            + self
                .hardlink_clusters
                .iter()
                .map(|cluster| cluster.files.len())
                .sum::<usize>()
    }

    /// Bytes that could be reclaimed by keeping one copy of the content.
    ///
    /// A hardlink cluster counts once however many paths it has, because its
    /// paths share one allocation.
    #[must_use]
    pub fn reclaimable_bytes(&self) -> u64 {
        let allocations = self.distinct_files.len() as u64 + self.hardlink_clusters.len() as u64;
        self.size * allocations.saturating_sub(1)
    }
}

/// Whether a scan ran to completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScanOutcome {
    /// Every stage finished; [`DuplicateScan::groups`] is exhaustive.
    Completed,
    /// The cancellation token fired. [`DuplicateScan::groups`] holds only the
    /// groups that were fully resolved beforehand and must never be presented
    /// as a complete answer.
    Cancelled,
}

/// Counters describing how much work each stage actually performed.
///
/// These are the evidence that the staging works: `fully_hashed` is normally
/// far smaller than `candidates`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateStats {
    /// Files handed to the detector.
    pub candidates: usize,
    /// Files that survived stage 1 (their size occurred more than once).
    pub size_survivors: usize,
    /// Files whose prefix was read in stage 2.
    pub partially_hashed: usize,
    /// Files streamed in full in stage 3.
    pub fully_hashed: usize,
    /// Total bytes fed through a hasher across stages 2 and 3.
    pub bytes_hashed: u64,
    /// Files skipped because they could not be opened or hashed.
    pub failed: usize,
}

/// The result of one duplicate scan.
#[derive(Debug, Clone)]
pub struct DuplicateScan {
    /// Whether the scan completed or was cancelled.
    pub outcome: ScanOutcome,
    /// Duplicate groups found, each with two or more paths.
    pub groups: Vec<DuplicateGroup>,
    /// How much work each stage performed.
    pub stats: DuplicateStats,
    /// Human-readable notes about files that had to be skipped.
    pub warnings: Vec<String>,
}

impl DuplicateScan {
    /// Whether the scan produced an exhaustive answer.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self.outcome, ScanOutcome::Completed)
    }
}

/// Tuning for a duplicate scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateOptions {
    /// Algorithm used for both the partial and the full hash.
    pub algorithm: ChecksumAlgorithm,
    /// Bytes of each file's prefix hashed in stage 2.
    pub partial_hash_bytes: u64,
    /// Whether zero-byte files participate. They are all trivially identical
    /// and are almost never what a user means by "duplicate", so they are
    /// excluded by default.
    pub include_empty_files: bool,
}

impl Default for DuplicateOptions {
    fn default() -> Self {
        Self {
            // BLAKE3 is the fastest of the four at equivalent strength, and
            // nothing outside this scan consumes the digest.
            algorithm: ChecksumAlgorithm::Blake3,
            partial_hash_bytes: DEFAULT_PARTIAL_HASH_BYTES,
            include_empty_files: false,
        }
    }
}

/// Which stage a progress report concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DuplicateStage {
    /// Grouping candidates by size — no I/O.
    GroupBySize,
    /// Hashing bounded prefixes.
    PartialHash,
    /// Streaming full contents.
    FullHash,
}

/// A progress report emitted between files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateProgress {
    /// The stage that produced this report.
    pub stage: DuplicateStage,
    /// Files completed in this stage so far.
    pub files_processed: usize,
    /// Files this stage will process in total.
    pub files_total: usize,
}

/// Receives progress reports so a caller can drive a progress bar — or, in a
/// test, cancel at an exactly known point.
pub trait DuplicateObserver: Send + Sync {
    /// Called after each file a stage completes.
    fn on_progress(&self, progress: DuplicateProgress);
}

/// Runs a staged duplicate scan over `candidates`.
///
/// Never returns an error: an individual file that cannot be opened is
/// recorded in [`DuplicateScan::warnings`] and skipped, because one
/// unreadable file must not discard the whole scan.
pub async fn find_duplicates(
    providers: &ProviderRegistry,
    candidates: Vec<DuplicateCandidate>,
    options: &DuplicateOptions,
    cancellation: &CancellationToken,
) -> DuplicateScan {
    find_duplicates_observed(providers, candidates, options, None, cancellation).await
}

/// [`find_duplicates`] with a progress observer attached.
pub async fn find_duplicates_observed(
    providers: &ProviderRegistry,
    candidates: Vec<DuplicateCandidate>,
    options: &DuplicateOptions,
    observer: Option<&dyn DuplicateObserver>,
    cancellation: &CancellationToken,
) -> DuplicateScan {
    let mut stats = DuplicateStats {
        candidates: candidates.len(),
        ..DuplicateStats::default()
    };
    let mut warnings = Vec::new();

    // Stage 1: group by exact size. A size seen once is dropped here and its
    // file is never opened.
    let size_groups = group_by_size(candidates, options, &mut stats);
    report(
        observer,
        DuplicateStage::GroupBySize,
        stats.size_survivors,
        stats.size_survivors,
    );
    if cancellation.is_cancelled() {
        return cancelled(stats, warnings);
    }

    // Stage 2: partial hash within each size group.
    let Some(prefix_groups) = partial_hash_stage(
        providers,
        size_groups,
        options,
        observer,
        &mut stats,
        &mut warnings,
        cancellation,
    )
    .await
    else {
        return cancelled(stats, warnings);
    };

    // Stage 3: full hash of what remains.
    let Some(groups) = full_hash_stage(
        providers,
        prefix_groups,
        options,
        observer,
        &mut stats,
        &mut warnings,
        cancellation,
    )
    .await
    else {
        return cancelled(stats, warnings);
    };

    DuplicateScan {
        outcome: ScanOutcome::Completed,
        groups,
        stats,
        warnings,
    }
}

fn cancelled(stats: DuplicateStats, warnings: Vec<String>) -> DuplicateScan {
    DuplicateScan {
        outcome: ScanOutcome::Cancelled,
        // Deliberately empty: a half-finished stage cannot produce a group
        // that is safe to act on, and a truncated list presented as results
        // would invite deleting a file whose partner had not been found yet.
        groups: Vec::new(),
        stats,
        warnings,
    }
}

fn report(
    observer: Option<&dyn DuplicateObserver>,
    stage: DuplicateStage,
    files_processed: usize,
    files_total: usize,
) {
    if let Some(observer) = observer {
        observer.on_progress(DuplicateProgress {
            stage,
            files_processed,
            files_total,
        });
    }
}

/// Stage 1: buckets candidates by size, keeping only buckets of two or more.
fn group_by_size(
    candidates: Vec<DuplicateCandidate>,
    options: &DuplicateOptions,
    stats: &mut DuplicateStats,
) -> Vec<Vec<DuplicateCandidate>> {
    let mut by_size: HashMap<u64, Vec<DuplicateCandidate>> = HashMap::new();
    for candidate in candidates {
        if candidate.size == 0 && !options.include_empty_files {
            continue;
        }
        by_size.entry(candidate.size).or_default().push(candidate);
    }
    let mut groups: Vec<Vec<DuplicateCandidate>> = by_size
        .into_values()
        .filter(|group| group.len() > 1)
        .collect();
    // Sorting keeps the scan order — and therefore progress reporting and
    // test expectations — deterministic despite the hash map above.
    groups.sort_by_key(|group| std::cmp::Reverse(group.first().map_or(0, |first| first.size)));
    stats.size_survivors = groups.iter().map(Vec::len).sum();
    groups
}

/// A candidate with its resolved identity, carried through stages 2 and 3.
struct Resolved {
    candidate: DuplicateCandidate,
    identity: Option<FileIdentity>,
}

#[allow(clippy::too_many_arguments)]
async fn partial_hash_stage(
    providers: &ProviderRegistry,
    size_groups: Vec<Vec<DuplicateCandidate>>,
    options: &DuplicateOptions,
    observer: Option<&dyn DuplicateObserver>,
    stats: &mut DuplicateStats,
    warnings: &mut Vec<String>,
    cancellation: &CancellationToken,
) -> Option<Vec<Vec<Resolved>>> {
    let total = stats.size_survivors;
    let mut processed = 0_usize;
    let mut survivors: Vec<Vec<Resolved>> = Vec::new();

    for group in size_groups {
        // Within one size group, files sharing an identity are the same file;
        // hashing one of them is enough for the whole cluster.
        let mut by_prefix: HashMap<String, Vec<Resolved>> = HashMap::new();
        let mut digest_by_identity: HashMap<FileIdentity, String> = HashMap::new();

        for candidate in group {
            if cancellation.is_cancelled() {
                return None;
            }
            let identity = FileIdentity::of(&candidate.entry);
            let cached = identity.and_then(|identity| digest_by_identity.get(&identity).cloned());
            let digest = match cached {
                Some(digest) => digest,
                None => {
                    let result = hash_entry_prefix(
                        providers,
                        &candidate.entry,
                        &[options.algorithm],
                        options.partial_hash_bytes,
                        cancellation,
                    )
                    .await;
                    match take_digest(result, options.algorithm, &candidate, stats, warnings) {
                        Outcome::Digest(digest) => {
                            stats.partially_hashed += 1;
                            if let Some(identity) = identity {
                                digest_by_identity.insert(identity, digest.clone());
                            }
                            digest
                        }
                        Outcome::Skipped => {
                            processed += 1;
                            continue;
                        }
                        Outcome::Cancelled => return None,
                    }
                }
            };
            processed += 1;
            report(observer, DuplicateStage::PartialHash, processed, total);
            by_prefix.entry(digest).or_default().push(Resolved {
                candidate,
                identity,
            });
        }

        survivors.extend(
            by_prefix
                .into_values()
                .filter(|bucket| bucket.len() > 1)
                .map(|mut bucket| {
                    bucket.sort_by(|left, right| {
                        left.candidate
                            .entry
                            .location
                            .uri
                            .cmp(&right.candidate.entry.location.uri)
                    });
                    bucket
                }),
        );
    }
    Some(survivors)
}

#[allow(clippy::too_many_arguments)]
async fn full_hash_stage(
    providers: &ProviderRegistry,
    prefix_groups: Vec<Vec<Resolved>>,
    options: &DuplicateOptions,
    observer: Option<&dyn DuplicateObserver>,
    stats: &mut DuplicateStats,
    warnings: &mut Vec<String>,
    cancellation: &CancellationToken,
) -> Option<Vec<DuplicateGroup>> {
    let total: usize = prefix_groups.iter().map(Vec::len).sum();
    let mut processed = 0_usize;
    let mut groups: Vec<DuplicateGroup> = Vec::new();

    for group in prefix_groups {
        let mut by_digest: HashMap<String, Vec<Resolved>> = HashMap::new();
        let mut digest_by_identity: HashMap<FileIdentity, String> = HashMap::new();

        for resolved in group {
            if cancellation.is_cancelled() {
                return None;
            }
            let cached = resolved
                .identity
                .and_then(|identity| digest_by_identity.get(&identity).cloned());
            let digest = match cached {
                Some(digest) => digest,
                None => {
                    let result = hash_entry(
                        providers,
                        &resolved.candidate.entry,
                        &[options.algorithm],
                        cancellation,
                    )
                    .await;
                    match take_digest(
                        result,
                        options.algorithm,
                        &resolved.candidate,
                        stats,
                        warnings,
                    ) {
                        Outcome::Digest(digest) => {
                            stats.fully_hashed += 1;
                            if let Some(identity) = resolved.identity {
                                digest_by_identity.insert(identity, digest.clone());
                            }
                            digest
                        }
                        Outcome::Skipped => {
                            processed += 1;
                            continue;
                        }
                        Outcome::Cancelled => return None,
                    }
                }
            };
            processed += 1;
            report(observer, DuplicateStage::FullHash, processed, total);
            by_digest.entry(digest).or_default().push(resolved);
        }

        for (full_hash, members) in by_digest {
            if members.len() < 2 {
                continue;
            }
            groups.push(build_group(full_hash, members));
        }
    }

    groups.sort_by(|left, right| {
        right
            .size
            .cmp(&left.size)
            .then_with(|| left.full_hash.cmp(&right.full_hash))
    });
    Some(groups)
}

/// Splits byte-identical members into hardlink clusters and distinct files.
fn build_group(full_hash: String, members: Vec<Resolved>) -> DuplicateGroup {
    let size = members.first().map_or(0, |first| first.candidate.size);
    let mut by_identity: HashMap<FileIdentity, Vec<FileEntry>> = HashMap::new();
    let mut unknown: Vec<FileEntry> = Vec::new();

    for member in members {
        let file = FileEntry {
            entry: member.candidate.entry,
            size: member.candidate.size,
            identity: member.identity,
        };
        match member.identity {
            Some(identity) => by_identity.entry(identity).or_default().push(file),
            None => unknown.push(file),
        }
    }

    let mut hardlink_clusters = Vec::new();
    let mut distinct_files = unknown;
    for (identity, files) in by_identity {
        if files.len() > 1 {
            hardlink_clusters.push(HardlinkCluster { identity, files });
        } else {
            distinct_files.extend(files);
        }
    }

    sort_files(&mut distinct_files);
    for cluster in &mut hardlink_clusters {
        sort_files(&mut cluster.files);
    }
    hardlink_clusters.sort_by_key(|cluster| (cluster.identity.device, cluster.identity.inode));

    DuplicateGroup {
        full_hash,
        size,
        hardlink_clusters,
        distinct_files,
    }
}

fn sort_files(files: &mut [FileEntry]) {
    files.sort_by(|left, right| left.entry.location.uri.cmp(&right.entry.location.uri));
}

/// What happened to one file's hash attempt.
enum Outcome {
    Digest(String),
    Skipped,
    Cancelled,
}

fn take_digest(
    result: Result<crate::hash::ChecksumSet, ChecksumError>,
    algorithm: ChecksumAlgorithm,
    candidate: &DuplicateCandidate,
    stats: &mut DuplicateStats,
    warnings: &mut Vec<String>,
) -> Outcome {
    match result {
        Ok(set) => {
            stats.bytes_hashed += set.bytes_hashed();
            match set.get(algorithm) {
                Some(digest) => Outcome::Digest(digest.to_owned()),
                None => {
                    stats.failed += 1;
                    warnings.push(format!(
                        "{}: no {algorithm} digest was produced",
                        candidate.entry.location.uri
                    ));
                    Outcome::Skipped
                }
            }
        }
        Err(ChecksumError::Cancelled) => Outcome::Cancelled,
        Err(error) => {
            stats.failed += 1;
            warnings.push(format!("{}: {error}", candidate.entry.location.uri));
            Outcome::Skipped
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_domain::{EntryId, Location, ProviderId};

    fn candidate(uri: &str, size: u64) -> DuplicateCandidate {
        DuplicateCandidate::new(
            EntryRef {
                id: EntryId::new(),
                location: Location::new(ProviderId::new("local"), uri),
            },
            size,
        )
    }

    #[test]
    fn drops_every_size_that_occurs_only_once() {
        let mut stats = DuplicateStats::default();
        let groups = group_by_size(
            vec![
                candidate("file:///a", 10),
                candidate("file:///b", 10),
                candidate("file:///c", 99),
            ],
            &DuplicateOptions::default(),
            &mut stats,
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
        assert_eq!(stats.size_survivors, 2);
    }

    #[test]
    fn excludes_empty_files_unless_asked_for_them() {
        let mut stats = DuplicateStats::default();
        let excluded = group_by_size(
            vec![candidate("file:///a", 0), candidate("file:///b", 0)],
            &DuplicateOptions::default(),
            &mut stats,
        );
        assert!(excluded.is_empty());

        let options = DuplicateOptions {
            include_empty_files: true,
            ..DuplicateOptions::default()
        };
        let included = group_by_size(
            vec![candidate("file:///a", 0), candidate("file:///b", 0)],
            &options,
            &mut stats,
        );
        assert_eq!(included.len(), 1);
    }

    #[test]
    fn separates_hardlinked_paths_from_distinct_duplicates() {
        let shared = FileIdentity {
            device: 1,
            inode: 42,
        };
        let members = vec![
            Resolved {
                candidate: candidate("file:///link-a", 8),
                identity: Some(shared),
            },
            Resolved {
                candidate: candidate("file:///link-b", 8),
                identity: Some(shared),
            },
            Resolved {
                candidate: candidate("file:///copy", 8),
                identity: Some(FileIdentity {
                    device: 1,
                    inode: 43,
                }),
            },
        ];
        let group = build_group("deadbeef".to_owned(), members);
        assert_eq!(group.hardlink_clusters.len(), 1);
        assert_eq!(group.hardlink_clusters[0].files.len(), 2);
        assert_eq!(group.distinct_files.len(), 1);
        assert_eq!(group.path_count(), 3);
        // Two allocations (the cluster plus the copy) means one is reclaimable.
        assert_eq!(group.reclaimable_bytes(), 8);
    }

    #[test]
    fn treats_unknown_identity_as_a_distinct_file() {
        let members = vec![
            Resolved {
                candidate: candidate("sftp://host/a", 4),
                identity: None,
            },
            Resolved {
                candidate: candidate("sftp://host/b", 4),
                identity: None,
            },
        ];
        let group = build_group("cafe".to_owned(), members);
        assert!(group.hardlink_clusters.is_empty());
        assert_eq!(group.distinct_files.len(), 2);
    }
}
