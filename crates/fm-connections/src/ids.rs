//! [`ConnectionId`]: the identifier for a [`crate::ConnectionProfile`] (task
//! 0103).
//!
//! Kept in this crate rather than `fm-domain`: nothing outside the
//! connections feature needs it yet. A future SFTP `Location` embeds a
//! connection id as a string in its URI (spec §6.5's
//! `sftp://<connection-id>/home/erik`), not as a structured cross-crate type.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Error returned when a [`ConnectionId`] fails to parse from text.
#[derive(Debug, thiserror::Error)]
#[error("invalid connection identifier: {0}")]
pub struct ConnectionIdParseError(#[from] uuid::Error);

/// Identifies a [`crate::ConnectionProfile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConnectionId(Uuid);

impl ConnectionId {
    /// Generates a new, randomly chosen identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Returns the underlying UUID value.
    #[must_use]
    pub fn into_inner(self) -> Uuid {
        self.0
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for ConnectionId {
    type Err = ConnectionIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::from_str(s)?))
    }
}

impl From<Uuid> for ConnectionId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<ConnectionId> for Uuid {
    fn from(value: ConnectionId) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn round_trips_through_display_and_from_str() {
        let id = ConnectionId::new();
        let parsed = ConnectionId::from_str(&id.to_string()).expect("valid uuid text must parse");
        assert_eq!(id, parsed);
    }

    #[test]
    fn round_trips_through_serde_json() {
        let id = ConnectionId::new();
        let json = serde_json::to_string(&id).expect("serialization must succeed");
        let parsed: ConnectionId =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(id, parsed);
    }

    #[test]
    fn rejects_invalid_text() {
        assert!(ConnectionId::from_str("not-a-uuid").is_err());
    }

    #[test]
    fn distinct_ids_are_not_equal() {
        assert_ne!(ConnectionId::new(), ConnectionId::new());
    }
}
