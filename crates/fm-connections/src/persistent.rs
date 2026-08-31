//! A JSON-file-backed [`ConnectionRepository`] (task 0103), mirroring
//! `fm-application`'s `JsonFileWorkspaceRepository`.
//!
//! Each connection is stored as `<connection_id>.json` inside a single
//! directory. Saves are atomic (write to a sibling temp file, then rename
//! into place); a connection file that fails to parse is backed up rather
//! than discarded or silently repaired.
//!
//! The persisted JSON contains only a `credential_ref` (an opaque reference),
//! never secret material - see this module's tests for both the structural
//! guarantee (the type has no secret-bearing field) and a defense-in-depth
//! check that a raw JSON file with an out-of-band secret field injected
//! directly on disk cannot survive a round trip through this repository.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use crate::error::ConnectionError;
use crate::ids::ConnectionId;
use crate::profile::ConnectionProfile;
use crate::repository::ConnectionRepository;

/// Persists connection profiles as versioned JSON files under a single
/// directory (typically the platform config directory), with atomic writes
/// and corrupt-file recovery.
pub struct JsonFileConnectionRepository {
    directory: PathBuf,
}

impl JsonFileConnectionRepository {
    /// Creates a repository rooted at `directory`.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    fn profile_path(&self, id: ConnectionId) -> PathBuf {
        self.directory.join(format!("{id}.json"))
    }

    async fn read_profile_file(
        &self,
        path: &Path,
    ) -> Result<Option<ConnectionProfile>, ConnectionError> {
        let bytes = match tokio::fs::read(path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(ConnectionError::Io(error.to_string())),
        };

        match serde_json::from_slice::<ConnectionProfile>(&bytes) {
            Ok(profile) => Ok(Some(profile)),
            Err(_) => {
                let stem = path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("connection")
                    .to_owned();
                let backup_path = self.backup_corrupt_file(path).await?;
                Err(ConnectionError::Corrupt {
                    id: stem,
                    backup_path,
                })
            }
        }
    }

    async fn backup_corrupt_file(&self, path: &Path) -> Result<PathBuf, ConnectionError> {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("connection.json");
        let backup_path = self
            .directory
            .join(format!("{file_name}.corrupt-{}", Utc::now().timestamp()));
        tokio::fs::rename(path, &backup_path)
            .await
            .map_err(|error| ConnectionError::Io(error.to_string()))?;
        Ok(backup_path)
    }
}

async fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), ConnectionError> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    tokio::fs::create_dir_all(directory)
        .await
        .map_err(|error| ConnectionError::Io(error.to_string()))?;

    let tmp_path = directory.join(format!(".tmp-{}", Uuid::new_v4()));
    tokio::fs::write(&tmp_path, contents)
        .await
        .map_err(|error| ConnectionError::Io(error.to_string()))?;
    tokio::fs::rename(&tmp_path, path)
        .await
        .map_err(|error| ConnectionError::Io(error.to_string()))?;
    Ok(())
}

#[async_trait]
impl ConnectionRepository for JsonFileConnectionRepository {
    async fn list(&self) -> Result<Vec<ConnectionProfile>, ConnectionError> {
        let mut entries = match tokio::fs::read_dir(&self.directory).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(ConnectionError::Io(error.to_string())),
        };

        let mut profiles = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| ConnectionError::Io(error.to_string()))?
        {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            match self.read_profile_file(&path).await {
                Ok(Some(profile)) => profiles.push(profile),
                Ok(None) => {}
                // Already backed up; exclude it from the list rather than
                // failing the whole call.
                Err(ConnectionError::Corrupt { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(profiles)
    }

    async fn load(&self, id: ConnectionId) -> Result<Option<ConnectionProfile>, ConnectionError> {
        self.read_profile_file(&self.profile_path(id)).await
    }

    async fn save(
        &self,
        profile: &ConnectionProfile,
    ) -> Result<ConnectionProfile, ConnectionError> {
        let path = self.profile_path(profile.id);
        let existing = match self.read_profile_file(&path).await {
            Ok(existing) => existing,
            Err(ConnectionError::Corrupt { .. }) => None,
            Err(error) => return Err(error),
        };

        let mut persisted = profile.clone();
        if let Some(existing) = existing {
            persisted.created_at = existing.created_at;
        }
        persisted.updated_at = Utc::now();

        let bytes = serde_json::to_vec_pretty(&persisted)
            .map_err(|error| ConnectionError::Serialization(error.to_string()))?;
        write_atomically(&path, &bytes).await?;
        Ok(persisted)
    }

    async fn delete(&self, id: ConnectionId) -> Result<(), ConnectionError> {
        let path = self.profile_path(id);
        if !path.exists() {
            return Err(ConnectionError::NotFound { id });
        }
        tokio::fs::remove_file(&path)
            .await
            .map_err(|error| ConnectionError::Io(error.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use fm_credentials::CredentialRef;
    use tempfile::TempDir;

    use super::*;
    use crate::configuration::{
        ConnectionConfiguration, HostKeyPolicy, SshAuthenticationMethod, SshConnectionConfiguration,
    };
    use crate::kind::ConnectionKind;

    fn sample_profile() -> ConnectionProfile {
        let now = Utc::now();
        ConnectionProfile {
            id: ConnectionId::new(),
            name: "Home Server".to_owned(),
            kind: ConnectionKind::Ssh,
            configuration: ConnectionConfiguration::Ssh(SshConnectionConfiguration {
                host: "example.test".to_owned(),
                port: 22,
                username: "erik".to_owned(),
                start_path: Some("/home/erik".to_owned()),
                authentication: SshAuthenticationMethod::Password,
                host_key_policy: HostKeyPolicy::PromptOnFirstUse,
                keepalive: None,
            }),
            credential_ref: Some(CredentialRef::new()),
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn save_writes_a_json_file_that_load_can_read_back() {
        let dir = TempDir::new().expect("temp dir");
        let repository = JsonFileConnectionRepository::new(dir.path());
        let profile = sample_profile();

        let saved = repository.save(&profile).await.expect("save must succeed");
        assert!(dir.path().join(format!("{}.json", profile.id)).exists());

        let loaded = repository
            .load(profile.id)
            .await
            .expect("load must succeed")
            .expect("profile must exist");
        assert_eq!(loaded, saved);
    }

    #[tokio::test]
    async fn save_is_atomic_and_leaves_no_temp_files_behind() {
        let dir = TempDir::new().expect("temp dir");
        let repository = JsonFileConnectionRepository::new(dir.path());
        repository
            .save(&sample_profile())
            .await
            .expect("save must succeed");

        let leftover_temp_files = std::fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with(".tmp-"));
        assert!(
            !leftover_temp_files,
            "no .tmp- files should remain after a save"
        );
    }

    #[tokio::test]
    async fn list_returns_every_saved_profile() {
        let dir = TempDir::new().expect("temp dir");
        let repository = JsonFileConnectionRepository::new(dir.path());
        let first = repository.save(&sample_profile()).await.unwrap();
        let second = repository.save(&sample_profile()).await.unwrap();

        let mut ids: Vec<_> = repository
            .list()
            .await
            .unwrap()
            .into_iter()
            .map(|profile| profile.id)
            .collect();
        ids.sort_by_key(ConnectionId::to_string);
        let mut expected = vec![first.id, second.id];
        expected.sort_by_key(ConnectionId::to_string);

        assert_eq!(ids, expected);
    }

    #[tokio::test]
    async fn delete_removes_the_file_and_a_second_delete_reports_not_found() {
        let dir = TempDir::new().expect("temp dir");
        let repository = JsonFileConnectionRepository::new(dir.path());
        let saved = repository.save(&sample_profile()).await.unwrap();

        repository
            .delete(saved.id)
            .await
            .expect("delete must succeed");

        assert_eq!(repository.load(saved.id).await.unwrap(), None);
        assert_eq!(
            repository.delete(saved.id).await.unwrap_err(),
            ConnectionError::NotFound { id: saved.id }
        );
    }

    #[tokio::test]
    async fn a_corrupt_connection_file_is_backed_up_instead_of_crashing() {
        let dir = TempDir::new().expect("temp dir");
        let id = ConnectionId::new();
        let path = dir.path().join(format!("{id}.json"));
        std::fs::write(&path, b"not valid json").expect("write corrupt file");

        let repository = JsonFileConnectionRepository::new(dir.path());
        let error = repository
            .load(id)
            .await
            .expect_err("a corrupt file must not be silently treated as absent");

        let backup_path = match error {
            ConnectionError::Corrupt { backup_path, .. } => backup_path,
            other => panic!("expected Corrupt, got {other:?}"),
        };
        assert!(
            !path.exists(),
            "the corrupt file must be moved out of the way"
        );
        assert!(backup_path.exists(), "the backup file must exist");
    }

    /// Defense in depth beyond "the struct has no such field" (task 0103's
    /// explicit testing requirement): even if a raw JSON file on disk was
    /// tampered with (or written by a buggy/malicious caller bypassing the
    /// typed API entirely) to include a `password` field alongside otherwise
    /// valid connection fields, loading it through the strongly typed
    /// `ConnectionProfile` and saving it back must not retain that field -
    /// serde silently drops unknown fields during deserialization, so the
    /// round-tripped file on disk can never carry it forward.
    #[tokio::test]
    async fn a_smuggled_secret_field_never_survives_a_load_then_save_round_trip() {
        let dir = TempDir::new().expect("temp dir");
        let id = ConnectionId::new();
        let credential_ref = CredentialRef::new();
        let now = Utc::now();
        let mut tampered = serde_json::to_value(sample_profile()).expect("serialize sample");
        tampered["id"] = serde_json::json!(id);
        tampered["credential_ref"] = serde_json::json!(credential_ref);
        tampered["created_at"] = serde_json::json!(now);
        tampered["updated_at"] = serde_json::json!(now);
        // A caller mistakenly (or maliciously) tries to smuggle a plaintext
        // secret into the persisted document, bypassing `ConnectionProfile`'s
        // typed field set entirely.
        tampered["password"] = serde_json::json!("hunter2-smuggled-secret");

        let path = dir.path().join(format!("{id}.json"));
        std::fs::write(&path, serde_json::to_vec(&tampered).unwrap())
            .expect("write tampered fixture");

        let repository = JsonFileConnectionRepository::new(dir.path());
        let loaded = repository
            .load(id)
            .await
            .expect("load must succeed")
            .expect("profile must exist");
        repository.save(&loaded).await.expect("save must succeed");

        let raw = std::fs::read_to_string(&path).expect("read saved file");
        assert!(
            !raw.contains("hunter2-smuggled-secret"),
            "a smuggled secret field must never survive a load-then-save round trip"
        );
        assert!(!raw.contains("password"));
    }
}
