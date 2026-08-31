//! `ConnectionService`: connection CRUD and connect/disconnect/test lifecycle
//! (task 0103, spec §5).
//!
//! ## Honest scope of `connect`/`test` before task 0104/0106
//!
//! No SSH/FTP protocol crate exists yet in this repository - that lands with
//! task 0104 (SFTP) and task 0106 (FTP/FTPS). Until a real
//! [`ConnectionDialer`] is registered for a [`ConnectionKind`] via
//! [`ConnectionService::with_dialer`], `connect`/`test` never perform a live
//! network handshake. Instead they do the most that is honestly possible
//! without one:
//!
//! 1. validate the connection's typed configuration is well-formed;
//! 2. if the configuration's authentication method needs a stored secret
//!    (spec §6.3), require a resolvable `credential_ref` - a missing or
//!    dangling reference reports [`ConnectionStatus::AuthenticationRequired`];
//! 3. resolve the credential (if any) through the injected
//!    [`fm_credentials::CredentialStore`];
//! 4. with all of the above satisfied and no dialer registered, report
//!    [`ConnectionStatus::Connected`] - this means "this connection's saved
//!    configuration and credential are usable", **not** "a live session to
//!    the remote host was established". Once task 0104/0106 registers a real
//!    dialer for a kind, its `dial` outcome replaces this stand-in entirely
//!    for that kind.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use fm_credentials::{CredentialStore, ResolvedCredential, SecretMaterial, StoreCredentialRequest};
use fm_events::{EventAudience, EventBus};

use crate::configuration::ConnectionConfiguration;
use crate::dialer::ConnectionDialer;
use crate::error::{ConnectionError, ConnectionValidationError};
use crate::events;
use crate::ids::ConnectionId;
use crate::kind::ConnectionKind;
use crate::profile::ConnectionProfile;
use crate::repository::ConnectionRepository;
use crate::status::ConnectionStatus;

/// The fields a caller supplies to create or update a [`ConnectionProfile`].
///
/// Deliberately distinct from [`ConnectionProfile`] itself: a draft has no
/// `id`/timestamps (assigned by the service) and carries `secret` as new
/// secret material to store, never a `credential_ref` directly - callers
/// (and the transport layer above them) can only ever write a *new* secret
/// or leave the existing one untouched, never read or directly set a
/// reference.
#[derive(Debug, Clone)]
pub struct ConnectionDraft {
    /// Display name.
    pub name: String,
    /// Protocol/provider family.
    pub kind: ConnectionKind,
    /// Protocol-specific configuration.
    pub configuration: ConnectionConfiguration,
    /// New secret material to store for this connection.
    ///
    /// On [`ConnectionService::create`], `None` means the connection has no
    /// stored credential (fine for, e.g., SSH agent authentication). On
    /// [`ConnectionService::update`], `None` means "leave the existing
    /// credential untouched"; `Some` replaces it (the previous credential is
    /// deleted on a best-effort basis, see that method's documentation).
    pub secret: Option<SecretMaterial>,
}

/// Orchestrates connection persistence and the connect/disconnect/test
/// lifecycle (spec §5) on top of any `R: ConnectionRepository`.
pub struct ConnectionService<R> {
    repository: R,
    credential_store: Arc<dyn CredentialStore>,
    events: EventBus,
    statuses: Mutex<HashMap<ConnectionId, ConnectionStatus>>,
    /// The dialer's failure message from the most recent `connect`/`test`
    /// that ended in [`ConnectionStatus::Failed`], so a caller (and
    /// ultimately the user) can see *why* rather than just that it failed.
    /// Cleared whenever a later attempt reaches any other status.
    last_error_messages: Mutex<HashMap<ConnectionId, String>>,
    dialers: HashMap<ConnectionKind, Arc<dyn ConnectionDialer>>,
}

impl<R> ConnectionService<R>
where
    R: ConnectionRepository,
{
    /// Builds a service backed by `repository`, resolving credentials
    /// through `credential_store` and publishing events onto `events`.
    pub fn new(
        repository: R,
        credential_store: Arc<dyn CredentialStore>,
        events: EventBus,
    ) -> Self {
        Self {
            repository,
            credential_store,
            events,
            statuses: Mutex::new(HashMap::new()),
            last_error_messages: Mutex::new(HashMap::new()),
            dialers: HashMap::new(),
        }
    }

    /// Registers a real protocol dialer for `kind` (task 0104/0106's
    /// extension point). Not called by anything in this task - no dialer is
    /// registered for any kind yet.
    #[must_use]
    pub fn with_dialer(mut self, kind: ConnectionKind, dialer: Arc<dyn ConnectionDialer>) -> Self {
        self.dialers.insert(kind, dialer);
        self
    }

    /// Lists every stored connection profile.
    pub async fn list(&self) -> Result<Vec<ConnectionProfile>, ConnectionError> {
        self.repository.list().await
    }

    /// Loads a single connection profile by id.
    pub async fn get(&self, id: ConnectionId) -> Result<ConnectionProfile, ConnectionError> {
        self.repository
            .load(id)
            .await?
            .ok_or(ConnectionError::NotFound { id })
    }

    /// Creates and persists a new connection profile.
    pub async fn create(
        &self,
        draft: ConnectionDraft,
    ) -> Result<ConnectionProfile, ConnectionError> {
        let now = Utc::now();
        let mut profile = ConnectionProfile {
            id: ConnectionId::new(),
            name: draft.name,
            kind: draft.kind,
            configuration: draft.configuration,
            credential_ref: None,
            created_at: now,
            updated_at: now,
        };
        validate_or_error(&profile)?;
        reject_onedrive_secret(profile.kind, draft.secret.as_ref())?;

        if let Some(secret) = draft.secret {
            let reference = self
                .credential_store
                .store(StoreCredentialRequest::new(profile.name.clone(), secret))
                .await?;
            profile.credential_ref = Some(reference);
        }

        let persisted = self.repository.save(&profile).await?;
        self.events.publish(
            EventAudience::Global,
            events::connection_created(persisted.id),
        );
        Ok(persisted)
    }

    /// Updates an existing connection profile's name, kind and
    /// configuration, and optionally replaces its stored credential.
    ///
    /// Replacing the credential stores the new secret first, then makes a
    /// best-effort attempt to delete the previous one: a transient
    /// credential-store failure while deleting the *old* secret never blocks
    /// the update (the new secret is already usable), so the old entry may
    /// occasionally be orphaned rather than the update failing outright.
    pub async fn update(
        &self,
        id: ConnectionId,
        draft: ConnectionDraft,
    ) -> Result<ConnectionProfile, ConnectionError> {
        let existing = self.get(id).await?;

        let mut profile = ConnectionProfile {
            id,
            name: draft.name,
            kind: draft.kind,
            configuration: draft.configuration,
            credential_ref: existing.credential_ref,
            created_at: existing.created_at,
            updated_at: Utc::now(),
        };
        validate_or_error(&profile)?;
        reject_onedrive_secret(profile.kind, draft.secret.as_ref())?;

        if let Some(secret) = draft.secret {
            let reference = self
                .credential_store
                .store(StoreCredentialRequest::new(profile.name.clone(), secret))
                .await?;
            profile.credential_ref = Some(reference);
            if let Some(previous) = existing.credential_ref {
                let _ = self.credential_store.delete(&previous).await;
            }
        }

        let persisted = self.repository.save(&profile).await?;
        self.events.publish(
            EventAudience::Global,
            events::connection_updated(persisted.id),
        );
        Ok(persisted)
    }

    /// Replaces a connection's configuration and/or stored credential
    /// without touching its `name`/`kind`, bypassing the "no directly
    /// supplied secret" restriction [`Self::create`]/[`Self::update`]
    /// enforce for [`ConnectionKind::OneDrive`] (task 0110).
    ///
    /// This is a backend-only orchestration seam - never wired to a raw
    /// transport DTO - for provider-derived state that only the backend
    /// itself can produce correctly: OneDrive's authorization completion
    /// capturing verified account identity/`driveType` alongside the newly
    /// issued OAuth credential, and silent refresh-token rotation replacing
    /// only the stored secret. `configuration`/`secret` are each replaced
    /// wholesale when supplied, `None` leaving that piece untouched exactly
    /// like [`Self::update`]'s existing secret convention.
    ///
    /// Mirrors [`Self::update`]'s credential-replacement sequencing: the new
    /// secret is stored and the profile saved with its reference *before* a
    /// best-effort attempt to delete whatever secret the profile referenced
    /// previously, so a transient failure deleting the predecessor never
    /// blocks the rotation - the new secret is already the one in effect.
    /// Callers that must never let two refreshes for the same connection
    /// race each other (task 0110's token resolver) are responsible for
    /// serializing their own calls; this method does not lock per-connection
    /// itself.
    pub async fn apply_provider_update(
        &self,
        id: ConnectionId,
        configuration: Option<ConnectionConfiguration>,
        secret: Option<SecretMaterial>,
    ) -> Result<ConnectionProfile, ConnectionError> {
        let existing = self.get(id).await?;

        let mut profile = existing.clone();
        if let Some(configuration) = configuration {
            profile.configuration = configuration;
        }
        profile.updated_at = Utc::now();
        validate_or_error(&profile)?;

        if let Some(secret) = secret {
            let reference = self
                .credential_store
                .store(StoreCredentialRequest::new(profile.name.clone(), secret))
                .await?;
            profile.credential_ref = Some(reference);
            if let Some(previous) = existing.credential_ref
                && Some(previous) != profile.credential_ref
            {
                let _ = self.credential_store.delete(&previous).await;
            }
        }

        let persisted = self.repository.save(&profile).await?;
        self.events.publish(
            EventAudience::Global,
            events::connection_updated(persisted.id),
        );
        Ok(persisted)
    }

    /// Deletes a connection profile and, on a best-effort basis, its stored
    /// credential.
    ///
    /// Credential cleanup never blocks the deletion itself: the connection
    /// record is authoritative, so a credential-store failure while removing
    /// the now-orphaned secret is swallowed rather than surfaced as a delete
    /// failure (documented gap, task 0103 Agent Notes).
    pub async fn delete(&self, id: ConnectionId) -> Result<(), ConnectionError> {
        let existing = self.get(id).await?;
        self.repository.delete(id).await?;
        if let Some(reference) = existing.credential_ref {
            let _ = self.credential_store.delete(&reference).await;
        }
        self.statuses
            .lock()
            .expect("status lock poisoned")
            .remove(&id);
        self.events
            .publish(EventAudience::Global, events::connection_deleted(id));
        Ok(())
    }

    /// Returns this connection's currently tracked status, defaulting to
    /// [`ConnectionStatus::Disconnected`] for a connection that has never
    /// been connected in this process.
    pub async fn status(&self, id: ConnectionId) -> Result<ConnectionStatus, ConnectionError> {
        self.get(id).await?;
        Ok(self.tracked_status(id))
    }

    /// Returns the dialer's failure message from the most recent
    /// `connect`/`test` that ended in [`ConnectionStatus::Failed`], or
    /// `None` if the connection has never failed that way (or a later
    /// attempt reached a different status since).
    pub async fn last_error(&self, id: ConnectionId) -> Result<Option<String>, ConnectionError> {
        self.get(id).await?;
        Ok(self
            .last_error_messages
            .lock()
            .expect("last-error lock poisoned")
            .get(&id)
            .cloned())
    }

    /// Checks whether this connection's configuration and referenced
    /// credential are currently usable, then persists and publishes the
    /// resulting reachability status.
    pub async fn test(&self, id: ConnectionId) -> Result<ConnectionStatus, ConnectionError> {
        let profile = self.get(id).await?;
        let outcome = self.evaluate(&profile).await;
        let status = outcome
            .as_ref()
            .copied()
            .unwrap_or(ConnectionStatus::Failed);
        self.set_status_and_publish(id, status);
        outcome
    }

    /// Attempts to connect, transitioning through
    /// [`ConnectionStatus::Connecting`] to the outcome reported by
    /// [`ConnectionService::evaluate`], persisting the tracked status and
    /// publishing `connection.statusChanged` after every transition.
    pub async fn connect(&self, id: ConnectionId) -> Result<ConnectionStatus, ConnectionError> {
        let profile = self.get(id).await?;
        self.set_status_and_publish(id, ConnectionStatus::Connecting);

        let outcome = self.evaluate(&profile).await;
        let status = outcome
            .as_ref()
            .copied()
            .unwrap_or(ConnectionStatus::Failed);
        self.set_status_and_publish(id, status);
        outcome
    }

    /// Marks a connection as disconnected and publishes
    /// `connection.statusChanged`.
    pub async fn disconnect(&self, id: ConnectionId) -> Result<ConnectionStatus, ConnectionError> {
        self.get(id).await?;
        self.clear_last_error(id);
        self.set_status_and_publish(id, ConnectionStatus::Disconnected);
        Ok(ConnectionStatus::Disconnected)
    }

    fn clear_last_error(&self, id: ConnectionId) {
        self.last_error_messages
            .lock()
            .expect("last-error lock poisoned")
            .remove(&id);
    }

    fn set_last_error(&self, id: ConnectionId, message: String) {
        self.last_error_messages
            .lock()
            .expect("last-error lock poisoned")
            .insert(id, message);
    }

    fn tracked_status(&self, id: ConnectionId) -> ConnectionStatus {
        self.statuses
            .lock()
            .expect("status lock poisoned")
            .get(&id)
            .copied()
            .unwrap_or_default()
    }

    fn set_status_and_publish(&self, id: ConnectionId, status: ConnectionStatus) {
        self.statuses
            .lock()
            .expect("status lock poisoned")
            .insert(id, status);
        self.events.publish(
            EventAudience::Global,
            events::connection_status_changed(id, status),
        );
    }

    async fn evaluate(
        &self,
        profile: &ConnectionProfile,
    ) -> Result<ConnectionStatus, ConnectionError> {
        // Every call is a fresh attempt: clear any stale message from a
        // previous failure up front, and only the generic-failure branch
        // below repopulates it.
        self.clear_last_error(profile.id);

        let validation_errors = profile.validate();
        if !validation_errors.is_empty() {
            return Err(ConnectionError::Invalid(validation_errors));
        }

        let resolved: Option<ResolvedCredential> = match &profile.credential_ref {
            Some(reference) => match self.credential_store.resolve(reference).await {
                Ok(resolved) => Some(resolved),
                Err(fm_credentials::CredentialError::NotFound { .. }) => {
                    return Ok(ConnectionStatus::AuthenticationRequired);
                }
                Err(error) => return Err(ConnectionError::Credential(error)),
            },
            None => {
                if profile.configuration.requires_stored_credential() {
                    return Ok(ConnectionStatus::AuthenticationRequired);
                }
                None
            }
        };

        if let Some(dialer) = self.dialers.get(&profile.kind) {
            return match dialer.dial(profile, resolved.as_ref()).await {
                Ok(()) => Ok(ConnectionStatus::Connected),
                // Host-key states are distinguished from a generic dial
                // failure (task 0104, spec §6.4) so a caller can offer an
                // explicit "accept this host key" action rather than a bare
                // "failed" status indistinguishable from a wrong password or
                // network outage.
                Err(ConnectionError::HostKeyUnverified { .. }) => {
                    Ok(ConnectionStatus::HostKeyUnverified)
                }
                Err(ConnectionError::HostKeyMismatch { .. }) => {
                    Ok(ConnectionStatus::HostKeyMismatch)
                }
                Err(other) => {
                    self.set_last_error(profile.id, other.to_string());
                    Ok(ConnectionStatus::Failed)
                }
            };
        }

        // No protocol dialer registered for this kind yet (true for every
        // kind before task 0104/0106): see this module's documentation for
        // what "Connected" means in this stand-in path.
        Ok(ConnectionStatus::Connected)
    }
}

fn validate_or_error(profile: &ConnectionProfile) -> Result<(), ConnectionError> {
    let errors = profile.validate();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ConnectionError::Invalid(errors))
    }
}

/// Rejects a caller-supplied secret for a OneDrive connection (task 0110):
/// its only credential is the OAuth refresh token attached by the
/// authorization flow (via [`ConnectionService::apply_provider_update`]),
/// never a value posted through [`ConnectionService::create`]/
/// [`ConnectionService::update`]'s generic `secret` field - structurally
/// preventing a caller from injecting or replacing a token that way.
fn reject_onedrive_secret(
    kind: ConnectionKind,
    secret: Option<&SecretMaterial>,
) -> Result<(), ConnectionError> {
    if kind == ConnectionKind::OneDrive && secret.is_some() {
        return Err(ConnectionError::Invalid(vec![
            ConnectionValidationError::OneDriveSecretNotAccepted,
        ]));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;
    use fm_credentials::{CredentialError, InMemoryCredentialStore};
    use fm_events::{
        BackendEventPayload, EventBus, EventSubscription, SessionId, SubscriptionEvent,
    };

    use super::*;
    use crate::configuration::{
        HostKeyPolicy, SshAuthenticationMethod, SshConnectionConfiguration,
    };
    use crate::memory::InMemoryConnectionRepository;

    fn subscribe(events: &EventBus) -> EventSubscription {
        events.subscribe_all_workspaces(SessionId::new("test"), None)
    }

    async fn recv_payload(subscription: &mut EventSubscription) -> BackendEventPayload {
        match subscription.recv().await.expect("event bus must not close") {
            SubscriptionEvent::Event(envelope) => envelope.payload,
            other => panic!("expected a typed event, got {other:?}"),
        }
    }

    fn ssh_configuration(auth: SshAuthenticationMethod) -> ConnectionConfiguration {
        ConnectionConfiguration::Ssh(SshConnectionConfiguration {
            host: "example.test".to_owned(),
            port: 22,
            username: "erik".to_owned(),
            start_path: Some("/home/erik".to_owned()),
            authentication: auth,
            host_key_policy: HostKeyPolicy::PromptOnFirstUse,
            keepalive: Some(Duration::from_secs(30)),
        })
    }

    fn draft(auth: SshAuthenticationMethod, secret: Option<SecretMaterial>) -> ConnectionDraft {
        ConnectionDraft {
            name: "Home Server".to_owned(),
            kind: ConnectionKind::Ssh,
            configuration: ssh_configuration(auth),
            secret,
        }
    }

    fn onedrive_draft(secret: Option<SecretMaterial>) -> ConnectionDraft {
        ConnectionDraft {
            name: "My OneDrive".to_owned(),
            kind: ConnectionKind::OneDrive,
            configuration: ConnectionConfiguration::OneDrive(
                crate::configuration::OneDriveConnectionConfiguration::default(),
            ),
            secret,
        }
    }

    fn service() -> ConnectionService<InMemoryConnectionRepository> {
        ConnectionService::new(
            InMemoryConnectionRepository::new(),
            Arc::new(InMemoryCredentialStore::new()),
            EventBus::new(16),
        )
    }

    #[tokio::test]
    async fn create_persists_a_valid_profile() {
        let service = service();

        let profile = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .expect("create must succeed");

        assert_eq!(profile.name, "Home Server");
        assert_eq!(profile.credential_ref, None);
        assert_eq!(service.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn create_rejects_an_invalid_configuration_with_typed_errors() {
        let service = service();
        let mut invalid = draft(SshAuthenticationMethod::Agent, None);
        invalid.name = String::new();

        let error = service.create(invalid).await.unwrap_err();

        assert!(
            matches!(error, ConnectionError::Invalid(errors) if errors.contains(&ConnectionValidationError::EmptyName))
        );
        assert!(service.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_with_a_secret_stores_it_and_sets_the_credential_ref() {
        let service = service();

        let profile = service
            .create(draft(
                SshAuthenticationMethod::Password,
                Some(SecretMaterial::password("hunter2")),
            ))
            .await
            .expect("create must succeed");

        let reference = profile.credential_ref.expect("credential ref must be set");
        let resolved = service.credential_store.resolve(&reference).await.unwrap();
        assert_eq!(resolved.secret, SecretMaterial::password("hunter2"));
    }

    #[tokio::test]
    async fn create_publishes_connection_created() {
        let repository = InMemoryConnectionRepository::new();
        let events = EventBus::new(16);
        let mut subscription = subscribe(&events);
        let service =
            ConnectionService::new(repository, Arc::new(InMemoryCredentialStore::new()), events);

        let profile = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .unwrap();

        let payload = recv_payload(&mut subscription).await;
        assert_eq!(
            payload,
            BackendEventPayload::ConnectionCreated {
                connection_id: profile.id.into_inner()
            }
        );
    }

    #[tokio::test]
    async fn update_changes_fields_and_publishes_connection_updated() {
        let repository = InMemoryConnectionRepository::new();
        let events = EventBus::new(16);
        let mut subscription = subscribe(&events);
        let service =
            ConnectionService::new(repository, Arc::new(InMemoryCredentialStore::new()), events);
        let created = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .unwrap();
        let _ = recv_payload(&mut subscription).await;

        let mut updated_draft = draft(SshAuthenticationMethod::Agent, None);
        updated_draft.name = "Renamed".to_owned();
        let updated = service.update(created.id, updated_draft).await.unwrap();

        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.created_at, created.created_at);
        let payload = recv_payload(&mut subscription).await;
        assert_eq!(
            payload,
            BackendEventPayload::ConnectionUpdated {
                connection_id: created.id.into_inner()
            }
        );
    }

    #[tokio::test]
    async fn update_without_a_new_secret_preserves_the_existing_credential_ref() {
        let service = service();
        let created = service
            .create(draft(
                SshAuthenticationMethod::Password,
                Some(SecretMaterial::password("hunter2")),
            ))
            .await
            .unwrap();

        let updated = service
            .update(created.id, draft(SshAuthenticationMethod::Password, None))
            .await
            .unwrap();

        assert_eq!(updated.credential_ref, created.credential_ref);
    }

    #[tokio::test]
    async fn update_with_a_new_secret_replaces_it_and_deletes_the_old_one() {
        let service = service();
        let created = service
            .create(draft(
                SshAuthenticationMethod::Password,
                Some(SecretMaterial::password("old-password")),
            ))
            .await
            .unwrap();
        let old_reference = created.credential_ref.expect("credential ref must be set");

        let updated = service
            .update(
                created.id,
                draft(
                    SshAuthenticationMethod::Password,
                    Some(SecretMaterial::password("new-password")),
                ),
            )
            .await
            .unwrap();
        let new_reference = updated.credential_ref.expect("credential ref must be set");

        assert_ne!(new_reference, old_reference);
        assert_eq!(
            service
                .credential_store
                .resolve(&new_reference)
                .await
                .unwrap()
                .secret,
            SecretMaterial::password("new-password")
        );
        assert_eq!(
            service
                .credential_store
                .resolve(&old_reference)
                .await
                .unwrap_err(),
            CredentialError::NotFound {
                reference: old_reference
            }
        );
    }

    #[tokio::test]
    async fn update_of_an_unknown_id_reports_not_found() {
        let service = service();
        let error = service
            .update(
                ConnectionId::new(),
                draft(SshAuthenticationMethod::Agent, None),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ConnectionError::NotFound { .. }));
    }

    #[tokio::test]
    async fn delete_removes_the_profile_and_its_credential_and_publishes_deleted() {
        let repository = InMemoryConnectionRepository::new();
        let events = EventBus::new(16);
        let mut subscription = subscribe(&events);
        let service =
            ConnectionService::new(repository, Arc::new(InMemoryCredentialStore::new()), events);
        let created = service
            .create(draft(
                SshAuthenticationMethod::Password,
                Some(SecretMaterial::password("hunter2")),
            ))
            .await
            .unwrap();
        let reference = created.credential_ref.unwrap();
        let _ = recv_payload(&mut subscription).await;

        service
            .delete(created.id)
            .await
            .expect("delete must succeed");

        assert!(matches!(
            service.get(created.id).await.unwrap_err(),
            ConnectionError::NotFound { .. }
        ));
        assert_eq!(
            service
                .credential_store
                .resolve(&reference)
                .await
                .unwrap_err(),
            CredentialError::NotFound { reference }
        );
        let payload = recv_payload(&mut subscription).await;
        assert_eq!(
            payload,
            BackendEventPayload::ConnectionDeleted {
                connection_id: created.id.into_inner()
            }
        );
    }

    #[tokio::test]
    async fn delete_of_a_profile_without_a_credential_succeeds() {
        let service = service();
        let created = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .unwrap();

        service
            .delete(created.id)
            .await
            .expect("delete must succeed");
    }

    #[tokio::test]
    async fn delete_of_an_unknown_id_reports_not_found() {
        let service = service();
        let error = service.delete(ConnectionId::new()).await.unwrap_err();
        assert!(matches!(error, ConnectionError::NotFound { .. }));
    }

    #[tokio::test]
    async fn status_defaults_to_disconnected_for_a_never_connected_profile() {
        let service = service();
        let created = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .unwrap();

        assert_eq!(
            service.status(created.id).await.unwrap(),
            ConnectionStatus::Disconnected
        );
    }

    #[tokio::test]
    async fn status_of_an_unknown_id_reports_not_found() {
        let service = service();
        assert!(matches!(
            service.status(ConnectionId::new()).await.unwrap_err(),
            ConnectionError::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn connect_transitions_to_connected_when_no_credential_is_required() {
        let service = service();
        let created = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .unwrap();

        let status = service
            .connect(created.id)
            .await
            .expect("connect must succeed");

        assert_eq!(status, ConnectionStatus::Connected);
        assert_eq!(
            service.status(created.id).await.unwrap(),
            ConnectionStatus::Connected
        );
    }

    #[tokio::test]
    async fn connect_transitions_to_connected_when_the_credential_resolves() {
        let service = service();
        let created = service
            .create(draft(
                SshAuthenticationMethod::Password,
                Some(SecretMaterial::password("hunter2")),
            ))
            .await
            .unwrap();

        let status = service
            .connect(created.id)
            .await
            .expect("connect must succeed");

        assert_eq!(status, ConnectionStatus::Connected);
    }

    #[tokio::test]
    async fn connect_reports_authentication_required_when_a_password_connection_has_no_credential_ref()
     {
        let service = service();
        let created = service
            .create(draft(SshAuthenticationMethod::Password, None))
            .await
            .unwrap();

        let status = service
            .connect(created.id)
            .await
            .expect("connect must succeed");

        assert_eq!(status, ConnectionStatus::AuthenticationRequired);
    }

    #[tokio::test]
    async fn connect_reports_authentication_required_when_the_credential_ref_is_dangling() {
        let service = service();
        let created = service
            .create(draft(
                SshAuthenticationMethod::Password,
                Some(SecretMaterial::password("hunter2")),
            ))
            .await
            .unwrap();
        let reference = created.credential_ref.unwrap();
        service.credential_store.delete(&reference).await.unwrap();

        let status = service
            .connect(created.id)
            .await
            .expect("connect must succeed");

        assert_eq!(status, ConnectionStatus::AuthenticationRequired);
    }

    #[tokio::test]
    async fn connect_publishes_connecting_then_the_final_status() {
        let repository = InMemoryConnectionRepository::new();
        let events = EventBus::new(16);
        let mut subscription = subscribe(&events);
        let service =
            ConnectionService::new(repository, Arc::new(InMemoryCredentialStore::new()), events);
        let created = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .unwrap();
        let _ = recv_payload(&mut subscription).await;

        service.connect(created.id).await.unwrap();

        let connecting = recv_payload(&mut subscription).await;
        assert_eq!(
            connecting,
            BackendEventPayload::ConnectionStatusChanged {
                connection_id: created.id.into_inner(),
                status: fm_events::ConnectionStatusPayload::Connecting,
            }
        );
        let connected = recv_payload(&mut subscription).await;
        assert_eq!(
            connected,
            BackendEventPayload::ConnectionStatusChanged {
                connection_id: created.id.into_inner(),
                status: fm_events::ConnectionStatusPayload::Connected,
            }
        );
    }

    #[tokio::test]
    async fn disconnect_sets_status_to_disconnected_and_publishes() {
        let service = service();
        let created = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .unwrap();
        service.connect(created.id).await.unwrap();

        let status = service
            .disconnect(created.id)
            .await
            .expect("disconnect must succeed");

        assert_eq!(status, ConnectionStatus::Disconnected);
        assert_eq!(
            service.status(created.id).await.unwrap(),
            ConnectionStatus::Disconnected
        );
    }

    #[tokio::test]
    async fn test_persists_the_reachable_status() {
        let service = service();
        let created = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .unwrap();

        let status = service.test(created.id).await.expect("test must succeed");

        assert_eq!(status, ConnectionStatus::Connected);
        assert_eq!(
            service.status(created.id).await.unwrap(),
            ConnectionStatus::Connected
        );
    }

    struct StubDialer {
        succeeds: AtomicBool,
    }

    #[async_trait]
    impl ConnectionDialer for StubDialer {
        async fn dial(
            &self,
            _profile: &ConnectionProfile,
            _credential: Option<&ResolvedCredential>,
        ) -> Result<(), ConnectionError> {
            if self.succeeds.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(ConnectionError::Invalid(vec![]))
            }
        }
    }

    #[tokio::test]
    async fn a_registered_dialer_s_success_becomes_the_connected_status() {
        let service = service().with_dialer(
            ConnectionKind::Ssh,
            Arc::new(StubDialer {
                succeeds: AtomicBool::new(true),
            }),
        );
        let created = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .unwrap();

        assert_eq!(
            service.connect(created.id).await.unwrap(),
            ConnectionStatus::Connected
        );
    }

    #[tokio::test]
    async fn a_registered_dialer_s_failure_becomes_the_failed_status() {
        let service = service().with_dialer(
            ConnectionKind::Ssh,
            Arc::new(StubDialer {
                succeeds: AtomicBool::new(false),
            }),
        );
        let created = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .unwrap();

        assert_eq!(
            service.connect(created.id).await.unwrap(),
            ConnectionStatus::Failed
        );
    }

    struct FixedErrorDialer {
        error: ConnectionError,
    }

    #[async_trait]
    impl ConnectionDialer for FixedErrorDialer {
        async fn dial(
            &self,
            _profile: &ConnectionProfile,
            _credential: Option<&ResolvedCredential>,
        ) -> Result<(), ConnectionError> {
            Err(self.error.clone())
        }
    }

    #[tokio::test]
    async fn a_dialer_reporting_an_unverified_host_key_becomes_that_distinct_status() {
        let service = service().with_dialer(
            ConnectionKind::Ssh,
            Arc::new(FixedErrorDialer {
                error: ConnectionError::HostKeyUnverified {
                    fingerprint: "SHA256:abc".to_owned(),
                },
            }),
        );
        let created = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .unwrap();

        assert_eq!(
            service.connect(created.id).await.unwrap(),
            ConnectionStatus::HostKeyUnverified
        );
    }

    #[tokio::test]
    async fn a_dialer_reporting_a_host_key_mismatch_becomes_that_distinct_status() {
        let service = service().with_dialer(
            ConnectionKind::Ssh,
            Arc::new(FixedErrorDialer {
                error: ConnectionError::HostKeyMismatch {
                    fingerprint: "SHA256:new".to_owned(),
                    expected_fingerprint: "SHA256:old".to_owned(),
                },
            }),
        );
        let created = service
            .create(draft(SshAuthenticationMethod::Agent, None))
            .await
            .unwrap();

        let status = service.connect(created.id).await.unwrap();
        assert_eq!(status, ConnectionStatus::HostKeyMismatch);
        // Distinct from both "unverified" and the generic "failed" bucket.
        assert_ne!(status, ConnectionStatus::HostKeyUnverified);
        assert_ne!(status, ConnectionStatus::Failed);
    }

    #[tokio::test]
    async fn create_allows_a_onedrive_connection_without_a_secret() {
        // Task 0110: create-before-authorize must remain possible.
        let service = service();

        let profile = service
            .create(onedrive_draft(None))
            .await
            .expect("create must succeed");

        assert_eq!(profile.credential_ref, None);
    }

    #[tokio::test]
    async fn create_rejects_secret_material_for_a_onedrive_connection() {
        let service = service();

        let error = service
            .create(onedrive_draft(Some(SecretMaterial::oauth_token(
                "smuggled-access-token",
                Some("smuggled-refresh-token".to_owned()),
            ))))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            ConnectionError::Invalid(errors)
                if errors == vec![ConnectionValidationError::OneDriveSecretNotAccepted]
        ));
        assert!(
            service.list().await.unwrap().is_empty(),
            "a rejected create must not persist a profile"
        );
    }

    #[tokio::test]
    async fn update_rejects_secret_material_for_a_onedrive_connection() {
        let service = service();
        let created = service.create(onedrive_draft(None)).await.unwrap();

        let error = service
            .update(
                created.id,
                onedrive_draft(Some(SecretMaterial::oauth_token("smuggled-token", None))),
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            ConnectionError::Invalid(errors)
                if errors == vec![ConnectionValidationError::OneDriveSecretNotAccepted]
        ));
    }

    #[tokio::test]
    async fn apply_provider_update_replaces_configuration_and_attaches_a_credential() {
        let service = service();
        let created = service.create(onedrive_draft(None)).await.unwrap();
        assert_eq!(
            service.status(created.id).await.unwrap(),
            ConnectionStatus::Disconnected
        );

        let configuration = ConnectionConfiguration::OneDrive(
            crate::configuration::OneDriveConnectionConfiguration {
                account_hint: None,
                email: Some("erik@example.test".to_owned()),
                display_name: Some("Erik Vullings".to_owned()),
                drive_type: Some(crate::configuration::OneDriveDriveType::Personal),
            },
        );
        let updated = service
            .apply_provider_update(
                created.id,
                Some(configuration.clone()),
                Some(SecretMaterial::oauth_token(
                    "fresh-access-token",
                    Some("fresh-refresh-token".to_owned()),
                )),
            )
            .await
            .expect("apply_provider_update must succeed");

        assert_eq!(updated.configuration, configuration);
        assert_eq!(updated.name, created.name, "name is left untouched");
        assert_eq!(updated.kind, created.kind, "kind is left untouched");
        let reference = updated.credential_ref.expect("credential ref must be set");
        let resolved = service.credential_store.resolve(&reference).await.unwrap();
        assert_eq!(
            resolved.secret,
            SecretMaterial::oauth_token(
                "fresh-access-token",
                Some("fresh-refresh-token".to_owned())
            )
        );
    }

    #[tokio::test]
    async fn apply_provider_update_can_rotate_only_the_credential() {
        let service = service();
        let created = service
            .apply_provider_update(
                service.create(onedrive_draft(None)).await.unwrap().id,
                None,
                Some(SecretMaterial::oauth_token(
                    "old-access-token",
                    Some("old-refresh-token".to_owned()),
                )),
            )
            .await
            .unwrap();
        let old_reference = created.credential_ref.expect("credential ref must be set");

        let rotated = service
            .apply_provider_update(
                created.id,
                None,
                Some(SecretMaterial::oauth_token(
                    "new-access-token",
                    Some("new-refresh-token".to_owned()),
                )),
            )
            .await
            .unwrap();
        let new_reference = rotated.credential_ref.expect("credential ref must be set");

        assert_eq!(
            rotated.configuration, created.configuration,
            "configuration is left untouched when not supplied"
        );
        assert_ne!(
            new_reference, old_reference,
            "rotation must replace the credential reference, never reuse it"
        );
        assert_eq!(
            service
                .credential_store
                .resolve(&new_reference)
                .await
                .unwrap()
                .secret,
            SecretMaterial::oauth_token("new-access-token", Some("new-refresh-token".to_owned()))
        );
        assert!(
            matches!(
                service.credential_store.resolve(&old_reference).await,
                Err(CredentialError::NotFound { .. })
            ),
            "the predecessor credential must be deleted on a best-effort basis"
        );
    }

    #[tokio::test]
    async fn apply_provider_update_publishes_connection_updated() {
        let repository = InMemoryConnectionRepository::new();
        let events = EventBus::new(16);
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let service = ConnectionService::new(repository, credentials, events.clone());
        let created = service.create(onedrive_draft(None)).await.unwrap();
        let mut subscription = subscribe(&events);

        service
            .apply_provider_update(
                created.id,
                None,
                Some(SecretMaterial::oauth_token("access-token", None)),
            )
            .await
            .unwrap();

        let payload = recv_payload(&mut subscription).await;
        assert_eq!(
            payload,
            BackendEventPayload::ConnectionUpdated {
                connection_id: created.id.into_inner()
            }
        );
    }

    #[tokio::test]
    async fn apply_provider_update_reports_not_found_for_an_unknown_connection() {
        let service = service();
        let unknown = ConnectionId::new();

        let error = service
            .apply_provider_update(unknown, None, None)
            .await
            .unwrap_err();

        assert_eq!(error, ConnectionError::NotFound { id: unknown });
    }
}
