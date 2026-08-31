//! Strongly typed identifiers used throughout the domain model (spec §5).
//!
//! UUID-backed identifiers (`WorkspaceId`, `PaneId`, `TabId`, `EntryId`,
//! `OperationId`) are opaque and generated locally. String-backed identifiers
//! (`ProviderId`, `PluginId`, `ActionId`) name externally defined concepts
//! (a provider scheme, a plugin manifest, a registered action) and are never
//! generated randomly.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Error returned when a UUID-backed identifier fails to parse from text.
#[derive(Debug, thiserror::Error)]
#[error("invalid identifier: {0}")]
pub struct IdParseError(#[from] uuid::Error);

/// Defines a UUID-backed newtype identifier with `Display`, `FromStr` and
/// serde support.
macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
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

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = IdParseError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(Uuid::from_str(s)?))
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

/// Defines a `String`-backed newtype identifier with `Display`, an infallible
/// `FromStr` and serde support.
macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates an identifier from any string-like value.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Returns the identifier as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = std::convert::Infallible;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(s.to_owned()))
            }
        }
    };
}

uuid_id! {
    /// Identifies a [`crate::Workspace`].
    WorkspaceId
}

uuid_id! {
    /// Identifies a [`crate::PaneState`] within a workspace.
    PaneId
}

uuid_id! {
    /// Identifies a [`crate::TabState`] within a pane.
    TabId
}

uuid_id! {
    /// Identifies an [`crate::EntrySummary`] (a file, directory or symlink).
    EntryId
}

uuid_id! {
    /// Identifies a long-running file operation (copy, move, delete, ...).
    OperationId
}

string_id! {
    /// Identifies a [`crate::Location`]'s virtual filesystem provider, for
    /// example `"file"`, `"archive"`, `"search"` or `"sftp"`.
    ProviderId
}

string_id! {
    /// Identifies a plugin manifest.
    PluginId
}

string_id! {
    /// Identifies a registered action (built-in or plugin-provided).
    ActionId
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn uuid_id_round_trips_through_display_and_from_str() {
        let id = WorkspaceId::new();
        let text = id.to_string();
        let parsed = WorkspaceId::from_str(&text).expect("valid uuid text must parse");
        assert_eq!(id, parsed);
    }

    #[test]
    fn uuid_id_round_trips_through_serde_json() {
        let id = PaneId::new();
        let json = serde_json::to_string(&id).expect("serialization must succeed");
        let parsed: PaneId = serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(id, parsed);
    }

    #[test]
    fn uuid_id_rejects_invalid_text() {
        assert!(TabId::from_str("not-a-uuid").is_err());
    }

    #[test]
    fn distinct_uuid_ids_are_not_equal() {
        assert_ne!(EntryId::new(), EntryId::new());
        assert_ne!(OperationId::new(), OperationId::new());
    }

    #[test]
    fn uuid_id_round_trips_through_the_uuid_conversion_traits() {
        let id = EntryId::new();
        let uuid: Uuid = id.into();
        assert_eq!(EntryId::from(uuid), id);
    }

    #[test]
    fn string_id_round_trips_through_display_and_from_str() {
        let id = ProviderId::new("file");
        let text = id.to_string();
        let parsed = ProviderId::from_str(&text).expect("string ids never fail to parse");
        assert_eq!(id, parsed);
        assert_eq!(id.as_str(), "file");
    }

    #[test]
    fn string_id_round_trips_through_serde_json() {
        let id = PluginId::new("sample-plugin");
        let json = serde_json::to_string(&id).expect("serialization must succeed");
        let parsed: PluginId = serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(id, parsed);
    }

    #[test]
    fn action_id_accepts_any_string() {
        let id = ActionId::new("core.rename");
        assert_eq!(id.as_str(), "core.rename");
    }
}
