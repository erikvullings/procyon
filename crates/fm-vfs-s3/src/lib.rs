//! S3-compatible object storage virtual filesystem provider (task 0146).
//!
//! Works against real AWS S3 and any S3-compatible endpoint (MinIO,
//! Cloudflare R2, Backblaze B2, DigitalOcean Spaces, ...) via a configurable
//! endpoint URL, mirroring the `fm-vfs-ftp`/`fm-vfs-sftp` split: this crate
//! implements [`fm_vfs::FileSystemProvider`], while resolving a connection id
//! to credentials and bucket configuration is left to
//! [`S3ConnectionResolver`], a seam `fm-application` implements so this crate
//! never depends on `fm-connections` (matching `fm-vfs-sftp`'s own
//! `SshConnectionResolver` seam and its documented rationale).

pub mod fixture;
mod provider;
mod resolver;

pub use provider::{DEFAULT_MULTIPART_THRESHOLD, S3FileSystemProvider};
pub use resolver::{S3ConnectionParameters, S3ConnectionResolver};
