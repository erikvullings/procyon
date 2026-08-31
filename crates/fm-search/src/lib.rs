//! Filesystem search (task 0068).
//!
//! Streams results as they are found rather than collecting them, and exposes
//! a completed search as a `search://` virtual location so the existing panes
//! can render it unchanged.

mod engine;
mod matcher;
mod provider;
mod scanner;
mod store;

pub use engine::{
    ProviderSearchLimitation, SearchEngine, SearchError, SearchOptions, SearchStart,
    UnevaluatedPredicate,
};
pub use matcher::{MatchMode, detect_match_mode, matches_name};
pub use provider::SearchFileSystemProvider;
pub use scanner::{FileScanError, FileScanResult, scan_file};
pub use store::SearchResultsStore;
