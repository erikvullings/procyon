//! Native OneDrive authorization and token-resolution capability (task
//! 0110). See `crates/fm-vfs-onedrive` for the actual Microsoft Graph
//! `FileSystemProvider`; this module owns everything upstream of it: OAuth
//! 2.0 Authorization Code + PKCE through the system browser (via
//! `fm-auth-oauth`), the backend-owned authorization-attempt state machine
//! ([`OneDriveAuthorizationService`]), silent token refresh/rotation
//! ([`OneDriveTokenResolver`]), and the `ConnectionDialer`
//! ([`dialer::OneDriveDialer`]) that verifies `/me/drive` for connect/test.
//!
//! Deliberately never depends on [`crate::service::FileManagerService`] -
//! `FileManagerService`'s own methods stay thin delegations into
//! [`OneDriveAuthorizationService`], matching every other deep capability
//! module in this crate (`SearchComparisonCoordinator`,
//! `ChecksumCoordinator`, ...).
//!
//! ## The authorization-attempt state machine
//!
//! [`OneDriveAuthorizationService::begin_authorization`] validates the
//! target connection, reserves it (rejecting a second concurrent attempt
//! for the *same* connection - different connections are never blocked by
//! each other), binds a loopback callback listener - so its exact
//! `redirect_uri` can be embedded in the authorization URL *before* it is
//! ever returned to a caller - generates a fresh PKCE pair and CSRF
//! `state`, then hands the caller an attempt id and the Microsoft
//! authorization URL while the callback wait, authorization-code exchange,
//! scope validation, Graph identity verification and credential persistence
//! all run on a spawned background task. [`OneDriveAuthorizationService::attempt_status`]
//! polls that attempt; [`OneDriveAuthorizationService::cancel_authorization`]
//! cancels it. Completed attempts are retained only briefly and only up to
//! a bounded count (see [`sweep`]), so this registry cannot grow without
//! bound across a long-running process.
//!
//! ## Conditional Access replay
//!
//! If Graph verification hits an `insufficient_claims` challenge, the
//! parsed [`fm_auth_oauth::claims::ClaimsChallenge`] is retained in bounded
//! in-memory state keyed by connection id (never logged, never part of any
//! DTO) and the attempt fails with
//! [`fm_transport_dto::OneDriveAuthorizationErrorCodeDto::ConditionalAccessRequired`].
//! The *next* [`OneDriveAuthorizationService::begin_authorization`] call for
//! that same connection consumes it automatically and builds a challenged
//! authorization request (fresh `state`/PKCE, the stored claims merged with
//! the client's `cp1` capability) instead of a plain one - an explicit,
//! narrowly-scoped "replay" path rather than a separate endpoint, since
//! nothing else about beginning authorization differs.

mod dialer;
mod graph;
#[cfg(test)]
mod test_support;
mod token;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fm_auth_oauth::authorization::{
    build_authorization_request, build_challenged_authorization_request,
};
use fm_auth_oauth::callback::CallbackListener;
use fm_auth_oauth::claims::ClaimsChallenge;
use fm_auth_oauth::config::PublicClientConfig;
use fm_auth_oauth::error::OAuthError;
use fm_auth_oauth::pkce::{CodeVerifier, generate_pkce_pair};
use fm_auth_oauth::token::exchange_authorization_code;
use fm_connections::{
    ConnectionConfiguration, ConnectionId, ConnectionRepository, ConnectionService,
    OneDriveConnectionConfiguration,
};
use fm_credentials::SecretMaterial;
use fm_transport_dto::{
    BeginOneDriveAuthorizationResponseDto, OneDriveAuthorizationAttemptDto,
    OneDriveAuthorizationErrorCodeDto, OneDriveAuthorizationStatusDto,
};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use self::dialer::OneDriveDialer;
use self::graph::GraphVerifyError;
pub(crate) use self::token::OneDriveTokenResolver;
use crate::error::ApplicationError;

/// Procyon's public-client Microsoft Entra application (client) id (task
/// 0110). Public configuration, not a secret: the registration has no
/// client secret, accepts both personal Microsoft accounts and any Entra
/// organizational tenant via the `common` authority, and PKCE replaces the
/// secret a confidential client would otherwise need.
pub(crate) const ONEDRIVE_CLIENT_ID: &str = "9b01b729-5908-492b-bcd1-32b4a36096de";

/// How long [`OneDriveAuthorizationService::begin_authorization`]'s
/// background task waits for the browser to complete sign-in before an
/// attempt fails with [`OneDriveAuthorizationErrorCodeDto::Timeout`].
pub(crate) const DEFAULT_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Upper bound, independent of and in addition to each individual HTTP
/// request's own timeout (see [`build_http_client`]), on the whole
/// post-callback sequence once the browser redirect has been received:
/// authorization-code exchange, scope validation, Graph identity
/// verification, credential persistence and status reconciliation. Bounds
/// a pathological sequence of several individually-not-quite-timed-out
/// calls so it still cannot leave one attempt `Pending` forever (task 0110
/// review: "an overall deadline, ensuring finish_attempt runs and
/// active_connections is released").
pub(crate) const DEFAULT_POST_CALLBACK_DEADLINE: Duration = Duration::from_secs(60);

/// Production request timeout for every OAuth/Graph HTTP call this module
/// makes: bounds the *whole* request (connect, send, and read the full
/// response) so a stalled or slow-to-respond identity-provider/Graph peer
/// can never hang a refresh, dial, or authorization attempt indefinitely
/// (task 0110 review: "token resolver/dialer cannot wait forever").
pub(crate) const DEFAULT_HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Production connect timeout: bounds only the TCP/TLS handshake,
/// independent of the overall request timeout above.
pub(crate) const DEFAULT_HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Upper bound on how many *terminal* (succeeded/failed/cancelled) attempts
/// are retained at once, oldest evicted first - bounds memory regardless of
/// timing (task 0110: "bound retention/cleanup of completed attempts").
const MAX_RETAINED_TERMINAL_ATTEMPTS: usize = 200;

/// How long a terminal attempt is retained before it becomes eligible for
/// eviction on the next sweep, so a caller polling shortly after completion
/// reliably still finds it.
const TERMINAL_ATTEMPT_RETENTION: Duration = Duration::from_secs(15 * 60);

/// Builds an HTTP client with an explicit request and connect timeout.
/// Every `reqwest::Client` this module constructs - production and test
/// alike - goes through this rather than a bare `reqwest::Client::new()`
/// (which has no timeout at all), so nothing here can wait on a stalled
/// peer forever (task 0110 review).
pub(crate) fn build_http_client(
    request_timeout: Duration,
    connect_timeout: Duration,
) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(request_timeout)
        .connect_timeout(connect_timeout)
        .build()
        .expect("a minimal reqwest client configuration always builds")
}

/// Builds the production [`OneDriveDialer`] for registration with
/// [`fm_connections::ConnectionService::with_dialer`]. Shares `resolver`
/// with the `FileSystemProvider` and this module's own authorization
/// completion, so every code path resolving a bearer token for one
/// connection shares the same cache and per-connection refresh
/// serialization.
pub(crate) fn dialer(
    resolver: Arc<OneDriveTokenResolver>,
    graph_base_url: Url,
    http: reqwest::Client,
) -> Arc<dyn fm_connections::ConnectionDialer> {
    Arc::new(OneDriveDialer::new(resolver, graph_base_url, http))
}

/// Builds a token resolver. Production wiring points `oauth`/`http` at the
/// real Microsoft identity platform; tests point them at injected loopback
/// fixtures (see [`OneDriveAuthorizationServiceConfig`]).
pub(crate) fn token_resolver(
    repository: Arc<dyn ConnectionRepository>,
    credentials: Arc<dyn fm_credentials::CredentialStore>,
    oauth: PublicClientConfig,
    http: reqwest::Client,
) -> Arc<OneDriveTokenResolver> {
    Arc::new(OneDriveTokenResolver::new(
        repository,
        credentials,
        oauth,
        http,
    ))
}

/// The Microsoft identity platform authority/Microsoft Graph base URL, HTTP
/// client and timing this service (and its dialer/resolver siblings) talk
/// to - injected so tests point every network call at in-process loopback
/// fixtures with short timeouts, never at a real Microsoft endpoint (task
/// 0110's testing requirement).
pub(crate) struct OneDriveAuthorizationServiceConfig {
    pub(crate) oauth: PublicClientConfig,
    pub(crate) graph_base_url: Url,
    pub(crate) http: reqwest::Client,
    pub(crate) callback_timeout: Duration,
    pub(crate) post_callback_deadline: Duration,
}

impl OneDriveAuthorizationServiceConfig {
    /// Production configuration: the real Microsoft identity platform
    /// `common` authority and Microsoft Graph v1.0 endpoint, with explicit
    /// HTTP timeouts (task 0110 review).
    pub(crate) fn production() -> Self {
        Self {
            oauth: PublicClientConfig::microsoft_common(ONEDRIVE_CLIENT_ID),
            graph_base_url: Url::parse(fm_vfs_onedrive::PRODUCTION_GRAPH_BASE_URL)
                .expect("static production Graph base URL always parses"),
            http: build_http_client(DEFAULT_HTTP_REQUEST_TIMEOUT, DEFAULT_HTTP_CONNECT_TIMEOUT),
            callback_timeout: DEFAULT_CALLBACK_TIMEOUT,
            post_callback_deadline: DEFAULT_POST_CALLBACK_DEADLINE,
        }
    }
}

/// Backend-owned authorization-attempt state machine (task 0110). See this
/// module's own documentation for the overall design.
pub(crate) struct OneDriveAuthorizationService<R: ConnectionRepository> {
    shared: Arc<Shared<R>>,
}

struct Shared<R: ConnectionRepository> {
    connections: Arc<ConnectionService<R>>,
    token_resolver: Arc<OneDriveTokenResolver>,
    oauth: PublicClientConfig,
    graph_base_url: Url,
    http: reqwest::Client,
    callback_timeout: Duration,
    post_callback_deadline: Duration,
    attempts: Mutex<AttemptRegistry>,
}

#[derive(Default)]
struct AttemptRegistry {
    by_id: HashMap<Uuid, AttemptEntry>,
    active_connections: HashSet<ConnectionId>,
    /// A Conditional Access challenge from a connection's most recent
    /// failed attempt, consumed (removed) by the next
    /// [`OneDriveAuthorizationService::begin_authorization`] call for that
    /// same connection.
    pending_claims: HashMap<ConnectionId, ClaimsChallenge>,
}

struct AttemptEntry {
    connection_id: ConnectionId,
    cancellation: CancellationToken,
    status: AttemptStatus,
    created_at: Instant,
}

#[derive(Clone)]
enum AttemptStatus {
    Pending,
    Succeeded,
    Failed(FailureReason),
    Cancelled,
}

/// Sanitized, actionable classification of an authorization failure -
/// mirrors [`OneDriveAuthorizationErrorCodeDto`] exactly, plus (for the
/// variants where the identity provider gave one) a pre-vetted-safe
/// description safe to surface verbatim (task 0110: "preserve Graph/OAuth
/// provider descriptions only when safe").
#[derive(Clone)]
enum FailureReason {
    AccessDenied(String),
    InvalidGrant(String),
    InteractionRequired(String),
    TenantPolicyRejected(String),
    /// `None` for a Microsoft Graph `insufficient_claims` challenge (the
    /// challenge itself is retained separately, never in this message);
    /// `Some` for an OAuth-level Conditional Access classification during
    /// sign-in itself, carrying its own safe description.
    ConditionalAccessRequired(Option<String>),
    /// Carries its own message since this covers two distinct causes: the
    /// granted `scope` string missing `Files.ReadWrite`/`User.Read`, and a
    /// successful code exchange that nonetheless omitted a `refresh_token`
    /// (meaning `offline_access` did not actually produce a renewable
    /// session - task 0110 review).
    InsufficientScope(String),
    Timeout,
    NetworkError(String),
    Internal(String),
}

impl FailureReason {
    fn code(&self) -> OneDriveAuthorizationErrorCodeDto {
        match self {
            Self::AccessDenied(_) => OneDriveAuthorizationErrorCodeDto::AccessDenied,
            Self::InvalidGrant(_) => OneDriveAuthorizationErrorCodeDto::InvalidGrant,
            Self::InteractionRequired(_) => OneDriveAuthorizationErrorCodeDto::InteractionRequired,
            Self::TenantPolicyRejected(_) => {
                OneDriveAuthorizationErrorCodeDto::TenantPolicyRejected
            }
            Self::ConditionalAccessRequired(_) => {
                OneDriveAuthorizationErrorCodeDto::ConditionalAccessRequired
            }
            Self::InsufficientScope(_) => OneDriveAuthorizationErrorCodeDto::InsufficientScope,
            Self::Timeout => OneDriveAuthorizationErrorCodeDto::Timeout,
            Self::NetworkError(_) => OneDriveAuthorizationErrorCodeDto::NetworkError,
            Self::Internal(_) => OneDriveAuthorizationErrorCodeDto::Internal,
        }
    }

    fn message(&self) -> String {
        match self {
            Self::AccessDenied(description)
            | Self::InvalidGrant(description)
            | Self::InteractionRequired(description)
            | Self::TenantPolicyRejected(description) => description.clone(),
            Self::ConditionalAccessRequired(Some(description)) => description.clone(),
            Self::ConditionalAccessRequired(None) => {
                "Microsoft requires additional verification (Conditional Access) before granting \
                 access. Reauthorize to continue."
                    .to_owned()
            }
            Self::InsufficientScope(message) => message.clone(),
            Self::Timeout => "Sign-in was not completed in time.".to_owned(),
            Self::NetworkError(message) => {
                format!("A network error occurred while contacting Microsoft: {message}")
            }
            Self::Internal(message) => message.clone(),
        }
    }
}

fn failure_reason_from_oauth_error(error: &OAuthError) -> FailureReason {
    match error {
        OAuthError::InteractionRequired { description } => {
            FailureReason::InteractionRequired(description.clone())
        }
        OAuthError::InvalidGrant { description } => {
            FailureReason::InvalidGrant(description.clone())
        }
        OAuthError::AccessDenied { description } => {
            FailureReason::AccessDenied(description.clone())
        }
        OAuthError::TenantPolicyRejected { description } => {
            FailureReason::TenantPolicyRejected(description.clone())
        }
        OAuthError::ConditionalAccessRequired { description } => {
            FailureReason::ConditionalAccessRequired(Some(description.clone()))
        }
        OAuthError::AuthorizationRejected { error, description } => {
            FailureReason::Internal(format!("{error}: {description}"))
        }
        OAuthError::MalformedCallback { reason }
        | OAuthError::MalformedClaimsChallenge { reason } => {
            FailureReason::Internal(reason.clone())
        }
        OAuthError::MalformedTokenResponse { reason } => {
            FailureReason::NetworkError(reason.clone())
        }
        OAuthError::Transport { message } => FailureReason::NetworkError(message.clone()),
        // Handled by the caller before this classifier ever runs; included
        // only for match exhaustiveness.
        OAuthError::Cancelled => FailureReason::Internal("sign-in was cancelled".to_owned()),
        OAuthError::TimedOut => FailureReason::Timeout,
    }
}

/// Whether `scope` (a space-separated OAuth scope string) contains `needle`,
/// compared case-insensitively (task 0110: "Validate granted scope includes
/// Files.ReadWrite and User.Read (case-insensitive)").
fn scope_contains(scope: &str, needle: &str) -> bool {
    scope
        .split_whitespace()
        .any(|token| token.eq_ignore_ascii_case(needle))
}

/// Evicts terminal attempts once they exceed [`TERMINAL_ATTEMPT_RETENTION`]
/// in age, then - regardless of age - evicts the oldest terminal attempts
/// until at most [`MAX_RETAINED_TERMINAL_ATTEMPTS`] remain. Never evicts a
/// still-`Pending` attempt.
fn sweep(registry: &mut AttemptRegistry) {
    let now = Instant::now();
    registry.by_id.retain(|_, entry| {
        matches!(entry.status, AttemptStatus::Pending)
            || now.duration_since(entry.created_at) < TERMINAL_ATTEMPT_RETENTION
    });

    let terminal_count = registry
        .by_id
        .values()
        .filter(|entry| !matches!(entry.status, AttemptStatus::Pending))
        .count();
    if terminal_count > MAX_RETAINED_TERMINAL_ATTEMPTS {
        let mut terminal: Vec<(Uuid, Instant)> = registry
            .by_id
            .iter()
            .filter(|(_, entry)| !matches!(entry.status, AttemptStatus::Pending))
            .map(|(id, entry)| (*id, entry.created_at))
            .collect();
        terminal.sort_by_key(|(_, created_at)| *created_at);
        for (id, _) in terminal
            .into_iter()
            .take(terminal_count - MAX_RETAINED_TERMINAL_ATTEMPTS)
        {
            registry.by_id.remove(&id);
        }
    }
}

impl<R> OneDriveAuthorizationService<R>
where
    R: ConnectionRepository + Send + Sync + 'static,
{
    pub(crate) fn new(
        connections: Arc<ConnectionService<R>>,
        token_resolver: Arc<OneDriveTokenResolver>,
        config: OneDriveAuthorizationServiceConfig,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                connections,
                token_resolver,
                oauth: config.oauth,
                graph_base_url: config.graph_base_url,
                http: config.http,
                callback_timeout: config.callback_timeout,
                post_callback_deadline: config.post_callback_deadline,
                attempts: Mutex::new(AttemptRegistry::default()),
            }),
        }
    }

    /// Begins a new authorization attempt for a saved OneDrive connection
    /// (task 0110). Binds the loopback callback listener - so the exact
    /// `redirect_uri` is fixed - before returning anything, generates a
    /// fresh `state`/PKCE pair, and runs the callback wait and token
    /// exchange on a spawned background task; this call itself returns as
    /// soon as the listener is bound and the attempt is registered.
    ///
    /// Rejects an unknown connection id, a connection that is not
    /// `ConnectionKind::OneDrive`, and a second concurrent attempt for a
    /// connection that already has one in flight (different connections
    /// are never blocked by each other).
    pub(crate) async fn begin_authorization(
        &self,
        connection_id: Uuid,
    ) -> Result<BeginOneDriveAuthorizationResponseDto, ApplicationError> {
        let connection_id: ConnectionId = connection_id.into();
        let profile = self.shared.connections.get(connection_id).await?;
        if !matches!(profile.configuration, ConnectionConfiguration::OneDrive(_)) {
            return Err(ApplicationError::InvalidRequest(
                "connection is not a OneDrive connection".to_owned(),
            ));
        }

        {
            let mut registry = self
                .shared
                .attempts
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            sweep(&mut registry);
            if !registry.active_connections.insert(connection_id) {
                return Err(ApplicationError::InvalidRequest(
                    "an authorization attempt is already in progress for this connection"
                        .to_owned(),
                ));
            }
        }

        let outcome = self.begin_after_reservation(connection_id).await;
        if outcome.is_err() {
            let mut registry = self
                .shared
                .attempts
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            registry.active_connections.remove(&connection_id);
        }
        outcome
    }

    async fn begin_after_reservation(
        &self,
        connection_id: ConnectionId,
    ) -> Result<BeginOneDriveAuthorizationResponseDto, ApplicationError> {
        let listener = CallbackListener::bind().await.map_err(|error| {
            ApplicationError::PlatformOperationFailed(format!(
                "failed to start the sign-in listener: {error}"
            ))
        })?;
        let redirect_uri = listener.redirect_uri().clone();
        let pkce = generate_pkce_pair();

        let pending_claims = {
            let mut registry = self
                .shared
                .attempts
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            registry.pending_claims.remove(&connection_id)
        };
        let request = match &pending_claims {
            Some(challenge) => build_challenged_authorization_request(
                &self.shared.oauth,
                &redirect_uri,
                &pkce.challenge,
                challenge,
            ),
            None => build_authorization_request(&self.shared.oauth, &redirect_uri, &pkce.challenge),
        };

        let attempt_id = Uuid::new_v4();
        let cancellation = CancellationToken::new();
        {
            let mut registry = self
                .shared
                .attempts
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            registry.by_id.insert(
                attempt_id,
                AttemptEntry {
                    connection_id,
                    cancellation: cancellation.clone(),
                    status: AttemptStatus::Pending,
                    created_at: Instant::now(),
                },
            );
        }

        let shared = Arc::clone(&self.shared);
        let state = request.state.clone();
        let verifier = pkce.verifier;
        tokio::spawn(async move {
            let outcome = run_attempt(
                &shared,
                connection_id,
                listener,
                state,
                verifier,
                cancellation,
            )
            .await;
            shared.finish_attempt(attempt_id, connection_id, outcome);
        });

        Ok(BeginOneDriveAuthorizationResponseDto {
            attempt_id,
            authorization_url: request.url.to_string(),
        })
    }

    /// Polls an authorization attempt's current status (task 0110).
    pub(crate) async fn attempt_status(
        &self,
        attempt_id: Uuid,
    ) -> Result<OneDriveAuthorizationAttemptDto, ApplicationError> {
        let (connection_id, status) = {
            let registry = self
                .shared
                .attempts
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let entry = registry
                .by_id
                .get(&attempt_id)
                .ok_or(ApplicationError::NotFound)?;
            (entry.connection_id, entry.status.clone())
        };
        self.status_dto(attempt_id, connection_id, status).await
    }

    /// Cancels a pending authorization attempt (task 0110). Idempotent: an
    /// already-terminal attempt is simply returned as-is rather than
    /// erroring.
    pub(crate) async fn cancel_authorization(
        &self,
        attempt_id: Uuid,
    ) -> Result<OneDriveAuthorizationAttemptDto, ApplicationError> {
        {
            let registry = self
                .shared
                .attempts
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let entry = registry
                .by_id
                .get(&attempt_id)
                .ok_or(ApplicationError::NotFound)?;
            entry.cancellation.cancel();
        }
        self.attempt_status(attempt_id).await
    }

    async fn status_dto(
        &self,
        attempt_id: Uuid,
        connection_id: ConnectionId,
        status: AttemptStatus,
    ) -> Result<OneDriveAuthorizationAttemptDto, ApplicationError> {
        let status = match status {
            AttemptStatus::Pending => OneDriveAuthorizationStatusDto::Pending,
            AttemptStatus::Cancelled => OneDriveAuthorizationStatusDto::Cancelled,
            AttemptStatus::Failed(reason) => OneDriveAuthorizationStatusDto::Failed {
                code: reason.code(),
                message: reason.message(),
            },
            AttemptStatus::Succeeded => {
                let profile = self.shared.connections.get(connection_id).await?;
                let connection_status = self.shared.connections.status(connection_id).await?;
                let last_error = self.shared.connections.last_error(connection_id).await?;
                OneDriveAuthorizationStatusDto::Succeeded {
                    connection: Box::new(crate::connection_dto::connection_dto(
                        profile,
                        connection_status,
                        last_error,
                    )),
                }
            }
        };
        Ok(OneDriveAuthorizationAttemptDto {
            id: attempt_id,
            status,
        })
    }
}

impl<R: ConnectionRepository> Shared<R> {
    fn finish_attempt(
        &self,
        attempt_id: Uuid,
        connection_id: ConnectionId,
        outcome: AttemptOutcome,
    ) {
        let status = match outcome {
            AttemptOutcome::Succeeded => AttemptStatus::Succeeded,
            AttemptOutcome::Failed(reason) => AttemptStatus::Failed(reason),
            AttemptOutcome::Cancelled => AttemptStatus::Cancelled,
        };
        let mut registry = self
            .attempts
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = registry.by_id.get_mut(&attempt_id) {
            entry.status = status;
        }
        registry.active_connections.remove(&connection_id);
        sweep(&mut registry);
    }
}

enum AttemptOutcome {
    Succeeded,
    Failed(FailureReason),
    Cancelled,
}

/// Runs one attempt's callback wait, then races the rest of it (code
/// exchange through credential persistence) against both cancellation and
/// an overall deadline (task 0110 review: "make the entire post-listener
/// attempt observe cancellation plus an overall deadline"). Always returns
/// a terminal outcome - this never panics or leaves the attempt unresolved,
/// even on an unexpected failure or a stalled peer, so a caller polling
/// never waits forever and `finish_attempt` always eventually runs,
/// releasing `active_connections`.
async fn run_attempt<R: ConnectionRepository + Send + Sync + 'static>(
    shared: &Shared<R>,
    connection_id: ConnectionId,
    listener: CallbackListener,
    state: String,
    verifier: CodeVerifier,
    cancellation: CancellationToken,
) -> AttemptOutcome {
    let redirect_uri = listener.redirect_uri().clone();
    // `listen` consumes its cancellation token by value; keep a clone so
    // the *rest* of this attempt (below) can still observe the same
    // cancellation request, not just the callback wait itself.
    let post_listener_cancellation = cancellation.clone();
    let code = match listener
        .listen(&state, shared.callback_timeout, cancellation)
        .await
    {
        Ok(code) => code,
        Err(OAuthError::Cancelled) => return AttemptOutcome::Cancelled,
        Err(OAuthError::TimedOut) => return AttemptOutcome::Failed(FailureReason::Timeout),
        Err(other) => return AttemptOutcome::Failed(failure_reason_from_oauth_error(&other)),
    };

    tokio::select! {
        biased;
        () = post_listener_cancellation.cancelled() => AttemptOutcome::Cancelled,
        outcome = complete_attempt_with_deadline(shared, connection_id, &redirect_uri, &code, &verifier) => outcome,
    }
}

/// Wraps [`complete_attempt`] with [`Shared::post_callback_deadline`] -
/// independent of, and in addition to, each individual HTTP request's own
/// timeout (defense in depth: a sequence of several individually-not-quite-
/// timed-out calls still cannot hang this attempt).
async fn complete_attempt_with_deadline<R: ConnectionRepository + Send + Sync + 'static>(
    shared: &Shared<R>,
    connection_id: ConnectionId,
    redirect_uri: &Url,
    code: &fm_auth_oauth::callback::AuthorizationCode,
    verifier: &CodeVerifier,
) -> AttemptOutcome {
    match tokio::time::timeout(
        shared.post_callback_deadline,
        complete_attempt(shared, connection_id, redirect_uri, code, verifier),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(_elapsed) => AttemptOutcome::Failed(FailureReason::Timeout),
    }
}

/// Authorization-code exchange, scope/refresh-token validation, Graph
/// identity verification and credential persistence - everything after the
/// browser callback itself completes (task 0110).
async fn complete_attempt<R: ConnectionRepository + Send + Sync + 'static>(
    shared: &Shared<R>,
    connection_id: ConnectionId,
    redirect_uri: &Url,
    code: &fm_auth_oauth::callback::AuthorizationCode,
    verifier: &CodeVerifier,
) -> AttemptOutcome {
    let tokens = match exchange_authorization_code(
        &shared.http,
        &shared.oauth,
        redirect_uri,
        code,
        verifier,
    )
    .await
    {
        Ok(tokens) => tokens,
        Err(error) => return AttemptOutcome::Failed(failure_reason_from_oauth_error(&error)),
    };

    if !scope_contains(&tokens.scope, "Files.ReadWrite")
        || !scope_contains(&tokens.scope, "User.Read")
    {
        return AttemptOutcome::Failed(FailureReason::InsufficientScope(
            "The granted permissions were missing Files.ReadWrite or User.Read. Reauthorize \
             and accept both permissions."
                .to_owned(),
        ));
    }

    // `offline_access` was requested precisely so a refresh token comes
    // back; without one the credential this attempt is about to persist
    // could never be silently renewed - the very next resolve would
    // unconditionally fail with `CredentialRequired`. Reporting `Succeeded`
    // here would be a lie, so this is a typed, actionable failure instead
    // (task 0110 review) rather than proceeding to persist an unusable
    // credential.
    let Some(refresh_token) = tokens.refresh_token.as_ref() else {
        return AttemptOutcome::Failed(FailureReason::InsufficientScope(
            "Microsoft did not grant a renewable session (offline_access). Reauthorize and \
             accept the offline access permission."
                .to_owned(),
        ));
    };

    let identity = match graph::verify_and_fetch_identity(
        &shared.http,
        &shared.graph_base_url,
        tokens.access_token.as_str(),
    )
    .await
    {
        Ok(identity) => identity,
        Err(GraphVerifyError::ConditionalAccessRequired(challenge)) => {
            let mut registry = shared
                .attempts
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            registry.pending_claims.insert(connection_id, challenge);
            return AttemptOutcome::Failed(FailureReason::ConditionalAccessRequired(None));
        }
        Err(GraphVerifyError::Unauthorized) => {
            return AttemptOutcome::Failed(FailureReason::AccessDenied(
                "Microsoft Graph rejected the newly issued access token".to_owned(),
            ));
        }
        Err(GraphVerifyError::Forbidden) => {
            return AttemptOutcome::Failed(FailureReason::TenantPolicyRejected(
                "Microsoft Graph denied access to this drive; your organization's policy may \
                 restrict this application"
                    .to_owned(),
            ));
        }
        Err(GraphVerifyError::Transport(message)) => {
            return AttemptOutcome::Failed(FailureReason::NetworkError(message));
        }
        Err(GraphVerifyError::Malformed(message)) => {
            return AttemptOutcome::Failed(FailureReason::Internal(message));
        }
    };

    let configuration = ConnectionConfiguration::OneDrive(OneDriveConnectionConfiguration {
        account_hint: identity.email.clone(),
        email: identity.email,
        display_name: identity.display_name,
        drive_type: Some(identity.drive_type),
    });
    let secret = SecretMaterial::oauth_token(
        tokens.access_token.as_str(),
        Some(refresh_token.as_str().to_owned()),
    );
    if let Err(error) = shared
        .connections
        .apply_provider_update(connection_id, Some(configuration), Some(secret))
        .await
    {
        return AttemptOutcome::Failed(FailureReason::Internal(error.to_string()));
    }

    // Prime the cache with the token just obtained so the `connect()` call
    // below - which dials through `OneDriveDialer`, which resolves a token
    // for this same connection - reuses it instead of performing an
    // immediate, redundant refresh-token rotation.
    shared.token_resolver.seed_cache(
        connection_id,
        tokens.access_token.as_str(),
        tokens.expires_in,
    );

    // Best-effort: reconciles the connection's tracked status (the same
    // path a manual "Connect" action takes) so a poller sees `Connected`
    // rather than the stale `AuthenticationRequired`/`Disconnected` default.
    // A failure here does not undo the authorization itself - the
    // credential and captured identity are already persisted - it only
    // means the next explicit connect/test surfaces whatever went wrong.
    let _ = shared.connections.connect(connection_id).await;

    AttemptOutcome::Succeeded
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use fm_auth_oauth::config::DEFAULT_SCOPES;
    use fm_auth_oauth::fixture::TokenEndpointFixture;
    use fm_connections::{
        ConnectionDraft, ConnectionError, ConnectionKind, ConnectionProfile,
        InMemoryConnectionRepository, SshAuthenticationMethod, SshConnectionConfiguration,
    };
    use fm_credentials::InMemoryCredentialStore;
    use fm_events::EventBus;
    use fm_transport_dto::{
        ConnectionConfigurationDto, ConnectionStatusDto, OneDriveAuthorizationErrorCodeDto,
        OneDriveDriveTypeDto,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::graph::fixture::GraphFixture;
    use super::*;

    /// Shares one [`InMemoryConnectionRepository`] between the
    /// [`ConnectionService`] (owned, by value) this test's
    /// [`ConnectionService`] requires and the [`OneDriveTokenResolver`]
    /// (which wants an `Arc<dyn ConnectionRepository>`) - both must see the
    /// exact same saved connection data. Local to this test module rather
    /// than a change to `fm-connections` itself, since production wiring
    /// never needs this (it simply constructs a second, independent
    /// `JsonFileConnectionRepository` pointed at the same directory,
    /// mirroring the existing `SshResolver` precedent in `service.rs`).
    struct SharedRepository(Arc<InMemoryConnectionRepository>);

    #[async_trait]
    impl ConnectionRepository for SharedRepository {
        async fn list(&self) -> Result<Vec<ConnectionProfile>, ConnectionError> {
            self.0.list().await
        }

        async fn load(
            &self,
            id: ConnectionId,
        ) -> Result<Option<ConnectionProfile>, ConnectionError> {
            self.0.load(id).await
        }

        async fn save(
            &self,
            profile: &ConnectionProfile,
        ) -> Result<ConnectionProfile, ConnectionError> {
            self.0.save(profile).await
        }

        async fn delete(&self, id: ConnectionId) -> Result<(), ConnectionError> {
            self.0.delete(id).await
        }
    }

    fn oauth_config(fixture: &TokenEndpointFixture) -> PublicClientConfig {
        PublicClientConfig {
            client_id: "test-client-id".to_owned(),
            authority: fixture.authority(),
            scopes: DEFAULT_SCOPES
                .iter()
                .map(|scope| (*scope).to_owned())
                .collect(),
        }
    }

    /// Generous default so existing tests (which never exercise the
    /// deadline itself) are never accidentally bounded by it; tests that
    /// specifically target [`Shared::post_callback_deadline`] override it
    /// via [`TestHarness::with_config`].
    const TEST_POST_CALLBACK_DEADLINE: Duration = Duration::from_secs(20);

    struct TestHarness {
        connections: Arc<ConnectionService<SharedRepository>>,
        service: OneDriveAuthorizationService<SharedRepository>,
    }

    /// Every knob [`TestHarness::with_config`] needs, so tests exercising
    /// HTTP timeouts, the overall post-callback deadline, or a stalled
    /// identity-provider authority can override just the one they care
    /// about (task 0110 review regression coverage) without duplicating
    /// the whole harness construction.
    struct TestHarnessConfig {
        oauth: PublicClientConfig,
        callback_timeout: Duration,
        post_callback_deadline: Duration,
        http: reqwest::Client,
    }

    impl TestHarnessConfig {
        fn new(token_fixture: &TokenEndpointFixture) -> Self {
            Self {
                oauth: oauth_config(token_fixture),
                callback_timeout: Duration::from_secs(30),
                post_callback_deadline: TEST_POST_CALLBACK_DEADLINE,
                http: reqwest::Client::new(),
            }
        }
    }

    impl TestHarness {
        async fn new(
            token_fixture: &TokenEndpointFixture,
            graph_fixture: &GraphFixture,
            callback_timeout: Duration,
        ) -> Self {
            Self::with_config(
                TestHarnessConfig {
                    callback_timeout,
                    ..TestHarnessConfig::new(token_fixture)
                },
                graph_fixture,
            )
            .await
        }

        async fn with_config(config: TestHarnessConfig, graph_fixture: &GraphFixture) -> Self {
            let repository = Arc::new(InMemoryConnectionRepository::new());
            let credentials: Arc<dyn fm_credentials::CredentialStore> =
                Arc::new(InMemoryCredentialStore::new());
            let oauth = config.oauth;
            let graph_base_url = graph_fixture.base_url();
            let http = config.http;

            let token_resolver = Arc::new(OneDriveTokenResolver::new(
                Arc::clone(&repository) as Arc<dyn ConnectionRepository>,
                Arc::clone(&credentials),
                oauth.clone(),
                http.clone(),
            ));

            let connections = Arc::new(
                ConnectionService::new(
                    SharedRepository(Arc::clone(&repository)),
                    Arc::clone(&credentials),
                    EventBus::new(16),
                )
                .with_dialer(
                    ConnectionKind::OneDrive,
                    Arc::new(dialer::OneDriveDialer::new(
                        Arc::clone(&token_resolver),
                        graph_base_url.clone(),
                        http.clone(),
                    )),
                ),
            );

            let service = OneDriveAuthorizationService::new(
                Arc::clone(&connections),
                token_resolver,
                OneDriveAuthorizationServiceConfig {
                    oauth,
                    graph_base_url,
                    http,
                    callback_timeout: config.callback_timeout,
                    post_callback_deadline: config.post_callback_deadline,
                },
            );

            Self {
                connections,
                service,
            }
        }

        async fn create_onedrive_connection(&self, name: &str) -> Uuid {
            let profile = self
                .connections
                .create(ConnectionDraft {
                    name: name.to_owned(),
                    kind: ConnectionKind::OneDrive,
                    configuration: ConnectionConfiguration::OneDrive(
                        OneDriveConnectionConfiguration::default(),
                    ),
                    secret: None,
                })
                .await
                .expect("create must succeed");
            profile.id.into_inner()
        }

        async fn create_ssh_connection(&self, name: &str) -> Uuid {
            let profile = self
                .connections
                .create(ConnectionDraft {
                    name: name.to_owned(),
                    kind: ConnectionKind::Ssh,
                    configuration: ConnectionConfiguration::Ssh(SshConnectionConfiguration {
                        host: "example.test".to_owned(),
                        port: 22,
                        username: "erik".to_owned(),
                        start_path: None,
                        authentication: SshAuthenticationMethod::Agent,
                        host_key_policy: fm_connections::HostKeyPolicy::PromptOnFirstUse,
                        keepalive: None,
                    }),
                    secret: None,
                })
                .await
                .expect("create must succeed");
            profile.id.into_inner()
        }
    }

    /// Simulates the system browser's redirect back to the loopback
    /// callback listener, extracting `redirect_uri`/`state` straight out of
    /// the authorization URL exactly like a real browser would carry them
    /// through unmodified.
    async fn simulate_browser_callback(
        authorization_url: &str,
        code: Option<&str>,
        error: Option<(&str, &str)>,
    ) {
        let parsed = Url::parse(authorization_url).expect("valid authorization url");
        let redirect_uri = parsed
            .query_pairs()
            .find(|(key, _)| key == "redirect_uri")
            .map(|(_, value)| value.into_owned())
            .expect("authorization url carries redirect_uri");
        let state = parsed
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned())
            .expect("authorization url carries state");
        let redirect_url = Url::parse(&redirect_uri).expect("valid redirect uri");

        let mut target = Url::parse("http://localhost/").expect("valid dummy base");
        target.query_pairs_mut().append_pair("state", &state);
        if let Some(code) = code {
            target.query_pairs_mut().append_pair("code", code);
        }
        if let Some((error, description)) = error {
            target
                .query_pairs_mut()
                .append_pair("error", error)
                .append_pair("error_description", description);
        }
        let path_and_query = format!(
            "{}{}",
            target.path(),
            target
                .query()
                .map(|query| format!("?{query}"))
                .unwrap_or_default()
        );

        let addr = format!(
            "{}:{}",
            redirect_url.host_str().expect("redirect uri has a host"),
            redirect_url.port().expect("redirect uri has a port")
        );
        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect to the loopback callback listener");
        stream
            .write_all(
                format!("GET {path_and_query} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes(),
            )
            .await
            .expect("write the callback request");
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response).await;
    }

    async fn await_terminal(
        service: &OneDriveAuthorizationService<SharedRepository>,
        attempt_id: Uuid,
    ) -> OneDriveAuthorizationAttemptDto {
        for _ in 0..300 {
            let attempt = service
                .attempt_status(attempt_id)
                .await
                .expect("attempt must be known");
            if !matches!(attempt.status, OneDriveAuthorizationStatusDto::Pending) {
                return attempt;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("attempt {attempt_id} never reached a terminal status in time");
    }

    async fn assert_succeeds_with_drive_type(
        drive_type_wire: &str,
        expected: OneDriveDriveTypeDto,
    ) {
        let token_fixture = TokenEndpointFixture::start().await;
        token_fixture
            .enqueue_success("issued-access-token", Some("issued-refresh-token"), 3600)
            .await;
        let graph_fixture = GraphFixture::start().await;
        graph_fixture
            .enqueue_json(
                200,
                serde_json::json!({ "mail": "erik@example.test", "displayName": "Erik Vullings" }),
            )
            .await;
        graph_fixture
            .enqueue_json(200, serde_json::json!({ "driveType": drive_type_wire }))
            .await;
        // The dialer's own re-verification triggered by `connect()`.
        graph_fixture
            .enqueue_json(200, serde_json::json!({ "driveType": drive_type_wire }))
            .await;

        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(30)).await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;

        let begin = harness
            .service
            .begin_authorization(connection_id)
            .await
            .expect("begin must succeed");
        assert!(begin.authorization_url.contains("oauth2/v2.0/authorize"));
        assert!(begin.authorization_url.contains("client_id=test-client-id"));
        assert!(!begin.authorization_url.contains("client_secret"));

        simulate_browser_callback(
            &begin.authorization_url,
            Some("fake-authorization-code"),
            None,
        )
        .await;

        let attempt = await_terminal(&harness.service, begin.attempt_id).await;
        let OneDriveAuthorizationStatusDto::Succeeded { connection } = attempt.status else {
            panic!("expected success, got {:?}", attempt.status);
        };
        assert!(connection.has_credential);
        assert_eq!(connection.status, ConnectionStatusDto::Connected);
        let ConnectionConfigurationDto::OneDrive(configuration) = connection.configuration else {
            panic!("expected a OneDrive configuration");
        };
        assert_eq!(configuration.email.as_deref(), Some("erik@example.test"));
        assert_eq!(configuration.display_name.as_deref(), Some("Erik Vullings"));
        assert_eq!(configuration.drive_type, Some(expected));

        let requests = token_fixture.requests().await;
        assert_eq!(requests.len(), 1, "exactly one code-exchange request");
        assert!(!requests[0].contains("client_secret"));
    }

    #[tokio::test]
    async fn begin_authorization_succeeds_end_to_end_for_a_personal_account() {
        assert_succeeds_with_drive_type("personal", OneDriveDriveTypeDto::Personal).await;
    }

    #[tokio::test]
    async fn begin_authorization_succeeds_end_to_end_for_a_business_account() {
        assert_succeeds_with_drive_type("business", OneDriveDriveTypeDto::Business).await;
    }

    #[tokio::test]
    async fn begin_authorization_fails_when_the_granted_scope_is_missing_files_read_write() {
        let token_fixture = TokenEndpointFixture::start().await;
        token_fixture
            .enqueue_raw(
                200,
                r#"{"access_token":"issued-access-token","refresh_token":"issued-refresh-token","expires_in":3600,"token_type":"Bearer","scope":"offline_access User.Read"}"#,
            )
            .await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(30)).await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;

        let begin = harness
            .service
            .begin_authorization(connection_id)
            .await
            .unwrap();
        simulate_browser_callback(&begin.authorization_url, Some("fake-code"), None).await;

        let attempt = await_terminal(&harness.service, begin.attempt_id).await;
        assert_eq!(
            attempt.status,
            OneDriveAuthorizationStatusDto::Failed {
                code: OneDriveAuthorizationErrorCodeDto::InsufficientScope,
                message: "The granted permissions were missing Files.ReadWrite or User.Read. \
                          Reauthorize and accept both permissions."
                    .to_owned(),
            }
        );
        // Missing scope is detected from the token response itself - Graph
        // is never even called.
        assert!(graph_fixture.requests().await.is_empty());
    }

    #[tokio::test]
    async fn begin_authorization_fails_when_the_code_exchange_omits_a_refresh_token() {
        // `offline_access` was requested precisely to get a refresh token
        // back; if the identity provider's response omits one anyway, this
        // must never report `Succeeded` with a credential that the very
        // next resolve would find unusable (task 0110 review).
        let token_fixture = TokenEndpointFixture::start().await;
        token_fixture
            .enqueue_raw(
                200,
                r#"{"access_token":"issued-access-token","expires_in":3600,"token_type":"Bearer","scope":"offline_access Files.ReadWrite User.Read"}"#,
            )
            .await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(30)).await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;

        let begin = harness
            .service
            .begin_authorization(connection_id)
            .await
            .unwrap();
        simulate_browser_callback(&begin.authorization_url, Some("fake-code"), None).await;

        let attempt = await_terminal(&harness.service, begin.attempt_id).await;
        assert_eq!(
            attempt.status,
            OneDriveAuthorizationStatusDto::Failed {
                code: OneDriveAuthorizationErrorCodeDto::InsufficientScope,
                message: "Microsoft did not grant a renewable session (offline_access). \
                          Reauthorize and accept the offline access permission."
                    .to_owned(),
            }
        );
        // Never even reaches Graph verification or persists anything.
        assert!(graph_fixture.requests().await.is_empty());
        let connection = harness.connections.get(connection_id.into()).await.unwrap();
        assert!(
            connection.credential_ref.is_none(),
            "a failed attempt must never leave behind a persisted, unusable credential"
        );
    }

    #[tokio::test]
    async fn begin_authorization_fails_with_invalid_grant_when_the_provider_rejects_the_code() {
        let token_fixture = TokenEndpointFixture::start().await;
        token_fixture
            .enqueue_error(
                400,
                "invalid_grant",
                "AADSTS70008: expired authorization code",
            )
            .await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(30)).await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;

        let begin = harness
            .service
            .begin_authorization(connection_id)
            .await
            .unwrap();
        simulate_browser_callback(&begin.authorization_url, Some("fake-code"), None).await;

        let attempt = await_terminal(&harness.service, begin.attempt_id).await;
        let OneDriveAuthorizationStatusDto::Failed { code, message } = attempt.status else {
            panic!("expected failure, got {:?}", attempt.status);
        };
        assert_eq!(code, OneDriveAuthorizationErrorCodeDto::InvalidGrant);
        assert!(message.contains("AADSTS70008"));
        assert_eq!(token_fixture.requests().await.len(), 1, "no silent retry");
    }

    #[tokio::test]
    async fn begin_authorization_fails_with_tenant_policy_rejected_on_admin_consent_required() {
        let token_fixture = TokenEndpointFixture::start().await;
        token_fixture
            .enqueue_error(400, "invalid_grant", "AADSTS90094: admin consent required")
            .await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(30)).await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;

        let begin = harness
            .service
            .begin_authorization(connection_id)
            .await
            .unwrap();
        simulate_browser_callback(&begin.authorization_url, Some("fake-code"), None).await;

        // `invalid_grant` classifies ahead of the AADSTS-code sniffing that
        // only applies to `access_denied` (see `fm_auth_oauth::error`), so
        // this still surfaces as `InvalidGrant` - the important behaviour
        // under test is that the safe description is preserved verbatim.
        let attempt = await_terminal(&harness.service, begin.attempt_id).await;
        let OneDriveAuthorizationStatusDto::Failed { message, .. } = attempt.status else {
            panic!("expected failure");
        };
        assert!(message.contains("AADSTS90094"));
    }

    #[tokio::test]
    async fn begin_authorization_fails_with_conditional_access_and_a_fresh_challenged_replay_follows()
     {
        let token_fixture = TokenEndpointFixture::start().await;
        token_fixture
            .enqueue_success("issued-access-token", Some("issued-refresh-token"), 3600)
            .await;
        let graph_fixture = GraphFixture::start().await;
        graph_fixture
            .enqueue_json(200, serde_json::json!({ "mail": "erik@example.test" }))
            .await;
        let raw_claims = {
            use base64::Engine as _;
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                r#"{"access_token":{"nbf":{"essential":true,"value":"161234"}}}"#.as_bytes(),
            )
        };
        graph_fixture
            .enqueue_conditional_access_challenge(403, &raw_claims)
            .await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(30)).await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;

        let first = harness
            .service
            .begin_authorization(connection_id)
            .await
            .unwrap();
        simulate_browser_callback(&first.authorization_url, Some("fake-code"), None).await;
        let attempt = await_terminal(&harness.service, first.attempt_id).await;
        assert_eq!(
            attempt.status,
            OneDriveAuthorizationStatusDto::Failed {
                code: OneDriveAuthorizationErrorCodeDto::ConditionalAccessRequired,
                message: "Microsoft requires additional verification (Conditional Access) before \
                          granting access. Reauthorize to continue."
                    .to_owned(),
            }
        );

        // The connection is unblocked again (the previous attempt finished),
        // and beginning again automatically replays with the claims
        // challenge merged in, using fresh state/PKCE.
        let second = harness
            .service
            .begin_authorization(connection_id)
            .await
            .expect("a fresh attempt must be allowed after the previous one finished");
        let first_url = Url::parse(&first.authorization_url).unwrap();
        let second_url = Url::parse(&second.authorization_url).unwrap();
        let param = |url: &Url, key: &str| {
            url.query_pairs()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.into_owned())
        };
        assert_ne!(param(&first_url, "state"), param(&second_url, "state"));
        assert_ne!(
            param(&first_url, "code_challenge"),
            param(&second_url, "code_challenge")
        );
        let claims: serde_json::Value =
            serde_json::from_str(&param(&second_url, "claims").expect("claims present"))
                .expect("claims parameter is valid JSON");
        assert_eq!(claims["access_token"]["nbf"]["value"], "161234");
        assert_eq!(claims["access_token"]["xms_cc"]["values"][0], "cp1");
        assert!(!second.authorization_url.contains(&raw_claims));
    }

    #[tokio::test]
    async fn begin_authorization_times_out_when_the_browser_never_completes_sign_in() {
        let token_fixture = TokenEndpointFixture::start().await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_millis(50)).await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;

        let begin = harness
            .service
            .begin_authorization(connection_id)
            .await
            .unwrap();
        let attempt = await_terminal(&harness.service, begin.attempt_id).await;

        assert_eq!(
            attempt.status,
            OneDriveAuthorizationStatusDto::Failed {
                code: OneDriveAuthorizationErrorCodeDto::Timeout,
                message: "Sign-in was not completed in time.".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn cancel_authorization_stops_a_pending_attempt() {
        let token_fixture = TokenEndpointFixture::start().await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(300)).await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;

        let begin = harness
            .service
            .begin_authorization(connection_id)
            .await
            .unwrap();
        let cancelled = harness
            .service
            .cancel_authorization(begin.attempt_id)
            .await
            .expect("cancel must succeed");
        // `cancel` itself may still observe `Pending` momentarily (the
        // background task notices the token asynchronously); poll for the
        // terminal state.
        let _ = cancelled;
        let attempt = await_terminal(&harness.service, begin.attempt_id).await;
        assert_eq!(attempt.status, OneDriveAuthorizationStatusDto::Cancelled);
    }

    #[tokio::test]
    async fn cancel_authorization_is_idempotent_for_an_already_terminal_attempt() {
        let token_fixture = TokenEndpointFixture::start().await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(300)).await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;
        let begin = harness
            .service
            .begin_authorization(connection_id)
            .await
            .unwrap();
        harness
            .service
            .cancel_authorization(begin.attempt_id)
            .await
            .unwrap();
        let _ = await_terminal(&harness.service, begin.attempt_id).await;

        let second_cancel = harness
            .service
            .cancel_authorization(begin.attempt_id)
            .await
            .expect("cancelling an already-cancelled attempt is a no-op, not an error");

        assert_eq!(
            second_cancel.status,
            OneDriveAuthorizationStatusDto::Cancelled
        );
    }

    /// Builds a [`TestHarnessConfig`] whose OAuth authority is a
    /// [`test_support::StalledServer`] that accepts a connection and then
    /// never responds - used by the two regression tests below to prove
    /// that a stalled post-callback exchange is bounded by cancellation and
    /// by [`Shared::post_callback_deadline`] respectively, never by waiting
    /// on the peer itself (task 0110 review finding 3).
    fn stalled_authority_config(
        stalled: &test_support::StalledServer,
        post_callback_deadline: Duration,
    ) -> TestHarnessConfig {
        TestHarnessConfig {
            oauth: PublicClientConfig {
                client_id: "test-client-id".to_owned(),
                authority: fm_auth_oauth::config::Authority::from_base_url(stalled.base_url()),
                scopes: DEFAULT_SCOPES
                    .iter()
                    .map(|scope| (*scope).to_owned())
                    .collect(),
            },
            // Deliberately generous: the HTTP client's own per-request
            // timeout must never be what resolves these tests - only
            // cancellation (first test) or `post_callback_deadline` (second
            // test) should.
            callback_timeout: Duration::from_secs(300),
            post_callback_deadline,
            http: build_http_client(Duration::from_secs(300), Duration::from_secs(300)),
        }
    }

    #[tokio::test]
    async fn cancelling_a_stalled_code_exchange_reaches_cancelled_quickly_and_releases_the_connection()
     {
        let stalled = test_support::StalledServer::start().await;
        let graph_fixture = GraphFixture::start().await;
        let harness = TestHarness::with_config(
            stalled_authority_config(&stalled, Duration::from_secs(300)),
            &graph_fixture,
        )
        .await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;

        let begin = harness
            .service
            .begin_authorization(connection_id)
            .await
            .unwrap();
        simulate_browser_callback(&begin.authorization_url, Some("fake-code"), None).await;
        // Give the background task a moment to actually reach the stalled
        // code-exchange call before cancelling it.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let started = std::time::Instant::now();
        harness
            .service
            .cancel_authorization(begin.attempt_id)
            .await
            .expect("cancel must succeed even while the code exchange is stalled");
        let attempt = await_terminal(&harness.service, begin.attempt_id).await;

        assert_eq!(attempt.status, OneDriveAuthorizationStatusDto::Cancelled);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "cancellation must win the race against a stalled peer almost immediately, not wait \
             for the peer's ~300s HTTP timeout or the ~300s post_callback_deadline; took {:?}",
            started.elapsed()
        );

        // `active_connections` must have been released by `finish_attempt`
        // - a fresh attempt for the very same connection must be accepted,
        // not rejected as "already in progress".
        let second_begin = harness.service.begin_authorization(connection_id).await;
        assert!(
            second_begin.is_ok(),
            "active_connections must be released after cancellation, got {second_begin:?}"
        );
    }

    #[tokio::test]
    async fn a_stalled_code_exchange_is_bounded_by_the_overall_post_callback_deadline() {
        let stalled = test_support::StalledServer::start().await;
        let graph_fixture = GraphFixture::start().await;
        let harness = TestHarness::with_config(
            stalled_authority_config(&stalled, Duration::from_millis(200)),
            &graph_fixture,
        )
        .await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;

        let begin = harness
            .service
            .begin_authorization(connection_id)
            .await
            .unwrap();
        let started = std::time::Instant::now();
        simulate_browser_callback(&begin.authorization_url, Some("fake-code"), None).await;

        let attempt = await_terminal(&harness.service, begin.attempt_id).await;

        assert_eq!(
            attempt.status,
            OneDriveAuthorizationStatusDto::Failed {
                code: OneDriveAuthorizationErrorCodeDto::Timeout,
                message: "Sign-in was not completed in time.".to_owned(),
            }
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the ~200ms post_callback_deadline - not the ~300s HTTP client timeout - must be \
             what bounds a stalled post-callback sequence with no cancellation in play; took \
             {:?}",
            started.elapsed()
        );

        // `active_connections` must have been released by `finish_attempt`
        // here too, not only on the cancellation path.
        let second_begin = harness.service.begin_authorization(connection_id).await;
        assert!(
            second_begin.is_ok(),
            "active_connections must be released after a deadline failure, got {second_begin:?}"
        );
    }

    #[tokio::test]
    async fn a_second_concurrent_attempt_for_the_same_connection_is_rejected() {
        let token_fixture = TokenEndpointFixture::start().await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(300)).await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;

        let first = harness.service.begin_authorization(connection_id).await;
        assert!(first.is_ok());

        let second = harness.service.begin_authorization(connection_id).await;

        assert!(matches!(second, Err(ApplicationError::InvalidRequest(_))));
    }

    #[tokio::test]
    async fn different_connections_never_block_each_other() {
        let token_fixture = TokenEndpointFixture::start().await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(300)).await;
        let connection_a = harness.create_onedrive_connection("Account A").await;
        let connection_b = harness.create_onedrive_connection("Account B").await;

        let begin_a = harness.service.begin_authorization(connection_a).await;
        let begin_b = harness.service.begin_authorization(connection_b).await;

        assert!(begin_a.is_ok());
        assert!(begin_b.is_ok());
    }

    #[tokio::test]
    async fn begin_authorization_reports_not_found_for_an_unknown_connection() {
        let token_fixture = TokenEndpointFixture::start().await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(30)).await;

        let error = harness
            .service
            .begin_authorization(Uuid::new_v4())
            .await
            .unwrap_err();

        assert!(matches!(error, ApplicationError::NotFound));
    }

    #[tokio::test]
    async fn begin_authorization_rejects_a_non_onedrive_connection() {
        let token_fixture = TokenEndpointFixture::start().await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(30)).await;
        let ssh_id = harness.create_ssh_connection("Home Server").await;

        let error = harness
            .service
            .begin_authorization(ssh_id)
            .await
            .unwrap_err();

        assert!(
            matches!(error, ApplicationError::InvalidRequest(message) if message.contains("not a OneDrive connection"))
        );
    }

    #[tokio::test]
    async fn attempt_status_reports_not_found_for_an_unknown_attempt() {
        let token_fixture = TokenEndpointFixture::start().await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(30)).await;

        let error = harness
            .service
            .attempt_status(Uuid::new_v4())
            .await
            .unwrap_err();

        assert!(matches!(error, ApplicationError::NotFound));
    }

    #[tokio::test]
    async fn cancel_authorization_reports_not_found_for_an_unknown_attempt() {
        let token_fixture = TokenEndpointFixture::start().await;
        let graph_fixture = GraphFixture::start().await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(30)).await;

        let error = harness
            .service
            .cancel_authorization(Uuid::new_v4())
            .await
            .unwrap_err();

        assert!(matches!(error, ApplicationError::NotFound));
    }

    #[tokio::test]
    async fn a_successful_attempt_never_leaks_the_access_or_refresh_token_through_the_dto() {
        let token_fixture = TokenEndpointFixture::start().await;
        token_fixture
            .enqueue_success(
                "planted-access-secret",
                Some("planted-refresh-secret"),
                3600,
            )
            .await;
        let graph_fixture = GraphFixture::start().await;
        graph_fixture
            .enqueue_json(200, serde_json::json!({ "mail": "erik@example.test" }))
            .await;
        graph_fixture
            .enqueue_json(200, serde_json::json!({ "driveType": "personal" }))
            .await;
        graph_fixture
            .enqueue_json(200, serde_json::json!({ "driveType": "personal" }))
            .await;
        let harness =
            TestHarness::new(&token_fixture, &graph_fixture, Duration::from_secs(30)).await;
        let connection_id = harness.create_onedrive_connection("My OneDrive").await;

        let begin = harness
            .service
            .begin_authorization(connection_id)
            .await
            .unwrap();
        assert!(!begin.authorization_url.contains("planted-access-secret"));
        assert!(!begin.authorization_url.contains("planted-refresh-secret"));
        simulate_browser_callback(&begin.authorization_url, Some("fake-code"), None).await;
        let attempt = await_terminal(&harness.service, begin.attempt_id).await;

        let serialized = serde_json::to_string(&attempt).expect("attempt serializes");
        assert!(!serialized.contains("planted-access-secret"));
        assert!(!serialized.contains("planted-refresh-secret"));
        let debugged = format!("{attempt:?}");
        assert!(!debugged.contains("planted-access-secret"));
        assert!(!debugged.contains("planted-refresh-secret"));
    }

    #[test]
    fn sweep_evicts_expired_terminal_attempts_but_keeps_pending_ones() {
        let mut registry = AttemptRegistry::default();
        let old_terminal = Uuid::new_v4();
        registry.by_id.insert(
            old_terminal,
            AttemptEntry {
                connection_id: ConnectionId::new(),
                cancellation: CancellationToken::new(),
                status: AttemptStatus::Succeeded,
                created_at: Instant::now() - TERMINAL_ATTEMPT_RETENTION - Duration::from_secs(1),
            },
        );
        let old_pending = Uuid::new_v4();
        registry.by_id.insert(
            old_pending,
            AttemptEntry {
                connection_id: ConnectionId::new(),
                cancellation: CancellationToken::new(),
                status: AttemptStatus::Pending,
                created_at: Instant::now() - TERMINAL_ATTEMPT_RETENTION - Duration::from_secs(1),
            },
        );

        sweep(&mut registry);

        assert!(!registry.by_id.contains_key(&old_terminal));
        assert!(
            registry.by_id.contains_key(&old_pending),
            "a pending attempt must never be evicted by age"
        );
    }

    #[test]
    fn sweep_bounds_the_number_of_retained_terminal_attempts() {
        let mut registry = AttemptRegistry::default();
        for _ in 0..(MAX_RETAINED_TERMINAL_ATTEMPTS + 10) {
            registry.by_id.insert(
                Uuid::new_v4(),
                AttemptEntry {
                    connection_id: ConnectionId::new(),
                    cancellation: CancellationToken::new(),
                    status: AttemptStatus::Cancelled,
                    created_at: Instant::now(),
                },
            );
        }

        sweep(&mut registry);

        assert_eq!(registry.by_id.len(), MAX_RETAINED_TERMINAL_ATTEMPTS);
    }

    #[test]
    fn sweep_never_evicts_pending_attempts_to_stay_within_the_retained_count() {
        let mut registry = AttemptRegistry::default();
        for _ in 0..(MAX_RETAINED_TERMINAL_ATTEMPTS + 10) {
            registry.by_id.insert(
                Uuid::new_v4(),
                AttemptEntry {
                    connection_id: ConnectionId::new(),
                    cancellation: CancellationToken::new(),
                    status: AttemptStatus::Pending,
                    created_at: Instant::now(),
                },
            );
        }

        sweep(&mut registry);

        assert_eq!(registry.by_id.len(), MAX_RETAINED_TERMINAL_ATTEMPTS + 10);
    }

    #[test]
    fn scope_contains_matches_case_insensitively() {
        assert!(scope_contains(
            "offline_access files.readwrite user.read",
            "Files.ReadWrite"
        ));
        assert!(scope_contains(
            "offline_access Files.ReadWrite User.Read",
            "user.read"
        ));
        assert!(!scope_contains(
            "offline_access User.Read",
            "Files.ReadWrite"
        ));
    }

    #[test]
    fn production_config_targets_the_real_microsoft_endpoints_with_the_public_client_id_and_no_secret()
     {
        assert_eq!(ONEDRIVE_CLIENT_ID, "9b01b729-5908-492b-bcd1-32b4a36096de");
        let config = OneDriveAuthorizationServiceConfig::production();

        assert_eq!(config.oauth.client_id, ONEDRIVE_CLIENT_ID);
        assert_eq!(
            config.oauth.authority.authorize_endpoint().as_str(),
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize"
        );
        assert_eq!(
            config.oauth.scopes,
            ["offline_access", "Files.ReadWrite", "User.Read"]
        );
        assert_eq!(
            config.graph_base_url.as_str(),
            "https://graph.microsoft.com/v1.0"
        );
    }
}
