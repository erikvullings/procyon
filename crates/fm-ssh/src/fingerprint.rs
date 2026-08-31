//! Host-key fingerprint formatting (task 0104, spec §6.4).

use russh::keys::{HashAlg, PublicKey};

/// Formats `key`'s SHA-256 fingerprint as `SHA256:<base64>`, matching the
/// form OpenSSH itself prints (for example in `ssh-keygen -lf`).
#[must_use]
pub fn fingerprint_of(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

#[cfg(test)]
mod tests {
    use russh::keys::{Algorithm, PrivateKey};

    use super::*;

    #[test]
    fn fingerprint_is_deterministic_for_the_same_key() {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let a = fingerprint_of(key.public_key());
        let b = fingerprint_of(key.public_key());
        assert_eq!(a, b);
        assert!(a.starts_with("SHA256:"));
    }

    #[test]
    fn distinct_keys_produce_distinct_fingerprints() {
        let first = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let second = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        assert_ne!(
            fingerprint_of(first.public_key()),
            fingerprint_of(second.public_key())
        );
    }
}
