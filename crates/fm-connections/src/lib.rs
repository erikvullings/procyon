//! Connection-profile domain model, persistence and lifecycle service for
//! application-managed remote connections (task 0103, spec §5, §14).
//!
//! Self-contained feature crate, matching `fm-search`/`fm-archive`'s shape:
//! this crate owns its own domain types and service/store logic rather than
//! routing through `fm-domain`/`fm-application`, and is wired into
//! `fm-application`'s `FileManagerService` facade as a field.
//!
//! Secrets never live here. [`ConnectionProfile`] only ever holds an opaque
//! [`fm_credentials::CredentialRef`]; actual secret material is resolved
//! on demand through an injected [`fm_credentials::CredentialStore`].
//!
//! This task deliberately does not implement any remote filesystem protocol
//! (SSH/SFTP, FTP/FTPS, ...) - see [`ConnectionService`]'s documentation for
//! exactly what `connect`/`test` do without one, and [`ConnectionDialer`] for
//! the extension point a later task registers a real dialer into.

mod configuration;
mod dialer;
mod error;
mod events;
mod ids;
mod kind;
mod memory;
mod persistent;
mod profile;
mod repository;
mod service;
mod status;

pub use configuration::{
    ConnectionConfiguration, FtpConnectionConfiguration, HostKeyPolicy,
    OneDriveConnectionConfiguration, OneDriveDriveType, S3ConnectionConfiguration,
    SmbConnectionConfiguration, SshAuthenticationMethod, SshConnectionConfiguration,
    WebDavAuthenticationScheme, WebDavConnectionConfiguration,
};
pub use dialer::ConnectionDialer;
pub use error::{ConnectionError, ConnectionValidationError};
pub use ids::{ConnectionId, ConnectionIdParseError};
pub use kind::ConnectionKind;
pub use memory::InMemoryConnectionRepository;
pub use persistent::JsonFileConnectionRepository;
pub use profile::ConnectionProfile;
pub use repository::ConnectionRepository;
pub use service::{ConnectionDraft, ConnectionService};
pub use status::ConnectionStatus;
