//! Test double: the local filesystem with offset writes switched on, standing in for a
//! provider such as SFTP that can append to a private partial (task 0303).

use async_trait::async_trait;
use fm_domain::{Location, ProviderId};
use fm_vfs::{
    EntryRef, FileSystemProvider, ListOptions, ProviderCapabilities, RemoveOptions,
    TransferCapabilities, TransferEndpoint, WriteOptions,
};
use tokio_util::sync::CancellationToken;

/// A destination that, like SFTP, can append to a private partial: the local filesystem
/// with offset writes switched on. Task 0303.
pub(crate) struct AppendableProvider {
    id: &'static str,
    inner: fm_vfs_local::LocalFileSystemProvider,
    /// Inspecting a private `.fm-copy-*` partial fails like a dropped connection.
    flaky_partial_inspect: bool,
}

impl AppendableProvider {
    pub(crate) fn new(id: &'static str) -> Self {
        Self {
            id,
            inner: fm_vfs_local::LocalFileSystemProvider::new(),
            flaky_partial_inspect: false,
        }
    }

    /// A provider whose link drops whenever a partial is inspected.
    pub(crate) fn with_flaky_partial_inspect(mut self) -> Self {
        self.flaky_partial_inspect = true;
        self
    }
}

#[async_trait]
impl FileSystemProvider for AppendableProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new(self.id)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.inner.capabilities()
    }

    fn schemes(&self) -> &'static [&'static str] {
        self.inner.schemes()
    }

    fn validate_location(&self, location: &Location) -> Result<(), fm_vfs::VfsError> {
        self.inner.validate_location(location)
    }

    fn transfer_capabilities(
        &self,
        _location: &Location,
    ) -> Result<TransferCapabilities, fm_vfs::VfsError> {
        Ok(TransferCapabilities {
            endpoint: TransferEndpoint::new("appendable-test"),
            server_side_copy: false,
            server_side_move: false,
            resumable_upload: true,
            resumable_download: false,
            random_read: false,
            random_write: true,
        })
    }

    async fn list(
        &self,
        location: &Location,
        options: ListOptions,
        cancellation: CancellationToken,
    ) -> Result<fm_vfs::DirectoryPage, fm_vfs::VfsError> {
        self.inner.list(location, options, cancellation).await
    }

    async fn metadata(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<fm_domain::EntryMetadata, fm_vfs::VfsError> {
        self.inner.metadata(entry, cancellation).await
    }

    async fn inspect(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<fm_domain::EntrySummary, fm_vfs::VfsError> {
        if self.flaky_partial_inspect
            && entry
                .location
                .name()
                .is_ok_and(|name| name.starts_with(".fm-copy-"))
        {
            return Err(fm_vfs::VfsError::Io {
                message: "connection reset".into(),
            });
        }
        self.inner.inspect(entry, cancellation).await
    }

    async fn create_directory(
        &self,
        location: &Location,
        name: &str,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, fm_vfs::VfsError> {
        self.inner
            .create_directory(location, name, cancellation)
            .await
    }

    async fn rename(
        &self,
        source: &EntryRef,
        destination: &Location,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, fm_vfs::VfsError> {
        self.inner.rename(source, destination, cancellation).await
    }

    async fn remove(
        &self,
        entry: &EntryRef,
        options: RemoveOptions,
        cancellation: CancellationToken,
    ) -> Result<(), fm_vfs::VfsError> {
        self.inner.remove(entry, options, cancellation).await
    }

    async fn open_read(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<fm_vfs::ProviderReadStream, fm_vfs::VfsError> {
        self.inner.open_read(entry, cancellation).await
    }

    async fn open_write(
        &self,
        destination: &Location,
        options: WriteOptions,
        cancellation: CancellationToken,
    ) -> Result<fm_vfs::ProviderWriteStream, fm_vfs::VfsError> {
        self.inner
            .open_write(destination, options, cancellation)
            .await
    }

    async fn open_write_at(
        &self,
        partial: &Location,
        offset: u64,
        _cancellation: CancellationToken,
    ) -> Result<fm_vfs::ProviderWriteStream, fm_vfs::VfsError> {
        use tokio::io::AsyncSeekExt;
        let path = partial.to_native_path().expect("native path");
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .await
            .map_err(|error| fm_vfs::VfsError::NotFound {
                location: error.to_string(),
            })?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|error| fm_vfs::VfsError::Io {
                message: error.to_string(),
            })?;
        Ok(Box::pin(file))
    }

    async fn commit_copy(
        &self,
        source: &EntryRef,
        temporary: &Location,
        destination: &Location,
        options: fm_vfs::CopyCommitOptions,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, fm_vfs::VfsError> {
        self.inner
            .commit_copy(source, temporary, destination, options, cancellation)
            .await
    }

    async fn discard_copy(
        &self,
        temporary: &Location,
        cancellation: CancellationToken,
    ) -> Result<(), fm_vfs::VfsError> {
        self.inner.discard_copy(temporary, cancellation).await
    }

    async fn watch(
        &self,
        _location: &Location,
        _cancellation: CancellationToken,
    ) -> Result<fm_vfs::ProviderChangeStream, fm_vfs::VfsError> {
        unreachable!("resume tests never watch")
    }
}
