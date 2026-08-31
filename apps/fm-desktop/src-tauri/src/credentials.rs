//! Selects the concrete [`CredentialStore`] this host injects into
//! [`fm_application::FileManagerService`] (task 0103).
//!
//! Mirrors `platform.rs`'s [`fm_platform::PlatformAdapter`] selection
//! exactly: `fm-application` is target-agnostic and must not depend on every
//! OS-specific credential-store crate just to pick one at runtime.

use std::sync::Arc;

use fm_credentials::CredentialStore;

/// Builds the credential store for the current desktop build target.
///
/// macOS builds get the real [`fm_credentials_macos::MacosCredentialStore`]
/// (Keychain); Windows builds get [`fm_credentials_windows::WindowsCredentialStore`]
/// (Credential Manager); any other target falls back to
/// [`fm_credentials::InMemoryCredentialStore`], which is explicitly not
/// protected storage (see that type's documentation).
#[must_use]
pub(crate) fn build_credential_store() -> Arc<dyn CredentialStore> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(fm_credentials_macos::MacosCredentialStore::new())
    }
    #[cfg(target_os = "windows")]
    {
        Arc::new(fm_credentials_windows::WindowsCredentialStore::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Arc::new(fm_credentials::InMemoryCredentialStore::new())
    }
}
