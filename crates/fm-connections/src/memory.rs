//! An in-memory [`ConnectionRepository`] (task 0103).
//!
//! Not durable across process restarts; used by tests and by
//! [`crate::ConnectionService`]'s own unit tests. The persistent, file-backed
//! implementation is [`super::persistent::JsonFileConnectionRepository`].

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;

use crate::error::ConnectionError;
use crate::ids::ConnectionId;
use crate::profile::ConnectionProfile;
use crate::repository::ConnectionRepository;

/// An in-memory connection store, guarded by a [`Mutex`] so it can be shared
/// across async tasks.
#[derive(Debug, Default)]
pub struct InMemoryConnectionRepository {
    profiles: Mutex<HashMap<ConnectionId, ConnectionProfile>>,
}

impl InMemoryConnectionRepository {
    /// Creates an empty repository.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ConnectionRepository for InMemoryConnectionRepository {
    async fn list(&self) -> Result<Vec<ConnectionProfile>, ConnectionError> {
        Ok(self
            .profiles
            .lock()
            .expect("connection lock poisoned")
            .values()
            .cloned()
            .collect())
    }

    async fn load(&self, id: ConnectionId) -> Result<Option<ConnectionProfile>, ConnectionError> {
        Ok(self
            .profiles
            .lock()
            .expect("connection lock poisoned")
            .get(&id)
            .cloned())
    }

    async fn save(
        &self,
        profile: &ConnectionProfile,
    ) -> Result<ConnectionProfile, ConnectionError> {
        let mut profiles = self.profiles.lock().expect("connection lock poisoned");
        let mut persisted = profile.clone();
        if let Some(existing) = profiles.get(&profile.id) {
            persisted.created_at = existing.created_at;
        }
        persisted.updated_at = Utc::now();
        profiles.insert(persisted.id, persisted.clone());
        Ok(persisted)
    }

    async fn delete(&self, id: ConnectionId) -> Result<(), ConnectionError> {
        let mut profiles = self.profiles.lock().expect("connection lock poisoned");
        if profiles.remove(&id).is_some() {
            Ok(())
        } else {
            Err(ConnectionError::NotFound { id })
        }
    }
}

#[cfg(test)]
mod tests {
    use fm_credentials::CredentialRef;

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
    async fn save_then_load_round_trips_the_profile() {
        let repository = InMemoryConnectionRepository::new();
        let profile = sample_profile();

        let saved = repository.save(&profile).await.expect("save must succeed");
        let loaded = repository
            .load(profile.id)
            .await
            .expect("load must succeed")
            .expect("profile must exist");

        assert_eq!(loaded, saved);
    }

    #[tokio::test]
    async fn save_preserves_created_at_and_refreshes_updated_at_on_update() {
        let repository = InMemoryConnectionRepository::new();
        let profile = sample_profile();
        let first = repository.save(&profile).await.unwrap();

        let mut updated = first.clone();
        updated.name = "Renamed".to_owned();
        let second = repository.save(&updated).await.unwrap();

        assert_eq!(second.created_at, first.created_at);
        assert!(second.updated_at >= first.updated_at);
        assert_eq!(second.name, "Renamed");
    }

    #[tokio::test]
    async fn list_returns_every_saved_profile() {
        let repository = InMemoryConnectionRepository::new();
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
    async fn delete_removes_the_profile_and_a_second_delete_reports_not_found() {
        let repository = InMemoryConnectionRepository::new();
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
}
