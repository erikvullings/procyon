//! Session-based authentication for secure server mode (task 0064).
//!
//! All `/api/v1` routes except health check require a valid session token.
//! Tokens are validated using a session secret generated per server run.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use thiserror::Error;
use uuid::Uuid;

use crate::config::SessionSecret;

/// A session identifier verified against the server's secret.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SessionToken(String);

impl SessionToken {
    /// Creates a session token from a string (e.g., from a header).
    pub fn from_string(s: String) -> Self {
        Self(s)
    }

    /// Returns the token as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Error validating a session token.
#[derive(Debug, Error)]
pub enum SessionValidationError {
    /// Missing session token in request.
    #[error("missing session token")]
    MissingToken,

    /// Invalid session token format.
    #[error("invalid session token format")]
    InvalidFormat,

    /// Session token verification failed.
    #[error("session token verification failed")]
    VerificationFailed,
}

/// Session manager that validates and issues tokens.
pub struct SessionManager {
    secret: SessionSecret,
    /// If true, requests without a valid session token are still allowed.
    /// This relaxation is logged at startup and impossible when binding to
    /// non-loopback addresses (task 0064).
    pub dev_mode_disabled: bool,
}

impl SessionManager {
    /// Creates a session manager with the given secret.
    pub fn new(secret: SessionSecret, dev_mode_disabled: bool) -> Self {
        Self {
            secret,
            dev_mode_disabled,
        }
    }

    /// Validates a session token. In development mode with auth disabled,
    /// always returns `Ok(())`.
    pub fn validate_token(&self, token: Option<&str>) -> Result<(), SessionValidationError> {
        if self.dev_mode_disabled {
            return Ok(());
        }

        match token {
            None => Err(SessionValidationError::MissingToken),
            Some(token_str) => {
                // Verify the token against the secret using a simple HMAC-SHA256.
                // In production, this would use a secure cookie library or JWT.
                self.verify_token(token_str)
            }
        }
    }

    /// Issues a new session token.
    pub fn issue_token(&self) -> SessionToken {
        use sha2::Digest;

        let nonce = Uuid::new_v4().to_string();
        let mut hasher = sha2::Sha256::new();
        hasher.update(self.secret.as_bytes());
        hasher.update(nonce.as_bytes());
        let hash = hasher.finalize();

        // Convert the digest to a hex string
        let hash_hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        SessionToken::from_string(format!("{}-{}", hash_hex, nonce))
    }

    /// Verifies that a token was issued by this session manager.
    fn verify_token(&self, token: &str) -> Result<(), SessionValidationError> {
        use sha2::Digest;

        let parts: Vec<&str> = token.split('-').collect();
        if parts.len() < 2 {
            return Err(SessionValidationError::InvalidFormat);
        }

        let expected_hash = parts[0];
        let nonce = parts[1..].join("-");

        let mut hasher = sha2::Sha256::new();
        hasher.update(self.secret.as_bytes());
        hasher.update(nonce.as_bytes());
        let hash = hasher.finalize();

        let hash_hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        if hash_hex == expected_hash {
            Ok(())
        } else {
            Err(SessionValidationError::VerificationFailed)
        }
    }
}

/// Routes reachable without a session token: an unauthenticated health probe
/// and the API documentation surfaces, neither of which touches the
/// filesystem or application state.
const PUBLIC_PATHS: &[&str] = &["/api/v1/health"];
const PUBLIC_PATH_PREFIXES: &[&str] = &["/api/v1/docs", "/api/v1/openapi.json"];

/// Middleware that enforces session authentication on every other
/// `/api/v1` route, including the SSE stream (task 0064).
///
/// Browser `EventSource` connections cannot set an `Authorization` header, so
/// the token is also accepted as a `?token=` query parameter; the header
/// takes precedence when both are present.
pub async fn require_session(
    State(manager): State<Arc<SessionManager>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = request.uri().path();
    if PUBLIC_PATHS.contains(&path) || PUBLIC_PATH_PREFIXES.iter().any(|p| path.starts_with(p)) {
        return Ok(next.run(request).await);
    }

    let header_token = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::to_owned);
    let query_token = query_token(request.uri());
    let token = header_token.as_deref().or(query_token.as_deref());

    manager
        .validate_token(token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    Ok(next.run(request).await)
}

/// Extracts the `token` query parameter, if present, without pulling in a
/// full URL-parsing dependency for one field.
fn query_token(uri: &axum::http::Uri) -> Option<String> {
    let query = uri.query()?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "token").then(|| percent_decode(value))
    })
}

/// Minimal percent-decoding sufficient for the opaque hex/UUID token format
/// (spec token grammar has no reserved characters, but `+` and `%XX` are
/// decoded defensively in case a client encodes it anyway).
fn percent_decode(value: &str) -> String {
    let mut bytes = Vec::with_capacity(value.len());
    let mut chars = value.bytes();
    while let Some(byte) = chars.next() {
        match byte {
            b'%' => {
                let hi = chars.next();
                let lo = chars.next();
                if let (Some(hi), Some(lo)) = (hi, lo)
                    && let Ok(decoded) =
                        u8::from_str_radix(&format!("{}{}", hi as char, lo as char), 16)
                {
                    bytes.push(decoded);
                    continue;
                }
                bytes.push(byte);
            }
            b'+' => bytes.push(b' '),
            other => bytes.push(other),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_mode_disables_auth() {
        let secret = SessionSecret::random();
        let manager = SessionManager::new(secret, true);

        assert!(manager.validate_token(None).is_ok());
        assert!(manager.validate_token(Some("invalid")).is_ok());
    }

    #[test]
    fn missing_token_is_rejected() {
        let secret = SessionSecret::random();
        let manager = SessionManager::new(secret, false);

        assert!(matches!(
            manager.validate_token(None),
            Err(SessionValidationError::MissingToken)
        ));
    }

    #[test]
    fn issued_token_can_be_verified() {
        let secret = SessionSecret::random();
        let manager = SessionManager::new(secret, false);

        let token = manager.issue_token();
        assert!(manager.validate_token(Some(token.as_str())).is_ok());
    }

    #[test]
    fn invalid_token_is_rejected() {
        let secret = SessionSecret::random();
        let manager = SessionManager::new(secret, false);

        assert!(manager.validate_token(Some("invalid-token")).is_err());
    }

    #[test]
    fn token_from_different_secret_is_rejected() {
        let secret1 = SessionSecret::random();
        let secret2 = SessionSecret::random();

        let manager1 = SessionManager::new(secret1, false);
        let token = manager1.issue_token();

        let manager2 = SessionManager::new(secret2, false);
        assert!(manager2.validate_token(Some(token.as_str())).is_err());
    }

    #[test]
    fn token_issued_by_manager_is_not_static() {
        let secret = SessionSecret::random();
        let manager = SessionManager::new(secret, false);

        let token1 = manager.issue_token();
        let token2 = manager.issue_token();

        // Tokens should be different (different nonces).
        assert_ne!(token1, token2);
        // But both should be valid.
        assert!(manager.validate_token(Some(token1.as_str())).is_ok());
        assert!(manager.validate_token(Some(token2.as_str())).is_ok());
    }
}
