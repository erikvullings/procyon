//! Shared accumulator of a streamed comparison's results (spec §16
//! milestone 5, mirrors `fm_search::SearchResultsStore`).
//!
//! Written by the background traversal task started from
//! [`crate::ComparisonEngine::start`] and read by the REST/Tauri paging
//! layer, so a pane can page through whatever has been found so far without
//! waiting for the whole comparison to finish.

use std::collections::HashMap;
use std::sync::Mutex;

use fm_domain::Location;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::model::{ComparisonCriteria, ComparisonEntry, ComparisonStatus};

struct ComparisonState {
    left_root: Location,
    right_root: Location,
    criteria: ComparisonCriteria,
    entries: Vec<ComparisonEntry>,
    complete: bool,
    warnings_count: u32,
    cancellation: CancellationToken,
}

/// One page of a comparison's accumulated results, plus its identifying
/// parameters so callers never need to remember them separately.
#[derive(Debug, Clone, PartialEq)]
pub struct ComparisonPage {
    /// Root compared as "left".
    pub left_root: Location,
    /// Root compared as "right".
    pub right_root: Location,
    /// Criteria used to classify entries.
    pub criteria: ComparisonCriteria,
    /// Entries in this page.
    pub entries: Vec<ComparisonEntry>,
    /// Whether the backend has stopped producing further entries (either
    /// finished or cancelled).
    pub is_complete: bool,
    /// Cumulative count of unreadable directories skipped so far.
    pub warnings_count: u32,
    /// Whether another page exists, or more entries may still arrive.
    pub has_more: bool,
    /// Total entries currently known to match the requested filter.
    pub total: usize,
}

/// Thread-safe, in-memory storage for every currently tracked comparison.
#[derive(Default)]
pub struct ComparisonResultsStore {
    comparisons: Mutex<HashMap<Uuid, ComparisonState>>,
}

impl ComparisonResultsStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new comparison, ready to receive batches.
    pub(crate) fn register(
        &self,
        comparison_id: Uuid,
        left_root: Location,
        right_root: Location,
        criteria: ComparisonCriteria,
        cancellation: CancellationToken,
    ) {
        self.lock().insert(
            comparison_id,
            ComparisonState {
                left_root,
                right_root,
                criteria,
                entries: Vec::new(),
                complete: false,
                warnings_count: 0,
                cancellation,
            },
        );
    }

    /// Appends a batch of newly compared entries and updates the cumulative
    /// unreadable-directory warning count.
    ///
    /// A no-op if `comparison_id` is unknown (for example, already evicted).
    pub(crate) fn append(
        &self,
        comparison_id: Uuid,
        mut entries: Vec<ComparisonEntry>,
        warnings_count: u32,
    ) {
        if let Some(state) = self.lock().get_mut(&comparison_id) {
            state.entries.append(&mut entries);
            state.warnings_count = warnings_count;
        }
    }

    /// Marks a comparison as finished; no further batches will arrive.
    pub(crate) fn mark_complete(&self, comparison_id: Uuid) {
        if let Some(state) = self.lock().get_mut(&comparison_id) {
            state.complete = true;
        }
    }

    /// Requests prompt cancellation of a running comparison's traversal.
    ///
    /// Returns `None` if no comparison with this id is currently tracked.
    #[must_use]
    pub fn cancel(&self, comparison_id: Uuid) -> Option<()> {
        let guard = self.lock();
        let state = guard.get(&comparison_id)?;
        state.cancellation.cancel();
        Some(())
    }

    /// Returns the left and right roots a comparison was started with.
    ///
    /// Used to resolve a sync-plan item's source/destination locations
    /// without paging through its (potentially large) result set.
    ///
    /// Returns `None` if no comparison with this id is currently tracked.
    #[must_use]
    pub fn roots(&self, comparison_id: Uuid) -> Option<(Location, Location)> {
        let guard = self.lock();
        let state = guard.get(&comparison_id)?;
        Some((state.left_root.clone(), state.right_root.clone()))
    }

    /// Returns the full set of entries accumulated so far, ignoring paging.
    ///
    /// Used by sync-plan generation, which needs every entry rather than one
    /// page of them.
    ///
    /// Returns `None` if no comparison with this id is currently tracked.
    #[must_use]
    pub fn all_entries(&self, comparison_id: Uuid) -> Option<Vec<ComparisonEntry>> {
        let guard = self.lock();
        let state = guard.get(&comparison_id)?;
        Some(state.entries.clone())
    }

    /// Returns one page of accumulated results, optionally restricted to
    /// entries whose status is not [`ComparisonStatus::Identical`].
    ///
    /// Returns `None` if no comparison with this id is currently tracked.
    #[must_use]
    pub fn page(
        &self,
        comparison_id: Uuid,
        offset: usize,
        limit: usize,
        differences_only: bool,
    ) -> Option<ComparisonPage> {
        let guard = self.lock();
        let state = guard.get(&comparison_id)?;
        let filtered: Vec<&ComparisonEntry> = state
            .entries
            .iter()
            .filter(|entry| !differences_only || entry.status != ComparisonStatus::Identical)
            .collect();
        let page: Vec<ComparisonEntry> = filtered
            .iter()
            .skip(offset)
            .take(limit)
            .map(|entry| (*entry).clone())
            .collect();
        let has_more = filtered.len() > offset + page.len() || !state.complete;
        Some(ComparisonPage {
            left_root: state.left_root.clone(),
            right_root: state.right_root.clone(),
            criteria: state.criteria,
            entries: page,
            is_complete: state.complete,
            warnings_count: state.warnings_count,
            has_more,
            total: filtered.len(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, ComparisonState>> {
        self.comparisons
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ComparisonEntrySide, ComparisonStatus};
    use fm_domain::{EntryKind, ProviderId};

    fn roots() -> (Location, Location) {
        (
            Location::new(ProviderId::new("local"), "file:///left"),
            Location::new(ProviderId::new("local"), "file:///right"),
        )
    }

    fn entry(name: &str, status: ComparisonStatus) -> ComparisonEntry {
        ComparisonEntry {
            relative_path: name.to_owned(),
            left: Some(ComparisonEntrySide {
                kind: EntryKind::File,
                size: Some(1),
                modified_at: None,
                content_hash: None,
            }),
            right: Some(ComparisonEntrySide {
                kind: EntryKind::File,
                size: Some(1),
                modified_at: None,
                content_hash: None,
            }),
            status,
        }
    }

    #[test]
    fn unknown_comparison_ids_report_no_results() {
        let store = ComparisonResultsStore::new();
        assert!(store.page(Uuid::new_v4(), 0, 10, false).is_none());
        assert!(store.cancel(Uuid::new_v4()).is_none());
        assert!(store.all_entries(Uuid::new_v4()).is_none());
    }

    #[test]
    fn has_more_is_true_while_incomplete_even_with_an_empty_page() {
        let store = ComparisonResultsStore::new();
        let id = Uuid::new_v4();
        let (left, right) = roots();
        store.register(
            id,
            left,
            right,
            ComparisonCriteria::NameOnly,
            CancellationToken::new(),
        );

        let page = store.page(id, 0, 10, false).unwrap();
        assert!(page.entries.is_empty());
        assert!(page.has_more);

        store.mark_complete(id);
        let page = store.page(id, 0, 10, false).unwrap();
        assert!(page.entries.is_empty());
        assert!(!page.has_more);
    }

    #[test]
    fn pages_through_appended_batches_in_order() {
        let store = ComparisonResultsStore::new();
        let id = Uuid::new_v4();
        let (left, right) = roots();
        store.register(
            id,
            left,
            right,
            ComparisonCriteria::NameOnly,
            CancellationToken::new(),
        );
        store.append(
            id,
            vec![
                entry("a", ComparisonStatus::Identical),
                entry("b", ComparisonStatus::Identical),
            ],
            0,
        );
        store.append(id, vec![entry("c", ComparisonStatus::Newer)], 1);
        store.mark_complete(id);

        let first = store.page(id, 0, 2, false).unwrap();
        assert_eq!(
            first
                .entries
                .iter()
                .map(|e| e.relative_path.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(first.has_more);
        assert_eq!(first.warnings_count, 1);

        let second = store.page(id, 2, 2, false).unwrap();
        assert_eq!(
            second
                .entries
                .iter()
                .map(|e| e.relative_path.as_str())
                .collect::<Vec<_>>(),
            ["c"]
        );
        assert!(!second.has_more);
        assert_eq!(second.total, 3);
    }

    #[test]
    fn differences_only_filter_excludes_identical_entries_and_reflows_paging() {
        let store = ComparisonResultsStore::new();
        let id = Uuid::new_v4();
        let (left, right) = roots();
        store.register(
            id,
            left,
            right,
            ComparisonCriteria::NameOnly,
            CancellationToken::new(),
        );
        store.append(
            id,
            vec![
                entry("a", ComparisonStatus::Identical),
                entry("b", ComparisonStatus::Newer),
                entry("c", ComparisonStatus::Identical),
                entry("d", ComparisonStatus::OnlyLeft),
            ],
            0,
        );
        store.mark_complete(id);

        let filtered = store.page(id, 0, 10, true).unwrap();
        assert_eq!(
            filtered
                .entries
                .iter()
                .map(|e| e.relative_path.as_str())
                .collect::<Vec<_>>(),
            ["b", "d"]
        );
        assert_eq!(filtered.total, 2);
        assert!(!filtered.has_more);
    }

    #[test]
    fn cancel_signals_the_registered_cancellation_token() {
        let store = ComparisonResultsStore::new();
        let id = Uuid::new_v4();
        let (left, right) = roots();
        let cancellation = CancellationToken::new();
        store.register(
            id,
            left,
            right,
            ComparisonCriteria::NameOnly,
            cancellation.clone(),
        );

        assert!(store.cancel(id).is_some());
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn roots_returns_the_left_and_right_registered_roots() {
        let store = ComparisonResultsStore::new();
        let id = Uuid::new_v4();
        let (left, right) = roots();
        store.register(
            id,
            left.clone(),
            right.clone(),
            ComparisonCriteria::NameOnly,
            CancellationToken::new(),
        );

        assert_eq!(store.roots(id), Some((left, right)));
        assert_eq!(store.roots(Uuid::new_v4()), None);
    }

    #[test]
    fn all_entries_returns_every_accumulated_entry_regardless_of_status() {
        let store = ComparisonResultsStore::new();
        let id = Uuid::new_v4();
        let (left, right) = roots();
        store.register(
            id,
            left,
            right,
            ComparisonCriteria::NameOnly,
            CancellationToken::new(),
        );
        store.append(
            id,
            vec![
                entry("a", ComparisonStatus::Identical),
                entry("b", ComparisonStatus::Newer),
            ],
            0,
        );

        let all = store.all_entries(id).unwrap();
        assert_eq!(all.len(), 2);
    }
}
