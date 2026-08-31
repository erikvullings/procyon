//! macOS Keychain implementation of [`fm_credentials::CredentialStore`] (task
//! 0103).
//!
//! The crate is a workspace member on every OS but compiles to nothing off
//! macOS (see `docs/decisions/0010-native-platform-adapters.md`, mirroring
//! `fm-platform-macos`/`fm-platform-windows`).

#![cfg(target_os = "macos")]

use async_trait::async_trait;
use fm_credentials::{
    CredentialError, CredentialRef, CredentialStore, ResolvedCredential, StoreCredentialRequest,
    codec,
};
use security_framework::base::Error as KeychainError;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

/// Keychain "service" every credential in this application is stored under.
/// The "account" is the credential's [`CredentialRef`] (a random UUID), so
/// entries from different connections never collide and one credential's
/// secret cannot be looked up without already knowing its reference.
const KEYCHAIN_SERVICE: &str = "dev.fm.credentials";

/// OSStatus `errSecItemNotFound` (Security framework `SecBase.h`): no item
/// matches the given service/account. `security-framework`'s public API does
/// not re-export this constant, so the stable numeric value is used directly
/// rather than adding `security-framework-sys` as an extra dependency for
/// one constant.
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// [`CredentialStore`] backed by the macOS Keychain's generic password items
/// (spec §5.3, §19; task 0103's acceptance criterion "macOS uses Keychain or
/// equivalent protected storage").
#[derive(Debug, Default, Clone, Copy)]
pub struct MacosCredentialStore;

impl MacosCredentialStore {
    /// Creates a store backed by the current user's login Keychain.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

async fn run_blocking<T, F>(f: F) -> Result<T, CredentialError>
where
    F: FnOnce() -> Result<T, CredentialError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f).await.unwrap_or_else(|_| {
        Err(CredentialError::Backend(
            "keychain task panicked".to_owned(),
        ))
    })
}

fn map_write_error(error: KeychainError) -> CredentialError {
    CredentialError::Backend(error.to_string())
}

fn map_lookup_error(error: KeychainError, reference: CredentialRef) -> CredentialError {
    if error.code() == ERR_SEC_ITEM_NOT_FOUND {
        CredentialError::NotFound { reference }
    } else {
        CredentialError::Backend(error.to_string())
    }
}

#[async_trait]
impl CredentialStore for MacosCredentialStore {
    async fn store(
        &self,
        request: StoreCredentialRequest,
    ) -> Result<CredentialRef, CredentialError> {
        let reference = CredentialRef::new();
        let account = reference.to_string();
        let bytes = codec::encode(&request.secret);
        run_blocking(move || {
            set_generic_password(KEYCHAIN_SERVICE, &account, &bytes).map_err(map_write_error)
        })
        .await?;
        Ok(reference)
    }

    async fn resolve(
        &self,
        reference: &CredentialRef,
    ) -> Result<ResolvedCredential, CredentialError> {
        let account = reference.to_string();
        let reference = *reference;
        let bytes = run_blocking(move || {
            get_generic_password(KEYCHAIN_SERVICE, &account)
                .map_err(|error| map_lookup_error(error, reference))
        })
        .await?;
        let secret = codec::decode(&bytes)?;
        Ok(ResolvedCredential { secret })
    }

    async fn delete(&self, reference: &CredentialRef) -> Result<(), CredentialError> {
        let account = reference.to_string();
        let reference = *reference;
        run_blocking(move || {
            delete_generic_password(KEYCHAIN_SERVICE, &account)
                .map_err(|error| map_lookup_error(error, reference))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use fm_credentials::SecretMaterial;

    use super::*;

    #[tokio::test]
    async fn store_then_resolve_round_trips_through_the_real_keychain() {
        let store = MacosCredentialStore::new();
        let secret = SecretMaterial::password("fm-credentials-macos-test-password");

        let reference = store
            .store(StoreCredentialRequest::new(
                "fm-credentials-macos test",
                secret.clone(),
            ))
            .await
            .expect("store must succeed");

        let resolved = store.resolve(&reference).await;
        // Clean up before asserting, so a failed assertion never leaves a
        // test credential behind in the developer's real login keychain.
        store.delete(&reference).await.expect("delete must succeed");

        assert_eq!(resolved.expect("resolve must succeed").secret, secret);
    }

    #[tokio::test]
    async fn delete_then_resolve_reports_not_found() {
        let store = MacosCredentialStore::new();
        let reference = store
            .store(StoreCredentialRequest::new(
                "fm-credentials-macos test",
                SecretMaterial::password("fm-credentials-macos-test-password"),
            ))
            .await
            .expect("store must succeed");

        store.delete(&reference).await.expect("delete must succeed");

        let error = store.resolve(&reference).await.unwrap_err();
        assert_eq!(error, CredentialError::NotFound { reference });
    }

    #[tokio::test]
    async fn resolve_of_an_unknown_reference_reports_not_found() {
        let store = MacosCredentialStore::new();
        let reference = CredentialRef::new();

        let error = store.resolve(&reference).await.unwrap_err();

        assert_eq!(error, CredentialError::NotFound { reference });
    }
}
