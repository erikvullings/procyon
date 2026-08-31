//! Failure modes for [`crate::CredentialStore`] operations (task 0103).

use crate::CredentialRef;

/// Errors a [`crate::CredentialStore`] implementation can report.
///
/// Never wraps a raw OS error string containing secret content; platform
/// backends sanitize their own error text before it reaches this type (spec
/// §19 "no secret logging").
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CredentialError {
    /// No credential exists for the given reference (including after it has
    /// been deleted).
    #[error("credential {reference} not found")]
    NotFound {
        /// The reference that was looked up.
        reference: CredentialRef,
    },
    /// The underlying protected store (Keychain, Credential Manager, ...) is
    /// not reachable or not supported on this host.
    #[error("credential store unavailable: {0}")]
    Unavailable(String),
    /// The store rejected the operation for a reason safe to surface to the
    /// caller, without leaking secret content.
    #[error("credential store operation failed: {0}")]
    Backend(String),
}
