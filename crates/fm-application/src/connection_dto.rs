//! Explicit conversions between `fm-connections`/`fm-credentials` domain
//! types and their `fm-transport-dto` wire types (task 0103).
//!
//! `fm-transport-dto` itself never depends on `fm-connections`/`fm-credentials`
//! (matching how it has no dependency on `fm-search`/`fm-archive` either), so
//! this mapping lives here in `fm-application`, which already depends on
//! both sides (spec §3 rule 5: DTOs are converted explicitly, never reused
//! as internal domain models).

use std::time::Duration;

use fm_connections::{
    ConnectionConfiguration, ConnectionKind, ConnectionProfile, ConnectionStatus,
    FtpConnectionConfiguration, HostKeyPolicy, OneDriveConnectionConfiguration, OneDriveDriveType,
    S3ConnectionConfiguration, SmbConnectionConfiguration, SshAuthenticationMethod,
    SshConnectionConfiguration, WebDavAuthenticationScheme, WebDavConnectionConfiguration,
};
use fm_credentials::SecretMaterial;
use fm_transport_dto::{
    ConnectionConfigurationDto, ConnectionDto, ConnectionKindDto, ConnectionSecretInputDto,
    ConnectionStatusDto, FtpConnectionConfigurationDto, HostKeyPolicyDto,
    OneDriveConnectionConfigurationDto, OneDriveDriveTypeDto, S3ConnectionConfigurationDto,
    SmbConnectionConfigurationDto, SshAuthenticationMethodDto, SshConnectionConfigurationDto,
    WebDavAuthenticationSchemeDto, WebDavConnectionConfigurationDto,
};

pub(crate) fn connection_kind_from_dto(kind: ConnectionKindDto) -> ConnectionKind {
    match kind {
        ConnectionKindDto::Ssh => ConnectionKind::Ssh,
        ConnectionKindDto::Ftp => ConnectionKind::Ftp,
        ConnectionKindDto::Ftps => ConnectionKind::Ftps,
        ConnectionKindDto::OneDrive => ConnectionKind::OneDrive,
        ConnectionKindDto::WebDav => ConnectionKind::WebDav,
        ConnectionKindDto::S3 => ConnectionKind::S3,
        ConnectionKindDto::Smb => ConnectionKind::Smb,
    }
}

fn connection_kind_to_dto(kind: ConnectionKind) -> ConnectionKindDto {
    match kind {
        ConnectionKind::Ssh => ConnectionKindDto::Ssh,
        ConnectionKind::Ftp => ConnectionKindDto::Ftp,
        ConnectionKind::Ftps => ConnectionKindDto::Ftps,
        ConnectionKind::OneDrive => ConnectionKindDto::OneDrive,
        ConnectionKind::WebDav => ConnectionKindDto::WebDav,
        ConnectionKind::S3 => ConnectionKindDto::S3,
        ConnectionKind::Smb => ConnectionKindDto::Smb,
    }
}

pub(crate) fn connection_status_to_dto(status: ConnectionStatus) -> ConnectionStatusDto {
    match status {
        ConnectionStatus::Disconnected => ConnectionStatusDto::Disconnected,
        ConnectionStatus::Connecting => ConnectionStatusDto::Connecting,
        ConnectionStatus::Connected => ConnectionStatusDto::Connected,
        ConnectionStatus::Reconnecting => ConnectionStatusDto::Reconnecting,
        ConnectionStatus::AuthenticationRequired => ConnectionStatusDto::AuthenticationRequired,
        ConnectionStatus::HostKeyUnverified => ConnectionStatusDto::HostKeyUnverified,
        ConnectionStatus::HostKeyMismatch => ConnectionStatusDto::HostKeyMismatch,
        ConnectionStatus::Failed => ConnectionStatusDto::Failed,
    }
}

fn ssh_authentication_from_dto(method: SshAuthenticationMethodDto) -> SshAuthenticationMethod {
    match method {
        SshAuthenticationMethodDto::Password => SshAuthenticationMethod::Password,
        SshAuthenticationMethodDto::PrivateKey => SshAuthenticationMethod::PrivateKey,
        SshAuthenticationMethodDto::Agent => SshAuthenticationMethod::Agent,
    }
}

fn ssh_authentication_to_dto(method: SshAuthenticationMethod) -> SshAuthenticationMethodDto {
    match method {
        SshAuthenticationMethod::Password => SshAuthenticationMethodDto::Password,
        SshAuthenticationMethod::PrivateKey => SshAuthenticationMethodDto::PrivateKey,
        SshAuthenticationMethod::Agent => SshAuthenticationMethodDto::Agent,
    }
}

fn host_key_policy_from_dto(policy: HostKeyPolicyDto) -> HostKeyPolicy {
    match policy {
        HostKeyPolicyDto::PromptOnFirstUse => HostKeyPolicy::PromptOnFirstUse,
        HostKeyPolicyDto::RequireKnownHost => HostKeyPolicy::RequireKnownHost,
    }
}

fn host_key_policy_to_dto(policy: HostKeyPolicy) -> HostKeyPolicyDto {
    match policy {
        HostKeyPolicy::PromptOnFirstUse => HostKeyPolicyDto::PromptOnFirstUse,
        HostKeyPolicy::RequireKnownHost => HostKeyPolicyDto::RequireKnownHost,
    }
}

fn webdav_authentication_from_dto(
    scheme: WebDavAuthenticationSchemeDto,
) -> WebDavAuthenticationScheme {
    match scheme {
        WebDavAuthenticationSchemeDto::Basic => WebDavAuthenticationScheme::Basic,
        WebDavAuthenticationSchemeDto::Digest => WebDavAuthenticationScheme::Digest,
    }
}

fn webdav_authentication_to_dto(
    scheme: WebDavAuthenticationScheme,
) -> WebDavAuthenticationSchemeDto {
    match scheme {
        WebDavAuthenticationScheme::Basic => WebDavAuthenticationSchemeDto::Basic,
        WebDavAuthenticationScheme::Digest => WebDavAuthenticationSchemeDto::Digest,
    }
}

fn onedrive_drive_type_from_dto(drive_type: OneDriveDriveTypeDto) -> OneDriveDriveType {
    match drive_type {
        OneDriveDriveTypeDto::Personal => OneDriveDriveType::Personal,
        OneDriveDriveTypeDto::Business => OneDriveDriveType::Business,
        OneDriveDriveTypeDto::DocumentLibrary => OneDriveDriveType::DocumentLibrary,
        OneDriveDriveTypeDto::Unknown => OneDriveDriveType::Unknown,
    }
}

fn onedrive_drive_type_to_dto(drive_type: OneDriveDriveType) -> OneDriveDriveTypeDto {
    match drive_type {
        OneDriveDriveType::Personal => OneDriveDriveTypeDto::Personal,
        OneDriveDriveType::Business => OneDriveDriveTypeDto::Business,
        OneDriveDriveType::DocumentLibrary => OneDriveDriveTypeDto::DocumentLibrary,
        OneDriveDriveType::Unknown => OneDriveDriveTypeDto::Unknown,
    }
}

pub(crate) fn connection_configuration_from_dto(
    configuration: ConnectionConfigurationDto,
) -> ConnectionConfiguration {
    match configuration {
        ConnectionConfigurationDto::Ssh(config) => {
            ConnectionConfiguration::Ssh(SshConnectionConfiguration {
                host: config.host,
                port: config.port,
                username: config.username,
                start_path: config.start_path,
                authentication: ssh_authentication_from_dto(config.authentication),
                host_key_policy: host_key_policy_from_dto(config.host_key_policy),
                keepalive: config.keepalive_seconds.map(Duration::from_secs),
            })
        }
        ConnectionConfigurationDto::Ftp(config) => {
            ConnectionConfiguration::Ftp(FtpConnectionConfiguration {
                host: config.host,
                port: config.port,
                username: config.username,
                start_path: config.start_path,
            })
        }
        ConnectionConfigurationDto::Ftps(config) => {
            ConnectionConfiguration::Ftps(FtpConnectionConfiguration {
                host: config.host,
                port: config.port,
                username: config.username,
                start_path: config.start_path,
            })
        }
        ConnectionConfigurationDto::OneDrive(config) => {
            ConnectionConfiguration::OneDrive(OneDriveConnectionConfiguration {
                account_hint: config.account_hint,
                email: config.email,
                display_name: config.display_name,
                drive_type: config.drive_type.map(onedrive_drive_type_from_dto),
            })
        }
        ConnectionConfigurationDto::WebDav(config) => {
            ConnectionConfiguration::WebDav(WebDavConnectionConfiguration {
                base_url: config.base_url,
                username: config.username,
                authentication: webdav_authentication_from_dto(config.authentication),
                path_prefix: config.path_prefix,
            })
        }
        ConnectionConfigurationDto::S3(config) => {
            ConnectionConfiguration::S3(S3ConnectionConfiguration {
                bucket: config.bucket,
                access_key_id: config.access_key_id,
                region: config.region,
                endpoint: config.endpoint,
                start_path: config.start_path,
            })
        }
        ConnectionConfigurationDto::Smb(config) => {
            ConnectionConfiguration::Smb(SmbConnectionConfiguration {
                server: config.server,
                share: config.share,
            })
        }
    }
}

fn connection_configuration_to_dto(
    configuration: &ConnectionConfiguration,
) -> ConnectionConfigurationDto {
    match configuration {
        ConnectionConfiguration::Ssh(config) => {
            ConnectionConfigurationDto::Ssh(SshConnectionConfigurationDto {
                host: config.host.clone(),
                port: config.port,
                username: config.username.clone(),
                start_path: config.start_path.clone(),
                authentication: ssh_authentication_to_dto(config.authentication),
                host_key_policy: host_key_policy_to_dto(config.host_key_policy),
                keepalive_seconds: config.keepalive.map(|duration| duration.as_secs()),
            })
        }
        ConnectionConfiguration::Ftp(config) => {
            ConnectionConfigurationDto::Ftp(FtpConnectionConfigurationDto {
                host: config.host.clone(),
                port: config.port,
                username: config.username.clone(),
                start_path: config.start_path.clone(),
            })
        }
        ConnectionConfiguration::Ftps(config) => {
            ConnectionConfigurationDto::Ftps(FtpConnectionConfigurationDto {
                host: config.host.clone(),
                port: config.port,
                username: config.username.clone(),
                start_path: config.start_path.clone(),
            })
        }
        ConnectionConfiguration::OneDrive(config) => {
            ConnectionConfigurationDto::OneDrive(OneDriveConnectionConfigurationDto {
                account_hint: config.account_hint.clone(),
                email: config.email.clone(),
                display_name: config.display_name.clone(),
                drive_type: config.drive_type.map(onedrive_drive_type_to_dto),
            })
        }
        ConnectionConfiguration::WebDav(config) => {
            ConnectionConfigurationDto::WebDav(WebDavConnectionConfigurationDto {
                base_url: config.base_url.clone(),
                username: config.username.clone(),
                authentication: webdav_authentication_to_dto(config.authentication),
                path_prefix: config.path_prefix.clone(),
            })
        }
        ConnectionConfiguration::S3(config) => {
            ConnectionConfigurationDto::S3(S3ConnectionConfigurationDto {
                bucket: config.bucket.clone(),
                access_key_id: config.access_key_id.clone(),
                region: config.region.clone(),
                endpoint: config.endpoint.clone(),
                start_path: config.start_path.clone(),
            })
        }
        ConnectionConfiguration::Smb(config) => {
            ConnectionConfigurationDto::Smb(SmbConnectionConfigurationDto {
                server: config.server.clone(),
                share: config.share.clone(),
            })
        }
    }
}

pub(crate) fn secret_material_from_dto(secret: ConnectionSecretInputDto) -> SecretMaterial {
    match secret {
        ConnectionSecretInputDto::Password { password } => SecretMaterial::password(password),
        ConnectionSecretInputDto::PrivateKey { key, passphrase } => {
            SecretMaterial::private_key(key, passphrase)
        }
        ConnectionSecretInputDto::PrivateKeyPath { path, passphrase } => {
            SecretMaterial::private_key_path(path, passphrase)
        }
        ConnectionSecretInputDto::OAuthToken {
            access_token,
            refresh_token,
        } => SecretMaterial::oauth_token(access_token, refresh_token),
        ConnectionSecretInputDto::AccessKey {
            access_key_id,
            secret_access_key,
        } => SecretMaterial::access_key(access_key_id, secret_access_key),
    }
}

/// The connection's browsable root `Location` URI, for a kind whose saved
/// connection maps onto exactly one navigable virtual root (task 0110:
/// OneDrive's `onedrive://<connection-id>/` under the frontend's `CLOUD`
/// favourites group). Deliberately scoped to that need rather than
/// generalized to every kind: nothing here changes what SSH/FTP/S3/WebDAV
/// connections already expose.
fn root_location(kind: ConnectionKind, id: fm_connections::ConnectionId) -> Option<String> {
    match kind {
        ConnectionKind::OneDrive => Some(format!("onedrive://{id}/")),
        ConnectionKind::Ssh
        | ConnectionKind::Ftp
        | ConnectionKind::Ftps
        | ConnectionKind::WebDav
        | ConnectionKind::S3
        | ConnectionKind::Smb => None,
    }
}

/// Builds the response DTO for a profile, paired with its separately
/// tracked runtime [`ConnectionStatus`] (never persisted on the profile
/// itself) and the dialer's last failure message, if any (task 0104 -
/// `None` whenever `status` isn't `Failed`, see
/// [`fm_connections::ConnectionService::last_error`]).
pub(crate) fn connection_dto(
    profile: ConnectionProfile,
    status: ConnectionStatus,
    last_error: Option<String>,
) -> ConnectionDto {
    ConnectionDto {
        id: profile.id.into_inner(),
        name: profile.name,
        kind: connection_kind_to_dto(profile.kind),
        configuration: connection_configuration_to_dto(&profile.configuration),
        has_credential: profile.credential_ref.is_some(),
        status: connection_status_to_dto(status),
        root_location: root_location(profile.kind, profile.id),
        last_error,
        created_at: profile.created_at,
        updated_at: profile.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use fm_credentials::CredentialRef;
    use fm_transport_dto::{ConnectionConfigurationDto, ConnectionKindDto};

    use super::*;

    #[test]
    fn ssh_configuration_round_trips_through_dto_conversion() {
        let configuration = ConnectionConfiguration::Ssh(SshConnectionConfiguration {
            host: "example.test".to_owned(),
            port: 22,
            username: "erik".to_owned(),
            start_path: Some("/home/erik".to_owned()),
            authentication: SshAuthenticationMethod::PrivateKey,
            host_key_policy: HostKeyPolicy::RequireKnownHost,
            keepalive: Some(Duration::from_secs(45)),
        });

        let dto = connection_configuration_to_dto(&configuration);
        let back = connection_configuration_from_dto(dto);

        assert_eq!(back, configuration);
    }

    #[test]
    fn onedrive_configuration_round_trips_through_dto_conversion_including_captured_identity() {
        let configuration = ConnectionConfiguration::OneDrive(OneDriveConnectionConfiguration {
            account_hint: Some("erik@example.test".to_owned()),
            email: Some("erik@example.test".to_owned()),
            display_name: Some("Erik Vullings".to_owned()),
            drive_type: Some(fm_connections::OneDriveDriveType::Business),
        });

        let dto = connection_configuration_to_dto(&configuration);
        assert!(matches!(dto, ConnectionConfigurationDto::OneDrive(_)));
        let back = connection_configuration_from_dto(dto);

        assert_eq!(back, configuration);
    }

    #[test]
    fn onedrive_configuration_before_authorization_round_trips_with_every_field_absent() {
        let configuration =
            ConnectionConfiguration::OneDrive(OneDriveConnectionConfiguration::default());

        let dto = connection_configuration_to_dto(&configuration);
        let back = connection_configuration_from_dto(dto);

        assert_eq!(back, configuration);
    }

    #[test]
    fn webdav_configuration_round_trips_through_dto_conversion() {
        let configuration = ConnectionConfiguration::WebDav(WebDavConnectionConfiguration {
            base_url: "https://cloud.example.test/remote.php/dav/files/erik".to_owned(),
            username: "erik".to_owned(),
            authentication: WebDavAuthenticationScheme::Digest,
            path_prefix: Some("/Photos".to_owned()),
        });

        let dto = connection_configuration_to_dto(&configuration);
        let back = connection_configuration_from_dto(dto);

        assert_eq!(back, configuration);
    }

    #[test]
    fn connection_dto_reports_has_credential_from_the_profiles_reference() {
        let now = Utc::now();
        let profile = ConnectionProfile {
            id: fm_connections::ConnectionId::new(),
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
        };

        let dto = connection_dto(profile, ConnectionStatus::Connected, None);

        assert!(dto.has_credential);
        assert_eq!(dto.status, ConnectionStatusDto::Connected);
        assert_eq!(dto.kind, ConnectionKindDto::Ssh);
        assert!(matches!(
            dto.configuration,
            ConnectionConfigurationDto::Ssh(_)
        ));
        assert_eq!(
            dto.root_location, None,
            "only OneDrive currently exposes a root location"
        );
    }

    #[test]
    fn connection_dto_exposes_the_onedrive_root_location_for_frontend_cloud_navigation() {
        let now = Utc::now();
        let id = fm_connections::ConnectionId::new();
        let profile = ConnectionProfile {
            id,
            name: "My OneDrive".to_owned(),
            kind: ConnectionKind::OneDrive,
            configuration: ConnectionConfiguration::OneDrive(
                fm_connections::OneDriveConnectionConfiguration::default(),
            ),
            credential_ref: Some(CredentialRef::new()),
            created_at: now,
            updated_at: now,
        };

        let dto = connection_dto(profile, ConnectionStatus::Connected, None);

        assert_eq!(dto.root_location, Some(format!("onedrive://{id}/")));
        // Never a token, never anything else - purely the deterministic
        // `onedrive://<connection-id>/` scheme `fm_domain::Location` parses.
        assert!(
            fm_domain::Location::parse(dto.root_location.as_deref().unwrap()).is_ok(),
            "root_location must be a valid onedrive:// location"
        );
    }

    #[test]
    fn secret_material_from_dto_maps_every_variant() {
        assert_eq!(
            secret_material_from_dto(ConnectionSecretInputDto::Password {
                password: "hunter2".to_owned()
            }),
            SecretMaterial::password("hunter2")
        );
        assert_eq!(
            secret_material_from_dto(ConnectionSecretInputDto::PrivateKey {
                key: "key-bytes".to_owned(),
                passphrase: Some("pw".to_owned()),
            }),
            SecretMaterial::private_key("key-bytes", Some("pw".to_owned()))
        );
        assert_eq!(
            secret_material_from_dto(ConnectionSecretInputDto::PrivateKeyPath {
                path: "~/.ssh/id_tno".to_owned(),
                passphrase: Some("pw".to_owned()),
            }),
            SecretMaterial::private_key_path("~/.ssh/id_tno", Some("pw".to_owned()))
        );
        assert_eq!(
            secret_material_from_dto(ConnectionSecretInputDto::OAuthToken {
                access_token: "access".to_owned(),
                refresh_token: None,
            }),
            SecretMaterial::oauth_token("access", None)
        );
    }
}
