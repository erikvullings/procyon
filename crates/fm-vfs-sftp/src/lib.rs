//! SFTP-over-SSH implementation of the virtual filesystem provider (task
//! 0104, spec §6).
//!
//! Mirrors `fm-vfs-local`'s shape closely (a thin, mostly-stateless
//! `FileSystemProvider` translating `Location`/`EntryRef` operations into
//! filesystem calls - here, SFTP calls through [`fm_ssh::SshConnectionManager`]
//! instead of `tokio::fs`).
//!
//! ## Resolving `ConnectionId` without depending on `fm-connections`
//!
//! `Location`s reference a connection id as an opaque path segment (spec
//! §6.5, `sftp://<connection-id>/home/erik`); this crate never depends on
//! `fm-connections` itself (the workspace's layer-fitness test would
//! otherwise make it impossible for `fm-application` to both register this
//! provider *and* wire an SSH dialer through `fm-ssh` - see `fm_ssh`'s crate
//! doc). Instead [`SshConnectionResolver`] is the seam: `fm-application`
//! implements it by looking up the `ConnectionProfile` and resolving its
//! credential, translating both into `fm_ssh`'s connection-agnostic types.

mod provider;
mod resolver;

pub use provider::SftpFileSystemProvider;
pub use resolver::SshConnectionResolver;
