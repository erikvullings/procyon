//! [`OneDriveConnectionResolver`]: the seam `fm-application` implements to
//! bridge a saved `fm-connections` connection (plus its `fm-credentials`
//! / `fm-auth-oauth` backed refresh flow) into the single thing this
//! provider actually needs to call Microsoft Graph - a currently valid
//! bearer access token. See this crate's module doc for why the seam
//! exists at all.

use async_trait::async_trait;
use fm_vfs::VfsError;
use zeroize::Zeroizing;

/// A Microsoft Graph bearer access token, redaction-safe by construction.
///
/// [`std::fmt::Debug`] never prints the token value (specification §19,
/// mirroring `fm_auth_oauth::token::TokenResponse`'s identical guarantee) -
/// so an accidental `{:?}` in a log line, panic message or test assertion
/// failure can never leak it. The token is held in a [`Zeroizing`] buffer so
/// it is also wiped from memory once dropped.
#[derive(Clone)]
pub struct OneDriveAccessToken(Zeroizing<String>);

impl OneDriveAccessToken {
    /// Wraps a bearer token value resolved for one call.
    #[must_use]
    pub fn new(token: impl Into<String>) -> Self {
        Self(Zeroizing::new(token.into()))
    }

    /// Returns the token text, for building an `Authorization` header.
    ///
    /// Callers must never log, `Display`, or otherwise persist the returned
    /// slice; it exists only to be attached to one outgoing request.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for OneDriveAccessToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("OneDriveAccessToken")
            .field(&"<redacted>")
            .finish()
    }
}

/// Resolves an opaque connection id (the text form of a saved connection's
/// `ConnectionId`, as it appears in an `onedrive://<connection-id>/...`
/// [`fm_domain::Location`]) to a currently valid Graph access token.
///
/// Implementations own everything this crate deliberately does not: looking
/// up the saved connection, resolving its refresh token from a
/// `CredentialStore`, and silently renewing the access token (rotating the
/// stored refresh token) when it has expired. This is called fresh for
/// essentially every provider operation rather than cached here, exactly
/// like `fm_vfs_sftp::SshConnectionResolver`/`fm_vfs_s3::S3ConnectionResolver`
/// - it is cheap for a real implementation to memoize internally, and this
/// provider must never assume a token stays valid across two calls.
#[async_trait]
pub trait OneDriveConnectionResolver: Send + Sync {
    /// Resolves a currently valid access token for `connection_id`.
    ///
    /// Returns [`VfsError::NotFound`] if no such connection is configured,
    /// [`VfsError::InvalidLocation`] if it exists but is not a OneDrive
    /// connection, and [`VfsError::CredentialRequired`] if it exists but has
    /// no usable credential (never authenticated, revoked consent, or a
    /// refresh failure) - the caller (this provider) surfaces that as an
    /// actionable authorization error rather than a generic I/O failure.
    async fn resolve(&self, connection_id: &str) -> Result<OneDriveAccessToken, VfsError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_contains_the_token_value() {
        let token = OneDriveAccessToken::new("super-secret-bearer-value");

        let formatted = format!("{token:?}");

        assert!(!formatted.contains("super-secret-bearer-value"));
        assert!(formatted.contains("redacted"));
    }

    #[test]
    fn as_str_returns_the_wrapped_token_value() {
        let token = OneDriveAccessToken::new("bearer-value");

        assert_eq!(token.as_str(), "bearer-value");
    }
}
