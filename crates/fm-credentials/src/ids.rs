//! [`CredentialRef`]: the opaque, non-secret handle a [`crate::CredentialStore`]
//! returns from `store` and that a [`fm-connections`]-style connection profile
//! persists in place of any secret material (task 0103).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Error returned when a [`CredentialRef`] fails to parse from text.
#[derive(Debug, thiserror::Error)]
#[error("invalid credential reference: {0}")]
pub struct CredentialRefParseError(#[from] uuid::Error);

/// An opaque reference to secret material held by a [`crate::CredentialStore`].
///
/// Carries no secret content itself, so it is always safe to log, persist in
/// plain JSON or return from an API response - only the store that issued it
/// can turn it back into a [`crate::ResolvedCredential`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialRef(Uuid);

impl CredentialRef {
    /// Generates a new, randomly chosen reference.
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

impl Default for CredentialRef {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for CredentialRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for CredentialRef {
    type Err = CredentialRefParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::from_str(s)?))
    }
}

impl From<Uuid> for CredentialRef {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<CredentialRef> for Uuid {
    fn from(value: CredentialRef) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn round_trips_through_display_and_from_str() {
        let reference = CredentialRef::new();
        let text = reference.to_string();
        let parsed = CredentialRef::from_str(&text).expect("valid uuid text must parse");
        assert_eq!(reference, parsed);
    }

    #[test]
    fn round_trips_through_serde_json() {
        let reference = CredentialRef::new();
        let json = serde_json::to_string(&reference).expect("serialization must succeed");
        let parsed: CredentialRef =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(reference, parsed);
    }

    #[test]
    fn rejects_invalid_text() {
        assert!(CredentialRef::from_str("not-a-uuid").is_err());
    }

    #[test]
    fn distinct_references_are_not_equal() {
        assert_ne!(CredentialRef::new(), CredentialRef::new());
    }
}
