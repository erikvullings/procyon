//! System-browser-compatible loopback callback listener (RFC 8252 §7.3).
//!
//! Binds an ephemeral `127.0.0.1` port up front so its exact port can be
//! embedded in the authorization request's `redirect_uri`, then waits for
//! exactly one browser redirect carrying either an authorization `code` or a
//! provider `error`, ignoring anything that does not carry the expected
//! `state` (spec §19: never treat a mismatched-state request as the awaited
//! callback - it may be stray browser traffic such as a `/favicon.ico`
//! request, or an adversarial probe of the loopback port).
//!
//! Hand-rolled over a raw [`tokio::net::TcpStream`] rather than built on
//! `axum`/`hyper`, matching `fm-vfs-webdav`'s fixture: this crate is a
//! layer-1 contract crate (`fm-test-support`'s architecture fitness test
//! reserves `axum`/`hyper` for `apps/` host binaries), and a one-shot,
//! one-request-at-a-time listener has no need for a full HTTP server stack.

use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::Zeroizing;

use crate::error::OAuthError;

/// Upper bound, in bytes, on a single request line or header line read from
/// an unauthenticated loopback connection. A local process that never sends
/// a newline (or sends one only after an enormous amount of data) must not
/// be able to grow an unbounded buffer in this process; any line exceeding
/// this bound is treated the same as a connection that closed early
/// (the connection is dropped without a response).
const MAX_LINE_BYTES: usize = 8 * 1024;

/// Upper bound on the number of header lines read before the blank line
/// terminating the headers - bounds total allocation from a connection that
/// sends many small header lines instead of one oversized one.
const MAX_HEADER_LINES: usize = 64;

/// An authorization code recovered from a loopback callback.
///
/// [`fmt::Debug`] never prints the code itself (spec §19).
#[derive(Clone)]
pub struct AuthorizationCode(Zeroizing<String>);

impl AuthorizationCode {
    fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    /// The code's characters, to send as `code` in the token exchange
    /// request.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AuthorizationCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AuthorizationCode")
            .field(&"<redacted>")
            .finish()
    }
}

/// A loopback HTTP listener that accepts exactly the system browser's
/// redirect back from the identity provider (RFC 8252's native-app
/// pattern).
///
/// Binds immediately so [`CallbackListener::redirect_uri`] can be embedded
/// in the authorization URL before the browser is opened; then
/// [`CallbackListener::listen`] blocks until a matching callback arrives,
/// the deadline passes, or the caller cancels.
pub struct CallbackListener {
    listener: TcpListener,
    redirect_uri: Url,
}

impl CallbackListener {
    /// Binds an ephemeral `127.0.0.1` port and derives its `redirect_uri` as
    /// `http://localhost:<port>/`, matching Microsoft identity platform's
    /// native-client `http://localhost` redirect URI registration (which
    /// accepts any concrete port at request time - the app registration
    /// itself carries no port; task 0110 implementation notes).
    pub async fn bind() -> Result<Self, OAuthError> {
        let listener =
            TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|error| OAuthError::Transport {
                    message: format!("failed to bind a loopback callback port: {error}"),
                })?;
        let port = listener
            .local_addr()
            .map_err(|error| OAuthError::Transport {
                message: format!("failed to read the loopback callback port: {error}"),
            })?
            .port();
        let redirect_uri = Url::parse(&format!("http://localhost:{port}/"))
            .expect("a loopback URL with a numeric port always parses");
        Ok(Self {
            listener,
            redirect_uri,
        })
    }

    /// The exact `redirect_uri` to embed in the authorization URL and later
    /// in the token exchange request - both must match the identity
    /// provider's registered redirect URI.
    #[must_use]
    pub fn redirect_uri(&self) -> &Url {
        &self.redirect_uri
    }

    /// The loopback address actually bound, mostly useful for logging.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.listener
            .local_addr()
            .expect("a bound listener always reports its address")
    }

    /// Waits for the browser's redirect carrying `expected_state`.
    ///
    /// Any request that does not carry exactly `expected_state` (wrong,
    /// missing, or a request to an unrelated path such as `/favicon.ico`) is
    /// answered and ignored rather than treated as the awaited callback.
    /// `timeout` bounds the whole wait; `cancellation` lets a caller (for
    /// example the user closing the sign-in window) abort it early.
    pub async fn listen(
        self,
        expected_state: &str,
        timeout: Duration,
        cancellation: CancellationToken,
    ) -> Result<AuthorizationCode, OAuthError> {
        let listener = self.listener;
        let work = async {
            loop {
                let (stream, _) = tokio::select! {
                    biased;
                    () = cancellation.cancelled() => return Err(OAuthError::Cancelled),
                    accepted = listener.accept() => accepted.map_err(|error| OAuthError::Transport {
                        message: format!("failed to accept a loopback connection: {error}"),
                    })?,
                };
                if let Some(outcome) = handle_connection(stream, expected_state).await {
                    return outcome;
                }
            }
        };
        match tokio::time::timeout(timeout, work).await {
            Ok(outcome) => outcome,
            Err(_) => Err(OAuthError::TimedOut),
        }
    }
}

/// Reads one HTTP/1.1 request, answers it, and reports whether it was the
/// awaited callback (`Some`) or should be ignored so the caller keeps
/// listening (`None`).
///
/// Only `GET /` is ever considered as a candidate callback; any other
/// method or path is answered and ignored regardless of its query string,
/// so a probe cannot even reach state validation by, say, guessing a path.
async fn handle_connection(
    stream: TcpStream,
    expected_state: &str,
) -> Option<Result<AuthorizationCode, OAuthError>> {
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);

    let request_line = read_line_bounded(&mut reader, MAX_LINE_BYTES).await?;
    if request_line.trim().is_empty() {
        return None;
    }
    if !drain_headers(&mut reader).await {
        return None;
    }

    let Some((method, target)) = parse_request_line(&request_line) else {
        respond(&mut write_half, 400, "malformed request").await;
        return None;
    };

    if method != "GET" {
        respond(&mut write_half, 405, "method not allowed").await;
        return None;
    }

    let Ok(parsed) = Url::parse(&format!("http://localhost{target}")) else {
        respond(&mut write_half, 400, "malformed request").await;
        return None;
    };

    if parsed.path() != "/" {
        respond(&mut write_half, 404, "not found").await;
        return None;
    }

    let mut state_values = Vec::new();
    let mut code_values = Vec::new();
    let mut error_values = Vec::new();
    let mut error_description = String::new();
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "state" => state_values.push(value.into_owned()),
            "code" => code_values.push(value.into_owned()),
            "error" => error_values.push(value.into_owned()),
            "error_description" => error_description = value.into_owned(),
            _ => {}
        }
    }

    // A duplicated `state` is inherently ambiguous - we cannot tell which
    // value (if either) is authentic, and an attacker could pad an extra
    // `state` to try to force a false match - so it is treated the same as
    // a mismatched state: ignored, not a terminal error.
    let state_matches = match state_values.as_slice() {
        [only] => only == expected_state,
        _ => false,
    };
    if !state_matches {
        respond(&mut write_half, 404, "not found").await;
        return None;
    }

    // Past this point the request's `state` genuinely matches, so it is the
    // awaited callback - but a duplicated or simultaneous `code`/`error`
    // means its shape does not match the identity provider's contract, and
    // must not be silently resolved by picking a value.
    if code_values.len() > 1
        || error_values.len() > 1
        || (!code_values.is_empty() && !error_values.is_empty())
    {
        respond(&mut write_half, 400, "malformed callback").await;
        return Some(Err(OAuthError::MalformedCallback {
            reason: "the callback matched the expected state but carried duplicate or \
                simultaneous `code`/`error` parameters"
                .to_owned(),
        }));
    }

    if let Some(code) = code_values.into_iter().next() {
        respond(
            &mut write_half,
            200,
            "Sign-in complete. You may close this window.",
        )
        .await;
        Some(Ok(AuthorizationCode::new(code)))
    } else if let Some(error) = error_values.into_iter().next() {
        respond(
            &mut write_half,
            200,
            "Sign-in failed. You may close this window.",
        )
        .await;
        Some(Err(OAuthError::from_provider_error(
            &error,
            &error_description,
        )))
    } else {
        respond(&mut write_half, 400, "malformed callback").await;
        Some(Err(OAuthError::MalformedCallback {
            reason:
                "the callback matched the expected state but carried neither `code` nor `error`"
                    .to_owned(),
        }))
    }
}

/// Reads and discards HTTP header lines up to the blank line terminating
/// them. Returns `false` if the connection ended before that blank line, if
/// a header line exceeded [`MAX_LINE_BYTES`], or if there were more than
/// [`MAX_HEADER_LINES`] of them.
async fn drain_headers<R: AsyncBufRead + Unpin>(reader: &mut R) -> bool {
    for _ in 0..MAX_HEADER_LINES {
        let Some(line) = read_line_bounded(reader, MAX_LINE_BYTES).await else {
            return false;
        };
        if line == "\r\n" || line == "\n" {
            return true;
        }
    }
    false
}

/// Reads one line (including its terminating `\n`) from `reader`, without
/// ever buffering more than `max_bytes`. Returns `None` if the connection
/// closed before a full line arrived, or if the line (so far) exceeded
/// `max_bytes` before a newline was found - both cases are handled
/// identically by the caller (drop the connection without a response),
/// which is what keeps this safe against an unauthenticated caller that
/// simply never sends a newline.
async fn read_line_bounded<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max_bytes: usize,
) -> Option<String> {
    let mut buf = Vec::new();
    loop {
        let available = reader.fill_buf().await.ok()?;
        if available.is_empty() {
            return None;
        }
        if let Some(newline_at) = available.iter().position(|&byte| byte == b'\n') {
            let take = newline_at + 1;
            if buf.len() + take > max_bytes {
                return None;
            }
            buf.extend_from_slice(&available[..take]);
            reader.consume(take);
            return String::from_utf8(buf).ok();
        }
        if buf.len() + available.len() > max_bytes {
            return None;
        }
        let consumed = available.len();
        buf.extend_from_slice(available);
        reader.consume(consumed);
    }
}

/// Splits an HTTP/1.1 request line (`METHOD target HTTP/1.1`) into its
/// method and target (`/path?query`). Requires all three tokens to be
/// present, matching real HTTP request lines.
fn parse_request_line(request_line: &str) -> Option<(&str, &str)> {
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    parts.next()?;
    Some((method, target))
}

async fn respond<W: tokio::io::AsyncWrite + Unpin>(write: &mut W, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = write.write_all(response.as_bytes()).await;
    let _ = write.shutdown().await;
}

/// Test-only helper letting other modules' tests construct an
/// [`AuthorizationCode`] directly, without driving a real loopback
/// connection just to obtain one.
#[cfg(test)]
pub(crate) mod test_support {
    use super::AuthorizationCode;

    pub(crate) fn authorization_code(value: &str) -> AuthorizationCode {
        AuthorizationCode::new(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    use super::*;

    async fn send_get(addr: SocketAddr, target: &str) -> String {
        send_raw(
            addr,
            format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
        )
        .await
    }

    /// Sends an arbitrary, possibly non-conformant, raw request and reads
    /// the response - used to exercise method/path/size hardening that
    /// `send_get` cannot express.
    async fn send_raw(addr: SocketAddr, raw_request: impl Into<String>) -> String {
        let mut stream = TcpStream::connect(addr)
            .await
            .expect("connect to the loopback listener");
        stream
            .write_all(raw_request.into().as_bytes())
            .await
            .expect("write the request");
        let mut response = String::new();
        // A rejected/ignored connection may be closed by the server without
        // a response body (or without any bytes at all); a read error or
        // empty read is a valid outcome here, not a test failure.
        let _ = stream.read_to_string(&mut response).await;
        response
    }

    #[tokio::test]
    async fn accepts_a_callback_carrying_the_expected_state_and_code() {
        let listener = CallbackListener::bind().await.expect("bind a listener");
        let addr = listener.local_addr();
        let task = tokio::spawn(async move {
            listener
                .listen(
                    "expected-state",
                    Duration::from_secs(5),
                    CancellationToken::new(),
                )
                .await
        });

        // Give the accept loop a moment to be ready before connecting - the
        // listener is already bound (that happened in `bind`), only the
        // `accept` call itself is asynchronous.
        tokio::time::sleep(Duration::from_millis(10)).await;
        let response = send_get(addr, "/?code=auth-code-123&state=expected-state").await;
        assert!(response.starts_with("HTTP/1.1 200"));

        let code = task
            .await
            .expect("listener task did not panic")
            .expect("callback succeeded");
        assert_eq!(code.as_str(), "auth-code-123");
    }

    #[tokio::test]
    async fn ignores_a_callback_with_a_mismatched_state_and_keeps_waiting() {
        let listener = CallbackListener::bind().await.expect("bind a listener");
        let addr = listener.local_addr();
        let task = tokio::spawn(async move {
            listener
                .listen(
                    "expected-state",
                    Duration::from_secs(5),
                    CancellationToken::new(),
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        let stray_response = send_get(addr, "/?code=stolen&state=wrong-state").await;
        assert!(stray_response.starts_with("HTTP/1.1 404"));

        let real_response = send_get(addr, "/?code=real-code&state=expected-state").await;
        assert!(real_response.starts_with("HTTP/1.1 200"));

        let code = task
            .await
            .expect("listener task did not panic")
            .expect("callback succeeded");
        assert_eq!(code.as_str(), "real-code");
    }

    #[tokio::test]
    async fn surfaces_a_matching_state_provider_error() {
        let listener = CallbackListener::bind().await.expect("bind a listener");
        let addr = listener.local_addr();
        let task = tokio::spawn(async move {
            listener
                .listen(
                    "expected-state",
                    Duration::from_secs(5),
                    CancellationToken::new(),
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        let response = send_get(
            addr,
            "/?error=access_denied&error_description=AADSTS65004%3A+declined&state=expected-state",
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 200"));

        let outcome = task.await.expect("listener task did not panic");
        assert!(matches!(outcome, Err(OAuthError::AccessDenied { .. })));
    }

    #[tokio::test]
    async fn times_out_when_no_callback_arrives() {
        let listener = CallbackListener::bind().await.expect("bind a listener");
        let outcome = listener
            .listen(
                "expected-state",
                Duration::from_millis(50),
                CancellationToken::new(),
            )
            .await;
        assert!(matches!(outcome, Err(OAuthError::TimedOut)));
    }

    #[tokio::test]
    async fn cancellation_stops_the_wait_immediately() {
        let listener = CallbackListener::bind().await.expect("bind a listener");
        let cancellation = CancellationToken::new();
        let cancel_clone = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            cancel_clone.cancel();
        });

        let outcome = listener
            .listen("expected-state", Duration::from_secs(30), cancellation)
            .await;
        assert!(matches!(outcome, Err(OAuthError::Cancelled)));
    }

    #[test]
    fn debug_output_never_contains_the_authorization_code() {
        let code = AuthorizationCode::new("super-secret-code".to_owned());
        let formatted = format!("{code:?}");
        assert!(!formatted.contains("super-secret-code"));
        assert!(formatted.contains("<redacted>"));
    }

    #[tokio::test]
    async fn rejects_a_non_get_method_and_keeps_waiting() {
        let listener = CallbackListener::bind().await.expect("bind a listener");
        let addr = listener.local_addr();
        let task = tokio::spawn(async move {
            listener
                .listen(
                    "expected-state",
                    Duration::from_secs(5),
                    CancellationToken::new(),
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        let stray_response = send_raw(
            addr,
            "POST /?code=stolen&state=expected-state HTTP/1.1\r\nHost: localhost\r\n\r\n",
        )
        .await;
        assert!(stray_response.starts_with("HTTP/1.1 405"));

        let real_response = send_get(addr, "/?code=real-code&state=expected-state").await;
        assert!(real_response.starts_with("HTTP/1.1 200"));

        let code = task
            .await
            .expect("listener task did not panic")
            .expect("callback succeeded");
        assert_eq!(code.as_str(), "real-code");
    }

    #[tokio::test]
    async fn rejects_a_non_root_path_and_keeps_waiting() {
        let listener = CallbackListener::bind().await.expect("bind a listener");
        let addr = listener.local_addr();
        let task = tokio::spawn(async move {
            listener
                .listen(
                    "expected-state",
                    Duration::from_secs(5),
                    CancellationToken::new(),
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        let stray_response = send_get(addr, "/callback?code=stolen&state=expected-state").await;
        assert!(stray_response.starts_with("HTTP/1.1 404"));

        let real_response = send_get(addr, "/?code=real-code&state=expected-state").await;
        assert!(real_response.starts_with("HTTP/1.1 200"));

        let code = task
            .await
            .expect("listener task did not panic")
            .expect("callback succeeded");
        assert_eq!(code.as_str(), "real-code");
    }

    #[tokio::test]
    async fn rejects_a_callback_with_both_code_and_error_as_malformed() {
        let listener = CallbackListener::bind().await.expect("bind a listener");
        let addr = listener.local_addr();
        let task = tokio::spawn(async move {
            listener
                .listen(
                    "expected-state",
                    Duration::from_secs(5),
                    CancellationToken::new(),
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        let response = send_get(
            addr,
            "/?code=real-code&error=access_denied&state=expected-state",
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 400"));

        let outcome = task.await.expect("listener task did not panic");
        assert!(matches!(outcome, Err(OAuthError::MalformedCallback { .. })));
    }

    #[tokio::test]
    async fn rejects_duplicate_state_and_keeps_waiting() {
        let listener = CallbackListener::bind().await.expect("bind a listener");
        let addr = listener.local_addr();
        let task = tokio::spawn(async move {
            listener
                .listen(
                    "expected-state",
                    Duration::from_secs(5),
                    CancellationToken::new(),
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        // Even though one of the two `state` values matches, the duplicate
        // is inherently ambiguous (an attacker could pad an extra `state`
        // hoping to confuse the check), so it must not be accepted.
        let stray_response = send_get(
            addr,
            "/?code=stolen&state=expected-state&state=expected-state",
        )
        .await;
        assert!(stray_response.starts_with("HTTP/1.1 404"));

        let real_response = send_get(addr, "/?code=real-code&state=expected-state").await;
        assert!(real_response.starts_with("HTTP/1.1 200"));

        let code = task
            .await
            .expect("listener task did not panic")
            .expect("callback succeeded");
        assert_eq!(code.as_str(), "real-code");
    }

    #[tokio::test]
    async fn rejects_duplicate_code_as_malformed() {
        let listener = CallbackListener::bind().await.expect("bind a listener");
        let addr = listener.local_addr();
        let task = tokio::spawn(async move {
            listener
                .listen(
                    "expected-state",
                    Duration::from_secs(5),
                    CancellationToken::new(),
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        let response = send_get(addr, "/?code=one-code&code=two-code&state=expected-state").await;
        assert!(response.starts_with("HTTP/1.1 400"));

        let outcome = task.await.expect("listener task did not panic");
        assert!(matches!(outcome, Err(OAuthError::MalformedCallback { .. })));
    }

    #[tokio::test]
    async fn rejects_duplicate_error_as_malformed() {
        let listener = CallbackListener::bind().await.expect("bind a listener");
        let addr = listener.local_addr();
        let task = tokio::spawn(async move {
            listener
                .listen(
                    "expected-state",
                    Duration::from_secs(5),
                    CancellationToken::new(),
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        let response = send_get(
            addr,
            "/?error=access_denied&error=invalid_request&state=expected-state",
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 400"));

        let outcome = task.await.expect("listener task did not panic");
        assert!(matches!(outcome, Err(OAuthError::MalformedCallback { .. })));
    }

    #[tokio::test]
    async fn rejects_an_oversized_request_line_and_keeps_waiting() {
        let listener = CallbackListener::bind().await.expect("bind a listener");
        let addr = listener.local_addr();
        let task = tokio::spawn(async move {
            listener
                .listen(
                    "expected-state",
                    Duration::from_secs(5),
                    CancellationToken::new(),
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        // A request line far larger than any real callback, sent by an
        // unauthenticated local process, must not be buffered without
        // bound - it should be dropped rather than accepted.
        let oversized_target = format!("/?state={}", "a".repeat(64 * 1024));
        let response = send_raw(
            addr,
            format!("GET {oversized_target} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
        )
        .await;
        assert!(response.is_empty());

        let real_response = send_get(addr, "/?code=real-code&state=expected-state").await;
        assert!(real_response.starts_with("HTTP/1.1 200"));

        let code = task
            .await
            .expect("listener task did not panic")
            .expect("callback succeeded");
        assert_eq!(code.as_str(), "real-code");
    }
}
