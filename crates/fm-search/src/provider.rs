//! The `search://local/{searchId}` virtual filesystem provider (spec §24).
//!
//! Read-only: only [`ProviderCapabilities::LIST`] is advertised. Watching is
//! deliberately not implemented (returns
//! [`VfsError::UnsupportedCapability`]) since results are a one-shot stream
//! pushed by [`crate::SearchEngine`] over the event bus, not a live
//! directory that changes underneath a listener; capabilities must stay
//! truthful; and `search://` locations do not support the `file:`-specific
//! `Location` helpers, so no other operation is meaningful here either.

use std::sync::Arc;

use async_trait::async_trait;
use fm_domain::{EntryMetadata, Location, ProviderId};
use fm_vfs::{
    DirectoryPage, EntryRef, FileSystemProvider, ListOptions, ProviderCapabilities,
    ProviderChangeStream, ProviderReadStream, ProviderWriteStream, RemoveOptions, VfsError,
    WriteOptions,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::store::SearchResultsStore;

const SEARCH_URI_PREFIX: &str = "search://local/";

/// Exposes accumulated results of one or more running or finished searches
/// as paged directory listings.
pub struct SearchFileSystemProvider {
    store: Arc<SearchResultsStore>,
}

impl SearchFileSystemProvider {
    /// Creates a provider backed by `store`, shared with the
    /// [`crate::SearchEngine`] that populates it.
    #[must_use]
    pub fn new(store: Arc<SearchResultsStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl FileSystemProvider for SearchFileSystemProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new("search")
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::LIST
    }

    async fn list(
        &self,
        location: &Location,
        options: ListOptions,
        cancellation: CancellationToken,
    ) -> Result<DirectoryPage, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        if options.page_size == 0 {
            return Err(VfsError::InvalidLocation {
                location: location.uri.clone(),
            });
        }
        let search_id = parse_search_id(location)?;
        let offset = decode_token(options.continuation_token.as_deref(), location)?;
        let (entries, has_more) = self
            .store
            .page(search_id, offset, options.page_size)
            .ok_or_else(|| VfsError::NotFound {
                location: location.uri.clone(),
            })?;
        let continuation_token = has_more.then(|| (offset + entries.len()).to_string());
        Ok(DirectoryPage {
            entries,
            total_known_entries: None,
            has_more,
            continuation_token,
        })
    }

    async fn metadata(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntryMetadata, VfsError> {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        // The search root itself has no interesting metadata; individual
        // result entries carry a real `file://` location and are resolved
        // through the local provider instead, never through here.
        Ok(EntryMetadata {
            entry_id: entry.id,
            permissions: None,
            ownership: None,
            extended_attributes: Default::default(),
            checksums: Default::default(),
            image_dimensions: None,
            media: None,
            archive: None,
            plugin_fields: Default::default(),
        })
    }

    async fn create_directory(
        &self,
        _location: &Location,
        _name: &str,
        _cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        unsupported(ProviderCapabilities::CREATE_DIRECTORY)
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
        _entry: &EntryRef,
        _options: RemoveOptions,
        _cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        unsupported(ProviderCapabilities::DELETE)
    }

    async fn open_read(
        &self,
        _entry: &EntryRef,
        _cancellation: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError> {
        unsupported(ProviderCapabilities::READ)
    }

    async fn open_write(
        &self,
        _destination: &Location,
        _options: WriteOptions,
        _cancellation: CancellationToken,
    ) -> Result<ProviderWriteStream, VfsError> {
        unsupported(ProviderCapabilities::WRITE)
    }

    async fn watch(
        &self,
        _location: &Location,
        _cancellation: CancellationToken,
    ) -> Result<ProviderChangeStream, VfsError> {
        unsupported(ProviderCapabilities::WATCH)
    }
}

fn parse_search_id(location: &Location) -> Result<Uuid, VfsError> {
    location
        .uri
        .strip_prefix(SEARCH_URI_PREFIX)
        .and_then(|remainder| Uuid::parse_str(remainder).ok())
        .ok_or_else(|| VfsError::InvalidLocation {
            location: location.uri.clone(),
        })
}

fn decode_token(token: Option<&str>, location: &Location) -> Result<usize, VfsError> {
    token.map_or(Ok(0), |value| {
        value.parse().map_err(|_| VfsError::InvalidLocation {
            location: location.uri.clone(),
        })
    })
}

fn unsupported<T>(capability: ProviderCapabilities) -> Result<T, VfsError> {
    Err(VfsError::UnsupportedCapability { capability })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location_for(search_id: Uuid) -> Location {
        Location::new(
            ProviderId::new("search"),
            format!("{SEARCH_URI_PREFIX}{search_id}"),
        )
    }

    #[tokio::test]
    async fn capabilities_report_only_list_since_watch_is_unsupported() {
        let provider = SearchFileSystemProvider::new(Arc::new(SearchResultsStore::new()));
        assert_eq!(provider.capabilities(), ProviderCapabilities::LIST);

        let result = provider
            .watch(&location_for(Uuid::new_v4()), CancellationToken::new())
            .await;
        assert!(matches!(
            result,
            Err(VfsError::UnsupportedCapability {
                capability: ProviderCapabilities::WATCH
            })
        ));
    }

    #[tokio::test]
    async fn list_serves_accumulated_pages_and_rejects_unknown_searches() {
        let store = Arc::new(SearchResultsStore::new());
        let search_id = Uuid::new_v4();
        store.register(search_id, CancellationToken::new());
        let provider = SearchFileSystemProvider::new(Arc::clone(&store));

        let page = provider
            .list(
                &location_for(search_id),
                ListOptions::default(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(page.entries.is_empty());
        assert!(!page.has_more);

        let error = provider
            .list(
                &location_for(Uuid::new_v4()),
                ListOptions::default(),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, VfsError::NotFound { .. }));
    }

    #[tokio::test]
    async fn mutating_operations_report_unsupported_capabilities() {
        let provider = SearchFileSystemProvider::new(Arc::new(SearchResultsStore::new()));
        let location = location_for(Uuid::new_v4());
        let entry = EntryRef {
            id: fm_domain::EntryId::new(),
            location: location.clone(),
        };

        assert!(matches!(
            provider
                .create_directory(&location, "name", CancellationToken::new())
                .await,
            Err(VfsError::UnsupportedCapability {
                capability: ProviderCapabilities::CREATE_DIRECTORY
            })
        ));
        assert!(matches!(
            provider
                .rename(&entry, &location, CancellationToken::new())
                .await,
            Err(VfsError::UnsupportedCapability {
                capability: ProviderCapabilities::RENAME
            })
        ));
        assert!(matches!(
            provider
                .remove(&entry, RemoveOptions::default(), CancellationToken::new())
                .await,
            Err(VfsError::UnsupportedCapability {
                capability: ProviderCapabilities::DELETE
            })
        ));
        assert!(matches!(
            provider.open_read(&entry, CancellationToken::new()).await,
            Err(VfsError::UnsupportedCapability {
                capability: ProviderCapabilities::READ
            })
        ));
        assert!(matches!(
            provider
                .open_write(&location, WriteOptions::default(), CancellationToken::new())
                .await,
            Err(VfsError::UnsupportedCapability {
                capability: ProviderCapabilities::WRITE
            })
        ));
    }
}
