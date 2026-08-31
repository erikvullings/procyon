//! Host-key persistence and verification (task 0104, spec §6.4).
//!
//! Mandatory behaviour, verified by this module's own tests plus
//! [`crate::session`]'s integration tests against a real handshake:
//!
//! - a host key never seen before is never auto-accepted;
//! - a host key that differs from a previously accepted one is never
//!   silently accepted, and is reported distinctly from "never seen before";
//! - accepting a key persists it so a later connection with the same
//!   fingerprint succeeds.
//!
//! Entries are keyed by an opaque `&str` chosen by the caller. This crate
//! does not depend on `fm-connections` (see `crate::types`'s module doc), so
//! it cannot use `ConnectionId` directly; callers key by that id's text form
//! in practice (`fm-application`'s documented choice, task 0104 Agent Notes),
//! meaning a fingerprint is reverified from scratch if a connection's
//! host/port is later edited to point elsewhere - a conservative,
//! fail-closed outcome rather than a stale trust surviving a retarget.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::SshError;

/// A previously accepted host key fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredHostKey {
    /// `SHA256:<base64>` fingerprint, as produced by [`crate::fingerprint::fingerprint_of`].
    pub fingerprint: String,
    /// When this fingerprint was accepted.
    pub accepted_at: DateTime<Utc>,
}

/// Persists and looks up accepted host-key fingerprints, keyed by an opaque
/// caller-chosen string (task 0104: keyed by connection id text).
#[async_trait]
pub trait KnownHostsStore: Send + Sync {
    /// Looks up the currently accepted fingerprint for `key`, if any.
    async fn lookup(&self, key: &str) -> Result<Option<StoredHostKey>, SshError>;

    /// Persists `fingerprint` as the accepted host key for `key`, replacing
    /// any previous entry. This is the only way a host key is ever trusted
    /// (spec §6.4: no automatic path reaches this method).
    async fn accept(&self, key: &str, fingerprint: String) -> Result<(), SshError>;

    /// Removes any stored fingerprint for `key` (for example when a
    /// connection is deleted).
    async fn forget(&self, key: &str) -> Result<(), SshError>;
}

/// An in-memory [`KnownHostsStore`], for tests and hosts without durable
/// storage.
#[derive(Debug, Default)]
pub struct InMemoryKnownHostsStore {
    entries: Mutex<HashMap<String, StoredHostKey>>,
}

impl InMemoryKnownHostsStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl KnownHostsStore for InMemoryKnownHostsStore {
    async fn lookup(&self, key: &str) -> Result<Option<StoredHostKey>, SshError> {
        Ok(self
            .entries
            .lock()
            .expect("known-hosts lock poisoned")
            .get(key)
            .cloned())
    }

    async fn accept(&self, key: &str, fingerprint: String) -> Result<(), SshError> {
        self.entries
            .lock()
            .expect("known-hosts lock poisoned")
            .insert(
                key.to_owned(),
                StoredHostKey {
                    fingerprint,
                    accepted_at: Utc::now(),
                },
            );
        Ok(())
    }

    async fn forget(&self, key: &str) -> Result<(), SshError> {
        self.entries
            .lock()
            .expect("known-hosts lock poisoned")
            .remove(key);
        Ok(())
    }
}

/// A JSON-file-backed [`KnownHostsStore`], one file holding every entry.
///
/// Writes are atomic (temp file + rename), mirroring
/// `fm_connections::JsonFileConnectionRepository`'s convention. Fingerprints
/// are not secret, so unlike that repository this store does not back up and
/// quarantine a corrupt file - it treats unreadable content as "no entries
/// yet", the same fail-closed outcome as a missing file, rather than
/// crashing a caller trying to verify a host key.
pub struct JsonFileKnownHostsStore {
    path: PathBuf,
}

impl JsonFileKnownHostsStore {
    /// Creates a store backed by the single JSON file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    async fn read_all(&self) -> HashMap<String, StoredHostKey> {
        match tokio::fs::read(&self.path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    async fn write_all(&self, entries: &HashMap<String, StoredHostKey>) -> Result<(), SshError> {
        let directory = self.path.parent().unwrap_or_else(|| Path::new("."));
        tokio::fs::create_dir_all(directory)
            .await
            .map_err(|error| SshError::KnownHostsStore(error.to_string()))?;
        let bytes = serde_json::to_vec_pretty(entries)
            .map_err(|error| SshError::KnownHostsStore(error.to_string()))?;
        let tmp_path = directory.join(format!(".tmp-{}", Uuid::new_v4()));
        tokio::fs::write(&tmp_path, &bytes)
            .await
            .map_err(|error| SshError::KnownHostsStore(error.to_string()))?;
        tokio::fs::rename(&tmp_path, &self.path)
            .await
            .map_err(|error| SshError::KnownHostsStore(error.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl KnownHostsStore for JsonFileKnownHostsStore {
    async fn lookup(&self, key: &str) -> Result<Option<StoredHostKey>, SshError> {
        Ok(self.read_all().await.get(key).cloned())
    }

    async fn accept(&self, key: &str, fingerprint: String) -> Result<(), SshError> {
        let mut entries = self.read_all().await;
        entries.insert(
            key.to_owned(),
            StoredHostKey {
                fingerprint,
                accepted_at: Utc::now(),
            },
        );
        self.write_all(&entries).await
    }

    async fn forget(&self, key: &str) -> Result<(), SshError> {
        let mut entries = self.read_all().await;
        entries.remove(key);
        self.write_all(&entries).await
    }
}

/// Outcome of comparing a freshly presented host-key fingerprint against
/// [`KnownHostsStore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyVerification {
    /// The presented fingerprint matches the stored one; safe to proceed.
    Trusted {
        /// The fingerprint the server presented (equal to the stored one).
        fingerprint: String,
    },
    /// No fingerprint is stored for this key yet.
    Unverified {
        /// The fingerprint the server just presented.
        fingerprint: String,
    },
    /// A fingerprint is stored, but it does not match what the server just
    /// presented.
    Mismatch {
        /// The fingerprint the server just presented.
        fingerprint: String,
        /// The fingerprint previously accepted and stored.
        expected_fingerprint: String,
    },
}

/// Compares `presented_fingerprint` against whatever is stored for `key`.
///
/// Never mutates the store: acceptance is always a distinct, explicit
/// caller action ([`KnownHostsStore::accept`]).
pub async fn verify_host_key(
    store: &dyn KnownHostsStore,
    key: &str,
    presented_fingerprint: &str,
) -> Result<HostKeyVerification, SshError> {
    Ok(match store.lookup(key).await? {
        None => HostKeyVerification::Unverified {
            fingerprint: presented_fingerprint.to_owned(),
        },
        Some(stored) if stored.fingerprint == presented_fingerprint => {
            HostKeyVerification::Trusted {
                fingerprint: presented_fingerprint.to_owned(),
            }
        }
        Some(stored) => HostKeyVerification::Mismatch {
            fingerprint: presented_fingerprint.to_owned(),
            expected_fingerprint: stored.fingerprint,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lookup_on_an_empty_store_returns_none() {
        let store = InMemoryKnownHostsStore::new();
        assert_eq!(store.lookup("conn-1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn accept_then_lookup_round_trips_the_fingerprint() {
        let store = InMemoryKnownHostsStore::new();
        store
            .accept("conn-1", "SHA256:abc".to_owned())
            .await
            .unwrap();

        let stored = store.lookup("conn-1").await.unwrap().unwrap();
        assert_eq!(stored.fingerprint, "SHA256:abc");
    }

    #[tokio::test]
    async fn accept_overwrites_a_previous_entry_for_the_same_key() {
        let store = InMemoryKnownHostsStore::new();
        store
            .accept("conn-1", "SHA256:old".to_owned())
            .await
            .unwrap();
        store
            .accept("conn-1", "SHA256:new".to_owned())
            .await
            .unwrap();

        let stored = store.lookup("conn-1").await.unwrap().unwrap();
        assert_eq!(stored.fingerprint, "SHA256:new");
    }

    #[tokio::test]
    async fn forget_removes_the_stored_entry() {
        let store = InMemoryKnownHostsStore::new();
        store
            .accept("conn-1", "SHA256:abc".to_owned())
            .await
            .unwrap();
        store.forget("conn-1").await.unwrap();
        assert_eq!(store.lookup("conn-1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn distinct_keys_never_see_each_other_s_entries() {
        let store = InMemoryKnownHostsStore::new();
        store
            .accept("conn-1", "SHA256:one".to_owned())
            .await
            .unwrap();
        assert_eq!(store.lookup("conn-2").await.unwrap(), None);
    }

    #[tokio::test]
    async fn verify_reports_unverified_when_nothing_is_stored() {
        let store = InMemoryKnownHostsStore::new();
        let outcome = verify_host_key(&store, "conn-1", "SHA256:fresh")
            .await
            .unwrap();
        assert_eq!(
            outcome,
            HostKeyVerification::Unverified {
                fingerprint: "SHA256:fresh".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn verify_reports_trusted_when_the_fingerprint_matches() {
        let store = InMemoryKnownHostsStore::new();
        store
            .accept("conn-1", "SHA256:known".to_owned())
            .await
            .unwrap();
        let outcome = verify_host_key(&store, "conn-1", "SHA256:known")
            .await
            .unwrap();
        assert_eq!(
            outcome,
            HostKeyVerification::Trusted {
                fingerprint: "SHA256:known".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn verify_reports_a_distinct_mismatch_when_the_fingerprint_changed() {
        let store = InMemoryKnownHostsStore::new();
        store
            .accept("conn-1", "SHA256:original".to_owned())
            .await
            .unwrap();
        let outcome = verify_host_key(&store, "conn-1", "SHA256:different")
            .await
            .unwrap();
        assert_eq!(
            outcome,
            HostKeyVerification::Mismatch {
                fingerprint: "SHA256:different".to_owned(),
                expected_fingerprint: "SHA256:original".to_owned(),
            }
        );
        // Mismatch is a different variant from Unverified - callers must be
        // able to tell "never seen" apart from "changed".
        assert_ne!(
            outcome,
            HostKeyVerification::Unverified {
                fingerprint: "SHA256:different".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn json_file_store_persists_across_instances() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts.json");

        JsonFileKnownHostsStore::new(&path)
            .accept("conn-1", "SHA256:persisted".to_owned())
            .await
            .unwrap();

        let reopened = JsonFileKnownHostsStore::new(&path);
        let stored = reopened.lookup("conn-1").await.unwrap().unwrap();
        assert_eq!(stored.fingerprint, "SHA256:persisted");
    }

    #[tokio::test]
    async fn json_file_store_write_is_atomic_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts.json");
        JsonFileKnownHostsStore::new(&path)
            .accept("conn-1", "SHA256:abc".to_owned())
            .await
            .unwrap();

        let leftover = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with(".tmp-"));
        assert!(!leftover);
    }

    #[tokio::test]
    async fn json_file_store_lookup_before_any_write_reports_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts.json");
        let store = JsonFileKnownHostsStore::new(&path);
        assert_eq!(store.lookup("conn-1").await.unwrap(), None);
    }
}
