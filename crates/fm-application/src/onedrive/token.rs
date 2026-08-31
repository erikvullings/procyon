//! [`OneDriveTokenResolver`]: the seam wiring saved OneDrive connections and
//! their stored OAuth credential into a currently valid Microsoft Graph
//! bearer token (task 0110). Implements
//! [`fm_vfs_onedrive::OneDriveConnectionResolver`] (the provider's own token
//! seam) and is also used directly by [`super::dialer::OneDriveDialer`] and
//! [`super::OneDriveAuthorizationService`]'s attempt-completion Graph
//! verification - one cache, one refresh path, shared by browsing,
//! connect/test, and authorization completion alike.
//!
//! Mirrors `fm-application`'s `S3Resolver`/`WebDavResolver`: it holds its own
//! [`ConnectionRepository`] handle rather than depending on
//! [`fm_connections::ConnectionService`], so `FileManagerService`'s
//! construction never has to break a cycle between "the dialer/resolver
//! needs the connection service" and "the connection service registers the
//! dialer".
//!
//! ## Cold-start safety
//!
//! The in-memory cache never survives a process restart, and a stored
//! access token is never trusted directly on a cache miss - even though
//! [`fm_credentials::SecretMaterial::OAuthToken`] also carries one. Microsoft
//! identity platform access tokens are typically valid under an hour, and
//! this resolver has no way to know how much of that a *persisted* access
//! token had left after however long the process was not running; trusting
//! it blindly risks handing Microsoft Graph a token that looks well-formed
//! but has already expired, producing a confusing mid-request 401 instead of
//! a clean, upfront refresh. So every cache miss - cold start or ordinary
//! expiry alike - always redeems the stored *refresh* token for a fresh pair
//! first.
//!
//! ## Refresh-token rotation and reauthorization
//!
//! Microsoft identity platform rotates the refresh token on every use; a
//! successful refresh always atomically replaces the stored credential
//! (store new, save, then best-effort delete the predecessor - task 0110).
//! If a response omits a rotated `refresh_token` (RFC 6749 §6 permits this;
//! the client may treat a still-omitted refresh token as still valid), the
//! previous one is carried forward rather than silently persisting a
//! credential with no refresh token at all. A refresh rejected by the
//! identity provider as an auth failure (`invalid_grant`,
//! `interaction_required`, `access_denied`, tenant policy, Conditional
//! Access) is treated as terminal: the stored credential is deleted so the
//! connection's tracked status becomes `AuthenticationRequired` on the next
//! connect/test, rather than repeatedly retrying a refresh token the
//! provider has already rejected. A bare transport/parsing failure, or a
//! provider-reported `temporarily_unavailable`/`server_error` (RFC 6749
//! §5.2 - the *server*, not this refresh token, is the problem), is treated
//! as transient and never destroys the stored credential - it may simply
//! succeed next time. Every persisting write re-loads the connection
//! immediately beforehand rather than reusing an earlier snapshot, so a
//! concurrent metadata update (a rename, or the authorization flow
//! capturing freshly verified identity) is never clobbered, and clearing a
//! credential only proceeds if the connection still references the exact
//! credential just determined to be dead - a concurrent, newer
//! reauthorization is never undone.
//!
//! ## Concurrency
//!
//! Refreshes are serialized per connection id, never globally: concurrent
//! callers resolving the *same* connection queue behind one in-flight
//! refresh and then observe its result from cache (the cache is checked
//! again immediately after acquiring the per-connection lock), so Microsoft
//! identity platform's rotate-on-use contract is never raced into
//! invalidating a refresh token this process itself just received.
//! Different connections refresh fully concurrently - each gets its own
//! lock.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fm_auth_oauth::config::PublicClientConfig;
use fm_auth_oauth::error::OAuthError;
use fm_auth_oauth::token::{TokenResponse, refresh_access_token};
use fm_connections::{
    ConnectionConfiguration, ConnectionId, ConnectionProfile, ConnectionRepository,
};
use fm_credentials::{CredentialRef, CredentialStore, SecretMaterial, StoreCredentialRequest};
use fm_vfs::VfsError;
use fm_vfs_onedrive::{OneDriveAccessToken, OneDriveConnectionResolver};
use tokio::sync::Mutex as AsyncMutex;
use zeroize::Zeroizing;

/// Safety margin subtracted from a token's reported `expires_in` before
/// treating it as due for renewal, guarding against clock skew and the
/// small delay between resolving a token and actually using it.
const EXPIRY_SAFETY_MARGIN: Duration = Duration::from_secs(60);

struct CachedToken {
    access_token: Zeroizing<String>,
    expires_at: Instant,
}

impl CachedToken {
    fn is_usable(&self) -> bool {
        Instant::now() < self.expires_at
    }
}

/// Per-connection serialization: one async lock per connection id, created
/// lazily and kept for the process lifetime (bounded in practice by the
/// number of saved OneDrive connections, never unbounded churn).
#[derive(Default)]
struct ConnectionSlot {
    refresh_lock: AsyncMutex<()>,
}

pub(crate) struct OneDriveTokenResolver {
    repository: Arc<dyn ConnectionRepository>,
    credentials: Arc<dyn CredentialStore>,
    oauth: PublicClientConfig,
    http: reqwest::Client,
    cache: Mutex<HashMap<ConnectionId, CachedToken>>,
    slots: Mutex<HashMap<ConnectionId, Arc<ConnectionSlot>>>,
}

impl OneDriveTokenResolver {
    pub(crate) fn new(
        repository: Arc<dyn ConnectionRepository>,
        credentials: Arc<dyn CredentialStore>,
        oauth: PublicClientConfig,
        http: reqwest::Client,
    ) -> Self {
        Self {
            repository,
            credentials,
            oauth,
            http,
            cache: Mutex::new(HashMap::new()),
            slots: Mutex::new(HashMap::new()),
        }
    }

    /// Primes the cache with a freshly obtained access token, so a caller
    /// that already holds one (the authorization flow, immediately after
    /// its own authorization-code exchange) does not force an immediate,
    /// redundant refresh - and a second refresh-token rotation - the moment
    /// something else (e.g. reconciling connection status via `connect`)
    /// calls [`OneDriveConnectionResolver::resolve`] for the same
    /// connection right afterward.
    pub(crate) fn seed_cache(&self, id: ConnectionId, access_token: &str, expires_in: Duration) {
        self.cache_token(id, access_token, expires_in);
    }

    fn slot_for(&self, id: ConnectionId) -> Arc<ConnectionSlot> {
        let mut slots = self.slots.lock().unwrap_or_else(|error| error.into_inner());
        Arc::clone(
            slots
                .entry(id)
                .or_insert_with(|| Arc::new(ConnectionSlot::default())),
        )
    }

    fn cached(&self, id: ConnectionId) -> Option<OneDriveAccessToken> {
        let cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
        cache
            .get(&id)
            .filter(|token| token.is_usable())
            .map(|token| OneDriveAccessToken::new(token.access_token.as_str().to_owned()))
    }

    fn cache_token(&self, id: ConnectionId, access_token: &str, expires_in: Duration) {
        let expires_at = Instant::now() + expires_in.saturating_sub(EXPIRY_SAFETY_MARGIN);
        let mut cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
        cache.insert(
            id,
            CachedToken {
                access_token: Zeroizing::new(access_token.to_owned()),
                expires_at,
            },
        );
    }

    fn invalidate(&self, id: ConnectionId) {
        self.cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&id);
    }

    async fn refresh_and_cache(&self, id: ConnectionId) -> Result<OneDriveAccessToken, VfsError> {
        let location = format!("onedrive://{id}/");
        let profile = self
            .repository
            .load(id)
            .await
            .map_err(|error| VfsError::Io {
                message: error.to_string(),
            })?
            .ok_or_else(|| VfsError::NotFound {
                location: location.clone(),
            })?;
        if !matches!(profile.configuration, ConnectionConfiguration::OneDrive(_)) {
            return Err(VfsError::InvalidLocation { location });
        }
        let Some(reference) = profile.credential_ref else {
            return Err(VfsError::CredentialRequired);
        };
        let resolved = self
            .credentials
            .resolve(&reference)
            .await
            .map_err(|_| VfsError::CredentialRequired)?;
        let SecretMaterial::OAuthToken {
            refresh_token: Some(refresh_token),
            ..
        } = &resolved.secret
        else {
            return Err(VfsError::CredentialRequired);
        };

        match refresh_access_token(&self.http, &self.oauth, refresh_token.as_str()).await {
            Ok(tokens) => {
                let access_token_text = tokens.access_token.as_str().to_owned();
                let expires_in = tokens.expires_in;
                self.rotate_credential(id, &profile, reference, refresh_token.as_str(), &tokens)
                    .await?;
                self.cache_token(id, &access_token_text, expires_in);
                Ok(OneDriveAccessToken::new(access_token_text))
            }
            Err(error) if is_terminal_refresh_failure(&error) => {
                self.clear_credential(id, reference).await;
                self.invalidate(id);
                Err(VfsError::CredentialRequired)
            }
            Err(_transient) => {
                // The stored refresh token itself was never rejected - this
                // is a transport/parsing failure or a provider-side
                // transient condition (`temporarily_unavailable`,
                // `server_error`) - so it must not be discarded; the next
                // attempt may simply succeed.
                Err(VfsError::Io {
                    message: "failed to refresh the OneDrive access token".to_owned(),
                })
            }
        }
    }

    /// Atomically-as-far-as-possible replaces the connection's credential
    /// with the rotated token pair: stores the new secret and persists its
    /// reference *before* best-effort deleting whatever secret it
    /// superseded, mirroring
    /// `fm_connections::ConnectionService::apply_provider_update`'s
    /// sequencing exactly (a transient failure deleting the predecessor
    /// must never leave the connection without its just-issued, already
    /// usable, new credential).
    ///
    /// Re-loads the connection immediately before persisting rather than
    /// reusing `loaded_profile` (the snapshot `refresh_and_cache` read
    /// before the network round-trip to the identity provider): a
    /// concurrent update - renaming the connection, or the authorization
    /// flow capturing freshly verified account identity/`driveType` via
    /// `ConnectionService::apply_provider_update` - that landed while that
    /// round-trip was in flight must not be clobbered by writing back
    /// stale field values (task 0110 review). The credential actually
    /// superseded is likewise whatever the fresh reload's `credential_ref`
    /// turns out to be, not necessarily `previous`, so a concurrent writer
    /// that already replaced it is never orphaned or double-handled
    /// incorrectly.
    ///
    /// If the identity provider's response omitted a rotated
    /// `refresh_token` (allowed by RFC 6749 §6 - the client "can be
    /// reasonably confident" a still-omitted refresh token remains valid),
    /// `previous_refresh_token` is carried forward unchanged instead of
    /// silently persisting a credential with no refresh token at all,
    /// which would otherwise make the *next* resolve unconditionally fail
    /// (task 0110 review).
    ///
    /// If that fresh reload instead finds the connection gone entirely
    /// (deleted while the refresh's network round-trip to the identity
    /// provider was in flight), this must not fall back to the stale
    /// `loaded_profile` snapshot and save it: `save` upserts by id on every
    /// `ConnectionRepository` implementation this module knows of, so doing
    /// so would resurrect a deliberately deleted connection, attach the
    /// just-rotated credential to it, and silently undo the deletion (task
    /// 0110 review). Instead the now-orphaned new secret - nothing else
    /// will ever reference it - is best-effort deleted and this reports
    /// [`VfsError::NotFound`], matching the identical check
    /// `refresh_and_cache` already performs when the connection is missing
    /// from the very first load. The predecessor credential's fate is left
    /// entirely to whatever concurrently deleted the connection - that
    /// flow, not this one, owns cleaning it up.
    async fn rotate_credential(
        &self,
        id: ConnectionId,
        loaded_profile: &ConnectionProfile,
        previous: CredentialRef,
        previous_refresh_token: &str,
        tokens: &TokenResponse,
    ) -> Result<(), VfsError> {
        let refresh_token = tokens
            .refresh_token
            .as_ref()
            .map(|value| value.as_str().to_owned())
            .unwrap_or_else(|| previous_refresh_token.to_owned());
        let secret = SecretMaterial::oauth_token(tokens.access_token.as_str(), Some(refresh_token));
        let new_reference = self
            .credentials
            .store(StoreCredentialRequest::new(
                loaded_profile.name.clone(),
                secret,
            ))
            .await
            .map_err(|error| VfsError::Io {
                message: error.to_string(),
            })?;

        let mut current = match self.repository.load(id).await {
            Ok(Some(profile)) => profile,
            Ok(None) => {
                // The connection was deleted while the refresh's network
                // round-trip was in flight. There is nothing left to
                // attach the just-rotated credential to, and this must
                // never resurrect it - see this method's own doc comment.
                let _ = self.credentials.delete(&new_reference).await;
                return Err(VfsError::NotFound {
                    location: format!("onedrive://{id}/"),
                });
            }
            Err(error) => {
                return Err(VfsError::Io {
                    message: error.to_string(),
                });
            }
        };
        let superseded = current.credential_ref.unwrap_or(previous);
        current.credential_ref = Some(new_reference);
        current.updated_at = chrono::Utc::now();
        self.repository
            .save(&current)
            .await
            .map_err(|error| VfsError::Io {
                message: error.to_string(),
            })?;
        if new_reference != superseded {
            let _ = self.credentials.delete(&superseded).await;
        }
        Ok(())
    }

    /// Best-effort clears a connection's credential after a terminal OAuth
    /// refresh failure, so `ConnectionService::evaluate` reports
    /// `AuthenticationRequired` on the next connect/test rather than
    /// silently keeping a refresh token the provider has already rejected.
    ///
    /// Re-loads the connection immediately before clearing (task 0110
    /// review, same reasoning as [`Self::rotate_credential`]) and only
    /// clears if it still references `reference`: a concurrent, later
    /// reauthorization may already have replaced the dead credential with a
    /// fresh, valid one while this (now-superseded) refresh was in flight,
    /// and that must never be clobbered back to "unauthenticated".
    async fn clear_credential(&self, id: ConnectionId, reference: CredentialRef) {
        let _ = self.credentials.delete(&reference).await;
        let Ok(Some(mut current)) = self.repository.load(id).await else {
            return;
        };
        if current.credential_ref != Some(reference) {
            return;
        }
        current.credential_ref = None;
        current.updated_at = chrono::Utc::now();
        let _ = self.repository.save(&current).await;
    }
}

/// Classifies a **refresh** failure as either requiring the stored
/// credential to be cleared (task 0110: "actionable reauthorization
/// required, not silent retry") or transient (never destroys the still-
/// possibly-valid stored refresh token). Deliberately an explicit,
/// exhaustive match over every [`OAuthError`] variant rather than a
/// catch-all, so a future variant added to `fm_auth_oauth` must be
/// consciously classified here rather than silently landing on whichever
/// side a wildcard happened to pick.
fn is_terminal_refresh_failure(error: &OAuthError) -> bool {
    match error {
        // The identity provider rejected the specific grant/token, or the
        // resource owner's/tenant's consent, itself - no retry without a
        // fresh interactive authorization can ever succeed.
        OAuthError::InteractionRequired { .. }
        | OAuthError::InvalidGrant { .. }
        | OAuthError::AccessDenied { .. }
        | OAuthError::TenantPolicyRejected { .. }
        | OAuthError::ConditionalAccessRequired { .. } => true,
        // `temporarily_unavailable`/`server_error` (RFC 6749 §5.2) mean the
        // authorization *server* is transiently unavailable or overloaded -
        // nothing about this refresh token was rejected, so it must
        // survive. Every other otherwise-unclassified code is treated as
        // terminal, matching this function's previous catch-all default.
        OAuthError::AuthorizationRejected { error, .. } => {
            !matches!(error.as_str(), "temporarily_unavailable" | "server_error")
        }
        // None of the following are ever actually returned by
        // `refresh_access_token` (only by the loopback callback or claims-
        // challenge parsing) - handled explicitly, and conservatively as
        // non-terminal, for exhaustiveness rather than a wildcard.
        OAuthError::Transport { .. }
        | OAuthError::MalformedTokenResponse { .. }
        | OAuthError::MalformedCallback { .. }
        | OAuthError::MalformedClaimsChallenge { .. }
        | OAuthError::Cancelled
        | OAuthError::TimedOut => false,
    }
}

#[async_trait]
impl OneDriveConnectionResolver for OneDriveTokenResolver {
    async fn resolve(&self, connection_id: &str) -> Result<OneDriveAccessToken, VfsError> {
        let id = ConnectionId::from_str(connection_id).map_err(|_| VfsError::InvalidLocation {
            location: format!("onedrive://{connection_id}/"),
        })?;
        let slot = self.slot_for(id);
        let _serialize = slot.refresh_lock.lock().await;
        if let Some(token) = self.cached(id) {
            return Ok(token);
        }
        self.refresh_and_cache(id).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use fm_auth_oauth::fixture::TokenEndpointFixture;
    use fm_connections::{
        ConnectionKind, InMemoryConnectionRepository, OneDriveConnectionConfiguration,
    };
    use fm_credentials::InMemoryCredentialStore;

    use super::*;

    fn oauth_config(fixture: &TokenEndpointFixture) -> PublicClientConfig {
        PublicClientConfig {
            client_id: "test-client-id".to_owned(),
            authority: fixture.authority(),
            scopes: fm_auth_oauth::config::DEFAULT_SCOPES
                .iter()
                .map(|scope| (*scope).to_owned())
                .collect(),
        }
    }

    async fn seed_connection(
        repository: &InMemoryConnectionRepository,
        credentials: &InMemoryCredentialStore,
        name: &str,
        stored_access_token: &str,
        refresh_token: &str,
    ) -> ConnectionId {
        let now = chrono::Utc::now();
        let mut profile = ConnectionProfile {
            id: ConnectionId::new(),
            name: name.to_owned(),
            kind: ConnectionKind::OneDrive,
            configuration: ConnectionConfiguration::OneDrive(
                OneDriveConnectionConfiguration::default(),
            ),
            credential_ref: None,
            created_at: now,
            updated_at: now,
        };
        let reference = credentials
            .store(StoreCredentialRequest::new(
                profile.name.clone(),
                SecretMaterial::oauth_token(stored_access_token, Some(refresh_token.to_owned())),
            ))
            .await
            .expect("store must succeed");
        profile.credential_ref = Some(reference);
        repository.save(&profile).await.expect("save must succeed");
        profile.id
    }

    fn resolver(
        repository: Arc<InMemoryConnectionRepository>,
        credentials: Arc<InMemoryCredentialStore>,
        fixture: &TokenEndpointFixture,
    ) -> OneDriveTokenResolver {
        OneDriveTokenResolver::new(
            repository,
            credentials,
            oauth_config(fixture),
            reqwest::Client::new(),
        )
    }

    #[tokio::test]
    async fn cold_start_always_refreshes_rather_than_trusting_a_stored_access_token() {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_success(
                "freshly-refreshed-access-token",
                Some("rotated-refresh"),
                3600,
            )
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "stale-access-token-from-before-restart",
            "stored-refresh-token",
        )
        .await;
        let resolver = resolver(repository, credentials, &fixture);

        let token = resolver
            .resolve(&id.to_string())
            .await
            .expect("resolve must succeed");

        assert_eq!(token.as_str(), "freshly-refreshed-access-token");
        let requests = fixture.requests().await;
        assert_eq!(requests.len(), 1, "cold start must always refresh");
        assert!(requests[0].contains("refresh_token=stored-refresh-token"));
    }

    #[tokio::test]
    async fn a_second_resolve_reuses_the_cache_without_another_refresh_request() {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_success("access-token-1", Some("refresh-token-1"), 3600)
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "refresh-0",
        )
        .await;
        let resolver = resolver(repository, credentials, &fixture);

        let first = resolver.resolve(&id.to_string()).await.unwrap();
        let second = resolver.resolve(&id.to_string()).await.unwrap();

        assert_eq!(first.as_str(), second.as_str());
        assert_eq!(fixture.requests().await.len(), 1);
    }

    #[tokio::test]
    async fn concurrent_resolves_for_the_same_connection_refresh_only_once() {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_success("access-token-once", Some("refresh-token-once"), 3600)
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "refresh-0",
        )
        .await;
        let resolver = Arc::new(resolver(repository, credentials, &fixture));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let resolver = Arc::clone(&resolver);
            handles.push(tokio::spawn(async move {
                resolver
                    .resolve(&id.to_string())
                    .await
                    .expect("resolve must succeed")
            }));
        }
        let mut tokens = Vec::new();
        for handle in handles {
            tokens.push(handle.await.expect("task must not panic"));
        }

        assert!(
            tokens
                .iter()
                .all(|token| token.as_str() == "access-token-once")
        );
        assert_eq!(
            fixture.requests().await.len(),
            1,
            "8 concurrent resolves for one connection must produce exactly one refresh"
        );
    }

    #[tokio::test]
    async fn two_connections_refresh_independently_and_never_share_a_token() {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_success("access-token-for-a", Some("refresh-for-a-rotated"), 3600)
            .await;
        fixture
            .enqueue_success("access-token-for-b", Some("refresh-for-b-rotated"), 3600)
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let account_a = seed_connection(
            &repository,
            &credentials,
            "Account A",
            "unused-a",
            "refresh-for-a",
        )
        .await;
        let account_b = seed_connection(
            &repository,
            &credentials,
            "Account B",
            "unused-b",
            "refresh-for-b",
        )
        .await;
        let resolver = resolver(repository, credentials, &fixture);

        let token_a = resolver.resolve(&account_a.to_string()).await.unwrap();
        let token_b = resolver.resolve(&account_b.to_string()).await.unwrap();

        assert_eq!(token_a.as_str(), "access-token-for-a");
        assert_eq!(token_b.as_str(), "access-token-for-b");
        assert_eq!(fixture.requests().await.len(), 2);
    }

    #[tokio::test]
    async fn an_invalid_grant_refresh_failure_clears_the_stored_credential() {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_error(400, "invalid_grant", "AADSTS70008: expired")
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "dead-refresh-token",
        )
        .await;
        let resolver = resolver(Arc::clone(&repository), Arc::clone(&credentials), &fixture);

        let error = resolver.resolve(&id.to_string()).await.unwrap_err();

        assert!(matches!(error, VfsError::CredentialRequired));
        let profile = repository
            .load(id)
            .await
            .unwrap()
            .expect("profile must still exist");
        assert_eq!(
            profile.credential_ref, None,
            "an invalid_grant failure must clear the credential so connect/test reports \
             AuthenticationRequired rather than silently retrying a dead refresh token"
        );
    }

    #[tokio::test]
    async fn a_conditional_access_refresh_failure_also_clears_the_stored_credential() {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_error(
                400,
                "access_denied",
                "AADSTS53003: blocked by conditional access",
            )
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "refresh",
        )
        .await;
        let resolver = resolver(Arc::clone(&repository), Arc::clone(&credentials), &fixture);

        let error = resolver.resolve(&id.to_string()).await.unwrap_err();

        assert!(matches!(error, VfsError::CredentialRequired));
        assert_eq!(
            repository.load(id).await.unwrap().unwrap().credential_ref,
            None
        );
    }

    #[tokio::test]
    async fn a_transient_transport_failure_does_not_clear_the_stored_credential() {
        let fixture = TokenEndpointFixture::start().await;
        fixture.enqueue_raw(200, "not valid json").await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "refresh",
        )
        .await;
        let resolver = resolver(Arc::clone(&repository), Arc::clone(&credentials), &fixture);

        let error = resolver.resolve(&id.to_string()).await.unwrap_err();

        assert!(matches!(error, VfsError::Io { .. }));
        assert!(
            repository
                .load(id)
                .await
                .unwrap()
                .unwrap()
                .credential_ref
                .is_some(),
            "a malformed/transient response must not destroy a still-possibly-valid refresh token"
        );
    }

    #[tokio::test]
    async fn a_temporarily_unavailable_refresh_failure_does_not_clear_the_stored_credential() {
        // RFC 6749 §5.2: `temporarily_unavailable` means the *authorization
        // server* is transiently overloaded/down for maintenance - nothing
        // about this specific refresh token was rejected. `from_provider_error`
        // has no dedicated variant for it, so it arrives as
        // `OAuthError::AuthorizationRejected { error: "temporarily_unavailable", .. }`;
        // review finding: this must not be treated as terminal.
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_error(
                503,
                "temporarily_unavailable",
                "the server is temporarily overloaded",
            )
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "still-good-refresh-token",
        )
        .await;
        let resolver = resolver(Arc::clone(&repository), Arc::clone(&credentials), &fixture);

        let error = resolver.resolve(&id.to_string()).await.unwrap_err();

        assert!(
            matches!(error, VfsError::Io { .. }),
            "temporarily_unavailable must be reported as a transient I/O failure, not CredentialRequired"
        );
        assert!(
            repository
                .load(id)
                .await
                .unwrap()
                .unwrap()
                .credential_ref
                .is_some(),
            "temporarily_unavailable must never clear a still-possibly-valid refresh token"
        );
    }

    #[tokio::test]
    async fn a_server_error_refresh_failure_does_not_clear_the_stored_credential() {
        // Same reasoning as `temporarily_unavailable` above: `server_error`
        // (RFC 6749 §5.2) is the authorization server's own unexpected
        // condition, not a rejection of this refresh token.
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_error(500, "server_error", "an unexpected condition occurred")
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "still-good-refresh-token",
        )
        .await;
        let resolver = resolver(Arc::clone(&repository), Arc::clone(&credentials), &fixture);

        let error = resolver.resolve(&id.to_string()).await.unwrap_err();

        assert!(matches!(error, VfsError::Io { .. }));
        assert!(
            repository
                .load(id)
                .await
                .unwrap()
                .unwrap()
                .credential_ref
                .is_some(),
            "server_error must never clear a still-possibly-valid refresh token"
        );
    }

    #[tokio::test]
    async fn an_unclassified_authorization_rejected_error_still_clears_the_stored_credential() {
        // Regression guard the other way: carving out `temporarily_unavailable`/
        // `server_error` must not accidentally make every other
        // `AuthorizationRejected` code non-terminal too - an explicit,
        // unrecognized client/request-shaped rejection (e.g. `invalid_scope`)
        // keeps the previous catch-all behaviour of clearing the credential.
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_error(400, "invalid_scope", "the requested scope is invalid")
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "refresh",
        )
        .await;
        let resolver = resolver(Arc::clone(&repository), Arc::clone(&credentials), &fixture);

        let error = resolver.resolve(&id.to_string()).await.unwrap_err();

        assert!(matches!(error, VfsError::CredentialRequired));
        assert_eq!(
            repository.load(id).await.unwrap().unwrap().credential_ref,
            None
        );
    }

    #[tokio::test]
    async fn a_refresh_response_omitting_a_refresh_token_carries_the_previous_one_forward() {
        // RFC 6749 §6: a client "can be reasonably confident" a refresh
        // token is still valid if the server's response simply omits a new
        // one. Silently persisting `refresh_token: None` here would make
        // every *subsequent* resolve unconditionally fail with
        // `CredentialRequired` even though nothing was actually revoked.
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_raw(
                200,
                r#"{"access_token":"new-access-token","expires_in":3600,"token_type":"Bearer","scope":"offline_access Files.ReadWrite User.Read"}"#,
            )
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "still-the-only-refresh-token",
        )
        .await;
        let resolver = resolver(Arc::clone(&repository), Arc::clone(&credentials), &fixture);

        let token = resolver
            .resolve(&id.to_string())
            .await
            .expect("resolve must succeed even though the response omitted refresh_token");
        assert_eq!(token.as_str(), "new-access-token");

        let profile = repository.load(id).await.unwrap().unwrap();
        let reference = profile.credential_ref.expect("credential ref must be set");
        let resolved = credentials.resolve(&reference).await.unwrap();
        assert_eq!(
            resolved.secret,
            SecretMaterial::oauth_token(
                "new-access-token",
                Some("still-the-only-refresh-token".to_owned())
            ),
            "the previous refresh token must be carried forward, not dropped"
        );

        // And it is genuinely usable on a subsequent cold-cache resolve, not
        // just present as a dangling reference.
        resolver.invalidate(id);
        fixture
            .enqueue_success("second-refresh-access-token", Some("finally-rotated"), 3600)
            .await;
        let second = resolver.resolve(&id.to_string()).await.unwrap();
        assert_eq!(second.as_str(), "second-refresh-access-token");
        let requests = fixture.requests().await;
        assert_eq!(requests.len(), 2);
        assert!(
            requests[1].contains("refresh_token=still-the-only-refresh-token"),
            "the carried-forward refresh token must be exactly what the next refresh uses"
        );
    }

    #[tokio::test]
    async fn a_stalled_token_endpoint_is_bounded_by_the_http_clients_own_timeout_not_forever() {
        // Proves the HTTP client's *own* configured timeout - not some
        // external safety net - is what bounds a stalled identity provider
        // (task 0110 review: "ensure token resolver/dialer cannot wait
        // forever").
        let stalled = crate::onedrive::test_support::StalledServer::start().await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "refresh-token",
        )
        .await;
        let oauth = PublicClientConfig {
            client_id: "test-client-id".to_owned(),
            authority: fm_auth_oauth::config::Authority::from_base_url(stalled.base_url()),
            scopes: fm_auth_oauth::config::DEFAULT_SCOPES
                .iter()
                .map(|scope| (*scope).to_owned())
                .collect(),
        };
        let http = crate::onedrive::build_http_client(
            Duration::from_millis(150),
            Duration::from_millis(150),
        );
        let resolver = OneDriveTokenResolver::new(
            repository as Arc<dyn ConnectionRepository>,
            credentials as Arc<dyn CredentialStore>,
            oauth,
            http,
        );

        let started = std::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            resolver.resolve(&id.to_string()),
        )
        .await
        .expect("resolve must itself return well within this outer safety-net timeout, not hang");

        assert!(
            matches!(result, Err(VfsError::Io { .. })),
            "a stalled server must be reported as a transient I/O failure, not hang or panic"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "must be bounded by the HTTP client's own ~150ms timeout, not an external one; took \
             {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn resolving_an_unknown_connection_reports_not_found() {
        let fixture = TokenEndpointFixture::start().await;
        let resolver = resolver(
            Arc::new(InMemoryConnectionRepository::new()),
            Arc::new(InMemoryCredentialStore::new()),
            &fixture,
        );

        let error = resolver
            .resolve(&ConnectionId::new().to_string())
            .await
            .unwrap_err();

        assert!(matches!(error, VfsError::NotFound { .. }));
    }

    #[tokio::test]
    async fn resolving_a_malformed_connection_id_reports_invalid_location() {
        let fixture = TokenEndpointFixture::start().await;
        let resolver = resolver(
            Arc::new(InMemoryConnectionRepository::new()),
            Arc::new(InMemoryCredentialStore::new()),
            &fixture,
        );

        let error = resolver.resolve("not-a-uuid").await.unwrap_err();

        assert!(matches!(error, VfsError::InvalidLocation { .. }));
    }

    #[tokio::test]
    async fn resolving_a_non_onedrive_connection_reports_invalid_location() {
        let fixture = TokenEndpointFixture::start().await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let now = chrono::Utc::now();
        let profile = ConnectionProfile {
            id: ConnectionId::new(),
            name: "Not OneDrive".to_owned(),
            kind: ConnectionKind::Smb,
            configuration: ConnectionConfiguration::Smb(
                fm_connections::SmbConnectionConfiguration {
                    server: "nas.local".to_owned(),
                    share: "media".to_owned(),
                },
            ),
            credential_ref: None,
            created_at: now,
            updated_at: now,
        };
        repository.save(&profile).await.unwrap();
        let resolver = resolver(
            Arc::clone(&repository),
            Arc::new(InMemoryCredentialStore::new()),
            &fixture,
        );

        let error = resolver.resolve(&profile.id.to_string()).await.unwrap_err();

        assert!(matches!(error, VfsError::InvalidLocation { .. }));
    }

    #[tokio::test]
    async fn resolving_a_connection_with_no_credential_reports_credential_required() {
        let fixture = TokenEndpointFixture::start().await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let now = chrono::Utc::now();
        let profile = ConnectionProfile {
            id: ConnectionId::new(),
            name: "Unauthorized OneDrive".to_owned(),
            kind: ConnectionKind::OneDrive,
            configuration: ConnectionConfiguration::OneDrive(
                OneDriveConnectionConfiguration::default(),
            ),
            credential_ref: None,
            created_at: now,
            updated_at: now,
        };
        repository.save(&profile).await.unwrap();
        let resolver = resolver(
            Arc::clone(&repository),
            Arc::new(InMemoryCredentialStore::new()),
            &fixture,
        );

        let error = resolver.resolve(&profile.id.to_string()).await.unwrap_err();

        assert!(matches!(error, VfsError::CredentialRequired));
    }

    #[tokio::test]
    async fn seed_cache_avoids_an_immediate_redundant_refresh() {
        let fixture = TokenEndpointFixture::start().await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "refresh",
        )
        .await;
        let resolver = resolver(repository, credentials, &fixture);

        resolver.seed_cache(id, "seeded-access-token", Duration::from_secs(3600));
        let token = resolver.resolve(&id.to_string()).await.unwrap();

        assert_eq!(token.as_str(), "seeded-access-token");
        assert_eq!(fixture.requests().await.len(), 0);
    }

    #[tokio::test]
    async fn debug_and_error_output_never_contain_planted_token_values() {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_success(
                "planted-access-secret",
                Some("planted-refresh-secret"),
                3600,
            )
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "planted-refresh-secret",
        )
        .await;
        let resolver = resolver(repository, credentials, &fixture);

        let token = resolver.resolve(&id.to_string()).await.unwrap();

        let formatted = format!("{token:?}");
        assert!(!formatted.contains("planted-access-secret"));
        assert!(!formatted.contains("planted-refresh-secret"));
    }

    #[tokio::test]
    async fn concurrency_smoke_uses_an_atomic_counter_to_document_single_flight_intent() {
        // Documents (rather than just asserting the fixture's request count)
        // that the per-connection lock is what prevents a second network
        // call, not incidental timing: a naive unserialized implementation
        // would let more than one task observe the "no cache yet" state.
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_success("access-token-single-flight", Some("refresh-rotated"), 3600)
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let id = seed_connection(
            &repository,
            &credentials,
            "My OneDrive",
            "unused",
            "refresh-0",
        )
        .await;
        let resolver = Arc::new(resolver(repository, credentials, &fixture));
        let observed_uncached_entries: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..4 {
            let resolver = Arc::clone(&resolver);
            let observed_uncached_entries = Arc::clone(&observed_uncached_entries);
            handles.push(tokio::spawn(async move {
                if resolver.cached(id).is_none() {
                    observed_uncached_entries.fetch_add(1, Ordering::SeqCst);
                }
                resolver.resolve(&id.to_string()).await.unwrap()
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }

        // However many tasks raced to observe an empty cache before
        // resolving, the fixture must still have been hit exactly once.
        assert_eq!(fixture.requests().await.len(), 1);
    }

    /// Wraps a repository and runs `on_second_load` exactly once, just
    /// before the *second* `load` call returns - simulating a concurrent
    /// writer's own already-persisted change becoming visible exactly when
    /// this module's own "re-load immediately before updating" fresh read
    /// happens. `refresh_and_cache` performs the first load;
    /// `rotate_credential`/`clear_credential` perform the second, so this
    /// deterministically exercises the exact race window task 0110's
    /// review flagged, without relying on real timing.
    struct InterleavedWrites<F: Fn(&mut ConnectionProfile) + Send + Sync + 'static> {
        inner: InMemoryConnectionRepository,
        load_count: AtomicUsize,
        on_second_load: F,
    }

    #[async_trait]
    impl<F: Fn(&mut ConnectionProfile) + Send + Sync + 'static> ConnectionRepository
        for InterleavedWrites<F>
    {
        async fn list(&self) -> Result<Vec<ConnectionProfile>, fm_connections::ConnectionError> {
            self.inner.list().await
        }

        async fn load(
            &self,
            id: ConnectionId,
        ) -> Result<Option<ConnectionProfile>, fm_connections::ConnectionError> {
            let count = self.load_count.fetch_add(1, Ordering::SeqCst);
            if count == 1
                && let Ok(Some(mut concurrent)) = self.inner.load(id).await
            {
                (self.on_second_load)(&mut concurrent);
                concurrent.updated_at = chrono::Utc::now();
                let _ = self.inner.save(&concurrent).await;
            }
            self.inner.load(id).await
        }

        async fn save(
            &self,
            profile: &ConnectionProfile,
        ) -> Result<ConnectionProfile, fm_connections::ConnectionError> {
            self.inner.save(profile).await
        }

        async fn delete(&self, id: ConnectionId) -> Result<(), fm_connections::ConnectionError> {
            self.inner.delete(id).await
        }
    }

    #[tokio::test]
    async fn a_concurrent_rename_during_refresh_is_preserved_not_clobbered() {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_success("new-access-token", Some("new-refresh-token"), 3600)
            .await;
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let seed_repository = InMemoryConnectionRepository::new();
        let id = seed_connection(
            &seed_repository,
            &credentials,
            "Original Name",
            "unused",
            "old-refresh-token",
        )
        .await;
        let repository = Arc::new(InterleavedWrites {
            inner: seed_repository,
            load_count: AtomicUsize::new(0),
            on_second_load: |profile: &mut ConnectionProfile| {
                profile.name = "Renamed Concurrently".to_owned();
            },
        });

        let resolver = OneDriveTokenResolver::new(
            Arc::clone(&repository) as Arc<dyn ConnectionRepository>,
            Arc::clone(&credentials) as Arc<dyn CredentialStore>,
            oauth_config(&fixture),
            reqwest::Client::new(),
        );

        let token = resolver
            .resolve(&id.to_string())
            .await
            .expect("resolve must succeed");
        assert_eq!(token.as_str(), "new-access-token");

        let final_profile = repository.inner.load(id).await.unwrap().unwrap();
        assert_eq!(
            final_profile.name, "Renamed Concurrently",
            "a concurrent rename must survive the refresh's own save, not be clobbered back \
             to the stale name it started with"
        );
        let reference = final_profile
            .credential_ref
            .expect("credential ref must be set");
        assert_eq!(
            credentials.resolve(&reference).await.unwrap().secret,
            SecretMaterial::oauth_token("new-access-token", Some("new-refresh-token".to_owned())),
            "the rotated credential must still be the one actually persisted"
        );
    }

    #[tokio::test]
    async fn a_terminal_refresh_failure_does_not_clobber_a_concurrent_reauthorization() {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_error(400, "invalid_grant", "AADSTS70008: expired")
            .await;
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let seed_repository = InMemoryConnectionRepository::new();
        let id = seed_connection(
            &seed_repository,
            &credentials,
            "My OneDrive",
            "unused",
            "dead-refresh-token",
        )
        .await;
        // What a *concurrent, independent* successful reauthorization would
        // have already stored by the time this (older, now-superseded)
        // refresh attempt gets around to clearing the credential it started
        // with.
        let concurrent_reference = credentials
            .store(StoreCredentialRequest::new(
                "My OneDrive",
                SecretMaterial::oauth_token(
                    "concurrently-issued-access-token",
                    Some("concurrently-issued-refresh-token".to_owned()),
                ),
            ))
            .await
            .unwrap();
        let repository = Arc::new(InterleavedWrites {
            inner: seed_repository,
            load_count: AtomicUsize::new(0),
            on_second_load: move |profile: &mut ConnectionProfile| {
                profile.credential_ref = Some(concurrent_reference);
            },
        });

        let resolver = OneDriveTokenResolver::new(
            Arc::clone(&repository) as Arc<dyn ConnectionRepository>,
            Arc::clone(&credentials) as Arc<dyn CredentialStore>,
            oauth_config(&fixture),
            reqwest::Client::new(),
        );

        let error = resolver.resolve(&id.to_string()).await.unwrap_err();
        assert!(matches!(error, VfsError::CredentialRequired));

        let final_profile = repository.inner.load(id).await.unwrap().unwrap();
        assert_eq!(
            final_profile.credential_ref,
            Some(concurrent_reference),
            "a concurrent reauthorization's credential must not be clobbered back to None by \
             an older, now-superseded refresh failure"
        );
        assert_eq!(
            credentials
                .resolve(&concurrent_reference)
                .await
                .unwrap()
                .secret,
            SecretMaterial::oauth_token(
                "concurrently-issued-access-token",
                Some("concurrently-issued-refresh-token".to_owned())
            ),
            "the concurrently-issued credential must still resolve fine"
        );
    }

    /// Wraps a repository, deleting the connection from the inner store
    /// just before the *second* `load` call returns - simulating a
    /// concurrent deletion of the connection landing in the exact window
    /// between `refresh_and_cache`'s own first load and
    /// `rotate_credential`'s "re-load immediately before persisting" second
    /// load (task 0110 review: an `Ok(None)` second load must never fall
    /// back to the stale first-load snapshot and resurrect a connection
    /// deleted mid-refresh).
    struct DeletedDuringSecondLoad {
        inner: InMemoryConnectionRepository,
        load_count: AtomicUsize,
    }

    #[async_trait]
    impl ConnectionRepository for DeletedDuringSecondLoad {
        async fn list(&self) -> Result<Vec<ConnectionProfile>, fm_connections::ConnectionError> {
            self.inner.list().await
        }

        async fn load(
            &self,
            id: ConnectionId,
        ) -> Result<Option<ConnectionProfile>, fm_connections::ConnectionError> {
            let count = self.load_count.fetch_add(1, Ordering::SeqCst);
            if count == 1 {
                let _ = self.inner.delete(id).await;
            }
            self.inner.load(id).await
        }

        async fn save(
            &self,
            profile: &ConnectionProfile,
        ) -> Result<ConnectionProfile, fm_connections::ConnectionError> {
            self.inner.save(profile).await
        }

        async fn delete(&self, id: ConnectionId) -> Result<(), fm_connections::ConnectionError> {
            self.inner.delete(id).await
        }
    }

    /// Wraps a credential store, recording the reference of the most
    /// recently stored secret - lets a test observe exactly which *new*
    /// credential `rotate_credential` created internally (never otherwise
    /// exposed outside the module), so it can assert that specific secret
    /// was cleaned up rather than merely that *some* secret was cleaned up.
    struct RecordingCredentialStore {
        inner: InMemoryCredentialStore,
        last_stored: Mutex<Option<CredentialRef>>,
    }

    #[async_trait]
    impl CredentialStore for RecordingCredentialStore {
        async fn store(
            &self,
            request: StoreCredentialRequest,
        ) -> Result<CredentialRef, fm_credentials::CredentialError> {
            let reference = self.inner.store(request).await?;
            *self
                .last_stored
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(reference);
            Ok(reference)
        }

        async fn resolve(
            &self,
            reference: &CredentialRef,
        ) -> Result<fm_credentials::ResolvedCredential, fm_credentials::CredentialError> {
            self.inner.resolve(reference).await
        }

        async fn delete(
            &self,
            reference: &CredentialRef,
        ) -> Result<(), fm_credentials::CredentialError> {
            self.inner.delete(reference).await
        }
    }

    #[tokio::test]
    async fn a_connection_deleted_mid_refresh_is_not_resurrected_and_the_new_credential_is_cleaned_up()
     {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_success("rotated-access-token", Some("rotated-refresh-token"), 3600)
            .await;
        let seed_credentials = InMemoryCredentialStore::new();
        let seed_repository = InMemoryConnectionRepository::new();
        let id = seed_connection(
            &seed_repository,
            &seed_credentials,
            "My OneDrive",
            "unused",
            "still-valid-refresh-token",
        )
        .await;
        let original_reference = seed_repository
            .load(id)
            .await
            .unwrap()
            .unwrap()
            .credential_ref
            .expect("seeded connection must already have a credential");

        let repository = Arc::new(DeletedDuringSecondLoad {
            inner: seed_repository,
            load_count: AtomicUsize::new(0),
        });
        let credentials = Arc::new(RecordingCredentialStore {
            inner: seed_credentials,
            last_stored: Mutex::new(None),
        });

        let resolver = OneDriveTokenResolver::new(
            Arc::clone(&repository) as Arc<dyn ConnectionRepository>,
            Arc::clone(&credentials) as Arc<dyn CredentialStore>,
            oauth_config(&fixture),
            reqwest::Client::new(),
        );

        let error = resolver.resolve(&id.to_string()).await.unwrap_err();
        assert!(
            matches!(error, VfsError::NotFound { .. }),
            "a connection deleted mid-refresh must be reported as not found, not silently \
             resurrected: {error:?}"
        );

        assert!(
            repository.inner.load(id).await.unwrap().is_none(),
            "the deleted connection must remain absent - never resurrected by the refresh's \
             own save"
        );

        let new_reference = credentials.last_stored.lock().unwrap().expect(
            "rotate_credential must have stored a new secret before discovering the \
                 connection was gone",
        );
        assert!(
            credentials.resolve(&new_reference).await.is_err(),
            "the now-orphaned newly rotated credential must be cleaned up, not left behind \
             forever"
        );
        assert!(
            credentials.resolve(&original_reference).await.is_ok(),
            "cleaning up the original credential is the deleting connection-delete flow's own \
             responsibility, not this refresh path's - it must be left untouched here"
        );
    }
}
