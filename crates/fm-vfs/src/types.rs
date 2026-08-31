use std::pin::Pin;

use fm_domain::{EntryId, EntrySummary, Location};
use futures::Stream;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::VfsError;

/// Lightweight reference to an entry owned by a provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntryRef {
    /// Stable identifier for the entry.
    pub id: EntryId,
    /// Provider-neutral location of the entry.
    pub location: Location,
}

/// Paging controls for a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListOptions {
    /// Maximum number of entries to return in this page.
    pub page_size: usize,
    /// Opaque token returned by the preceding page, if any.
    pub continuation_token: Option<String>,
}

impl Default for ListOptions {
    fn default() -> Self {
        Self {
            page_size: 256,
            continuation_token: None,
        }
    }
}

/// One page returned by a provider directory listing.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryPage {
    /// Entries in this page.
    pub entries: Vec<EntrySummary>,
    /// Total entries when the provider can determine it cheaply.
    pub total_known_entries: Option<u64>,
    /// Whether another page is available.
    pub has_more: bool,
    /// Opaque token for retrieving the next page.
    pub continuation_token: Option<String>,
}

/// Controls permanent or recoverable removal of an entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RemoveOptions {
    /// Remove a non-empty directory tree.
    pub recursive: bool,
    /// Request recoverable removal through provider trash support.
    pub use_trash: bool,
}

/// Controls creation of a provider write stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteOptions {
    /// Permit replacing an existing destination.
    ///
    /// The safe default is `false`, preventing silent overwrite.
    pub overwrite: bool,
}

/// Controls publication of a fully-written temporary copy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CopyCommitOptions {
    /// Permit atomically replacing an existing regular file.
    pub overwrite: bool,
    /// Preserve timestamps and permissions supported by the provider.
    pub preserve_metadata: bool,
}

/// Streaming reader returned by a provider.
pub type ProviderReadStream = Pin<Box<dyn AsyncRead + Send>>;

/// Streaming writer returned by a provider.
pub type ProviderWriteStream = Pin<Box<dyn AsyncWrite + Send>>;

/// Provider-neutral indication that a watched location may have changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderChange {
    /// One or more filesystem changes were coalesced.
    Changed,
    /// Events were lost and the consumer must replace its snapshot.
    ResetRequired,
}

/// Stream of invalidations observed at a provider location.
pub type ProviderChangeStream =
    Pin<Box<dyn Stream<Item = Result<ProviderChange, VfsError>> + Send>>;
