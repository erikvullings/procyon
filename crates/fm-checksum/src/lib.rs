//! Streaming checksum calculation, checksum-file verification and staged
//! duplicate detection (task 0077; spec §16 milestone 5, §18
//! `core.calculateChecksum`, §37).
//!
//! The crate is deliberately provider-neutral: every byte it hashes arrives
//! through a [`fm_vfs::FileSystemProvider`], so a local file, an entry inside
//! an archive and a remote SFTP file are hashed by one code path. The single
//! exception is hardlink identity in [`duplicates`], which is inherently a
//! local-filesystem concept and degrades to "unknown" everywhere else.
//!
//! Availability is gated by [`fm_vfs::ProviderCapabilities::CHECKSUM`] at the
//! layer that wires this crate into an engine job; nothing here assumes the
//! capability has been checked, but nothing here bypasses a provider either.
//!
//! # Memory
//!
//! Hashing never buffers a whole file: every loop reads into one reusable
//! [`hash::HASH_CHUNK_BYTES`] buffer and feeds each chunk to the hashers
//! incrementally. The integration tests hash a file far larger than the
//! buffer to prove the loop is correct across many chunks; note that they do
//! not *measure* process memory, which is not portably observable from a
//! Rust test — the bounded-buffer guarantee is established by the code
//! itself, which allocates exactly one buffer of a compile-time-bounded size
//! per calculation.

pub mod checksum_file;
pub mod duplicates;
pub mod engine;
pub mod error;
pub mod hash;
pub mod store;

pub use engine::{
    ChecksumEngine, ChecksumEngineError, ChecksumJobOptions, ChecksumTarget, DuplicateScanOptions,
};
pub use store::{
    ChecksumEntryResult, ChecksumPage, ChecksumResultsStore, DuplicatePage, DuplicateResultsStore,
};

pub use checksum_file::{
    ChecksumFileEntry, VerificationResult, VerificationStatus, read_checksum_file, verify,
    write_checksum_file,
};
pub use duplicates::{
    DEFAULT_PARTIAL_HASH_BYTES, DuplicateCandidate, DuplicateGroup, DuplicateObserver,
    DuplicateOptions, DuplicateProgress, DuplicateScan, DuplicateStage, DuplicateStats, FileEntry,
    FileIdentity, HardlinkCluster, ScanOutcome, find_duplicates, find_duplicates_observed,
};
pub use error::{ChecksumError, ChecksumFileError};
pub use hash::{
    ChecksumAlgorithm, ChecksumSet, HASH_CHUNK_BYTES, hash_blocking, hash_blocking_prefix,
    hash_entry, hash_entry_prefix, hash_stream, hash_stream_prefix,
};
