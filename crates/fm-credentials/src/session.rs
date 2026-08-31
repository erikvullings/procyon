//! Application-session cache in front of a protected credential store.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::{
    CredentialError, CredentialRef, CredentialStore, ResolvedCredential, StoreCredentialRequest,
};

/// Keeps resolved credentials in zeroizing process memory for the lifetime of
/// one application service, avoiding repeated OS authorization prompts.
pub struct SessionCredentialStore {
    inner: Arc<dyn CredentialStore>,
    cache: Mutex<HashMap<CredentialRef, ResolvedCredential>>,
}

impl SessionCredentialStore {
    /// Wraps a durable, platform-protected credential store with an empty cache.
    #[must_use]
    pub fn new(inner: Arc<dyn CredentialStore>) -> Self {
        Self {
            inner,
            cache: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl CredentialStore for SessionCredentialStore {
    async fn store(
        &self,
        request: StoreCredentialRequest,
    ) -> Result<CredentialRef, CredentialError> {
        let cached = ResolvedCredential {
            secret: request.secret.clone(),
        };
        let reference = self.inner.store(request).await?;
        self.cache.lock().await.insert(reference, cached);
        Ok(reference)
    }

    async fn resolve(
        &self,
        reference: &CredentialRef,
    ) -> Result<ResolvedCredential, CredentialError> {
        // Keep the lock while resolving so simultaneous first users of the
        // same credential produce one protected-store access and one prompt.
        let mut cache = self.cache.lock().await;
        if let Some(credential) = cache.get(reference) {
            return Ok(credential.clone());
        }
        let credential = self.inner.resolve(reference).await?;
        cache.insert(*reference, credential.clone());
        Ok(credential)
    }

    async fn delete(&self, reference: &CredentialRef) -> Result<(), CredentialError> {
        self.inner.delete(reference).await?;
        self.cache.lock().await.remove(reference);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::{InMemoryCredentialStore, SecretMaterial};

    use super::*;

    struct CountingStore {
        inner: InMemoryCredentialStore,
        resolves: AtomicUsize,
    }

    #[async_trait]
    impl CredentialStore for CountingStore {
        async fn store(
            &self,
            request: StoreCredentialRequest,
        ) -> Result<CredentialRef, CredentialError> {
            self.inner.store(request).await
        }

        async fn resolve(
            &self,
            reference: &CredentialRef,
        ) -> Result<ResolvedCredential, CredentialError> {
            self.resolves.fetch_add(1, Ordering::SeqCst);
            self.inner.resolve(reference).await
        }

        async fn delete(&self, reference: &CredentialRef) -> Result<(), CredentialError> {
            self.inner.delete(reference).await
        }
    }

    #[tokio::test]
    async fn repeated_resolves_access_the_protected_store_once() {
        let inner = Arc::new(CountingStore {
            inner: InMemoryCredentialStore::new(),
            resolves: AtomicUsize::new(0),
        });
        let reference = inner
            .store(StoreCredentialRequest::new(
                "server",
                SecretMaterial::password("secret"),
            ))
            .await
            .unwrap();
        let store = SessionCredentialStore::new(inner.clone());

        let (first, second) = tokio::join!(store.resolve(&reference), store.resolve(&reference));

        assert_eq!(first.unwrap(), second.unwrap());
        assert_eq!(inner.resolves.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn newly_stored_credentials_are_available_without_a_protected_store_read() {
        let inner = Arc::new(CountingStore {
            inner: InMemoryCredentialStore::new(),
            resolves: AtomicUsize::new(0),
        });
        let store = SessionCredentialStore::new(inner.clone());

        let reference = store
            .store(StoreCredentialRequest::new(
                "server",
                SecretMaterial::password("secret"),
            ))
            .await
            .unwrap();

        assert!(store.resolve(&reference).await.is_ok());
        assert_eq!(inner.resolves.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn deleting_a_credential_evicts_it_from_the_session_cache() {
        let inner = Arc::new(CountingStore {
            inner: InMemoryCredentialStore::new(),
            resolves: AtomicUsize::new(0),
        });
        let store = SessionCredentialStore::new(inner.clone());
        let reference = store
            .store(StoreCredentialRequest::new(
                "server",
                SecretMaterial::password("secret"),
            ))
            .await
            .unwrap();

        store.delete(&reference).await.unwrap();

        assert!(matches!(
            store.resolve(&reference).await,
            Err(CredentialError::NotFound { .. })
        ));
        assert_eq!(inner.resolves.load(Ordering::SeqCst), 1);
    }
}
