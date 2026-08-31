//! Audit logging for destructive operations (task 0064).
//!
//! Logs delete, trash, and overwrite operations including who (session), what,
//! and when, without file contents or secrets.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::{DateTime, Utc};
use sha2::Digest;
use tracing::info;

/// Extracts a short, non-reversible identifier for the caller's session
/// token, suitable for the "who" field of an [`AuditEvent`] without logging
/// the credential itself (spec §30, task 0064). `None` when the request
/// carries no token (dev mode).
pub(crate) struct SessionIdHeader(pub(crate) Option<String>);

impl<S> FromRequestParts<S> for SessionIdHeader
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "));
        Ok(Self(token.map(hash_token)))
    }
}

/// A short, stable, non-reversible fingerprint of a session token.
fn hash_token(token: &str) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(token.as_bytes());
    let hash = hasher.finalize();
    hash.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// Audit events for file operations.
#[derive(Debug)]
pub struct AuditEvent {
    /// Operation type (delete, trash, overwrite).
    pub operation: AuditOperation,
    /// Path being modified (relative to root if possible).
    pub path: String,
    /// Session identifier (if authenticated).
    pub session_id: Option<String>,
    /// Timestamp of the operation.
    pub timestamp: DateTime<Utc>,
}

/// Operation types tracked in audit logs.
#[derive(Debug, Clone, Copy)]
pub enum AuditOperation {
    /// File deletion operation.
    Delete,
    /// File trash operation.
    Trash,
    /// File overwrite operation.
    Overwrite,
}

impl std::fmt::Display for AuditOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditOperation::Delete => write!(f, "delete"),
            AuditOperation::Trash => write!(f, "trash"),
            AuditOperation::Overwrite => write!(f, "overwrite"),
        }
    }
}

impl AuditEvent {
    /// Creates a new audit event.
    pub fn new(operation: AuditOperation, path: String, session_id: Option<String>) -> Self {
        Self {
            operation,
            path,
            session_id,
            timestamp: Utc::now(),
        }
    }

    /// Logs the audit event. Does not include file contents or secrets.
    pub fn log(&self) {
        match self.session_id.as_ref() {
            Some(session_id) => {
                info!(
                    operation = %self.operation,
                    path = %self.path,
                    session_id = %session_id,
                    timestamp = %self.timestamp,
                    "audit: destructive operation"
                );
            }
            None => {
                info!(
                    operation = %self.operation,
                    path = %self.path,
                    timestamp = %self.timestamp,
                    "audit: destructive operation (unauthenticated)"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_event_creation() {
        let event = AuditEvent::new(
            AuditOperation::Delete,
            "/home/user/file.txt".to_string(),
            Some("session-123".to_string()),
        );

        assert_eq!(event.path, "/home/user/file.txt");
        assert_eq!(event.session_id, Some("session-123".to_string()));
    }

    #[test]
    fn audit_operation_display() {
        assert_eq!(AuditOperation::Delete.to_string(), "delete");
        assert_eq!(AuditOperation::Trash.to_string(), "trash");
        assert_eq!(AuditOperation::Overwrite.to_string(), "overwrite");
    }

    #[test]
    fn audit_event_logs_without_panic() {
        // Just verify the logging doesn't panic; actual log output
        // is tested via integration tests with log capture.
        let event = AuditEvent::new(
            AuditOperation::Delete,
            "/home/user/file.txt".to_string(),
            Some("session-123".to_string()),
        );
        event.log();
    }
}
