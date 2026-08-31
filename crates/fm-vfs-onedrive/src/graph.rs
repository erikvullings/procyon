//! Low-level Microsoft Graph HTTP plumbing shared by `provider.rs`,
//! `upload.rs` and `delta.rs`: `onedrive://` location parsing, Graph URL
//! building, the `DriveItem` wire model, retry/backoff, and status-code
//! error mapping. Concentrating this here means every higher-level
//! operation only has to describe *which* Graph call to make, never how to
//! make one safely (never leak a token or a preauthenticated URL, always
//! retry throttling the same way, always map statuses the same way).

use std::time::Duration;

use chrono::{DateTime, Utc};
use fm_domain::{EntryId, EntryKind, EntrySummary, Location};
use fm_vfs::VfsError;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::resolver::OneDriveAccessToken;

/// The [`fm_domain::ProviderId`]/URI scheme every `onedrive://` location uses.
pub(crate) const ONEDRIVE_PROVIDER: &str = "onedrive";

/// Default production Microsoft Graph base URL (task 0110).
pub const PRODUCTION_GRAPH_BASE_URL: &str = "https://graph.microsoft.com/v1.0";

/// Conservative production default for how often
/// [`fm_vfs::FileSystemProvider::watch`] (as implemented by
/// [`crate::OneDriveFileSystemProvider`]) polls its retained delta link when
/// idle (task 0110's delta-based change tracking). Delta queries are cheap
/// relative to a full re-list, but still should not be hammered constantly.
pub const DEFAULT_DELTA_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Conservative production default for how many changed items one delta
/// poll requests per page (`$top`) before following `@odata.nextLink`.
pub const DEFAULT_DELTA_PAGE_SIZE: usize = 200;

/// Configures which Graph endpoint a provider talks to and how it retries.
///
/// Production uses [`GraphConfig::default`] ([`PRODUCTION_GRAPH_BASE_URL`],
/// conservative retry timings). Tests/fixtures build one with
/// [`GraphConfig::new`] pointed at an in-process loopback server, with
/// [`RetryPolicy`]'s durations and `delta_poll_interval` shrunk to keep the
/// suite fast while still exercising the real retry/backoff and delta-poll
/// logic deterministically; [`GraphConfig::with_delta_page_size`] additionally
/// lets a test force multi-page delta traversal without seeding hundreds of
/// changes.
#[derive(Debug, Clone)]
pub struct GraphConfig {
    /// Base URL every request is issued relative to, e.g.
    /// `https://graph.microsoft.com/v1.0`. Never carries a trailing slash.
    pub base_url: Url,
    /// Retry/backoff policy for throttled or transiently failed requests.
    pub retry: RetryPolicy,
    /// Minimum time between successive delta polls in
    /// [`fm_vfs::FileSystemProvider::watch`].
    pub delta_poll_interval: Duration,
    /// `$top` requested per delta page before following `@odata.nextLink`.
    pub delta_page_size: usize,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            base_url: Url::parse(PRODUCTION_GRAPH_BASE_URL).expect("static URL always parses"),
            retry: RetryPolicy::default(),
            delta_poll_interval: DEFAULT_DELTA_POLL_INTERVAL,
            delta_page_size: DEFAULT_DELTA_PAGE_SIZE,
        }
    }
}

impl GraphConfig {
    /// Builds a config pointed at an arbitrary base URL (tests/fixtures),
    /// with the production [`DEFAULT_DELTA_PAGE_SIZE`]; chain
    /// [`Self::with_delta_page_size`] to override it.
    #[must_use]
    pub fn new(base_url: Url, retry: RetryPolicy, delta_poll_interval: Duration) -> Self {
        Self {
            base_url,
            retry,
            delta_poll_interval,
            delta_page_size: DEFAULT_DELTA_PAGE_SIZE,
        }
    }

    /// Overrides the delta page size (tests forcing multi-page traversal
    /// without seeding hundreds of changes).
    #[must_use]
    pub fn with_delta_page_size(mut self, delta_page_size: usize) -> Self {
        self.delta_page_size = delta_page_size;
        self
    }
}

/// Bounded exponential backoff, honouring a server's `Retry-After` when
/// present (task 0110's throttling requirement).
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Maximum number of attempts (the first send, plus retries).
    pub max_attempts: u32,
    /// Backoff before the first retry; doubled on each subsequent one.
    pub base_backoff: Duration,
    /// Backoff never grows past this, regardless of attempt count.
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    /// Conservative production timings: up to 5 attempts, starting at 500ms
    /// and capped at 30s.
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// A policy with short, deterministic timings for tests. `max_backoff`
    /// is exactly 4x `base_backoff` so a test can assert the fallback
    /// backoff sequence (`base`, `2*base`, `4*base`, capped at `4*base`)
    /// without depending on production-scale durations.
    #[must_use]
    pub fn for_tests(base_backoff: Duration) -> Self {
        Self {
            max_attempts: 4,
            base_backoff,
            max_backoff: base_backoff * 4,
        }
    }

    fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let scale = 1_u32.checked_shl(attempt).unwrap_or(u32::MAX);
        self.base_backoff
            .saturating_mul(scale)
            .min(self.max_backoff)
    }
}

/// Whether a request is safe to resend after a bare transport failure (no
/// HTTP response at all).
///
/// Throttling (429/503) is always retried regardless of this - the server
/// explicitly asked for a retry, which [Microsoft's own guidance] treats as
/// safe for any verb. This only governs the separate case of a dropped
/// connection or similar, where whether the server actually processed the
/// request is unknown.
///
/// [Microsoft's own guidance]: https://learn.microsoft.com/en-us/graph/throttling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryClass {
    /// Safe to resend after a bare transport failure: a `GET` has no side
    /// effect, and every mutating request this crate makes fully replaces
    /// state (a `PUT` of the same bytes, a `DELETE` of the same item)
    /// rather than incrementing/appending it, so repeating it is safe even
    /// if the first attempt secretly succeeded.
    Idempotent,
    /// Never resent after a bare transport failure (for example
    /// `createUploadSession`, which allocates a new server-side session) -
    /// only Graph's own explicit 429/503 instruction is honoured.
    NonIdempotent,
}

/// Sends a request, retrying 429/503 (honouring `Retry-After` if present,
/// else bounded exponential backoff) and, for [`RetryClass::Idempotent`]
/// requests, bare transport failures too. Cancellation is checked before
/// every attempt and interrupts every backoff sleep immediately.
///
/// `build` is called fresh for every attempt rather than accepting one
/// built [`reqwest::RequestBuilder`], since a request carrying a body
/// cannot be replayed after being sent once.
pub(crate) async fn send_with_retry<F>(
    build: F,
    class: RetryClass,
    policy: &RetryPolicy,
    cancellation: &CancellationToken,
) -> Result<reqwest::Response, VfsError>
where
    F: Fn() -> reqwest::RequestBuilder,
{
    let mut attempt = 0_u32;
    loop {
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled);
        }
        attempt += 1;
        let last_attempt = attempt >= policy.max_attempts;
        match build().send().await {
            Ok(response) if is_throttled(response.status()) && !last_attempt => {
                let wait =
                    retry_after(&response).unwrap_or_else(|| policy.backoff_for_attempt(attempt));
                sleep_cancellably(wait, cancellation).await?;
            }
            Ok(response) => return Ok(response),
            Err(error) if class == RetryClass::Idempotent && !last_attempt => {
                sleep_cancellably(policy.backoff_for_attempt(attempt), cancellation).await?;
                let _ = error;
            }
            Err(error) => return Err(map_transport_error(&error)),
        }
    }
}

fn is_throttled(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
}

async fn sleep_cancellably(
    duration: Duration,
    cancellation: &CancellationToken,
) -> Result<(), VfsError> {
    tokio::select! {
        () = cancellation.cancelled() => Err(VfsError::Cancelled),
        () = tokio::time::sleep(duration) => Ok(()),
    }
}

/// Parses a `Retry-After` header as a whole number of seconds.
///
/// Graph may also send an HTTP-date form; parsing that adds complexity this
/// workspace's acceptance criteria do not exercise, so an unparsable or
/// absent value safely falls back to the caller's bounded exponential
/// backoff rather than failing outright.
fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Reduces a transport-level failure to a message safe to surface across
/// process/transport boundaries.
///
/// Deliberately never formats the [`reqwest::Error`] itself: its
/// `Display`/`Debug` embed the request URL, which for a preauthenticated
/// download/upload transfer *is itself a bearer secret* (task 0110: never
/// leak a bearer token or a preauthenticated URL in an error). A coarse,
/// fixed classification is used instead, safe for every request this crate
/// makes - Graph-direct or preauthenticated alike.
fn sanitize_transport_error(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "request timed out"
    } else if error.is_connect() {
        "could not connect"
    } else if error.is_body() || error.is_decode() {
        "request or response body error"
    } else if error.is_redirect() {
        "unexpected redirect"
    } else {
        "request failed"
    }
}

fn map_transport_error(error: &reqwest::Error) -> VfsError {
    VfsError::Io {
        message: sanitize_transport_error(error).to_owned(),
    }
}

/// Maps an unsuccessful Graph HTTP status to a [`VfsError`] (task 0110's
/// error-envelope table). `location` is always the caller-facing
/// `onedrive://` location URI, never a Graph request URL or response body -
/// consistent with every other provider in this workspace, and satisfying
/// "never raw response bodies or URLs/tokens" by construction rather than
/// by care at every call site.
pub(crate) fn map_status(status: reqwest::StatusCode, location: &str) -> VfsError {
    match status {
        reqwest::StatusCode::NOT_FOUND => VfsError::NotFound {
            location: location.to_owned(),
        },
        reqwest::StatusCode::UNAUTHORIZED => VfsError::CredentialRequired,
        reqwest::StatusCode::FORBIDDEN => VfsError::PermissionDenied {
            location: location.to_owned(),
        },
        reqwest::StatusCode::CONFLICT | reqwest::StatusCode::PRECONDITION_FAILED => {
            VfsError::AlreadyExists {
                location: location.to_owned(),
            }
        }
        reqwest::StatusCode::LOCKED => VfsError::Locked {
            location: location.to_owned(),
        },
        reqwest::StatusCode::INSUFFICIENT_STORAGE => VfsError::Io {
            message: "OneDrive storage quota exceeded".to_owned(),
        },
        other => VfsError::Io {
            message: format!("Microsoft Graph request failed with status {other}"),
        },
    }
}

pub(crate) fn invalid_location(location: &Location) -> VfsError {
    VfsError::InvalidLocation {
        location: location.uri.clone(),
    }
}

/// Bearer header value for an authenticated Graph request.
pub(crate) fn bearer_header_value(token: &OneDriveAccessToken) -> String {
    format!("Bearer {}", token.as_str())
}

/// Whether `candidate` is safe to reattach `Authorization: Bearer ...` to:
/// same scheme, host and port as `base`, and its path falls under `base`'s
/// path family.
///
/// Required before ever calling a Graph-returned opaque continuation URL
/// (`@odata.nextLink`/`@odata.deltaLink`) - task 0110's explicit SSRF/token-
/// exfiltration guard. A link that failed this check is never followed at
/// all (not just sent unauthenticated), since an opaque link that does not
/// even point at the configured Graph endpoint cannot be a legitimate
/// continuation of a request that endpoint returned.
pub(crate) fn same_origin_family(candidate: &str, base: &Url) -> bool {
    let Ok(candidate_url) = Url::parse(candidate) else {
        return false;
    };
    candidate_url.scheme() == base.scheme()
        && candidate_url.host_str() == base.host_str()
        && candidate_url.port_or_known_default() == base.port_or_known_default()
        && candidate_url.path().starts_with(base.path())
}

/// A parsed `onedrive://<connection-id>/<percent-encoded-path>` location.
pub(crate) struct Parsed {
    pub(crate) connection_id: String,
    /// The path text exactly as it appears in the location's URI (already
    /// percent-encoded per `fm_domain::Location`'s convention), with no
    /// leading or trailing slash - safe to embed directly into a Graph
    /// path-addressed URL segment without re-encoding. Empty for the drive
    /// root.
    pub(crate) encoded_path: String,
}

impl Parsed {
    pub(crate) fn parse(location: &Location) -> Result<Self, VfsError> {
        if location.provider_id.as_str() != ONEDRIVE_PROVIDER {
            return Err(invalid_location(location));
        }
        let remainder = location
            .uri
            .strip_prefix("onedrive://")
            .ok_or_else(|| invalid_location(location))?;
        let (connection_id, path) = remainder
            .split_once('/')
            .ok_or_else(|| invalid_location(location))?;
        if connection_id.is_empty() {
            return Err(invalid_location(location));
        }
        Ok(Self {
            connection_id: connection_id.to_owned(),
            encoded_path: path.to_owned(),
        })
    }

    pub(crate) fn is_root(&self) -> bool {
        self.encoded_path.is_empty()
    }

    /// The Graph item-address fragment shared by every path-addressed
    /// endpoint under `/me/drive/`: `root` for the drive root, or
    /// `root:/<path>:` for a nested item.
    fn item_address(&self) -> String {
        if self.is_root() {
            "root".to_owned()
        } else {
            format!("root:/{}:", self.encoded_path)
        }
    }

    /// Relative path (no leading slash, no query string) for fetching this
    /// item's own metadata.
    pub(crate) fn metadata_relative_path(&self) -> String {
        format!("me/drive/{}", self.item_address())
    }

    /// Relative path for listing this item's children.
    pub(crate) fn children_relative_path(&self) -> String {
        format!("me/drive/{}/children", self.item_address())
    }

    /// Relative path for a simple upload/download of file content. Callers
    /// must not invoke this for the drive root (a folder); the guard lives
    /// at the call site, where an `IsADirectory` error already carries the
    /// right location context.
    pub(crate) fn content_relative_path(&self) -> String {
        format!("me/drive/{}/content", self.item_address())
    }

    /// Relative path for creating a resumable upload session.
    pub(crate) fn create_upload_session_relative_path(&self) -> String {
        format!("me/drive/{}/createUploadSession", self.item_address())
    }
}

/// Builds a Graph request URL from `config`'s base URL and a relative path
/// (and optional pre-formed query string) this crate always constructs
/// itself out of ASCII-safe, already-encoded pieces - a single fresh
/// [`Url::parse`] of the fully composed text, rather than incremental
/// mutation methods whose percent-encoding semantics for an
/// already-encoded input are easy to get subtly wrong.
pub(crate) fn build_url(
    config: &GraphConfig,
    relative_path_and_query: &str,
) -> Result<Url, VfsError> {
    let base = config.base_url.as_str().trim_end_matches('/');
    Url::parse(&format!("{base}/{relative_path_and_query}")).map_err(|_| VfsError::Io {
        message: "failed to build a Microsoft Graph request URL".to_owned(),
    })
}

/// Percent-encodes a Graph item id for safe embedding as one URL path
/// segment (`me/drive/items/<id>`). Real Graph ids are already URL-safe,
/// but this never assumes that (task 0110: "never assume Graph item IDs are
/// UUIDs" - by extension, never assume any particular shape at all).
pub(crate) fn percent_encode_component(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'!') {
            encoded.push(char::from(*byte));
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

/// Translates a [`fm_domain::LocationError`] from `Location::join`/`::name`/
/// `::parent` into the [`VfsError`] a provider caller expects, mirroring
/// `fm_vfs_sftp`'s identical helper.
pub(crate) fn map_join_error(error: fm_domain::LocationError, location: &Location) -> VfsError {
    match error {
        fm_domain::LocationError::EmptySegment => VfsError::EmptyName,
        fm_domain::LocationError::InvalidName(_) => VfsError::PathTraversalName,
        fm_domain::LocationError::NullByte => VfsError::InvalidNameCharacters,
        fm_domain::LocationError::ReservedWindowsName(_) => VfsError::ReservedName,
        _ => invalid_location(location),
    }
}

/// One Microsoft Graph `driveItem` resource, deserialized directly (task
/// 0110's DriveItem-facet mapping). Only the fields this provider actually
/// uses are modeled; unknown fields are ignored by `serde_json` by default.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DriveItem {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) size: Option<u64>,
    #[serde(default)]
    pub(crate) folder: Option<FolderFacet>,
    #[serde(default)]
    pub(crate) file: Option<FileFacet>,
    #[serde(default)]
    pub(crate) deleted: Option<DeletedFacet>,
    #[serde(rename = "lastModifiedDateTime", default)]
    pub(crate) last_modified_date_time: Option<String>,
    #[serde(rename = "createdDateTime", default)]
    pub(crate) created_date_time: Option<String>,
    #[serde(rename = "@microsoft.graph.downloadUrl", default)]
    pub(crate) download_url: Option<String>,
}

/// Presence-only facet: a `driveItem` is a folder iff Graph includes this
/// (possibly empty) object, personal and business accounts alike.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct FolderFacet {}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct FileFacet {
    #[serde(rename = "mimeType", default)]
    pub(crate) mime_type: Option<String>,
}

/// Presence of this facet means the item was deleted (delta feeds only).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DeletedFacet {
    #[serde(default)]
    #[allow(dead_code)]
    // surfaced for future finer-grained delta handling; not required by this slice's coalesced Changed signal
    pub(crate) state: Option<String>,
}

/// One page of `GET .../children`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ChildrenPage {
    #[serde(default)]
    pub(crate) value: Vec<DriveItem>,
    #[serde(rename = "@odata.nextLink", default)]
    pub(crate) next_link: Option<String>,
}

/// One page of `GET .../delta`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DeltaPage {
    #[serde(default)]
    pub(crate) value: Vec<DriveItem>,
    #[serde(rename = "@odata.nextLink", default)]
    pub(crate) next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink", default)]
    pub(crate) delta_link: Option<String>,
}

/// Response body of `POST .../createUploadSession`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct UploadSession {
    #[serde(rename = "uploadUrl")]
    pub(crate) upload_url: String,
}

pub(crate) fn entry_kind(item: &DriveItem) -> EntryKind {
    if item.folder.is_some() {
        EntryKind::Directory
    } else {
        EntryKind::File
    }
}

fn parse_graph_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

/// Derives a deterministic, stable [`EntryId`] from a location's URI text
/// (task 0110: "never assume Graph item IDs are UUIDs" - this never even
/// looks at the Graph item id). Identical to `fm_vfs_sftp`'s
/// `entry_id_for`/`fm_archive`'s equivalent: the same location always
/// yields the same id across separate listing calls, without requiring any
/// server-side id to be UUID-shaped.
pub(crate) fn entry_id_for(location: &Location) -> EntryId {
    EntryId::from(uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        location.uri.as_bytes(),
    ))
}

/// Converts one `DriveItem` (plus the already-joined child `Location` and
/// its decoded display name) into a provider-neutral [`EntrySummary`].
pub(crate) fn to_entry_summary(location: Location, name: String, item: &DriveItem) -> EntrySummary {
    let kind = entry_kind(item);
    let is_file = kind == EntryKind::File;
    EntrySummary {
        id: entry_id_for(&location),
        extension: is_file
            .then(|| {
                std::path::Path::new(&name)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(str::to_owned)
            })
            .flatten(),
        hidden: name.starts_with('.'),
        name,
        location,
        size: is_file.then_some(item.size.unwrap_or(0)),
        modified_at: item
            .last_modified_date_time
            .as_deref()
            .and_then(parse_graph_timestamp),
        created_at: item
            .created_date_time
            .as_deref()
            .and_then(parse_graph_timestamp),
        read_only: false,
        kind,
        mime_type: item.file.as_ref().and_then(|file| file.mime_type.clone()),
        icon_key: None,
        metadata_revision: 0,
        git_status: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_for(base: &str) -> GraphConfig {
        GraphConfig::new(
            Url::parse(base).expect("valid test base URL"),
            RetryPolicy::for_tests(Duration::from_millis(1)),
            Duration::from_millis(5),
        )
    }

    #[test]
    fn default_config_targets_the_production_graph_endpoint() {
        let config = GraphConfig::default();
        assert_eq!(config.base_url.as_str(), "https://graph.microsoft.com/v1.0");
    }

    #[test]
    fn parses_a_root_and_nested_onedrive_location() {
        let connection_id = "11111111-1111-4111-8111-111111111111";
        let root = Location::parse(&format!("onedrive://{connection_id}/")).unwrap();
        let parsed_root = Parsed::parse(&root).unwrap();
        assert!(parsed_root.is_root());
        assert_eq!(parsed_root.metadata_relative_path(), "me/drive/root");
        assert_eq!(
            parsed_root.children_relative_path(),
            "me/drive/root/children"
        );

        let nested = Location::parse(&format!(
            "onedrive://{connection_id}/Documents/My%20Report.pdf"
        ))
        .unwrap();
        let parsed_nested = Parsed::parse(&nested).unwrap();
        assert!(!parsed_nested.is_root());
        assert_eq!(
            parsed_nested.metadata_relative_path(),
            "me/drive/root:/Documents/My%20Report.pdf:"
        );
        assert_eq!(
            parsed_nested.content_relative_path(),
            "me/drive/root:/Documents/My%20Report.pdf:/content"
        );
        assert_eq!(
            parsed_nested.create_upload_session_relative_path(),
            "me/drive/root:/Documents/My%20Report.pdf:/createUploadSession"
        );
    }

    #[test]
    fn rejects_a_location_from_a_different_provider() {
        let location = Location::new(fm_domain::ProviderId::new("s3"), "s3://x/y");
        assert!(matches!(
            Parsed::parse(&location),
            Err(VfsError::InvalidLocation { .. })
        ));
    }

    #[test]
    fn build_url_composes_the_base_and_relative_path_without_double_encoding() {
        let config = config_for("http://127.0.0.1:9999/v1.0");
        let url = build_url(&config, "me/drive/root:/My%20Report.pdf:/content").unwrap();
        assert_eq!(
            url.as_str(),
            "http://127.0.0.1:9999/v1.0/me/drive/root:/My%20Report.pdf:/content"
        );
    }

    #[test]
    fn same_origin_family_accepts_only_matching_scheme_host_port_and_path_prefix() {
        let base = Url::parse("https://graph.microsoft.com/v1.0").unwrap();
        assert!(same_origin_family(
            "https://graph.microsoft.com/v1.0/me/drive/root/children?$skiptoken=abc",
            &base
        ));
        assert!(!same_origin_family(
            "https://evil.example.test/v1.0/me/drive/root/children",
            &base
        ));
        assert!(!same_origin_family(
            "http://graph.microsoft.com/v1.0/me/drive/root/children",
            &base
        ));
        assert!(!same_origin_family(
            "https://graph.microsoft.com/v2.0/me/drive/root/children",
            &base
        ));
        assert!(!same_origin_family("not a url", &base));
    }

    #[test]
    fn entry_id_derivation_is_deterministic_and_never_uses_the_graph_item_id() {
        let connection_id = "11111111-1111-4111-8111-111111111111";
        let location =
            Location::parse(&format!("onedrive://{connection_id}/Documents/report.pdf")).unwrap();
        let first = entry_id_for(&location);
        let second = entry_id_for(&location);
        assert_eq!(first, second);

        let other_location =
            Location::parse(&format!("onedrive://{connection_id}/Documents/other.pdf")).unwrap();
        assert_ne!(first, entry_id_for(&other_location));
    }

    #[test]
    fn maps_the_documented_status_table() {
        let location = "onedrive://11111111-1111-4111-8111-111111111111/x";
        assert!(matches!(
            map_status(reqwest::StatusCode::NOT_FOUND, location),
            VfsError::NotFound { .. }
        ));
        assert!(matches!(
            map_status(reqwest::StatusCode::UNAUTHORIZED, location),
            VfsError::CredentialRequired
        ));
        assert!(matches!(
            map_status(reqwest::StatusCode::FORBIDDEN, location),
            VfsError::PermissionDenied { .. }
        ));
        assert!(matches!(
            map_status(reqwest::StatusCode::CONFLICT, location),
            VfsError::AlreadyExists { .. }
        ));
        assert!(matches!(
            map_status(reqwest::StatusCode::PRECONDITION_FAILED, location),
            VfsError::AlreadyExists { .. }
        ));
        assert!(matches!(
            map_status(reqwest::StatusCode::LOCKED, location),
            VfsError::Locked { .. }
        ));
        assert!(matches!(
            map_status(reqwest::StatusCode::INSUFFICIENT_STORAGE, location),
            VfsError::Io { .. }
        ));
        let VfsError::Io { message } = map_status(reqwest::StatusCode::BAD_GATEWAY, location)
        else {
            panic!("expected a generic Io mapping for an undocumented status");
        };
        assert!(message.contains("502"));
        assert!(!message.contains(location));
    }
}
