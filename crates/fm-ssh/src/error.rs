//! [`SshError`]: the single error type for every SSH/SFTP-transport operation
//! in this crate (task 0104).
//!
//! Deliberately flat rather than nested per-module error types: every caller
//! (an `fm-vfs-sftp` provider method, an application-layer `ConnectionDialer`
//! adapter, a future SSH-terminal consumer) needs to distinguish the same
//! small set of outcomes - a network/transport failure, a rejected
//! credential, and the two host-key states that must never be silently
//! resolved (spec §6.4) - so one flat enum keeps every caller's match
//! exhaustive and easy to read.

use thiserror::Error;

/// Failure modes for SSH connection, authentication and host-key
/// verification.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SshError {
    /// The underlying TCP/SSH transport could not be established.
    #[error("failed to connect to {host}:{port}: {message}")]
    Connect {
        /// Target host.
        host: String,
        /// Target port.
        port: u16,
        /// Sanitized failure description; never a raw credential.
        message: String,
    },
    /// The connection attempt exceeded its configured timeout.
    #[error("connection to {host}:{port} timed out")]
    Timeout {
        /// Target host.
        host: String,
        /// Target port.
        port: u16,
    },
    /// The presented host key has never been seen for this connection and
    /// was therefore not accepted (spec §6.4: never auto-accept on first
    /// use).
    #[error("host key {fingerprint} has not been verified yet")]
    HostKeyUnverified {
        /// SHA-256 fingerprint of the presented host key, in
        /// `SHA256:<base64>` form.
        fingerprint: String,
    },
    /// The presented host key differs from a previously accepted one for
    /// this connection and was therefore rejected (spec §6.4: never
    /// silently accept a changed key).
    #[error("host key {fingerprint} does not match the previously accepted {expected_fingerprint}")]
    HostKeyMismatch {
        /// SHA-256 fingerprint presented by the server this time.
        fingerprint: String,
        /// SHA-256 fingerprint previously accepted and stored for this
        /// connection.
        expected_fingerprint: String,
    },
    /// Authentication was attempted but rejected by the server.
    #[error("authentication failed")]
    AuthenticationFailed,
    /// The configured authentication method is not implemented yet.
    #[error("authentication method not supported: {0}")]
    UnsupportedAuthenticationMethod(&'static str),
    /// SSH-agent authentication could not reach or use the local agent - for
    /// example `SSH_AUTH_SOCK` is unset, the socket is unreachable, or the
    /// agent holds no usable public-key identities. Distinguished from
    /// [`SshError::AuthenticationFailed`] (a reachable agent whose keys the
    /// server rejected) so a caller can tell "no agent available" apart from
    /// "wrong key".
    #[error("ssh-agent error: {0}")]
    Agent(String),
    /// A supplied private key could not be parsed or its passphrase was
    /// rejected.
    #[error("invalid private key: {0}")]
    InvalidPrivateKey(String),
    /// The caller cancelled the operation.
    #[error("operation cancelled")]
    Cancelled,
    /// A general I/O or protocol failure on an established session.
    #[error("ssh session error: {0}")]
    Session(String),
    /// An SFTP-protocol-level failure on an established session.
    #[error("sftp error: {0}")]
    Sftp(String),
    /// The persistent known-hosts store could not be read or written.
    #[error("known-hosts store error: {0}")]
    KnownHostsStore(String),
    /// [`crate::KnownHostsStore::accept`] was called with a fingerprint that
    /// does not match the host's currently presented key, so it was not
    /// persisted (defense against confirming a stale or attacker-supplied
    /// fingerprint).
    #[error("host key {presented} does not match the fingerprint being accepted {attempted}")]
    HostKeyAcceptanceMismatch {
        /// Fingerprint the server actually presented just now.
        presented: String,
        /// Fingerprint the caller attempted to accept.
        attempted: String,
    },
}
