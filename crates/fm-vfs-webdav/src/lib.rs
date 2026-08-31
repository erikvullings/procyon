//! WebDAV (RFC 4918) virtual filesystem provider (task 0147).
mod digest;
pub mod fixture;
mod provider;
mod xml;

pub use provider::{
    WebDavAuthScheme, WebDavConnectionParameters, WebDavConnectionResolver,
    WebDavFileSystemProvider,
};
