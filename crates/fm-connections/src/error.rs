//! Errors returned by connection validation, persistence and lifecycle
//! operations (task 0103).

use std::path::PathBuf;

use fm_credentials::CredentialError;

use crate::ids::ConnectionId;
use crate::kind::ConnectionKind;

/// A single structured validation failure for a [`crate::ConnectionProfile`]
/// or its [`crate::ConnectionConfiguration`] (never a bare string: task
/// 0103's testing requirements call for "structured validation failures...
/// as typed errors, not stringly-typed ones").
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConnectionValidationError {
    /// The connection's display name is empty.
    #[error("connection name must not be empty")]
    EmptyName,
    /// The profile's `kind` does not match its `configuration`'s kind.
    #[error("connection kind {kind:?} does not match configuration kind {configuration_kind:?}")]
    KindMismatch {
        /// The profile's declared kind.
        kind: ConnectionKind,
        /// The kind implied by the active `configuration` variant.
        configuration_kind: ConnectionKind,
    },
    /// An SSH configuration's `host` is empty.
    #[error("ssh host must not be empty")]
    EmptySshHost,
    /// An SSH configuration's `username` is empty.
    #[error("ssh username must not be empty")]
    EmptySshUsername,
    /// An SSH configuration's `port` is zero.
    #[error("ssh port must not be zero")]
    InvalidSshPort,
    /// An FTP/FTPS configuration's `host` is empty.
    #[error("ftp host must not be empty")]
    EmptyFtpHost,
    /// An FTP/FTPS configuration's `username` is empty.
    #[error("ftp username must not be empty")]
    EmptyFtpUsername,
    /// An FTP/FTPS configuration's `port` is zero.
    #[error("ftp port must not be zero")]
    InvalidFtpPort,
    /// A WebDAV configuration's `base_url` is empty.
    #[error("webdav base url must not be empty")]
    EmptyWebDavBaseUrl,
    /// A WebDAV configuration's `username` is empty.
    #[error("webdav username must not be empty")]
    EmptyWebDavUsername,
    /// An S3 configuration's `bucket` is empty.
    #[error("s3 bucket must not be empty")]
    EmptyS3Bucket,
    /// An S3 configuration's `access_key_id` is empty.
    #[error("s3 access key id must not be empty")]
    EmptyS3AccessKeyId,
    /// An SMB configuration's `server` is empty.
    #[error("smb server must not be empty")]
    EmptySmbServer,
    /// An SMB configuration's `share` is empty.
    #[error("smb share must not be empty")]
    EmptySmbShare,
    /// A create/update request supplied secret material for a OneDrive
    /// connection (task 0110). OneDrive never accepts a directly-supplied
    /// secret through this path: its only credential is the OAuth refresh
    /// token attached by the authorization flow, so a caller cannot inject
    /// or replace it by posting `secret` on the generic connection
    /// create/update surface.
    #[error(
        "OneDrive connections do not accept secret material through this request; authorize the connection instead"
    )]
    OneDriveSecretNotAccepted,
}

/// Failure modes for connection persistence and lifecycle operations. Hosts
/// translate these into transport-specific responses; no variant leaks a raw
/// OS or serde error message beyond a plain string.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ConnectionError {
    /// No connection exists with the given id.
    #[error("connection {id} not found")]
    NotFound {
        /// The id that was looked up.
        id: ConnectionId,
    },
    /// The connection profile failed one or more structural invariants.
    #[error("connection failed validation: {0:?}")]
    Invalid(Vec<ConnectionValidationError>),
    /// A filesystem operation failed.
    #[error("connection storage I/O error: {0}")]
    Io(String),
    /// (De)serializing connection JSON failed outside of a recognised
    /// corruption case.
    #[error("connection serialization error: {0}")]
    Serialization(String),
    /// The stored connection data could not be parsed. It has been backed up
    /// rather than discarded or silently repaired.
    #[error("connection data for {id} is corrupt; backed up to {}", backup_path.display())]
    Corrupt {
        /// The corrupt file's stem (its would-be connection id, if
        /// parseable).
        id: String,
        /// Where the unreadable file was moved to.
        backup_path: PathBuf,
    },
    /// A [`fm_credentials::CredentialStore`] operation failed while
    /// resolving, storing or deleting this connection's credential.
    #[error("connection credential error: {0}")]
    Credential(#[from] CredentialError),
    /// A [`crate::ConnectionDialer`] reported that the remote host presented
    /// a host key that has never been verified before (task 0104, spec
    /// §6.4). Distinct from a generic dial failure so a caller can offer an
    /// explicit "accept this host key" action instead of reporting a bare
    /// connection failure; never returned as a side effect of any other
    /// operation.
    #[error("host key {fingerprint} has not been verified yet")]
    HostKeyUnverified {
        /// `SHA256:<base64>` fingerprint of the presented host key.
        fingerprint: String,
    },
    /// A [`crate::ConnectionDialer`] reported that the remote host's key
    /// changed since it was last accepted (task 0104, spec §6.4). Never
    /// silently accepted, and always distinguishable from
    /// [`ConnectionError::HostKeyUnverified`].
    #[error("host key {fingerprint} does not match the previously accepted {expected_fingerprint}")]
    HostKeyMismatch {
        /// `SHA256:<base64>` fingerprint the host presented this time.
        fingerprint: String,
        /// `SHA256:<base64>` fingerprint previously accepted and stored.
        expected_fingerprint: String,
    },
    /// A [`crate::ConnectionDialer`] reported a failure other than the two
    /// host-key states above - a wrong password, an unreachable host, a
    /// rejected key, an unusable SSH-agent identity, and so on. Carries the
    /// dialer's own message verbatim so [`ConnectionService::evaluate`] can
    /// surface *why* a `connect`/`test` reported
    /// [`crate::ConnectionStatus::Failed`] instead of just that it did.
    #[error("{0}")]
    DialFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_error_converts_into_connection_error() {
        let reference = fm_credentials::CredentialRef::new();
        let error: ConnectionError = CredentialError::NotFound { reference }.into();
        assert!(matches!(error, ConnectionError::Credential(_)));
    }
}
