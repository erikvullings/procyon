//! Transfer-planning capabilities (task 0108).
//!
//! [`ProviderCapabilities`] answers "can this provider type do X at all".
//! Cross-provider transfer planning needs a second, finer question: *for this
//! concrete location*, which backend does it live on, and which fast paths are
//! safe against it. A provider id is not enough — two `sftp://` locations may
//! sit on completely different hosts, and a server-side clone between them
//! would silently corrupt or fail. [`TransferCapabilities`] therefore pairs the
//! transfer-relevant capability answers with an opaque [`TransferEndpoint`]
//! that identifies the concrete connection/volume.
//!
//! Consumers must not interpret an endpoint's text; it is only ever compared
//! for equality (see [`TransferCapabilities::shares_endpoint_with`]).

use std::fmt;

use crate::ProviderCapabilities;

/// Opaque identity of the concrete backend one location lives on.
///
/// Two locations may be operated on with a provider-native, same-backend fast
/// path only when their endpoints are equal. Providers choose the text
/// (`"local"`, `"sftp:<connection-id>"`, `"ftps:<connection-id>"`, ...); it is
/// never parsed, only compared.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TransferEndpoint(String);

impl TransferEndpoint {
    /// Creates an endpoint identity from provider-chosen text.
    #[must_use]
    pub fn new(identity: impl Into<String>) -> Self {
        Self(identity.into())
    }

    /// Returns the opaque identity text, for diagnostics only.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TransferEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// What one provider location supports when it takes part in a transfer.
///
/// Every flag is a promise the provider has actually implemented, never an
/// aspiration: the operation planner picks fast paths purely from these, so an
/// over-advertised capability turns into a failed or corrupted transfer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransferCapabilities {
    /// Concrete backend this location lives on.
    pub endpoint: TransferEndpoint,
    /// The provider can duplicate bytes within [`Self::endpoint`] itself,
    /// without streaming them through the application.
    pub server_side_copy: bool,
    /// The provider can relocate an entry within [`Self::endpoint`] itself
    /// (a rename/`MOVE`), without copying its bytes.
    pub server_side_move: bool,
    /// An interrupted upload can be continued from a byte offset rather than
    /// restarted.
    pub resumable_upload: bool,
    /// An interrupted download can be continued from a byte offset rather than
    /// restarted.
    pub resumable_download: bool,
    /// Reads can start at an arbitrary offset (see
    /// [`crate::FileSystemProvider::read_range`]).
    pub random_read: bool,
    /// Writes can be placed at an arbitrary offset rather than only appended
    /// to a freshly truncated destination.
    pub random_write: bool,
}

impl TransferCapabilities {
    /// Derives the conservative default from a provider's static capability
    /// bits.
    ///
    /// Only capabilities with a corresponding [`ProviderCapabilities`] bit are
    /// inferred. Resumability and random writes have no such bit — nothing in
    /// the flag set implies them — so they default to `false` and a provider
    /// that genuinely supports them must say so by overriding
    /// [`crate::FileSystemProvider::transfer_capabilities`].
    #[must_use]
    pub const fn from_provider_capabilities(
        endpoint: TransferEndpoint,
        capabilities: ProviderCapabilities,
    ) -> Self {
        Self {
            endpoint,
            server_side_copy: capabilities.contains(ProviderCapabilities::SERVER_SIDE_COPY),
            server_side_move: capabilities.contains(ProviderCapabilities::MOVE),
            resumable_upload: false,
            resumable_download: false,
            random_read: capabilities.contains(ProviderCapabilities::RANDOM_ACCESS),
            random_write: false,
        }
    }

    /// Reports whether both sides of a transfer sit on the same backend, the
    /// precondition for every provider-native same-backend fast path.
    #[must_use]
    pub fn shares_endpoint_with(&self, other: &Self) -> bool {
        self.endpoint == other.endpoint
    }
}
