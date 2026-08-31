//! Microsoft Graph (`/me/drive`) virtual filesystem provider (task 0110).
//!
//! Provides direct Microsoft Graph access for personal Microsoft accounts and
//! Microsoft Entra work/school accounts, without an OS-mounted sync client
//! folder (task 0101 remains the simpler path when the OS already exposes
//! one). This crate implements [`fm_vfs::FileSystemProvider`] against the
//! default drive's Graph REST surface (`/me/drive/root...`); OAuth token
//! acquisition and refresh (`fm-auth-oauth`) and translating a saved
//! `fm-connections` connection into a bearer token are both out of scope
//! here, mirroring the `fm-vfs-sftp`/`fm-vfs-s3`/`fm-vfs-webdav` precedent
//! exactly: [`OneDriveConnectionResolver`] is the seam `fm-application`
//! implements, so this crate never depends on `fm-connections` or
//! `fm-credentials`.
//!
//! ## Locations never carry tokens or Graph item ids
//!
//! An `onedrive://<connection-id>/<percent-encoded-path>`
//! ([`fm_domain::Location`]) addresses an item purely by its path under the
//! connection's default drive root. Mutating Graph APIs that require a
//! stable item id (rename/move) resolve it fresh, out of band, from the
//! location's path - never cached inside the location or the domain-level
//! [`fm_domain::EntryId`] (which is instead a deterministic hash of the
//! location's URI, exactly like `fm-vfs-sftp`'s `entry_id_for`, so it never
//! has to assume a Graph item id happens to look like a UUID).
//!
//! ## Preauthenticated transfer URLs never see the bearer token
//!
//! Downloads resolve `@microsoft.graph.downloadUrl` via one authenticated
//! metadata request, then fetch that URL with **no** `Authorization` header
//! (task 0110's explicit constraint - Graph's download URLs are
//! preauthenticated by the URL itself, and forwarding a bearer token to an
//! arbitrary opaque URL would be exfiltration to a host wholly outside this
//! provider's trust boundary). Uploads mirror this: only the initial
//! `createUploadSession` call is authenticated, every chunk `PUT` afterwards
//! targets the returned `uploadUrl` with no bearer. Opaque continuation URLs
//! that legitimately *do* need the bearer reattached (`@odata.nextLink`
//! paging, delta links) are validated same-origin against the configured
//! Graph base URL first, so a compromised or malformed link can never be
//! used to smuggle the token to an unrelated host.

mod delta;
pub mod fixture;
mod graph;
mod provider;
mod resolver;
mod upload;

pub use graph::{
    DEFAULT_DELTA_PAGE_SIZE, DEFAULT_DELTA_POLL_INTERVAL, GraphConfig, PRODUCTION_GRAPH_BASE_URL,
    RetryPolicy,
};
pub use provider::OneDriveFileSystemProvider;
pub use resolver::{OneDriveAccessToken, OneDriveConnectionResolver};
pub use upload::{SIMPLE_UPLOAD_THRESHOLD, UPLOAD_FRAGMENT_SIZE};
