//! Minimal Microsoft Graph client used only by OneDrive authorization
//! completion and connect/test verification (task 0110): `GET /me`,
//! `GET /me/drive`, and safely parsing a Conditional Access
//! `WWW-Authenticate: Bearer ..., error="insufficient_claims", claims="..."`
//! challenge.
//!
//! Deliberately separate from `fm-vfs-onedrive`'s own (private) Graph
//! plumbing, which is scoped to browsing/transfer operations, never
//! authorization. Parsing the `WWW-Authenticate` header's auth-param
//! grammar is explicitly out of `fm_auth_oauth::claims`' scope too (see that
//! module's docs) - this is the "whatever component makes the Microsoft
//! Graph request that can receive this challenge" it defers to.

use std::collections::HashMap;

use fm_auth_oauth::claims::ClaimsChallenge;
use fm_connections::OneDriveDriveType;
use reqwest::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use serde::Deserialize;
use url::Url;

/// Non-secret account identity plus drive classification captured once
/// authorization succeeds (task 0110: "capture non-secret display identity
/// ... and driveType").
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphIdentity {
    pub(crate) email: Option<String>,
    pub(crate) display_name: Option<String>,
    pub(crate) drive_type: OneDriveDriveType,
}

/// Typed, sanitized failure verifying access right after authorization (or
/// during a `/me/drive` connect/test dial). Never carries a raw response
/// body, header, URL, or token - only a coarse classification; callers turn
/// this into a fixed, pre-vetted-safe message.
#[derive(Debug)]
pub(crate) enum GraphVerifyError {
    /// A Conditional Access policy requires additional verification. The
    /// challenge itself is handed back so the caller can retain it in
    /// bounded in-memory attempt/account state for a fresh challenged
    /// authorization - never logged or returned raw.
    ConditionalAccessRequired(ClaimsChallenge),
    /// Microsoft Graph rejected the token outright (401 without a
    /// recognizable Conditional Access challenge).
    Unauthorized,
    /// Microsoft Graph denied access (403 without a recognizable
    /// Conditional Access challenge) - commonly a tenant policy blocking
    /// this application's access to Graph/OneDrive.
    Forbidden,
    /// A network-level failure.
    Transport(String),
    /// A response this client could not parse as the expected shape.
    Malformed(String),
}

#[derive(Deserialize, Default)]
struct MeResponse {
    mail: Option<String>,
    #[serde(rename = "userPrincipalName")]
    user_principal_name: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Deserialize, Default)]
struct DriveResponse {
    #[serde(rename = "driveType")]
    drive_type: Option<String>,
}

fn parse_drive_type(raw: Option<&str>) -> OneDriveDriveType {
    match raw {
        Some("personal") => OneDriveDriveType::Personal,
        Some("business") => OneDriveDriveType::Business,
        Some("documentLibrary") => OneDriveDriveType::DocumentLibrary,
        _ => OneDriveDriveType::Unknown,
    }
}

/// Verifies granted access and captures identity by calling `GET /me` and
/// `GET /me/drive` against `graph_base_url` with `access_token` (task 0110's
/// authorization-success verification step).
pub(crate) async fn verify_and_fetch_identity(
    http: &reqwest::Client,
    graph_base_url: &Url,
    access_token: &str,
) -> Result<GraphIdentity, GraphVerifyError> {
    let me: MeResponse = get_json(http, graph_base_url, "me", access_token).await?;
    let drive_type = verify_drive_access(http, graph_base_url, access_token).await?;
    Ok(GraphIdentity {
        email: me.mail.or(me.user_principal_name),
        display_name: me.display_name,
        drive_type,
    })
}

/// Verifies `/me/drive` alone (task 0110's `ConnectionDialer`, re-run on
/// every connect/test - never blocks either personal or business
/// `driveType`).
pub(crate) async fn verify_drive_access(
    http: &reqwest::Client,
    graph_base_url: &Url,
    access_token: &str,
) -> Result<OneDriveDriveType, GraphVerifyError> {
    let drive: DriveResponse = get_json(http, graph_base_url, "me/drive", access_token).await?;
    Ok(parse_drive_type(drive.drive_type.as_deref()))
}

async fn get_json<T: serde::de::DeserializeOwned + Default>(
    http: &reqwest::Client,
    graph_base_url: &Url,
    path: &str,
    access_token: &str,
) -> Result<T, GraphVerifyError> {
    let mut url = graph_base_url.clone();
    {
        let mut segments = url.path_segments_mut().map_err(|()| {
            GraphVerifyError::Malformed("invalid Microsoft Graph base URL".to_owned())
        })?;
        segments.pop_if_empty();
        for part in path.split('/') {
            segments.push(part);
        }
    }
    let response = http
        .get(url)
        .header(AUTHORIZATION, format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|error| GraphVerifyError::Transport(error.to_string()))?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        if let Some(challenge) = parse_conditional_access_challenge(&response) {
            return Err(GraphVerifyError::ConditionalAccessRequired(challenge));
        }
        return Err(if status == reqwest::StatusCode::UNAUTHORIZED {
            GraphVerifyError::Unauthorized
        } else {
            GraphVerifyError::Forbidden
        });
    }
    if !status.is_success() {
        return Err(GraphVerifyError::Malformed(format!(
            "Microsoft Graph request failed with status {status}"
        )));
    }
    response.json::<T>().await.map_err(|_| {
        GraphVerifyError::Malformed(
            "Microsoft Graph returned a response this client could not parse".to_owned(),
        )
    })
}

/// Parses a `WWW-Authenticate` response header for an
/// `error="insufficient_claims", claims="<base64>"` Conditional Access
/// challenge (Microsoft Graph's documented shape).
fn parse_conditional_access_challenge(response: &reqwest::Response) -> Option<ClaimsChallenge> {
    let header = response.headers().get(WWW_AUTHENTICATE)?;
    let header = header.to_str().ok()?;
    let params = parse_auth_params(header);
    if params.get("error").map(String::as_str) != Some("insufficient_claims") {
        return None;
    }
    ClaimsChallenge::parse(params.get("claims")?).ok()
}

/// Parses the comma-separated `key="value"` parameters following an HTTP
/// `WWW-Authenticate` header's leading scheme token (`Bearer`).
fn parse_auth_params(header: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let Some((_scheme, rest)) = header.split_once(' ') else {
        return params;
    };
    for part in split_top_level_commas(rest) {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        params.insert(
            key.trim().to_owned(),
            value.trim().trim_matches('"').to_owned(),
        );
    }
    params
}

/// Splits on commas that are not inside a quoted value. A `claims="..."`
/// value is base64 and never itself contains a comma or quote, but this
/// still parses the quoting properly rather than assuming that, since a
/// sibling parameter (`realm`, `authorization_uri`) legitimately could.
fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    for (index, byte) in input.bytes().enumerate() {
        match byte {
            b'"' => in_quotes = !in_quotes,
            b',' if !in_quotes => {
                parts.push(input[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(input[start..].trim());
    parts
}

#[cfg(test)]
pub(crate) mod fixture {
    //! A tiny in-process Graph fixture serving `/me`/`/me/drive`, hand-rolled
    //! over a raw [`tokio::net::TcpStream`] like every other fixture in this
    //! workspace (`fm_auth_oauth::fixture`, `fm_vfs_onedrive::fixture`) -
    //! keeps this test-only helper from needing an HTTP server dependency
    //! this crate does not otherwise have.

    use std::collections::VecDeque;
    use std::net::SocketAddr;
    use std::sync::Arc;

    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    struct QueuedResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    }

    type Responses = Arc<Mutex<VecDeque<QueuedResponse>>>;
    type CapturedRequests = Arc<Mutex<Vec<String>>>;

    pub(crate) struct GraphFixture {
        addr: SocketAddr,
        responses: Responses,
        requests: CapturedRequests,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for GraphFixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    impl GraphFixture {
        pub(crate) async fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("fixture must bind a loopback port");
            let addr = listener
                .local_addr()
                .expect("bound listener must report its address");
            let responses: Responses = Arc::new(Mutex::new(VecDeque::new()));
            let requests: CapturedRequests = Arc::new(Mutex::new(Vec::new()));
            let served_responses = Arc::clone(&responses);
            let served_requests = Arc::clone(&requests);
            let task = tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    let responses = Arc::clone(&served_responses);
                    let requests = Arc::clone(&served_requests);
                    tokio::spawn(async move { serve(stream, responses, requests).await });
                }
            });
            Self {
                addr,
                responses,
                requests,
                task,
            }
        }

        pub(crate) fn base_url(&self) -> url::Url {
            url::Url::parse(&format!("http://{}", self.addr)).expect("loopback address parses")
        }

        pub(crate) async fn enqueue_json(&self, status: u16, body: serde_json::Value) {
            self.responses.lock().await.push_back(QueuedResponse {
                status,
                headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
                body: body.to_string(),
            });
        }

        pub(crate) async fn enqueue_conditional_access_challenge(
            &self,
            status: u16,
            claims_base64: &str,
        ) {
            self.responses.lock().await.push_back(QueuedResponse {
                status,
                headers: vec![(
                    "WWW-Authenticate".to_owned(),
                    format!(
                        "Bearer realm=\"\", authorization_uri=\"https://login.microsoftonline.com/common/oauth2/authorize\", error=\"insufficient_claims\", claims=\"{claims_base64}\""
                    ),
                )],
                body: String::new(),
            });
        }

        pub(crate) async fn enqueue_status(&self, status: u16) {
            self.responses.lock().await.push_back(QueuedResponse {
                status,
                headers: Vec::new(),
                body: String::new(),
            });
        }

        pub(crate) async fn requests(&self) -> Vec<String> {
            self.requests.lock().await.clone()
        }
    }

    async fn serve(
        stream: tokio::net::TcpStream,
        responses: Responses,
        requests: CapturedRequests,
    ) {
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);

        let mut request_line = String::new();
        if reader.read_line(&mut request_line).await.is_err() {
            return;
        }

        let mut content_length = 0usize;
        let mut authorization = None;
        loop {
            let mut header_line = String::new();
            match reader.read_line(&mut header_line).await {
                Ok(0) => return,
                Ok(_) if header_line == "\r\n" || header_line == "\n" => break,
                Ok(_) => {
                    let lower = header_line.to_ascii_lowercase();
                    if let Some(value) = lower.strip_prefix("content-length:") {
                        content_length = value.trim().parse().unwrap_or(0);
                    } else if lower.starts_with("authorization:") {
                        authorization =
                            Some(header_line["authorization:".len()..].trim().to_owned());
                    }
                }
                Err(_) => return,
            }
        }
        if content_length > 0 {
            let mut body = vec![0_u8; content_length];
            if reader.read_exact(&mut body).await.is_err() {
                return;
            }
        }
        requests.lock().await.push(format!(
            "{}{}",
            request_line.trim(),
            authorization
                .map(|value| format!(" | Authorization: {value}"))
                .unwrap_or_default()
        ));

        let queued = responses.lock().await.pop_front();
        let (status, headers, body) = match queued {
            Some(response) => (response.status, response.headers, response.body),
            None => (
                500,
                Vec::new(),
                "{\"error\":\"no fixture response queued\"}".to_owned(),
            ),
        };
        let mut head = format!(
            "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\n",
            reason_phrase(status),
            body.len()
        );
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("Connection: close\r\n\r\n");
        let _ = write_half.write_all(head.as_bytes()).await;
        let _ = write_half.write_all(body.as_bytes()).await;
        let _ = write_half.shutdown().await;
    }

    fn reason_phrase(status: u16) -> &'static str {
        match status {
            200 => "OK",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            _ => "Error",
        }
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use super::fixture::GraphFixture;
    use super::*;

    #[tokio::test]
    async fn verify_and_fetch_identity_captures_email_display_name_and_drive_type() {
        let fixture = GraphFixture::start().await;
        fixture
            .enqueue_json(
                200,
                serde_json::json!({
                    "mail": "erik@example.test",
                    "userPrincipalName": "erik@example.onmicrosoft.com",
                    "displayName": "Erik Vullings",
                }),
            )
            .await;
        fixture
            .enqueue_json(200, serde_json::json!({ "driveType": "business" }))
            .await;
        let http = reqwest::Client::new();

        let identity = verify_and_fetch_identity(&http, &fixture.base_url(), "access-token-value")
            .await
            .expect("verification must succeed");

        assert_eq!(identity.email.as_deref(), Some("erik@example.test"));
        assert_eq!(identity.display_name.as_deref(), Some("Erik Vullings"));
        assert_eq!(identity.drive_type, OneDriveDriveType::Business);
        let requests = fixture.requests().await;
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("GET /me "));
        assert!(requests[1].contains("GET /me/drive "));
        assert!(requests[0].contains("Authorization: Bearer access-token-value"));
    }

    #[tokio::test]
    async fn verify_and_fetch_identity_falls_back_to_user_principal_name_when_mail_is_absent() {
        let fixture = GraphFixture::start().await;
        fixture
            .enqueue_json(
                200,
                serde_json::json!({
                    "mail": null,
                    "userPrincipalName": "erik@contoso.onmicrosoft.com",
                    "displayName": "Erik Vullings",
                }),
            )
            .await;
        fixture
            .enqueue_json(200, serde_json::json!({ "driveType": "personal" }))
            .await;
        let http = reqwest::Client::new();

        let identity = verify_and_fetch_identity(&http, &fixture.base_url(), "token")
            .await
            .unwrap();

        assert_eq!(
            identity.email.as_deref(),
            Some("erik@contoso.onmicrosoft.com")
        );
        assert_eq!(identity.drive_type, OneDriveDriveType::Personal);
    }

    #[tokio::test]
    async fn an_unrecognized_drive_type_string_is_classified_as_unknown_without_failing() {
        let fixture = GraphFixture::start().await;
        fixture
            .enqueue_json(200, serde_json::json!({ "mail": "erik@example.test" }))
            .await;
        fixture
            .enqueue_json(200, serde_json::json!({ "driveType": "somethingNew" }))
            .await;
        let http = reqwest::Client::new();

        let identity = verify_and_fetch_identity(&http, &fixture.base_url(), "token")
            .await
            .unwrap();

        assert_eq!(identity.drive_type, OneDriveDriveType::Unknown);
    }

    #[tokio::test]
    async fn a_401_without_a_challenge_is_reported_as_unauthorized() {
        let fixture = GraphFixture::start().await;
        fixture.enqueue_status(401).await;
        let http = reqwest::Client::new();

        let error = verify_drive_access(&http, &fixture.base_url(), "token")
            .await
            .unwrap_err();

        assert!(matches!(error, GraphVerifyError::Unauthorized));
    }

    #[tokio::test]
    async fn a_403_without_a_challenge_is_reported_as_forbidden_tenant_policy() {
        let fixture = GraphFixture::start().await;
        fixture.enqueue_status(403).await;
        let http = reqwest::Client::new();

        let error = verify_drive_access(&http, &fixture.base_url(), "token")
            .await
            .unwrap_err();

        assert!(matches!(error, GraphVerifyError::Forbidden));
    }

    #[tokio::test]
    async fn a_stalled_graph_endpoint_is_bounded_by_the_http_clients_own_timeout_not_forever() {
        // Same guarantee as the token endpoint: a stalled Graph server must
        // be bounded by the HTTP client's own configured timeout, not hang
        // forever (task 0110 review finding 3).
        let stalled = crate::onedrive::test_support::StalledServer::start().await;
        let http = crate::onedrive::build_http_client(
            std::time::Duration::from_millis(150),
            std::time::Duration::from_millis(150),
        );

        let started = std::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            verify_drive_access(&http, &stalled.base_url(), "token"),
        )
        .await
        .expect("verify_drive_access must itself return well within this outer safety net");

        assert!(
            matches!(result, Err(GraphVerifyError::Transport(_))),
            "a stalled server must be reported as a transport failure, not hang or panic: {result:?}"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "must be bounded by the HTTP client's own ~150ms timeout, not an external one; took \
             {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_403_with_an_insufficient_claims_challenge_is_parsed_and_never_leaks_the_raw_header()
    {
        let fixture = GraphFixture::start().await;
        let raw_claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"access_token":{"nbf":{"essential":true,"value":"161234"}}}"#.as_bytes());
        fixture
            .enqueue_conditional_access_challenge(403, &raw_claims)
            .await;
        let http = reqwest::Client::new();

        let error = verify_drive_access(&http, &fixture.base_url(), "token")
            .await
            .unwrap_err();

        let GraphVerifyError::ConditionalAccessRequired(challenge) = error else {
            panic!("expected a Conditional Access challenge, got {error:?}");
        };
        // The parsed challenge is usable (merges cleanly) but the error's
        // own `Debug` never contains the raw base64 challenge text.
        let _ = challenge.merge_with_cp1();
        let formatted = format!(
            "{:?}",
            GraphVerifyError::ConditionalAccessRequired(challenge)
        );
        assert!(!formatted.contains(&raw_claims));
    }

    #[tokio::test]
    async fn a_401_with_an_insufficient_claims_challenge_is_also_parsed() {
        let fixture = GraphFixture::start().await;
        let raw_claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"access_token":{"acrs":["c1"]}}"#.as_bytes());
        fixture
            .enqueue_conditional_access_challenge(401, &raw_claims)
            .await;
        let http = reqwest::Client::new();

        let error = verify_drive_access(&http, &fixture.base_url(), "token")
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            GraphVerifyError::ConditionalAccessRequired(_)
        ));
    }

    #[tokio::test]
    async fn a_malformed_response_body_is_reported_as_malformed_not_a_panic() {
        let fixture = GraphFixture::start().await;
        fixture.enqueue_status(200).await;
        let http = reqwest::Client::new();

        let error = verify_drive_access(&http, &fixture.base_url(), "token")
            .await
            .unwrap_err();

        assert!(matches!(error, GraphVerifyError::Malformed(_)));
    }

    #[test]
    fn split_top_level_commas_respects_quoted_values() {
        let parts = split_top_level_commas(
            r#"realm="", authorization_uri="https://x/y,z", error="insufficient_claims", claims="abc""#,
        );
        assert_eq!(parts.len(), 4);
        assert!(parts[1].starts_with("authorization_uri="));
        assert!(parts[1].contains("https://x/y,z"));
    }

    #[test]
    fn parse_auth_params_extracts_error_and_claims() {
        let params = parse_auth_params(
            r#"Bearer realm="", error="insufficient_claims", claims="ZmFrZS1jbGFpbXM""#,
        );
        assert_eq!(
            params.get("error").map(String::as_str),
            Some("insufficient_claims")
        );
        assert_eq!(
            params.get("claims").map(String::as_str),
            Some("ZmFrZS1jbGFpbXM")
        );
    }
}
