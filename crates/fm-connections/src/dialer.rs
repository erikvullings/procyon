//! [`ConnectionDialer`]: the extension point a real protocol crate registers
//! into to perform an actual network handshake for `connect`/`test` (task
//! 0103).
//!
//! No implementation is registered for any [`crate::ConnectionKind`] by this
//! task - see [`crate::ConnectionService`]'s documentation for exactly what
//! `connect`/`test` do in that (current, pre-0104/0106) state. Task 0104
//! (SFTP) is expected to register a real SSH dialer here; task 0106 (FTP/FTPS)
//! a real FTP dialer.

use async_trait::async_trait;
use fm_credentials::ResolvedCredential;

use crate::error::ConnectionError;
use crate::profile::ConnectionProfile;

/// Attempts real protocol-level connectivity for one [`crate::ConnectionKind`].
///
/// Implemented by a future protocol crate (`fm-ssh`/`fm-vfs-sftp` for task
/// 0104, an FTP crate for task 0106), never by `fm-connections` itself: this
/// crate only defines the seam.
#[async_trait]
pub trait ConnectionDialer: Send + Sync {
    /// Attempts to establish or verify connectivity for `profile`, using
    /// `credential` if the profile's configuration resolved one.
    ///
    /// Returning `Ok(())` means the dialer considers the connection usable;
    /// an `Err` communicates why not (typically wrapping a protocol-specific
    /// authentication or network failure through [`ConnectionError`]).
    async fn dial(
        &self,
        profile: &ConnectionProfile,
        credential: Option<&ResolvedCredential>,
    ) -> Result<(), ConnectionError>;
}
