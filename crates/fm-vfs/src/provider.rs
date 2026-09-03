use async_trait::async_trait;
use fm_domain::{EntryMetadata, EntrySummary, Location, ProviderId};
use tokio_util::sync::CancellationToken;

use crate::{
    ChangeTracking, CopyCommitOptions, DirectoryPage, EntryRef, ListOptions, ProviderCapabilities,
    ProviderChangeStream, ProviderReadStream, ProviderWriteStream, RemoveOptions,
    TransferCapabilities, TransferEndpoint, VfsError, WriteOptions,
};

/// Provider-neutral asynchronous filesystem interface.
///
/// Implementations may represent local files, archives, remote stores or
/// virtual result sets. Callers must check [`Self::capabilities`] before
/// invoking a capability-gated operation.
#[async_trait]
pub trait FileSystemProvider: Send + Sync {
    /// Returns the stable id used by owned [`Location`] values.
    fn id(&self) -> ProviderId;

    /// Declares the URI schemes routed to this provider.
    ///
    /// Providers whose id equals their sole scheme may keep the default.
    fn schemes(&self) -> &'static [&'static str] {
        &[]
    }

    /// Validates an opaque location before any provider operation.
    ///
    /// The default supports providers whose id and URI scheme are identical.
    fn validate_location(&self, location: &Location) -> Result<(), VfsError> {
        let provider_id = self.id();
        let scheme = location.scheme().map_err(|_| VfsError::InvalidLocation {
            location: location.uri.clone(),
        })?;
        let owned_scheme = if self.schemes().is_empty() {
            scheme == provider_id.as_str()
        } else {
            self.schemes().contains(&scheme)
        };
        if location.provider_id == provider_id && owned_scheme {
            Ok(())
        } else {
            Err(VfsError::InvalidLocation {
                location: location.uri.clone(),
            })
        }
    }

    /// Reports the operations this provider currently supports.
    fn capabilities(&self) -> ProviderCapabilities;

    /// Reports capabilities for a concrete location when a provider contains backends with
    /// different support levels. Providers with uniform behavior inherit the static result.
    fn capabilities_for(&self, _location: &Location) -> Result<ProviderCapabilities, VfsError> {
        Ok(self.capabilities())
    }

    /// Reports the transfer-planning capabilities of a concrete location
    /// (task 0108), including the [`TransferEndpoint`] identifying which
    /// backend it lives on.
    ///
    /// The default derives them from [`Self::capabilities_for`] and uses the
    /// provider id as the endpoint, which is only correct for providers with a
    /// single backend (such as the local filesystem). Any provider that can
    /// address more than one host, connection or volume **must** override this
    /// so two different connections do not compare equal — otherwise the
    /// operation planner would pick a same-backend fast path across
    /// unrelated servers.
    fn transfer_capabilities(&self, location: &Location) -> Result<TransferCapabilities, VfsError> {
        Ok(TransferCapabilities::from_provider_capabilities(
            TransferEndpoint::new(self.id().to_string()),
            self.capabilities_for(location)?,
        ))
    }

    /// Reports how this provider's directory contents can be tracked for
    /// external changes (task 0109).
    ///
    /// The default derives a [`ChangeTracking`] from [`Self::capabilities`]:
    /// providers advertising [`ProviderCapabilities::WATCH`] are assumed to
    /// stream real native events through [`Self::watch`], everything else is
    /// [`ChangeTracking::Unsupported`]. Providers whose [`Self::watch`] is
    /// backed by a remote delta/sync-token API, or that only support
    /// conservative polling, must override this.
    fn change_tracking(&self) -> ChangeTracking {
        if self.capabilities().contains(ProviderCapabilities::WATCH) {
            ChangeTracking::NativeWatch
        } else {
            ChangeTracking::Unsupported
        }
    }

    /// Lists one page of a directory.
    async fn list(
        &self,
        location: &Location,
        options: ListOptions,
        cancellation: CancellationToken,
    ) -> Result<DirectoryPage, VfsError>;

    /// Fetches detailed metadata for an entry.
    async fn metadata(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntryMetadata, VfsError>;

    /// Inspects one entry without following symbolic links.
    async fn inspect(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntrySummary, VfsError> {
        let _ = (entry, cancellation);
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::LIST,
        })
    }

    /// Returns a regular file's logical byte length for operation planning.
    async fn file_size(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<u64, VfsError> {
        let _ = (entry, cancellation);
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::READ,
        })
    }

    /// Creates a child directory at a location.
    async fn create_directory(
        &self,
        location: &Location,
        name: &str,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError>;

    /// Renames or moves an entry to a destination location.
    async fn rename(
        &self,
        source: &EntryRef,
        destination: &Location,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError>;

    /// Removes an entry according to the supplied safety options.
    async fn remove(
        &self,
        entry: &EntryRef,
        options: RemoveOptions,
        cancellation: CancellationToken,
    ) -> Result<(), VfsError>;

    /// Opens an entry as a streaming reader.
    async fn open_read(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError>;

    /// Opens a bounded byte range of an entry as a streaming reader (task 0088), for random-access
    /// consumers such as a large-file viewer that must not read from the start of the file to
    /// reach an arbitrary offset.
    ///
    /// `length` bounds how many bytes the returned stream yields; `None` reads to the end of the
    /// entry. The default implementation reports [`ProviderCapabilities::RANDOM_ACCESS`] as
    /// unsupported; callers without that capability may fall back to skip-reading through
    /// [`Self::open_read`] instead (a documented reduced-capability path, see task 0088).
    async fn read_range(
        &self,
        entry: &EntryRef,
        offset: u64,
        length: Option<u64>,
        cancellation: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError> {
        let _ = (entry, offset, length, cancellation);
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::RANDOM_ACCESS,
        })
    }

    /// Opens a destination as a streaming writer.
    async fn open_write(
        &self,
        destination: &Location,
        options: WriteOptions,
        cancellation: CancellationToken,
    ) -> Result<ProviderWriteStream, VfsError>;

    /// Opens a destination as a streaming writer when the caller already
    /// knows the exact final size of the data it is about to write (task
    /// 0110), for example the operation engine copying a source whose
    /// `file_size` was already read during planning.
    ///
    /// Knowing the size upfront lets providers whose native upload protocol
    /// requires it declared in advance (for example Microsoft Graph's
    /// resumable upload sessions, where every chunk's `Content-Range` header
    /// must state the total) drive a real bounded-memory, resumable upload
    /// instead of buffering the whole payload or refusing the request. The
    /// default forwards to [`Self::open_write`] and ignores `expected_size`,
    /// which is exactly correct for every provider whose native write path
    /// does not need the size ahead of time (the local filesystem, SFTP,
    /// FTP, WebDAV, S3): this method exists purely as an optional,
    /// backward-compatible extension point, not a second write path every
    /// provider must reason about.
    async fn open_write_sized(
        &self,
        destination: &Location,
        options: WriteOptions,
        expected_size: u64,
        cancellation: CancellationToken,
    ) -> Result<ProviderWriteStream, VfsError> {
        let _ = expected_size;
        self.open_write(destination, options, cancellation).await
    }

    /// Attempts a provider-native clone into a private temporary destination.
    ///
    /// `false` requests the caller's streaming fallback without exposing a destination.
    async fn server_side_copy(
        &self,
        source: &EntryRef,
        temporary: &Location,
        cancellation: CancellationToken,
    ) -> Result<bool, VfsError> {
        let _ = (source, temporary, cancellation);
        Ok(false)
    }

    /// Atomically publishes a completed temporary copy and applies supported source metadata.
    async fn commit_copy(
        &self,
        source: &EntryRef,
        temporary: &Location,
        destination: &Location,
        options: CopyCommitOptions,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        let _ = (source, temporary, destination, options, cancellation);
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::WRITE,
        })
    }

    /// Removes a provider-private temporary copy after failure or cancellation.
    async fn discard_copy(
        &self,
        temporary: &Location,
        cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        let _ = (temporary, cancellation);
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::WRITE,
        })
    }

    /// Copies a symbolic link itself rather than following its target.
    async fn copy_symlink(
        &self,
        source: &EntryRef,
        destination: &Location,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        let _ = (source, destination, cancellation);
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::WRITE,
        })
    }

    /// Resolves a symbolic link for an explicit copy-target request.
    async fn resolve_symlink(
        &self,
        source: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntrySummary, VfsError> {
        let _ = (source, cancellation);
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::READ,
        })
    }

    /// Applies supported timestamps and permissions from one entry to another.
    async fn preserve_metadata(
        &self,
        source: &EntryRef,
        destination: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        let _ = (source, destination, cancellation);
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::SET_TIMESTAMPS,
        })
    }

    /// Reports whether an atomic rename can span the two locations.
    async fn same_filesystem(
        &self,
        source: &EntryRef,
        destination_directory: &Location,
        cancellation: CancellationToken,
    ) -> Result<bool, VfsError> {
        let _ = (source, destination_directory, cancellation);
        Ok(false)
    }

    /// Watches a location for incremental directory changes.
    async fn watch(
        &self,
        location: &Location,
        cancellation: CancellationToken,
    ) -> Result<ProviderChangeStream, VfsError>;
}
