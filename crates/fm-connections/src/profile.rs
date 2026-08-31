//! [`ConnectionProfile`] (task 0103, spec §5.2).

use chrono::{DateTime, Utc};
use fm_credentials::CredentialRef;
use serde::{Deserialize, Serialize};

use crate::configuration::ConnectionConfiguration;
use crate::error::ConnectionValidationError;
use crate::ids::ConnectionId;
use crate::kind::ConnectionKind;

/// A saved connection profile: describes and authenticates an
/// application-managed endpoint/account (spec §14).
///
/// Never carries secret content. `credential_ref` is an opaque, non-secret
/// handle into a [`fm_credentials::CredentialStore`]; the acceptance
/// criterion "profiles never persist passwords, passphrases, OAuth tokens or
/// similar secrets" holds structurally, since no field here can hold one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionProfile {
    /// Stable identifier for this connection.
    pub id: ConnectionId,
    /// User-facing display name.
    pub name: String,
    /// The protocol/provider family, kept in sync with `configuration`'s own
    /// kind (checked by [`ConnectionProfile::validate`]).
    pub kind: ConnectionKind,
    /// Protocol-specific configuration.
    pub configuration: ConnectionConfiguration,
    /// Opaque reference to this connection's stored secret, if any (for
    /// example [`crate::configuration::SshAuthenticationMethod::Agent`]
    /// needs none).
    pub credential_ref: Option<CredentialRef>,
    /// When this connection was first created.
    pub created_at: DateTime<Utc>,
    /// When this connection was last persisted.
    pub updated_at: DateTime<Utc>,
}

impl ConnectionProfile {
    /// Validates this profile's structural invariants: a non-empty name, and
    /// `kind` matching `configuration`'s own kind, plus whatever
    /// [`ConnectionConfiguration::validate`] reports for the active variant.
    #[must_use]
    pub fn validate(&self) -> Vec<ConnectionValidationError> {
        let mut errors = Vec::new();
        if self.name.trim().is_empty() {
            errors.push(ConnectionValidationError::EmptyName);
        }
        let configuration_kind = self.configuration.kind();
        if configuration_kind != self.kind {
            errors.push(ConnectionValidationError::KindMismatch {
                kind: self.kind,
                configuration_kind,
            });
        }
        errors.extend(self.configuration.validate());
        errors
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::configuration::{
        HostKeyPolicy, SshAuthenticationMethod, SshConnectionConfiguration,
    };

    fn sample() -> ConnectionProfile {
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
                keepalive: Some(Duration::from_secs(30)),
            }),
            credential_ref: Some(CredentialRef::new()),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn a_well_formed_profile_has_no_validation_errors() {
        assert!(sample().validate().is_empty());
    }

    #[test]
    fn an_empty_name_is_reported() {
        let mut profile = sample();
        profile.name = "   ".to_owned();
        assert_eq!(
            profile.validate(),
            vec![ConnectionValidationError::EmptyName]
        );
    }

    #[test]
    fn a_kind_mismatch_with_the_configuration_is_reported() {
        let mut profile = sample();
        profile.kind = ConnectionKind::Ftp;

        let errors = profile.validate();

        assert_eq!(
            errors,
            vec![ConnectionValidationError::KindMismatch {
                kind: ConnectionKind::Ftp,
                configuration_kind: ConnectionKind::Ssh,
            }]
        );
    }

    #[test]
    fn profile_serializes_without_any_secret_bearing_field() {
        // Defense in depth beyond "the struct has no such field": assert the
        // serialized JSON never contains a key that could plausibly hold
        // secret material, so a future field addition that reintroduces one
        // is caught here too.
        let json = serde_json::to_value(sample()).expect("serialization must succeed");
        let object = json.as_object().expect("profile serializes as an object");
        for forbidden in [
            "password",
            "passphrase",
            "secret",
            "token",
            "privateKey",
            "apiKey",
        ] {
            assert!(
                !object.contains_key(forbidden),
                "profile JSON must not contain a {forbidden:?} field"
            );
        }
        // The only credential-related field is the opaque reference.
        assert!(object.contains_key("credentialRef") || object.contains_key("credential_ref"));
    }
}
