//! Shared accumulators for streamed checksum and duplicate-scan results
//! (task 0077; mirrors `fm_comparison::ComparisonResultsStore` and
//! `fm_search::SearchResultsStore`).
//!
//! Written by the background task started from [`crate::ChecksumEngine`] and
//! read by the REST/Tauri paging layer, so the UI can page through whatever
//! has been computed so far without waiting for the whole job to finish.

use std::collections::HashMap;
use std::sync::Mutex;

use fm_domain::Location;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::duplicates::{DuplicateGroup, DuplicateStats};
use crate::hash::{ChecksumAlgorithm, ChecksumSet};

/// One entry's computed checksums, or the reason it has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksumEntryResult {
    /// Location of the hashed entry.
    pub location: Location,
    /// Path relative to the request's common root, for display and for
    /// writing a checksum file.
    pub relative_path: String,
    /// Byte size actually hashed.
    pub size: u64,
    /// Digests computed for this entry, empty when `error` is set.
    pub checksums: ChecksumSet,
    /// Why this entry could not be hashed, when it could not be.
    pub error: Option<String>,
}

struct ChecksumJobState {
    algorithms: Vec<ChecksumAlgorithm>,
    entries: Vec<ChecksumEntryResult>,
    complete: bool,
    cancelled: bool,
    total_entries: usize,
    cancellation: CancellationToken,
}

/// One page of a checksum job's accumulated results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecksumPage {
    /// Algorithms the job was started with.
    pub algorithms: Vec<ChecksumAlgorithm>,
    /// Entries in this page.
    pub entries: Vec<ChecksumEntryResult>,
    /// Whether the backend has stopped producing entries.
    pub is_complete: bool,
    /// Whether the job stopped because it was cancelled.
    pub is_cancelled: bool,
    /// Whether another page exists, or more entries may still arrive.
    pub has_more: bool,
    /// Entries computed so far.
    pub total: usize,
    /// Entries the job was asked to hash.
    pub total_entries: usize,
}

/// Thread-safe, in-memory storage for every tracked checksum job.
#[derive(Default)]
pub struct ChecksumResultsStore {
    jobs: Mutex<HashMap<Uuid, ChecksumJobState>>,
}

impl ChecksumResultsStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register(
        &self,
        job_id: Uuid,
        algorithms: Vec<ChecksumAlgorithm>,
        total_entries: usize,
        cancellation: CancellationToken,
    ) {
        self.lock().insert(
            job_id,
            ChecksumJobState {
                algorithms,
                entries: Vec::new(),
                complete: false,
                cancelled: false,
                total_entries,
                cancellation,
            },
        );
    }

    pub(crate) fn append(&self, job_id: Uuid, mut entries: Vec<ChecksumEntryResult>) {
        if let Some(state) = self.lock().get_mut(&job_id) {
            state.entries.append(&mut entries);
        }
    }

    pub(crate) fn mark_complete(&self, job_id: Uuid, cancelled: bool) {
        if let Some(state) = self.lock().get_mut(&job_id) {
            state.complete = true;
            state.cancelled = cancelled;
        }
    }

    /// Requests prompt cancellation of a running job.
    ///
    /// Returns `None` if no job with this id is currently tracked.
    #[must_use]
    pub fn cancel(&self, job_id: Uuid) -> Option<()> {
        let guard = self.lock();
        guard.get(&job_id)?.cancellation.cancel();
        Some(())
    }

    /// Returns every entry accumulated so far, ignoring paging.
    ///
    /// Used when saving a checksum file or verifying against one, both of
    /// which need the whole result set rather than one page.
    ///
    /// Returns `None` if no job with this id is currently tracked.
    #[must_use]
    pub fn all_entries(&self, job_id: Uuid) -> Option<Vec<ChecksumEntryResult>> {
        let guard = self.lock();
        Some(guard.get(&job_id)?.entries.clone())
    }

    /// Returns the algorithms a job was started with.
    ///
    /// Returns `None` if no job with this id is currently tracked.
    #[must_use]
    pub fn algorithms(&self, job_id: Uuid) -> Option<Vec<ChecksumAlgorithm>> {
        let guard = self.lock();
        Some(guard.get(&job_id)?.algorithms.clone())
    }

    /// Returns one page of accumulated results.
    ///
    /// Returns `None` if no job with this id is currently tracked.
    #[must_use]
    pub fn page(&self, job_id: Uuid, offset: usize, limit: usize) -> Option<ChecksumPage> {
        let guard = self.lock();
        let state = guard.get(&job_id)?;
        let page: Vec<ChecksumEntryResult> = state
            .entries
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();
        let has_more = state.entries.len() > offset + page.len() || !state.complete;
        Some(ChecksumPage {
            algorithms: state.algorithms.clone(),
            entries: page,
            is_complete: state.complete,
            is_cancelled: state.cancelled,
            has_more,
            total: state.entries.len(),
            total_entries: state.total_entries,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, ChecksumJobState>> {
        // A poisoned lock only means some other thread panicked while holding
        // it; the map itself is still structurally sound, so recovering keeps
        // one panicking job from disabling every other one.
        self.jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

struct DuplicateScanState {
    roots: Vec<Location>,
    groups: Vec<DuplicateGroup>,
    complete: bool,
    cancelled: bool,
    stats: DuplicateStats,
    warnings: Vec<String>,
    cancellation: CancellationToken,
}

/// One page of a duplicate scan's grouped results.
#[derive(Debug, Clone)]
pub struct DuplicatePage {
    /// Roots the scan covered.
    pub roots: Vec<Location>,
    /// Groups in this page.
    pub groups: Vec<DuplicateGroup>,
    /// Whether the scan has finished producing groups.
    pub is_complete: bool,
    /// Whether the scan stopped because it was cancelled. Groups are empty
    /// in that case: a cancelled scan never presents partial results.
    pub is_cancelled: bool,
    /// Whether another page exists.
    pub has_more: bool,
    /// Total groups found.
    pub total: usize,
    /// How much work each stage performed.
    pub stats: DuplicateStats,
    /// Notes about files that had to be skipped.
    pub warnings: Vec<String>,
}

/// Thread-safe, in-memory storage for every tracked duplicate scan.
#[derive(Default)]
pub struct DuplicateResultsStore {
    scans: Mutex<HashMap<Uuid, DuplicateScanState>>,
}

impl DuplicateResultsStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register(
        &self,
        scan_id: Uuid,
        roots: Vec<Location>,
        cancellation: CancellationToken,
    ) {
        self.lock().insert(
            scan_id,
            DuplicateScanState {
                roots,
                groups: Vec::new(),
                complete: false,
                cancelled: false,
                stats: DuplicateStats::default(),
                warnings: Vec::new(),
                cancellation,
            },
        );
    }

    pub(crate) fn finish(
        &self,
        scan_id: Uuid,
        groups: Vec<DuplicateGroup>,
        stats: DuplicateStats,
        warnings: Vec<String>,
        cancelled: bool,
    ) {
        if let Some(state) = self.lock().get_mut(&scan_id) {
            state.groups = groups;
            state.stats = stats;
            state.warnings = warnings;
            state.complete = true;
            state.cancelled = cancelled;
        }
    }

    /// Requests prompt cancellation of a running scan.
    ///
    /// Returns `None` if no scan with this id is currently tracked.
    #[must_use]
    pub fn cancel(&self, scan_id: Uuid) -> Option<()> {
        let guard = self.lock();
        guard.get(&scan_id)?.cancellation.cancel();
        Some(())
    }

    /// Returns one page of grouped results.
    ///
    /// Returns `None` if no scan with this id is currently tracked.
    #[must_use]
    pub fn page(&self, scan_id: Uuid, offset: usize, limit: usize) -> Option<DuplicatePage> {
        let guard = self.lock();
        let state = guard.get(&scan_id)?;
        let page: Vec<DuplicateGroup> = state
            .groups
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();
        let has_more = state.groups.len() > offset + page.len() || !state.complete;
        Some(DuplicatePage {
            roots: state.roots.clone(),
            groups: page,
            is_complete: state.complete,
            is_cancelled: state.cancelled,
            has_more,
            total: state.groups.len(),
            stats: state.stats,
            warnings: state.warnings.clone(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, DuplicateScanState>> {
        self.scans
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_domain::ProviderId;

    fn location(uri: &str) -> Location {
        Location::new(ProviderId::new("local"), uri)
    }

    fn entry(uri: &str) -> ChecksumEntryResult {
        ChecksumEntryResult {
            location: location(uri),
            relative_path: uri.to_owned(),
            size: 1,
            checksums: ChecksumSet::default(),
            error: None,
        }
    }

    #[test]
    fn pages_checksum_results_and_reports_more_until_complete() {
        let store = ChecksumResultsStore::new();
        let id = Uuid::new_v4();
        store.register(
            id,
            vec![ChecksumAlgorithm::Sha256],
            3,
            CancellationToken::new(),
        );
        store.append(id, vec![entry("a"), entry("b")]);

        let page = store.page(id, 0, 1).expect("job must be tracked");
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.total, 2);
        assert_eq!(page.total_entries, 3);
        assert!(page.has_more, "a second entry is already buffered");
        assert!(!page.is_complete);

        // Even a fully drained page reports more while the job is running.
        let drained = store.page(id, 0, 10).expect("job must be tracked");
        assert!(drained.has_more, "more entries may still arrive");

        store.mark_complete(id, false);
        let final_page = store.page(id, 0, 10).expect("job must be tracked");
        assert!(!final_page.has_more);
        assert!(final_page.is_complete);
        assert!(!final_page.is_cancelled);
    }

    #[test]
    fn reports_an_unknown_job_as_absent() {
        let store = ChecksumResultsStore::new();
        assert!(store.page(Uuid::new_v4(), 0, 10).is_none());
        assert!(store.cancel(Uuid::new_v4()).is_none());
        assert!(store.all_entries(Uuid::new_v4()).is_none());
    }

    #[test]
    fn cancelling_a_checksum_job_fires_its_token() {
        let store = ChecksumResultsStore::new();
        let id = Uuid::new_v4();
        let token = CancellationToken::new();
        store.register(id, vec![ChecksumAlgorithm::Sha256], 1, token.clone());
        assert!(store.cancel(id).is_some());
        assert!(token.is_cancelled());
    }

    #[test]
    fn a_cancelled_duplicate_scan_is_flagged_and_carries_no_groups() {
        let store = DuplicateResultsStore::new();
        let id = Uuid::new_v4();
        store.register(id, vec![location("file:///root")], CancellationToken::new());
        store.finish(id, Vec::new(), DuplicateStats::default(), Vec::new(), true);

        let page = store.page(id, 0, 10).expect("scan must be tracked");
        assert!(page.is_complete);
        assert!(page.is_cancelled);
        assert!(page.groups.is_empty());
        assert!(!page.has_more);
    }
}
