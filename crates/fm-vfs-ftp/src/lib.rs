//! Passive FTP and explicit FTPS virtual filesystem provider.
pub mod fixture;
mod provider;
pub use provider::{FtpConnectionParameters, FtpConnectionResolver, FtpFileSystemProvider};
