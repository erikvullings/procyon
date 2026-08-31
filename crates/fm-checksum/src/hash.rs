//! Streaming checksum calculation (task 0077, spec §18 `core.calculateChecksum`).
//!
//! Every entry point here reads its source in bounded [`HASH_CHUNK_BYTES`]
//! chunks and feeds each chunk to the requested hashers incrementally, so a
//! multi-gigabyte file is hashed with a fixed, small amount of memory —
//! matching `fm_comparison::engine`'s `hash_entry`, which this module
//! generalises to several algorithms at once.
//!
//! Two flavours of the same loop are provided deliberately:
//!
//! * [`hash_stream`] / [`hash_stream_prefix`] are `async` and consume any
//!   [`tokio::io::AsyncRead`], which is what a
//!   [`fm_vfs::FileSystemProvider`] hands back from `open_read`. This is the
//!   provider-neutral path used for remote and archive entries.
//! * [`hash_blocking`] / [`hash_blocking_prefix`] consume any
//!   [`std::io::Read`]. They exist for callers that already hold a
//!   `std::fs::File` (or are inside `spawn_blocking`) and would otherwise pay
//!   for an async wrapper around a plainly synchronous read.
//!
//! Both flavours share [`Hashers`], so an algorithm is implemented once.

use std::collections::BTreeMap;
use std::io::Read;

use fm_domain::Location;
use fm_vfs::{EntryRef, ProviderRegistry};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

use crate::error::ChecksumError;

/// Bytes read per chunk while streaming a checksum, so a large file is never
/// loaded into memory at once.
pub const HASH_CHUNK_BYTES: usize = 64 * 1024;

/// A checksum algorithm the application can compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChecksumAlgorithm {
    /// SHA-256, the default and the algorithm used for content comparison.
    Sha256,
    /// BLAKE3, considerably faster than SHA-256 at the same security level.
    Blake3,
    /// CRC-32 (IEEE), for compatibility with archive and download manifests.
    Crc32,
    /// MD5, for compatibility with legacy `md5sum` manifests only.
    Md5,
}

impl ChecksumAlgorithm {
    /// Every algorithm, in the order they are presented to the user.
    pub const ALL: [Self; 4] = [Self::Sha256, Self::Blake3, Self::Crc32, Self::Md5];

    /// The canonical lower-case name, matching the coreutils tool names
    /// (`sha256`, `md5`) where one exists.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Blake3 => "blake3",
            Self::Crc32 => "crc32",
            Self::Md5 => "md5",
        }
    }

    /// Number of hex characters a digest of this algorithm occupies.
    #[must_use]
    pub const fn hex_len(self) -> usize {
        match self {
            Self::Sha256 | Self::Blake3 => 64,
            Self::Crc32 => 8,
            Self::Md5 => 32,
        }
    }

    /// Parses a canonical algorithm name, case-insensitively.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let lowered = name.to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|algorithm| algorithm.as_str() == lowered)
    }
}

impl std::fmt::Display for ChecksumAlgorithm {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The digests computed for one entry, keyed by algorithm.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecksumSet {
    digests: BTreeMap<ChecksumAlgorithm, String>,
    bytes_hashed: u64,
}

impl ChecksumSet {
    /// Returns the lower-case hex digest computed for `algorithm`, if it was
    /// requested.
    #[must_use]
    pub fn get(&self, algorithm: ChecksumAlgorithm) -> Option<&str> {
        self.digests.get(&algorithm).map(String::as_str)
    }

    /// Number of source bytes fed into the hashers.
    #[must_use]
    pub const fn bytes_hashed(&self) -> u64 {
        self.bytes_hashed
    }

    /// Iterates the computed digests in algorithm order.
    pub fn iter(&self) -> impl Iterator<Item = (ChecksumAlgorithm, &str)> {
        self.digests
            .iter()
            .map(|(algorithm, digest)| (*algorithm, digest.as_str()))
    }

    /// Number of algorithms computed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.digests.len()
    }

    /// Whether no digest was computed at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.digests.is_empty()
    }
}

/// The set of incremental hashers backing one calculation.
///
/// Held as individual `Option`s rather than boxed trait objects: the four
/// algorithms have incompatible APIs (`crc32fast` and `blake3` are not
/// `digest::Digest` implementors), and this keeps the hot `update` path free
/// of dynamic dispatch.
struct Hashers {
    sha256: Option<Sha256>,
    blake3: Option<Box<blake3::Hasher>>,
    crc32: Option<crc32fast::Hasher>,
    md5: Option<md5::Md5>,
    bytes: u64,
}

impl Hashers {
    fn new(algorithms: &[ChecksumAlgorithm]) -> Result<Self, ChecksumError> {
        if algorithms.is_empty() {
            return Err(ChecksumError::NoAlgorithmRequested);
        }
        Ok(Self {
            sha256: algorithms
                .contains(&ChecksumAlgorithm::Sha256)
                .then(Sha256::new),
            blake3: algorithms
                .contains(&ChecksumAlgorithm::Blake3)
                .then(|| Box::new(blake3::Hasher::new())),
            crc32: algorithms
                .contains(&ChecksumAlgorithm::Crc32)
                .then(crc32fast::Hasher::new),
            md5: algorithms
                .contains(&ChecksumAlgorithm::Md5)
                .then(md5::Md5::new),
            bytes: 0,
        })
    }

    fn update(&mut self, chunk: &[u8]) {
        if let Some(hasher) = self.sha256.as_mut() {
            hasher.update(chunk);
        }
        if let Some(hasher) = self.blake3.as_mut() {
            hasher.update(chunk);
        }
        if let Some(hasher) = self.crc32.as_mut() {
            hasher.update(chunk);
        }
        if let Some(hasher) = self.md5.as_mut() {
            hasher.update(chunk);
        }
        self.bytes += chunk.len() as u64;
    }

    fn finish(self) -> ChecksumSet {
        let mut digests = BTreeMap::new();
        if let Some(hasher) = self.sha256 {
            digests.insert(ChecksumAlgorithm::Sha256, to_hex(&hasher.finalize()));
        }
        if let Some(hasher) = self.blake3 {
            digests.insert(
                ChecksumAlgorithm::Blake3,
                to_hex(hasher.finalize().as_bytes()),
            );
        }
        if let Some(hasher) = self.crc32 {
            digests.insert(
                ChecksumAlgorithm::Crc32,
                to_hex(&hasher.finalize().to_be_bytes()),
            );
        }
        if let Some(hasher) = self.md5 {
            digests.insert(ChecksumAlgorithm::Md5, to_hex(&hasher.finalize()));
        }
        ChecksumSet {
            digests,
            bytes_hashed: self.bytes,
        }
    }
}

/// Renders bytes as lower-case hex, the representation every checksum tool
/// uses.
fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            // Writing into a `String` is infallible, so the result is discarded
            // deliberately rather than propagated.
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// How many bytes of the current chunk still fall within `limit`.
///
/// Returns `None` once the limit is reached, which ends the read loop.
fn chunk_budget(limit: Option<u64>, already: u64, read: usize) -> Option<usize> {
    match limit {
        None => Some(read),
        Some(limit) => {
            let remaining = limit.saturating_sub(already);
            if remaining == 0 {
                None
            } else {
                Some(usize::try_from(remaining).unwrap_or(usize::MAX).min(read))
            }
        }
    }
}

/// Streams `reader` to completion, computing every requested digest.
///
/// # Errors
///
/// Returns [`ChecksumError::Cancelled`] if `cancellation` fires before the
/// stream is consumed, [`ChecksumError::NoAlgorithmRequested`] if
/// `algorithms` is empty, or [`ChecksumError::Read`] on an I/O failure.
pub async fn hash_stream<R>(
    reader: R,
    algorithms: &[ChecksumAlgorithm],
    cancellation: &CancellationToken,
) -> Result<ChecksumSet, ChecksumError>
where
    R: AsyncRead + Unpin,
{
    hash_stream_bounded(reader, algorithms, None, cancellation).await
}

/// Streams at most `max_bytes` of `reader`, computing every requested digest
/// over that prefix only.
///
/// This is the cheap discriminator used by stage 2 of duplicate detection.
///
/// # Errors
///
/// As [`hash_stream`].
pub async fn hash_stream_prefix<R>(
    reader: R,
    algorithms: &[ChecksumAlgorithm],
    max_bytes: u64,
    cancellation: &CancellationToken,
) -> Result<ChecksumSet, ChecksumError>
where
    R: AsyncRead + Unpin,
{
    hash_stream_bounded(reader, algorithms, Some(max_bytes), cancellation).await
}

async fn hash_stream_bounded<R>(
    mut reader: R,
    algorithms: &[ChecksumAlgorithm],
    limit: Option<u64>,
    cancellation: &CancellationToken,
) -> Result<ChecksumSet, ChecksumError>
where
    R: AsyncRead + Unpin,
{
    let mut hashers = Hashers::new(algorithms)?;
    if limit == Some(0) {
        return Ok(hashers.finish());
    }
    let mut buffer = vec![0_u8; HASH_CHUNK_BYTES];
    loop {
        if cancellation.is_cancelled() {
            return Err(ChecksumError::Cancelled);
        }
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let Some(usable) = chunk_budget(limit, hashers.bytes, read) else {
            break;
        };
        hashers.update(&buffer[..usable]);
        if limit.is_some_and(|limit| hashers.bytes >= limit) {
            break;
        }
    }
    Ok(hashers.finish())
}

/// Synchronous counterpart of [`hash_stream`] for callers holding a
/// [`std::io::Read`] such as a `std::fs::File`.
///
/// # Errors
///
/// As [`hash_stream`].
pub fn hash_blocking<R>(
    reader: R,
    algorithms: &[ChecksumAlgorithm],
    cancellation: &CancellationToken,
) -> Result<ChecksumSet, ChecksumError>
where
    R: Read,
{
    hash_blocking_bounded(reader, algorithms, None, cancellation)
}

/// Synchronous counterpart of [`hash_stream_prefix`].
///
/// # Errors
///
/// As [`hash_stream`].
pub fn hash_blocking_prefix<R>(
    reader: R,
    algorithms: &[ChecksumAlgorithm],
    max_bytes: u64,
    cancellation: &CancellationToken,
) -> Result<ChecksumSet, ChecksumError>
where
    R: Read,
{
    hash_blocking_bounded(reader, algorithms, Some(max_bytes), cancellation)
}

fn hash_blocking_bounded<R>(
    mut reader: R,
    algorithms: &[ChecksumAlgorithm],
    limit: Option<u64>,
    cancellation: &CancellationToken,
) -> Result<ChecksumSet, ChecksumError>
where
    R: Read,
{
    let mut hashers = Hashers::new(algorithms)?;
    if limit == Some(0) {
        return Ok(hashers.finish());
    }
    let mut buffer = vec![0_u8; HASH_CHUNK_BYTES];
    loop {
        if cancellation.is_cancelled() {
            return Err(ChecksumError::Cancelled);
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let Some(usable) = chunk_budget(limit, hashers.bytes, read) else {
            break;
        };
        hashers.update(&buffer[..usable]);
        if limit.is_some_and(|limit| hashers.bytes >= limit) {
            break;
        }
    }
    Ok(hashers.finish())
}

/// Opens `entry` through its registered provider and streams its content
/// through every requested hasher.
///
/// Provider-neutral by construction: nothing here assumes a local file, so a
/// remote or in-archive entry is hashed by exactly the same code path.
///
/// # Errors
///
/// Returns [`ChecksumError::Open`] if the provider cannot open the entry, and
/// otherwise as [`hash_stream`].
pub async fn hash_entry(
    providers: &ProviderRegistry,
    entry: &EntryRef,
    algorithms: &[ChecksumAlgorithm],
    cancellation: &CancellationToken,
) -> Result<ChecksumSet, ChecksumError> {
    let reader = open_entry(providers, entry, cancellation).await?;
    hash_stream(reader, algorithms, cancellation).await
}

/// Opens `entry` and hashes at most `max_bytes` of its leading content.
///
/// # Errors
///
/// As [`hash_entry`].
pub async fn hash_entry_prefix(
    providers: &ProviderRegistry,
    entry: &EntryRef,
    algorithms: &[ChecksumAlgorithm],
    max_bytes: u64,
    cancellation: &CancellationToken,
) -> Result<ChecksumSet, ChecksumError> {
    let reader = open_entry(providers, entry, cancellation).await?;
    hash_stream_prefix(reader, algorithms, max_bytes, cancellation).await
}

async fn open_entry(
    providers: &ProviderRegistry,
    entry: &EntryRef,
    cancellation: &CancellationToken,
) -> Result<fm_vfs::ProviderReadStream, ChecksumError> {
    let location: &Location = &entry.location;
    let provider = providers
        .resolve(location)
        .map_err(|error| ChecksumError::open(location, error))?;
    provider
        .open_read(entry, cancellation.clone())
        .await
        .map_err(|error| ChecksumError::open(location, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_bytes_as_lower_case_hex() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
    }

    #[test]
    fn parses_algorithm_names_case_insensitively() {
        assert_eq!(
            ChecksumAlgorithm::from_name("SHA256"),
            Some(ChecksumAlgorithm::Sha256)
        );
        assert_eq!(
            ChecksumAlgorithm::from_name("blake3"),
            Some(ChecksumAlgorithm::Blake3)
        );
        assert_eq!(ChecksumAlgorithm::from_name("sha512"), None);
    }

    #[test]
    fn reports_the_hex_width_of_each_algorithm() {
        for algorithm in ChecksumAlgorithm::ALL {
            let set = hash_blocking(std::io::empty(), &[algorithm], &CancellationToken::new())
                .expect("hashing an empty reader must succeed");
            let digest = set.get(algorithm).expect("digest must be present");
            assert_eq!(digest.len(), algorithm.hex_len(), "for {algorithm}");
        }
    }

    #[test]
    fn rejects_an_empty_algorithm_request() {
        let error = hash_blocking(std::io::empty(), &[], &CancellationToken::new())
            .expect_err("an empty request must be rejected");
        assert!(matches!(error, ChecksumError::NoAlgorithmRequested));
    }

    #[test]
    fn stops_at_the_prefix_limit_without_reading_the_rest() {
        let data = vec![b'x'; HASH_CHUNK_BYTES * 3];
        let prefix = hash_blocking_prefix(
            data.as_slice(),
            &[ChecksumAlgorithm::Sha256],
            10,
            &CancellationToken::new(),
        )
        .expect("prefix hashing must succeed");
        assert_eq!(prefix.bytes_hashed(), 10);

        let expected = hash_blocking(
            &data[..10],
            &[ChecksumAlgorithm::Sha256],
            &CancellationToken::new(),
        )
        .expect("hashing must succeed");
        assert_eq!(
            prefix.get(ChecksumAlgorithm::Sha256),
            expected.get(ChecksumAlgorithm::Sha256)
        );
    }

    #[test]
    fn a_prefix_limit_of_zero_hashes_nothing() {
        let set = hash_blocking_prefix(
            [1_u8, 2, 3].as_slice(),
            &[ChecksumAlgorithm::Sha256],
            0,
            &CancellationToken::new(),
        )
        .expect("a zero-length prefix must succeed");
        assert_eq!(set.bytes_hashed(), 0);
        assert_eq!(
            set.get(ChecksumAlgorithm::Sha256),
            Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
    }

    #[test]
    fn reports_cancellation_instead_of_a_short_digest() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = hash_blocking(
            [1_u8, 2, 3].as_slice(),
            &[ChecksumAlgorithm::Sha256],
            &cancellation,
        )
        .expect_err("a cancelled calculation must not succeed");
        assert!(matches!(error, ChecksumError::Cancelled));
    }

    #[tokio::test]
    async fn the_async_and_blocking_paths_agree() {
        let data = vec![7_u8; HASH_CHUNK_BYTES * 2 + 13];
        let cancellation = CancellationToken::new();
        let asynchronous = hash_stream(data.as_slice(), &ChecksumAlgorithm::ALL, &cancellation)
            .await
            .expect("async hashing must succeed");
        let blocking = hash_blocking(data.as_slice(), &ChecksumAlgorithm::ALL, &cancellation)
            .expect("blocking hashing must succeed");
        assert_eq!(asynchronous, blocking);
        assert_eq!(asynchronous.len(), 4);
    }

    #[test]
    fn caps_a_chunk_at_the_remaining_budget() {
        assert_eq!(chunk_budget(None, 0, 500), Some(500));
        assert_eq!(chunk_budget(Some(1_000), 0, 500), Some(500));
        assert_eq!(chunk_budget(Some(400), 0, 500), Some(400));
        assert_eq!(chunk_budget(Some(400), 400, 500), None);
    }
}
