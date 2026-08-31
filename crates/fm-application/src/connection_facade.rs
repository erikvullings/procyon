//! Connection facade (task 0122).
//!
//! Wraps [`ConnectionService`] and SSH host-key probing, returning fully
//! assembled [`ConnectionDto`] values rather than raw profile/status/error
//! tuples.

use std::sync::Arc;

use fm_connections::{
    ConnectionConfiguration, ConnectionDraft, ConnectionId, ConnectionProfile, ConnectionService,
    HostKeyPolicy, JsonFileConnectionRepository, SshConnectionConfiguration,
};
use fm_ssh::{HostKeyVerification, SshConnectTarget};
use fm_transport_dto::{
    ConnectionDto, CreateConnectionRequestDto, HostKeyProbeDto, UpdateConnectionRequestDto,
};
use uuid::Uuid;

use crate::connection_dto;
use crate::error::ApplicationError;

pub(crate) struct ConnectionFacade {
    connections: Arc<ConnectionService<JsonFileConnectionRepository>>,
    ssh_connections: Arc<fm_ssh::SshConnectionManager>,
}

impl ConnectionFacade {
    pub(crate) fn new(
        connections: Arc<ConnectionService<JsonFileConnectionRepository>>,
        ssh_connections: Arc<fm_ssh::SshConnectionManager>,
    ) -> Self {
        Self {
            connections,
            ssh_connections,
        }
    }

    /// Lists every stored connection profile with its current runtime status.
    pub(crate) async fn list_connections(&self) -> Result<Vec<ConnectionDto>, ApplicationError> {
        let profiles = self.connections.list().await?;
        let mut dtos = Vec::with_capacity(profiles.len());
        for profile in profiles {
            let status = self.connections.status(profile.id).await?;
            let last_error = self.connections.last_error(profile.id).await?;
            dtos.push(connection_dto::connection_dto(profile, status, last_error));
        }
        Ok(dtos)
    }

    /// Loads a single connection profile with its current runtime status.
    pub(crate) async fn get_connection(&self, id: Uuid) -> Result<ConnectionDto, ApplicationError> {
        let connection_id: ConnectionId = id.into();
        let profile = self.connections.get(connection_id).await?;
        let status = self.connections.status(connection_id).await?;
        let last_error = self.connections.last_error(connection_id).await?;
        Ok(connection_dto::connection_dto(profile, status, last_error))
    }

    /// Creates and persists a new connection profile.
    pub(crate) async fn create_connection(
        &self,
        request: CreateConnectionRequestDto,
    ) -> Result<ConnectionDto, ApplicationError> {
        let draft = connection_draft_from_create(&request);
        let profile = self.connections.create(draft).await?;
        let status = self.connections.status(profile.id).await?;
        let last_error = self.connections.last_error(profile.id).await?;
        Ok(connection_dto::connection_dto(profile, status, last_error))
    }

    /// Updates an existing connection profile, optionally replacing its
    /// stored credential.
    pub(crate) async fn update_connection(
        &self,
        id: Uuid,
        request: UpdateConnectionRequestDto,
    ) -> Result<ConnectionDto, ApplicationError> {
        let connection_id: ConnectionId = id.into();
        let draft = connection_draft_from_update(&request);
        let profile = self.connections.update(connection_id, draft).await?;
        let status = self.connections.status(connection_id).await?;
        let last_error = self.connections.last_error(connection_id).await?;
        Ok(connection_dto::connection_dto(profile, status, last_error))
    }

    /// Deletes a connection profile and its stored credential, if any.
    pub(crate) async fn delete_connection(&self, id: Uuid) -> Result<(), ApplicationError> {
        self.connections.delete(id.into()).await?;
        Ok(())
    }

    /// Attempts to connect.
    pub(crate) async fn connect_connection(
        &self,
        id: Uuid,
    ) -> Result<ConnectionDto, ApplicationError> {
        let connection_id: ConnectionId = id.into();
        let status = self.connections.connect(connection_id).await?;
        let profile = self.connections.get(connection_id).await?;
        let last_error = self.connections.last_error(connection_id).await?;
        Ok(connection_dto::connection_dto(profile, status, last_error))
    }

    /// Marks a connection as disconnected.
    pub(crate) async fn disconnect_connection(
        &self,
        id: Uuid,
    ) -> Result<ConnectionDto, ApplicationError> {
        let connection_id: ConnectionId = id.into();
        let status = self.connections.disconnect(connection_id).await?;
        let profile = self.connections.get(connection_id).await?;
        let last_error = self.connections.last_error(connection_id).await?;
        Ok(connection_dto::connection_dto(profile, status, last_error))
    }

    /// Checks whether a connection's configuration and credential are usable
    /// without changing its tracked status.
    pub(crate) async fn test_connection(
        &self,
        id: Uuid,
    ) -> Result<ConnectionDto, ApplicationError> {
        let connection_id: ConnectionId = id.into();
        let status = self.connections.test(connection_id).await?;
        let profile = self.connections.get(connection_id).await?;
        let last_error = self.connections.last_error(connection_id).await?;
        Ok(connection_dto::connection_dto(profile, status, last_error))
    }

    /// Probes an SSH connection's currently presented host key without
    /// authenticating.
    pub(crate) async fn probe_ssh_host_key(
        &self,
        id: Uuid,
    ) -> Result<fm_transport_dto::HostKeyProbeDto, ApplicationError> {
        let connection_id: ConnectionId = id.into();
        let profile = self.connections.get(connection_id).await?;
        let target = ssh_target_of(&profile)?;
        let verification = self
            .ssh_connections
            .probe_host_key(&connection_id.to_string(), &target)
            .await
            .map_err(|error| ApplicationError::PlatformOperationFailed(error.to_string()))?;
        Ok(host_key_probe_dto(verification))
    }

    /// Accepts (persists) a host-key fingerprint for an SSH connection after
    /// re-probing to confirm the host is still presenting the same key.
    pub(crate) async fn accept_ssh_host_key(
        &self,
        id: Uuid,
        fingerprint: String,
    ) -> Result<(), ApplicationError> {
        let connection_id: ConnectionId = id.into();
        let profile = self.connections.get(connection_id).await?;
        let target = ssh_target_of(&profile)?;
        let key = connection_id.to_string();
        let verification = self
            .ssh_connections
            .probe_host_key(&key, &target)
            .await
            .map_err(|error| ApplicationError::PlatformOperationFailed(error.to_string()))?;
        let presented = host_key_verification_fingerprint(&verification);
        if presented != fingerprint {
            return Err(ApplicationError::InvalidRequest(
                "the host key changed again since it was probed; re-probe before accepting"
                    .to_owned(),
            ));
        }
        if matches!(verification, HostKeyVerification::Unverified { .. })
            && matches!(
                ssh_configuration_of(&profile)?.host_key_policy,
                HostKeyPolicy::RequireKnownHost
            )
        {
            return Err(ApplicationError::InvalidRequest(
                "this connection requires a pre-established known host key and does not accept \
                 first-time trust"
                    .to_owned(),
            ));
        }
        self.ssh_connections
            .known_hosts()
            .accept(&key, fingerprint)
            .await
            .map_err(|error| ApplicationError::PlatformOperationFailed(error.to_string()))?;
        Ok(())
    }
}

fn connection_draft_from_create(request: &CreateConnectionRequestDto) -> ConnectionDraft {
    ConnectionDraft {
        name: request.name.clone(),
        kind: connection_dto::connection_kind_from_dto(request.kind),
        configuration: connection_dto::connection_configuration_from_dto(
            request.configuration.clone(),
        ),
        secret: request
            .secret
            .clone()
            .map(connection_dto::secret_material_from_dto),
    }
}

fn connection_draft_from_update(request: &UpdateConnectionRequestDto) -> ConnectionDraft {
    ConnectionDraft {
        name: request.name.clone(),
        kind: connection_dto::connection_kind_from_dto(request.kind),
        configuration: connection_dto::connection_configuration_from_dto(
            request.configuration.clone(),
        ),
        secret: request
            .secret
            .clone()
            .map(connection_dto::secret_material_from_dto),
    }
}

fn ssh_configuration_of(
    profile: &ConnectionProfile,
) -> Result<&SshConnectionConfiguration, ApplicationError> {
    match &profile.configuration {
        ConnectionConfiguration::Ssh(configuration) => Ok(configuration),
        _ => Err(ApplicationError::InvalidRequest(
            "connection is not an SSH connection".to_owned(),
        )),
    }
}

fn ssh_target_of(profile: &ConnectionProfile) -> Result<SshConnectTarget, ApplicationError> {
    let configuration = ssh_configuration_of(profile)?;
    Ok(SshConnectTarget {
        host: configuration.host.clone(),
        port: configuration.port,
        username: configuration.username.clone(),
    })
}

fn host_key_verification_fingerprint(verification: &HostKeyVerification) -> String {
    match verification {
        HostKeyVerification::Trusted { fingerprint }
        | HostKeyVerification::Unverified { fingerprint }
        | HostKeyVerification::Mismatch { fingerprint, .. } => fingerprint.clone(),
    }
}

fn host_key_probe_dto(verification: HostKeyVerification) -> HostKeyProbeDto {
    match verification {
        HostKeyVerification::Trusted { fingerprint } => HostKeyProbeDto::Trusted { fingerprint },
        HostKeyVerification::Unverified { fingerprint } => {
            HostKeyProbeDto::Unverified { fingerprint }
        }
        HostKeyVerification::Mismatch {
            fingerprint,
            expected_fingerprint,
        } => HostKeyProbeDto::Mismatch {
            fingerprint,
            expected_fingerprint,
        },
    }
}
