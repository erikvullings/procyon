//! Continuous Access Evaluation (CAE) `claims` request-parameter support
//! (task 0110 addendum).
//!
//! Microsoft identity platform expects every authorization request from a
//! CAE-capable client to declare the `cp1` client capability via a `claims`
//! query parameter: `{"access_token":{"xms_cc":{"values":["cp1"]}}}`. When
//! Microsoft Graph later rejects a request, it can challenge for
//! additional claims via a `WWW-Authenticate` header shaped like:
//!
//! ```text
//! Bearer error="insufficient_claims", claims="<base64 JSON>"
//! ```
//!
//! The caller must decode that challenge, merge its `access_token` claims
//! with the same `cp1` declaration (without dropping anything the
//! challenge carried), and restart the authorization-code flow with a
//! fresh `state`/PKCE pair and this merged value as the new `claims`
//! parameter.
//!
//! The `claims` parameter is sent as **plain, URL-encoded JSON** on
//! `/authorize` - unlike the challenge that prompts it, which Microsoft
//! Graph issues **base64-encoded** - and it must never be sent on a
//! token/refresh-token POST: `claims` is an authorization-request-only
//! parameter, not part of the token/refresh redemption contract.
//!
//! This module only decodes/merges an already-extracted challenge value
//! (the string that would appear as a `WWW-Authenticate` header's `claims`
//! parameter) - parsing the `WWW-Authenticate` header's auth-param grammar
//! itself belongs with whatever component makes the Microsoft Graph
//! request that can receive this challenge (out of this crate's "token-shaped
//! protocol work" scope, per [`crate`]'s module docs).

use std::fmt;

use serde_json::{Map, Value};

use crate::error::OAuthError;

/// Upper bound, in bytes, on a `claims` challenge payload - checked both
/// before and after base64 decoding. A malicious or malfunctioning server
/// must not be able to force unbounded allocation through an oversized
/// `WWW-Authenticate` challenge value.
const MAX_CHALLENGE_BYTES: usize = 16 * 1024;

/// A `claims` request-parameter value, ready to be sent as plain,
/// URL-encoded JSON on an authorization request.
///
/// [`fmt::Debug`] never prints the JSON itself: a challenge-derived value
/// can carry tenant- or policy-specific claim identifiers that should not
/// end up in logs or error messages.
#[derive(Clone, PartialEq, Eq)]
pub struct ClaimsParameter(String);

impl ClaimsParameter {
    /// The `cp1` client-capability declaration Microsoft identity platform
    /// expects on every authorization request to enable Continuous Access
    /// Evaluation.
    #[must_use]
    pub fn cp1_capability() -> Self {
        Self(cp1_only_document().to_string())
    }

    /// The parameter's plain (not base64) JSON text - the caller (or the
    /// [`url`] crate's query-pair builder) is responsible for
    /// percent-encoding it when placing it in a URL.
    #[must_use]
    pub fn as_json(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ClaimsParameter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ClaimsParameter")
            .field(&"<redacted>")
            .finish()
    }
}

fn cp1_only_document() -> Value {
    serde_json::json!({ "access_token": { "xms_cc": { "values": ["cp1"] } } })
}

/// A validated `claims` challenge extracted from a Microsoft Graph
/// `WWW-Authenticate: Bearer error="insufficient_claims", claims="…"`
/// response.
///
/// [`fmt::Debug`] never prints the challenge content, for the same reason
/// as [`ClaimsParameter`].
#[derive(Clone, PartialEq, Eq)]
pub struct ClaimsChallenge {
    document: Map<String, Value>,
}

impl fmt::Debug for ClaimsChallenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ClaimsChallenge")
            .field(&"<redacted>")
            .finish()
    }
}

impl ClaimsChallenge {
    /// Parses a challenge's raw `claims` value as it would appear in a
    /// `WWW-Authenticate` header's `claims="…"` parameter (already
    /// extracted from the surrounding header grammar by the caller).
    ///
    /// Microsoft Graph issues this base64-encoded, but the exact variant is
    /// not documented as stable, so this accepts URL-safe or standard
    /// alphabet base64, padded or not; a literal JSON object is also
    /// accepted, for any surface that does not base64-encode it.
    ///
    /// # Errors
    ///
    /// Returns [`OAuthError::MalformedClaimsChallenge`] if `raw` (or its
    /// decoded form) exceeds [`MAX_CHALLENGE_BYTES`], does not decode to
    /// valid JSON, is not a JSON object, or has a non-object `access_token`
    /// member.
    pub fn parse(raw: &str) -> Result<Self, OAuthError> {
        if raw.len() > MAX_CHALLENGE_BYTES {
            return Err(malformed(
                "the claims challenge exceeded the maximum accepted size",
            ));
        }
        let decoded = decode_challenge_bytes(raw)?;
        if decoded.len() > MAX_CHALLENGE_BYTES {
            return Err(malformed(
                "the claims challenge exceeded the maximum accepted size",
            ));
        }
        let value: Value = serde_json::from_slice(&decoded)
            .map_err(|_| malformed("the claims challenge was not valid JSON"))?;
        let Value::Object(document) = value else {
            return Err(malformed("the claims challenge was not a JSON object"));
        };
        if let Some(access_token) = document.get("access_token")
            && !access_token.is_object()
        {
            return Err(malformed(
                "the claims challenge's `access_token` member was not a JSON object",
            ));
        }
        Ok(Self { document })
    }

    /// Merges this challenge's `access_token` claims with the client's
    /// `cp1` capability declaration, preserving every other field the
    /// challenge carried - both other `access_token` claims and any other
    /// top-level member, including any capability values or other members
    /// an existing `xms_cc` declaration already carried - and returns the
    /// value ready to place on a fresh authorization request's `claims`
    /// parameter. `cp1` is added at most once, even if the challenge
    /// already declared it.
    #[must_use]
    pub fn merge_with_cp1(&self) -> ClaimsParameter {
        let mut document = self.document.clone();
        let access_token = document
            .entry("access_token".to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        // `parse` already rejected a non-object `access_token`, so this
        // always matches.
        if let Value::Object(access_token_claims) = access_token {
            merge_xms_cc_cp1(access_token_claims);
        }
        ClaimsParameter(Value::Object(document).to_string())
    }
}

/// Adds the `cp1` client capability to `access_token_claims`'s `xms_cc`
/// member without dropping anything already there: an existing `values`
/// array keeps its entries (deduplicating `cp1` if already present), and
/// any other member of an existing `xms_cc` object - or of `access_token`
/// itself - is left untouched.
fn merge_xms_cc_cp1(access_token_claims: &mut Map<String, Value>) {
    let Some(Value::Object(xms_cc)) = access_token_claims.get_mut("xms_cc") else {
        // No usable existing `xms_cc` declaration (missing, or not an
        // object) - install a fresh `cp1`-only one.
        access_token_claims.insert(
            "xms_cc".to_owned(),
            serde_json::json!({ "values": ["cp1"] }),
        );
        return;
    };
    match xms_cc.get_mut("values") {
        Some(Value::Array(values)) => {
            let already_declared = values.iter().any(|value| value.as_str() == Some("cp1"));
            if !already_declared {
                values.push(Value::String("cp1".to_owned()));
            }
        }
        _ => {
            xms_cc.insert("values".to_owned(), serde_json::json!(["cp1"]));
        }
    }
}

/// Decodes `raw` as base64 (trying the URL-safe and standard alphabets,
/// padded and unpadded) or, if it looks like a literal JSON object already,
/// returns it as-is.
fn decode_challenge_bytes(raw: &str) -> Result<Vec<u8>, OAuthError> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        return Ok(trimmed.as_bytes().to_vec());
    }
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(trimmed))
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(trimmed))
        .map_err(|_| malformed("the claims challenge was neither valid base64 nor a JSON object"))
}

fn malformed(reason: &str) -> OAuthError {
    OAuthError::MalformedClaimsChallenge {
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use super::*;

    fn encode_url_safe_no_pad(json: &str) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.as_bytes())
    }

    fn encode_standard_padded(json: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
    }

    fn parsed(parameter: &ClaimsParameter) -> Value {
        serde_json::from_str(parameter.as_json()).expect("claims parameter is valid JSON")
    }

    #[test]
    fn cp1_capability_declares_the_client_capability() {
        let claims = ClaimsParameter::cp1_capability();
        assert_eq!(
            parsed(&claims),
            serde_json::json!({ "access_token": { "xms_cc": { "values": ["cp1"] } } })
        );
    }

    #[test]
    fn debug_output_never_contains_the_claims_parameter_content() {
        let claims = ClaimsParameter::cp1_capability();
        let formatted = format!("{claims:?}");
        assert!(!formatted.contains("xms_cc"));
        assert!(!formatted.contains("access_token"));
        assert!(formatted.contains("<redacted>"));
    }

    #[test]
    fn parses_a_url_safe_no_pad_base64_challenge() {
        let raw = encode_url_safe_no_pad(
            r#"{"access_token":{"nbf":{"essential":true,"value":"161234"}}}"#,
        );
        let challenge = ClaimsChallenge::parse(&raw).expect("valid challenge");
        let merged = challenge.merge_with_cp1();
        assert_eq!(
            parsed(&merged),
            serde_json::json!({
                "access_token": {
                    "nbf": { "essential": true, "value": "161234" },
                    "xms_cc": { "values": ["cp1"] },
                }
            })
        );
    }

    #[test]
    fn parses_a_standard_padded_base64_challenge() {
        let raw = encode_standard_padded(r#"{"access_token":{"acrs":{"values":["c1"]}}}"#);
        let challenge = ClaimsChallenge::parse(&raw).expect("valid challenge");
        let merged = challenge.merge_with_cp1();
        assert_eq!(
            parsed(&merged),
            serde_json::json!({
                "access_token": {
                    "acrs": { "values": ["c1"] },
                    "xms_cc": { "values": ["cp1"] },
                }
            })
        );
    }

    #[test]
    fn parses_a_literal_json_object_challenge() {
        let raw = r#"{"access_token":{"nbf":{"essential":true}}}"#;
        let challenge = ClaimsChallenge::parse(raw).expect("valid challenge");
        let merged = challenge.merge_with_cp1();
        assert_eq!(
            parsed(&merged),
            serde_json::json!({
                "access_token": {
                    "nbf": { "essential": true },
                    "xms_cc": { "values": ["cp1"] },
                }
            })
        );
    }

    #[test]
    fn merge_preserves_top_level_and_access_token_fields_it_did_not_touch() {
        let raw = encode_url_safe_no_pad(
            r#"{"access_token":{"acrs":["c1"]},"id_token":{"auth_time":{"essential":true}}}"#,
        );
        let challenge = ClaimsChallenge::parse(&raw).expect("valid challenge");
        let merged = challenge.merge_with_cp1();
        assert_eq!(
            parsed(&merged),
            serde_json::json!({
                "access_token": {
                    "acrs": ["c1"],
                    "xms_cc": { "values": ["cp1"] },
                },
                "id_token": { "auth_time": { "essential": true } },
            })
        );
    }

    #[test]
    fn merge_adds_an_access_token_member_when_the_challenge_lacked_one() {
        let raw = encode_url_safe_no_pad(r#"{"id_token":{"auth_time":{"essential":true}}}"#);
        let challenge = ClaimsChallenge::parse(&raw).expect("valid challenge");
        let merged = challenge.merge_with_cp1();
        assert_eq!(
            parsed(&merged),
            serde_json::json!({
                "access_token": { "xms_cc": { "values": ["cp1"] } },
                "id_token": { "auth_time": { "essential": true } },
            })
        );
    }

    #[test]
    fn merge_preserves_an_existing_xms_cc_capability_and_other_members_while_adding_cp1() {
        // A challenge that already carries its own `xms_cc` declaration -
        // for example, one requesting a different client capability, or
        // one with extra members alongside `values` - must keep every bit
        // of that intact: `merge_with_cp1` only adds `cp1`, it never
        // replaces the whole `xms_cc` object (that would drop challenge
        // fields, violating the "preserve challenge fields" guarantee).
        let raw = encode_url_safe_no_pad(
            r#"{"access_token":{"xms_cc":{"values":["other_capability"],"essential":true}}}"#,
        );
        let challenge = ClaimsChallenge::parse(&raw).expect("valid challenge");
        let merged = challenge.merge_with_cp1();
        assert_eq!(
            parsed(&merged),
            serde_json::json!({
                "access_token": {
                    "xms_cc": {
                        "values": ["other_capability", "cp1"],
                        "essential": true,
                    },
                }
            })
        );
    }

    #[test]
    fn merge_does_not_duplicate_cp1_when_the_challenge_already_declared_it() {
        let raw = encode_url_safe_no_pad(r#"{"access_token":{"xms_cc":{"values":["cp1"]}}}"#);
        let challenge = ClaimsChallenge::parse(&raw).expect("valid challenge");
        let merged = challenge.merge_with_cp1();
        assert_eq!(
            parsed(&merged),
            serde_json::json!({
                "access_token": { "xms_cc": { "values": ["cp1"] } }
            })
        );
    }

    #[test]
    fn debug_output_never_contains_the_challenge_content() {
        let raw = encode_url_safe_no_pad(r#"{"access_token":{"acrs":["super-secret-policy"]}}"#);
        let challenge = ClaimsChallenge::parse(&raw).expect("valid challenge");
        let formatted = format!("{challenge:?}");
        assert!(!formatted.contains("super-secret-policy"));
        assert!(!formatted.contains("acrs"));
        assert!(formatted.contains("<redacted>"));
    }

    #[test]
    fn rejects_a_challenge_that_is_neither_valid_base64_nor_json() {
        let raw = "not-json-and-not-base64-!!!";
        let error = ClaimsChallenge::parse(raw).expect_err("not a valid challenge");
        assert!(matches!(error, OAuthError::MalformedClaimsChallenge { .. }));
        assert!(!format!("{error}").contains(raw));
    }

    #[test]
    fn rejects_a_challenge_that_is_valid_base64_but_not_json() {
        let raw = encode_url_safe_no_pad("this is not json");
        let error = ClaimsChallenge::parse(&raw).expect_err("not a valid challenge");
        assert!(matches!(error, OAuthError::MalformedClaimsChallenge { .. }));
    }

    #[test]
    fn rejects_a_challenge_that_is_valid_json_but_not_an_object() {
        let raw = encode_url_safe_no_pad("[1, 2, 3]");
        let error = ClaimsChallenge::parse(&raw).expect_err("not a valid challenge");
        assert!(matches!(error, OAuthError::MalformedClaimsChallenge { .. }));
    }

    #[test]
    fn rejects_a_challenge_whose_access_token_member_is_not_an_object() {
        let raw = encode_url_safe_no_pad(r#"{"access_token":"not-an-object"}"#);
        let error = ClaimsChallenge::parse(&raw).expect_err("not a valid challenge");
        assert!(matches!(error, OAuthError::MalformedClaimsChallenge { .. }));
    }

    #[test]
    fn rejects_an_oversized_raw_challenge() {
        let raw = "a".repeat(MAX_CHALLENGE_BYTES + 1);
        let error = ClaimsChallenge::parse(&raw).expect_err("oversized challenge rejected");
        assert!(matches!(error, OAuthError::MalformedClaimsChallenge { .. }));
        assert!(!format!("{error}").contains(&raw));
    }
}
