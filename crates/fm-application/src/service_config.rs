//! Construction-time configuration for [`crate::FileManagerService`].
//!
//! Replaces the former chain of positional-argument constructors
//! (`with_event_bus` -> `with_platform_adapter` -> ... -> `..._and_search_accelerator`)
//! with one value a host fills in once.

use std::path::PathBuf;
use std::sync::Arc;

use fm_credentials::{CredentialStore, InMemoryCredentialStore};
use fm_events::EventBus;
use fm_platform::{FallbackPlatformAdapter, PlatformAdapter};
use fm_search_acceleration::{SearchAcceleration, UnsupportedSearchAccelerator};
use fm_transport_dto::RuntimeKindDto;

/// Host-provided adapters and directories. Defaults are the inert ones: a fresh
/// [`EventBus`], [`FallbackPlatformAdapter`] (browser/server mode has no native access to a
/// remote client's OS; capability flags derive from the adapter, spec §21),
/// [`InMemoryCredentialStore`] (real hosts must supply Keychain/Credential Manager storage)
/// and [`UnsupportedSearchAccelerator`] (index search is a narrow optional optimisation, not
/// part of [`PlatformAdapter`]).
pub struct ServiceConfig {
    pub(crate) runtime: RuntimeKindDto,
    pub(crate) workspace_directory: PathBuf,
    pub(crate) settings_directory: PathBuf,
    pub(crate) events: EventBus,
    pub(crate) platform: Arc<dyn PlatformAdapter>,
    pub(crate) credential_store: Arc<dyn CredentialStore>,
    pub(crate) search_accelerator: Arc<dyn SearchAcceleration>,
}

impl ServiceConfig {
    /// Starts from the inert defaults for `runtime` and the two persistence directories.
    pub fn new(
        runtime: RuntimeKindDto,
        workspace_directory: impl Into<PathBuf>,
        settings_directory: impl Into<PathBuf>,
    ) -> Self {
        Self {
            runtime,
            workspace_directory: workspace_directory.into(),
            settings_directory: settings_directory.into(),
            events: EventBus::default(),
            platform: Arc::new(FallbackPlatformAdapter),
            credential_store: Arc::new(InMemoryCredentialStore::new()),
            search_accelerator: Arc::new(UnsupportedSearchAccelerator),
        }
    }

    /// Replaces the event bus (share one bus with the host's own subscribers).
    #[must_use]
    pub fn events(mut self, events: EventBus) -> Self {
        self.events = events;
        self
    }

    /// Replaces the platform adapter that supplies native integration and capability flags.
    #[must_use]
    pub fn platform(mut self, platform: Arc<dyn PlatformAdapter>) -> Self {
        self.platform = platform;
        self
    }

    /// Replaces the credential store; real hosts pass OS-protected storage.
    #[must_use]
    pub fn credential_store(mut self, credential_store: Arc<dyn CredentialStore>) -> Self {
        self.credential_store = credential_store;
        self
    }

    /// Replaces the native index-search accelerator.
    #[must_use]
    pub fn search_accelerator(mut self, search_accelerator: Arc<dyn SearchAcceleration>) -> Self {
        self.search_accelerator = search_accelerator;
        self
    }
}
