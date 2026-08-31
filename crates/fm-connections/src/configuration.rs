//! Protocol-specific connection configuration (task 0103, spec §5.2, §6.3).
//!
//! [`ConnectionConfiguration`] is a tagged enum, never a generic map: every
//! kind gets its own typed configuration struct. Only [`SshConnectionConfiguration`]
//! is fully modeled in this task, matching spec §6.3 exactly, since task 0104
//! (SFTP) depends on its shape; the remaining kinds get minimal, honestly
//! scoped stubs so the framework has somewhere to grow without inventing
//! protocol semantics that belong to later tasks (0106, 0108+).
//!
//! None of these structs ever carry secret material - authentication
//! *methods* are typed here, but the secret behind a method (a password, key
//! or token) lives only in a [`fm_credentials::CredentialStore`], referenced
//! by a [`crate::ConnectionProfile`]'s `credential_ref`.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::ConnectionValidationError;
use crate::kind::ConnectionKind;

/// How an SSH connection authenticates (spec §6.3's `SshAuthenticationMethod`).
///
/// Carries no secret content: [`SshAuthenticationMethod::Password`] and
/// [`SshAuthenticationMethod::PrivateKey`] both mean "resolve the secret
/// behind this connection's `credential_ref`"; only the shape of that secret
/// differs (checked by [`crate::ConnectionService`] against the resolved
/// [`fm_credentials::SecretMaterial`] variant, not by this type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SshAuthenticationMethod {
    /// Password authentication.
    Password,
    /// Private-key authentication, with an optional passphrase.
    PrivateKey,
    /// Delegates to a running SSH agent; no stored secret is needed.
    Agent,
}

impl SshAuthenticationMethod {
    /// Whether this method needs a resolvable `credential_ref` on the owning
    /// profile.
    #[must_use]
    pub fn requires_stored_credential(self) -> bool {
        matches!(self, Self::Password | Self::PrivateKey)
    }
}

/// How an SSH connection treats the remote host key (spec §6.4).
///
/// This task only defines the typed policy value; enforcing it against a
/// known-hosts store is task 0104's job once a real SSH dial exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostKeyPolicy {
    /// Prompt for explicit confirmation the first time a host key is seen,
    /// then persist it; any later mismatch must be rejected, never silently
    /// accepted (spec §6.4).
    PromptOnFirstUse,
    /// Only connect if the host key was already explicitly accepted and
    /// stored; never auto-accept, even on the first connection.
    RequireKnownHost,
}

/// SSH connection configuration (spec §6.3, verbatim field set).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SshConnectionConfiguration {
    /// Hostname or IP address.
    pub host: String,
    /// TCP port, conventionally 22.
    pub port: u16,
    /// Remote username.
    pub username: String,
    /// Optional initial remote directory to open when browsing this connection.
    pub start_path: Option<String>,
    /// Authentication method.
    pub authentication: SshAuthenticationMethod,
    /// Host-key verification policy.
    pub host_key_policy: HostKeyPolicy,
    /// Keepalive interval, if any.
    pub keepalive: Option<Duration>,
}

/// Minimal FTP/FTPS configuration (spec §8; fully modeled in task 0106).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FtpConnectionConfiguration {
    /// Hostname or IP address.
    pub host: String,
    /// TCP port, conventionally 21.
    pub port: u16,
    /// Remote username.
    pub username: String,
    /// Optional initial remote directory.
    pub start_path: Option<String>,
}

/// Which Microsoft Graph `driveType` a OneDrive connection's default drive
/// reported (task 0110). Never blocks either kind - both personal and
/// business accounts are fully supported; this is captured purely so the
/// frontend can label the connection accurately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OneDriveDriveType {
    /// A personal Microsoft account's OneDrive.
    Personal,
    /// A Microsoft Entra work or school account's OneDrive for Business.
    Business,
    /// A SharePoint document library exposed as a default drive.
    DocumentLibrary,
    /// Microsoft Graph reported a `driveType` value this workspace does not
    /// yet classify; never a hard failure (spec: never block either drive
    /// type), just an honest "we don't have a friendlier label for this".
    Unknown,
}

/// Native OneDrive configuration (task 0110). Every field here is non-secret
/// display/classification data; the OAuth access and refresh tokens
/// themselves are never stored here - only behind the owning profile's
/// `credential_ref`, resolved through a [`fm_credentials::CredentialStore`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OneDriveConnectionConfiguration {
    /// Optional display hint for which account this connection targets
    /// (for example an email address), shown before authentication
    /// completes.
    pub account_hint: Option<String>,
    /// The signed-in account's email/`userPrincipalName`, captured from
    /// Microsoft Graph `/me` once authorization succeeds. `None` before the
    /// first successful authorization.
    pub email: Option<String>,
    /// The signed-in account's display name, captured from Microsoft Graph
    /// `/me` once authorization succeeds. `None` before the first
    /// successful authorization.
    pub display_name: Option<String>,
    /// The default drive's `driveType`, captured from Microsoft Graph
    /// `/me/drive` once authorization succeeds. `None` before the first
    /// successful authorization.
    pub drive_type: Option<OneDriveDriveType>,
}

/// How a WebDAV connection authenticates (task 0147). WebDAV has no single
/// standard auth scheme, so both must be supported: `Basic` sends the
/// password in every request (only ever over TLS in practice), `Digest`
/// (RFC 7616/2617) never sends the password itself, only a challenge-response
/// hash. Either way, the password is resolved from a
/// [`crate::ConnectionProfile`]'s `credential_ref`, never carried here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebDavAuthenticationScheme {
    /// HTTP Basic authentication.
    Basic,
    /// HTTP Digest authentication.
    Digest,
}

/// WebDAV (RFC 4918) configuration (task 0147).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebDavConnectionConfiguration {
    /// The WebDAV collection's base URL, e.g. `https://cloud.example.test/remote.php/dav/files/erik`.
    pub base_url: String,
    /// Login name.
    pub username: String,
    /// Authentication scheme to use against the server.
    pub authentication: WebDavAuthenticationScheme,
    /// Optional path prefix under `base_url` to open by default.
    pub path_prefix: Option<String>,
}

/// S3-compatible object store configuration (task 0146).
///
/// Only the access key id sits here - it is not secret (compare
/// [`SshConnectionConfiguration::username`]). The secret access key lives
/// behind the owning profile's `credential_ref` as an
/// [`fm_credentials::SecretMaterial::AccessKey`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct S3ConnectionConfiguration {
    /// Target bucket name.
    pub bucket: String,
    /// The access key id.
    pub access_key_id: String,
    /// Optional region.
    pub region: Option<String>,
    /// Optional custom endpoint URL, for S3-compatible (non-AWS) stores such
    /// as MinIO, Cloudflare R2 or Backblaze B2. `None` defaults to AWS S3.
    pub endpoint: Option<String>,
    /// Optional key prefix to browse as this connection's root.
    pub start_path: Option<String>,
}

/// Minimal native SMB configuration stub (later task).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmbConnectionConfiguration {
    /// Server hostname or IP address.
    pub server: String,
    /// Share name.
    pub share: String,
}

/// Tagged, protocol-specific connection configuration (spec §5.2: "must be
/// typed/tagged, not an unstructured map").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConnectionConfiguration {
    /// SSH configuration (spec §6.3).
    Ssh(SshConnectionConfiguration),
    /// Plain FTP configuration.
    Ftp(FtpConnectionConfiguration),
    /// FTP over TLS configuration.
    Ftps(FtpConnectionConfiguration),
    /// Native OneDrive configuration.
    OneDrive(OneDriveConnectionConfiguration),
    /// WebDAV configuration.
    WebDav(WebDavConnectionConfiguration),
    /// S3-compatible object store configuration.
    S3(S3ConnectionConfiguration),
    /// Native SMB configuration.
    Smb(SmbConnectionConfiguration),
}

impl ConnectionConfiguration {
    /// The [`ConnectionKind`] this configuration belongs to.
    #[must_use]
    pub fn kind(&self) -> ConnectionKind {
        match self {
            Self::Ssh(_) => ConnectionKind::Ssh,
            Self::Ftp(_) => ConnectionKind::Ftp,
            Self::Ftps(_) => ConnectionKind::Ftps,
            Self::OneDrive(_) => ConnectionKind::OneDrive,
            Self::WebDav(_) => ConnectionKind::WebDav,
            Self::S3(_) => ConnectionKind::S3,
            Self::Smb(_) => ConnectionKind::Smb,
        }
    }

    /// Whether this configuration's authentication method needs a
    /// resolvable `credential_ref` on the owning profile.
    #[must_use]
    pub fn requires_stored_credential(&self) -> bool {
        match self {
            Self::Ssh(config) => config.authentication.requires_stored_credential(),
            // Other kinds are not deeply modeled yet (their real
            // authentication shape lands with 0106/0108+); assume no stored
            // credential is required rather than guessing at a policy for a
            // protocol not yet implemented.
            Self::Ftp(_) | Self::Ftps(_) | Self::WebDav(_) | Self::S3(_) => true,
            // A OneDrive connection is only ever usable once its OAuth
            // refresh token has been stored by the authorization flow (task
            // 0110); `create`/`update` never accept one directly (see
            // `ConnectionError::Invalid` with
            // `ConnectionValidationError::OneDriveSecretNotAccepted`), so
            // "no stored credential yet" always means "not authorized yet",
            // never "this kind has no such concept".
            Self::OneDrive(_) => true,
            Self::Smb(_) => false,
        }
    }

    /// Validates this configuration's own fields, independent of the owning
    /// profile's `kind`/`name` (checked separately by
    /// [`crate::ConnectionProfile::validate`]).
    pub fn validate(&self) -> Vec<ConnectionValidationError> {
        let mut errors = Vec::new();
        match self {
            Self::Ssh(config) => {
                if config.host.trim().is_empty() {
                    errors.push(ConnectionValidationError::EmptySshHost);
                }
                if config.username.trim().is_empty() {
                    errors.push(ConnectionValidationError::EmptySshUsername);
                }
                if config.port == 0 {
                    errors.push(ConnectionValidationError::InvalidSshPort);
                }
            }
            Self::Ftp(config) | Self::Ftps(config) => {
                if config.host.trim().is_empty() {
                    errors.push(ConnectionValidationError::EmptyFtpHost);
                }
                if config.username.trim().is_empty() {
                    errors.push(ConnectionValidationError::EmptyFtpUsername);
                }
                if config.port == 0 {
                    errors.push(ConnectionValidationError::InvalidFtpPort);
                }
            }
            Self::WebDav(config) => {
                if config.base_url.trim().is_empty() {
                    errors.push(ConnectionValidationError::EmptyWebDavBaseUrl);
                }
                if config.username.trim().is_empty() {
                    errors.push(ConnectionValidationError::EmptyWebDavUsername);
                }
            }
            Self::S3(config) => {
                if config.bucket.trim().is_empty() {
                    errors.push(ConnectionValidationError::EmptyS3Bucket);
                }
                if config.access_key_id.trim().is_empty() {
                    errors.push(ConnectionValidationError::EmptyS3AccessKeyId);
                }
            }
            Self::Smb(config) => {
                if config.server.trim().is_empty() {
                    errors.push(ConnectionValidationError::EmptySmbServer);
                }
                if config.share.trim().is_empty() {
                    errors.push(ConnectionValidationError::EmptySmbShare);
                }
            }
            Self::OneDrive(_) => {}
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ssh() -> SshConnectionConfiguration {
        SshConnectionConfiguration {
            host: "example.test".to_owned(),
            port: 22,
            username: "erik".to_owned(),
            start_path: Some("/home/erik".to_owned()),
            authentication: SshAuthenticationMethod::Password,
            host_key_policy: HostKeyPolicy::PromptOnFirstUse,
            keepalive: Some(Duration::from_secs(30)),
        }
    }

    #[test]
    fn ssh_configuration_round_trips_through_serde_json() {
        let config = ConnectionConfiguration::Ssh(sample_ssh());
        let json = serde_json::to_value(&config).expect("serialization must succeed");
        assert_eq!(json["kind"], "ssh");
        assert_eq!(json["host"], "example.test");

        let parsed: ConnectionConfiguration =
            serde_json::from_value(json).expect("deserialization must succeed");
        assert_eq!(parsed, config);
    }

    #[test]
    fn kind_matches_the_active_variant() {
        assert_eq!(
            ConnectionConfiguration::Ssh(sample_ssh()).kind(),
            ConnectionKind::Ssh
        );
        assert_eq!(
            ConnectionConfiguration::Smb(SmbConnectionConfiguration {
                server: "nas.local".to_owned(),
                share: "media".to_owned(),
            })
            .kind(),
            ConnectionKind::Smb
        );
    }

    #[test]
    fn password_and_private_key_authentication_require_a_stored_credential() {
        assert!(SshAuthenticationMethod::Password.requires_stored_credential());
        assert!(SshAuthenticationMethod::PrivateKey.requires_stored_credential());
        assert!(!SshAuthenticationMethod::Agent.requires_stored_credential());
    }

    #[test]
    fn valid_ssh_configuration_has_no_validation_errors() {
        assert!(
            ConnectionConfiguration::Ssh(sample_ssh())
                .validate()
                .is_empty()
        );
    }

    #[test]
    fn ssh_configuration_reports_every_malformed_field() {
        let config = ConnectionConfiguration::Ssh(SshConnectionConfiguration {
            host: "  ".to_owned(),
            port: 0,
            username: String::new(),
            start_path: None,
            authentication: SshAuthenticationMethod::Agent,
            host_key_policy: HostKeyPolicy::PromptOnFirstUse,
            keepalive: None,
        });

        let errors = config.validate();

        assert!(errors.contains(&ConnectionValidationError::EmptySshHost));
        assert!(errors.contains(&ConnectionValidationError::EmptySshUsername));
        assert!(errors.contains(&ConnectionValidationError::InvalidSshPort));
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn ftp_configuration_validates_its_own_fields() {
        let config = ConnectionConfiguration::Ftp(FtpConnectionConfiguration {
            host: String::new(),
            port: 21,
            username: "erik".to_owned(),
            start_path: None,
        });
        assert_eq!(
            config.validate(),
            vec![ConnectionValidationError::EmptyFtpHost]
        );
    }

    fn sample_s3() -> S3ConnectionConfiguration {
        S3ConnectionConfiguration {
            bucket: "my-bucket".to_owned(),
            access_key_id: "AKIAEXAMPLE".to_owned(),
            region: Some("us-east-1".to_owned()),
            endpoint: None,
            start_path: Some("photos".to_owned()),
        }
    }

    #[test]
    fn s3_configuration_round_trips_through_serde_json() {
        let config = ConnectionConfiguration::S3(sample_s3());
        let json = serde_json::to_value(&config).expect("serialization must succeed");
        assert_eq!(json["kind"], "s3");
        assert_eq!(json["bucket"], "my-bucket");

        let parsed: ConnectionConfiguration =
            serde_json::from_value(json).expect("deserialization must succeed");
        assert_eq!(parsed, config);
    }

    #[test]
    fn valid_s3_configuration_has_no_validation_errors() {
        assert!(
            ConnectionConfiguration::S3(sample_s3())
                .validate()
                .is_empty()
        );
    }

    #[test]
    fn s3_configuration_reports_every_malformed_field() {
        let config = ConnectionConfiguration::S3(S3ConnectionConfiguration {
            bucket: String::new(),
            access_key_id: String::new(),
            region: None,
            endpoint: None,
            start_path: None,
        });

        let errors = config.validate();

        assert!(errors.contains(&ConnectionValidationError::EmptyS3Bucket));
        assert!(errors.contains(&ConnectionValidationError::EmptyS3AccessKeyId));
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn s3_configuration_requires_a_stored_credential() {
        assert!(ConnectionConfiguration::S3(sample_s3()).requires_stored_credential());
    }

    #[test]
    fn webdav_configuration_validates_its_own_fields() {
        let config = ConnectionConfiguration::WebDav(WebDavConnectionConfiguration {
            base_url: String::new(),
            username: String::new(),
            authentication: WebDavAuthenticationScheme::Basic,
            path_prefix: None,
        });
        let errors = config.validate();
        assert!(errors.contains(&ConnectionValidationError::EmptyWebDavBaseUrl));
        assert!(errors.contains(&ConnectionValidationError::EmptyWebDavUsername));
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn webdav_authentication_requires_a_stored_credential() {
        let config = ConnectionConfiguration::WebDav(WebDavConnectionConfiguration {
            base_url: "https://cloud.example.test/dav".to_owned(),
            username: "erik".to_owned(),
            authentication: WebDavAuthenticationScheme::Basic,
            path_prefix: None,
        });
        assert!(config.requires_stored_credential());
    }

    #[test]
    fn onedrive_requires_a_stored_credential_so_connect_test_report_authentication_required() {
        // Task 0110: `connect`/`test` must report `AuthenticationRequired`
        // until a OneDrive connection has completed OAuth authorization,
        // exactly like every other credentialed provider - never
        // `Connected` just because no dialer happens to be registered yet.
        assert!(
            ConnectionConfiguration::OneDrive(OneDriveConnectionConfiguration::default())
                .requires_stored_credential()
        );
    }

    #[test]
    fn smb_does_not_require_a_stored_credential() {
        assert!(
            !ConnectionConfiguration::Smb(SmbConnectionConfiguration {
                server: "nas.local".to_owned(),
                share: "media".to_owned(),
            })
            .requires_stored_credential()
        );
    }

    #[test]
    fn onedrive_configuration_has_no_validation_errors_before_authorization() {
        // Create-before-authorize (task 0110) must remain possible: an
        // unauthenticated OneDrive connection (every field `None`) is
        // structurally valid.
        assert!(
            ConnectionConfiguration::OneDrive(OneDriveConnectionConfiguration::default())
                .validate()
                .is_empty()
        );
    }

    #[test]
    fn onedrive_configuration_round_trips_every_field_through_serde_json() {
        let config = ConnectionConfiguration::OneDrive(OneDriveConnectionConfiguration {
            account_hint: Some("erik@example.test".to_owned()),
            email: Some("erik@example.test".to_owned()),
            display_name: Some("Erik Vullings".to_owned()),
            drive_type: Some(OneDriveDriveType::Business),
        });

        let json = serde_json::to_value(&config).expect("serialization must succeed");
        assert_eq!(json["kind"], "oneDrive");
        assert_eq!(json["email"], "erik@example.test");
        assert_eq!(json["display_name"], "Erik Vullings");
        assert_eq!(json["drive_type"], "Business");

        let parsed: ConnectionConfiguration =
            serde_json::from_value(json).expect("deserialization must succeed");
        assert_eq!(parsed, config);
    }

    #[test]
    fn onedrive_configuration_never_carries_a_secret_bearing_field() {
        // Defense in depth mirroring `ConnectionProfile`'s own equivalent
        // test: every field this struct can ever gain must stay non-secret,
        // since the OAuth tokens live only behind `credential_ref`.
        let config = OneDriveConnectionConfiguration {
            account_hint: Some("erik@example.test".to_owned()),
            email: Some("erik@example.test".to_owned()),
            display_name: Some("Erik Vullings".to_owned()),
            drive_type: Some(OneDriveDriveType::Personal),
        };
        let json = serde_json::to_value(config).expect("serialization must succeed");
        let object = json
            .as_object()
            .expect("configuration serializes as an object");
        for forbidden in [
            "accessToken",
            "access_token",
            "refreshToken",
            "refresh_token",
            "token",
            "secret",
        ] {
            assert!(
                !object.contains_key(forbidden),
                "OneDrive configuration JSON must not contain a {forbidden:?} field"
            );
        }
    }
}
