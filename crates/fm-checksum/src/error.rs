//! Typed failures for checksum calculation, checksum-file handling and
//! duplicate detection (spec §2.2: libraries use `thiserror`).

use fm_domain::Location;

/// A failure while computing a checksum over a byte stream.
#[derive(Debug, thiserror::Error)]
pub enum ChecksumError {
    /// The caller's cancellation token fired before the stream was consumed.
    ///
    /// Reported explicitly rather than as a short, "successful" digest so a
    /// caller can never mistake a cancelled calculation for a complete one.
    #[error("checksum calculation was cancelled")]
    Cancelled,
    /// No algorithm was requested, so there is nothing to compute.
    #[error("no checksum algorithm was requested")]
    NoAlgorithmRequested,
    /// The underlying byte stream failed while being read.
    #[error("failed to read entry for checksum: {0}")]
    Read(#[from] std::io::Error),
    /// The entry could not be opened through its provider.
    #[error("failed to open `{uri}` for checksum: {source}")]
    Open {
        /// URI of the entry that could not be opened.
        uri: String,
        /// Underlying provider failure, rendered as text so the error stays
        /// comparable and cheap to clone-free-propagate.
        source: Box<fm_vfs::VfsError>,
    },
}

impl ChecksumError {
    /// Wraps a provider failure that occurred while opening `location`.
    #[must_use]
    pub fn open(location: &Location, source: fm_vfs::VfsError) -> Self {
        Self::Open {
            uri: location.uri.clone(),
            source: Box::new(source),
        }
    }
}

/// A failure while reading or writing a checksum file.
#[derive(Debug, thiserror::Error)]
pub enum ChecksumFileError {
    /// A line did not match the `<digest><space><space><path>` layout.
    #[error("line {line} is not a valid checksum entry")]
    MalformedLine {
        /// One-based line number within the checksum file.
        line: usize,
    },
    /// A line's digest field contained a non-hexadecimal character or had an
    /// odd length.
    #[error("line {line} has a malformed hex digest")]
    MalformedDigest {
        /// One-based line number within the checksum file.
        line: usize,
    },
    /// The checksum file could not be read or written.
    #[error("checksum file I/O failed: {0}")]
    Io(#[from] std::io::Error),
}
