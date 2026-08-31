//! Remote embedded-terminal shell channels over SSH (task 0105).
//!
//! Extends the embedded terminal drawer (task 0126, `apps/fm-desktop`'s
//! `TerminalRegistry`) to locations backed by an SSH connection: instead of
//! spawning a local shell, [`RemoteTerminalService::open_shell`] resolves the
//! connection's already-stored configuration/credential and drives a real
//! remote PTY over `fm-ssh`.

use std::sync::Arc;

use fm_ssh::{RemoteShellChannel, SshConnectionManager};
use fm_vfs_sftp::SshConnectionResolver;
use uuid::Uuid;

use crate::error::ApplicationError;

/// Opens interactive shell channels on SSH-backed connections, reusing the
/// same pooled [`SshConnectionManager`] session an open SFTP browse for that
/// connection already established (keyed identically, by connection id
/// text) - never a second, separately authenticated connection.
pub(crate) struct RemoteTerminalService {
    ssh_connections: Arc<SshConnectionManager>,
    resolver: Arc<dyn SshConnectionResolver>,
}

impl RemoteTerminalService {
    pub(crate) fn new(
        ssh_connections: Arc<SshConnectionManager>,
        resolver: Arc<dyn SshConnectionResolver>,
    ) -> Self {
        Self {
            ssh_connections,
            resolver,
        }
    }

    /// Opens a new interactive shell channel on `connection_id`, starting in
    /// `remote_path` if given.
    ///
    /// Fails with [`ApplicationError::InvalidRequest`] if `connection_id`
    /// names a connection that is not SSH (via [`SshConnectionResolver`]'s
    /// own kind check) - the same "unavailable rather than merely hidden"
    /// gating every other capability-dependent action follows (spec §22).
    pub(crate) async fn open_shell(
        &self,
        connection_id: Uuid,
        remote_path: Option<&str>,
        term: &str,
        cols: u16,
        rows: u16,
    ) -> Result<RemoteShellChannel, ApplicationError> {
        let key = connection_id.to_string();
        let params = self.resolver.resolve(&key).await?;
        let session = self
            .ssh_connections
            .session(&key, &params)
            .await
            .map_err(|error| ApplicationError::PlatformOperationFailed(error.to_string()))?;
        session
            .open_shell(term, u32::from(cols), u32::from(rows), remote_path)
            .await
            .map_err(|error| ApplicationError::PlatformOperationFailed(error.to_string()))
    }
}
