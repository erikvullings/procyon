use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a random identity.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Creates an identity from an existing UUID.
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            /// Returns the underlying UUID.
            #[must_use]
            pub const fn into_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

uuid_id! {
    /// Stable identity of one device-local or administrator-defined semantic library.
    LibraryId
}

uuid_id! {
    /// Stable content-addressed catalog document identity.
    DocumentId
}

uuid_id! {
    /// Stable provider-entry occurrence identity.
    OccurrenceId
}

uuid_id! {
    /// Opaque identity of one derived excerpt, summary, label, or vector record.
    DerivedArtifactId
}

uuid_id! {
    /// Stable identity of a saved-conversation evidence pin.
    ConversationPinId
}

uuid_id! {
    /// Stable identity of a resumable exclusion deletion plan.
    DeletionPlanId
}

uuid_id! {
    /// Stable, path-independent identity of an enrolled root.
    RootId
}

uuid_id! {
    /// Stable identity of an explicit descendant exclusion.
    ExclusionId
}

macro_rules! text_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated opaque identifier.
            ///
            /// # Errors
            ///
            /// Rejects empty or structurally unsafe values.
            pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > 256
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                    })
                {
                    return Err(IdentifierError);
                }
                Ok(Self(value))
            }

            /// Returns the opaque identifier text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, formatter)
            }
        }
    };
}

text_id! {
    /// Opaque server tenant identity.
    TenantId
}

text_id! {
    /// Opaque authenticated user identity.
    UserId
}

/// Stable identifier of an attached semantic vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VocabularyId(String);

impl VocabularyId {
    /// Creates a vocabulary identifier.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the opaque identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Invalid opaque semantic identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("semantic identifier is invalid")]
pub struct IdentifierError;
