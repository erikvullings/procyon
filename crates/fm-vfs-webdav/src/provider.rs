use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use fm_domain::{EntryId, EntryKind, EntryMetadata, EntrySummary, Location, ProviderId};
use fm_vfs::{
    CONSERVATIVE_POLL_INTERVAL, ChangeTracking, CopyCommitOptions, DirectoryPage, EntryRef,
    FileSystemProvider, ListOptions, ProviderCapabilities, ProviderChangeStream,
    ProviderReadStream, ProviderWriteStream, RemoveOptions, TransferCapabilities, TransferEndpoint,
    VfsError, WriteOptions,
};
use futures::StreamExt;
use reqwest::{Method, StatusCode};
use tokio::sync::RwLock;
use tokio_util::io::{ReaderStream, StreamReader};
use tokio_util::sync::CancellationToken;

use crate::digest::{DigestChallenge, generate_client_nonce};
use crate::xml::parse_multistatus;

/// How a WebDAV connection authenticates. Mirrors
/// `fm_connections::WebDavAuthenticationScheme` (`fm-vfs-webdav` must not
/// depend on `fm-connections`, matching the SSH/FTP precedent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebDavAuthScheme {
    /// HTTP Basic authentication.
    Basic,
    /// HTTP Digest authentication (RFC 2617/7616, `MD5`/`MD5-sess`, `qop=auth`).
    Digest,
}

/// Resolved connection parameters. The password must never be logged.
#[derive(Clone)]
pub struct WebDavConnectionParameters {
    /// The WebDAV collection's base URL, e.g.
    /// `https://cloud.example.test/remote.php/dav/files/erik`.
    pub base_url: String,
    /// Login name.
    pub username: String,
    /// Login password.
    pub password: String,
    /// Authentication scheme to use.
    pub auth_scheme: WebDavAuthScheme,
}

/// Resolves an opaque connection id to configuration and credentials.
#[async_trait]
pub trait WebDavConnectionResolver: Send + Sync {
    /// Resolve one saved connection.
    async fn resolve(&self, id: &str) -> Result<WebDavConnectionParameters, VfsError>;
}

struct CachedDigest {
    challenge: DigestChallenge,
    nonce_count: AtomicU32,
}

/// VFS provider for WebDAV (RFC 4918) over HTTP(S), supporting Basic and
/// Digest authentication.
///
/// TLS certificate validation is real: this provider issues every request
/// through a plain `reqwest::Client`, whose `rustls` backend (this
/// workspace's default, see `Cargo.toml`) validates certificates against the
/// platform trust store by default and offers no
/// `danger_accept_invalid_certs` opt-in anywhere in this crate.
pub struct WebDavFileSystemProvider {
    resolver: Arc<dyn WebDavConnectionResolver>,
    client: reqwest::Client,
    digest_cache: RwLock<HashMap<String, Arc<CachedDigest>>>,
    range_support: RwLock<HashMap<String, bool>>,
}

struct Parsed {
    connection_id: String,
    segments: Vec<String>,
    uri: String,
}

impl Parsed {
    fn parse(location: &Location) -> Result<Self, VfsError> {
        if location.provider_id.as_str() != "webdav" {
            return Err(invalid(&location.uri));
        }
        let remainder = location
            .uri
            .strip_prefix("webdav://")
            .ok_or_else(|| invalid(&location.uri))?;
        let (connection_id, path) = remainder
            .split_once('/')
            .ok_or_else(|| invalid(&location.uri))?;
        uuid::Uuid::parse_str(connection_id).map_err(|_| invalid(&location.uri))?;
        let segments = if path.is_empty() {
            Vec::new()
        } else {
            path.split('/')
                .map(|segment| percent_decode(segment).ok_or_else(|| invalid(&location.uri)))
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Self {
            connection_id: connection_id.to_owned(),
            segments,
            uri: location.uri.clone(),
        })
    }
}

fn percent_decode(segment: &str) -> Option<String> {
    percent_encoding::percent_decode_str(segment)
        .decode_utf8()
        .ok()
        .map(std::borrow::Cow::into_owned)
}

impl WebDavFileSystemProvider {
    /// Creates a provider.
    #[must_use]
    pub fn new(resolver: Arc<dyn WebDavConnectionResolver>) -> Self {
        Self {
            resolver,
            client: reqwest::Client::new(),
            digest_cache: RwLock::new(HashMap::new()),
            range_support: RwLock::new(HashMap::new()),
        }
    }

    /// Verifies transport security and login without retaining a session.
    pub async fn verify_connectivity(
        parameters: &WebDavConnectionParameters,
    ) -> Result<(), VfsError> {
        let client = reqwest::Client::new();
        let url = base_url(parameters)?;
        let digest_cache = RwLock::new(HashMap::new());
        let response = execute(
            &client,
            &parameters.base_url,
            parameters,
            &digest_cache,
            propfind_method(),
            url,
            Some(("Depth", "0")),
            Some(PROPFIND_BODY.as_bytes().to_vec()),
        )
        .await?;
        if response.status().is_success() || response.status() == StatusCode::MULTI_STATUS {
            Ok(())
        } else {
            Err(map_status(response.status(), "webdav connection"))
        }
    }

    fn url_for(
        &self,
        parsed: &Parsed,
        parameters: &WebDavConnectionParameters,
    ) -> Result<url::Url, VfsError> {
        let mut url = base_url(parameters)?;
        {
            let mut segments = url.path_segments_mut().map_err(|()| invalid(&parsed.uri))?;
            segments.pop_if_empty();
            for segment in &parsed.segments {
                segments.push(segment);
            }
        }
        Ok(url)
    }

    async fn resolve(&self, parsed: &Parsed) -> Result<WebDavConnectionParameters, VfsError> {
        self.resolver.resolve(&parsed.connection_id).await
    }

    async fn request(
        &self,
        parameters: &WebDavConnectionParameters,
        method: Method,
        url: url::Url,
        extra_header: Option<(&str, &str)>,
        body: Option<Vec<u8>>,
    ) -> Result<reqwest::Response, VfsError> {
        execute(
            &self.client,
            &parameters.base_url,
            parameters,
            &self.digest_cache,
            method,
            url,
            extra_header,
            body,
        )
        .await
    }

    async fn probe_range_support(
        &self,
        connection_id: &str,
        parameters: &WebDavConnectionParameters,
    ) -> bool {
        if let Some(cached) = self.range_support.read().await.get(connection_id) {
            return *cached;
        }
        let Ok(base) = base_url(parameters) else {
            return false;
        };
        let supported = self
            .request(parameters, Method::HEAD, base, None, None)
            .await
            .map(|response| {
                response
                    .headers()
                    .get(reqwest::header::ACCEPT_RANGES)
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| value.contains("bytes"))
            })
            .unwrap_or(false);
        self.range_support
            .write()
            .await
            .insert(connection_id.to_owned(), supported);
        supported
    }
}

const PROPFIND_BODY: &str =
    r#"<?xml version="1.0" encoding="utf-8"?><D:propfind xmlns:D="DAV:"><D:allprop/></D:propfind>"#;

fn propfind_method() -> Method {
    Method::from_bytes(b"PROPFIND").expect("PROPFIND is a valid HTTP method token")
}
fn mkcol_method() -> Method {
    Method::from_bytes(b"MKCOL").expect("MKCOL is a valid HTTP method token")
}
fn move_method() -> Method {
    Method::from_bytes(b"MOVE").expect("MOVE is a valid HTTP method token")
}
fn copy_method() -> Method {
    Method::from_bytes(b"COPY").expect("COPY is a valid HTTP method token")
}

fn base_url(parameters: &WebDavConnectionParameters) -> Result<url::Url, VfsError> {
    url::Url::parse(&parameters.base_url).map_err(|_| VfsError::InvalidLocation {
        location: parameters.base_url.clone(),
    })
}

#[allow(clippy::too_many_arguments)]
async fn execute(
    client: &reqwest::Client,
    cache_key: &str,
    parameters: &WebDavConnectionParameters,
    digest_cache: &RwLock<HashMap<String, Arc<CachedDigest>>>,
    method: Method,
    url: url::Url,
    extra_header: Option<(&str, &str)>,
    body: Option<Vec<u8>>,
) -> Result<reqwest::Response, VfsError> {
    let path = url.path().to_owned();
    let build = |body: Option<Vec<u8>>| {
        let mut builder = client.request(method.clone(), url.clone());
        if let Some((name, value)) = extra_header {
            builder = builder.header(name, value);
        }
        if let Some(body) = body {
            builder = builder.body(body);
        }
        builder
    };

    match parameters.auth_scheme {
        WebDavAuthScheme::Basic => build(body)
            .basic_auth(&parameters.username, Some(&parameters.password))
            .send()
            .await
            .map_err(map_reqwest),
        WebDavAuthScheme::Digest => {
            let cached = digest_cache.read().await.get(cache_key).cloned();
            if let Some(cached) = cached {
                let nc = cached.nonce_count.fetch_add(1, Ordering::SeqCst) + 1;
                let cnonce = generate_client_nonce();
                let header = cached.challenge.authorization(
                    &parameters.username,
                    &parameters.password,
                    method.as_str(),
                    &path,
                    nc,
                    &cnonce,
                );
                let response = build(body.clone())
                    .header(reqwest::header::AUTHORIZATION, header)
                    .send()
                    .await
                    .map_err(map_reqwest)?;
                if response.status() != StatusCode::UNAUTHORIZED {
                    return Ok(response);
                }
                // Stale or rejected cached nonce: fall through and refresh below.
            }

            let unauthenticated = build(body.clone()).send().await.map_err(map_reqwest)?;
            if unauthenticated.status() != StatusCode::UNAUTHORIZED {
                return Ok(unauthenticated);
            }
            let Some(header) = unauthenticated
                .headers()
                .get(reqwest::header::WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
            else {
                return Ok(unauthenticated);
            };
            let challenge = DigestChallenge::parse(&header).map_err(|error| VfsError::Io {
                message: error.to_string(),
            })?;
            let cnonce = generate_client_nonce();
            let auth_header = challenge.authorization(
                &parameters.username,
                &parameters.password,
                method.as_str(),
                &path,
                1,
                &cnonce,
            );
            digest_cache.write().await.insert(
                cache_key.to_owned(),
                Arc::new(CachedDigest {
                    challenge,
                    nonce_count: AtomicU32::new(1),
                }),
            );
            build(body)
                .header(reqwest::header::AUTHORIZATION, auth_header)
                .send()
                .await
                .map_err(map_reqwest)
        }
    }
}

fn map_reqwest(error: reqwest::Error) -> VfsError {
    VfsError::Io {
        message: error.to_string(),
    }
}

fn map_status(status: StatusCode, location: &str) -> VfsError {
    match status {
        StatusCode::NOT_FOUND => VfsError::NotFound {
            location: location.to_owned(),
        },
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => VfsError::PermissionDenied {
            location: location.to_owned(),
        },
        StatusCode::LOCKED => VfsError::Locked {
            location: location.to_owned(),
        },
        StatusCode::PRECONDITION_FAILED | StatusCode::CONFLICT => VfsError::AlreadyExists {
            location: location.to_owned(),
        },
        other => VfsError::Io {
            message: format!("WebDAV request failed with status {other}"),
        },
    }
}

fn cancelled(cancellation: &CancellationToken) -> Result<(), VfsError> {
    if cancellation.is_cancelled() {
        Err(VfsError::Cancelled)
    } else {
        Ok(())
    }
}

fn invalid(location: &str) -> VfsError {
    VfsError::InvalidLocation {
        location: location.to_owned(),
    }
}

fn entry(location: Location) -> EntryRef {
    EntryRef {
        id: EntryId::new(),
        location,
    }
}

fn decoded_path(text: &str) -> String {
    // `href`/URL text may include a scheme+authority or be server-relative;
    // only the path component matters for comparison and name extraction.
    let path = url::Url::parse(text)
        .map(|url| url.path().to_owned())
        .unwrap_or_else(|_| text.to_owned());
    percent_encoding::percent_decode_str(&path)
        .decode_utf8()
        .map(std::borrow::Cow::into_owned)
        .unwrap_or(path)
}

#[async_trait]
impl FileSystemProvider for WebDavFileSystemProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new("webdav")
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::LIST
            | ProviderCapabilities::READ
            | ProviderCapabilities::WRITE
            | ProviderCapabilities::CREATE_DIRECTORY
            | ProviderCapabilities::RENAME
            | ProviderCapabilities::MOVE
            | ProviderCapabilities::SERVER_SIDE_COPY
            | ProviderCapabilities::DELETE
    }

    /// Task 0108. `server_side_copy`/`server_side_move` are `true` — WebDAV's
    /// native `COPY`/`MOVE` methods. `random_read` reflects whether this
    /// connection's server advertised `Accept-Ranges: bytes` the last time it
    /// was probed (never assumed, see [`Self::probe_range_support`]);
    /// `random_write` stays `false` (no provider-side offset-write primitive
    /// is implemented).
    fn transfer_capabilities(&self, location: &Location) -> Result<TransferCapabilities, VfsError> {
        let parsed = Parsed::parse(location)?;
        let random_read = self
            .range_support
            .try_read()
            .ok()
            .and_then(|cache| cache.get(&parsed.connection_id).copied())
            .unwrap_or(false);
        Ok(TransferCapabilities {
            endpoint: TransferEndpoint::new(format!("webdav:{}", parsed.connection_id)),
            server_side_copy: true,
            server_side_move: true,
            resumable_upload: false,
            resumable_download: false,
            random_read,
            random_write: false,
        })
    }

    /// WebDAV has no native change-notification mechanism; `fm-application`'s
    /// directory service polls conservatively instead (task 0109).
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
        cancelled(&cancellation)?;
        if options.page_size == 0 {
            return Err(invalid(&location.uri));
        }
        let parsed = Parsed::parse(location)?;
        let parameters = self.resolve(&parsed).await?;
        self.probe_range_support(&parsed.connection_id, &parameters)
            .await;
        let url = self.url_for(&parsed, &parameters)?;
        let response = self
            .request(
                &parameters,
                propfind_method(),
                url.clone(),
                Some(("Depth", "1")),
                Some(PROPFIND_BODY.as_bytes().to_vec()),
            )
            .await?;
        if !(response.status().is_success() || response.status() == StatusCode::MULTI_STATUS) {
            return Err(map_status(response.status(), &location.uri));
        }
        let body = response.bytes().await.map_err(map_reqwest)?;
        let dav_entries = parse_multistatus(&body).map_err(|error| VfsError::Io {
            message: error.to_string(),
        })?;
        cancelled(&cancellation)?;

        let self_path = decoded_path(url.as_str());
        let mut summaries = Vec::new();
        for dav_entry in dav_entries {
            let href_path = decoded_path(&dav_entry.href);
            if href_path.trim_end_matches('/') == self_path.trim_end_matches('/') {
                continue; // The collection's own PROPFIND response describes itself.
            }
            let name = href_path
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            if name.is_empty() {
                continue;
            }
            let child_location = location.join(&name).map_err(|_| invalid(&location.uri))?;
            summaries.push(EntrySummary {
                id: EntryId::new(),
                location: child_location,
                name: name.clone(),
                kind: if dav_entry.is_collection {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                },
                size: (!dav_entry.is_collection).then_some(dav_entry.content_length.unwrap_or(0)),
                modified_at: dav_entry.last_modified,
                created_at: None,
                hidden: name.starts_with('.'),
                read_only: false,
                extension: std::path::Path::new(&name)
                    .extension()
                    .and_then(|value| value.to_str())
                    .map(str::to_owned),
                mime_type: None,
                icon_key: None,
                metadata_revision: 0,
                git_status: None,
            });
        }

        let offset = options
            .continuation_token
            .as_deref()
            .unwrap_or("0")
            .parse::<usize>()
            .map_err(|_| invalid(&location.uri))?;
        let total = summaries.len();
        let page: Vec<_> = summaries
            .into_iter()
            .skip(offset)
            .take(options.page_size)
            .collect();
        let next = offset + page.len();
        let has_more = next < total;
        Ok(DirectoryPage {
            entries: page,
            total_known_entries: Some(total as u64),
            has_more,
            continuation_token: has_more.then(|| next.to_string()),
        })
    }

    async fn metadata(
        &self,
        entry_ref: &EntryRef,
        _cancellation: CancellationToken,
    ) -> Result<EntryMetadata, VfsError> {
        Ok(EntryMetadata {
            entry_id: entry_ref.id,
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

    async fn inspect(
        &self,
        entry_ref: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntrySummary, VfsError> {
        let parent = entry_ref
            .location
            .parent()
            .map_err(|_| invalid(&entry_ref.location.uri))?
            .ok_or_else(|| invalid(&entry_ref.location.uri))?;
        let name = entry_ref
            .location
            .name()
            .map_err(|_| invalid(&entry_ref.location.uri))?;
        self.list(&parent, ListOptions::default(), cancellation)
            .await?
            .entries
            .into_iter()
            .find(|candidate| candidate.name == name)
            .ok_or_else(|| VfsError::NotFound {
                location: entry_ref.location.uri.clone(),
            })
    }

    async fn file_size(
        &self,
        entry_ref: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<u64, VfsError> {
        self.inspect(entry_ref, cancellation)
            .await?
            .size
            .ok_or_else(|| VfsError::IsADirectory {
                location: entry_ref.location.uri.clone(),
            })
    }

    async fn create_directory(
        &self,
        location: &Location,
        name: &str,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        cancelled(&cancellation)?;
        let destination = location.join(name).map_err(|_| invalid(&location.uri))?;
        let parsed = Parsed::parse(&destination)?;
        let parameters = self.resolve(&parsed).await?;
        let url = self.url_for(&parsed, &parameters)?;
        let response = self
            .request(&parameters, mkcol_method(), url, None, None)
            .await?;
        if response.status().is_success() {
            Ok(entry(destination))
        } else {
            Err(map_status(response.status(), &destination.uri))
        }
    }

    async fn rename(
        &self,
        source: &EntryRef,
        destination: &Location,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        cancelled(&cancellation)?;
        let source_parsed = Parsed::parse(&source.location)?;
        let destination_parsed = Parsed::parse(destination)?;
        if source_parsed.connection_id != destination_parsed.connection_id {
            return Err(invalid(&destination.uri));
        }
        let parameters = self.resolve(&source_parsed).await?;
        let source_url = self.url_for(&source_parsed, &parameters)?;
        let destination_url = self.url_for(&destination_parsed, &parameters)?;
        let response = self
            .request(
                &parameters,
                move_method(),
                source_url,
                Some(("Destination", destination_url.as_str())),
                None,
            )
            .await?;
        if response.status().is_success() {
            Ok(entry(destination.clone()))
        } else {
            Err(map_status(response.status(), &destination.uri))
        }
    }

    async fn remove(
        &self,
        entry_ref: &EntryRef,
        options: RemoveOptions,
        cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        cancelled(&cancellation)?;
        if options.use_trash {
            return Err(VfsError::UnsupportedCapability {
                capability: ProviderCapabilities::DELETE,
            });
        }
        let parsed = Parsed::parse(&entry_ref.location)?;
        let parameters = self.resolve(&parsed).await?;
        let url = self.url_for(&parsed, &parameters)?;
        let response = self
            .request(&parameters, Method::DELETE, url, None, None)
            .await?;
        if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(map_status(response.status(), &entry_ref.location.uri))
        }
    }

    async fn open_read(
        &self,
        entry_ref: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError> {
        cancelled(&cancellation)?;
        let parsed = Parsed::parse(&entry_ref.location)?;
        let parameters = self.resolve(&parsed).await?;
        let url = self.url_for(&parsed, &parameters)?;
        let response = self
            .request(&parameters, Method::GET, url, None, None)
            .await?;
        if !response.status().is_success() {
            return Err(map_status(response.status(), &entry_ref.location.uri));
        }
        let stream = response
            .bytes_stream()
            .map(|chunk| chunk.map_err(std::io::Error::other));
        Ok(Box::pin(StreamReader::new(stream)))
    }

    async fn open_write(
        &self,
        destination: &Location,
        options: WriteOptions,
        cancellation: CancellationToken,
    ) -> Result<ProviderWriteStream, VfsError> {
        cancelled(&cancellation)?;
        if !options.overwrite
            && self
                .inspect(&entry(destination.clone()), CancellationToken::new())
                .await
                .is_ok()
        {
            return Err(VfsError::AlreadyExists {
                location: destination.uri.clone(),
            });
        }
        let parsed = Parsed::parse(destination)?;
        let parameters = self.resolve(&parsed).await?;
        let url = self.url_for(&parsed, &parameters)?;
        // A cached Digest challenge must exist before the streaming body
        // starts: a mid-stream 401 cannot be safely replayed against a
        // one-shot reader, so Digest connections are primed with a cheap
        // `HEAD` request first (documented limitation, see the crate-level
        // module doc).
        if parameters.auth_scheme == WebDavAuthScheme::Digest {
            let _ = self
                .request(&parameters, Method::HEAD, url.clone(), None, None)
                .await;
        }
        let request_path = url.path().to_owned();
        let cached_digest = self
            .digest_cache
            .read()
            .await
            .get(&parameters.base_url)
            .cloned();
        let client = self.client.clone();
        let (writer, reader) = tokio::io::duplex(65536);
        tokio::spawn(async move {
            let body = reqwest::Body::wrap_stream(ReaderStream::new(reader));
            let mut builder = client.request(Method::PUT, url);
            match parameters.auth_scheme {
                WebDavAuthScheme::Basic => {
                    builder = builder.basic_auth(&parameters.username, Some(&parameters.password));
                }
                WebDavAuthScheme::Digest => {
                    if let Some(cached) = cached_digest {
                        let nc = cached.nonce_count.fetch_add(1, Ordering::SeqCst) + 1;
                        let cnonce = generate_client_nonce();
                        let header = cached.challenge.authorization(
                            &parameters.username,
                            &parameters.password,
                            "PUT",
                            &request_path,
                            nc,
                            &cnonce,
                        );
                        builder = builder.header(reqwest::header::AUTHORIZATION, header);
                    }
                }
            }
            let _ = builder.body(body).send().await;
        });
        Ok(Box::pin(writer))
    }

    async fn server_side_copy(
        &self,
        source: &EntryRef,
        temporary: &Location,
        cancellation: CancellationToken,
    ) -> Result<bool, VfsError> {
        cancelled(&cancellation)?;
        let source_parsed = Parsed::parse(&source.location)?;
        let temporary_parsed = Parsed::parse(temporary)?;
        if source_parsed.connection_id != temporary_parsed.connection_id {
            return Ok(false);
        }
        let parameters = self.resolve(&source_parsed).await?;
        let source_url = self.url_for(&source_parsed, &parameters)?;
        let temporary_url = self.url_for(&temporary_parsed, &parameters)?;
        let response = self
            .request(
                &parameters,
                copy_method(),
                source_url,
                Some(("Destination", temporary_url.as_str())),
                None,
            )
            .await?;
        if response.status().is_success() {
            Ok(true)
        } else {
            Err(map_status(response.status(), &temporary.uri))
        }
    }

    async fn commit_copy(
        &self,
        _source: &EntryRef,
        temporary: &Location,
        destination: &Location,
        options: CopyCommitOptions,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        if !options.overwrite
            && self
                .inspect(&entry(destination.clone()), CancellationToken::new())
                .await
                .is_ok()
        {
            return Err(VfsError::AlreadyExists {
                location: destination.uri.clone(),
            });
        }
        self.rename(&entry(temporary.clone()), destination, cancellation)
            .await
    }

    /// Discarding a temporary that was never created must succeed (matches
    /// the local/SFTP/FTP providers, see task 0108's identical note there).
    async fn discard_copy(
        &self,
        temporary: &Location,
        cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        match self
            .remove(
                &entry(temporary.clone()),
                RemoveOptions::default(),
                cancellation,
            )
            .await
        {
            Ok(()) | Err(VfsError::NotFound { .. }) => Ok(()),
            Err(other) => Err(other),
        }
    }

    async fn same_filesystem(
        &self,
        source: &EntryRef,
        destination_directory: &Location,
        cancellation: CancellationToken,
    ) -> Result<bool, VfsError> {
        cancelled(&cancellation)?;
        let a = Parsed::parse(&source.location)?;
        let b = Parsed::parse(destination_directory)?;
        Ok(a.connection_id == b.connection_id)
    }

    async fn watch(
        &self,
        _location: &Location,
        _cancellation: CancellationToken,
    ) -> Result<ProviderChangeStream, VfsError> {
        Err(VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::WATCH,
        })
    }
}
