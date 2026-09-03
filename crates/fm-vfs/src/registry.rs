use std::collections::HashMap;
use std::sync::Arc;

use fm_domain::{Location, ProviderId};

use crate::{FileSystemProvider, VfsError};

/// Collection of providers addressable by [`Location::provider_id`].
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: HashMap<ProviderId, Arc<dyn FileSystemProvider>>,
    schemes: HashMap<String, ProviderId>,
}

impl ProviderRegistry {
    /// Creates an empty provider registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a provider, replacing an existing provider with the same id.
    pub fn register(&mut self, provider: Arc<dyn FileSystemProvider>) {
        let provider_id = provider.id();
        self.schemes.retain(|_, owner| owner != &provider_id);
        let declared_schemes = provider.schemes();
        if declared_schemes.is_empty() {
            self.register_scheme(provider_id.as_str(), &provider_id);
        } else {
            for scheme in declared_schemes {
                self.register_scheme(scheme, &provider_id);
            }
        }
        self.providers.insert(provider_id, provider);
    }

    /// Parses an opaque URI and validates it with the provider registered for
    /// its scheme.
    pub fn parse(&self, uri: &str) -> Result<Location, VfsError> {
        let parsed = Location::parse(uri).map_err(|_| VfsError::InvalidLocation {
            location: uri.to_owned(),
        })?;
        let scheme = parsed
            .scheme()
            .map_err(|_| VfsError::InvalidLocation {
                location: uri.to_owned(),
            })?
            .to_owned();
        let provider_id =
            self.schemes
                .get(&scheme)
                .cloned()
                .ok_or_else(|| VfsError::UnknownProvider {
                    provider_id: ProviderId::new(scheme),
                })?;
        let location = Location::new(provider_id.clone(), parsed.uri);
        let provider =
            self.providers
                .get(&provider_id)
                .ok_or_else(|| VfsError::UnknownProvider {
                    provider_id: provider_id.clone(),
                })?;
        provider.validate_location(&location)?;
        Ok(location)
    }

    /// Resolves the provider owning a location.
    pub fn resolve(&self, location: &Location) -> Result<Arc<dyn FileSystemProvider>, VfsError> {
        let provider = self
            .providers
            .get(&location.provider_id)
            .cloned()
            .ok_or_else(|| VfsError::UnknownProvider {
                provider_id: location.provider_id.clone(),
            })?;
        provider.validate_location(location)?;
        Ok(provider)
    }

    fn register_scheme(&mut self, scheme: &str, provider_id: &ProviderId) {
        if let Some(existing) = self.schemes.get(scheme) {
            assert_eq!(
                existing, provider_id,
                "URI scheme `{scheme}` is already registered by provider `{existing}`"
            );
        }
        self.schemes.insert(scheme.to_owned(), provider_id.clone());
    }
}
