//! [`OneDriveDialer`]: verifies `/me/drive` reachability for `connect`/`test`
//! (task 0110), reusing the same [`OneDriveTokenResolver`] the
//! `FileSystemProvider` and authorization completion use - a real network
//! round trip, not a stand-in "no dialer registered" success. Never blocks
//! either personal or business `driveType`: this only confirms the
//! connection's stored credential currently resolves to a usable Graph
//! session, distinguishing missing-scope/Conditional Access/tenant-policy
//! failures with an actionable message rather than a bare "failed".

use std::sync::Arc;

use async_trait::async_trait;
use fm_connections::{
    ConnectionConfiguration, ConnectionDialer, ConnectionError, ConnectionProfile,
};
use fm_credentials::ResolvedCredential;
use fm_vfs::VfsError;
use fm_vfs_onedrive::OneDriveConnectionResolver;
use url::Url;

use super::graph::{self, GraphVerifyError};
use super::token::OneDriveTokenResolver;

pub(crate) struct OneDriveDialer {
    token_resolver: Arc<OneDriveTokenResolver>,
    graph_base_url: Url,
    http: reqwest::Client,
}

impl OneDriveDialer {
    pub(crate) fn new(
        token_resolver: Arc<OneDriveTokenResolver>,
        graph_base_url: Url,
        http: reqwest::Client,
    ) -> Self {
        Self {
            token_resolver,
            graph_base_url,
            http,
        }
    }
}

#[async_trait]
impl ConnectionDialer for OneDriveDialer {
    async fn dial(
        &self,
        profile: &ConnectionProfile,
        credential: Option<&ResolvedCredential>,
    ) -> Result<(), ConnectionError> {
        if !matches!(profile.configuration, ConnectionConfiguration::OneDrive(_)) {
            return Err(ConnectionError::DialFailed(
                "connection is not a OneDrive connection".to_owned(),
            ));
        }
        // `ConnectionService::evaluate` already reports
        // `AuthenticationRequired` before ever calling the dialer when no
        // `credential_ref` is stored - this is defense in depth for a
        // caller that invokes the dialer directly.
        if credential.is_none() {
            return Err(ConnectionError::DialFailed(
                "no stored OneDrive credential; authorize this connection first".to_owned(),
            ));
        }

        let token = self
            .token_resolver
            .resolve(&profile.id.to_string())
            .await
            .map_err(|error| ConnectionError::DialFailed(vfs_error_message(&error)))?;

        graph::verify_drive_access(&self.http, &self.graph_base_url, token.as_str())
            .await
            .map(|_drive_type| ())
            .map_err(|error| ConnectionError::DialFailed(graph_error_message(&error)))
    }
}

fn vfs_error_message(error: &VfsError) -> String {
    match error {
        VfsError::CredentialRequired => {
            "reauthorization is required for this OneDrive connection".to_owned()
        }
        other => other.to_string(),
    }
}

fn graph_error_message(error: &GraphVerifyError) -> String {
    match error {
        GraphVerifyError::ConditionalAccessRequired(_) => {
            "Microsoft requires additional verification (Conditional Access) before granting \
             access; reauthorize this connection"
                .to_owned()
        }
        GraphVerifyError::Unauthorized => {
            "Microsoft Graph rejected the current access token; reauthorize this connection"
                .to_owned()
        }
        GraphVerifyError::Forbidden => {
            "Microsoft Graph denied access to this drive; the tenant's policy may restrict this \
             application"
                .to_owned()
        }
        GraphVerifyError::Transport(message) => {
            format!("could not reach Microsoft Graph: {message}")
        }
        GraphVerifyError::Malformed(message) => message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use fm_auth_oauth::config::PublicClientConfig;
    use fm_auth_oauth::fixture::TokenEndpointFixture;
    use fm_connections::{
        ConnectionId, ConnectionKind, InMemoryConnectionRepository, OneDriveConnectionConfiguration,
    };
    use fm_credentials::{
        CredentialStore, InMemoryCredentialStore, SecretMaterial, StoreCredentialRequest,
    };

    use super::super::graph::fixture::GraphFixture;
    use super::*;

    async fn seed_connection(
        repository: &InMemoryConnectionRepository,
        credentials: &InMemoryCredentialStore,
        refresh_token: &str,
    ) -> ConnectionProfile {
        use fm_connections::ConnectionRepository;

        let now = chrono::Utc::now();
        let mut profile = ConnectionProfile {
            id: ConnectionId::new(),
            name: "My OneDrive".to_owned(),
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
                SecretMaterial::oauth_token("unused-access-token", Some(refresh_token.to_owned())),
            ))
            .await
            .unwrap();
        profile.credential_ref = Some(reference);
        repository.save(&profile).await.unwrap();
        profile
    }

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

    async fn dialer_with(
        token_fixture: &TokenEndpointFixture,
        repository: Arc<InMemoryConnectionRepository>,
        credentials: Arc<InMemoryCredentialStore>,
        graph_fixture: &GraphFixture,
    ) -> OneDriveDialer {
        let resolver = Arc::new(OneDriveTokenResolver::new(
            repository,
            credentials,
            oauth_config(token_fixture),
            reqwest::Client::new(),
        ));
        OneDriveDialer::new(resolver, graph_fixture.base_url(), reqwest::Client::new())
    }

    #[tokio::test]
    async fn dial_succeeds_for_a_personal_drive() {
        let token_fixture = TokenEndpointFixture::start().await;
        token_fixture
            .enqueue_success("access-token", Some("rotated-refresh"), 3600)
            .await;
        let graph_fixture = GraphFixture::start().await;
        graph_fixture
            .enqueue_json(200, serde_json::json!({ "driveType": "personal" }))
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let profile = seed_connection(&repository, &credentials, "refresh-token").await;
        let resolved = credentials
            .resolve(&profile.credential_ref.unwrap())
            .await
            .unwrap();
        let dialer = dialer_with(&token_fixture, repository, credentials, &graph_fixture).await;

        dialer
            .dial(&profile, Some(&resolved))
            .await
            .expect("dial must succeed for a personal drive");
    }

    #[tokio::test]
    async fn dial_succeeds_for_a_business_drive() {
        let token_fixture = TokenEndpointFixture::start().await;
        token_fixture
            .enqueue_success("access-token", Some("rotated-refresh"), 3600)
            .await;
        let graph_fixture = GraphFixture::start().await;
        graph_fixture
            .enqueue_json(200, serde_json::json!({ "driveType": "business" }))
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let profile = seed_connection(&repository, &credentials, "refresh-token").await;
        let resolved = credentials
            .resolve(&profile.credential_ref.unwrap())
            .await
            .unwrap();
        let dialer = dialer_with(&token_fixture, repository, credentials, &graph_fixture).await;

        dialer
            .dial(&profile, Some(&resolved))
            .await
            .expect("dial must succeed for a business drive, never blocked");
    }

    #[tokio::test]
    async fn dial_rejects_a_non_onedrive_connection() {
        let token_fixture = TokenEndpointFixture::start().await;
        let graph_fixture = GraphFixture::start().await;
        let dialer = dialer_with(
            &token_fixture,
            Arc::new(InMemoryConnectionRepository::new()),
            Arc::new(InMemoryCredentialStore::new()),
            &graph_fixture,
        )
        .await;
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

        let error = dialer.dial(&profile, None).await.unwrap_err();

        assert!(
            matches!(error, ConnectionError::DialFailed(message) if message.contains("not a OneDrive connection"))
        );
    }

    #[tokio::test]
    async fn dial_reports_an_actionable_message_when_no_credential_was_resolved() {
        let token_fixture = TokenEndpointFixture::start().await;
        let graph_fixture = GraphFixture::start().await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let profile = seed_connection(&repository, &credentials, "refresh-token").await;
        let dialer = dialer_with(&token_fixture, repository, credentials, &graph_fixture).await;

        let error = dialer.dial(&profile, None).await.unwrap_err();

        assert!(
            matches!(error, ConnectionError::DialFailed(message) if message.contains("authorize"))
        );
    }

    #[tokio::test]
    async fn dial_surfaces_conditional_access_as_an_actionable_reauthorize_message() {
        let token_fixture = TokenEndpointFixture::start().await;
        token_fixture
            .enqueue_success("access-token", Some("rotated-refresh"), 3600)
            .await;
        let graph_fixture = GraphFixture::start().await;
        let raw_claims = {
            use base64::Engine as _;
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(r#"{"access_token":{"acrs":["c1"]}}"#.as_bytes())
        };
        graph_fixture
            .enqueue_conditional_access_challenge(403, &raw_claims)
            .await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let profile = seed_connection(&repository, &credentials, "refresh-token").await;
        let resolved = credentials
            .resolve(&profile.credential_ref.unwrap())
            .await
            .unwrap();
        let dialer = dialer_with(&token_fixture, repository, credentials, &graph_fixture).await;

        let error = dialer.dial(&profile, Some(&resolved)).await.unwrap_err();

        let ConnectionError::DialFailed(message) = error else {
            panic!("expected DialFailed");
        };
        assert!(message.contains("Conditional Access"));
        assert!(
            !message.contains(&raw_claims),
            "must never leak the raw challenge"
        );
    }

    #[tokio::test]
    async fn dial_surfaces_a_dead_refresh_token_as_a_reauthorization_message() {
        let token_fixture = TokenEndpointFixture::start().await;
        token_fixture
            .enqueue_error(400, "invalid_grant", "AADSTS70008: expired")
            .await;
        let graph_fixture = GraphFixture::start().await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let profile = seed_connection(&repository, &credentials, "dead-refresh-token").await;
        let resolved = credentials
            .resolve(&profile.credential_ref.unwrap())
            .await
            .unwrap();
        let dialer = dialer_with(&token_fixture, repository, credentials, &graph_fixture).await;

        let error = dialer.dial(&profile, Some(&resolved)).await.unwrap_err();

        let ConnectionError::DialFailed(message) = error else {
            panic!("expected DialFailed");
        };
        assert!(message.contains("reauthorization"));
        assert!(!message.contains("dead-refresh-token"));
    }

    #[tokio::test]
    async fn dial_failure_messages_never_contain_a_bearer_token_value() {
        let token_fixture = TokenEndpointFixture::start().await;
        token_fixture
            .enqueue_success(
                "planted-access-secret",
                Some("planted-refresh-secret"),
                3600,
            )
            .await;
        let graph_fixture = GraphFixture::start().await;
        graph_fixture.enqueue_status(403).await;
        let repository = Arc::new(InMemoryConnectionRepository::new());
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let profile = seed_connection(&repository, &credentials, "planted-refresh-secret").await;
        let resolved = credentials
            .resolve(&profile.credential_ref.unwrap())
            .await
            .unwrap();
        let dialer = dialer_with(&token_fixture, repository, credentials, &graph_fixture).await;

        let error = dialer.dial(&profile, Some(&resolved)).await.unwrap_err();

        let ConnectionError::DialFailed(message) = error else {
            panic!("expected DialFailed");
        };
        assert!(!message.contains("planted-access-secret"));
        assert!(!message.contains("planted-refresh-secret"));
    }
}
