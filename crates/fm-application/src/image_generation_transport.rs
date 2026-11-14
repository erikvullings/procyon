//! The images transports for the image role (task 0325).
//!
//! One endpoint description, resolved from the image role by
//! [`crate::llm_profiles::LlmProfileService`], is enough to reach a provider: the auth header and
//! the profile headers are common, and [`ImageApiStyle`] picks the request body and reply shape.
//! Two contracts are spoken today: the OpenAI-style images API that OpenAI-compatible servers and
//! hosted APIs answer, and Alibaba Cloud Model Studio's asynchronous task API behind `qwen-image`
//! and the `wan*` models. Local servers arrive later as presets of the first shape.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt as _;
use reqwest::header::CONTENT_TYPE;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::image_generation::{
    GeneratedBatch, ImageGenerationError, ImageProvider, MAX_IMAGE_BYTES, MAX_IMAGES, SourceImage,
    ValidatedImageRequest, images_edit_fields, images_request_body, parse_images_response,
};

/// Largest reply accepted from an images endpoint; base64 makes a reply about a third larger than
/// the images it carries.
const MAX_REPLY_BYTES: usize = 96 * 1024 * 1024;

/// Largest task reply accepted from an async endpoint: status text, never image bytes.
const MAX_TASK_REPLY_BYTES: usize = 1024 * 1024;

/// How often a running task is asked whether it is finished.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Which images contract an endpoint answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageApiStyle {
    /// `POST {base}/v1/images/generations`, answering with base64 in one reply.
    OpenAiImages,
    /// Alibaba Cloud Model Studio: `POST {base}/api/v1/services/aigc/text2image/image-synthesis`
    /// creates a task, which is then polled at `{task_url_prefix}/{task_id}` until it hands back
    /// image URLs on the same service.
    DashscopeTasks {
        /// `{base}/api/v1/tasks`, without a trailing slash.
        task_url_prefix: String,
    },
}

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
    /// Request timeout, at least [`super::MIN_IMAGE_TIMEOUT_SECONDS`]. It bounds the whole
    /// generation, including the time an async task spends queued and running.
    pub timeout: Duration,
    /// Which contract the endpoint answers.
    pub api: ImageApiStyle,
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
            .field("api", &self.api)
            .finish()
    }
}

/// Talks to one image endpoint in the contract its profile selects.
pub struct ImageEndpointTransport {
    endpoint: ImageEndpointRequest,
    client: reqwest::Client,
    poll_interval: Duration,
}

impl ImageEndpointTransport {
    /// A transport bound to one resolved endpoint, asking a running task once a second.
    #[must_use]
    pub fn new(endpoint: ImageEndpointRequest) -> Self {
        Self::with_poll_interval(endpoint, DEFAULT_POLL_INTERVAL)
    }

    /// As [`Self::new`], with the interval an async task is polled at. Only a local server or a
    /// test has a reason to ask more often.
    #[must_use]
    pub fn with_poll_interval(endpoint: ImageEndpointRequest, poll_interval: Duration) -> Self {
        Self {
            endpoint,
            client: reqwest::Client::new(),
            poll_interval,
        }
    }

    /// Carries the credential and the profile's own headers on every request to this endpoint.
    fn apply_headers(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut builder = builder.timeout(self.endpoint.timeout);
        if let Some(key) = &self.endpoint.api_key {
            builder = builder.header(self.endpoint.api_key_header, key);
        }
        for (name, value) in &self.endpoint.headers {
            builder = builder.header(name, value);
        }
        builder
    }

    /// Sends one request, giving up as soon as the person stops the generation.
    async fn send(
        &self,
        builder: reqwest::RequestBuilder,
        cancellation: &CancellationToken,
    ) -> Result<reqwest::Response, ImageGenerationError> {
        tokio::select! {
            () = cancellation.cancelled() => Err(ImageGenerationError::Cancelled),
            result = builder.send() => result.map_err(|_| {
                tracing::debug!("the image endpoint could not be reached");
                ImageGenerationError::Unavailable
            }),
        }
    }

    /// One request whose reply is read in full, refusing a reply that outgrows `limit`.
    async fn reply_bytes(
        &self,
        builder: reqwest::RequestBuilder,
        limit: usize,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, ImageGenerationError> {
        let response = self.send(builder, cancellation).await?;
        let status = response.status();
        if !status.is_success() {
            // The provider's own text is not shown: it can quote the prompt back.
            tracing::warn!(
                status = status.as_u16(),
                "image endpoint refused the request"
            );
            return Err(ImageGenerationError::Unavailable);
        }
        read_bounded(response, cancellation, limit).await
    }

    /// One OpenAI-style call: the images arrive base64-encoded in the reply itself. With a picture
    /// to start from the call goes to the edits endpoint beside it, which takes the file as a form
    /// part rather than as JSON.
    async fn generate_openai(
        &self,
        request: &ValidatedImageRequest,
        source: Option<&SourceImage>,
        cancellation: &CancellationToken,
    ) -> Result<GeneratedBatch, ImageGenerationError> {
        let builder = match source {
            None => self
                .apply_headers(self.client.post(&self.endpoint.url))
                .json(&images_request_body(&self.endpoint.model, request)),
            Some(source) => {
                let part = reqwest::multipart::Part::bytes(source.bytes.clone())
                    .file_name(source.name.clone())
                    .mime_str(source.media_type)
                    .map_err(|_| {
                        ImageGenerationError::InvalidRequest("the source image cannot be sent")
                    })?;
                let form = images_edit_fields(&self.endpoint.model, request)
                    .into_iter()
                    .fold(
                        reqwest::multipart::Form::new().part("image", part),
                        |form, (name, value)| form.text(name, value),
                    );
                self.apply_headers(self.client.post(edits_url(&self.endpoint.url)?))
                    .multipart(form)
            }
        };
        let bytes = self
            .reply_bytes(builder, MAX_REPLY_BYTES, cancellation)
            .await?;
        Ok(GeneratedBatch {
            model: self.endpoint.model.clone(),
            images: parse_images_response(&bytes)?,
        })
    }

    /// One Model Studio generation: create a task, wait for it, then fetch the images it hands
    /// back as URLs.
    async fn generate_dashscope(
        &self,
        request: &ValidatedImageRequest,
        task_url_prefix: &str,
        cancellation: &CancellationToken,
    ) -> Result<GeneratedBatch, ImageGenerationError> {
        let builder = self
            .apply_headers(self.client.post(&self.endpoint.url))
            .header("x-dashscope-async", "enable")
            .json(&dashscope_submit_body(&self.endpoint.model, request));
        let reply = self
            .reply_bytes(builder, MAX_TASK_REPLY_BYTES, cancellation)
            .await?;
        let task_id = task_id_of(&reply)?;
        let urls = self
            .await_task(task_url_prefix, &task_id, cancellation)
            .await?;
        let mut images = Vec::with_capacity(urls.len());
        for url in urls {
            images.push(self.fetch_image(&url, cancellation).await?);
        }
        Ok(GeneratedBatch {
            model: self.endpoint.model.clone(),
            images,
        })
    }

    /// Polls one task until it produces image URLs, is refused, is stopped, or runs out of the
    /// time the endpoint's timeout allows.
    async fn await_task(
        &self,
        task_url_prefix: &str,
        task_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<String>, ImageGenerationError> {
        let deadline = tokio::time::Instant::now() + self.endpoint.timeout;
        let poll_url = format!("{task_url_prefix}/{task_id}");
        loop {
            if tokio::time::Instant::now() >= deadline {
                tracing::warn!("the image task did not finish within its timeout");
                return Err(ImageGenerationError::Unavailable);
            }
            let builder = self.apply_headers(self.client.get(&poll_url));
            let reply = self
                .reply_bytes(builder, MAX_TASK_REPLY_BYTES, cancellation)
                .await?;
            match task_outcome_of(&reply)? {
                TaskOutcome::Ready(urls) => return Ok(urls),
                TaskOutcome::Running => {}
                TaskOutcome::Failed => {
                    tracing::warn!("the image endpoint gave up on the task");
                    return Err(ImageGenerationError::Unavailable);
                }
            }
            tokio::select! {
                () = cancellation.cancelled() => return Err(ImageGenerationError::Cancelled),
                () = tokio::time::sleep(self.poll_interval) => {}
            }
        }
    }

    /// Fetches one image the endpoint pointed at. The URL is the provider's, so it is only followed
    /// when it belongs to that provider's own service and says it is an image.
    async fn fetch_image(
        &self,
        url: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, ImageGenerationError> {
        let (Ok(image), Ok(endpoint)) = (
            reqwest::Url::parse(url),
            reqwest::Url::parse(&self.endpoint.url),
        ) else {
            tracing::warn!("the image endpoint returned a URL that could not be read");
            return Err(ImageGenerationError::Unavailable);
        };
        if !served_by_the_endpoint(&image, &endpoint) {
            tracing::warn!(
                host = image.host_str().unwrap_or_default(),
                "refusing to fetch an image from outside the endpoint's own service"
            );
            return Err(ImageGenerationError::Unavailable);
        }
        let builder = self.apply_headers(self.client.get(url));
        let response = self.send(builder, cancellation).await?;
        let status = response.status();
        if !status.is_success() {
            tracing::warn!(
                status = status.as_u16(),
                "the image store refused to hand the image over"
            );
            return Err(ImageGenerationError::Unavailable);
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !content_type.starts_with("image/") {
            tracing::warn!(
                content_type,
                "the image endpoint handed back something else"
            );
            return Err(ImageGenerationError::Unavailable);
        }
        let bytes = read_bounded(response, cancellation, MAX_IMAGE_BYTES).await?;
        if bytes.is_empty() {
            return Err(ImageGenerationError::Unavailable);
        }
        Ok(bytes)
    }
}

#[async_trait]
impl ImageProvider for ImageEndpointTransport {
    async fn generate(
        &self,
        request: &ValidatedImageRequest,
        source: Option<&SourceImage>,
        cancellation: &CancellationToken,
    ) -> Result<GeneratedBatch, ImageGenerationError> {
        match &self.endpoint.api {
            ImageApiStyle::OpenAiImages => {
                self.generate_openai(request, source, cancellation).await
            }
            ImageApiStyle::DashscopeTasks { task_url_prefix } => {
                if source.is_some() {
                    // The task API takes a picture by URL, so it would mean uploading the person's
                    // file to somebody else's storage first. That is not this app's call to make.
                    return Err(ImageGenerationError::SourceUnsupported);
                }
                self.generate_dashscope(request, task_url_prefix, cancellation)
                    .await
            }
        }
    }
}

/// The edits endpoint beside a generations endpoint: the same server, asked to change a picture
/// rather than invent one. An endpoint whose URL is not the documented pair cannot be sent there.
fn edits_url(url: &str) -> Result<String, ImageGenerationError> {
    url.strip_suffix("/generations")
        .map(|prefix| format!("{prefix}/edits"))
        .ok_or(ImageGenerationError::SourceUnsupported)
}

/// Builds the Model Studio submit body: the wording goes in `input`, the settings in `parameters`.
pub(crate) fn dashscope_submit_body(model: &str, request: &ValidatedImageRequest) -> Value {
    let mut input = serde_json::json!({ "prompt": request.prompt });
    if let Some(negative) = &request.negative_prompt {
        input["negative_prompt"] = serde_json::json!(negative);
    }
    let mut parameters = serde_json::json!({
        "size": format!("{}x{}", request.width, request.height),
        "n": request.count,
        // The person reviewed the prompt they wrote, so the service must not rewrite it, and a
        // watermark would end up in a file they save under their own name.
        "prompt_extend": false,
        "watermark": false,
    });
    if let Some(seed) = request.seed {
        parameters["seed"] = serde_json::json!(seed);
    }
    serde_json::json!({ "model": model, "input": input, "parameters": parameters })
}

/// The task the submit reply named.
fn task_id_of(bytes: &[u8]) -> Result<String, ImageGenerationError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| ImageGenerationError::Unavailable)?;
    value
        .get("output")
        .and_then(|output| output.get("task_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|task_id| !task_id.trim().is_empty())
        .ok_or(ImageGenerationError::Unavailable)
}

/// Where one poll found a task.
enum TaskOutcome {
    /// Still queued or drawing; ask again later.
    Running,
    /// Finished, with the URL of every image it produced.
    Ready(Vec<String>),
    /// The endpoint gave up on it. Its own explanation is deliberately not kept.
    Failed,
}

/// Reads one task reply.
fn task_outcome_of(bytes: &[u8]) -> Result<TaskOutcome, ImageGenerationError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| ImageGenerationError::Unavailable)?;
    let output = value
        .get("output")
        .ok_or(ImageGenerationError::Unavailable)?;
    match output
        .get("task_status")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_uppercase()
        .as_str()
    {
        "PENDING" | "RUNNING" | "SUSPENDED" => Ok(TaskOutcome::Running),
        "SUCCEEDED" => {
            let items = output
                .get("results")
                .and_then(Value::as_array)
                .filter(|items| !items.is_empty())
                .ok_or(ImageGenerationError::Unavailable)?;
            if items.len() > usize::try_from(MAX_IMAGES).unwrap_or(usize::MAX) {
                // More images than the contract allows: refuse rather than fill the store.
                return Err(ImageGenerationError::Unavailable);
            }
            let urls = items
                .iter()
                .map(|item| {
                    item.get("url")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .filter(|url| !url.trim().is_empty())
                        .ok_or(ImageGenerationError::Unavailable)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(TaskOutcome::Ready(urls))
        }
        _ => Ok(TaskOutcome::Failed),
    }
}

/// Whether an image URL may be fetched because it belongs to the endpoint's own service.
///
/// A task reply carries URLs the person never typed, so they are only followed when they stay
/// within the endpoint's service: the same scheme and the same host or a sibling of its registered
/// domain, which is how Model Studio hands images over from object storage
/// (`dashscope.aliyuncs.com` to `dashscope-result-bj.oss-cn-beijing.aliyuncs.com`). Anything else
/// would let an endpoint turn the app into a fetcher for a host the person never chose.
fn served_by_the_endpoint(image: &reqwest::Url, endpoint: &reqwest::Url) -> bool {
    image.scheme() == endpoint.scheme()
        && match (image.host_str(), endpoint.host_str()) {
            (Some(host), Some(owner)) => {
                host.eq_ignore_ascii_case(owner)
                    || host
                        .to_ascii_lowercase()
                        .ends_with(&format!(".{}", registered_domain(owner)))
            }
            _ => false,
        }
}

/// The host a service is registered under: its last two labels, or the whole host if it has fewer.
fn registered_domain(host: &str) -> &str {
    let bytes = host.as_bytes();
    let outer = bytes.iter().rposition(|byte| *byte == b'.');
    match outer.and_then(|outer| {
        bytes[..outer]
            .iter()
            .rposition(|byte| *byte == b'.')
            .map(|inner| inner + 1)
    }) {
        Some(start) => &host[start..],
        None => host,
    }
}

/// Reads a reply body that is entirely in memory, giving up as soon as the person cancels and
/// refusing a reply that outgrows `limit`.
async fn read_bounded(
    response: reqwest::Response,
    cancellation: &CancellationToken,
    limit: usize,
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
        if bytes.len().saturating_add(chunk.len()) > limit {
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
                api: ImageApiStyle::OpenAiImages,
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
            quality: None,
            steps: None,
            source: None,
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
        let transport = ImageEndpointTransport::new(endpoint.request());
        let batch = transport
            .generate(&validated(), None, &CancellationToken::new())
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
        let transport = ImageEndpointTransport::new(ImageEndpointRequest {
            url: format!("http://{addr}/v1/images/generations"),
            model: "m".to_owned(),
            api_key: None,
            api_key_header: "authorization",
            headers: BTreeMap::new(),
            timeout: Duration::from_secs(10),
            api: ImageApiStyle::OpenAiImages,
        });
        let error = transport
            .generate(&validated(), None, &CancellationToken::new())
            .await
            .expect_err("refused");
        assert_eq!(error, ImageGenerationError::Unavailable);
        server.await.expect("server finished");
    }

    #[tokio::test]
    async fn cancelling_a_waiting_request_stops_it_and_reports_the_cancellation() {
        let endpoint = Endpoint::start(None).await;
        let transport = ImageEndpointTransport::new(endpoint.request());
        let cancellation = CancellationToken::new();
        let request = validated();
        let generate = transport.generate(&request, None, &cancellation);
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
            api: ImageApiStyle::OpenAiImages,
        };
        let shown = format!("{request:?}");
        assert!(shown.contains("[redacted]"));
        assert!(!shown.contains("sk-secret"));

        let task_based = ImageEndpointRequest {
            api: ImageApiStyle::DashscopeTasks {
                task_url_prefix: "https://dashscope.aliyuncs.com/api/v1/tasks".to_owned(),
            },
            ..request
        };
        let shown = format!("{task_based:?}");
        assert!(shown.contains("[redacted]"));
        assert!(!shown.contains("sk-secret"));
    }

    /// One canned reply: the content type it is served under and its bytes.
    type CannedReply = (String, Vec<u8>);

    /// The replies a fixture still owes, in the order it owes them.
    type ReplyQueue = Arc<tokio::sync::Mutex<std::collections::VecDeque<CannedReply>>>;

    /// A loopback endpoint that answers each request in turn from a queue of
    /// `(content-type, body)` pairs and records every request it received. Binding first and
    /// queueing later lets a test name one fixture's port inside another's reply.
    struct Script {
        addr: SocketAddr,
        replies: ReplyQueue,
        requests: Arc<Mutex<Vec<String>>>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for Script {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    impl Script {
        async fn bind(host: &str) -> Self {
            let ip = host
                .parse::<std::net::IpAddr>()
                .expect("a loopback address to bind");
            let listener = tokio::net::TcpListener::bind((ip, 0))
                .await
                .expect("fixture binds a loopback port");
            let addr = listener
                .local_addr()
                .expect("a bound listener reports its address");
            let replies: ReplyQueue =
                Arc::new(tokio::sync::Mutex::new(std::collections::VecDeque::new()));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let served = Arc::clone(&replies);
            let recorded = Arc::clone(&requests);
            let task = tokio::spawn(async move {
                loop {
                    let next = loop {
                        if let Some(next) = served.lock().await.pop_front() {
                            break next;
                        }
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    };
                    let Ok((mut stream, _)) = listener.accept().await else {
                        return;
                    };
                    let received = read_request(&mut stream).await;
                    recorded.lock().expect("record").push(received);
                    let (content_type, body) = next;
                    let header = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(&body).await;
                    let _ = stream.flush().await;
                }
            });
            Self {
                addr,
                replies,
                requests,
                task,
            }
        }

        async fn start(host: &str, replies: Vec<(&'static str, Vec<u8>)>) -> Self {
            let script = Self::bind(host).await;
            script.queue(replies).await;
            script
        }

        async fn queue(&self, replies: Vec<(&'static str, Vec<u8>)>) {
            self.replies.lock().await.extend(
                replies
                    .into_iter()
                    .map(|(kind, body)| (kind.to_owned(), body)),
            );
        }

        fn url(&self, path: &str) -> String {
            format!("http://{}{}", self.addr, path)
        }

        /// The same port under another host name, for a reply that names its image store
        /// differently from the way the endpoint was typed.
        fn url_as(&self, host: &str, path: &str) -> String {
            format!("http://{host}:{}{path}", self.addr.port())
        }

        /// The request lines and bodies it received, in order.
        fn received(&self) -> Vec<String> {
            self.requests.lock().expect("record").clone()
        }
    }

    const TASK_PATH: &str = "/api/v1/services/aigc/text2image/image-synthesis";

    fn json_reply(body: String) -> (&'static str, Vec<u8>) {
        ("application/json", body.into_bytes())
    }

    fn task_reply(status: &str, output: &str) -> String {
        format!("{{\"output\":{{\"task_status\":\"{status}\",{output}}},\"request_id\":\"req-1\"}}")
    }

    fn submitted(id: &str) -> String {
        task_reply("PENDING", &format!("\"task_id\":\"{id}\""))
    }

    fn succeeded_with_url(id: &str, url: &str) -> String {
        task_reply(
            "SUCCEEDED",
            &format!("\"task_id\":\"{id}\",\"results\":[{{\"url\":\"{url}\"}}]"),
        )
    }

    /// The bytes of a real PNG, served with whatever content type the test picks.
    fn png_bytes() -> Vec<u8> {
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

    fn dashscope_endpoint(
        submit_url: String,
        task_host: &str,
        task_port: u16,
    ) -> ImageEndpointRequest {
        ImageEndpointRequest {
            url: submit_url,
            model: "qwen-image".to_owned(),
            api_key: Some("Bearer secret".to_owned()),
            api_key_header: "authorization",
            headers: BTreeMap::from([("x-request-source".to_owned(), "procyon".to_owned())]),
            timeout: Duration::from_secs(10),
            api: ImageApiStyle::DashscopeTasks {
                task_url_prefix: format!("http://{task_host}:{task_port}/api/v1/tasks"),
            },
        }
    }

    #[tokio::test]
    async fn a_dashscope_task_is_submitted_then_polled_until_its_images_are_downloaded() {
        let images = Script::start("127.0.0.1", vec![("image/png", png_bytes())]).await;
        let endpoint = Script::start(
            "127.0.0.1",
            vec![
                json_reply(submitted("t-1")),
                json_reply(task_reply("RUNNING", "\"task_id\":\"t-1\"")),
                json_reply(succeeded_with_url("t-1", &images.url("/out.png"))),
            ],
        )
        .await;
        let transport = ImageEndpointTransport::with_poll_interval(
            dashscope_endpoint(endpoint.url(TASK_PATH), "127.0.0.1", endpoint.addr.port()),
            Duration::from_millis(5),
        );

        let batch = transport
            .generate(&validated(), None, &CancellationToken::new())
            .await
            .expect("generated");

        assert_eq!(batch.model, "qwen-image");
        assert_eq!(batch.images.len(), 1);
        let requests = endpoint.received();
        assert_eq!(requests.len(), 3, "one submit, then polls until ready");
        let (head, body) = requests[0]
            .split_once("\r\n\r\n")
            .expect("a body follows the headers");
        assert!(head.starts_with(&format!("POST {TASK_PATH} HTTP/1.1")));
        assert!(
            head.contains("x-dashscope-async: enable"),
            "a task is created asynchronously"
        );
        assert!(head.contains("authorization: Bearer secret"));
        assert!(head.contains("x-request-source: procyon"));
        let sent: serde_json::Value = serde_json::from_str(body).expect("json body");
        assert_eq!(sent["model"], "qwen-image");
        assert_eq!(sent["input"]["prompt"], "a lighthouse at dusk");
        assert_eq!(sent["input"]["negative_prompt"], "text, watermark");
        assert_eq!(sent["parameters"]["size"], "1024x768");
        assert_eq!(sent["parameters"]["n"], MAX_IMAGES);
        assert_eq!(sent["parameters"]["seed"], 42);
        // The person saw the prompt they wrote, so the endpoint must not rewrite it or stamp a
        // watermark onto a file they are about to save.
        assert_eq!(sent["parameters"]["prompt_extend"], false);
        assert_eq!(sent["parameters"]["watermark"], false);
        for (index, request) in requests.iter().enumerate().skip(1) {
            assert!(
                request.starts_with("GET /api/v1/tasks/t-1 HTTP/1.1"),
                "request {index} polls the task it was given"
            );
        }
        let fetched = images.received();
        assert_eq!(fetched.len(), 1, "the reply URL was fetched once");
        assert!(fetched[0].starts_with("GET /out.png HTTP/1.1"));
    }

    #[tokio::test]
    async fn a_failed_task_is_refused_without_showing_what_the_provider_said() {
        let endpoint = Script::start(
            "127.0.0.1",
            vec![
                json_reply(submitted("t-1")),
                json_reply(task_reply(
                    "FAILED",
                    "\"task_id\":\"t-1\",\"message\":\"prompt 'a lighthouse at dusk' is not allowed\"",
                )),
            ],
        )
        .await;
        let port = endpoint.addr.port();
        let transport = ImageEndpointTransport::with_poll_interval(
            dashscope_endpoint(endpoint.url(TASK_PATH), "127.0.0.1", port),
            Duration::from_millis(5),
        );
        let error = transport
            .generate(&validated(), None, &CancellationToken::new())
            .await
            .expect_err("refused");
        assert_eq!(error, ImageGenerationError::Unavailable);
        assert!(
            !error.to_string().contains("lighthouse"),
            "the provider's own text, which repeats the prompt, is not shown: {error}"
        );
    }

    #[tokio::test]
    async fn cancelling_while_a_task_is_running_stops_the_polling() {
        // Every poll answers PENDING, so only the person's own stop ends this.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture binds");
        let addr = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = [0_u8; 4096];
                let _ = stream.read(&mut buffer).await;
                let body = submitted("t-1");
                let header = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes()).await;
                let _ = stream.write_all(body.as_bytes()).await;
                let _ = stream.flush().await;
            }
        });
        let transport = ImageEndpointTransport::with_poll_interval(
            dashscope_endpoint(
                format!("http://{addr}{TASK_PATH}"),
                "127.0.0.1",
                addr.port(),
            ),
            Duration::from_millis(20),
        );
        let cancellation = CancellationToken::new();
        let request = validated();
        let generate = transport.generate(&request, None, &cancellation);
        let stop = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(60)).await;
            stop.cancel();
        });
        assert_eq!(
            generate.await.expect_err("cancelled"),
            ImageGenerationError::Cancelled
        );
        server.abort();
    }

    #[tokio::test]
    async fn an_image_url_outside_the_endpoint_domain_is_never_fetched() {
        let endpoint = Script::bind("127.0.0.1").await;
        let elsewhere = Script::bind("127.0.0.1").await;
        // The same interface, reached under a host name the endpoint does not own.
        let elsewhere_url = elsewhere.url_as("not-the-endpoint.localhost", "/out.png");
        endpoint
            .queue(vec![
                json_reply(submitted("t-2")),
                json_reply(succeeded_with_url("t-2", &elsewhere_url)),
            ])
            .await;
        elsewhere.queue(vec![("image/png", png_bytes())]).await;
        let port = endpoint.addr.port();
        let transport = ImageEndpointTransport::with_poll_interval(
            dashscope_endpoint(endpoint.url(TASK_PATH), "127.0.0.1", port),
            Duration::from_millis(5),
        );
        assert_eq!(
            transport
                .generate(&validated(), None, &CancellationToken::new())
                .await
                .expect_err("another host is not the endpoint's"),
            ImageGenerationError::Unavailable
        );
        assert!(
            elsewhere.received().is_empty(),
            "a host the endpoint does not own is never contacted"
        );
    }

    #[tokio::test]
    async fn a_reply_that_is_not_an_image_is_refused() {
        let images =
            Script::start("127.0.0.1", vec![("application/pdf", b"%PDF-1.4".to_vec())]).await;
        let endpoint = Script::start(
            "127.0.0.1",
            vec![
                json_reply(submitted("t-3")),
                json_reply(succeeded_with_url("t-3", &images.url("/doc.pdf"))),
            ],
        )
        .await;
        let port = endpoint.addr.port();
        let transport = ImageEndpointTransport::with_poll_interval(
            dashscope_endpoint(endpoint.url(TASK_PATH), "127.0.0.1", port),
            Duration::from_millis(5),
        );
        assert_eq!(
            transport
                .generate(&validated(), None, &CancellationToken::new())
                .await
                .expect_err("not an image"),
            ImageGenerationError::Unavailable
        );
        assert_eq!(images.received().len(), 1, "it was fetched, then refused");
    }

    /// A picture to start from goes to the edits endpoint beside the generations one, as a form part
    /// the provider reads as a file rather than as JSON it has to decode first.
    #[tokio::test]
    async fn a_source_picture_is_posted_to_the_edits_endpoint_as_a_form_part() {
        let endpoint = Endpoint::start(Some(reply(1))).await;
        let transport = ImageEndpointTransport::new(endpoint.request());
        let source = SourceImage {
            name: "harbour.png".to_owned(),
            media_type: "image/png",
            bytes: encoded_png(),
        };
        let batch = transport
            .generate(&validated(), Some(&source), &CancellationToken::new())
            .await
            .expect("generated");
        assert_eq!(batch.images.len(), 1);
        let seen = endpoint.received();
        let (head, body) = seen.split_once("\r\n\r\n").expect("a body follows");
        assert!(
            head.starts_with("POST /v1/images/edits HTTP/1.1"),
            "the ask beside a picture goes to the edits endpoint: {head}"
        );
        assert!(
            head.contains("content-type: multipart/form-data; boundary="),
            "the file travels as a form: {head}"
        );
        assert!(
            body.contains("name=\"image\"; filename=\"harbour.png\""),
            "the part is named the way the provider expects: {body}"
        );
        assert!(
            body.to_lowercase().contains("content-type: image/png"),
            "the part says what it is: {body}"
        );
        assert!(
            body.contains("name=\"prompt\"") && body.contains("a lighthouse at dusk"),
            "the ask travels beside the picture: {body}"
        );
    }

    /// A server whose URL is not the documented pair has no edits endpoint to guess at, so the
    /// person is told rather than a request sent somewhere arbitrary with their file in it.
    #[tokio::test]
    async fn an_endpoint_outside_the_documented_pair_is_not_asked_to_edit_a_picture() {
        let endpoint = Endpoint::start(Some(reply(1))).await;
        let mut request = endpoint.request();
        request.url = format!("http://{}/custom/draw", endpoint.addr);
        let transport = ImageEndpointTransport::new(request);
        let source = SourceImage {
            name: "harbour.png".to_owned(),
            media_type: "image/png",
            bytes: encoded_png(),
        };
        assert_eq!(
            transport
                .generate(&validated(), Some(&source), &CancellationToken::new())
                .await,
            Err(ImageGenerationError::SourceUnsupported)
        );
    }

    /// The task API takes a starting picture by URL, which would mean uploading the person's file to
    /// somebody else's storage first. That is refused where they can still act on it.
    #[tokio::test]
    async fn a_task_endpoint_is_not_asked_to_start_from_a_picture_it_cannot_take() {
        let endpoint = Script::start("127.0.0.1", Vec::new()).await;
        let port = endpoint.addr.port();
        let transport = ImageEndpointTransport::with_poll_interval(
            dashscope_endpoint(endpoint.url(TASK_PATH), "127.0.0.1", port),
            Duration::from_millis(5),
        );
        let source = SourceImage {
            name: "harbour.png".to_owned(),
            media_type: "image/png",
            bytes: encoded_png(),
        };
        assert_eq!(
            transport
                .generate(&validated(), Some(&source), &CancellationToken::new())
                .await,
            Err(ImageGenerationError::SourceUnsupported)
        );
        assert!(
            endpoint.received().is_empty(),
            "the person's file is not sent to an endpoint that cannot take it"
        );
    }
}
