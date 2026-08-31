//! Shared accumulator of streamed search results (spec §24, §28).
//!
//! Written by the background traversal task started from
//! [`crate::SearchEngine::start`] and read by
//! [`crate::SearchFileSystemProvider::list`], so a pane can page through
//! whatever has been found so far without waiting for the whole traversal.

use std::collections::HashMap;
use std::sync::Mutex;

use fm_domain::EntrySummary;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct SearchState {
    entries: Vec<EntrySummary>,
    warnings_count: u32,
    cancellation: CancellationToken,
}

/// Thread-safe, in-memory storage for every currently tracked search.
///
/// Shared (via `Arc`) between a [`crate::SearchEngine`], which writes
/// batches as traversal progresses, and a
/// [`crate::SearchFileSystemProvider`], which serves paged listings from
/// whatever has been accumulated so far.
#[derive(Default)]
pub struct SearchResultsStore {
    searches: Mutex<HashMap<Uuid, SearchState>>,
}

impl SearchResultsStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new search, ready to receive batches.
    pub(crate) fn register(&self, search_id: Uuid, cancellation: CancellationToken) {
        self.lock().insert(
            search_id,
            SearchState {
                entries: Vec::new(),
                warnings_count: 0,
                cancellation,
            },
        );
    }

    /// Appends a batch of newly discovered entries and updates the
    /// cumulative unreadable-directory warning count.
    ///
    /// A no-op if `search_id` is unknown (for example, already evicted).
    pub(crate) fn append(
        &self,
        search_id: Uuid,
        mut entries: Vec<EntrySummary>,
        warnings_count: u32,
    ) {
        if let Some(state) = self.lock().get_mut(&search_id) {
            state.entries.append(&mut entries);
            state.warnings_count = warnings_count;
        }
    }

    /// Requests prompt cancellation of a running search's traversal.
    ///
    /// Returns `None` if no search with this id is currently tracked.
    #[must_use]
    pub fn cancel(&self, search_id: Uuid) -> Option<()> {
        let guard = self.lock();
        let state = guard.get(&search_id)?;
        state.cancellation.cancel();
        Some(())
    }

    /// Returns one page of accumulated results and whether another buffered
    /// page is available. Results appended later are announced separately by
    /// `search.resultsBatch` events and must not keep a directory listing open.
    ///
    /// Returns `None` if no search with this id is currently tracked.
    #[must_use]
    pub fn page(
        &self,
        search_id: Uuid,
        offset: usize,
        limit: usize,
    ) -> Option<(Vec<EntrySummary>, bool)> {
        let guard = self.lock();
        let state = guard.get(&search_id)?;
        let page: Vec<EntrySummary> = state
            .entries
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();
        let has_more = state.entries.len() > offset + page.len();
        Some((page, has_more))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, SearchState>> {
        self.searches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_domain::{EntryId, EntryKind, Location, ProviderId};

    fn sample_entry(name: &str) -> EntrySummary {
        EntrySummary {
            id: EntryId::new(),
            location: Location::new(ProviderId::new("local"), format!("file:///tmp/{name}")),
            name: name.to_owned(),
            kind: EntryKind::File,
            size: Some(0),
            modified_at: None,
            created_at: None,
            hidden: false,
            read_only: false,
            extension: None,
            mime_type: None,
            icon_key: None,
            metadata_revision: 0,
            git_status: None,
        }
    }

    #[test]
    fn unknown_search_ids_report_no_results() {
        let store = SearchResultsStore::new();
        assert!(store.page(Uuid::new_v4(), 0, 10).is_none());
        assert!(store.cancel(Uuid::new_v4()).is_none());
    }

    #[test]
    fn incomplete_search_does_not_advertise_results_that_are_not_buffered_yet() {
        let store = SearchResultsStore::new();
        let id = Uuid::new_v4();
        store.register(id, CancellationToken::new());

        let (page, has_more) = store.page(id, 0, 10).unwrap();
        assert!(page.is_empty());
        assert!(!has_more);

        store.append(id, vec![sample_entry("match")], 0);
        let (page, has_more) = store.page(id, 0, 10).unwrap();
        assert_eq!(page.len(), 1);
        assert!(!has_more);
    }

    #[test]
    fn cancelling_a_search_keeps_its_accumulated_results_available() {
        let store = SearchResultsStore::new();
        let id = Uuid::new_v4();
        store.register(id, CancellationToken::new());
        store.append(id, vec![sample_entry("match")], 0);

        assert!(store.cancel(id).is_some());
        let (page, has_more) = store.page(id, 0, 10).unwrap();
        assert_eq!(page.len(), 1);
        assert!(!has_more);
    }

    #[test]
    fn pages_through_appended_batches_in_order() {
        let store = SearchResultsStore::new();
        let id = Uuid::new_v4();
        store.register(id, CancellationToken::new());
        store.append(id, vec![sample_entry("a"), sample_entry("b")], 1);
        store.append(id, vec![sample_entry("c")], 2);

        let (first, has_more) = store.page(id, 0, 2).unwrap();
        assert_eq!(
            first
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(has_more);

        let (second, has_more) = store.page(id, 2, 2).unwrap();
        assert_eq!(
            second
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["c"]
        );
        assert!(!has_more);
    }

    #[test]
    fn cancel_signals_the_registered_cancellation_token() {
        let store = SearchResultsStore::new();
        let id = Uuid::new_v4();
        let cancellation = CancellationToken::new();
        store.register(id, cancellation.clone());

        assert!(store.cancel(id).is_some());
        assert!(cancellation.is_cancelled());
    }
}
