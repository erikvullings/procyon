//! [`ConnectionKind`] (task 0103, spec §5.2).

use serde::{Deserialize, Serialize};

/// The protocol/provider family a [`crate::ConnectionProfile`] connects to.
///
/// Every kind named by the specification is listed now, even though only
/// [`ConnectionKind::Ssh`] has a fully modeled configuration in this task -
/// SFTP itself (task 0104), FTP/FTPS (task 0106) and the native cloud/SMB
/// providers (task 0108+) land later and reuse this same framework.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConnectionKind {
    /// SSH, used for SFTP file access, terminal sessions and remote-desktop
    /// launch (spec §6, §7).
    Ssh,
    /// Plain FTP (spec §8).
    Ftp,
    /// FTP over TLS (spec §8).
    Ftps,
    /// Native OneDrive access, independent of an OS-exposed OneDrive folder
    /// (spec §10, later task).
    OneDrive,
    /// WebDAV.
    WebDav,
    /// Amazon S3 or an S3-compatible object store.
    S3,
    /// Native SMB access, independent of an OS-mounted SMB share (spec §10,
    /// later task).
    Smb,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_serde_json() {
        for kind in [
            ConnectionKind::Ssh,
            ConnectionKind::Ftp,
            ConnectionKind::Ftps,
            ConnectionKind::OneDrive,
            ConnectionKind::WebDav,
            ConnectionKind::S3,
            ConnectionKind::Smb,
        ] {
            let json = serde_json::to_string(&kind).expect("serialization must succeed");
            let parsed: ConnectionKind =
                serde_json::from_str(&json).expect("deserialization must succeed");
            assert_eq!(parsed, kind);
        }
    }
}
