//! The [`CredentialStore`] abstraction (task 0103, spec §5.3).

use async_trait::async_trait;

use crate::error::CredentialError;
use crate::ids::CredentialRef;
use crate::secret::SecretMaterial;

/// A request to store new secret material.
#[derive(Debug, Clone)]
pub struct StoreCredentialRequest {
    /// A human-readable label shown by the underlying OS store's own UI
    /// (for example Keychain Access), not by this application. Never secret
    /// itself.
    pub label: String,
    /// The secret content to store.
    pub secret: SecretMaterial,
}

impl StoreCredentialRequest {
    /// Builds a request from a label and secret.
    #[must_use]
    pub fn new(label: impl Into<String>, secret: SecretMaterial) -> Self {
        Self {
            label: label.into(),
            secret,
        }
    }
}

/// Secret material resolved back from a [`CredentialRef`].
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCredential {
    /// The resolved secret content.
    pub secret: SecretMaterial,
}

/// Secure storage for secret material referenced by opaque
/// [`CredentialRef`]s, so a connection profile or any other persisted
/// document never has to hold a password, passphrase or token directly (spec
/// §5.3, §19).
///
/// macOS and Windows implementations back onto the platform's protected
/// credential store (Keychain, Credential Manager); [`crate::InMemoryCredentialStore`]
/// is a fallback for unsupported hosts and tests only - see its own
/// documentation before using it in a production host.
#[async_trait]
pub trait CredentialStore: Send + Sync {
    /// Stores new secret material, returning an opaque reference to it.
    async fn store(
        &self,
        request: StoreCredentialRequest,
    ) -> Result<CredentialRef, CredentialError>;

    /// Resolves a previously stored reference back into its secret material.
    async fn resolve(
        &self,
        reference: &CredentialRef,
    ) -> Result<ResolvedCredential, CredentialError>;

    /// Deletes stored secret material. A reference that no longer exists
    /// (including one already deleted) is reported as
    /// [`CredentialError::NotFound`], matching [`CredentialStore::resolve`]'s
    /// behaviour for the same reference.
    async fn delete(&self, reference: &CredentialRef) -> Result<(), CredentialError>;
}
