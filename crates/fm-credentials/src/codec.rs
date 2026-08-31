//! Binary encoding for [`SecretMaterial`], used only by [`crate::CredentialStore`]
//! implementations that must hand an opaque byte blob to a real OS-protected
//! store (a macOS Keychain generic password's data, or a Windows Credential
//! Manager credential's `CredentialBlob`).
//!
//! Never use this to log, persist outside a protected store, or return
//! secret content through an API - the encoded bytes are plaintext JSON until
//! whatever store receives them encrypts/protects them at rest.

use serde::{Deserialize, Serialize};

use crate::error::CredentialError;
use crate::secret::SecretMaterial;

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum EncodedSecret {
    Password {
        password: String,
    },
    PrivateKey {
        key: String,
        passphrase: Option<String>,
    },
    PrivateKeyPath {
        path: String,
        passphrase: Option<String>,
    },
    OAuthToken {
        access_token: String,
        refresh_token: Option<String>,
    },
    AccessKey {
        access_key_id: String,
        secret_access_key: String,
    },
}

impl From<&SecretMaterial> for EncodedSecret {
    fn from(value: &SecretMaterial) -> Self {
        match value {
            SecretMaterial::Password { password } => Self::Password {
                password: password.to_string(),
            },
            SecretMaterial::PrivateKey { key, passphrase } => Self::PrivateKey {
                key: key.to_string(),
                passphrase: passphrase.as_ref().map(|value| value.as_str().to_owned()),
            },
            SecretMaterial::PrivateKeyPath { path, passphrase } => Self::PrivateKeyPath {
                path: path.clone(),
                passphrase: passphrase.as_ref().map(|value| value.as_str().to_owned()),
            },
            SecretMaterial::OAuthToken {
                access_token,
                refresh_token,
            } => Self::OAuthToken {
                access_token: access_token.to_string(),
                refresh_token: refresh_token
                    .as_ref()
                    .map(|value| value.as_str().to_owned()),
            },
            SecretMaterial::AccessKey {
                access_key_id,
                secret_access_key,
            } => Self::AccessKey {
                access_key_id: access_key_id.clone(),
                secret_access_key: secret_access_key.to_string(),
            },
        }
    }
}

impl From<EncodedSecret> for SecretMaterial {
    fn from(value: EncodedSecret) -> Self {
        match value {
            EncodedSecret::Password { password } => Self::password(password),
            EncodedSecret::PrivateKey { key, passphrase } => Self::private_key(key, passphrase),
            EncodedSecret::PrivateKeyPath { path, passphrase } => {
                Self::private_key_path(path, passphrase)
            }
            EncodedSecret::OAuthToken {
                access_token,
                refresh_token,
            } => Self::oauth_token(access_token, refresh_token),
            EncodedSecret::AccessKey {
                access_key_id,
                secret_access_key,
            } => Self::access_key(access_key_id, secret_access_key),
        }
    }
}

/// Serializes secret material to an opaque byte blob for storage in a real
/// OS-protected credential store.
#[must_use]
pub fn encode(secret: &SecretMaterial) -> Vec<u8> {
    // `EncodedSecret` fields are plain `String`s, so encoding a value built
    // from `SecretMaterial` can never fail.
    serde_json::to_vec(&EncodedSecret::from(secret)).expect("secret material always encodes")
}

/// Deserializes secret material previously written by [`encode`].
///
/// # Errors
/// Returns [`CredentialError::Backend`] if `bytes` was not produced by
/// [`encode`] (for example the stored item was corrupted or created by an
/// incompatible version).
pub fn decode(bytes: &[u8]) -> Result<SecretMaterial, CredentialError> {
    serde_json::from_slice::<EncodedSecret>(bytes)
        .map(SecretMaterial::from)
        .map_err(|_| CredentialError::Backend("stored credential data is corrupt".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trips_through_encode_and_decode() {
        let secret = SecretMaterial::password("hunter2");
        let decoded = decode(&encode(&secret)).expect("decode must succeed");
        assert_eq!(decoded, secret);
    }

    #[test]
    fn private_key_with_passphrase_round_trips() {
        let secret = SecretMaterial::private_key("key-bytes", Some("passphrase".to_owned()));
        let decoded = decode(&encode(&secret)).expect("decode must succeed");
        assert_eq!(decoded, secret);
    }

    #[test]
    fn private_key_without_passphrase_round_trips() {
        let secret = SecretMaterial::private_key("key-bytes", None);
        let decoded = decode(&encode(&secret)).expect("decode must succeed");
        assert_eq!(decoded, secret);
    }

    #[test]
    fn private_key_path_with_passphrase_round_trips() {
        let secret =
            SecretMaterial::private_key_path("~/.ssh/id_tno", Some("passphrase".to_owned()));
        let decoded = decode(&encode(&secret)).expect("decode must succeed");
        assert_eq!(decoded, secret);
    }

    #[test]
    fn private_key_path_without_passphrase_round_trips() {
        let secret = SecretMaterial::private_key_path("~/.ssh/id_tno", None);
        let decoded = decode(&encode(&secret)).expect("decode must succeed");
        assert_eq!(decoded, secret);
    }

    #[test]
    fn oauth_token_round_trips() {
        let secret = SecretMaterial::oauth_token("access", Some("refresh".to_owned()));
        let decoded = decode(&encode(&secret)).expect("decode must succeed");
        assert_eq!(decoded, secret);
    }

    #[test]
    fn access_key_round_trips() {
        let secret = SecretMaterial::access_key("AKIAEXAMPLE", "secret");
        let decoded = decode(&encode(&secret)).expect("decode must succeed");
        assert_eq!(decoded, secret);
    }

    #[test]
    fn decode_of_garbage_bytes_reports_a_backend_error_rather_than_panicking() {
        let error = decode(b"not encoded secret material").unwrap_err();
        assert!(matches!(error, CredentialError::Backend(_)));
    }
}
