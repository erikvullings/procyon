//! Shared Axum handler state (spec §7: handlers only call the service).

use std::sync::Arc;
use std::sync::Mutex;
use std::time::SystemTime;

use fm_application::FileManagerService;
use fm_application::diagnostics::DiagnosticErrorBuffer;
use fm_application::semantic_library::SemanticAccessContext;
use tokio_util::sync::CancellationToken;

use crate::auth::SessionManager;
use crate::rate_limit::MutationLimiter;

/// Connection state tracking for diagnostics (spec §30).
#[derive(Clone)]
pub(crate) struct ConnectionState {
    state: Arc<Mutex<ConnectionStateInner>>,
}

#[derive(Clone, Debug)]
struct ConnectionStateInner {
    connected: bool,
    last_event_received: Option<String>,
    uptime_start: SystemTime,
    events_received: u64,
}

impl ConnectionState {
    /// Create a new connection state.
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ConnectionStateInner {
                connected: true,
                last_event_received: None,
                uptime_start: SystemTime::now(),
                events_received: 0,
            })),
        }
    }

    /// Mark an event as received.
    #[allow(dead_code)]
    pub(crate) fn record_event(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.events_received += 1;
            state.last_event_received =
                Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        }
    }

    /// Set connection status.
    #[allow(dead_code)]
    pub(crate) fn set_connected(&self, connected: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.connected = connected;
            if connected {
                state.uptime_start = SystemTime::now();
            }
        }
    }

    /// Get current connection state snapshot.
    pub(crate) fn snapshot(&self) -> (bool, Option<String>, u64, u64) {
        if let Ok(state) = self.state.lock() {
            let uptime = state
                .uptime_start
                .elapsed()
                .map(|d| d.as_secs())
                .unwrap_or(0);
            (
                state.connected,
                state.last_event_received.clone(),
                uptime,
                state.events_received,
            )
        } else {
            (false, None, 0, 0)
        }
    }
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self::new()
    }
}

/// State injected into every Axum handler.
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) service: Arc<FileManagerService>,
    pub(crate) cors_allowed_origins: Arc<[String]>,
    pub(crate) session_end: CancellationToken,
    pub(crate) error_buffer: DiagnosticErrorBuffer,
    pub(crate) connection_state: ConnectionState,
    /// Validates session tokens for every `/api/v1` route except health and
    /// docs (task 0064). Extracted directly by the `require_session`
    /// middleware via [`axum::extract::FromRef`].
    pub(crate) session_manager: Arc<SessionManager>,
    /// Filesystem roots the server is permitted to expose; empty means
    /// unrestricted (task 0064).
    pub(crate) accessible_roots: Arc<[std::path::PathBuf]>,
    /// Shared token bucket throttling mutating requests (task 0064).
    pub(crate) mutation_limiter: Arc<MutationLimiter>,
    /// Semantic-library principal this server serves every request as
    /// (task 0179).
    ///
    /// Built once, from operator-owned server configuration, and never from
    /// request data. Server mode is single-user, so a valid session token means
    /// "this server's configured principal" and nothing more; a crafted header,
    /// query parameter, or request body cannot select another tenant, user, or
    /// library. `Anonymous` when the configured values are structurally
    /// invalid, which denies every semantic-library call.
    pub(crate) semantic_access: Arc<SemanticAccessContext>,
}

impl AppState {
    /// Returns the trusted semantic principal for one request.
    pub(crate) fn semantic_access(&self) -> &SemanticAccessContext {
        &self.semantic_access
    }
}

impl axum::extract::FromRef<AppState> for Arc<SessionManager> {
    fn from_ref(state: &AppState) -> Self {
        state.session_manager.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<MutationLimiter> {
    fn from_ref(state: &AppState) -> Self {
        state.mutation_limiter.clone()
    }
}
