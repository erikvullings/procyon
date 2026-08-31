//! HTTP Digest authentication (RFC 2617 / RFC 7616 `MD5`/`MD5-sess`, `qop=auth`).
//!
//! WebDAV has no single standard auth scheme (task 0147's own acceptance
//! criteria), so both Basic and Digest must work. Basic is handled directly
//! by `reqwest::RequestBuilder::basic_auth`; Digest needs the
//! challenge/response dance implemented here, since neither `reqwest` nor
//! any workspace dependency provides it. Only `MD5`/`MD5-sess` with
//! `qop=auth` are implemented - the overwhelmingly common real-world
//! configuration (e.g. Apache `mod_dav`/`mod_auth_digest`); `auth-int` and
//! the RFC 7616 `SHA-256` algorithm are not implemented and are reported as
//! an explicit, typed error rather than silently mishandled.

use md5::{Digest, Md5};

/// A parsed `WWW-Authenticate: Digest ...` challenge.
#[derive(Debug, Clone)]
pub(crate) struct DigestChallenge {
    realm: String,
    nonce: String,
    opaque: Option<String>,
    qop: Option<String>,
    algorithm: Option<String>,
}

/// A challenge this module cannot answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum DigestError {
    /// The `WWW-Authenticate` header was not a `Digest` challenge, or was
    /// missing a mandatory `realm`/`nonce` parameter.
    #[error("not a well-formed Digest challenge")]
    MalformedChallenge,
    /// The challenge named an algorithm or qop this module does not
    /// implement (only `MD5`/`MD5-sess` with `qop=auth` are supported).
    #[error("unsupported Digest algorithm or qop: {0}")]
    Unsupported(String),
}

impl DigestChallenge {
    /// Parses a `WWW-Authenticate` header value.
    pub(crate) fn parse(header: &str) -> Result<Self, DigestError> {
        let rest = header
            .trim()
            .strip_prefix("Digest")
            .ok_or(DigestError::MalformedChallenge)?;
        let params = parse_auth_params(rest);
        let realm = params
            .get("realm")
            .cloned()
            .ok_or(DigestError::MalformedChallenge)?;
        let nonce = params
            .get("nonce")
            .cloned()
            .ok_or(DigestError::MalformedChallenge)?;
        let algorithm = params.get("algorithm").cloned();
        if let Some(algorithm) = &algorithm
            && !algorithm.eq_ignore_ascii_case("MD5")
            && !algorithm.eq_ignore_ascii_case("MD5-sess")
        {
            return Err(DigestError::Unsupported(algorithm.clone()));
        }
        let qop = params.get("qop").cloned();
        if let Some(qop) = &qop
            && !qop.split(',').any(|value| value.trim() == "auth")
        {
            return Err(DigestError::Unsupported(qop.clone()));
        }
        Ok(Self {
            realm,
            nonce,
            opaque: params.get("opaque").cloned(),
            qop: qop.map(|_| "auth".to_owned()),
            algorithm,
        })
    }

    /// Builds the `Authorization: Digest ...` header value answering this
    /// challenge for one request.
    #[must_use]
    pub(crate) fn authorization(
        &self,
        username: &str,
        password: &str,
        method: &str,
        uri: &str,
        nonce_count: u32,
        client_nonce: &str,
    ) -> String {
        let ha1_base = hex_md5(&format!("{username}:{}:{password}", self.realm));
        let ha1 = if self
            .algorithm
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("MD5-sess"))
        {
            hex_md5(&format!("{ha1_base}:{}:{client_nonce}", self.nonce))
        } else {
            ha1_base
        };
        let ha2 = hex_md5(&format!("{method}:{uri}"));
        let nc = format!("{nonce_count:08x}");
        let response = if self.qop.is_some() {
            hex_md5(&format!(
                "{ha1}:{}:{nc}:{client_nonce}:auth:{ha2}",
                self.nonce
            ))
        } else {
            hex_md5(&format!("{ha1}:{}:{ha2}", self.nonce))
        };

        let mut header = format!(
            "Digest username=\"{username}\", realm=\"{}\", nonce=\"{}\", uri=\"{uri}\", response=\"{response}\"",
            self.realm, self.nonce
        );
        if let Some(algorithm) = &self.algorithm {
            header.push_str(&format!(", algorithm={algorithm}"));
        }
        if let Some(opaque) = &self.opaque {
            header.push_str(&format!(", opaque=\"{opaque}\""));
        }
        if self.qop.is_some() {
            header.push_str(&format!(", qop=auth, nc={nc}, cnonce=\"{client_nonce}\""));
        }
        header
    }
}

fn hex_md5(input: &str) -> String {
    let digest = Md5::digest(input.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Parses comma-separated `key=value` / `key="value"` auth parameters.
fn parse_auth_params(rest: &str) -> std::collections::HashMap<String, String> {
    let mut params = std::collections::HashMap::new();
    for part in split_auth_params(rest) {
        if let Some((key, value)) = part.split_once('=') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().trim_matches('"').to_owned();
            params.insert(key, value);
        }
    }
    params
}

/// Splits on commas that are not inside a quoted value.
fn split_auth_params(rest: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in rest.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ',' if !in_quotes => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

/// Generates a random client nonce for one Digest exchange.
#[must_use]
pub(crate) fn generate_client_nonce() -> String {
    let bytes: [u8; 16] = rand::random();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_typical_apache_challenge() {
        let header = r#"Digest realm="example.test", nonce="abc123", qop="auth", opaque="xyz", algorithm=MD5"#;
        let challenge = DigestChallenge::parse(header).expect("must parse");
        assert_eq!(challenge.realm, "example.test");
        assert_eq!(challenge.nonce, "abc123");
        assert_eq!(challenge.opaque.as_deref(), Some("xyz"));
    }

    #[test]
    fn rejects_a_non_digest_header() {
        assert_eq!(
            DigestChallenge::parse(r#"Basic realm="example.test""#).unwrap_err(),
            DigestError::MalformedChallenge
        );
    }

    #[test]
    fn rejects_an_unsupported_algorithm() {
        let header = r#"Digest realm="example.test", nonce="abc123", algorithm=SHA-256"#;
        assert_eq!(
            DigestChallenge::parse(header).unwrap_err(),
            DigestError::Unsupported("SHA-256".to_owned())
        );
    }

    #[test]
    fn authorization_matches_rfc_2617s_worked_example() {
        // RFC 2617 §3.5's worked example.
        let challenge = DigestChallenge {
            realm: "testrealm@host.com".to_owned(),
            nonce: "dcd98b7102dd2f0e8b11d0f600bfb0c093".to_owned(),
            opaque: Some("5ccc069c403ebaf9f0171e9517f40e41".to_owned()),
            qop: Some("auth".to_owned()),
            algorithm: None,
        };
        let header = challenge.authorization(
            "Mufasa",
            "Circle Of Life",
            "GET",
            "/dir/index.html",
            1,
            "0a4f113b",
        );
        assert!(header.contains(r#"response="6629fae49393a05397450978507c4ef1""#));
    }
}
