use crate::VfsError;

bitflags::bitflags! {
    /// Operations a virtual filesystem provider can perform.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ProviderCapabilities: u64 {
        /// Enumerate a directory.
        const LIST = 1 << 0;
        /// Open an entry for reading.
        const READ = 1 << 1;
        /// Open a destination for writing.
        const WRITE = 1 << 2;
        /// Create a directory.
        const CREATE_DIRECTORY = 1 << 3;
        /// Rename an entry without changing its parent.
        const RENAME = 1 << 4;
        /// Move an entry to another location.
        const MOVE = 1 << 5;
        /// Copy data within the provider without streaming it through the application.
        const SERVER_SIDE_COPY = 1 << 6;
        /// Permanently remove an entry.
        const DELETE = 1 << 7;
        /// Move an entry to a recoverable trash location.
        const TRASH = 1 << 8;
        /// Watch a location for changes.
        const WATCH = 1 << 9;
        /// Seek within an open stream.
        const RANDOM_ACCESS = 1 << 10;
        /// Change entry timestamps.
        const SET_TIMESTAMPS = 1 << 11;
        /// Change entry permissions.
        const SET_PERMISSIONS = 1 << 12;
        /// Calculate entry checksums.
        const CHECKSUM = 1 << 13;
    }
}

impl ProviderCapabilities {
    /// Verifies that all requested capabilities are available.
    ///
    /// Callers use this guard before invoking a provider operation, ensuring
    /// unsupported work is rejected before the provider performs any I/O.
    pub fn require(self, required: Self) -> Result<(), VfsError> {
        if self.contains(required) {
            Ok(())
        } else {
            Err(VfsError::UnsupportedCapability {
                capability: required,
            })
        }
    }
}
