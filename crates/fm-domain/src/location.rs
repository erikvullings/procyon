//! Provider-neutral location addressing (spec §5.1).
//!
//! Local filesystem locations use stable `file:` URIs while the owning
//! provider is identified as `local`. Path operations decode and re-encode
//! complete segments so callers never need to concatenate URI strings.
//!
//! `search://local/{searchId}` locations (spec §24, task 0068) address a
//! virtual, provider-owned result set rather than a native path. They parse
//! and carry a `provider_id` of `search`, but deliberately do not support
//! [`Location::to_native_path`], [`Location::join`], [`Location::name`] or
//! [`Location::parent`] (all `file:`-specific); the search provider builds
//! and interprets these URIs directly instead.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ids::ProviderId;

const LOCAL_SCHEME: &str = "file";
const SEARCH_SCHEME: &str = "search";
const SEARCH_AUTHORITY: &str = "local";
/// Schemes named by the specification but not yet backed by a provider
/// (task 0104 removed `"sftp"` from this list once it gained a real
/// provider; kept as the seam a later task, e.g. FTP/FTPS, reserves its own
/// scheme ahead of implementing it).
const RESERVED_SCHEMES: &[&str] = &[];

/// Static scheme-to-provider mapping for data providers (excludes search).
const SCHEME_MAP: [(&str, &str); 8] = [
    ("file", "local"),
    ("archive", "archive"),
    ("sftp", "sftp"),
    ("ftp", "ftp"),
    ("ftps", "ftp"),
    ("s3", "s3"),
    ("webdav", "webdav"),
    ("onedrive", "onedrive"),
];

/// A provider-neutral pointer to a location.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    /// The virtual filesystem provider that owns this location.
    pub provider_id: ProviderId,
    /// The full, provider-specific URI text.
    pub uri: String,
}

/// A typed failure while parsing or manipulating a [`Location`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LocationError {
    /// The URI has no syntactically valid scheme.
    #[error("location URI has no valid scheme")]
    InvalidUri,
    /// The URI names a known but not-yet-implemented provider.
    #[error("provider `{0}` is reserved but unsupported")]
    UnsupportedProvider(String),
    /// The URI names no registered provider.
    #[error("unknown provider `{0}`")]
    UnknownProvider(String),
    /// The explicit provider does not own the URI scheme.
    #[error("provider `{provider_id}` does not match URI scheme `{scheme}`")]
    MismatchedProvider {
        /// Explicit provider identifier.
        provider_id: String,
        /// Scheme parsed from the URI.
        scheme: String,
    },
    /// A path segment contains a null byte.
    #[error("location contains a null byte")]
    NullByte,
    /// A path contains an empty segment.
    #[error("location contains an empty path segment")]
    EmptySegment,
    /// Percent-encoding is malformed.
    #[error("location contains invalid percent-encoding")]
    InvalidPercentEncoding,
    /// A decoded segment is not valid UTF-8 where text is required.
    #[error("location name is not valid UTF-8")]
    InvalidUnicode,
    /// A segment is a reserved Windows device name.
    #[error("`{0}` is a reserved Windows device name")]
    ReservedWindowsName(String),
    /// A child name contains separators or traversal components.
    #[error("`{0}` is not a single safe path name")]
    InvalidName(String),
    /// The location is outside the configured root.
    #[error("location escapes the configured root")]
    EscapesRoot,
    /// The root belongs to a different provider or filesystem authority.
    #[error("location and root are from different providers or authorities")]
    RootMismatch,
    /// A URI cannot be represented as a native path on this platform.
    #[error("location cannot be represented as a native path on this platform")]
    UnsupportedNativePath,
    /// Only absolute native paths can become stable locations.
    #[error("native path must be absolute")]
    RelativeNativePath,
}

impl Location {
    /// Creates a location without validation.
    ///
    /// Prefer [`Self::parse`] or [`Self::try_new`] at trust boundaries. This
    /// constructor remains available for deserialization adapters and existing
    /// domain fixtures whose validation happens at a higher boundary.
    pub fn new(provider_id: ProviderId, uri: impl Into<String>) -> Self {
        Self {
            provider_id,
            uri: uri.into(),
        }
    }

    /// Parses and validates a provider-neutral URI.
    pub fn parse(uri: &str) -> Result<Self, LocationError> {
        let scheme = parse_scheme(uri)?;
        if scheme == SEARCH_SCHEME {
            validate_search_uri(uri)?;
            return Ok(Self::new(ProviderId::new(SEARCH_SCHEME), uri));
        }
        let provider_id = provider_for_scheme(scheme)?;
        if scheme == "archive" {
            ParsedArchiveUri::parse(uri)?;
            return Ok(Self::new(ProviderId::new(provider_id), uri));
        }
        if scheme == "sftp" {
            let parsed = ParsedSftpUri::parse(uri)?;
            parsed.validate_segments()?;
            return Ok(Self::new(ProviderId::new(provider_id), uri));
        }
        if matches!(scheme, "ftp" | "ftps") {
            let parsed = ParsedFtpUri::parse(uri)?;
            parsed.validate_segments()?;
            return Ok(Self::new(ProviderId::new(provider_id), uri));
        }
        if scheme == "s3" {
            let parsed = ParsedS3Uri::parse(uri)?;
            parsed.validate_segments()?;
            return Ok(Self::new(ProviderId::new(provider_id), uri));
        }
        if scheme == "webdav" {
            let parsed = ParsedWebDavUri::parse(uri)?;
            parsed.validate_segments()?;
            return Ok(Self::new(ProviderId::new(provider_id), uri));
        }
        if scheme == "onedrive" {
            let parsed = ParsedOneDriveUri::parse(uri)?;
            parsed.validate_segments()?;
            return Ok(Self::new(ProviderId::new(provider_id), uri));
        }
        // file scheme (fallthrough)
        let parsed = ParsedFileUri::parse(uri)?;
        parsed.validate_segments()?;
        // A directory's trailing slash is dropped so one directory has one URI.
        let canonical = if parsed.segments.is_empty() {
            uri
        } else {
            uri.strip_suffix('/').unwrap_or(uri)
        };
        Ok(Self::new(ProviderId::new(provider_id), canonical))
    }

    /// Creates a validated location and verifies its provider matches the URI.
    pub fn try_new(provider_id: ProviderId, uri: impl Into<String>) -> Result<Self, LocationError> {
        let uri = uri.into();
        let scheme = parse_scheme(&uri)?;
        let expected_provider = provider_for_scheme(scheme)?;
        if provider_id.as_str() != expected_provider {
            return Err(LocationError::MismatchedProvider {
                provider_id: provider_id.as_str().to_owned(),
                scheme: scheme.to_owned(),
            });
        }
        Self::parse(&uri)
    }

    /// Converts an absolute native local path into a stable `file:` location.
    pub fn from_native_path(path: &Path) -> Result<Self, LocationError> {
        if !path.is_absolute() {
            return Err(LocationError::RelativeNativePath);
        }
        native_path_to_location(path)
    }

    /// Converts a validated local location to a native path without I/O.
    pub fn to_native_path(&self) -> Result<PathBuf, LocationError> {
        self.ensure_local()?;
        location_to_native_path(&ParsedFileUri::parse(&self.uri)?)
    }

    /// Lexically resolves `.` and `..` and rejects a result outside `root`.
    pub fn normalize_within(&self, root: &Self) -> Result<Self, LocationError> {
        self.ensure_local()?;
        root.ensure_local()?;
        let parsed = ParsedFileUri::parse(&self.uri)?;
        let parsed_root = ParsedFileUri::parse(&root.uri)?;
        if parsed.authority != parsed_root.authority {
            return Err(LocationError::RootMismatch);
        }

        let normalized = normalize_segments(&parsed.segments)?;
        let normalized_root = normalize_segments(&parsed_root.segments)?;
        if !normalized.starts_with(&normalized_root) {
            return Err(LocationError::EscapesRoot);
        }
        ParsedFileUri {
            authority: parsed.authority,
            segments: normalized,
        }
        .into_location()
    }

    /// Returns the parent without string concatenation, or `None` for a root.
    pub fn parent(&self) -> Result<Option<Self>, LocationError> {
        if self.provider_id.as_str() == "archive" {
            return ParsedArchiveUri::parse(&self.uri)?.parent();
        }
        if self.provider_id.as_str() == "sftp" {
            return ParsedSftpUri::parse(&self.uri)?.parent();
        }
        if self.provider_id.as_str() == "ftp" {
            return ParsedFtpUri::parse(&self.uri)?.parent();
        }
        if self.provider_id.as_str() == "s3" {
            return ParsedS3Uri::parse(&self.uri)?.parent();
        }
        if self.provider_id.as_str() == "webdav" {
            return ParsedWebDavUri::parse(&self.uri)?.parent();
        }
        if self.provider_id.as_str() == "onedrive" {
            return ParsedOneDriveUri::parse(&self.uri)?.parent();
        }
        self.ensure_local()?;
        let mut parsed = ParsedFileUri::parse(&self.uri)?;
        parsed.validate_segments()?;
        if parsed.segments.pop().is_none() {
            return Ok(None);
        }
        parsed.into_location().map(Some)
    }

    /// Appends one validated native name as a complete path segment.
    pub fn join(&self, name: &str) -> Result<Self, LocationError> {
        if self.provider_id.as_str() == "archive" {
            validate_name(name)?;
            return ParsedArchiveUri::parse(&self.uri)?.join(name);
        }
        if self.provider_id.as_str() == "sftp" {
            validate_name(name)?;
            return ParsedSftpUri::parse(&self.uri)?.join(name);
        }
        if self.provider_id.as_str() == "ftp" {
            validate_name(name)?;
            return ParsedFtpUri::parse(&self.uri)?.join(name);
        }
        if self.provider_id.as_str() == "s3" {
            validate_name(name)?;
            return ParsedS3Uri::parse(&self.uri)?.join(name);
        }
        if self.provider_id.as_str() == "webdav" {
            validate_name(name)?;
            return ParsedWebDavUri::parse(&self.uri)?.join(name);
        }
        if self.provider_id.as_str() == "onedrive" {
            validate_name(name)?;
            return ParsedOneDriveUri::parse(&self.uri)?.join(name);
        }
        self.ensure_local()?;
        validate_name(name)?;
        let mut parsed = ParsedFileUri::parse(&self.uri)?;
        parsed.validate_segments()?;
        parsed.segments.push(name.as_bytes().to_vec());
        parsed.into_location()
    }

    /// Returns the decoded final path segment.
    pub fn name(&self) -> Result<String, LocationError> {
        if self.provider_id.as_str() == "archive" {
            return ParsedArchiveUri::parse(&self.uri)?.name();
        }
        if self.provider_id.as_str() == "sftp" {
            return ParsedSftpUri::parse(&self.uri)?.name();
        }
        if self.provider_id.as_str() == "ftp" {
            return ParsedFtpUri::parse(&self.uri)?.name();
        }
        if self.provider_id.as_str() == "s3" {
            return ParsedS3Uri::parse(&self.uri)?.name();
        }
        if self.provider_id.as_str() == "webdav" {
            return ParsedWebDavUri::parse(&self.uri)?.name();
        }
        if self.provider_id.as_str() == "onedrive" {
            return ParsedOneDriveUri::parse(&self.uri)?.name();
        }
        self.ensure_local()?;
        let parsed = ParsedFileUri::parse(&self.uri)?;
        parsed.validate_segments()?;
        let bytes = parsed.segments.last().ok_or(LocationError::InvalidName(
            "filesystem root has no name".to_owned(),
        ))?;
        String::from_utf8(bytes.clone()).map_err(|_| LocationError::InvalidUnicode)
    }

    fn ensure_local(&self) -> Result<(), LocationError> {
        let scheme = parse_scheme(&self.uri)?;
        if self.provider_id.as_str() != "local" || scheme != LOCAL_SCHEME {
            return Err(LocationError::MismatchedProvider {
                provider_id: self.provider_id.as_str().to_owned(),
                scheme: scheme.to_owned(),
            });
        }
        Ok(())
    }
}

#[derive(Debug)]
struct ParsedFileUri {
    authority: String,
    segments: Vec<Vec<u8>>,
}

#[derive(Debug)]
struct ParsedArchiveUri {
    outer: String,
    inner_segments: Vec<Vec<u8>>,
}

/// `sftp://<connection-id>/<remote-path>` (spec §6.5).
///
/// `connection_id` is a plain, opaque string path segment here - the same
/// text form as `fm_connections::ConnectionId::to_string()` produces - not a
/// structured cross-crate type: `fm-domain` must not depend on
/// `fm-connections` (see `crates/fm-connections/src/ids.rs`'s own doc
/// comment, which anticipates exactly this). It is validated as UUID text
/// using the `uuid` crate directly (already a dependency of this crate for
/// [`crate::ids::EntryId`]/[`crate::ids::WorkspaceId`]), which catches
/// malformed ids without coupling to `ConnectionId` itself.
#[derive(Debug)]
struct ParsedSftpUri {
    connection_id: String,
    segments: Vec<Vec<u8>>,
}

impl ParsedSftpUri {
    fn parse(uri: &str) -> Result<Self, LocationError> {
        let remainder = uri
            .strip_prefix("sftp://")
            .ok_or(LocationError::InvalidUri)?;
        if remainder.contains(['?', '#']) {
            return Err(LocationError::InvalidUri);
        }
        let (connection_id, path) = remainder.split_once('/').ok_or(LocationError::InvalidUri)?;
        if connection_id.is_empty() || uuid::Uuid::parse_str(connection_id).is_err() {
            return Err(LocationError::InvalidUri);
        }
        let segments = if path.is_empty() {
            Vec::new()
        } else {
            let raw_segments: Vec<&str> = path.split('/').collect();
            if raw_segments.iter().any(|segment| segment.is_empty()) {
                return Err(LocationError::EmptySegment);
            }
            raw_segments
                .into_iter()
                .map(percent_decode)
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Self {
            connection_id: connection_id.to_owned(),
            segments,
        })
    }

    fn validate_segments(&self) -> Result<(), LocationError> {
        for segment in &self.segments {
            if segment.contains(&0) {
                return Err(LocationError::NullByte);
            }
            if segment.contains(&b'/') || segment.contains(&b'\\') {
                return Err(LocationError::InvalidName(
                    String::from_utf8_lossy(segment).into_owned(),
                ));
            }
            if let Ok(name) = std::str::from_utf8(segment) {
                reject_windows_device_name(name)?;
            }
        }
        Ok(())
    }

    fn into_location(self) -> Result<Location, LocationError> {
        self.validate_segments()?;
        let mut uri = format!("sftp://{}/", self.connection_id);
        uri.push_str(
            &self
                .segments
                .iter()
                .map(|segment| percent_encode(segment))
                .collect::<Vec<_>>()
                .join("/"),
        );
        Location::parse(&uri)
    }

    fn join(mut self, name: &str) -> Result<Location, LocationError> {
        self.segments.push(name.as_bytes().to_vec());
        self.into_location()
    }

    fn parent(mut self) -> Result<Option<Location>, LocationError> {
        if self.segments.pop().is_none() {
            return Ok(None);
        }
        self.into_location().map(Some)
    }

    fn name(&self) -> Result<String, LocationError> {
        let bytes = self
            .segments
            .last()
            .ok_or_else(|| LocationError::InvalidName("sftp root has no name".to_owned()))?;
        String::from_utf8(bytes.clone()).map_err(|_| LocationError::InvalidUnicode)
    }
}

/// `s3://<connection-id>/<key-prefix>` (task 0146), mirroring
/// [`ParsedSftpUri`] exactly: an S3 key prefix behaves as a path-like segment
/// list even though the bucket has no real directories.
#[derive(Debug)]
struct ParsedS3Uri {
    connection_id: String,
    segments: Vec<Vec<u8>>,
}

impl ParsedS3Uri {
    fn parse(uri: &str) -> Result<Self, LocationError> {
        let remainder = uri.strip_prefix("s3://").ok_or(LocationError::InvalidUri)?;
        if remainder.contains(['?', '#']) {
            return Err(LocationError::InvalidUri);
        }
        let (connection_id, path) = remainder.split_once('/').ok_or(LocationError::InvalidUri)?;
        if connection_id.is_empty() || uuid::Uuid::parse_str(connection_id).is_err() {
            return Err(LocationError::InvalidUri);
        }
        let segments = if path.is_empty() {
            Vec::new()
        } else {
            let raw_segments: Vec<&str> = path.split('/').collect();
            if raw_segments.iter().any(|segment| segment.is_empty()) {
                return Err(LocationError::EmptySegment);
            }
            raw_segments
                .into_iter()
                .map(percent_decode)
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Self {
            connection_id: connection_id.to_owned(),
            segments,
        })
    }

    fn validate_segments(&self) -> Result<(), LocationError> {
        for segment in &self.segments {
            if segment.contains(&0) {
                return Err(LocationError::NullByte);
            }
            if segment.contains(&b'/') || segment.contains(&b'\\') {
                return Err(LocationError::InvalidName(
                    String::from_utf8_lossy(segment).into_owned(),
                ));
            }
            if let Ok(name) = std::str::from_utf8(segment) {
                reject_windows_device_name(name)?;
            }
        }
        Ok(())
    }

    fn into_location(self) -> Result<Location, LocationError> {
        self.validate_segments()?;
        let mut uri = format!("s3://{}/", self.connection_id);
        uri.push_str(
            &self
                .segments
                .iter()
                .map(|segment| percent_encode(segment))
                .collect::<Vec<_>>()
                .join("/"),
        );
        Location::parse(&uri)
    }

    fn join(mut self, name: &str) -> Result<Location, LocationError> {
        self.segments.push(name.as_bytes().to_vec());
        self.into_location()
    }

    fn parent(mut self) -> Result<Option<Location>, LocationError> {
        if self.segments.pop().is_none() {
            return Ok(None);
        }
        self.into_location().map(Some)
    }

    fn name(&self) -> Result<String, LocationError> {
        let bytes = self
            .segments
            .last()
            .ok_or_else(|| LocationError::InvalidName("s3 root has no name".to_owned()))?;
        String::from_utf8(bytes.clone()).map_err(|_| LocationError::InvalidUnicode)
    }
}

/// `webdav://<connection-id>/<remote-path>` (task 0147), mirroring
/// [`ParsedSftpUri`] exactly: the connection id is an opaque UUID-text path
/// segment, never a structured `fm_connections` type (`fm-domain` must not
/// depend on `fm-connections`).
#[derive(Debug)]
struct ParsedWebDavUri {
    connection_id: String,
    segments: Vec<Vec<u8>>,
}

impl ParsedWebDavUri {
    fn parse(uri: &str) -> Result<Self, LocationError> {
        let remainder = uri
            .strip_prefix("webdav://")
            .ok_or(LocationError::InvalidUri)?;
        if remainder.contains(['?', '#']) {
            return Err(LocationError::InvalidUri);
        }
        let (connection_id, path) = remainder.split_once('/').ok_or(LocationError::InvalidUri)?;
        if connection_id.is_empty() || uuid::Uuid::parse_str(connection_id).is_err() {
            return Err(LocationError::InvalidUri);
        }
        let segments = if path.is_empty() {
            Vec::new()
        } else {
            let raw_segments: Vec<&str> = path.split('/').collect();
            if raw_segments.iter().any(|segment| segment.is_empty()) {
                return Err(LocationError::EmptySegment);
            }
            raw_segments
                .into_iter()
                .map(percent_decode)
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Self {
            connection_id: connection_id.to_owned(),
            segments,
        })
    }

    fn validate_segments(&self) -> Result<(), LocationError> {
        for segment in &self.segments {
            if segment.contains(&0) {
                return Err(LocationError::NullByte);
            }
            if segment.contains(&b'/') || segment.contains(&b'\\') {
                return Err(LocationError::InvalidName(
                    String::from_utf8_lossy(segment).into_owned(),
                ));
            }
            if let Ok(name) = std::str::from_utf8(segment) {
                reject_windows_device_name(name)?;
            }
        }
        Ok(())
    }

    fn into_location(self) -> Result<Location, LocationError> {
        self.validate_segments()?;
        let mut uri = format!("webdav://{}/", self.connection_id);
        uri.push_str(
            &self
                .segments
                .iter()
                .map(|segment| percent_encode(segment))
                .collect::<Vec<_>>()
                .join("/"),
        );
        Location::parse(&uri)
    }

    fn join(mut self, name: &str) -> Result<Location, LocationError> {
        self.segments.push(name.as_bytes().to_vec());
        self.into_location()
    }

    fn parent(mut self) -> Result<Option<Location>, LocationError> {
        if self.segments.pop().is_none() {
            return Ok(None);
        }
        self.into_location().map(Some)
    }

    fn name(&self) -> Result<String, LocationError> {
        let bytes = self
            .segments
            .last()
            .ok_or_else(|| LocationError::InvalidName("webdav root has no name".to_owned()))?;
        String::from_utf8(bytes.clone()).map_err(|_| LocationError::InvalidUnicode)
    }
}

/// `onedrive://<connection-id>/<percent-encoded-path>` (task 0110), mirroring
/// [`ParsedSftpUri`]/[`ParsedS3Uri`]/[`ParsedWebDavUri`] exactly: the
/// connection id is an opaque UUID-text path segment referencing a saved
/// connection, never a structured `fm_connections` type (`fm-domain` must
/// not depend on `fm-connections`) and never a Microsoft Graph drive item id
/// or access token - the provider resolves those separately, out of band
/// from the location itself. The path segments address the item relative to
/// the connection's default drive root (`/me/drive/root`), not a Graph item
/// id, so a location never needs to change when a file is only renamed on
/// the Graph side without moving through this provider.
#[derive(Debug)]
struct ParsedOneDriveUri {
    connection_id: String,
    segments: Vec<Vec<u8>>,
}

impl ParsedOneDriveUri {
    fn parse(uri: &str) -> Result<Self, LocationError> {
        let remainder = uri
            .strip_prefix("onedrive://")
            .ok_or(LocationError::InvalidUri)?;
        if remainder.contains(['?', '#']) {
            return Err(LocationError::InvalidUri);
        }
        let (connection_id, path) = remainder.split_once('/').ok_or(LocationError::InvalidUri)?;
        if connection_id.is_empty() || uuid::Uuid::parse_str(connection_id).is_err() {
            return Err(LocationError::InvalidUri);
        }
        let segments = if path.is_empty() {
            Vec::new()
        } else {
            let raw_segments: Vec<&str> = path.split('/').collect();
            if raw_segments.iter().any(|segment| segment.is_empty()) {
                return Err(LocationError::EmptySegment);
            }
            raw_segments
                .into_iter()
                .map(percent_decode)
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Self {
            connection_id: connection_id.to_owned(),
            segments,
        })
    }

    fn validate_segments(&self) -> Result<(), LocationError> {
        for segment in &self.segments {
            if segment.contains(&0) {
                return Err(LocationError::NullByte);
            }
            if segment.contains(&b'/') || segment.contains(&b'\\') {
                return Err(LocationError::InvalidName(
                    String::from_utf8_lossy(segment).into_owned(),
                ));
            }
            if let Ok(name) = std::str::from_utf8(segment) {
                reject_windows_device_name(name)?;
            }
        }
        Ok(())
    }

    fn into_location(self) -> Result<Location, LocationError> {
        self.validate_segments()?;
        let mut uri = format!("onedrive://{}/", self.connection_id);
        uri.push_str(
            &self
                .segments
                .iter()
                .map(|segment| percent_encode(segment))
                .collect::<Vec<_>>()
                .join("/"),
        );
        Location::parse(&uri)
    }

    fn join(mut self, name: &str) -> Result<Location, LocationError> {
        self.segments.push(name.as_bytes().to_vec());
        self.into_location()
    }

    fn parent(mut self) -> Result<Option<Location>, LocationError> {
        if self.segments.pop().is_none() {
            return Ok(None);
        }
        self.into_location().map(Some)
    }

    fn name(&self) -> Result<String, LocationError> {
        let bytes = self
            .segments
            .last()
            .ok_or_else(|| LocationError::InvalidName("onedrive root has no name".to_owned()))?;
        String::from_utf8(bytes.clone()).map_err(|_| LocationError::InvalidUnicode)
    }
}

#[derive(Debug)]
struct ParsedFtpUri {
    scheme: String,
    connection_id: String,
    segments: Vec<Vec<u8>>,
}
impl ParsedFtpUri {
    fn parse(uri: &str) -> Result<Self, LocationError> {
        let scheme = parse_scheme(uri)?;
        if !matches!(scheme, "ftp" | "ftps") {
            return Err(LocationError::InvalidUri);
        }
        let remainder = uri
            .strip_prefix(&format!("{scheme}://"))
            .ok_or(LocationError::InvalidUri)?;
        if remainder.contains(['?', '#']) {
            return Err(LocationError::InvalidUri);
        }
        let (connection_id, path) = remainder.split_once('/').ok_or(LocationError::InvalidUri)?;
        if uuid::Uuid::parse_str(connection_id).is_err() {
            return Err(LocationError::InvalidUri);
        }
        let segments = if path.is_empty() {
            Vec::new()
        } else {
            let raw: Vec<_> = path.split('/').collect();
            if raw.iter().any(|segment| segment.is_empty()) {
                return Err(LocationError::EmptySegment);
            }
            raw.into_iter()
                .map(percent_decode)
                .collect::<Result<_, _>>()?
        };
        Ok(Self {
            scheme: scheme.to_owned(),
            connection_id: connection_id.to_owned(),
            segments,
        })
    }
    fn validate_segments(&self) -> Result<(), LocationError> {
        for segment in &self.segments {
            if segment.contains(&0) {
                return Err(LocationError::NullByte);
            }
            if segment.contains(&b'/') || segment.contains(&b'\\') {
                return Err(LocationError::InvalidName(
                    String::from_utf8_lossy(segment).into_owned(),
                ));
            }
        }
        Ok(())
    }
    fn into_location(self) -> Result<Location, LocationError> {
        self.validate_segments()?;
        let path = self
            .segments
            .iter()
            .map(|segment| percent_encode(segment))
            .collect::<Vec<_>>()
            .join("/");
        Location::parse(&format!("{}://{}/{path}", self.scheme, self.connection_id))
    }
    fn join(mut self, name: &str) -> Result<Location, LocationError> {
        self.segments.push(name.as_bytes().to_vec());
        self.into_location()
    }
    fn parent(mut self) -> Result<Option<Location>, LocationError> {
        if self.segments.pop().is_none() {
            Ok(None)
        } else {
            self.into_location().map(Some)
        }
    }
    fn name(&self) -> Result<String, LocationError> {
        String::from_utf8(
            self.segments
                .last()
                .ok_or_else(|| LocationError::InvalidName("FTP root has no name".to_owned()))?
                .clone(),
        )
        .map_err(|_| LocationError::InvalidUnicode)
    }
}

impl ParsedArchiveUri {
    fn parse(uri: &str) -> Result<Self, LocationError> {
        let remainder = uri
            .strip_prefix("archive://")
            .ok_or(LocationError::InvalidUri)?;
        if remainder.contains(['?', '#']) {
            return Err(LocationError::InvalidUri);
        }
        let (outer, inner) = remainder.split_once('!').ok_or(LocationError::InvalidUri)?;
        if outer.is_empty() || outer.contains('!') {
            return Err(LocationError::InvalidUri);
        }
        let outer_file_uri = format!("file://{outer}");
        let parsed_outer = ParsedFileUri::parse(&outer_file_uri)?;
        parsed_outer.validate_segments()?;
        if parsed_outer.segments.is_empty() {
            return Err(LocationError::InvalidUri);
        }
        let inner = if inner.is_empty() {
            ""
        } else {
            inner.strip_prefix('/').ok_or(LocationError::InvalidUri)?
        };
        let inner_segments = if inner.is_empty() {
            Vec::new()
        } else {
            inner
                .split('/')
                .map(|segment| {
                    let decoded = percent_decode(segment)?;
                    let name = String::from_utf8(decoded.clone())
                        .map_err(|_| LocationError::InvalidUnicode)?;
                    validate_name(&name)?;
                    Ok(decoded)
                })
                .collect::<Result<Vec<_>, LocationError>>()?
        };
        Ok(Self {
            outer: outer.to_owned(),
            inner_segments,
        })
    }

    fn into_location(self) -> Location {
        let inner = self
            .inner_segments
            .iter()
            .map(|segment| percent_encode(segment))
            .collect::<Vec<_>>()
            .join("/");
        let separator = if inner.is_empty() { "" } else { "/" };
        Location::new(
            ProviderId::new("archive"),
            format!("archive://{}!{separator}{inner}", self.outer),
        )
    }

    fn join(mut self, name: &str) -> Result<Location, LocationError> {
        self.inner_segments.push(name.as_bytes().to_vec());
        Ok(self.into_location())
    }

    fn parent(mut self) -> Result<Option<Location>, LocationError> {
        if self.inner_segments.pop().is_none() {
            return Ok(None);
        }
        Ok(Some(self.into_location()))
    }

    fn name(&self) -> Result<String, LocationError> {
        let segment = self
            .inner_segments
            .last()
            .ok_or_else(|| LocationError::InvalidName("archive root has no name".to_owned()))?;
        String::from_utf8(segment.clone()).map_err(|_| LocationError::InvalidUnicode)
    }
}

impl ParsedFileUri {
    fn parse(uri: &str) -> Result<Self, LocationError> {
        let remainder = uri
            .strip_prefix("file://")
            .ok_or(LocationError::InvalidUri)?;
        if remainder.contains(['?', '#']) {
            return Err(LocationError::InvalidUri);
        }
        let (authority, path) = if let Some(path) = remainder.strip_prefix('/') {
            ("", path)
        } else {
            remainder.split_once('/').ok_or(LocationError::InvalidUri)?
        };
        if path.is_empty() {
            return Ok(Self {
                authority: authority.to_owned(),
                segments: Vec::new(),
            });
        }
        // A single trailing slash is the natural way to write a directory,
        // and the only way to write a Windows drive root (`C:\` -> `file:///C:/`).
        let path = path.strip_suffix('/').unwrap_or(path);
        if path.is_empty() {
            return Ok(Self {
                authority: authority.to_owned(),
                segments: Vec::new(),
            });
        }
        let raw_segments: Vec<&str> = path.split('/').collect();
        if raw_segments.iter().any(|segment| segment.is_empty()) {
            return Err(LocationError::EmptySegment);
        }
        let segments = raw_segments
            .into_iter()
            .map(percent_decode)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            authority: authority.to_owned(),
            segments,
        })
    }

    fn validate_segments(&self) -> Result<(), LocationError> {
        let authority = percent_decode(&self.authority)?;
        if authority.contains(&0) {
            return Err(LocationError::NullByte);
        }
        for segment in &self.segments {
            if segment.contains(&0) {
                return Err(LocationError::NullByte);
            }
            if segment.contains(&b'/') || segment.contains(&b'\\') {
                return Err(LocationError::InvalidName(
                    String::from_utf8_lossy(segment).into_owned(),
                ));
            }
            if let Ok(name) = std::str::from_utf8(segment) {
                reject_windows_device_name(name)?;
            }
        }
        Ok(())
    }

    fn into_location(self) -> Result<Location, LocationError> {
        self.validate_segments()?;
        let mut uri = String::from("file://");
        uri.push_str(&self.authority);
        uri.push('/');
        uri.push_str(
            &self
                .segments
                .iter()
                .map(|segment| percent_encode(segment))
                .collect::<Vec<_>>()
                .join("/"),
        );
        Location::parse(&uri)
    }
}

fn parse_scheme(uri: &str) -> Result<&str, LocationError> {
    let (scheme, _) = uri.split_once(':').ok_or(LocationError::InvalidUri)?;
    if scheme.is_empty()
        || !scheme.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphabetic()
                || index > 0 && (byte.is_ascii_digit() || b"+-.".contains(&byte))
        })
    {
        return Err(LocationError::InvalidUri);
    }
    Ok(scheme)
}

fn provider_for_scheme(scheme: &str) -> Result<&'static str, LocationError> {
    if scheme == SEARCH_SCHEME {
        return Ok(SEARCH_SCHEME);
    }
    SCHEME_MAP
        .iter()
        .find(|(s, _)| *s == scheme)
        .map(|(_, p)| *p)
        .ok_or_else(|| {
            if RESERVED_SCHEMES.contains(&scheme) {
                LocationError::UnsupportedProvider(scheme.to_owned())
            } else {
                LocationError::UnknownProvider(scheme.to_owned())
            }
        })
}

/// Validates the `search://local/{searchId}` shape (spec §24, task 0068).
///
/// Search locations are deliberately not routed through [`ParsedFileUri`]:
/// they address a virtual, provider-owned result set rather than a native
/// path, so segment decoding, percent-encoding and native-path conversion
/// never apply to them.
fn validate_search_uri(uri: &str) -> Result<(), LocationError> {
    let remainder = uri
        .strip_prefix("search://")
        .ok_or(LocationError::InvalidUri)?;
    let mut segments = remainder.split('/');
    let authority = segments.next().unwrap_or_default();
    if authority != SEARCH_AUTHORITY {
        return Err(LocationError::InvalidUri);
    }
    let search_id = segments.next().ok_or(LocationError::InvalidUri)?;
    if search_id.is_empty() || segments.next().is_some() {
        return Err(LocationError::InvalidUri);
    }
    Ok(())
}

/// Validates a single path segment name (empty, traversal, separators, null bytes, Windows reserved names).
pub fn validate_name(name: &str) -> Result<(), LocationError> {
    if name.is_empty() {
        return Err(LocationError::EmptySegment);
    }
    if name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(LocationError::InvalidName(name.to_owned()));
    }
    if name.contains('\0') {
        return Err(LocationError::NullByte);
    }
    reject_windows_device_name(name)
}

fn reject_windows_device_name(name: &str) -> Result<(), LocationError> {
    let stem = name
        .trim_end_matches([' ', '.'])
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || stem.strip_prefix("COM").is_some_and(|number| {
            matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
        || stem.strip_prefix("LPT").is_some_and(|number| {
            matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        });
    if reserved {
        Err(LocationError::ReservedWindowsName(name.to_owned()))
    } else {
        Ok(())
    }
}

fn normalize_segments(segments: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, LocationError> {
    let mut normalized = Vec::new();
    for segment in segments {
        match segment.as_slice() {
            b"." => {}
            b".." => {
                normalized.pop().ok_or(LocationError::EscapesRoot)?;
            }
            _ => normalized.push(segment.clone()),
        }
    }
    Ok(normalized)
}

fn percent_decode(segment: &str) -> Result<Vec<u8>, LocationError> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes
                .get(index + 1)
                .ok_or(LocationError::InvalidPercentEncoding)?;
            let low = *bytes
                .get(index + 2)
                .ok_or(LocationError::InvalidPercentEncoding)?;
            decoded.push(
                hex_value(high)
                    .and_then(|high| hex_value(low).map(|low| high * 16 + low))
                    .ok_or(LocationError::InvalidPercentEncoding)?,
            );
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Ok(decoded)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::new();
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b':') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

#[cfg(unix)]
fn native_path_to_location(path: &Path) -> Result<Location, LocationError> {
    use std::os::unix::ffi::OsStrExt;

    let segments = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(name) => Some(name.as_bytes().to_vec()),
            _ => None,
        })
        .collect();
    ParsedFileUri {
        authority: String::new(),
        segments,
    }
    .into_location()
}

#[cfg(unix)]
fn location_to_native_path(parsed: &ParsedFileUri) -> Result<PathBuf, LocationError> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    if !parsed.authority.is_empty() {
        return Err(LocationError::UnsupportedNativePath);
    }
    parsed.validate_segments()?;
    let mut path = PathBuf::from("/");
    for segment in &parsed.segments {
        path.push(OsStr::from_bytes(segment));
    }
    Ok(path)
}

#[cfg(windows)]
fn native_path_to_location(path: &Path) -> Result<Location, LocationError> {
    let mut native = path.to_string_lossy().replace('\\', "/");
    if let Some(without_prefix) = native.strip_prefix("//?/UNC/") {
        native = format!("//{without_prefix}");
    } else if let Some(without_prefix) = native.strip_prefix("//?/") {
        native = without_prefix.to_owned();
    }
    let native = native.trim_end_matches('/');
    let (authority, path) = if let Some(unc) = native.strip_prefix("//") {
        unc.split_once('/').ok_or(LocationError::InvalidUri)?
    } else {
        ("", native.trim_start_matches('/'))
    };
    let segments = path
        .split('/')
        .map(|segment| segment.as_bytes().to_vec())
        .collect();
    ParsedFileUri {
        authority: authority.to_owned(),
        segments,
    }
    .into_location()
}

#[cfg(windows)]
fn location_to_native_path(parsed: &ParsedFileUri) -> Result<PathBuf, LocationError> {
    parsed.validate_segments()?;
    let mut decoded = parsed
        .segments
        .iter()
        .map(|segment| {
            String::from_utf8(segment.clone()).map_err(|_| LocationError::InvalidUnicode)
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("\\");
    if parsed.authority.is_empty()
        && parsed.segments.len() == 1
        && decoded.as_bytes().get(1) == Some(&b':')
    {
        decoded.push('\\');
    }
    let path = if parsed.authority.is_empty() {
        decoded
    } else {
        format!(r"\\{}\{}", parsed.authority, decoded)
    };
    if path.encode_utf16().count() >= 260 {
        if let Some(unc) = path.strip_prefix(r"\\") {
            Ok(PathBuf::from(format!(r"\\?\UNC\{unc}")))
        } else {
            Ok(PathBuf::from(format!(r"\\?\{path}")))
        }
    } else {
        Ok(PathBuf::from(path))
    }
}
