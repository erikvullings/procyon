//! [`SftpFileSystemProvider`]: the `FileSystemProvider` implementation
//! (task 0104, spec §6.6).
//!
//! ## Capability choices (spec §6.6's table, applied literally)
//!
//! `LIST`, `READ`, `WRITE`, `CREATE_DIRECTORY`, `RENAME`, `MOVE` and
//! `DELETE` are implemented for real. `SERVER_SIDE_COPY` is left at the
//! trait's default (unsupported): spec §6.6 calls it "usually
//! limited/unsupported" for SFTP, and no clean, portable SFTPv3 primitive
//! exists for it. `TRASH` is left unsupported ("generally no"). `WATCH` has
//! no default and is implemented to always report unsupported (spec §6.6
//! "no native watch"); instead, [`FileSystemProvider::change_tracking`] is
//! overridden to [`fm_vfs::ChangeTracking::Poll`] at
//! [`fm_vfs::CONSERVATIVE_POLL_INTERVAL`], so `fm-application`'s directory
//! service polls this provider's `list` conservatively rather than treating
//! it as untracked (task 0109). `RANDOM_ACCESS`,
//! `SET_TIMESTAMPS`, `SET_PERMISSIONS` and `CHECKSUM` are left unset: SFTPv3
//! technically supports seeking/`fsetstat`, but nothing in this task's
//! acceptance criteria needs them, and advertising a capability this
//! provider hasn't actually exercised against a real server would be a
//! worse failure mode (a caller trusting a claim that was never verified)
//! than honestly under-advertising - a documented, explicit gap rather than
//! a silent one.
//!
//! ## Temporary files during transfers (spec §6.7 "do not require temporary
//! local files")
//!
//! [`FileSystemProvider::commit_copy`]/[`FileSystemProvider::discard_copy`]
//! are implemented: the operation engine writes a `.fm-copy-{uuid}` *remote*
//! file next to the real destination (a temporary file on the SFTP server
//! itself, streamed to directly - never staged through a local disk file),
//! then this provider renames it into place. This mirrors
//! `fm-vfs-local`'s own temp-file-then-publish pattern on its own
//! filesystem; "no temporary local files" refers to local-disk staging, not
//! to same-provider temp-then-rename, which is how every
//! [`fm_vfs::FileSystemProvider`] implementation in this workspace commits a
//! streamed write.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fm_domain::{
    EntryId, EntryKind, EntryMetadata, EntrySummary, Location, LocationError, OwnershipInfo,
    PermissionsInfo, ProviderId,
};
use fm_ssh::{SshConnectionManager, SshConnectionParameters, SshError};
use fm_vfs::{
    CONSERVATIVE_POLL_INTERVAL, ChangeTracking, CopyCommitOptions, DirectoryPage, EntryRef,
    FileSystemProvider, ListOptions, ProviderCapabilities, ProviderChangeStream,
    ProviderReadStream, ProviderWriteStream, RemoveOptions, TransferCapabilities, TransferEndpoint,
    VfsError, WriteOptions,
};
use futures::future::BoxFuture;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, FileType, OpenFlags};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::resolver::SshConnectionResolver;

const SFTP_PROVIDER: &str = "sftp";

/// SFTP-over-SSH `FileSystemProvider` (spec §6.2's `SftpProvider`).
pub struct SftpFileSystemProvider {
    connections: Arc<SshConnectionManager>,
    resolver: Arc<dyn SshConnectionResolver>,
}

impl SftpFileSystemProvider {
    /// Creates a provider backed by `connections` (session pooling/reconnect,
    /// task 0104's `fm-ssh`) and `resolver` (translates a connection id into
    /// dial parameters, implemented by `fm-application`).
    #[must_use]
    pub fn new(
        connections: Arc<SshConnectionManager>,
        resolver: Arc<dyn SshConnectionResolver>,
    ) -> Self {
        Self {
            connections,
            resolver,
        }
    }

    async fn acquire_sftp(
        &self,
        connection_id: &str,
        params: &SshConnectionParameters,
    ) -> Result<Arc<SftpSession>, VfsError> {
        let session = self
            .connections
            .session(connection_id, params)
            .await
            .map_err(|error| map_ssh_error(error, connection_id))?;
        session
            .sftp()
            .await
            .map_err(|error| map_ssh_error(error, connection_id))
    }

    /// Runs `operation` against a live SFTP session for `connection_id`,
    /// reconnecting and retrying with a short backoff if an attempt fails
    /// with a transport-shaped error (spec §6.8 "reconnect for browsing") -
    /// a protocol-level response (file not found, permission denied, ...) is
    /// never retried. A flaky link (e.g. dropping over VPN) often recovers
    /// within a second or two, so this absorbs a couple of those drops
    /// before surfacing anything to the caller, rather than the previous
    /// single-retry behaviour that gave up as soon as one reconnect failed.
    async fn with_sftp<T, F, Fut>(&self, connection_id: &str, operation: F) -> Result<T, VfsError>
    where
        F: Fn(Arc<SftpSession>) -> Fut,
        Fut: std::future::Future<Output = Result<T, russh_sftp::client::error::Error>>,
    {
        const RECONNECT_BACKOFF: [std::time::Duration; 2] = [
            std::time::Duration::from_millis(200),
            std::time::Duration::from_millis(600),
        ];

        let params = self.resolver.resolve(connection_id).await?;
        let sftp = self.acquire_sftp(connection_id, &params).await?;
        let mut last_error = match operation(sftp).await {
            Ok(value) => return Ok(value),
            Err(error) if is_transport_error(&error) => map_sftp_error(error, connection_id),
            Err(error) => return Err(map_sftp_error(error, connection_id)),
        };

        for backoff in RECONNECT_BACKOFF {
            self.connections.invalidate(connection_id).await;
            tokio::time::sleep(backoff).await;
            let sftp = match self.acquire_sftp(connection_id, &params).await {
                Ok(sftp) => sftp,
                Err(error) => {
                    last_error = error;
                    continue;
                }
            };
            match operation(sftp).await {
                Ok(value) => return Ok(value),
                Err(error) if is_transport_error(&error) => {
                    last_error = map_sftp_error(error, connection_id);
                }
                Err(error) => return Err(map_sftp_error(error, connection_id)),
            }
        }
        Err(last_error)
    }

    fn remove_directory_recursive<'a>(
        &'a self,
        connection_id: &'a str,
        remote_path: &'a str,
    ) -> BoxFuture<'a, Result<(), VfsError>> {
        Box::pin(async move {
            let path = remote_path.to_owned();
            let entries = self
                .with_sftp(connection_id, move |sftp| {
                    let path = path.clone();
                    async move { sftp.read_dir(path).await.map(Iterator::collect::<Vec<_>>) }
                })
                .await?;
            for entry in entries {
                let child_path = join_remote_path(remote_path, &entry.file_name());
                if entry.metadata().is_dir() {
                    self.remove_directory_recursive(connection_id, &child_path)
                        .await?;
                } else {
                    let path = child_path.clone();
                    self.with_sftp(connection_id, move |sftp| {
                        let path = path.clone();
                        async move { sftp.remove_file(path).await }
                    })
                    .await?;
                }
            }
            let path = remote_path.to_owned();
            self.with_sftp(connection_id, move |sftp| {
                let path = path.clone();
                async move { sftp.remove_dir(path).await }
            })
            .await
        })
    }
}

#[async_trait]
impl FileSystemProvider for SftpFileSystemProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new(SFTP_PROVIDER)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::LIST
            | ProviderCapabilities::READ
            | ProviderCapabilities::WRITE
            | ProviderCapabilities::CREATE_DIRECTORY
            | ProviderCapabilities::RENAME
            | ProviderCapabilities::MOVE
            | ProviderCapabilities::DELETE
            // Checksums stream through `open_read`, so `READ` is the only
            // thing they need (task 0077, spec §6). Note this means the file
            // is transferred to be hashed: there is no server-side digest.
            | ProviderCapabilities::CHECKSUM
    }

    /// Task 0108. The endpoint is the *connection id*, not the provider id:
    /// two `sftp://` locations on different saved connections are different
    /// servers, and a server-side `rename` between them is impossible, so the
    /// operation planner must never treat them as one backend.
    ///
    /// `server_side_move` is `true` (SFTPv3 `rename` within one connection);
    /// `server_side_copy` stays `false` for the reason given in this module's
    /// documentation. Resumable transfers and offset reads/writes are left
    /// `false`: SFTPv3 can express them, but this provider implements neither,
    /// and advertising an unimplemented fast path would make the planner pick
    /// one that cannot work.
    fn transfer_capabilities(&self, location: &Location) -> Result<TransferCapabilities, VfsError> {
        let parsed = ParsedSftpLocation::parse(location)?;
        Ok(TransferCapabilities {
            endpoint: TransferEndpoint::new(format!("sftp:{}", parsed.connection_id)),
            server_side_copy: false,
            server_side_move: true,
            resumable_upload: false,
            resumable_download: false,
            random_read: false,
            random_write: false,
        })
    }

    fn change_tracking(&self) -> ChangeTracking {
        ChangeTracking::Poll {
            interval: CONSERVATIVE_POLL_INTERVAL,
        }
    }

    async fn list(
        &self,
        location: &Location,
        options: ListOptions,
        cancellation: CancellationToken,
    ) -> Result<DirectoryPage, VfsError> {
        check_cancelled(&cancellation)?;
        if options.page_size == 0 {
            return Err(invalid(location));
        }
        let parsed = ParsedSftpLocation::parse(location)?;
        let remote_path = parsed.remote_path.clone();
        let entries = self
            .with_sftp(&parsed.connection_id, move |sftp| {
                let remote_path = remote_path.clone();
                async move {
                    sftp.read_dir(remote_path)
                        .await
                        .map(Iterator::collect::<Vec<_>>)
                }
            })
            .await?;

        check_cancelled(&cancellation)?;
        let offset = decode_token(options.continuation_token.as_deref(), location)?;
        let mut summaries = Vec::new();
        for dir_entry in entries.iter().skip(offset).take(options.page_size) {
            let name = dir_entry.file_name();
            let child_location = location
                .join(&name)
                .map_err(|error| map_join_error(error, location))?;
            summaries.push(build_summary(child_location, name, dir_entry.metadata()));
        }
        let has_more = entries.len() > offset + summaries.len();
        let continuation_token = has_more.then(|| (offset + summaries.len()).to_string());
        Ok(DirectoryPage {
            total_known_entries: Some(entries.len() as u64),
            entries: summaries,
            has_more,
            continuation_token,
        })
    }

    async fn metadata(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntryMetadata, VfsError> {
        check_cancelled(&cancellation)?;
        let parsed = ParsedSftpLocation::parse(&entry.location)?;
        let remote_path = parsed.remote_path.clone();
        let attrs = self
            .with_sftp(&parsed.connection_id, move |sftp| {
                let remote_path = remote_path.clone();
                async move { sftp.symlink_metadata(remote_path).await }
            })
            .await?;
        Ok(EntryMetadata {
            entry_id: entry.id,
            permissions: Some(permissions_info(&attrs)),
            ownership: Some(OwnershipInfo {
                owner: attrs
                    .user
                    .clone()
                    .or_else(|| attrs.uid.map(|uid| uid.to_string())),
                group: attrs
                    .group
                    .clone()
                    .or_else(|| attrs.gid.map(|gid| gid.to_string())),
            }),
            extended_attributes: BTreeMap::new(),
            checksums: BTreeMap::new(),
            image_dimensions: None,
            media: None,
            archive: None,
            plugin_fields: BTreeMap::new(),
        })
    }

    async fn inspect(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntrySummary, VfsError> {
        check_cancelled(&cancellation)?;
        let parsed = ParsedSftpLocation::parse(&entry.location)?;
        let remote_path = parsed.remote_path.clone();
        let attrs = self
            .with_sftp(&parsed.connection_id, move |sftp| {
                let remote_path = remote_path.clone();
                async move { sftp.symlink_metadata(remote_path).await }
            })
            .await?;
        let name = entry
            .location
            .name()
            .map_err(|_| invalid(&entry.location))?;
        Ok(build_summary(entry.location.clone(), name, attrs))
    }

    async fn file_size(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<u64, VfsError> {
        check_cancelled(&cancellation)?;
        let parsed = ParsedSftpLocation::parse(&entry.location)?;
        let remote_path = parsed.remote_path.clone();
        let attrs = self
            .with_sftp(&parsed.connection_id, move |sftp| {
                let remote_path = remote_path.clone();
                async move { sftp.symlink_metadata(remote_path).await }
            })
            .await?;
        if attrs.is_dir() {
            return Err(VfsError::IsADirectory {
                location: entry.location.uri.clone(),
            });
        }
        Ok(attrs.len())
    }

    async fn create_directory(
        &self,
        location: &Location,
        name: &str,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        check_cancelled(&cancellation)?;
        let child_location = location
            .join(name)
            .map_err(|error| map_join_error(error, location))?;
        let parsed = ParsedSftpLocation::parse(&child_location)?;
        let remote_path = parsed.remote_path.clone();
        self.with_sftp(&parsed.connection_id, move |sftp| {
            let remote_path = remote_path.clone();
            async move { sftp.create_dir(remote_path).await }
        })
        .await?;
        Ok(EntryRef {
            id: entry_id_for(&child_location),
            location: child_location,
        })
    }

    async fn rename(
        &self,
        source: &EntryRef,
        destination: &Location,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        check_cancelled(&cancellation)?;
        let source_parsed = ParsedSftpLocation::parse(&source.location)?;
        let destination_parsed = ParsedSftpLocation::parse(destination)?;
        if source_parsed.connection_id != destination_parsed.connection_id {
            return Err(invalid(destination));
        }
        let old_path = source_parsed.remote_path.clone();
        let new_path = destination_parsed.remote_path.clone();
        self.with_sftp(&source_parsed.connection_id, move |sftp| {
            let (old_path, new_path) = (old_path.clone(), new_path.clone());
            async move { sftp.rename(old_path, new_path).await }
        })
        .await?;
        Ok(EntryRef {
            id: source.id,
            location: destination.clone(),
        })
    }

    async fn remove(
        &self,
        entry: &EntryRef,
        options: RemoveOptions,
        cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        check_cancelled(&cancellation)?;
        if options.use_trash {
            return Err(VfsError::UnsupportedCapability {
                capability: ProviderCapabilities::TRASH,
            });
        }
        let parsed = ParsedSftpLocation::parse(&entry.location)?;
        let remote_path = parsed.remote_path.clone();
        let attrs = self
            .with_sftp(&parsed.connection_id, move |sftp| {
                let remote_path = remote_path.clone();
                async move { sftp.symlink_metadata(remote_path).await }
            })
            .await?;
        if attrs.is_dir() {
            if options.recursive {
                self.remove_directory_recursive(&parsed.connection_id, &parsed.remote_path)
                    .await
            } else {
                let remote_path = parsed.remote_path.clone();
                self.with_sftp(&parsed.connection_id, move |sftp| {
                    let remote_path = remote_path.clone();
                    async move { sftp.remove_dir(remote_path).await }
                })
                .await
            }
        } else {
            let remote_path = parsed.remote_path.clone();
            self.with_sftp(&parsed.connection_id, move |sftp| {
                let remote_path = remote_path.clone();
                async move { sftp.remove_file(remote_path).await }
            })
            .await
        }
    }

    async fn open_read(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError> {
        check_cancelled(&cancellation)?;
        let parsed = ParsedSftpLocation::parse(&entry.location)?;
        let remote_path = parsed.remote_path.clone();
        let file = self
            .with_sftp(&parsed.connection_id, move |sftp| {
                let remote_path = remote_path.clone();
                async move { sftp.open(remote_path).await }
            })
            .await?;
        Ok(Box::pin(file))
    }

    async fn open_write(
        &self,
        destination: &Location,
        options: WriteOptions,
        cancellation: CancellationToken,
    ) -> Result<ProviderWriteStream, VfsError> {
        check_cancelled(&cancellation)?;
        let parsed = ParsedSftpLocation::parse(destination)?;
        let remote_path = parsed.remote_path.clone();
        let overwrite = options.overwrite;
        let file = self
            .with_sftp(&parsed.connection_id, move |sftp| {
                let remote_path = remote_path.clone();
                async move {
                    let mut flags = OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE;
                    if !overwrite {
                        flags |= OpenFlags::EXCLUDE;
                    }
                    sftp.open_with_flags(remote_path, flags).await
                }
            })
            .await?;
        Ok(Box::pin(file))
    }

    async fn commit_copy(
        &self,
        _source: &EntryRef,
        temporary: &Location,
        destination: &Location,
        options: CopyCommitOptions,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        check_cancelled(&cancellation)?;
        let temporary_parsed = ParsedSftpLocation::parse(temporary)?;
        let destination_parsed = ParsedSftpLocation::parse(destination)?;
        if temporary_parsed.connection_id != destination_parsed.connection_id {
            return Err(invalid(destination));
        }
        let connection_id = destination_parsed.connection_id.clone();

        if options.overwrite {
            // SFTPv3 `rename` refuses to replace an existing file on most
            // servers; best-effort remove the previous destination first.
            let existing_path = destination_parsed.remote_path.clone();
            let _ = self
                .with_sftp(&connection_id, move |sftp| {
                    let existing_path = existing_path.clone();
                    async move { sftp.remove_file(existing_path).await }
                })
                .await;
        } else {
            let existing_path = destination_parsed.remote_path.clone();
            let exists = self
                .with_sftp(&connection_id, move |sftp| {
                    let existing_path = existing_path.clone();
                    async move { sftp.try_exists(existing_path).await }
                })
                .await?;
            if exists {
                return Err(VfsError::AlreadyExists {
                    location: destination.uri.clone(),
                });
            }
        }

        let temporary_path = temporary_parsed.remote_path.clone();
        let destination_path = destination_parsed.remote_path.clone();
        self.with_sftp(&connection_id, move |sftp| {
            let (temporary_path, destination_path) =
                (temporary_path.clone(), destination_path.clone());
            async move { sftp.rename(temporary_path, destination_path).await }
        })
        .await?;

        Ok(EntryRef {
            id: entry_id_for(destination),
            location: destination.clone(),
        })
    }

    async fn discard_copy(
        &self,
        temporary: &Location,
        _cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        let parsed = ParsedSftpLocation::parse(temporary)?;
        let remote_path = parsed.remote_path.clone();
        match self
            .with_sftp(&parsed.connection_id, move |sftp| {
                let remote_path = remote_path.clone();
                async move { sftp.remove_file(remote_path).await }
            })
            .await
        {
            Ok(()) => Ok(()),
            Err(VfsError::NotFound { .. }) => Ok(()),
            Err(other) => Err(other),
        }
    }

    async fn same_filesystem(
        &self,
        source: &EntryRef,
        destination_directory: &Location,
        cancellation: CancellationToken,
    ) -> Result<bool, VfsError> {
        check_cancelled(&cancellation)?;
        let source_parsed = ParsedSftpLocation::parse(&source.location)?;
        let destination_parsed = ParsedSftpLocation::parse(destination_directory)?;
        Ok(source_parsed.connection_id == destination_parsed.connection_id)
    }

    async fn watch(
        &self,
        location: &Location,
        cancellation: CancellationToken,
    ) -> Result<ProviderChangeStream, VfsError> {
        let _ = (location, cancellation);
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::WATCH,
        })
    }
}

/// A `sftp://<connection-id>/<remote-path>` location, split into its
/// connection id (opaque, per `fm_domain::Location`'s own SFTP parsing) and
/// the real remote path text with each segment percent-decoded - mirroring
/// how `fm-archive` parses its own scheme directly off `Location::uri`
/// rather than reaching into `fm-domain` internals (see
/// `crates/fm-archive/src/lib.rs`'s `ParsedArchiveLocation`).
struct ParsedSftpLocation {
    connection_id: String,
    remote_path: String,
}

impl ParsedSftpLocation {
    fn parse(location: &Location) -> Result<Self, VfsError> {
        if location.provider_id.as_str() != SFTP_PROVIDER {
            return Err(invalid(location));
        }
        let remainder = location
            .uri
            .strip_prefix("sftp://")
            .ok_or_else(|| invalid(location))?;
        let (connection_id, path) = remainder.split_once('/').ok_or_else(|| invalid(location))?;
        if connection_id.is_empty() {
            return Err(invalid(location));
        }
        let remote_path = if path.is_empty() {
            "/".to_owned()
        } else {
            let mut decoded = String::from("/");
            let segments = path
                .split('/')
                .map(|segment| decode_percent(segment, location))
                .collect::<Result<Vec<_>, _>>()?;
            decoded.push_str(&segments.join("/"));
            decoded
        };
        Ok(Self {
            connection_id: connection_id.to_owned(),
            remote_path,
        })
    }
}

fn decode_percent(segment: &str, location: &Location) -> Result<String, VfsError> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = hex_value(bytes[index + 1]);
            let low = hex_value(bytes[index + 2]);
            if let (Some(high), Some(low)) = (high, low) {
                decoded.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).map_err(|_| invalid(location))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn join_remote_path(parent: &str, name: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

fn is_transport_error(error: &russh_sftp::client::error::Error) -> bool {
    use russh_sftp::client::error::Error;
    matches!(
        error,
        Error::IO(_) | Error::Timeout | Error::UnexpectedPacket | Error::UnexpectedBehavior(_)
    )
}

fn map_sftp_error(error: russh_sftp::client::error::Error, connection_id: &str) -> VfsError {
    use russh_sftp::client::error::Error;
    use russh_sftp::protocol::StatusCode;
    match error {
        Error::Status(status) => match status.status_code {
            StatusCode::NoSuchFile => VfsError::NotFound {
                location: format!("sftp://{connection_id}"),
            },
            StatusCode::PermissionDenied => VfsError::PermissionDenied {
                location: format!("sftp://{connection_id}"),
            },
            _ => VfsError::Io {
                message: status.error_message,
            },
        },
        other => VfsError::Io {
            message: other.to_string(),
        },
    }
}

fn map_ssh_error(error: SshError, connection_id: &str) -> VfsError {
    match error {
        SshError::AuthenticationFailed => VfsError::PermissionDenied {
            location: format!("sftp://{connection_id}"),
        },
        SshError::Cancelled => VfsError::Cancelled,
        other => VfsError::Io {
            message: other.to_string(),
        },
    }
}

fn map_join_error(error: LocationError, location: &Location) -> VfsError {
    match error {
        LocationError::EmptySegment => VfsError::EmptyName,
        LocationError::InvalidName(_) => VfsError::PathTraversalName,
        LocationError::NullByte => VfsError::InvalidNameCharacters,
        LocationError::ReservedWindowsName(_) => VfsError::ReservedName,
        _ => invalid(location),
    }
}

fn invalid(location: &Location) -> VfsError {
    VfsError::InvalidLocation {
        location: location.uri.clone(),
    }
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), VfsError> {
    if cancellation.is_cancelled() {
        Err(VfsError::Cancelled)
    } else {
        Ok(())
    }
}

fn decode_token(token: Option<&str>, location: &Location) -> Result<usize, VfsError> {
    token.map_or(Ok(0), |value| value.parse().map_err(|_| invalid(location)))
}

fn entry_id_for(location: &Location) -> EntryId {
    EntryId::from(Uuid::new_v5(&Uuid::NAMESPACE_URL, location.uri.as_bytes()))
}

fn entry_kind(attrs: &FileAttributes) -> EntryKind {
    match attrs.file_type() {
        FileType::Dir => EntryKind::Directory,
        FileType::Symlink => EntryKind::Symlink,
        FileType::File | FileType::Other => EntryKind::File,
    }
}

fn permissions_info(attrs: &FileAttributes) -> PermissionsInfo {
    let permissions = attrs.permissions();
    PermissionsInfo {
        readable: true,
        writable: !permissions.is_readonly(),
        executable: permissions.owner_exec,
        unix_mode: attrs.permissions,
    }
}

fn build_summary(location: Location, name: String, attrs: FileAttributes) -> EntrySummary {
    let kind = entry_kind(&attrs);
    EntrySummary {
        id: entry_id_for(&location),
        extension: Path::new(&name)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_owned),
        hidden: name.starts_with('.'),
        name,
        location,
        size: (kind == EntryKind::File).then_some(attrs.len()),
        modified_at: attrs.mtime.map(|seconds| {
            DateTime::<Utc>::from(UNIX_EPOCH + Duration::from_secs(u64::from(seconds)))
        }),
        created_at: None,
        read_only: attrs.permissions().is_readonly(),
        kind,
        mime_type: None,
        icon_key: None,
        metadata_revision: 0,
        git_status: None,
    }
}
