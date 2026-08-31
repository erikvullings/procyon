use async_trait::async_trait;
use fm_domain::{EntryId, EntryKind, EntryMetadata, EntrySummary, Location, ProviderId};
use fm_vfs::{
    CONSERVATIVE_POLL_INTERVAL, ChangeTracking, CopyCommitOptions, DirectoryPage, EntryRef,
    FileSystemProvider, ListOptions, ProviderCapabilities, ProviderChangeStream,
    ProviderReadStream, ProviderWriteStream, RemoveOptions, TransferCapabilities, TransferEndpoint,
    VfsError, WriteOptions,
};
use rustls_platform_verifier::ConfigVerifierExt;
use std::{collections::BTreeMap, str::FromStr, sync::Arc};
use suppaftp::{
    Mode,
    list::File,
    tokio::{AsyncFtpStream, AsyncRustlsConnector, AsyncRustlsFtpStream},
};
use tokio_util::sync::CancellationToken;

/// Resolved connection parameters. The password must never be logged.
#[derive(Clone)]
pub struct FtpConnectionParameters {
    /// Hostname, also used for TLS identity verification.
    pub host: String,
    /// Control port.
    pub port: u16,
    /// Login name.
    pub username: String,
    /// Login password.
    pub password: String,
    /// Whether explicit TLS is required.
    pub explicit_tls: bool,
}
/// Resolves an opaque connection id to configuration and credentials.
#[async_trait]
pub trait FtpConnectionResolver: Send + Sync {
    /// Resolve one saved connection.
    async fn resolve(&self, id: &str) -> Result<FtpConnectionParameters, VfsError>;
}
enum Client {
    Plain(AsyncFtpStream),
    Secure(AsyncRustlsFtpStream),
}
impl Client {
    async fn connect(p: &FtpConnectionParameters) -> Result<Self, VfsError> {
        let address = format!("{}:{}", p.host, p.port);
        if p.explicit_tls {
            let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
            let stream = AsyncRustlsFtpStream::connect(address)
                .await
                .map_err(map_ftp)?;
            let config =
                tokio_rustls::rustls::ClientConfig::with_platform_verifier().map_err(|error| {
                    VfsError::Io {
                        message: error.to_string(),
                    }
                })?;
            let connector =
                AsyncRustlsConnector::from(tokio_rustls::TlsConnector::from(Arc::new(config)));
            let mut stream = stream
                .into_secure(connector, &p.host)
                .await
                .map_err(map_ftp)?;
            stream
                .login(&p.username, &p.password)
                .await
                .map_err(map_ftp)?;
            stream.set_mode(Mode::Passive);
            Ok(Self::Secure(stream))
        } else {
            let mut stream = AsyncFtpStream::connect(address).await.map_err(map_ftp)?;
            stream
                .login(&p.username, &p.password)
                .await
                .map_err(map_ftp)?;
            stream.set_mode(Mode::Passive);
            Ok(Self::Plain(stream))
        }
    }
    async fn list(&mut self, p: &str) -> Result<Vec<String>, VfsError> {
        match self {
            Self::Plain(c) => c.list(Some(p)).await,
            Self::Secure(c) => c.list(Some(p)).await,
        }
        .map_err(map_ftp)
    }
    async fn mkdir(&mut self, p: &str) -> Result<(), VfsError> {
        match self {
            Self::Plain(c) => c.mkdir(p).await,
            Self::Secure(c) => c.mkdir(p).await,
        }
        .map_err(map_ftp)
    }
    async fn rename(&mut self, a: &str, b: &str) -> Result<(), VfsError> {
        match self {
            Self::Plain(c) => c.rename(a, b).await,
            Self::Secure(c) => c.rename(a, b).await,
        }
        .map_err(map_ftp)
    }
    async fn rm(&mut self, p: &str) -> Result<(), VfsError> {
        match self {
            Self::Plain(c) => c.rm(p).await,
            Self::Secure(c) => c.rm(p).await,
        }
        .map_err(map_ftp)
    }
    async fn rmdir(&mut self, p: &str) -> Result<(), VfsError> {
        match self {
            Self::Plain(c) => c.rmdir(p).await,
            Self::Secure(c) => c.rmdir(p).await,
        }
        .map_err(map_ftp)
    }
    async fn size(&mut self, p: &str) -> Result<u64, VfsError> {
        match self {
            Self::Plain(c) => c.size(p).await,
            Self::Secure(c) => c.size(p).await,
        }
        .map(|v| v as u64)
        .map_err(map_ftp)
    }
}
/// VFS provider for passive FTP and explicit FTPS.
pub struct FtpFileSystemProvider {
    resolver: Arc<dyn FtpConnectionResolver>,
}
impl FtpFileSystemProvider {
    /// Creates a provider.
    #[must_use]
    pub fn new(resolver: Arc<dyn FtpConnectionResolver>) -> Self {
        Self { resolver }
    }
    /// Verifies transport security and login without retaining a session.
    pub async fn verify_connectivity(p: &FtpConnectionParameters) -> Result<(), VfsError> {
        Client::connect(p).await.map(|_| ())
    }
    async fn client(&self, p: &Parsed) -> Result<Client, VfsError> {
        let resolved = self.resolver.resolve(&p.id).await?;
        if resolved.explicit_tls != p.secure {
            return Err(invalid(&p.uri));
        }
        Client::connect(&resolved).await
    }
}
#[async_trait]
impl FileSystemProvider for FtpFileSystemProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new("ftp")
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::LIST
            | ProviderCapabilities::READ
            | ProviderCapabilities::WRITE
            | ProviderCapabilities::CREATE_DIRECTORY
            | ProviderCapabilities::RENAME
            | ProviderCapabilities::MOVE
            | ProviderCapabilities::DELETE
    }
    /// Task 0108. The endpoint identifies the concrete connection — its id
    /// *and* its transport security, because `ftp://<id>/` and
    /// `ftps://<id>/` are rejected against each other by
    /// [`FtpFileSystemProvider::client`] and so are never one backend.
    ///
    /// `server_side_move` is `true` (`RNFR`/`RNTO` within one connection).
    /// FTP has no server-side copy. `REST`-based resumption and offset
    /// reads/writes are not implemented here, so they stay `false` rather than
    /// tempting the planner into a fast path this provider cannot honour.
    fn transfer_capabilities(&self, l: &Location) -> Result<TransferCapabilities, VfsError> {
        let p = Parsed::parse(l)?;
        let scheme = if p.secure { "ftps" } else { "ftp" };
        Ok(TransferCapabilities {
            endpoint: TransferEndpoint::new(format!("{scheme}:{}", p.id)),
            server_side_copy: false,
            server_side_move: true,
            resumable_upload: false,
            resumable_download: false,
            random_read: false,
            random_write: false,
        })
    }
    /// FTP/FTPS has no native change-notification mechanism (task 0106
    /// deliberately does not fake `WATCH`); `fm-application`'s directory
    /// service instead polls `list` conservatively (task 0109).
    fn change_tracking(&self) -> ChangeTracking {
        ChangeTracking::Poll {
            interval: CONSERVATIVE_POLL_INTERVAL,
        }
    }
    async fn list(
        &self,
        l: &Location,
        o: ListOptions,
        c: CancellationToken,
    ) -> Result<DirectoryPage, VfsError> {
        cancelled(&c)?;
        if o.page_size == 0 {
            return Err(invalid(&l.uri));
        }
        let p = Parsed::parse(l)?;
        let lines = self.client(&p).await?.list(&p.path).await?;
        cancelled(&c)?;
        let offset = o
            .continuation_token
            .as_deref()
            .unwrap_or("0")
            .parse::<usize>()
            .map_err(|_| invalid(&l.uri))?;
        let files = lines
            .iter()
            .filter_map(|v| File::from_str(v).ok())
            .collect::<Vec<_>>();
        let entries = files
            .iter()
            .skip(offset)
            .take(o.page_size)
            .map(|f| Ok(summary(l.join(f.name()).map_err(|_| invalid(&l.uri))?, f)))
            .collect::<Result<Vec<_>, VfsError>>()?;
        let next = offset + entries.len();
        let has_more = next < files.len();
        Ok(DirectoryPage {
            entries,
            total_known_entries: Some(files.len() as u64),
            has_more,
            continuation_token: has_more.then(|| next.to_string()),
        })
    }
    async fn metadata(
        &self,
        e: &EntryRef,
        c: CancellationToken,
    ) -> Result<EntryMetadata, VfsError> {
        cancelled(&c)?;
        Ok(EntryMetadata {
            entry_id: e.id,
            permissions: None,
            ownership: None,
            extended_attributes: BTreeMap::new(),
            checksums: BTreeMap::new(),
            image_dimensions: None,
            media: None,
            archive: None,
            plugin_fields: BTreeMap::new(),
        })
    }
    async fn inspect(&self, e: &EntryRef, c: CancellationToken) -> Result<EntrySummary, VfsError> {
        let parent = e
            .location
            .parent()
            .map_err(|_| invalid(&e.location.uri))?
            .ok_or_else(|| invalid(&e.location.uri))?;
        let name = e.location.name().map_err(|_| invalid(&e.location.uri))?;
        self.list(&parent, ListOptions::default(), c)
            .await?
            .entries
            .into_iter()
            .find(|v| v.name == name)
            .ok_or_else(|| VfsError::NotFound {
                location: e.location.uri.clone(),
            })
    }
    async fn file_size(&self, e: &EntryRef, c: CancellationToken) -> Result<u64, VfsError> {
        cancelled(&c)?;
        let p = Parsed::parse(&e.location)?;
        self.client(&p).await?.size(&p.path).await
    }
    async fn create_directory(
        &self,
        l: &Location,
        n: &str,
        c: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        cancelled(&c)?;
        let d = l.join(n).map_err(|_| invalid(&l.uri))?;
        let p = Parsed::parse(&d)?;
        self.client(&p).await?.mkdir(&p.path).await?;
        Ok(entry(d))
    }
    async fn rename(
        &self,
        s: &EntryRef,
        d: &Location,
        c: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        cancelled(&c)?;
        let a = Parsed::parse(&s.location)?;
        let b = Parsed::parse(d)?;
        if a.id != b.id || a.secure != b.secure {
            return Err(invalid(&d.uri));
        }
        self.client(&a).await?.rename(&a.path, &b.path).await?;
        Ok(entry(d.clone()))
    }
    async fn remove(
        &self,
        e: &EntryRef,
        o: RemoveOptions,
        c: CancellationToken,
    ) -> Result<(), VfsError> {
        cancelled(&c)?;
        if o.use_trash {
            return Err(VfsError::UnsupportedCapability {
                capability: ProviderCapabilities::DELETE,
            });
        }
        let p = Parsed::parse(&e.location)?;
        let mut client = self.client(&p).await?;
        if client.rm(&p.path).await.is_ok() {
            Ok(())
        } else {
            client.rmdir(&p.path).await
        }
    }
    async fn open_read(
        &self,
        e: &EntryRef,
        c: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError> {
        cancelled(&c)?;
        let p = Parsed::parse(&e.location)?;
        let mut client = self.client(&p).await?;
        let (mut tx, rx) = tokio::io::duplex(65536);
        tokio::spawn(async move {
            match &mut client {
                Client::Plain(v) => {
                    if let Ok(mut data) = v.retr_as_stream(p.path).await {
                        let _ = tokio::select! {r=tokio::io::copy(&mut data,&mut tx)=>r,_=c.cancelled()=>Ok(0)};
                        let _ = v.finalize_retr_stream(data).await;
                    }
                }
                Client::Secure(v) => {
                    if let Ok(mut data) = v.retr_as_stream(p.path).await {
                        let _ = tokio::select! {r=tokio::io::copy(&mut data,&mut tx)=>r,_=c.cancelled()=>Ok(0)};
                        let _ = v.finalize_retr_stream(data).await;
                    }
                }
            }
        });
        Ok(Box::pin(rx))
    }
    async fn open_write(
        &self,
        d: &Location,
        o: WriteOptions,
        c: CancellationToken,
    ) -> Result<ProviderWriteStream, VfsError> {
        cancelled(&c)?;
        if !o.overwrite
            && self
                .inspect(&entry(d.clone()), CancellationToken::new())
                .await
                .is_ok()
        {
            return Err(VfsError::AlreadyExists {
                location: d.uri.clone(),
            });
        }
        let p = Parsed::parse(d)?;
        let mut client = self.client(&p).await?;
        let (tx, mut rx) = tokio::io::duplex(65536);
        tokio::spawn(async move {
            match &mut client {
                Client::Plain(v) => {
                    if let Ok(mut data) = v.put_with_stream(p.path).await {
                        let _ = tokio::select! {r=tokio::io::copy(&mut rx,&mut data)=>r,_=c.cancelled()=>Ok(0)};
                        let _ = v.finalize_put_stream(data).await;
                    }
                }
                Client::Secure(v) => {
                    if let Ok(mut data) = v.put_with_stream(p.path).await {
                        let _ = tokio::select! {r=tokio::io::copy(&mut rx,&mut data)=>r,_=c.cancelled()=>Ok(0)};
                        let _ = v.finalize_put_stream(data).await;
                    }
                }
            }
        });
        Ok(Box::pin(tx))
    }
    async fn commit_copy(
        &self,
        _: &EntryRef,
        t: &Location,
        d: &Location,
        o: CopyCommitOptions,
        c: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        if !o.overwrite
            && self
                .inspect(&entry(d.clone()), CancellationToken::new())
                .await
                .is_ok()
        {
            return Err(VfsError::AlreadyExists {
                location: d.uri.clone(),
            });
        }
        self.rename(&entry(t.clone()), d, c).await
    }
    /// Discarding a temporary that was never created must succeed: the
    /// operation engine calls this on every cancellation and failure path,
    /// including ones that abort before `STOR` ever ran, and turning a
    /// missing temporary into an error would fail the *cleanup* of an
    /// already-cancelled operation (task 0108). Matches the local and SFTP
    /// providers, which both already swallow `NotFound` here.
    async fn discard_copy(&self, t: &Location, c: CancellationToken) -> Result<(), VfsError> {
        match self
            .remove(&entry(t.clone()), RemoveOptions::default(), c)
            .await
        {
            Ok(()) | Err(VfsError::NotFound { .. }) => Ok(()),
            Err(other) => Err(other),
        }
    }
    async fn same_filesystem(
        &self,
        s: &EntryRef,
        d: &Location,
        c: CancellationToken,
    ) -> Result<bool, VfsError> {
        cancelled(&c)?;
        let a = Parsed::parse(&s.location)?;
        let b = Parsed::parse(d)?;
        Ok(a.id == b.id && a.secure == b.secure)
    }
    async fn watch(
        &self,
        _: &Location,
        _: CancellationToken,
    ) -> Result<ProviderChangeStream, VfsError> {
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::WATCH,
        })
    }
}
struct Parsed {
    id: String,
    path: String,
    secure: bool,
    uri: String,
}
impl Parsed {
    fn parse(l: &Location) -> Result<Self, VfsError> {
        if l.provider_id.as_str() != "ftp" {
            return Err(invalid(&l.uri));
        }
        let (secure, rest) = if let Some(v) = l.uri.strip_prefix("ftps://") {
            (true, v)
        } else if let Some(v) = l.uri.strip_prefix("ftp://") {
            (false, v)
        } else {
            return Err(invalid(&l.uri));
        };
        let (id, path) = rest.split_once('/').ok_or_else(|| invalid(&l.uri))?;
        uuid::Uuid::parse_str(id).map_err(|_| invalid(&l.uri))?;
        Ok(Self {
            id: id.to_owned(),
            path: format!("/{path}"),
            secure,
            uri: l.uri.clone(),
        })
    }
}
fn cancelled(c: &CancellationToken) -> Result<(), VfsError> {
    if c.is_cancelled() {
        Err(VfsError::Cancelled)
    } else {
        Ok(())
    }
}
fn invalid(v: &str) -> VfsError {
    VfsError::InvalidLocation {
        location: v.to_owned(),
    }
}
fn map_ftp(e: suppaftp::FtpError) -> VfsError {
    let m = e.to_string();
    if m.contains("550") {
        VfsError::NotFound {
            location: "remote FTP path".to_owned(),
        }
    } else if m.contains("530") {
        VfsError::PermissionDenied {
            location: "FTP connection".to_owned(),
        }
    } else {
        VfsError::Io { message: m }
    }
}
fn entry(location: Location) -> EntryRef {
    EntryRef {
        id: EntryId::new(),
        location,
    }
}
fn summary(location: Location, f: &File) -> EntrySummary {
    let name = f.name().to_owned();
    let kind = if f.is_directory() {
        EntryKind::Directory
    } else if f.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::File
    };
    EntrySummary {
        id: EntryId::new(),
        location,
        name: name.clone(),
        kind,
        size: (kind == EntryKind::File).then(|| f.size() as u64),
        modified_at: Some(f.modified().into()),
        created_at: None,
        hidden: name.starts_with('.'),
        read_only: false,
        extension: std::path::Path::new(&name)
            .extension()
            .and_then(|v| v.to_str())
            .map(str::to_owned),
        mime_type: None,
        icon_key: None,
        metadata_revision: 0,
        git_status: None,
    }
}
