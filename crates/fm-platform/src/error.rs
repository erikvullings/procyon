use crate::PlatformCapabilities;

/// Failures reported by a [`crate::PlatformAdapter`].
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// The adapter does not implement this capability on the current
    /// platform. Callers should have checked
    /// [`crate::PlatformAdapter::capabilities`] first; this variant exists so
    /// default trait methods can fail safely if they are called anyway.
    #[error("platform does not support capability {capability:?}")]
    Unsupported {
        /// Capability the caller attempted to use.
        capability: PlatformCapabilities,
    },
    /// The requested path does not exist or could not be accessed.
    #[error("path not found: {path}")]
    NotFound {
        /// Path that could not be found.
        path: String,
    },
    /// The native call failed for a reason safe to report across layers.
    #[error("platform operation failed: {message}")]
    Io {
        /// Sanitized failure description; never a raw OS error message.
        message: String,
    },
}
