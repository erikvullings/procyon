//! The [`ConnectionRepository`] persistence abstraction (task 0103), mirroring
//! `fm-application`'s `WorkspaceRepository` shape.

use async_trait::async_trait;

use crate::error::ConnectionError;
use crate::ids::ConnectionId;
use crate::profile::ConnectionProfile;

/// Persists and retrieves [`ConnectionProfile`]s.
///
/// Every method returns a typed [`ConnectionError`] rather than panicking or
/// leaking a storage-specific error type. `save` always returns the
/// persisted copy with `updated_at` refreshed and, for an existing
/// connection, `created_at` preserved from what was already stored -
/// regardless of what the caller passed in.
#[async_trait]
pub trait ConnectionRepository: Send + Sync {
    /// Lists every stored connection profile.
    async fn list(&self) -> Result<Vec<ConnectionProfile>, ConnectionError>;

    /// Loads a single connection profile, or `None` if it does not exist.
    async fn load(&self, id: ConnectionId) -> Result<Option<ConnectionProfile>, ConnectionError>;

    /// Persists a connection profile, returning the persisted copy.
    async fn save(&self, profile: &ConnectionProfile)
    -> Result<ConnectionProfile, ConnectionError>;

    /// Deletes a connection profile. Deleting an id that does not exist
    /// reports [`ConnectionError::NotFound`].
    async fn delete(&self, id: ConnectionId) -> Result<(), ConnectionError>;
}
