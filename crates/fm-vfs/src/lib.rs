//! The virtual filesystem provider abstraction (task 0016).
//!
//! Keeping the engine behind a provider trait is what allows archives, search
//! results and remote filesystems to be added later without redesigning the
//! core. Providers advertise what they can do through capability flags rather
//! than failing at call time.

mod capabilities;
mod change_tracking;
mod content;
mod error;
mod provider;
mod registry;
mod transfer;
mod types;

pub use capabilities::ProviderCapabilities;
pub use change_tracking::{CONSERVATIVE_POLL_INTERVAL, ChangeTracking};
pub use content::{
    ContentMatch, ContentQuery, ContentQueryError, ContentSearchOutcome, looks_like_binary,
    search_content,
};
pub use error::VfsError;
pub use provider::FileSystemProvider;
pub use registry::ProviderRegistry;
pub use transfer::{TransferCapabilities, TransferEndpoint};
pub use types::{
    CopyCommitOptions, DirectoryPage, EntryRef, ListOptions, ProviderChange, ProviderChangeStream,
    ProviderReadStream, ProviderWriteStream, RemoveOptions, WriteOptions,
};
