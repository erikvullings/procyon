//! An in-memory [`CredentialStore`] (task 0103).
//!
//! Not durable across process restarts and not protected by the OS - this is
//! a fallback for hosts with no native secure store (any OS other than
//! macOS/Windows, until such a store is added) and for tests. Production
//! hosts on macOS/Windows must select `fm-credentials-macos`/
//! `fm-credentials-windows` instead; see each host binary's credential-store
//! selection module.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::CredentialError;
use crate::ids::CredentialRef;
use crate::store::{CredentialStore, ResolvedCredential, StoreCredentialRequest};

/// An in-memory credential store, guarded by a [`Mutex`] so it can be shared
/// across async tasks.
#[derive(Debug, Default)]
pub struct InMemoryCredentialStore {
    secrets: Mutex<HashMap<CredentialRef, crate::secret::SecretMaterial>>,
}

impl InMemoryCredentialStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl CredentialStore for InMemoryCredentialStore {
    async fn store(
        &self,
        request: StoreCredentialRequest,
    ) -> Result<CredentialRef, CredentialError> {
        let reference = CredentialRef::new();
        self.secrets
            .lock()
            .expect("credential lock poisoned")
            .insert(reference, request.secret);
        Ok(reference)
    }

    async fn resolve(
        &self,
        reference: &CredentialRef,
    ) -> Result<ResolvedCredential, CredentialError> {
        self.secrets
            .lock()
            .expect("credential lock poisoned")
            .get(reference)
            .cloned()
            .map(|secret| ResolvedCredential { secret })
            .ok_or(CredentialError::NotFound {
                reference: *reference,
            })
    }

    async fn delete(&self, reference: &CredentialRef) -> Result<(), CredentialError> {
        let mut secrets = self.secrets.lock().expect("credential lock poisoned");
        if secrets.remove(reference).is_some() {
            Ok(())
        } else {
            Err(CredentialError::NotFound {
                reference: *reference,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::SecretMaterial;

    #[tokio::test]
    async fn store_then_resolve_round_trips_the_secret() {
        let store = InMemoryCredentialStore::new();
        let secret = SecretMaterial::password("hunter2");

        let reference = store
            .store(StoreCredentialRequest::new("Home Server", secret.clone()))
            .await
            .expect("store must succeed");
        let resolved = store
            .resolve(&reference)
            .await
            .expect("resolve must succeed");

        assert_eq!(resolved.secret, secret);
    }

    #[tokio::test]
    async fn resolve_of_an_unknown_reference_reports_not_found() {
        let store = InMemoryCredentialStore::new();
        let reference = CredentialRef::new();

        let error = store.resolve(&reference).await.unwrap_err();

        assert_eq!(error, CredentialError::NotFound { reference });
    }

    #[tokio::test]
    async fn delete_then_resolve_reports_not_found() {
        let store = InMemoryCredentialStore::new();
        let reference = store
            .store(StoreCredentialRequest::new(
                "Home Server",
                SecretMaterial::password("hunter2"),
            ))
            .await
            .expect("store must succeed");

        store.delete(&reference).await.expect("delete must succeed");

        let error = store.resolve(&reference).await.unwrap_err();
        assert_eq!(error, CredentialError::NotFound { reference });
    }

    #[tokio::test]
    async fn delete_of_an_unknown_reference_reports_not_found() {
        let store = InMemoryCredentialStore::new();
        let reference = CredentialRef::new();

        let error = store.delete(&reference).await.unwrap_err();

        assert_eq!(error, CredentialError::NotFound { reference });
    }

    #[tokio::test]
    async fn distinct_store_calls_never_reuse_a_reference() {
        let store = InMemoryCredentialStore::new();
        let first = store
            .store(StoreCredentialRequest::new(
                "Home Server",
                SecretMaterial::password("first"),
            ))
            .await
            .unwrap();
        let second = store
            .store(StoreCredentialRequest::new(
                "NAS",
                SecretMaterial::password("second"),
            ))
            .await
            .unwrap();

        assert_ne!(first, second);
        assert_eq!(
            store.resolve(&first).await.unwrap().secret,
            SecretMaterial::password("first")
        );
        assert_eq!(
            store.resolve(&second).await.unwrap().secret,
            SecretMaterial::password("second")
        );
    }
}
