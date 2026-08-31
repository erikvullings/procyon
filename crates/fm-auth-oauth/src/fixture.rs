//! In-process fake identity-provider token endpoint for tests (task 0110).
//!
//! Mirrors `fm-vfs-webdav`'s fixture: a real HTTP/1.1 responder over a raw
//! [`tokio::net::TcpStream`] bound to an ephemeral loopback port, so
//! [`crate::token::exchange_authorization_code`]/
//! [`crate::token::refresh_access_token`] exercise the real wire format
//! (`POST` with a form-urlencoded body, a JSON response body) end to end -
//! without ever calling the real Microsoft identity platform from a test.
//! Public (not `#[cfg(test)]`) so `fm-application`'s future integration
//! tests can reuse it too.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::config::Authority;

/// One queued response the fixture hands out to the next request it
/// receives, in order.
struct QueuedResponse {
    status: u16,
    body: String,
}

type Responses = Arc<Mutex<VecDeque<QueuedResponse>>>;
type CapturedRequests = Arc<Mutex<Vec<String>>>;

/// A running in-process token-endpoint fixture.
///
/// Dropping it aborts its accept loop and every connection it served.
pub struct TokenEndpointFixture {
    addr: SocketAddr,
    responses: Responses,
    requests: CapturedRequests,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TokenEndpointFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TokenEndpointFixture {
    /// Starts a fixture on an ephemeral loopback port with an empty response
    /// queue.
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the fixture must bind a loopback port");
        let addr = listener
            .local_addr()
            .expect("a bound listener must report its address");
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

    /// The [`Authority`] pointing at this fixture, for building a
    /// [`crate::config::PublicClientConfig`] under test.
    #[must_use]
    pub fn authority(&self) -> Authority {
        Authority::from_base_url(
            format!("http://{}", self.addr)
                .parse()
                .expect("a loopback address always parses as a URL"),
        )
    }

    /// Queues a successful token response, as JSON, for the next request.
    pub async fn enqueue_success(
        &self,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_in: u64,
    ) {
        let body = serde_json::json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "expires_in": expires_in,
            "token_type": "Bearer",
            "scope": "offline_access Files.ReadWrite User.Read",
        })
        .to_string();
        self.responses
            .lock()
            .await
            .push_back(QueuedResponse { status: 200, body });
    }

    /// Queues an OAuth-shaped error response (`{"error": ..., "error_description": ...}`)
    /// for the next request.
    pub async fn enqueue_error(&self, status: u16, error: &str, error_description: &str) {
        let body = serde_json::json!({
            "error": error,
            "error_description": error_description,
        })
        .to_string();
        self.responses
            .lock()
            .await
            .push_back(QueuedResponse { status, body });
    }

    /// Queues an arbitrary raw response body, for exercising malformed
    /// responses.
    pub async fn enqueue_raw(&self, status: u16, body: &str) {
        self.responses.lock().await.push_back(QueuedResponse {
            status,
            body: body.to_owned(),
        });
    }

    /// The raw bodies of every request received so far, in order - useful
    /// for asserting a request never carried a `client_secret`.
    pub async fn requests(&self) -> Vec<String> {
        self.requests.lock().await.clone()
    }
}

async fn serve(stream: tokio::net::TcpStream, responses: Responses, requests: CapturedRequests) {
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).await.is_err() {
        return;
    }

    let mut content_length = 0usize;
    loop {
        let mut header_line = String::new();
        match reader.read_line(&mut header_line).await {
            Ok(0) => return,
            Ok(_) if header_line == "\r\n" || header_line == "\n" => break,
            Ok(_) => {
                if let Some(value) = header_line
                    .to_ascii_lowercase()
                    .strip_prefix("content-length:")
                {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            Err(_) => return,
        }
    }

    let mut body = vec![0_u8; content_length];
    if content_length > 0 && reader.read_exact(&mut body).await.is_err() {
        return;
    }
    let body_text = String::from_utf8_lossy(&body).into_owned();
    requests.lock().await.push(body_text);

    let queued = responses.lock().await.pop_front();
    let (status, response_body) = match queued {
        Some(response) => (response.status, response.body),
        None => (
            500,
            "{\"error\":\"server_error\",\"error_description\":\"no fixture response queued\"}"
                .to_owned(),
        ),
    };
    let reason = reason_phrase(status);
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response_body.len()
    );
    let _ = write_half.write_all(head.as_bytes()).await;
    let _ = write_half.write_all(response_body.as_bytes()).await;
    let _ = write_half.shutdown().await;
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        _ => "Internal Server Error",
    }
}
