//! Connection-agnostic SSH types (task 0104).
//!
//! This crate never depends on `fm-connections`: the workspace's layering
//! fitness test (`fm-test-support`) requires strictly-downward dependencies,
//! and `fm-connections` already sits above `fm-vfs`/`fm-credentials` in that
//! ordering, so a real dependency here would push `fm-ssh`, and transitively
//! `fm-vfs-sftp`, above `fm-application` - which then could no longer
//! register the provider or wire the dialer. Instead this module defines its
//! own minimal, connection-agnostic types (mirroring how `fm_events::ConnectionStatusPayload`
//! deliberately duplicates `fm_connections::ConnectionStatus` for the same
//! reason); `fm-application` is the one layer allowed to depend on both
//! crates and is where the translation between them happens.

use std::time::Duration;

use zeroize::Zeroizing;

/// Where to connect and as whom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConnectTarget {
    /// Hostname or IP address.
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// Remote username.
    pub username: String,
}

/// How to authenticate an SSH session.
///
/// Mirrors `fm_connections::SshAuthenticationMethod`'s resolved secret shape,
/// not the method enum itself: by the time a caller builds this, the
/// credential has already been resolved from a `CredentialStore`.
#[derive(Clone)]
pub enum SshCredential {
    /// No credential is presented (for example a server configured to allow
    /// anonymous or host-based auth); rarely useful, kept for completeness.
    None,
    /// Password authentication.
    Password(Zeroizing<String>),
    /// Private-key authentication. `key` is PEM (OpenSSH-format) text, not a
    /// file path - credentials are held in memory only.
    PrivateKey {
        /// PEM/OpenSSH-format private key text.
        key: Zeroizing<String>,
        /// Passphrase protecting `key`, if any.
        passphrase: Option<Zeroizing<String>>,
    },
    /// Delegates to a running SSH agent.
    ///
    /// Not implemented in this task (spec/task notes: "start with
    /// password/private-key auth; agent... can follow") - using this variant
    /// reports [`crate::SshError::UnsupportedAuthenticationMethod`].
    Agent,
}

/// How this session treats the remote host key, mirroring
/// `fm_connections::HostKeyPolicy` (kept as a separate type for the same
/// layering reason documented on this module).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshHostKeyPolicy {
    /// Prompt for explicit confirmation the first time a host key is seen,
    /// then persist it; a later mismatch is always rejected.
    PromptOnFirstUse,
    /// Only ever succeed if the host key was already explicitly accepted and
    /// stored; never auto-accept, even on the first connection.
    RequireKnownHost,
}

/// Full parameters for one SSH connection attempt.
#[derive(Clone)]
pub struct SshConnectionParameters {
    /// Where to connect and as whom.
    pub target: SshConnectTarget,
    /// How to authenticate.
    pub credential: SshCredential,
    /// How to treat the remote host key.
    pub host_key_policy: SshHostKeyPolicy,
    /// Keepalive interval, if any.
    pub keepalive: Option<Duration>,
}
