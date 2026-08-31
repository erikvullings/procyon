//! Public-client configuration: which Microsoft identity platform authority,
//! application (client) id and delegated scopes an authorization/token
//! request targets.

use url::Url;

/// Delegated Microsoft Graph scopes task 0110 requires: `offline_access` so
/// a refresh token is issued, `Files.ReadWrite` for OneDrive access, and
/// `User.Read` to resolve the signed-in account's profile.
pub const DEFAULT_SCOPES: &[&str] = &["offline_access", "Files.ReadWrite", "User.Read"];

/// The Microsoft identity platform authority an authorization/token request
/// targets, expressed as a base URL so tests can point this at an in-process
/// fixture instead of `login.microsoftonline.com` (no real Microsoft calls
/// in tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authority {
    base_url: Url,
}

impl Authority {
    /// The Microsoft identity platform `common` authority: accepts both
    /// personal Microsoft accounts and any Microsoft Entra organizational
    /// tenant through one public-client registration (task 0110).
    #[must_use]
    pub fn microsoft_common() -> Self {
        Self::from_base_url(
            Url::parse("https://login.microsoftonline.com/common").expect("valid authority URL"),
        )
    }

    /// An authority rooted at an arbitrary base URL, for tests that point
    /// this crate at a local fixture server instead of the real Microsoft
    /// identity platform.
    #[must_use]
    pub fn from_base_url(base_url: Url) -> Self {
        Self { base_url }
    }

    /// The `/oauth2/v2.0/authorize` endpoint under this authority.
    #[must_use]
    pub fn authorize_endpoint(&self) -> Url {
        self.join("oauth2/v2.0/authorize")
    }

    /// The `/oauth2/v2.0/token` endpoint under this authority.
    #[must_use]
    pub fn token_endpoint(&self) -> Url {
        self.join("oauth2/v2.0/token")
    }

    fn join(&self, segment: &str) -> Url {
        let mut url = self.base_url.clone();
        {
            let mut path_segments = url
                .path_segments_mut()
                .expect("authority base URL must be able to be a base");
            path_segments.pop_if_empty();
            for part in segment.split('/') {
                path_segments.push(part);
            }
        }
        url
    }
}

/// A public-client OAuth application, as registered with the identity
/// provider. Never carries a client secret - task 0110's application is a
/// public desktop client where PKCE replaces the secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicClientConfig {
    /// The application (client) id, for example Procyon's
    /// `9b01b729-5908-492b-bcd1-32b4a36096de`. Public configuration, not a
    /// secret.
    pub client_id: String,
    /// The authority to authenticate against.
    pub authority: Authority,
    /// Delegated scopes to request.
    pub scopes: Vec<String>,
}

impl PublicClientConfig {
    /// Builds a configuration against the Microsoft identity platform
    /// `common` authority, requesting [`DEFAULT_SCOPES`].
    #[must_use]
    pub fn microsoft_common(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            authority: Authority::microsoft_common(),
            scopes: DEFAULT_SCOPES
                .iter()
                .map(|scope| (*scope).to_owned())
                .collect(),
        }
    }

    /// The scopes as one space-separated string, the wire format OAuth
    /// requests use.
    #[must_use]
    pub fn scope_parameter(&self) -> String {
        self.scopes.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microsoft_common_authority_uses_the_common_tenant() {
        let authority = Authority::microsoft_common();
        assert_eq!(
            authority.authorize_endpoint().as_str(),
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize"
        );
        assert_eq!(
            authority.token_endpoint().as_str(),
            "https://login.microsoftonline.com/common/oauth2/v2.0/token"
        );
    }

    #[test]
    fn fixture_authority_joins_endpoints_under_its_base_url() {
        let base = Url::parse("http://127.0.0.1:4000").expect("valid URL");
        let authority = Authority::from_base_url(base);
        assert_eq!(
            authority.token_endpoint().as_str(),
            "http://127.0.0.1:4000/oauth2/v2.0/token"
        );
    }

    #[test]
    fn default_scopes_match_task_0110() {
        let config = PublicClientConfig::microsoft_common("client-id");
        assert_eq!(
            config.scope_parameter(),
            "offline_access Files.ReadWrite User.Read"
        );
    }
}
