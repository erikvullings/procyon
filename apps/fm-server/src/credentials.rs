//! Selects the concrete [`CredentialStore`] this host injects into
//! [`fm_application::FileManagerService`] (task 0103).
//!
//! Mirrors `apps/fm-desktop/src-tauri/src/platform.rs`'s reasoning for
//! [`fm_platform::PlatformAdapter`] selection: `fm-application` is
//! target-agnostic and must not depend on every OS-specific credential-store
//! crate just to pick one at runtime, so the `#[cfg(target_os = ...)]`
//! branch lives here in the host binary instead.
//!
//! Like the platform adapter, credential storage is local to wherever this
//! server process itself runs, so this host selects a real
//! Keychain/Credential Manager-backed store on macOS/Windows, matching task
//! 0103's acceptance criteria. Any other host OS falls back to
//! [`fm_credentials::InMemoryCredentialStore`], which is explicitly not
//! protected storage (see that type's documentation).

use std::sync::Arc;

use fm_credentials::CredentialStore;

/// Builds the credential store for the current server build target.
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
