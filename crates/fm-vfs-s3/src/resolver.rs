use async_trait::async_trait;
use fm_vfs::VfsError;

/// Resolved connection parameters for one S3-compatible connection.
///
/// The secret access key must never be logged; callers obtain it fresh from
/// a [`fm_credentials::CredentialStore`] via [`S3ConnectionResolver::resolve`]
/// rather than caching it beyond the lifetime of one dial.
#[derive(Clone)]
pub struct S3ConnectionParameters {
    /// Endpoint URL. `None` targets AWS S3 in `region`
    /// (`https://s3.<region>.amazonaws.com`); `Some` targets a
    /// S3-compatible endpoint such as MinIO, Cloudflare R2 or Backblaze B2.
    pub endpoint: Option<String>,
    /// The bucket region, for example `"us-east-1"`. S3-compatible stores
    /// that ignore region still require a non-empty value for SigV4 signing.
    pub region: String,
    /// Target bucket name.
    pub bucket: String,
    /// The access key id.
    pub access_key_id: String,
    /// The secret access key.
    pub secret_access_key: String,
}

/// Resolves an opaque connection id to configuration and credentials.
#[async_trait]
pub trait S3ConnectionResolver: Send + Sync {
    /// Resolve one saved connection.
    async fn resolve(&self, connection_id: &str) -> Result<S3ConnectionParameters, VfsError>;
}
