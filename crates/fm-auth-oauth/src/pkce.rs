//! RFC 7636 PKCE code verifier/challenge and anti-CSRF `state` generation.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Random bytes behind [`CodeVerifier::generate`]. Base64url (no padding)
/// encodes 32 bytes as 43 characters - RFC 7636's minimum verifier length,
/// comfortably under its 128-character maximum, and generated from enough
/// entropy that guessing it is infeasible.
const VERIFIER_RANDOM_BYTES: usize = 32;

/// Random bytes behind [`generate_state`]. Not an RFC 7636 requirement -
/// chosen so the anti-CSRF `state` value is unguessable in practice.
const STATE_RANDOM_BYTES: usize = 24;

/// An RFC 7636 PKCE code verifier: 43-128 URL-safe characters, generated
/// fresh for one authorization attempt and never persisted or logged.
///
/// [`fmt::Debug`] never prints the verifier itself (spec §19).
#[derive(Clone)]
pub struct CodeVerifier(Zeroizing<String>);

impl CodeVerifier {
    /// Generates a new, cryptographically random verifier.
    #[must_use]
    pub fn generate() -> Self {
        let bytes: [u8; VERIFIER_RANDOM_BYTES] = rand::random();
        Self(Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes)))
    }

    /// The verifier's characters, to send as `code_verifier` in the token
    /// exchange request.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The RFC 7636 `S256` challenge derived from this verifier.
    fn challenge(&self) -> String {
        let digest = Sha256::digest(self.0.as_bytes());
        URL_SAFE_NO_PAD.encode(digest)
    }
}

impl fmt::Debug for CodeVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CodeVerifier").field(&"<redacted>").finish()
    }
}

/// A freshly generated PKCE verifier/challenge pair (RFC 7636, `S256`
/// method).
#[derive(Debug, Clone)]
pub struct PkcePair {
    /// The secret verifier, sent only to the token endpoint.
    pub verifier: CodeVerifier,
    /// The `S256` challenge derived from `verifier`, sent in the
    /// authorization URL. Not secret - it only proves knowledge of
    /// `verifier` later, it does not reveal it.
    pub challenge: String,
}

/// Generates a fresh verifier/challenge pair for one authorization attempt.
#[must_use]
pub fn generate_pkce_pair() -> PkcePair {
    let verifier = CodeVerifier::generate();
    let challenge = verifier.challenge();
    PkcePair {
        verifier,
        challenge,
    }
}

/// Generates a fresh anti-CSRF `state` value for one authorization attempt.
#[must_use]
pub fn generate_state() -> String {
    let bytes: [u8; STATE_RANDOM_BYTES] = rand::random();
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7636 §4.1: the verifier is a "high-entropy cryptographic random
    /// string using the unreserved characters `[A-Z] [a-z] [0-9] "-" "." "_"
    /// "~"` ... with a minimum length of 43 characters and a maximum length
    /// of 128 characters."
    #[test]
    fn generated_verifiers_meet_rfc_7636_length_and_charset() {
        for _ in 0..64 {
            let verifier = CodeVerifier::generate();
            let text = verifier.as_str();
            assert!(
                (43..=128).contains(&text.len()),
                "verifier length {} outside RFC 7636 bounds",
                text.len()
            );
            assert!(
                text.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "verifier {text:?} contains a character outside the unreserved set"
            );
        }
    }

    #[test]
    fn two_generated_verifiers_are_not_the_same() {
        let first = CodeVerifier::generate();
        let second = CodeVerifier::generate();
        assert_ne!(first.as_str(), second.as_str());
    }

    #[test]
    fn challenge_is_the_base64url_s256_hash_of_the_verifier() {
        let pair = generate_pkce_pair();
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(pair.verifier.as_str().as_bytes()));
        assert_eq!(pair.challenge, expected);
    }

    #[test]
    fn challenge_never_equals_the_verifier() {
        let pair = generate_pkce_pair();
        assert_ne!(pair.challenge, pair.verifier.as_str());
    }

    #[test]
    fn debug_output_never_contains_the_verifier() {
        let verifier = CodeVerifier::generate();
        let formatted = format!("{verifier:?}");
        assert!(!formatted.contains(verifier.as_str()));
        assert!(formatted.contains("<redacted>"));
    }

    #[test]
    fn generated_state_values_are_url_safe_and_distinct() {
        let first = generate_state();
        let second = generate_state();
        assert_ne!(first, second);
        assert!(
            first
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        assert!(first.len() >= 32);
    }
}
