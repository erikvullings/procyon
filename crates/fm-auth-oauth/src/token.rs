//! Authorization-code and refresh-token exchange against the identity
//! provider's token endpoint (RFC 6749 §4.1.3/§6), sending PKCE's
//! `code_verifier` instead of a `client_secret` (task 0110: public desktop
//! client).

use std::fmt;
use std::time::Duration;

use serde::Deserialize;
use url::Url;
use zeroize::Zeroizing;

use crate::callback::AuthorizationCode;
use crate::config::PublicClientConfig;
use crate::error::OAuthError;
use crate::pkce::CodeVerifier;

/// A successful token response.
///
/// [`fmt::Debug`] never prints `access_token`/`refresh_token` (spec §19).
#[derive(Clone)]
pub struct TokenResponse {
    /// The bearer access token to send to Microsoft Graph.
    pub access_token: Zeroizing<String>,
    /// The refresh token to use for the next silent renewal, if the
    /// provider issued one. Microsoft identity platform rotates refresh
    /// tokens on every use: callers must atomically replace whatever they
    /// had stored with this value rather than keep the old one alongside
    /// it. This crate does not persist tokens itself (task 0110:
    /// `CredentialStore` persistence is owned by `fm-application`) - it only
    /// hands back the rotated value so that atomic swap is possible.
    pub refresh_token: Option<Zeroizing<String>>,
    /// How long `access_token` remains valid for, from the moment this
    /// response was received.
    pub expires_in: Duration,
    /// The token type, expected to always be `Bearer`.
    pub token_type: String,
    /// The space-separated scopes actually granted. Not secret, and may be
    /// a subset of what was requested if the tenant only consented to part
    /// of it.
    pub scope: String,
}

impl fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_in", &self.expires_in)
            .field("token_type", &self.token_type)
            .field("scope", &self.scope)
            .finish()
    }
}

#[derive(Deserialize)]
struct RawTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    #[serde(default = "default_token_type")]
    token_type: String,
    #[serde(default)]
    scope: String,
}

fn default_token_type() -> String {
    "Bearer".to_owned()
}

#[derive(Deserialize)]
struct RawTokenError {
    error: String,
    #[serde(default)]
    error_description: String,
}

impl From<RawTokenResponse> for TokenResponse {
    fn from(raw: RawTokenResponse) -> Self {
        Self {
            access_token: Zeroizing::new(raw.access_token),
            refresh_token: raw.refresh_token.map(Zeroizing::new),
            expires_in: Duration::from_secs(raw.expires_in),
            token_type: raw.token_type,
            scope: raw.scope,
        }
    }
}

/// Exchanges an authorization code for tokens (RFC 6749 §4.1.3 + RFC 7636
/// §4.5), sending the PKCE `code_verifier` instead of a client secret.
pub async fn exchange_authorization_code(
    http: &reqwest::Client,
    config: &PublicClientConfig,
    redirect_uri: &Url,
    code: &AuthorizationCode,
    verifier: &CodeVerifier,
) -> Result<TokenResponse, OAuthError> {
    let redirect_uri_text = redirect_uri.to_string();
    let scope = config.scope_parameter();
    let form = [
        ("client_id", config.client_id.as_str()),
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect_uri_text.as_str()),
        ("code_verifier", verifier.as_str()),
        ("scope", scope.as_str()),
    ];
    post_token_request(http, config, &form).await
}

/// Redeems a refresh token for a new access token, and - since Microsoft
/// identity platform rotates refresh tokens on every use - a new refresh
/// token to atomically replace it with in the caller's `CredentialStore`.
pub async fn refresh_access_token(
    http: &reqwest::Client,
    config: &PublicClientConfig,
    refresh_token: &str,
) -> Result<TokenResponse, OAuthError> {
    let scope = config.scope_parameter();
    let form = [
        ("client_id", config.client_id.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("scope", scope.as_str()),
    ];
    post_token_request(http, config, &form).await
}

async fn post_token_request(
    http: &reqwest::Client,
    config: &PublicClientConfig,
    form: &[(&str, &str)],
) -> Result<TokenResponse, OAuthError> {
    let response = http
        .post(config.authority.token_endpoint())
        .form(form)
        .send()
        .await
        .map_err(|error| OAuthError::Transport {
            message: sanitize_transport_error(&error),
        })?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| OAuthError::Transport {
            message: sanitize_transport_error(&error),
        })?;

    if status.is_success() {
        let raw: RawTokenResponse =
            serde_json::from_str(&body).map_err(|error| OAuthError::MalformedTokenResponse {
                reason: error.to_string(),
            })?;
        return Ok(raw.into());
    }

    match serde_json::from_str::<RawTokenError>(&body) {
        Ok(error_body) => Err(OAuthError::from_provider_error(
            &error_body.error,
            &error_body.error_description,
        )),
        Err(_) => Err(OAuthError::MalformedTokenResponse {
            reason: format!("token endpoint returned HTTP {status} with an unparsable body"),
        }),
    }
}

/// Reduces a `reqwest::Error` to a message safe to surface. `reqwest`'s
/// `Display` never includes request bodies or headers - so no
/// `code_verifier`/token ever leaks through it - and every request built
/// here carries its parameters in the POST body rather than the query
/// string, so the request URL it can include is also just the bare token
/// endpoint.
fn sanitize_transport_error(error: &reqwest::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::TokenEndpointFixture;

    fn config_for(fixture: &TokenEndpointFixture) -> PublicClientConfig {
        PublicClientConfig {
            client_id: "test-client-id".to_owned(),
            authority: fixture.authority(),
            scopes: crate::config::DEFAULT_SCOPES
                .iter()
                .map(|scope| (*scope).to_owned())
                .collect(),
        }
    }

    #[tokio::test]
    async fn exchanges_a_code_without_a_client_secret() {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_success("access-token-value", Some("refresh-token-value"), 3600)
            .await;
        let config = config_for(&fixture);
        let http = reqwest::Client::new();
        let redirect_uri = Url::parse("http://localhost:9999/").expect("valid URL");
        let code = crate::callback::test_support::authorization_code("auth-code-value");
        let verifier = CodeVerifier::generate();

        let tokens = exchange_authorization_code(&http, &config, &redirect_uri, &code, &verifier)
            .await
            .expect("token exchange succeeds");

        assert_eq!(tokens.access_token.as_str(), "access-token-value");
        assert_eq!(
            tokens.refresh_token.as_ref().map(|value| value.as_str()),
            Some("refresh-token-value")
        );
        assert_eq!(tokens.expires_in, Duration::from_secs(3600));

        let requests = fixture.requests().await;
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("grant_type=authorization_code"));
        assert!(requests[0].contains("code_verifier="));
        assert!(!requests[0].contains("client_secret"));
    }

    #[tokio::test]
    async fn refresh_returns_the_rotated_refresh_token() {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_success("new-access-token", Some("rotated-refresh-token"), 3600)
            .await;
        let config = config_for(&fixture);
        let http = reqwest::Client::new();

        let tokens = refresh_access_token(&http, &config, "old-refresh-token")
            .await
            .expect("refresh succeeds");

        assert_eq!(tokens.access_token.as_str(), "new-access-token");
        assert_eq!(
            tokens.refresh_token.as_ref().map(|value| value.as_str()),
            Some("rotated-refresh-token")
        );

        let requests = fixture.requests().await;
        assert!(requests[0].contains("grant_type=refresh_token"));
        assert!(requests[0].contains("refresh_token=old-refresh-token"));
        assert!(!requests[0].contains("client_secret"));
    }

    #[tokio::test]
    async fn token_requests_never_carry_a_claims_parameter() {
        // `claims` (including the `cp1` capability declaration and any
        // merged Continuous Access Evaluation challenge) is an
        // authorization-request-only parameter: it must never appear on a
        // code exchange or a refresh, on either grant type.
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_success("access-token-value", Some("refresh-token-value"), 3600)
            .await;
        fixture
            .enqueue_success("new-access-token", Some("rotated-refresh-token"), 3600)
            .await;
        let config = config_for(&fixture);
        let http = reqwest::Client::new();
        let redirect_uri = Url::parse("http://localhost:9999/").expect("valid URL");
        let code = crate::callback::test_support::authorization_code("auth-code-value");
        let verifier = CodeVerifier::generate();

        exchange_authorization_code(&http, &config, &redirect_uri, &code, &verifier)
            .await
            .expect("token exchange succeeds");
        refresh_access_token(&http, &config, "refresh-token-value")
            .await
            .expect("refresh succeeds");

        let requests = fixture.requests().await;
        assert_eq!(requests.len(), 2);
        for request in requests {
            assert!(!request.contains("claims"));
        }
    }

    #[tokio::test]
    async fn classifies_an_invalid_grant_error_response() {
        let fixture = TokenEndpointFixture::start().await;
        fixture
            .enqueue_error(400, "invalid_grant", "AADSTS70008: expired")
            .await;
        let config = config_for(&fixture);
        let http = reqwest::Client::new();

        let error = refresh_access_token(&http, &config, "dead-refresh-token")
            .await
            .expect_err("refresh fails");

        assert!(matches!(error, OAuthError::InvalidGrant { .. }));
    }

    #[tokio::test]
    async fn malformed_json_bodies_are_reported_as_malformed_token_responses() {
        let fixture = TokenEndpointFixture::start().await;
        fixture.enqueue_raw(200, "not json").await;
        let config = config_for(&fixture);
        let http = reqwest::Client::new();

        let error = refresh_access_token(&http, &config, "refresh-token")
            .await
            .expect_err("refresh fails");

        assert!(matches!(error, OAuthError::MalformedTokenResponse { .. }));
    }

    #[test]
    fn debug_output_never_contains_the_access_or_refresh_token() {
        let tokens = TokenResponse {
            access_token: Zeroizing::new("access-secret".to_owned()),
            refresh_token: Some(Zeroizing::new("refresh-secret".to_owned())),
            expires_in: Duration::from_secs(3600),
            token_type: "Bearer".to_owned(),
            scope: "Files.ReadWrite".to_owned(),
        };
        let formatted = format!("{tokens:?}");
        assert!(!formatted.contains("access-secret"));
        assert!(!formatted.contains("refresh-secret"));
        assert!(formatted.contains("Bearer"));
        assert!(formatted.contains("Files.ReadWrite"));
    }
}
