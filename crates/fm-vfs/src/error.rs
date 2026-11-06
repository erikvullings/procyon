use fm_domain::ProviderId;

use crate::ProviderCapabilities;

/// Provider-neutral failures exposed by the virtual filesystem boundary.
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    /// The requested location or entry does not exist.
    #[error("location not found: {location}")]
    NotFound {
        /// Provider-neutral URI that could not be found.
        location: String,
    },
    /// Access to the requested location was denied.
    #[error("permission denied: {location}")]
    PermissionDenied {
        /// Provider-neutral URI that could not be accessed.
        location: String,
    },
    /// Another process holds the entry open without sharing it, so the
    /// operation cannot proceed until that process releases it. Distinct
    /// from [`Self::PermissionDenied`], which is about access rights
    /// (specification §8/§23; Windows sharing and lock violations).
    #[error("file is in use by another program: {location}")]
    Locked {
        /// Provider-neutral URI that is currently locked.
        location: String,
    },
    /// Creating or moving an entry would overwrite an existing entry.
    #[error("location already exists: {location}")]
    AlreadyExists {
        /// Provider-neutral URI that already exists.
        location: String,
    },
    /// A directory name was empty.
    #[error("directory name is empty")]
    EmptyName,
    /// A directory name contains characters forbidden by the target platform.
    #[error("directory name contains invalid characters")]
    InvalidNameCharacters,
    /// A directory name is reserved by Windows, including device names with extensions.
    #[error("directory name is reserved")]
    ReservedName,
    /// A directory name attempted to address a parent or nested path.
    #[error("directory name must be one path component")]
    PathTraversalName,
    /// An operation requiring a directory targeted another entry kind.
    #[error("location is not a directory: {location}")]
    NotADirectory {
        /// Provider-neutral URI with the wrong entry kind.
        location: String,
    },
    /// An operation requiring a non-directory targeted a directory.
    #[error("location is a directory: {location}")]
    IsADirectory {
        /// Provider-neutral URI with the wrong entry kind.
        location: String,
    },
    /// The provider does not advertise a required capability.
    #[error("provider does not support capability {capability:?}")]
    UnsupportedCapability {
        /// Capability required by the rejected operation.
        capability: ProviderCapabilities,
    },
    /// An archive entry would escape its virtual root if materialized.
    #[error("archive contains an unsafe entry path")]
    UnsafeArchiveEntry,
    /// Archive expansion exceeded a configured safety limit.
    #[error("archive resource limit exceeded: {kind}")]
    ArchiveResourceLimit {
        /// Stable, non-secret description of the exceeded limit.
        kind: &'static str,
    },
    /// The archive is encrypted and no session credential is available.
    #[error("archive credential required")]
    CredentialRequired,
    /// The cached or supplied archive credential was rejected.
    #[error("archive credential is invalid")]
    InvalidCredential,
    /// A remote provider rejected authentication or could not access the configured
    /// authentication mechanism. The provider supplies recovery guidance that is
    /// safe to show to the user.
    #[error("{message}")]
    AuthenticationFailed {
        /// Sanitized, actionable authentication failure message.
        message: String,
    },
    /// The destination cannot accommodate the requested write.
    #[error("the destination does not have enough available space")]
    InsufficientSpace {
        /// Bytes available when the provider could determine the value.
        available: Option<u64>,
        /// Bytes required when the operation could determine the value.
        required: Option<u64>,
    },
    /// The operating system requires an elevated privilege for this request, e.g. Windows
    /// symbolic links without Developer Mode or `SeCreateSymbolicLinkPrivilege`.
    #[error("an elevated privilege is required: {location}")]
    PrivilegeRequired {
        /// Provider-neutral URI the request targeted.
        location: String,
    },
    /// Creating the requested link would make a directory reachable from itself.
    #[error("the link would create a cycle: {location}")]
    LinkCycle {
        /// Provider-neutral URI of the rejected link.
        location: String,
    },
    /// The caller cancelled the operation.
    #[error("operation cancelled")]
    Cancelled,
    /// The provider encountered an I/O failure safe to report across layers.
    #[error("provider I/O failed: {message}")]
    Io {
        /// Sanitized failure description; never a raw transport response.
        message: String,
    },
    /// A location is malformed or invalid for its provider.
    #[error("invalid location: {location}")]
    InvalidLocation {
        /// Invalid provider-neutral URI.
        location: String,
    },
    /// No provider is registered for a location's provider id.
    #[error("unknown provider: {provider_id}")]
    UnknownProvider {
        /// Provider id that could not be resolved.
        provider_id: ProviderId,
    },
}

impl VfsError {
    /// Whether retrying the same request later can plausibly succeed (task 0301).
    ///
    /// Only provider I/O failures that name a network-shaped cause (dropped or reset
    /// connection, timeout, throttling, a temporarily unavailable server) qualify; an
    /// unclassified I/O error (a bad sector, a read-only filesystem, a name that is too long)
    /// is not retried. Every other variant is a decision, a missing precondition or a security
    /// boundary, and must surface: authentication and credential failures, permission and
    /// privilege errors, conflicts, missing entries, space shortage and cancellation. A host-key
    /// problem reaches this layer as an I/O failure naming the host key, so those are held back
    /// explicitly even when their text also mentions a connection.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        const TRANSIENT: [&str; 12] = [
            "connection reset",
            "connection closed",
            "connection aborted",
            "connection refused",
            "broken pipe",
            "timed out",
            "timeout",
            "temporarily",
            "too many requests",
            "service unavailable",
            "unexpected eof",
            "network",
        ];
        const BLOCKED: [&str; 8] = [
            "host key",
            "hostkey",
            "authenticat",
            "permission denied",
            "access denied",
            "unauthorized",
            "forbidden",
            "not permitted",
        ];
        match self {
            Self::Io { message } => {
                let message = message.to_ascii_lowercase();
                TRANSIENT.iter().any(|needle| message.contains(needle))
                    && !BLOCKED.iter().any(|needle| message.contains(needle))
            }
            _ => false,
        }
    }

    /// Returns the stable machine-readable code used by transport adapters.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "notFound",
            Self::PermissionDenied { .. } => "permissionDenied",
            Self::Locked { .. } => "locked",
            Self::AlreadyExists { .. } => "alreadyExists",
            Self::EmptyName => "emptyName",
            Self::InvalidNameCharacters => "invalidNameCharacters",
            Self::ReservedName => "reservedName",
            Self::PathTraversalName => "pathTraversalName",
            Self::NotADirectory { .. } => "notADirectory",
            Self::IsADirectory { .. } => "isADirectory",
            Self::UnsupportedCapability { .. } => "unsupportedCapability",
            Self::UnsafeArchiveEntry => "unsafeArchiveEntry",
            Self::ArchiveResourceLimit { .. } => "archiveResourceLimit",
            Self::CredentialRequired => "credentialRequired",
            Self::InvalidCredential => "invalidCredential",
            Self::AuthenticationFailed { .. } => "authenticationFailed",
            Self::InsufficientSpace { .. } => "insufficientSpace",
            Self::PrivilegeRequired { .. } => "privilegeRequired",
            Self::LinkCycle { .. } => "linkCycle",
            Self::Cancelled => "cancelled",
            Self::Io { .. } => "io",
            Self::InvalidLocation { .. } => "invalidLocation",
            Self::UnknownProvider { .. } => "unknownProvider",
        }
    }
}
