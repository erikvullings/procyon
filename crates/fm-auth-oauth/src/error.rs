//! Typed OAuth/PKCE failures (task 0110).
//!
//! Distinct variants exist so a caller can react differently to "the
//! refresh token is dead, restart interactive sign-in" versus "the tenant's
//! policy blocked this" versus "the user closed the browser" instead of
//! parsing provider prose. Every field here is diagnostic text the provider
//! or transport layer already considered safe to surface (an OAuth
//! `error_description`, a sanitized transport message) - never token
//! material (spec §19 "no secret logging").

use thiserror::Error;

/// Failure modes across the whole PKCE authorization-code and refresh flow.
#[derive(Debug, Error)]
pub enum OAuthError {
    /// The identity provider requires an interactive sign-in (expired
    /// session, revoked consent, step-up authentication, ...) before it will
    /// issue a token; a silent refresh cannot proceed and the caller must
    /// restart the interactive authorization-code flow.
    #[error("interactive sign-in is required: {description}")]
    InteractionRequired {
        /// Provider-supplied `error_description`.
        description: String,
    },

    /// The authorization code or refresh token was rejected as invalid,
    /// expired, revoked, or already used.
    #[error("the authorization grant was rejected: {description}")]
    InvalidGrant {
        /// Provider-supplied `error_description`.
        description: String,
    },

    /// The resource owner declined the consent prompt.
    #[error("the user denied consent: {description}")]
    AccessDenied {
        /// Provider-supplied `error_description`.
        description: String,
    },

    /// The tenant's administrator has not consented to (or has blocked) this
    /// application or one of its requested scopes.
    #[error("the tenant's policy rejected this application: {description}")]
    TenantPolicyRejected {
        /// Provider-supplied `error_description`.
        description: String,
    },

    /// A Conditional Access policy (multi-factor authentication, a
    /// compliant-device requirement, a named-location restriction, ...)
    /// blocked the sign-in.
    #[error("a conditional access policy blocked sign-in: {description}")]
    ConditionalAccessRequired {
        /// Provider-supplied `error_description`.
        description: String,
    },

    /// An authorization failure this crate does not classify into a more
    /// specific variant above.
    #[error("authorization failed ({error}): {description}")]
    AuthorizationRejected {
        /// The OAuth `error` code, for example `unauthorized_client`.
        error: String,
        /// Provider-supplied `error_description`.
        description: String,
    },

    /// The loopback callback could not be understood as an OAuth redirect:
    /// it matched the awaited `state` but carried neither `code` nor
    /// `error`, or the request itself could not be parsed as HTTP.
    #[error("the provider callback was malformed: {reason}")]
    MalformedCallback {
        /// Why the callback could not be interpreted.
        reason: String,
    },

    /// The token endpoint returned a response body this crate could not
    /// parse as either a token or an error response.
    #[error("the token response was malformed: {reason}")]
    MalformedTokenResponse {
        /// Why the response could not be interpreted.
        reason: String,
    },

    /// A `claims` challenge (typically from a Microsoft Graph
    /// `WWW-Authenticate: insufficient_claims` response) could not be
    /// interpreted: it was not valid base64/JSON, exceeded the accepted
    /// size, or was not shaped like a claims document. The raw challenge
    /// text is never included here (spec: no raw challenge leaks through
    /// errors) - only a static description of what was wrong with it.
    #[error("the claims challenge was malformed: {reason}")]
    MalformedClaimsChallenge {
        /// Why the challenge could not be interpreted. Never the raw
        /// challenge content itself.
        reason: String,
    },

    /// A network-level failure talking to the identity provider (DNS,
    /// connection refused, TLS, a loopback socket failing to bind or
    /// accept, ...).
    #[error("a transport error occurred: {message}")]
    Transport {
        /// Sanitized description of the underlying transport failure.
        message: String,
    },

    /// The caller cancelled the interactive flow before it completed.
    #[error("sign-in was cancelled")]
    Cancelled,

    /// No valid callback arrived before the configured deadline.
    #[error("sign-in timed out waiting for the browser to complete")]
    TimedOut,
}

impl OAuthError {
    /// Classifies a provider `error`/`error_description` pair - from either
    /// the authorization callback's query string or a token-endpoint error
    /// response - into a typed variant.
    ///
    /// The bare OAuth `error` code is too coarse on its own: Microsoft
    /// identity platform reports plain user cancellation, admin-consent
    /// rejection, and Conditional Access blocks alike as `access_denied`,
    /// distinguished only by an `AADSTS*` code embedded in
    /// `error_description`. This is a best-effort classification over that
    /// prose, not a documented contract - callers that need the exact code
    /// still have it in the variant's `description`/`error` field.
    #[must_use]
    pub fn from_provider_error(error: &str, description: &str) -> Self {
        match error {
            "interaction_required" => Self::InteractionRequired {
                description: description.to_owned(),
            },
            "invalid_grant" => Self::InvalidGrant {
                description: description.to_owned(),
            },
            "access_denied" => classify_access_denied(description),
            other => Self::AuthorizationRejected {
                error: other.to_owned(),
                description: description.to_owned(),
            },
        }
    }
}

/// AADSTS codes Microsoft identity platform documents for a Conditional
/// Access denial (device/location/MFA/app-protection policy blocks).
const CONDITIONAL_ACCESS_CODES: &[&str] = &[
    "AADSTS53000",
    "AADSTS53001",
    "AADSTS53002",
    "AADSTS53003",
    "AADSTS50079",
    "AADSTS50076",
    "AADSTS50158",
];

/// AADSTS codes for a tenant/administrator policy rejection: consent not
/// granted, the application blocked outright, or disabled for the tenant.
const TENANT_POLICY_CODES: &[&str] = &["AADSTS90094", "AADSTS50020", "AADSTS700016"];

fn classify_access_denied(description: &str) -> OAuthError {
    if contains_any(description, CONDITIONAL_ACCESS_CODES) {
        return OAuthError::ConditionalAccessRequired {
            description: description.to_owned(),
        };
    }
    if contains_any(description, TENANT_POLICY_CODES) {
        return OAuthError::TenantPolicyRejected {
            description: description.to_owned(),
        };
    }
    OAuthError::AccessDenied {
        description: description.to_owned(),
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_interaction_required() {
        assert!(matches!(
            OAuthError::from_provider_error("interaction_required", "need MFA"),
            OAuthError::InteractionRequired { .. }
        ));
    }

    #[test]
    fn classifies_invalid_grant() {
        assert!(matches!(
            OAuthError::from_provider_error("invalid_grant", "AADSTS70008: expired"),
            OAuthError::InvalidGrant { .. }
        ));
    }

    #[test]
    fn classifies_plain_access_denied() {
        assert!(matches!(
            OAuthError::from_provider_error("access_denied", "AADSTS65004: user declined"),
            OAuthError::AccessDenied { .. }
        ));
    }

    #[test]
    fn classifies_conditional_access_denials() {
        assert!(matches!(
            OAuthError::from_provider_error(
                "access_denied",
                "AADSTS53003: blocked by conditional access"
            ),
            OAuthError::ConditionalAccessRequired { .. }
        ));
    }

    #[test]
    fn classifies_tenant_policy_denials() {
        assert!(matches!(
            OAuthError::from_provider_error("access_denied", "AADSTS90094: admin consent required"),
            OAuthError::TenantPolicyRejected { .. }
        ));
    }

    #[test]
    fn falls_back_to_authorization_rejected_for_unknown_errors() {
        assert!(matches!(
            OAuthError::from_provider_error("unauthorized_client", "not allowed"),
            OAuthError::AuthorizationRejected { .. }
        ));
    }
}
