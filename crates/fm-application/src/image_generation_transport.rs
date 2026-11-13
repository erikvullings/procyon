//! The OpenAI-style images transport for the image role (task 0325).
//!
//! One endpoint description, resolved from the image role by
//! [`crate::llm_profiles::LlmProfileService`], is enough for every provider supported today: the
//! request body, the reply shape and the auth header are the same for OpenAI-compatible servers and
//! hosted APIs. Local servers arrive later as presets of the same shape.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt as _;
use tokio_util::sync::CancellationToken;

use crate::image_generation::{
    GeneratedBatch, ImageGenerationError, ImageProvider, ValidatedImageRequest,
    images_request_body, parse_images_response,
};

/// Largest reply accepted from an images endpoint; base64 makes a reply about a third larger than
/// the images it carries.
const MAX_REPLY_BYTES: usize = 96 * 1024 * 1024;

/// Where and how to reach one image endpoint. The credential is resolved by the profile capability,
/// so the secret never leaves it, and the timeout is already floored for image generation.
#[derive(Clone, PartialEq, Eq)]
pub struct ImageEndpointRequest {
    /// Full images endpoint URL.
    pub url: String,
    /// Model the image role assigned.
    pub model: String,
    /// Value for the auth header, already shaped the way the provider wants it.
    pub api_key: Option<String>,
    /// Header the key travels in.
    pub api_key_header: &'static str,
    /// Extra headers from the profile.
    pub headers: BTreeMap<String, String>,
    /// Request timeout, at least [`super::MIN_IMAGE_TIMEOUT_SECONDS`].
    pub timeout: Duration,
}

impl std::fmt::Debug for ImageEndpointRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ImageEndpointRequest")
            .field("url", &self.url)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .field("api_key_header", &self.api_key_header)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Talks to one images endpoint using the OpenAI-style `POST /v1/images/generations` contract.
pub struct OpenAiImagesTransport {
    endpoint: ImageEndpointRequest,
    client: reqwest::Client,
}

impl OpenAiImagesTransport {
    /// A transport bound to one resolved endpoint.
    #[must_use]
    pub fn new(endpoint: ImageEndpointRequest) -> Self {
        Self {
            endpoint,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl ImageProvider for OpenAiImagesTransport {
    async fn generate(
        &self,
        request: &ValidatedImageRequest,
        cancellation: &CancellationToken,
    ) -> Result<GeneratedBatch, ImageGenerationError> {
        let mut builder = self
            .client
            .post(&self.endpoint.url)
            .timeout(self.endpoint.timeout)
            .json(&images_request_body(&self.endpoint.model, request));
        if let Some(key) = &self.endpoint.api_key {
            builder = builder.header(self.endpoint.api_key_header, key);
        }
        for (name, value) in &self.endpoint.headers {
            builder = builder.header(name, value);
        }
        let response = tokio::select! {
            () = cancellation.cancelled() => return Err(ImageGenerationError::Cancelled),
            result = builder.send() => result.map_err(|_| {
                tracing::debug!("the image endpoint could not be reached");
                ImageGenerationError::Unavailable
            })?,
        };
        let status = response.status();
        if !status.is_success() {
            // The provider's own text is not shown: it can quote the prompt back.
            tracing::warn!(
                status = status.as_u16(),
                "image endpoint refused the request"
            );
            return Err(ImageGenerationError::Unavailable);
        }
        let bytes = read_bounded(response, cancellation).await?;
        let images = parse_images_response(&bytes)?;
        Ok(GeneratedBatch {
            model: self.endpoint.model.clone(),
            images,
        })
    }
}

/// Reads a reply body that is entirely in memory, giving up as soon as the person cancels and
/// refusing a reply that outgrows [`MAX_REPLY_BYTES`].
async fn read_bounded(
    response: reqwest::Response,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, ImageGenerationError> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    loop {
        let next = tokio::select! {
            () = cancellation.cancelled() => return Err(ImageGenerationError::Cancelled),
            next = stream.next() => next,
        };
        let Some(chunk) = next else {
            return Ok(bytes);
        };
        let chunk = chunk.map_err(|_| ImageGenerationError::Unavailable)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_REPLY_BYTES {
            return Err(ImageGenerationError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    use base64::Engine as _;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    use super::*;
    use crate::image_generation::MAX_IMAGES;

    /// The bytes one canned reply carries.
    fn encoded_png() -> Vec<u8> {
        let picture = image::DynamicImage::new_rgb8(8, 8).into_rgba8();
        let mut bytes = Vec::new();
        picture
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encodes");
        bytes
    }

    /// A one-shot images endpoint on loopback. It records the request it received and answers with
    /// `reply`; a `None` reply leaves the connection open so a test can watch cancellation win.
    struct Endpoint {
        addr: SocketAddr,
        received: Arc<Mutex<Option<String>>>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for Endpoint {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    /// Reads one request (headers plus the advertised body) and returns it as text.
    async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
        let mut seen: Vec<u8> = Vec::new();
        let mut buffer = vec![0_u8; 8 * 1024];
        let header_end = loop {
            let read = match stream.read(&mut buffer).await {
                Ok(0) | Err(_) => return String::from_utf8_lossy(&seen).into_owned(),
                Ok(read) => read,
            };
            seen.extend_from_slice(&buffer[..read]);
            if let Some(index) = seen.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
            if seen.len() > 256 * 1024 {
                return String::from_utf8_lossy(&seen).into_owned();
            }
        };
        let head = String::from_utf8_lossy(&seen[..header_end]).to_lowercase();
        let wanted = head
            .split("content-length:")
            .nth(1)
            .and_then(|rest| {
                rest.trim_start()
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
            })
            .and_then(|digits| digits.parse::<usize>().ok())
            .unwrap_or(0);
        while seen.len() < header_end + wanted {
            match stream.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => seen.extend_from_slice(&buffer[..read]),
            }
        }
        String::from_utf8_lossy(&seen).into_owned()
    }

    impl Endpoint {
        async fn start(reply: Option<String>) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("fixture binds a loopback port");
            let addr = listener
                .local_addr()
                .expect("a bound listener reports its address");
            let received = Arc::new(Mutex::new(None));
            let recorded = Arc::clone(&received);
            let task = tokio::spawn(async move {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let received_text = read_request(&mut stream).await;
                *recorded.lock().expect("record") = Some(received_text);
                let Some(body) = reply else {
                    // Stay open: only the caller's own cancellation ends the request.
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    return;
                };
                let header = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes()).await;
                let _ = stream.write_all(body.as_bytes()).await;
                let _ = stream.flush().await;
            });
            Self {
                addr,
                received,
                task,
            }
        }

        fn url(&self) -> String {
            format!("http://{}/v1/images/generations", self.addr)
        }

        fn request(&self) -> ImageEndpointRequest {
            ImageEndpointRequest {
                url: self.url(),
                model: "flux-schnell".to_owned(),
                api_key: Some("Bearer secret".to_owned()),
                api_key_header: "authorization",
                headers: BTreeMap::from([("x-request-source".to_owned(), "procyon".to_owned())]),
                timeout: Duration::from_secs(10),
            }
        }

        /// The request line and body the endpoint received.
        fn received(&self) -> String {
            self.received
                .lock()
                .expect("record")
                .clone()
                .expect("the request arrived")
        }
    }

    fn validated() -> ValidatedImageRequest {
        ValidatedImageRequest {
            prompt: "a lighthouse at dusk".to_owned(),
            negative_prompt: Some("text, watermark".to_owned()),
            width: 1_024,
            height: 768,
            count: MAX_IMAGES,
            seed: Some(42),
        }
    }

    fn reply(count: usize) -> String {
        let encoded = base64::engine::general_purpose::STANDARD.encode(encoded_png());
        let items = (0..count)
            .map(|_| format!("{{\"b64_json\":\"{encoded}\"}}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"data\":[{items}]}}")
    }

    #[tokio::test]
    async fn a_request_carries_the_prompt_size_and_key_and_returns_the_reply_images() {
        let endpoint = Endpoint::start(Some(reply(2))).await;
        let transport = OpenAiImagesTransport::new(endpoint.request());
        let batch = transport
            .generate(&validated(), &CancellationToken::new())
            .await
            .expect("generated");
        assert_eq!(batch.model, "flux-schnell");
        assert_eq!(batch.images.len(), 2);
        let seen = endpoint.received();
        let (head, body) = seen.split_once("\r\n\r\n").expect("a body follows");
        assert!(head.starts_with("POST /v1/images/generations HTTP/1.1"));
        assert!(
            head.contains("authorization: Bearer secret"),
            "the key travels in the header the provider wants"
        );
        assert!(
            head.contains("x-request-source: procyon"),
            "profile headers travel"
        );
        let sent: serde_json::Value = serde_json::from_str(body).expect("json body");
        assert_eq!(sent["model"], "flux-schnell");
        assert_eq!(sent["prompt"], "a lighthouse at dusk");
        assert_eq!(sent["negative_prompt"], "text, watermark");
        assert_eq!(sent["size"], "1024x768");
        assert_eq!(sent["n"], MAX_IMAGES);
        assert_eq!(sent["response_format"], "b64_json");
        assert_eq!(sent["seed"], 42);
    }

    #[tokio::test]
    async fn a_refusal_or_a_useless_reply_is_reported_without_leaking_it() {
        // A server that answers 500 never gets to explain itself to the person.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture binds");
        let addr = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer).await;
            let _ = stream
                .write_all(b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 5\r\n\r\nnope\n")
                .await;
            let _ = stream.flush().await;
        });
        let transport = OpenAiImagesTransport::new(ImageEndpointRequest {
            url: format!("http://{addr}/v1/images/generations"),
            model: "m".to_owned(),
            api_key: None,
            api_key_header: "authorization",
            headers: BTreeMap::new(),
            timeout: Duration::from_secs(10),
        });
        let error = transport
            .generate(&validated(), &CancellationToken::new())
            .await
            .expect_err("refused");
        assert_eq!(error, ImageGenerationError::Unavailable);
        server.await.expect("server finished");
    }

    #[tokio::test]
    async fn cancelling_a_waiting_request_stops_it_and_reports_the_cancellation() {
        let endpoint = Endpoint::start(None).await;
        let transport = OpenAiImagesTransport::new(endpoint.request());
        let cancellation = CancellationToken::new();
        let request = validated();
        let generate = transport.generate(&request, &cancellation);
        let stop = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            stop.cancel();
        });
        assert_eq!(
            generate.await.expect_err("cancelled"),
            ImageGenerationError::Cancelled
        );
    }

    #[tokio::test]
    async fn the_debug_view_never_carries_the_key() {
        let request = ImageEndpointRequest {
            url: "https://api.example.test/v1/images/generations".to_owned(),
            model: "m".to_owned(),
            api_key: Some("sk-secret".to_owned()),
            api_key_header: "authorization",
            headers: BTreeMap::new(),
            timeout: Duration::from_secs(120),
        };
        let shown = format!("{request:?}");
        assert!(shown.contains("[redacted]"));
        assert!(!shown.contains("sk-secret"));
    }
}
