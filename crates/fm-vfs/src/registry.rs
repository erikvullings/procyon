use std::collections::HashMap;
use std::sync::Arc;

use fm_domain::{Location, ProviderId};

use crate::{FileSystemProvider, VfsError};

/// Collection of providers addressable by [`Location::provider_id`].
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: HashMap<ProviderId, Arc<dyn FileSystemProvider>>,
}

impl ProviderRegistry {
    /// Creates an empty provider registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a provider, replacing an existing provider with the same id.
    pub fn register(&mut self, provider: Arc<dyn FileSystemProvider>) {
        self.providers.insert(provider.id(), provider);
    }

    /// Resolves the provider owning a location.
    pub fn resolve(&self, location: &Location) -> Result<Arc<dyn FileSystemProvider>, VfsError> {
        self.providers
            .get(&location.provider_id)
            .cloned()
            .ok_or_else(|| VfsError::UnknownProvider {
                provider_id: location.provider_id.clone(),
            })
    }
}
