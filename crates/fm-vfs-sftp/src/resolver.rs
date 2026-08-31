//! [`SshConnectionResolver`]: the seam `fm-application` implements to bridge
//! `fm-connections`' `ConnectionProfile`s into `fm-ssh`'s connection-agnostic
//! [`fm_ssh::SshConnectionParameters`] (see this crate's module doc for why).

use async_trait::async_trait;
use fm_ssh::SshConnectionParameters;
use fm_vfs::VfsError;

/// Resolves an opaque connection id (the text form of a `ConnectionId`) into
/// the parameters needed to actually dial it.
#[async_trait]
pub trait SshConnectionResolver: Send + Sync {
    /// Looks up and resolves everything needed to connect to `connection_id`
    /// - host/port/username, credential, host-key policy and keepalive -
    /// reporting [`VfsError::NotFound`] if no such connection is configured
    /// and [`VfsError::InvalidLocation`] if it exists but is not an SSH
    /// connection.
    async fn resolve(&self, connection_id: &str) -> Result<SshConnectionParameters, VfsError>;
}
