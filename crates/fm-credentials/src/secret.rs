//! [`SecretMaterial`]: the tagged set of secret shapes a [`crate::CredentialStore`]
//! can hold (task 0103, spec §5.3, §19). Never a generic string/bytes map -
//! callers state which kind of secret they mean.

use std::fmt;

use zeroize::Zeroizing;

/// Secret content held by a [`crate::CredentialStore`], tagged by shape.
///
/// Every string field is wrapped in [`Zeroizing`] so the plaintext is wiped
/// from memory as soon as the value is dropped. [`fmt::Debug`] is implemented
/// by hand below and never prints secret content, so an accidental
/// `{:?}`-formatted log line cannot leak a password, key or token (spec §19
/// "no secret logging").
#[derive(Clone)]
pub enum SecretMaterial {
    /// A plain password, for example SSH or FTP password authentication.
    Password {
        /// The password itself.
        password: Zeroizing<String>,
    },
    /// A private key, optionally protected by a passphrase.
    PrivateKey {
        /// PEM (or provider-specific) private key content.
        key: Zeroizing<String>,
        /// Passphrase protecting `key`, if any.
        passphrase: Option<Zeroizing<String>>,
    },
    /// A private key referenced by its filesystem path rather than pasted
    /// content, read fresh from disk at dial time - matching how `ssh`
    /// itself uses an `IdentityFile`, and avoiding storing key bytes at rest
    /// at all. The path is not secret (only the file's content and the
    /// passphrase are); it is kept as a plain `String` rather than
    /// `Zeroizing` for that reason.
    PrivateKeyPath {
        /// Absolute or `~`-relative path to a PEM/OpenSSH private key file.
        path: String,
        /// Passphrase protecting the key file, if any.
        passphrase: Option<Zeroizing<String>>,
    },
    /// An OAuth token pair, for example a future native OneDrive connection.
    OAuthToken {
        /// The current access token.
        access_token: Zeroizing<String>,
        /// The refresh token, if the provider issued one.
        refresh_token: Option<Zeroizing<String>>,
    },
    /// An S3-style access key pair (task 0146). The access key id is not
    /// secret (compare `PrivateKeyPath::path`) - kept as a plain `String` -
    /// but is stored alongside the secret access key so the credential store
    /// holds the whole pair the request signer needs.
    AccessKey {
        /// The access key id.
        access_key_id: String,
        /// The secret access key.
        secret_access_key: Zeroizing<String>,
    },
}

const REDACTED: &str = "<redacted>";

impl fmt::Debug for SecretMaterial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password { .. } => f
                .debug_struct("Password")
                .field("password", &REDACTED)
                .finish(),
            Self::PrivateKey { .. } => f
                .debug_struct("PrivateKey")
                .field("key", &REDACTED)
                .field("passphrase", &REDACTED)
                .finish(),
            Self::PrivateKeyPath { path, .. } => f
                .debug_struct("PrivateKeyPath")
                .field("path", path)
                .field("passphrase", &REDACTED)
                .finish(),
            Self::OAuthToken { .. } => f
                .debug_struct("OAuthToken")
                .field("access_token", &REDACTED)
                .field("refresh_token", &REDACTED)
                .finish(),
            Self::AccessKey { access_key_id, .. } => f
                .debug_struct("AccessKey")
                .field("access_key_id", access_key_id)
                .field("secret_access_key", &REDACTED)
                .finish(),
        }
    }
}

impl PartialEq for SecretMaterial {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Password { password: a }, Self::Password { password: b }) => {
                a.as_str() == b.as_str()
            }
            (
                Self::PrivateKey {
                    key: key_a,
                    passphrase: passphrase_a,
                },
                Self::PrivateKey {
                    key: key_b,
                    passphrase: passphrase_b,
                },
            ) => {
                key_a.as_str() == key_b.as_str()
                    && passphrase_a.as_deref() == passphrase_b.as_deref()
            }
            (
                Self::PrivateKeyPath {
                    path: path_a,
                    passphrase: passphrase_a,
                },
                Self::PrivateKeyPath {
                    path: path_b,
                    passphrase: passphrase_b,
                },
            ) => path_a == path_b && passphrase_a.as_deref() == passphrase_b.as_deref(),
            (
                Self::OAuthToken {
                    access_token: access_a,
                    refresh_token: refresh_a,
                },
                Self::OAuthToken {
                    access_token: access_b,
                    refresh_token: refresh_b,
                },
            ) => {
                access_a.as_str() == access_b.as_str()
                    && refresh_a.as_deref() == refresh_b.as_deref()
            }
            (
                Self::AccessKey {
                    access_key_id: id_a,
                    secret_access_key: secret_a,
                },
                Self::AccessKey {
                    access_key_id: id_b,
                    secret_access_key: secret_b,
                },
            ) => id_a == id_b && secret_a.as_str() == secret_b.as_str(),
            _ => false,
        }
    }
}

impl SecretMaterial {
    /// Builds password secret material from an owned string.
    #[must_use]
    pub fn password(password: impl Into<String>) -> Self {
        Self::Password {
            password: Zeroizing::new(password.into()),
        }
    }

    /// Builds private-key secret material from owned strings.
    #[must_use]
    pub fn private_key(key: impl Into<String>, passphrase: Option<String>) -> Self {
        Self::PrivateKey {
            key: Zeroizing::new(key.into()),
            passphrase: passphrase.map(Zeroizing::new),
        }
    }

    /// Builds private-key-path secret material: a reference to a key file on
    /// disk, resolved fresh at dial time rather than stored at rest.
    #[must_use]
    pub fn private_key_path(path: impl Into<String>, passphrase: Option<String>) -> Self {
        Self::PrivateKeyPath {
            path: path.into(),
            passphrase: passphrase.map(Zeroizing::new),
        }
    }

    /// Builds OAuth token secret material from owned strings.
    #[must_use]
    pub fn oauth_token(access_token: impl Into<String>, refresh_token: Option<String>) -> Self {
        Self::OAuthToken {
            access_token: Zeroizing::new(access_token.into()),
            refresh_token: refresh_token.map(Zeroizing::new),
        }
    }

    /// Builds S3-style access key secret material from owned strings.
    #[must_use]
    pub fn access_key(
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        Self::AccessKey {
            access_key_id: access_key_id.into(),
            secret_access_key: Zeroizing::new(secret_access_key.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_contains_the_password() {
        let secret = SecretMaterial::password("hunter2");
        let formatted = format!("{secret:?}");
        assert!(!formatted.contains("hunter2"));
        assert!(formatted.contains(REDACTED));
    }

    #[test]
    fn debug_output_never_contains_the_private_key_or_passphrase() {
        let secret = SecretMaterial::private_key(
            "-----BEGIN PRIVATE KEY-----secret-----END PRIVATE KEY-----",
            Some("swordfish".to_owned()),
        );
        let formatted = format!("{secret:?}");
        assert!(!formatted.contains("BEGIN PRIVATE KEY"));
        assert!(!formatted.contains("swordfish"));
    }

    #[test]
    fn debug_output_shows_the_path_but_never_the_passphrase() {
        let secret =
            SecretMaterial::private_key_path("~/.ssh/id_tno", Some("swordfish".to_owned()));
        let formatted = format!("{secret:?}");
        assert!(formatted.contains("~/.ssh/id_tno"));
        assert!(!formatted.contains("swordfish"));
        assert!(formatted.contains(REDACTED));
    }

    #[test]
    fn debug_output_never_contains_oauth_tokens() {
        let secret =
            SecretMaterial::oauth_token("access-abc123", Some("refresh-xyz789".to_owned()));
        let formatted = format!("{secret:?}");
        assert!(!formatted.contains("access-abc123"));
        assert!(!formatted.contains("refresh-xyz789"));
    }

    #[test]
    fn debug_output_shows_the_access_key_id_but_never_the_secret_key() {
        let secret = SecretMaterial::access_key("AKIAEXAMPLE", "shhh-its-secret");
        let formatted = format!("{secret:?}");
        assert!(formatted.contains("AKIAEXAMPLE"));
        assert!(!formatted.contains("shhh-its-secret"));
        assert!(formatted.contains(REDACTED));
    }

    #[test]
    fn equal_secrets_of_the_same_shape_compare_equal() {
        assert_eq!(
            SecretMaterial::password("hunter2"),
            SecretMaterial::password("hunter2")
        );
    }

    #[test]
    fn secrets_of_different_shapes_never_compare_equal() {
        assert_ne!(
            SecretMaterial::password("hunter2"),
            SecretMaterial::oauth_token("hunter2", None)
        );
        assert_ne!(
            SecretMaterial::password("hunter2"),
            SecretMaterial::access_key("hunter2", "hunter2")
        );
    }

    #[test]
    fn equal_access_keys_compare_equal() {
        assert_eq!(
            SecretMaterial::access_key("AKIAEXAMPLE", "secret"),
            SecretMaterial::access_key("AKIAEXAMPLE", "secret")
        );
        assert_ne!(
            SecretMaterial::access_key("AKIAEXAMPLE", "secret"),
            SecretMaterial::access_key("AKIAOTHER", "secret")
        );
    }
}
