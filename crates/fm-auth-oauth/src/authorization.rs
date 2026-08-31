//! Builds the Microsoft identity platform authorization URL: RFC 7636 PKCE,
//! `response_type=code`, `response_mode=query`.

use url::Url;

use crate::claims::{ClaimsChallenge, ClaimsParameter};
use crate::config::PublicClientConfig;
use crate::pkce::generate_state;

/// Everything [`build_authorization_request`] produces: the URL to open in
/// the system browser plus the `state` the caller must hold onto to
/// validate the eventual callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    /// The URL to open in the system browser. Opening it is
    /// `fm-application`'s responsibility - this crate must not depend on any
    /// OS/browser-launching crate.
    pub url: Url,
    /// The `state` value embedded in `url`, to check against the callback.
    pub state: String,
}

/// Builds an authorization URL for `config`, to be completed at
/// `redirect_uri` (typically a
/// [`crate::callback::CallbackListener::redirect_uri`]) with PKCE challenge
/// `challenge` and CSRF token `state`.
///
/// Always includes the `cp1` client-capability `claims` declaration (see
/// [`ClaimsParameter::cp1_capability`]) so Microsoft identity platform can
/// issue Continuous-Access-Evaluation-capable tokens; use
/// [`build_challenged_authorization_request`] instead after a Microsoft
/// Graph `insufficient_claims` challenge.
#[must_use]
pub fn build_authorization_url(
    config: &PublicClientConfig,
    redirect_uri: &Url,
    state: &str,
    challenge: &str,
) -> Url {
    build_authorization_url_with_claims(
        config,
        redirect_uri,
        state,
        challenge,
        &ClaimsParameter::cp1_capability(),
    )
}

/// Builds an authorization URL for `config` together with a freshly
/// generated `state`, so most callers do not need
/// [`crate::pkce::generate_state`] directly.
#[must_use]
pub fn build_authorization_request(
    config: &PublicClientConfig,
    redirect_uri: &Url,
    challenge: &str,
) -> AuthorizationRequest {
    let state = generate_state();
    let url = build_authorization_url(config, redirect_uri, &state, challenge);
    AuthorizationRequest { url, state }
}

/// Builds an authorization URL that responds to a Microsoft Graph
/// `insufficient_claims` challenge: `claims_challenge`'s `access_token`
/// claims are merged with the client's `cp1` capability declaration (spec:
/// merge without dropping challenge fields), and a fresh `state` and PKCE
/// `challenge` are used - the identity provider must not see a repeated
/// `state` across two independent authorization attempts.
#[must_use]
pub fn build_challenged_authorization_request(
    config: &PublicClientConfig,
    redirect_uri: &Url,
    challenge: &str,
    claims_challenge: &ClaimsChallenge,
) -> AuthorizationRequest {
    let state = generate_state();
    let claims = claims_challenge.merge_with_cp1();
    let url = build_authorization_url_with_claims(config, redirect_uri, &state, challenge, &claims);
    AuthorizationRequest { url, state }
}

fn build_authorization_url_with_claims(
    config: &PublicClientConfig,
    redirect_uri: &Url,
    state: &str,
    challenge: &str,
    claims: &ClaimsParameter,
) -> Url {
    let mut url = config.authority.authorize_endpoint();
    url.query_pairs_mut()
        .append_pair("client_id", &config.client_id)
        .append_pair("response_type", "code")
        .append_pair("response_mode", "query")
        .append_pair("redirect_uri", redirect_uri.as_str())
        .append_pair("scope", &config.scope_parameter())
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("claims", claims.as_json());
    url
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn authorization_url_carries_pkce_and_response_mode_query() {
        let config = PublicClientConfig::microsoft_common("9b01b729-5908-492b-bcd1-32b4a36096de");
        let redirect_uri = Url::parse("http://localhost:51234/").expect("valid URL");
        let request = build_authorization_request(&config, &redirect_uri, "challenge-value");

        assert_eq!(request.url.scheme(), "https");
        assert_eq!(request.url.host_str(), Some("login.microsoftonline.com"));
        assert_eq!(request.url.path(), "/common/oauth2/v2.0/authorize");

        let params: HashMap<String, String> = request
            .url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(
            params.get("client_id").map(String::as_str),
            Some("9b01b729-5908-492b-bcd1-32b4a36096de")
        );
        assert_eq!(
            params.get("response_type").map(String::as_str),
            Some("code")
        );
        assert_eq!(
            params.get("response_mode").map(String::as_str),
            Some("query")
        );
        assert_eq!(
            params.get("redirect_uri").map(String::as_str),
            Some("http://localhost:51234/")
        );
        assert_eq!(
            params.get("scope").map(String::as_str),
            Some("offline_access Files.ReadWrite User.Read")
        );
        assert_eq!(
            params.get("state").map(String::as_str),
            Some(request.state.as_str())
        );
        assert_eq!(
            params.get("code_challenge").map(String::as_str),
            Some("challenge-value")
        );
        assert_eq!(
            params.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );

        let claims: serde_json::Value =
            serde_json::from_str(params.get("claims").expect("claims parameter present"))
                .expect("claims parameter is valid JSON");
        assert_eq!(
            claims,
            serde_json::json!({ "access_token": { "xms_cc": { "values": ["cp1"] } } })
        );
    }

    #[test]
    fn each_authorization_request_gets_a_fresh_state() {
        let config = PublicClientConfig::microsoft_common("client-id");
        let redirect_uri = Url::parse("http://localhost:1/").expect("valid URL");
        let first = build_authorization_request(&config, &redirect_uri, "challenge");
        let second = build_authorization_request(&config, &redirect_uri, "challenge");
        assert_ne!(first.state, second.state);
    }

    #[test]
    fn build_challenged_authorization_request_merges_claims_and_gets_a_fresh_state() {
        let config = PublicClientConfig::microsoft_common("9b01b729-5908-492b-bcd1-32b4a36096de");
        let redirect_uri = Url::parse("http://localhost:51234/").expect("valid URL");
        let initial = build_authorization_request(&config, &redirect_uri, "first-challenge");

        let raw_challenge = {
            use base64::Engine as _;
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                r#"{"access_token":{"nbf":{"essential":true,"value":"161234"}}}"#.as_bytes(),
            )
        };
        let claims_challenge = ClaimsChallenge::parse(&raw_challenge).expect("valid challenge");

        let challenged = build_challenged_authorization_request(
            &config,
            &redirect_uri,
            "second-challenge",
            &claims_challenge,
        );

        assert_ne!(initial.state, challenged.state);

        let params: HashMap<String, String> = challenged
            .url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(
            params.get("code_challenge").map(String::as_str),
            Some("second-challenge")
        );
        assert_eq!(
            params.get("state").map(String::as_str),
            Some(challenged.state.as_str())
        );

        let claims: serde_json::Value =
            serde_json::from_str(params.get("claims").expect("claims parameter present"))
                .expect("claims parameter is valid JSON");
        assert_eq!(
            claims,
            serde_json::json!({
                "access_token": {
                    "nbf": { "essential": true, "value": "161234" },
                    "xms_cc": { "values": ["cp1"] },
                }
            })
        );
    }
}
