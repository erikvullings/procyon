//! Secure credential storage for application-managed connections (task 0103,
//! spec §5.3, §19).
//!
//! [`CredentialStore`] is the seam every host injects a platform-appropriate
//! implementation behind: `fm-credentials-macos` (Keychain) and
//! `fm-credentials-windows` (Credential Manager) are separate crates, each
//! gated with a whole-crate `#![cfg(target_os = "...")]` so the workspace
//! still builds everywhere (see `docs/decisions/0010-native-platform-adapters.md`).
//! [`InMemoryCredentialStore`] here is the explicit, documented fallback for
//! any other host and for tests - never a silent substitute for real
//! protected storage on macOS/Windows.
//!
//! Secret material is always represented by the tagged [`SecretMaterial`]
//! enum, never a generic string/byte map, and is never `Debug`-printable in
//! a form that reveals its content (spec §19 "no secret logging").

pub mod codec;
mod error;
mod ids;
mod memory;
mod secret;
mod session;
mod store;

pub use error::CredentialError;
pub use ids::{CredentialRef, CredentialRefParseError};
pub use memory::InMemoryCredentialStore;
pub use secret::SecretMaterial;
pub use session::SessionCredentialStore;
pub use store::{CredentialStore, ResolvedCredential, StoreCredentialRequest};
