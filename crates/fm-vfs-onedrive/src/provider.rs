//! [`OneDriveFileSystemProvider`]: the `FileSystemProvider` implementation
//! (task 0110).
//!
//! ## Capability choices
//!
//! `LIST`, `READ`, `WRITE`, `CREATE_DIRECTORY`, `RENAME`, `MOVE`, `WATCH` and
//! `RANDOM_ACCESS` (ranged downloads, see [`Self::read_range`]) are
//! implemented for real. `TRASH` is advertised, but **not** `DELETE`:
//! Microsoft Graph's `DELETE` always moves an item to the recycle bin for
//! both personal and business accounts - there is no API for a genuinely
//! permanent single-item delete - so [`Self::remove`] honestly refuses a
//! caller that explicitly asked for one (`RemoveOptions.use_trash == false`)
//! rather than silently downgrading to a softer guarantee than requested
//! (task 0110: "advertise TRASH honestly and never imply permanent delete
//! if unavailable"). `SERVER_SIDE_COPY` is left unset: a robust same-drive
//! async-copy implementation would need to poll an unauthenticated monitor
//! URL with its own cancellation/timeout handling, which this slice does
//! not implement; the shared streaming fallback (open_read + a temporary,
//! then [`Self::commit_copy`]) is used unconditionally instead, exactly as
//! documented as acceptable in the task's own implementation notes.
//! `SET_TIMESTAMPS`, `SET_PERMISSIONS` and `CHECKSUM` are left unset -
//! nothing in this task's acceptance criteria needs them, and advertising a
//! capability this provider has not actually exercised would be a worse
//! failure mode than honestly under-advertising (the same reasoning
//! `fm-vfs-sftp` documents for its own analogous gaps).

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use fm_domain::{EntryId, EntryMetadata, EntrySummary, Location, ProviderId};
use fm_vfs::{
    ChangeTracking, CopyCommitOptions, DirectoryPage, EntryRef, FileSystemProvider, ListOptions,
    ProviderCapabilities, ProviderChangeStream, ProviderReadStream, ProviderWriteStream,
    RemoveOptions, TransferCapabilities, TransferEndpoint, VfsError, WriteOptions,
};
use reqwest::header::AUTHORIZATION;
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::graph::{
    ChildrenPage, DriveItem, GraphConfig, ONEDRIVE_PROVIDER, Parsed, RetryClass,
    bearer_header_value, build_url, entry_id_for, invalid_location, map_join_error, map_status,
    same_origin_family, send_with_retry, to_entry_summary,
};
use crate::resolver::OneDriveConnectionResolver;
use crate::{delta, upload};

/// Microsoft Graph (`/me/drive`) `FileSystemProvider` (task 0110).
pub struct OneDriveFileSystemProvider {
    resolver: Arc<dyn OneDriveConnectionResolver>,
    http: reqwest::Client,
    config: GraphConfig,
}

impl OneDriveFileSystemProvider {
    /// Creates a provider against the production Microsoft Graph endpoint
    /// ([`GraphConfig::default`]).
    #[must_use]
    pub fn new(resolver: Arc<dyn OneDriveConnectionResolver>) -> Self {
        Self::with_config(resolver, GraphConfig::default())
    }

    /// Creates a provider with an explicit [`GraphConfig`] - the seam tests
    /// and fixtures use to point at an in-process loopback endpoint with
    /// short retry/poll timings instead of the real Microsoft Graph.
    #[must_use]
    pub fn with_config(resolver: Arc<dyn OneDriveConnectionResolver>, config: GraphConfig) -> Self {
        Self {
            resolver,
            // Redirects are never followed automatically. Every request
            // this provider issues to a preauthenticated transfer URL
            // (download/upload) is built and sent by this crate itself once
            // `@microsoft.graph.downloadUrl`/`uploadUrl` has been resolved
            // from an authenticated Graph response - never by trusting an
            // implicit redirect-following policy to correctly strip the
            // `Authorization` header on a cross-host hop (task 0110's
            // explicit bearer-exfiltration concern).
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("a minimal reqwest client configuration always builds"),
            config,
        }
    }

    async fn token_for(
        &self,
        connection_id: &str,
    ) -> Result<crate::resolver::OneDriveAccessToken, VfsError> {
        self.resolver.resolve(connection_id).await
    }

    /// Fetches and JSON-decodes one authenticated Graph resource.
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: Url,
        connection_id: &str,
        location_text: &str,
        cancellation: &CancellationToken,
    ) -> Result<T, VfsError> {
        let token = self.token_for(connection_id).await?;
        let response = send_with_retry(
            || {
                self.http
                    .get(url.clone())
                    .header(AUTHORIZATION, bearer_header_value(&token))
            },
            RetryClass::Idempotent,
            &self.config.retry,
            cancellation,
        )
        .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(map_status(status, location_text));
        }
        response.json::<T>().await.map_err(|_| VfsError::Io {
            message: "Microsoft Graph returned a response this provider could not parse".to_owned(),
        })
    }

    /// Resolves one item's metadata directly from its path address, used by
    /// both [`Self::inspect`] and [`Self::rename`] (to learn a real Graph
    /// item id - task 0110: mutation APIs that require one resolve it fresh
    /// rather than ever caching it in a [`Location`] or [`EntryId`]).
    async fn fetch_item(
        &self,
        location: &Location,
        cancellation: &CancellationToken,
    ) -> Result<DriveItem, VfsError> {
        let parsed = Parsed::parse(location)?;
        let url = build_url(&self.config, &parsed.metadata_relative_path())?;
        self.get_json(url, &parsed.connection_id, &location.uri, cancellation)
            .await
    }
}

#[async_trait]
impl FileSystemProvider for OneDriveFileSystemProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new(ONEDRIVE_PROVIDER)
    }

    fn schemes(&self) -> &'static [&'static str] {
        &["onedrive"]
    }

    fn validate_location(&self, location: &Location) -> Result<(), VfsError> {
        fm_vfs::validate_connection_location(location, ONEDRIVE_PROVIDER, self.schemes(), true)?;
        crate::graph::Parsed::parse(location).map(|_| ())
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::LIST
            | ProviderCapabilities::READ
            | ProviderCapabilities::WRITE
            | ProviderCapabilities::CREATE_DIRECTORY
            | ProviderCapabilities::RENAME
            | ProviderCapabilities::MOVE
            | ProviderCapabilities::TRASH
            | ProviderCapabilities::WATCH
            | ProviderCapabilities::RANDOM_ACCESS
    }

    /// Task 0108. The endpoint identifies the concrete connection - two
    /// different saved OneDrive connections must never compare equal, so a
    /// same-backend fast path is never chosen across them (task 0110:
    /// "multiple connection IDs remain distinct").
    ///
    /// `resumable_upload`/`resumable_download` are `true` because both are
    /// genuinely implemented ([`Self::open_write_sized`]'s upload session;
    /// [`Self::read_range`]'s ranged download). `server_side_copy` is
    /// `false` (see this module's doc comment); `server_side_move` is
    /// `true` (a real same-drive `PATCH` rename/move, see [`Self::rename`]).
    /// `random_write` stays `false` - nothing in this slice implements an
    /// offset-write.
    fn transfer_capabilities(&self, location: &Location) -> Result<TransferCapabilities, VfsError> {
        let parsed = Parsed::parse(location)?;
        Ok(TransferCapabilities {
            endpoint: TransferEndpoint::new(format!(
                "{ONEDRIVE_PROVIDER}:{}",
                parsed.connection_id
            )),
            server_side_copy: false,
            server_side_move: true,
            resumable_upload: true,
            resumable_download: true,
            random_read: true,
            random_write: false,
        })
    }

    /// Microsoft Graph has no OS-level watch mechanism; change tracking is
    /// backed by its delta API instead (task 0109/0110).
    fn change_tracking(&self) -> ChangeTracking {
        ChangeTracking::DeltaApi
    }

    async fn list(
        &self,
        location: &Location,
        options: ListOptions,
        cancellation: CancellationToken,
    ) -> Result<DirectoryPage, VfsError> {
        cancelled(&cancellation)?;
        if options.page_size == 0 {
            return Err(invalid_location(location));
        }
        let parsed = Parsed::parse(location)?;

        let url = if let Some(continuation) = &options.continuation_token {
            // Task 0110: an opaque continuation token is always called
            // verbatim, never reconstructed - and never trusted with the
            // bearer token unless it demonstrably still targets the
            // configured Graph endpoint.
            if !same_origin_family(continuation, &self.config.base_url) {
                return Err(VfsError::Io {
                    message: "Microsoft Graph continuation link failed a same-origin safety check"
                        .to_owned(),
                });
            }
            Url::parse(continuation).map_err(|_| VfsError::Io {
                message: "invalid Microsoft Graph continuation link".to_owned(),
            })?
        } else {
            build_url(
                &self.config,
                &format!(
                    "{}?$top={}",
                    parsed.children_relative_path(),
                    options.page_size
                ),
            )?
        };

        let page: ChildrenPage = self
            .get_json(url, &parsed.connection_id, &location.uri, &cancellation)
            .await?;
        cancelled(&cancellation)?;

        let mut entries = Vec::with_capacity(page.value.len());
        for item in page.value {
            let Some(name) = item.name.clone() else {
                continue;
            };
            if item.deleted.is_some() {
                continue;
            }
            let child_location = location
                .join(&name)
                .map_err(|error| map_join_error(error, location))?;
            entries.push(to_entry_summary(child_location, name, &item));
        }
        let has_more = page.next_link.is_some();
        Ok(DirectoryPage {
            entries,
            total_known_entries: None,
            has_more,
            continuation_token: page.next_link,
        })
    }

    async fn metadata(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntryMetadata, VfsError> {
        cancelled(&cancellation)?;
        Ok(EntryMetadata {
            entry_id: entry.id,
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
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<EntrySummary, VfsError> {
        cancelled(&cancellation)?;
        let parsed = Parsed::parse(&entry.location)?;
        let item = self.fetch_item(&entry.location, &cancellation).await?;
        let name = if parsed.is_root() {
            String::new()
        } else {
            entry
                .location
                .name()
                .map_err(|error| map_join_error(error, &entry.location))?
        };
        Ok(to_entry_summary(entry.location.clone(), name, &item))
    }

    async fn file_size(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<u64, VfsError> {
        let summary = self.inspect(entry, cancellation).await?;
        summary.size.ok_or_else(|| VfsError::IsADirectory {
            location: entry.location.uri.clone(),
        })
    }

    async fn create_directory(
        &self,
        location: &Location,
        name: &str,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        cancelled(&cancellation)?;
        let parsed = Parsed::parse(location)?;
        let child_location = location
            .join(name)
            .map_err(|error| map_join_error(error, location))?;
        let token = self.token_for(&parsed.connection_id).await?;
        let url = build_url(&self.config, &parsed.children_relative_path())?;
        let body = serde_json::json!({
            "name": name,
            "folder": {},
            "@microsoft.graph.conflictBehavior": "fail",
        });
        let response = send_with_retry(
            || {
                self.http
                    .post(url.clone())
                    .header(AUTHORIZATION, bearer_header_value(&token))
                    .json(&body)
            },
            // A `POST .../children` is not resent after a bare transport
            // failure: it creates a new resource rather than replacing
            // existing state, so whether a prior attempt secretly
            // succeeded is unknown. `conflictBehavior: "fail"` at least
            // ensures a genuine retry attempt reports `AlreadyExists`
            // rather than silently creating a duplicate, but that is a
            // safety net for the 429/503 case (which *is* retried - the
            // server explicitly said the request was not processed), not a
            // reason to also retry a bare transport failure.
            RetryClass::NonIdempotent,
            &self.config.retry,
            &cancellation,
        )
        .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(map_status(status, &child_location.uri));
        }
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
        cancelled(&cancellation)?;
        let from = Parsed::parse(&source.location)?;
        let to = Parsed::parse(destination)?;
        if from.connection_id != to.connection_id {
            return Err(invalid_location(destination));
        }
        let token = self.token_for(&from.connection_id).await?;

        // Resolve the source's real Graph item id: mutation APIs need it,
        // but it is never cached in the `Location`/`EntryId` (task 0110).
        let source_item = self.fetch_item(&source.location, &cancellation).await?;

        // Resolve the destination *parent's* real Graph item id (task
        // 0110: "rename/move via PATCH using destination parent real ID").
        let destination_parent = destination
            .parent()
            .map_err(|error| map_join_error(error, destination))?
            .ok_or_else(|| invalid_location(destination))?;
        let destination_parent_item = self.fetch_item(&destination_parent, &cancellation).await?;
        let new_name = destination
            .name()
            .map_err(|error| map_join_error(error, destination))?;

        let url = build_url(
            &self.config,
            &format!(
                "me/drive/items/{}",
                crate::graph::percent_encode_component(&source_item.id)
            ),
        )?;
        let body = serde_json::json!({
            "name": new_name,
            "parentReference": { "id": destination_parent_item.id },
        });
        let response = send_with_retry(
            // The PATCH body states the fully desired end state (absolute
            // name and parent, not a relative delta), so resending it after
            // a bare transport failure converges to the same result either
            // way - safe to treat as idempotent.
            || {
                self.http
                    .patch(url.clone())
                    .header(AUTHORIZATION, bearer_header_value(&token))
                    .json(&body)
            },
            RetryClass::Idempotent,
            &self.config.retry,
            &cancellation,
        )
        .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(map_status(status, &destination.uri));
        }
        Ok(EntryRef {
            id: entry_id_for(destination),
            location: destination.clone(),
        })
    }

    /// Microsoft Graph's `DELETE` recycles a folder's entire subtree in one
    /// call - there is no separate recursive flag to honour, unlike a plain
    /// filesystem.
    async fn remove(
        &self,
        entry: &EntryRef,
        options: RemoveOptions,
        cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        cancelled(&cancellation)?;
        if !options.use_trash {
            return Err(VfsError::UnsupportedCapability {
                capability: ProviderCapabilities::DELETE,
            });
        }
        let parsed = Parsed::parse(&entry.location)?;
        let token = self.token_for(&parsed.connection_id).await?;
        let item = match self.fetch_item(&entry.location, &cancellation).await {
            Ok(item) => item,
            Err(VfsError::NotFound { .. }) => return Ok(()),
            Err(other) => return Err(other),
        };
        let url = build_url(
            &self.config,
            &format!(
                "me/drive/items/{}",
                crate::graph::percent_encode_component(&item.id)
            ),
        )?;
        let response = send_with_retry(
            || {
                self.http
                    .delete(url.clone())
                    .header(AUTHORIZATION, bearer_header_value(&token))
            },
            RetryClass::Idempotent,
            &self.config.retry,
            &cancellation,
        )
        .await?;
        let status = response.status();
        if status.is_success() || status == reqwest::StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(map_status(status, &entry.location.uri))
        }
    }

    async fn open_read(
        &self,
        entry: &EntryRef,
        cancellation: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError> {
        self.read_range(entry, 0, None, cancellation).await
    }

    async fn read_range(
        &self,
        entry: &EntryRef,
        offset: u64,
        length: Option<u64>,
        cancellation: CancellationToken,
    ) -> Result<ProviderReadStream, VfsError> {
        cancelled(&cancellation)?;
        let parsed = Parsed::parse(&entry.location)?;
        if parsed.is_root() {
            return Err(VfsError::IsADirectory {
                location: entry.location.uri.clone(),
            });
        }
        // One authenticated request resolves the preauthenticated download
        // URL; the transfer itself is a second, wholly separate,
        // **unauthenticated** request (task 0110: never forward the bearer
        // to a preauthenticated URL).
        let item = self.fetch_item(&entry.location, &cancellation).await?;
        let Some(download_url) = item.download_url else {
            return Err(VfsError::IsADirectory {
                location: entry.location.uri.clone(),
            });
        };

        let range_header = match length {
            Some(length) if length > 0 => Some(format!("bytes={offset}-{}", offset + length - 1)),
            Some(_) => Some(format!("bytes={offset}-{offset}")),
            None if offset > 0 => Some(format!("bytes={offset}-")),
            None => None,
        };

        let http = self.http.clone();
        let response = send_with_retry(
            move || {
                let mut request = http.get(&download_url);
                if let Some(range) = &range_header {
                    request = request.header(reqwest::header::RANGE, range.clone());
                }
                request
            },
            RetryClass::Idempotent,
            &self.config.retry,
            &cancellation,
        )
        .await?;
        let status = response.status();
        if status != reqwest::StatusCode::OK && status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(map_status(status, &entry.location.uri));
        }
        use futures::TryStreamExt;
        let stream = response.bytes_stream().map_err(std::io::Error::other);
        Ok(Box::pin(StreamReader::new(stream)))
    }

    async fn open_write(
        &self,
        destination: &Location,
        options: WriteOptions,
        cancellation: CancellationToken,
    ) -> Result<ProviderWriteStream, VfsError> {
        cancelled(&cancellation)?;
        self.reject_existing_unless_overwrite(destination, options, &cancellation)
            .await?;
        Ok(upload::open_write(
            self.http.clone(),
            self.config.clone(),
            Arc::clone(&self.resolver),
            destination.clone(),
            cancellation,
        ))
    }

    async fn open_write_sized(
        &self,
        destination: &Location,
        options: WriteOptions,
        expected_size: u64,
        cancellation: CancellationToken,
    ) -> Result<ProviderWriteStream, VfsError> {
        cancelled(&cancellation)?;
        self.reject_existing_unless_overwrite(destination, options, &cancellation)
            .await?;
        Ok(upload::open_write_sized(
            self.http.clone(),
            self.config.clone(),
            Arc::clone(&self.resolver),
            destination.clone(),
            expected_size,
            cancellation,
        ))
    }

    /// Attempts a real, provider-native `commit_copy`/`discard_copy`-backed
    /// publish, but never a native same-drive `server_side_copy` (this
    /// slice's documented, honest gap - see this module's doc comment).
    async fn server_side_copy(
        &self,
        _source: &EntryRef,
        _temporary: &Location,
        cancellation: CancellationToken,
    ) -> Result<bool, VfsError> {
        cancelled(&cancellation)?;
        Ok(false)
    }

    async fn commit_copy(
        &self,
        _source: &EntryRef,
        temporary: &Location,
        destination: &Location,
        options: CopyCommitOptions,
        cancellation: CancellationToken,
    ) -> Result<EntryRef, VfsError> {
        cancelled(&cancellation)?;
        if self
            .inspect(&entry_ref(destination.clone()), cancellation.clone())
            .await
            .is_ok()
        {
            if !options.overwrite {
                return Err(VfsError::AlreadyExists {
                    location: destination.uri.clone(),
                });
            }
            // Graph's rename/move `PATCH` is not guaranteed to silently
            // replace an existing item sharing the destination's name (this
            // slice implements no conditional/atomic overwrite-via-rename
            // primitive), so the existing destination is recycled first.
            // This narrows, but does not remove, the window between
            // "destination gone" and "temporary renamed into place" - a
            // documented limitation shared with every provider in this
            // workspace lacking a true atomic replace primitive.
            self.remove(
                &entry_ref(destination.clone()),
                RemoveOptions {
                    recursive: true,
                    use_trash: true,
                },
                cancellation.clone(),
            )
            .await?;
        }
        self.rename(&entry_ref(temporary.clone()), destination, cancellation)
            .await
    }

    /// Discarding a temporary that was never created must succeed: the
    /// operation engine calls this on every cancellation and failure path,
    /// matching every other provider in this workspace.
    async fn discard_copy(
        &self,
        temporary: &Location,
        cancellation: CancellationToken,
    ) -> Result<(), VfsError> {
        match self
            .remove(
                &entry_ref(temporary.clone()),
                RemoveOptions {
                    recursive: true,
                    use_trash: true,
                },
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
        let from = Parsed::parse(&source.location)?;
        let to = Parsed::parse(destination_directory)?;
        Ok(from.connection_id == to.connection_id)
    }

    async fn watch(
        &self,
        location: &Location,
        cancellation: CancellationToken,
    ) -> Result<ProviderChangeStream, VfsError> {
        delta::watch(
            self.http.clone(),
            self.config.clone(),
            Arc::clone(&self.resolver),
            location.clone(),
            cancellation,
        )
    }
}

impl OneDriveFileSystemProvider {
    async fn reject_existing_unless_overwrite(
        &self,
        destination: &Location,
        options: WriteOptions,
        cancellation: &CancellationToken,
    ) -> Result<(), VfsError> {
        if !options.overwrite
            && self
                .inspect(&entry_ref(destination.clone()), cancellation.clone())
                .await
                .is_ok()
        {
            return Err(VfsError::AlreadyExists {
                location: destination.uri.clone(),
            });
        }
        Ok(())
    }
}

fn entry_ref(location: Location) -> EntryRef {
    EntryRef {
        id: EntryId::new(),
        location,
    }
}

fn cancelled(cancellation: &CancellationToken) -> Result<(), VfsError> {
    if cancellation.is_cancelled() {
        Err(VfsError::Cancelled)
    } else {
        Ok(())
    }
}
