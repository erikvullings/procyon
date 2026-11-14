//! Named OpenAI-compatible generation profiles (task 0184).
//!
//! Profiles are an application capability, not VFS connections. Durable JSON
//! stores only opaque credential references; secret values stay behind
//! [`CredentialStore`] and are resolved only for a bounded request.

#![allow(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fm_credentials::{CredentialRef, CredentialStore, SecretMaterial, StoreCredentialRequest};
use fm_settings::{SettingsStore, VersionedDocument};
use fm_transport_dto::{
    LlmAdvancedSettingsDto, LlmApiCapabilityDto, LlmEndpointLocalityDto, LlmModelLimitsDto,
    LlmPresetDto, LlmProfileDto, LlmProfileExportDto, LlmProfilePresetDto, LlmProfileTestResultDto,
    LlmRoleAssignmentDto, LlmRoleDto, LlmRolesDto, LlmTestErrorCategoryDto, LlmTlsPolicyDto,
    OrphanLlmCredentialDispositionDto, SaveLlmProfileRequestDto, SaveLlmRoleRequestDto,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use url::{Host, Url};
use uuid::Uuid;

use crate::image_generation_transport::{ImageApiStyle, ImageEndpointRequest};

const PROFILE_SCHEMA_VERSION: u32 = 5;
const TEST_PROMPT: &str = "Reply with OK.";
const MAX_STREAM_BYTES: usize = 256 * 1024;
/// Hard application bounds, independent of editable profile or provider metadata.
pub const MAX_GENERATION_CONTEXT_WINDOW: u32 = 1_048_576;
pub const MAX_GENERATION_ANSWER_TOKENS: u32 = 16_384;
const ALLOWED_CUSTOM_HEADERS: &[&str] =
    &["openai-organization", "openai-project", "x-request-source"];

/// Built-in safe configuration starting points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LlmPreset {
    Ollama,
    LmStudio,
    Vllm,
    Sglang,
    Omlx,
    MlxServe,
    /// Unsloth Studio: a local app whose OpenAI-compatible API answers on the Studio port (8888 by
    /// default). Whether its server takes the thinking-off switches is not documented, so it is left
    /// with none.
    Unsloth,
    OpenAiCompatible,
    AzureOpenAi,
    /// Alibaba Cloud Model Studio: `qwen-image` and the `wan*` image models. Its images API is an
    /// asynchronous task API rather than the OpenAI-style one.
    ModelStudio,
    /// Model Studio's Qwen-Image model, offered on its own so the image role can be filled without
    /// knowing which model id to type. `wan2.1` and the other Model Studio image models are reached
    /// by typing their id into the same profile.
    QwenImage,
}

/// How a local OpenAI-compatible server is asked not to emit reasoning (thinking) tokens. The
/// working variant differs per server and model, so the service learns it and remembers it on the
/// profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReasoningSwitch {
    /// `chat_template_kwargs.enable_thinking=false` and `reasoning_effort="none"`.
    Both,
    /// `chat_template_kwargs.enable_thinking=false` (vLLM, SGLang, oMLX; Qwen-style templates).
    TemplateKwargs,
    /// `reasoning_effort="none"` (LM Studio, gpt-oss style).
    EffortNone,
    /// No switch: the server rejects or ignores them.
    Plain,
}

impl ReasoningSwitch {
    /// Variants to try, most capable first, until the server accepts one.
    const LADDER: [Self; 4] = [
        Self::Both,
        Self::TemplateKwargs,
        Self::EffortNone,
        Self::Plain,
    ];

    /// The variant to use after this one was accepted but the model reasoned anyway.
    const fn after_ineffective(self) -> Self {
        match self {
            Self::TemplateKwargs => Self::EffortNone,
            Self::Both | Self::EffortNone | Self::Plain => Self::Plain,
        }
    }
}

const fn supports_reasoning_switch(preset: LlmPreset) -> bool {
    matches!(
        preset,
        LlmPreset::LmStudio
            | LlmPreset::Vllm
            | LlmPreset::Sglang
            | LlmPreset::Omlx
            | LlmPreset::MlxServe
    )
}

/// Protocol capability advertised by a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LlmApiCapability {
    ChatCompletions,
    Responses,
    ModelDiscovery,
    /// Answers `POST {base}/v1/images/generations` with base64 images in one reply.
    ImageGenerations,
    /// Answers Model Studio's async text-to-image task API (`qwen-image`, `wan*`).
    DashscopeTasks,
    /// The model reads images sent in chat messages. Set by the person; providers do not disclose it
    /// reliably, so the endpoint is never sent image bytes without it.
    Vision,
}

/// Network locality visible wherever a profile is selected. `Loopback` is the local class: the
/// loopback interface or a private LAN address. Everything else, including host names, is `Cloud`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EndpointLocality {
    Loopback,
    Cloud,
}

/// TLS constraints. Disabling certificate validation is deliberately absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LlmTlsPolicy {
    #[default]
    RequireValidCertificate,
    RequireHttps,
}

/// Conservative generation and transport settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmAdvancedSettings {
    pub context_window: u32,
    pub maximum_answer_tokens: u32,
    pub temperature: f32,
    pub timeout_seconds: u64,
    pub tls_policy: LlmTlsPolicy,
    pub custom_headers: BTreeMap<String, String>,
}

impl Default for LlmAdvancedSettings {
    fn default() -> Self {
        Self {
            context_window: 8_192,
            maximum_answer_tokens: 1_024,
            temperature: 0.2,
            timeout_seconds: 30,
            tls_policy: LlmTlsPolicy::RequireValidCertificate,
            custom_headers: BTreeMap::new(),
        }
    }
}

/// Reusable, non-secret generation endpoint profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmProfile {
    pub id: Uuid,
    pub name: String,
    pub preset: LlmPreset,
    pub base_url: String,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
    pub model: String,
    pub credential_ref: Option<CredentialRef>,
    pub advanced: LlmAdvancedSettings,
    pub capabilities: BTreeSet<LlmApiCapability>,
    /// Historical field name; stores the exact normalized endpoint in schema 4.
    pub consented_host: Option<String>,
    pub redact_filenames: bool,
    /// The thinking-off variant this server and model accepted last time (learned, not user-set).
    #[serde(default)]
    pub reasoning_switch: Option<ReasoningSwitch>,
}

/// User-editable profile values and an optional write-only API key.
#[derive(Clone)]
pub struct LlmProfileDraft {
    pub name: String,
    pub preset: LlmPreset,
    pub base_url: String,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
    pub model: String,
    pub api_key: Option<String>,
    pub advanced: LlmAdvancedSettings,
    pub capabilities: BTreeSet<LlmApiCapability>,
    pub redact_filenames: bool,
}

/// Explicit credential handling when deleting a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanCredentialDisposition {
    Delete,
    Retain,
}

/// Administrator-owned outbound host policy.
#[derive(Debug, Clone)]
pub struct LlmHostPolicy {
    allowed_cloud_hosts: BTreeSet<String>,
    allow_loopback: bool,
    allow_all_cloud_hosts: bool,
}

impl LlmHostPolicy {
    #[must_use]
    pub fn desktop() -> Self {
        Self {
            allowed_cloud_hosts: BTreeSet::new(),
            allow_loopback: true,
            allow_all_cloud_hosts: true,
        }
    }

    #[must_use]
    pub fn server(allowed_cloud_hosts: impl IntoIterator<Item = String>) -> Self {
        Self {
            allowed_cloud_hosts: allowed_cloud_hosts
                .into_iter()
                .map(|host| host.to_ascii_lowercase())
                .collect(),
            allow_loopback: false,
            allow_all_cloud_hosts: false,
        }
    }

    fn permits(&self, endpoint: &NormalizedEndpoint) -> bool {
        // Private LAN endpoints are local for consent but stay under the administrator's host list.
        if endpoint.loopback {
            return self.allow_loopback;
        }
        self.allow_all_cloud_hosts || self.allowed_cloud_hosts.contains(&endpoint.host)
    }
}

/// Stable normalized failure classes safe for diagnostics and UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LlmTestErrorCategory {
    InvalidConfiguration,
    PolicyDenied,
    ConsentRequired,
    Authentication,
    ModelUnavailable,
    Timeout,
    Tls,
    MalformedResponse,
    Cancelled,
    Transport,
}

impl LlmTestErrorCategory {
    /// The provider failure this category stands for, for callers that keep only the content-free
    /// category (e.g. a `Copy` answer error) but must still report the reason to the user.
    #[must_use]
    pub(crate) const fn into_error(self) -> LlmProfileError {
        match self {
            Self::InvalidConfiguration => LlmProfileError::InvalidConfiguration,
            Self::PolicyDenied => LlmProfileError::PolicyDenied,
            Self::Authentication => LlmProfileError::Authentication,
            Self::ModelUnavailable => LlmProfileError::ModelUnavailable,
            Self::Timeout => LlmProfileError::Timeout,
            Self::Tls => LlmProfileError::Tls,
            Self::MalformedResponse => LlmProfileError::MalformedResponse,
            Self::Cancelled => LlmProfileError::Cancelled,
            Self::ConsentRequired | Self::Transport => LlmProfileError::Transport,
        }
    }
}

/// Content-free profile test outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmProfileTestResult {
    pub profile_id: Uuid,
    pub provider: LlmPreset,
    pub locality: EndpointLocality,
    pub success: bool,
    pub category: Option<LlmTestErrorCategory>,
    pub duration_ms: u64,
    pub model_available: Option<bool>,
    pub available_models: Option<Vec<String>>,
    pub capabilities: BTreeSet<LlmApiCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedEndpoint {
    base_url: String,
    host: String,
    locality: EndpointLocality,
    /// True only for the loopback interface; private LAN endpoints are local but not loopback.
    loopback: bool,
}

#[derive(Clone)]
pub struct LlmProbeRequest {
    pub url: String,
    pub ollama_generation_url: Option<String>,
    pub model: String,
    pub api_key: Option<String>,
    pub api_key_header: &'static str,
    pub headers: BTreeMap<String, String>,
    pub timeout: Duration,
    /// How to ask the server not to emit reasoning tokens; `Plain` for everything but the local
    /// OpenAI-compatible presets (strict cloud APIs reject unknown parameters).
    pub reasoning_switch: ReasoningSwitch,
}

impl std::fmt::Debug for LlmProbeRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LlmProbeRequest")
            .field("url", &self.url)
            .field("ollama_generation_url", &self.ollama_generation_url)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .field("api_key_header", &self.api_key_header)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("timeout", &self.timeout)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProbeResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LlmModelCapabilities {
    pub vision: bool,
}

/// Effective advertised limits; absent fields mean the provider did not disclose them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LlmModelLimits {
    pub context_window: Option<u32>,
    pub maximum_answer_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmImageAttachment {
    pub media_type: String,
    pub data: Vec<u8>,
}

/// Bounded host-owned generation request using a saved profile.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmChatGeneration {
    /// Trusted system instruction.
    pub system_prompt: String,
    /// User-role evidence payload.
    pub user_prompt: String,
    /// Bounded image evidence. Only transports with verified vision support may accept it.
    pub images: Vec<LlmImageAttachment>,
    /// Maximum generated tokens, further bounded by the saved profile.
    pub maximum_tokens: u32,
    /// Explicit runtime context request, used only after summary capacity is verified.
    pub context_window: Option<u32>,
    /// Sampling temperature, further bounded by the saved profile. `None` keeps the model's own
    /// default, which the server applies when the field is absent.
    pub temperature: Option<f32>,
}

/// Result of one generation attempt that may carry a thinking-off switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatOutcome {
    Generated {
        text: String,
        /// The model produced reasoning tokens although a switch was sent.
        reasoning_seen: bool,
    },
    /// The server rejected the request because of the thinking-off parameters.
    Rejected,
}

#[async_trait]
pub trait LlmProbeTransport: Send + Sync {
    async fn discover_model_limits(
        &self,
        _request: &LlmProbeRequest,
        _cancellation: &CancellationToken,
    ) -> Result<LlmModelLimits, LlmProfileError> {
        Ok(LlmModelLimits::default())
    }

    async fn model_capabilities(
        &self,
        _request: &LlmProbeRequest,
        _cancellation: &CancellationToken,
    ) -> Result<LlmModelCapabilities, LlmProfileError> {
        Ok(LlmModelCapabilities::default())
    }

    async fn discover_models(
        &self,
        request: &LlmProbeRequest,
        cancellation: &CancellationToken,
    ) -> Result<Option<Vec<String>>, LlmProfileError>;

    async fn stream_chat(
        &self,
        request: &LlmProbeRequest,
        cancellation: &CancellationToken,
    ) -> Result<LlmProbeResponse, LlmProfileError>;

    /// Streams a bounded generation request and returns only validated content.
    async fn generate_chat(
        &self,
        request: &LlmProbeRequest,
        generation: &LlmChatGeneration,
        cancellation: &CancellationToken,
    ) -> Result<String, LlmProfileError>;

    /// Like [`Self::generate_chat`], but honours `request.reasoning_switch` and reports whether the
    /// server rejected it or the model reasoned anyway. Transports without such a switch need not
    /// override this.
    async fn generate_chat_adaptive(
        &self,
        request: &LlmProbeRequest,
        generation: &LlmChatGeneration,
        cancellation: &CancellationToken,
    ) -> Result<ChatOutcome, LlmProfileError> {
        self.generate_chat(request, generation, cancellation)
            .await
            .map(|text| ChatOutcome::Generated {
                text,
                reasoning_seen: false,
            })
    }

    /// Like [`Self::generate_chat_adaptive`], but reports each content fragment through
    /// `on_delta` as it arrives. Transports that cannot read incrementally report the whole text
    /// once.
    async fn generate_chat_streamed(
        &self,
        request: &LlmProbeRequest,
        generation: &LlmChatGeneration,
        cancellation: &CancellationToken,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<ChatOutcome, LlmProfileError> {
        let outcome = self
            .generate_chat_adaptive(request, generation, cancellation)
            .await?;
        if let ChatOutcome::Generated { text, .. } = &outcome {
            on_delta(text);
        }
        Ok(outcome)
    }
}

/// Reqwest implementation used by real hosts.
pub struct ReqwestLlmProbeTransport {
    client: reqwest::Client,
}

impl ReqwestLlmProbeTransport {
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn request(
        &self,
        method: reqwest::Method,
        request: &LlmProbeRequest,
        url: &str,
    ) -> reqwest::RequestBuilder {
        self.request_with_timeout(method, request, url, request.timeout)
    }

    /// A generation request: its total wait is capped far above the idle timeout, because a local
    /// model may take minutes to load and a reply may stream for a long time. Waits are enforced by
    /// [`send_generation`] and [`read_streamed_body`] instead.
    fn generation_request(
        &self,
        method: reqwest::Method,
        request: &LlmProbeRequest,
        url: &str,
    ) -> reqwest::RequestBuilder {
        self.request_with_timeout(method, request, url, GENERATION_MAX_WAIT)
    }

    fn request_with_timeout(
        &self,
        method: reqwest::Method,
        request: &LlmProbeRequest,
        url: &str,
        timeout: Duration,
    ) -> reqwest::RequestBuilder {
        let mut builder = self.client.request(method, url).timeout(timeout);
        if let Some(key) = &request.api_key {
            builder = builder.header(request.api_key_header, key);
        }
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        builder
    }
}

impl Default for ReqwestLlmProbeTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProbeTransport for ReqwestLlmProbeTransport {
    async fn discover_model_limits(
        &self,
        request: &LlmProbeRequest,
        cancellation: &CancellationToken,
    ) -> Result<LlmModelLimits, LlmProfileError> {
        let (url, method) = if let Some(chat_url) = &request.ollama_generation_url {
            let base = chat_url
                .strip_suffix("/api/chat")
                .ok_or(LlmProfileError::InvalidConfiguration)?;
            (format!("{base}/api/show"), reqwest::Method::POST)
        } else {
            let Some(url) = models_url(&request.url) else {
                return Ok(LlmModelLimits::default());
            };
            (url, reqwest::Method::GET)
        };
        let mut builder = self.request(method.clone(), request, &url);
        if method == reqwest::Method::POST {
            builder = builder.json(&json!({ "model": request.model }));
        }
        let response = tokio::select! {
            () = cancellation.cancelled() => return Err(LlmProfileError::Cancelled),
            response = builder.send() => response.map_err(map_reqwest)?
        };
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(LlmModelLimits::default());
        }
        classify_http_status(response.status().as_u16())?;
        let body = read_bounded_body(response, cancellation).await?;
        let value: Value =
            serde_json::from_slice(&body).map_err(|_| LlmProfileError::MalformedResponse)?;
        Ok(if method == reqwest::Method::POST {
            parse_ollama_model_limits(&value)
        } else {
            parse_openai_model_limits(&value, &request.model)
        })
    }

    async fn model_capabilities(
        &self,
        request: &LlmProbeRequest,
        cancellation: &CancellationToken,
    ) -> Result<LlmModelCapabilities, LlmProfileError> {
        let Some(chat_url) = &request.ollama_generation_url else {
            return Ok(LlmModelCapabilities::default());
        };
        let Some(base_url) = chat_url.strip_suffix("/api/chat") else {
            return Err(LlmProfileError::InvalidConfiguration);
        };
        let response = tokio::select! {
            () = cancellation.cancelled() => return Err(LlmProfileError::Cancelled),
            response = self
                .request(reqwest::Method::POST, request, &format!("{base_url}/api/show"))
                .json(&json!({ "model": request.model }))
                .send() => {
                response.map_err(map_reqwest)?
            }
        };
        classify_http_status(response.status().as_u16())?;
        let body = read_bounded_body(response, cancellation).await?;
        parse_ollama_model_capabilities(&body)
    }

    async fn discover_models(
        &self,
        request: &LlmProbeRequest,
        cancellation: &CancellationToken,
    ) -> Result<Option<Vec<String>>, LlmProfileError> {
        let Some(url) = models_url(&request.url) else {
            return Ok(None);
        };
        let response = tokio::select! {
            () = cancellation.cancelled() => return Err(LlmProfileError::Cancelled),
            response = self.request(reqwest::Method::GET, request, &url).send() => {
                response.map_err(map_reqwest)?
            }
        };
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        classify_http_status(response.status().as_u16())?;
        let body = read_bounded_body(response, cancellation).await?;
        let value: Value =
            serde_json::from_slice(&body).map_err(|_| LlmProfileError::MalformedResponse)?;
        let models = value
            .get("data")
            .and_then(Value::as_array)
            .ok_or(LlmProfileError::MalformedResponse)?
            .iter()
            .filter_map(|model| model.get("id").and_then(Value::as_str).map(str::to_owned))
            .collect();
        Ok(Some(models))
    }

    async fn stream_chat(
        &self,
        request: &LlmProbeRequest,
        cancellation: &CancellationToken,
    ) -> Result<LlmProbeResponse, LlmProfileError> {
        let body = json!({
            "model": request.model,
            "messages": [{"role": "user", "content": TEST_PROMPT}],
            "stream": true,
            "max_tokens": 1,
            "temperature": 0,
        });
        let response = tokio::select! {
            () = cancellation.cancelled() => return Err(LlmProfileError::Cancelled),
            response = self.request(reqwest::Method::POST, request, &request.url).json(&body).send() => {
                response.map_err(map_reqwest)?
            }
        };
        let status = response.status().as_u16();
        classify_http_status(status)?;
        let bytes = read_bounded_body(response, cancellation).await?;
        Ok(LlmProbeResponse {
            status,
            body: bytes,
        })
    }

    async fn generate_chat(
        &self,
        request: &LlmProbeRequest,
        generation: &LlmChatGeneration,
        cancellation: &CancellationToken,
    ) -> Result<String, LlmProfileError> {
        if let Some(url) = &request.ollama_generation_url {
            let body = ollama_generation_body(request, generation);
            let response = send_generation(
                self.generation_request(reqwest::Method::POST, request, url)
                    .json(&body),
                request,
                cancellation,
            )
            .await?;
            classify_http_status(response.status().as_u16())?;
            let bytes = read_bounded_body(response, cancellation).await?;
            return parse_ollama_chat(&bytes);
        }
        match self
            .generate_chat_adaptive(request, generation, cancellation)
            .await?
        {
            ChatOutcome::Generated { text, .. } => Ok(text),
            ChatOutcome::Rejected => Err(LlmProfileError::Transport),
        }
    }

    async fn generate_chat_adaptive(
        &self,
        request: &LlmProbeRequest,
        generation: &LlmChatGeneration,
        cancellation: &CancellationToken,
    ) -> Result<ChatOutcome, LlmProfileError> {
        self.generate_chat_streamed(request, generation, cancellation, &mut |_| {})
            .await
    }

    async fn generate_chat_streamed(
        &self,
        request: &LlmProbeRequest,
        generation: &LlmChatGeneration,
        cancellation: &CancellationToken,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<ChatOutcome, LlmProfileError> {
        if request.ollama_generation_url.is_some() {
            let text = self
                .generate_chat(request, generation, cancellation)
                .await?;
            on_delta(&text);
            return Ok(ChatOutcome::Generated {
                text,
                reasoning_seen: false,
            });
        }
        let body = openai_generation_body(request, generation, request.reasoning_switch);
        let response = send_generation(
            self.generation_request(reqwest::Method::POST, request, &request.url)
                .json(&body),
            request,
            cancellation,
        )
        .await?;
        let status = response.status().as_u16();
        if request.reasoning_switch != ReasoningSwitch::Plain && matches!(status, 400 | 422) {
            return Ok(ChatOutcome::Rejected);
        }
        classify_http_status(status)?;
        let bytes = read_streamed_body(response, cancellation, request.timeout, on_delta).await?;
        let (text, reasoning_seen) = parse_streaming_chat_detailed(&bytes)?;
        Ok(ChatOutcome::Generated {
            text,
            reasoning_seen,
        })
    }
}

async fn read_bounded_body(
    response: reqwest::Response,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, LlmProfileError> {
    read_streamed_body(response, cancellation, GENERATION_MAX_WAIT, &mut |_| {}).await
}

/// Longest total wait for one generation request; the first reply and each streamed chunk have
/// their own, shorter limits.
const GENERATION_MAX_WAIT: Duration = Duration::from_secs(2 * 60 * 60);
/// Least time to wait for the first byte of a reply, so a model that is still loading gets time.
const FIRST_REPLY_MIN_WAIT: Duration = Duration::from_secs(300);

/// Sends a generation request. The first reply may take as long as a model load needs; after that
/// the stream is limited by its idle timeout, not by its total length.
async fn send_generation(
    builder: reqwest::RequestBuilder,
    request: &LlmProbeRequest,
    cancellation: &CancellationToken,
) -> Result<reqwest::Response, LlmProfileError> {
    let first_reply = request.timeout.max(FIRST_REPLY_MIN_WAIT);
    tokio::select! {
        () = cancellation.cancelled() => Err(LlmProfileError::Cancelled),
        sent = tokio::time::timeout(first_reply, builder.send()) => match sent {
            Ok(response) => response.map_err(map_reqwest),
            Err(_) => Err(LlmProfileError::Timeout),
        },
    }
}

/// Reads a bounded response body and reports the content fragments of its `data:` events as they
/// complete. The whole body is still returned for the strict final parse.
async fn read_streamed_body(
    response: reqwest::Response,
    cancellation: &CancellationToken,
    idle: Duration,
    on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
) -> Result<Vec<u8>, LlmProfileError> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    let mut events = StreamedContent {
        pending: Vec::new(),
        on_delta,
    };
    loop {
        // A chunk must arrive within the idle time; a stream that keeps arriving may run on.
        let next = tokio::select! {
            () = cancellation.cancelled() => return Err(LlmProfileError::Cancelled),
            next = tokio::time::timeout(idle, stream.next()) => {
                next.map_err(|_| LlmProfileError::Timeout)?
            }
        };
        let Some(chunk) = next else {
            // A final line without its newline still counts.
            events.push(b"\n");
            return Ok(bytes);
        };
        let chunk = chunk.map_err(map_reqwest)?;
        if bytes.len().saturating_add(chunk.len()) > MAX_STREAM_BYTES {
            return Err(LlmProfileError::MalformedResponse);
        }
        events.push(&chunk);
        bytes.extend_from_slice(&chunk);
    }
}

/// Splits streamed SSE bytes into lines and reports each `choices[].delta.content` fragment.
/// Lines that do not parse are skipped here; the strict parse of the whole body decides validity.
struct StreamedContent<'a> {
    pending: Vec<u8>,
    on_delta: &'a mut (dyn for<'b> FnMut(&'b str) + Send),
}

impl StreamedContent<'_> {
    fn push(&mut self, chunk: &[u8]) {
        self.pending.extend_from_slice(chunk);
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            if let Ok(line) = std::str::from_utf8(&line) {
                self.report(line);
            }
        }
    }

    fn report(&mut self, line: &str) {
        let Some(data) = line.trim().strip_prefix("data:") else {
            return;
        };
        let Ok(value) = serde_json::from_str::<Value>(data.trim()) else {
            return;
        };
        let fragments = value
            .get("choices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|choice| choice.get("delta")?.get("content")?.as_str());
        for fragment in fragments.filter(|fragment| !fragment.is_empty()) {
            (self.on_delta)(fragment);
        }
    }
}

/// A feature that needs a model. Only the roles with a consumer today are listed; natural-language
/// search is reserved in the settings model (task 0322).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LlmRole {
    Chat,
    Embeddings,
    /// Text-to-image generation (task 0325). Deliberately separate from `Chat`: a different
    /// endpoint class, a different request shape, and its own cloud permission.
    Image,
}

/// One role's binding: an endpoint profile, the model to use on it, and whether a cloud endpoint
/// may serve the role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssignment {
    pub profile_id: Uuid,
    pub model: String,
    #[serde(default)]
    pub allow_cloud: bool,
}

/// The role bindings that are configured globally.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmRoles {
    #[serde(default)]
    pub chat: Option<RoleAssignment>,
    #[serde(default)]
    pub embeddings: Option<RoleAssignment>,
    /// The text-to-image endpoint (task 0325). Absent until the person assigns one.
    #[serde(default)]
    pub image: Option<RoleAssignment>,
}

/// A role binding requested by the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleAssignmentInput {
    pub profile_id: Uuid,
    pub model: String,
    pub allow_cloud: bool,
}

/// A role binding that passed endpoint, consent and cloud-permission checks.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmResolvedRole {
    pub profile: LlmProfile,
    pub model: String,
}

/// Durable profile collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LlmProfilesDocument {
    schema_version: u32,
    #[serde(default)]
    active_profile_id: Option<Uuid>,
    profiles: Vec<LlmProfile>,
    #[serde(default)]
    roles: LlmRoles,
}

impl Default for LlmProfilesDocument {
    fn default() -> Self {
        Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            active_profile_id: None,
            profiles: Vec::new(),
            roles: LlmRoles::default(),
        }
    }
}

/// Schema 4 had one active profile; it becomes both the chat and the embeddings role (task
/// 0322, scenario A). Embeddings stay local-only until the person enables the cloud flag.
fn legacy_roles(value: &Value) -> Value {
    let Some(active) = value
        .get("activeProfileId")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return json!({});
    };
    let model = value
        .get("profiles")
        .and_then(Value::as_array)
        .and_then(|profiles| {
            profiles
                .iter()
                .find(|profile| profile.get("id").and_then(Value::as_str) == Some(active.as_str()))
        })
        .and_then(|profile| profile.get("model"))
        .cloned()
        .unwrap_or(Value::Null);
    let assignment = json!({ "profileId": active, "model": model, "allowCloud": false });
    json!({ "chat": assignment.clone(), "embeddings": assignment })
}

impl LlmProfilesDocument {
    /// Drops role bindings whose profile no longer exists, for example after an external edit.
    fn prune_dangling_roles(&mut self) {
        let ids: BTreeSet<Uuid> = self.profiles.iter().map(|profile| profile.id).collect();
        for role in [
            &mut self.roles.chat,
            &mut self.roles.embeddings,
            &mut self.roles.image,
        ] {
            if role
                .as_ref()
                .is_some_and(|binding| !ids.contains(&binding.profile_id))
            {
                *role = None;
            }
        }
    }
}

impl VersionedDocument for LlmProfilesDocument {
    type MigrationError = LlmProfileError;

    const FILE_NAME: &'static str = "llm-profiles.json";
    const CURRENT_SCHEMA_VERSION: u32 = PROFILE_SCHEMA_VERSION;

    fn migrate(mut value: Value, version: u32) -> Result<Value, Self::MigrationError> {
        match version {
            1 => {
                value["activeProfileId"] = Value::Null;
                if let Some(profiles) = value.get_mut("profiles").and_then(Value::as_array_mut) {
                    for profile in profiles {
                        profile["capabilities"] = json!(["chatCompletions", "modelDiscovery"]);
                        profile["redactFilenames"] = Value::Bool(false);
                    }
                }
            }
            2 => {
                value["activeProfileId"] = Value::Null;
            }
            3 | 4 => {}
            PROFILE_SCHEMA_VERSION => return Ok(value),
            _ => return Err(LlmProfileError::InvalidConfiguration),
        }
        value["schemaVersion"] = Value::from(PROFILE_SCHEMA_VERSION);
        if let Some(profiles) = value.get_mut("profiles").and_then(Value::as_array_mut) {
            for profile in profiles {
                let consented = profile.get("consentedHost").and_then(Value::as_str);
                let endpoint = profile
                    .get("baseUrl")
                    .and_then(Value::as_str)
                    .map(normalize_endpoint)
                    .transpose()?;
                let exact = endpoint.and_then(|endpoint| {
                    (consented == Some(endpoint.host.as_str())
                        || consented == Some(endpoint.base_url.as_str()))
                    .then_some(endpoint.base_url)
                });
                profile["consentedHost"] = exact.map_or(Value::Null, Value::String);
            }
        }
        if version <= 4 {
            value["roles"] = legacy_roles(&value);
        }
        Ok(value)
    }

    fn validate(&self) -> Result<(), Self::MigrationError> {
        if self.schema_version != PROFILE_SCHEMA_VERSION {
            return Err(LlmProfileError::InvalidConfiguration);
        }
        let mut ids = BTreeSet::new();
        for profile in &self.profiles {
            validate_profile(profile)?;
            if !ids.insert(profile.id) {
                return Err(LlmProfileError::InvalidConfiguration);
            }
        }
        if self
            .active_profile_id
            .is_some_and(|active| !ids.contains(&active))
        {
            return Err(LlmProfileError::InvalidConfiguration);
        }
        for assignment in [&self.roles.chat, &self.roles.embeddings, &self.roles.image]
            .into_iter()
            .flatten()
        {
            // A dangling profile reference is pruned on load; only a blank model is invalid.
            if assignment.model.trim().is_empty() {
                return Err(LlmProfileError::InvalidConfiguration);
            }
        }
        Ok(())
    }
}

/// Thread-safe profile lifecycle and test capability.
pub struct LlmProfileService {
    settings: SettingsStore,
    profiles: Mutex<LlmProfilesDocument>,
    credentials: Arc<dyn CredentialStore>,
    transport: Arc<dyn LlmProbeTransport>,
    host_policy: LlmHostPolicy,
}

impl LlmProfileService {
    /// Opens a durable profile collection.
    pub fn new(
        settings: SettingsStore,
        credentials: Arc<dyn CredentialStore>,
        transport: Arc<dyn LlmProbeTransport>,
        host_policy: LlmHostPolicy,
    ) -> Result<Self, LlmProfileError> {
        let mut profiles = settings
            .load_document::<LlmProfilesDocument>()
            .map_err(|_| LlmProfileError::Persistence)?
            .unwrap_or_default();
        profiles.prune_dangling_roles();
        Ok(Self {
            settings,
            profiles: Mutex::new(profiles),
            credentials,
            transport,
            host_policy,
        })
    }

    /// Creates an empty collection after the caller has surfaced a load
    /// failure. This keeps the infallible application composition root usable
    /// without pretending the persisted document loaded successfully.
    #[must_use]
    pub fn empty(
        settings: SettingsStore,
        credentials: Arc<dyn CredentialStore>,
        transport: Arc<dyn LlmProbeTransport>,
        host_policy: LlmHostPolicy,
    ) -> Self {
        Self {
            settings,
            profiles: Mutex::new(LlmProfilesDocument::default()),
            credentials,
            transport,
            host_policy,
        }
    }

    #[must_use]
    pub fn presets() -> Vec<LlmProfileDraft> {
        [
            (LlmPreset::Ollama, "Ollama", "http://127.0.0.1:11434"),
            (LlmPreset::LmStudio, "LM Studio", "http://127.0.0.1:1234"),
            (LlmPreset::Vllm, "vLLM", "http://127.0.0.1:8000"),
            (LlmPreset::Sglang, "SGLang", "http://127.0.0.1:30000"),
            (LlmPreset::Omlx, "OMLX", "http://127.0.0.1:8000"),
            (LlmPreset::MlxServe, "mlx-serve", "http://127.0.0.1:11234"),
            (
                LlmPreset::Unsloth,
                "Unsloth Studio",
                "http://127.0.0.1:8888",
            ),
            (
                LlmPreset::OpenAiCompatible,
                "OpenAI-compatible",
                "https://api.openai.com",
            ),
            (
                LlmPreset::AzureOpenAi,
                "Azure OpenAI",
                "https://example.openai.azure.com",
            ),
            (
                LlmPreset::ModelStudio,
                "Alibaba Cloud Model Studio",
                "https://dashscope.aliyuncs.com/api/v1",
            ),
            (
                LlmPreset::QwenImage,
                "Qwen Image (Alibaba Cloud)",
                "https://dashscope.aliyuncs.com/api/v1",
            ),
        ]
        .into_iter()
        .map(|(preset, name, base_url)| {
            let mut draft = preset_draft(preset, name, base_url);
            if matches!(preset, LlmPreset::ModelStudio | LlmPreset::QwenImage) {
                // Reached over the internet, and it answers the image task API rather than chat.
                draft.capabilities =
                    std::collections::BTreeSet::from([LlmApiCapability::DashscopeTasks]);
                draft.redact_filenames = true;
            }
            if preset == LlmPreset::QwenImage {
                // Named so the image role can be filled without looking the model id up.
                draft.model = "qwen-image".to_owned();
            }
            draft
        })
        .collect()
    }

    /// The configured role bindings, unchanged by reads.
    pub fn roles(&self) -> Result<LlmRoles, LlmProfileError> {
        Ok(self.lock()?.roles.clone())
    }

    pub fn list(&self) -> Result<Vec<LlmProfile>, LlmProfileError> {
        let document = self.lock()?;
        let mut profiles = document.profiles.clone();
        if let Some(active) = document.active_profile_id
            && let Some(index) = profiles.iter().position(|profile| profile.id == active)
        {
            profiles.rotate_left(index);
        }
        Ok(profiles)
    }

    pub async fn create(&self, mut draft: LlmProfileDraft) -> Result<LlmProfile, LlmProfileError> {
        let api_key = draft.api_key.take();
        let mut profile = profile_from_draft(Uuid::new_v4(), draft, None);
        validate_profile(&profile)?;
        let credential_ref = self.store_api_key(&profile.name, api_key).await?;
        profile.credential_ref = credential_ref;
        profile.consented_host = None;
        let persist_error = {
            let mut document = self.lock()?;
            let previous_active = document.active_profile_id;
            document.profiles.push(profile.clone());
            if document.active_profile_id.is_none() {
                document.active_profile_id = Some(profile.id);
            }
            match self.persist(&document) {
                Ok(()) => None,
                Err(error) => {
                    document.profiles.pop();
                    document.active_profile_id = previous_active;
                    Some(error)
                }
            }
        };
        if let Some(error) = persist_error {
            if let Some(reference) = credential_ref {
                let _ = self.credentials.delete(&reference).await;
            }
            return Err(error);
        }
        Ok(profile)
    }

    pub async fn update(
        &self,
        id: Uuid,
        mut draft: LlmProfileDraft,
    ) -> Result<LlmProfile, LlmProfileError> {
        let old = self
            .lock()?
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
            .ok_or(LlmProfileError::NotFound)?;
        let api_key = draft.api_key.take();
        let mut profile = profile_from_draft(id, draft, old.credential_ref);
        let old_endpoint = normalize_endpoint(&old.base_url)?.base_url;
        let new_endpoint = normalize_endpoint(&profile.base_url)?.base_url;
        profile.consented_host = (old_endpoint == new_endpoint)
            .then_some(old.consented_host)
            .flatten();
        if old_endpoint == new_endpoint
            && old.preset == profile.preset
            && old.model == profile.model
        {
            profile.reasoning_switch = old.reasoning_switch;
        }
        validate_profile(&profile)?;
        let replacement_ref = match api_key {
            Some(api_key) => Some(
                self.store_api_key(&profile.name, Some(api_key))
                    .await?
                    .ok_or(LlmProfileError::Credential)?,
            ),
            None => old.credential_ref,
        };
        profile.credential_ref = replacement_ref;
        let persist_error = {
            let mut document = self.lock()?;
            let stored = document
                .profiles
                .iter_mut()
                .find(|candidate| candidate.id == id)
                .ok_or(LlmProfileError::NotFound)?;
            let previous = stored.clone();
            *stored = profile.clone();
            match self.persist(&document) {
                Ok(()) => None,
                Err(error) => {
                    *document
                        .profiles
                        .iter_mut()
                        .find(|candidate| candidate.id == id)
                        .ok_or(LlmProfileError::NotFound)? = previous;
                    Some(error)
                }
            }
        };
        if let Some(error) = persist_error {
            if replacement_ref != old.credential_ref
                && let Some(reference) = replacement_ref
            {
                let _ = self.credentials.delete(&reference).await;
            }
            return Err(error);
        }
        if old.credential_ref != profile.credential_ref
            && let Some(reference) = old.credential_ref
        {
            let _ = self.credentials.delete(&reference).await;
        }
        Ok(profile)
    }

    pub async fn delete(
        &self,
        id: Uuid,
        disposition: OrphanCredentialDisposition,
    ) -> Result<(), LlmProfileError> {
        let profile = {
            let mut document = self.lock()?;
            let index = document
                .profiles
                .iter()
                .position(|profile| profile.id == id)
                .ok_or(LlmProfileError::NotFound)?;
            let profile = document.profiles.remove(index);
            let previous_active = document.active_profile_id;
            let previous_roles = document.roles.clone();
            if document.active_profile_id == Some(id) {
                document.active_profile_id = document.profiles.first().map(|profile| profile.id);
            }
            let roles = &mut document.roles;
            for role in [&mut roles.chat, &mut roles.embeddings, &mut roles.image] {
                if role
                    .as_ref()
                    .is_some_and(|binding| binding.profile_id == id)
                {
                    *role = None;
                }
            }
            if let Err(error) = self.persist(&document) {
                document.profiles.insert(index, profile);
                document.active_profile_id = previous_active;
                document.roles = previous_roles;
                return Err(error);
            }
            profile
        };
        if disposition == OrphanCredentialDisposition::Delete
            && let Some(reference) = profile.credential_ref
        {
            match self.credentials.delete(&reference).await {
                Ok(()) | Err(fm_credentials::CredentialError::NotFound { .. }) => {}
                Err(_) => return Err(LlmProfileError::Credential),
            }
        }
        Ok(())
    }

    pub fn clone_profile(&self, id: Uuid) -> Result<LlmProfile, LlmProfileError> {
        let mut profile = self.profile(id)?;
        profile.id = Uuid::new_v4();
        profile.name = format!("{} copy", profile.name);
        profile.credential_ref = None;
        profile.consented_host = None;
        let mut document = self.lock()?;
        document.profiles.push(profile.clone());
        if let Err(error) = self.persist(&document) {
            document.profiles.pop();
            return Err(error);
        }
        Ok(profile)
    }

    pub fn export_profile(&self, id: Uuid) -> Result<LlmProfile, LlmProfileError> {
        let mut profile = self.profile(id)?;
        profile.credential_ref = None;
        profile.consented_host = None;
        Ok(profile)
    }

    pub fn activate(&self, id: Uuid, consent: bool) -> Result<LlmProfile, LlmProfileError> {
        let mut document = self.lock()?;
        let profile = {
            let profile = document
                .profiles
                .iter_mut()
                .find(|profile| profile.id == id)
                .ok_or(LlmProfileError::NotFound)?;
            let endpoint = normalize_endpoint(&profile.base_url)?;
            self.enforce_policy(&endpoint, profile.advanced.tls_policy)?;
            if endpoint.locality == EndpointLocality::Cloud
                && profile.consented_host.as_deref() != Some(endpoint.base_url.as_str())
            {
                if !consent {
                    return Err(LlmProfileError::ConsentRequired(endpoint.host));
                }
                profile.consented_host = Some(endpoint.base_url);
            }
            profile.clone()
        };
        document.active_profile_id = Some(id);
        self.persist(&document)?;
        Ok(profile)
    }

    /// Records the person's consent for the endpoint's exact host. Local endpoints need none.
    pub fn grant_endpoint_consent(&self, id: Uuid) -> Result<LlmProfile, LlmProfileError> {
        let mut document = self.lock()?;
        let profile = document
            .profiles
            .iter_mut()
            .find(|profile| profile.id == id)
            .ok_or(LlmProfileError::NotFound)?;
        let endpoint = normalize_endpoint(&profile.base_url)?;
        if endpoint.locality == EndpointLocality::Cloud {
            profile.consented_host = Some(endpoint.base_url);
        }
        let saved = profile.clone();
        self.persist(&document)?;
        Ok(saved)
    }

    /// Binds a role to an endpoint and model, or clears it. A cloud endpoint is refused for the
    /// embeddings and image roles until the person allows cloud use for that role; chat needs only
    /// the endpoint consent recorded in the endpoint editor.
    pub fn set_role(
        &self,
        role: LlmRole,
        assignment: Option<RoleAssignmentInput>,
    ) -> Result<(), LlmProfileError> {
        let mut document = self.lock()?;
        let next = match assignment {
            None => None,
            Some(input) => {
                if input.model.trim().is_empty() {
                    return Err(LlmProfileError::InvalidConfiguration);
                }
                let profile = document
                    .profiles
                    .iter()
                    .find(|profile| profile.id == input.profile_id)
                    .ok_or(LlmProfileError::NotFound)?;
                let endpoint = normalize_endpoint(&profile.base_url)?;
                if role != LlmRole::Chat
                    && endpoint.locality == EndpointLocality::Cloud
                    && !input.allow_cloud
                {
                    return Err(cloud_role_denied(role));
                }
                if role == LlmRole::Chat
                    && !profile
                        .capabilities
                        .contains(&LlmApiCapability::ChatCompletions)
                {
                    // Binding an image-only endpoint to the chat role would only fail later, at the
                    // provider, so it is refused where the person can still act on it.
                    return Err(LlmProfileError::InvalidConfiguration);
                }
                Some(RoleAssignment {
                    profile_id: input.profile_id,
                    model: input.model,
                    allow_cloud: input.allow_cloud,
                })
            }
        };
        let previous = document.roles.clone();
        match role {
            LlmRole::Chat => document.roles.chat = next,
            LlmRole::Embeddings => document.roles.embeddings = next,
            LlmRole::Image => document.roles.image = next,
        }
        if let Err(error) = self.persist(&document) {
            document.roles = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Returns the endpoint and model for a role if policy, consent and cloud permission allow
    /// it. Never prompts; a missing consent is reported so the caller can direct the person to
    /// Settings.
    pub fn resolve_role(&self, role: LlmRole) -> Result<LlmResolvedRole, LlmProfileError> {
        let document = self.lock()?;
        let assignment = match role {
            LlmRole::Chat => document.roles.chat.as_ref(),
            LlmRole::Embeddings => document.roles.embeddings.as_ref(),
            LlmRole::Image => document.roles.image.as_ref(),
        }
        .ok_or(LlmProfileError::RoleNotAssigned)?;
        let profile = document
            .profiles
            .iter()
            .find(|profile| profile.id == assignment.profile_id)
            .ok_or(LlmProfileError::NotFound)?;
        if role == LlmRole::Chat
            && !profile
                .capabilities
                .contains(&LlmApiCapability::ChatCompletions)
        {
            // An image-only endpoint bound to the chat role would fail at the provider; say so here.
            return Err(LlmProfileError::InvalidConfiguration);
        }
        let endpoint = normalize_endpoint(&profile.base_url)?;
        self.enforce_policy(&endpoint, profile.advanced.tls_policy)?;
        if endpoint.locality == EndpointLocality::Cloud {
            if role != LlmRole::Chat && !assignment.allow_cloud {
                return Err(cloud_role_denied(role));
            }
            if profile.consented_host.as_deref() != Some(endpoint.base_url.as_str()) {
                return Err(LlmProfileError::ConsentRequired(endpoint.host));
            }
        }
        Ok(LlmResolvedRole {
            profile: profile.clone(),
            model: assignment.model.clone(),
        })
    }

    pub async fn test(
        &self,
        id: Uuid,
        cancellation: &CancellationToken,
    ) -> Result<LlmProfileTestResult, LlmProfileError> {
        let profile = self.profile(id)?;
        let endpoint = normalize_endpoint(&profile.base_url)?;
        let started = Instant::now();
        let mut available_models = None;
        let outcome: Result<(Option<LlmTestErrorCategory>, Option<bool>), LlmProfileError> =
            async {
                self.enforce_policy(&endpoint, profile.advanced.tls_policy)?;
                let request = self.probe_request(&profile).await?;
                let discovered_models = if profile
                    .capabilities
                    .contains(&LlmApiCapability::ModelDiscovery)
                    && profile.preset != LlmPreset::AzureOpenAi
                {
                    self.transport
                        .discover_models(&request, cancellation)
                        .await?
                } else {
                    None
                };
                let model_available = discovered_models
                    .as_ref()
                    .map(|models| models.iter().any(|model| model == &profile.model));
                available_models = discovered_models.map(bounded_model_ids);
                if model_available == Some(false) {
                    return Ok((
                        Some(LlmTestErrorCategory::ModelUnavailable),
                        model_available,
                    ));
                }
                let response = self.transport.stream_chat(&request, cancellation).await?;
                validate_streaming_chat(&response.body)?;
                Ok((None, model_available))
            }
            .await;
        let (category, model_available) = match outcome {
            Ok(result) => result,
            Err(error) => (Some(error.category()), None),
        };
        let result = test_result(
            &profile,
            endpoint.locality,
            started,
            category,
            model_available,
            available_models,
        );
        tracing::info!(
            profile_id = %result.profile_id,
            provider = ?result.provider,
            category = ?result.category,
            duration_ms = result.duration_ms,
            success = result.success,
            "LLM profile test completed"
        );
        Ok(result)
    }

    /// Discovers bounded provider model identifiers without sending a chat prompt.
    pub async fn discover_models(
        &self,
        id: Uuid,
        cancellation: &CancellationToken,
    ) -> Result<Vec<String>, LlmProfileError> {
        let profile = self.profile(id)?;
        if !profile
            .capabilities
            .contains(&LlmApiCapability::ModelDiscovery)
            || profile.preset == LlmPreset::AzureOpenAi
        {
            return Err(LlmProfileError::InvalidConfiguration);
        }
        let endpoint = normalize_endpoint(&profile.base_url)?;
        self.enforce_policy(&endpoint, profile.advanced.tls_policy)?;
        let request = self.probe_request(&profile).await?;
        Ok(bounded_model_ids(
            self.transport
                .discover_models(&request, cancellation)
                .await?
                .unwrap_or_default(),
        ))
    }

    /// Discovers provider models from an unsaved draft without persisting a
    /// placeholder profile or credential.
    pub async fn discover_draft_models(
        &self,
        mut draft: LlmProfileDraft,
        cancellation: &CancellationToken,
    ) -> Result<Vec<String>, LlmProfileError> {
        if !draft
            .capabilities
            .contains(&LlmApiCapability::ModelDiscovery)
            || draft.preset == LlmPreset::AzureOpenAi
        {
            return Err(LlmProfileError::InvalidConfiguration);
        }
        let api_key = draft.api_key.take();
        let mut profile = profile_from_draft(Uuid::new_v4(), draft, None);
        profile.model = "__model_discovery__".to_owned();
        validate_profile(&profile)?;
        let endpoint = normalize_endpoint(&profile.base_url)?;
        self.enforce_policy(&endpoint, profile.advanced.tls_policy)?;
        let mut request = self.probe_request(&profile).await?;
        request.api_key = api_key.map(|key| {
            if profile.preset == LlmPreset::AzureOpenAi {
                key
            } else {
                format!("Bearer {key}")
            }
        });
        Ok(bounded_model_ids(
            self.transport
                .discover_models(&request, cancellation)
                .await?
                .unwrap_or_default(),
        ))
    }

    /// Discovers limits for the selected model in an unsaved draft, without
    /// persisting values or sending prompts.
    pub async fn discover_draft_model_limits(
        &self,
        mut draft: LlmProfileDraft,
        cancellation: &CancellationToken,
    ) -> Result<LlmModelLimits, LlmProfileError> {
        let api_key = draft.api_key.take();
        let profile = profile_from_draft(Uuid::new_v4(), draft, None);
        validate_profile(&profile)?;
        if !profile
            .capabilities
            .contains(&LlmApiCapability::ModelDiscovery)
        {
            return Err(LlmProfileError::InvalidConfiguration);
        }
        let endpoint = normalize_endpoint(&profile.base_url)?;
        self.enforce_policy(&endpoint, profile.advanced.tls_policy)?;
        if profile.preset == LlmPreset::AzureOpenAi {
            return Ok(LlmModelLimits::default());
        }
        let mut request = self.probe_request(&profile).await?;
        request.api_key = api_key.map(|key| ["Bearer ", key.as_str()].concat());
        self.transport
            .discover_model_limits(&request, cancellation)
            .await
            .map(cap_model_limits)
    }

    /// Generates bounded text through a saved profile after policy and consent checks.
    pub async fn generate(
        &self,
        id: Uuid,
        generation: LlmChatGeneration,
        cancellation: &CancellationToken,
    ) -> Result<String, LlmProfileError> {
        self.generate_streamed(id, generation, cancellation, &mut |_| {})
            .await
    }

    /// Like [`Self::generate`], but reports each fragment of the answer through `on_delta` as it
    /// arrives. Fragments are raw model output; reasoning is removed only from the final text.
    pub async fn generate_streamed(
        &self,
        id: Uuid,
        generation: LlmChatGeneration,
        cancellation: &CancellationToken,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, LlmProfileError> {
        let profile = self.profile(id)?;
        self.generate_profile_streamed(profile, generation, cancellation, on_delta)
            .await
    }

    /// Like [`Self::generate_streamed`], but the endpoint and model come from a role assignment
    /// (task 0322). Never prompts: a cloud endpoint without consent is reported as
    /// [`LlmProfileError::ConsentRequired`].
    pub async fn generate_role_streamed(
        &self,
        role: LlmRole,
        generation: LlmChatGeneration,
        cancellation: &CancellationToken,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, LlmProfileError> {
        let resolved = self.resolve_role(role)?;
        let profile = LlmProfile {
            model: resolved.model,
            ..resolved.profile
        };
        self.generate_profile_streamed(profile, generation, cancellation, on_delta)
            .await
    }

    async fn generate_profile_streamed(
        &self,
        profile: LlmProfile,
        generation: LlmChatGeneration,
        cancellation: &CancellationToken,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, LlmProfileError> {
        let id = profile.id;
        let endpoint = normalize_endpoint(&profile.base_url)?;
        self.enforce_policy(&endpoint, profile.advanced.tls_policy)?;
        if endpoint.locality == EndpointLocality::Cloud
            && profile.consented_host.as_deref() != Some(endpoint.base_url.as_str())
        {
            return Err(LlmProfileError::ConsentRequired(endpoint.host));
        }
        if generation.system_prompt.is_empty()
            || generation.user_prompt.is_empty()
            || generation.system_prompt.len() > 64 * 1024
            || generation.user_prompt.len() > 1024 * 1024
        {
            return Err(LlmProfileError::InvalidConfiguration);
        }
        let request = self.probe_request(&profile).await?;
        if !generation.images.is_empty()
            && request.ollama_generation_url.is_none()
            && !profile.capabilities.contains(&LlmApiCapability::Vision)
        {
            return Err(LlmProfileError::InvalidConfiguration);
        }
        let bounded = LlmChatGeneration {
            maximum_tokens: generation
                .maximum_tokens
                .min(profile.advanced.maximum_answer_tokens)
                .min(MAX_GENERATION_ANSWER_TOKENS),
            temperature: generation
                .temperature
                .map(|value| value.clamp(0.0, profile.advanced.temperature)),
            ..generation
        };
        let capable = supports_reasoning_switch(profile.preset);
        let mut candidates = Vec::new();
        if capable {
            // The variant that worked last time goes first; the rest only matter if the server
            // changed its mind.
            candidates.extend(profile.reasoning_switch);
            candidates.extend(
                ReasoningSwitch::LADDER
                    .into_iter()
                    .filter(|switch| Some(*switch) != profile.reasoning_switch),
            );
        } else {
            candidates.push(ReasoningSwitch::Plain);
        }
        let emitted = AtomicBool::new(false);
        let mut report = |fragment: &str| {
            emitted.store(true, Ordering::Relaxed);
            on_delta(fragment);
        };
        for switch in candidates {
            let attempt = LlmProbeRequest {
                reasoning_switch: switch,
                ..request.clone()
            };
            let outcome = match self
                .transport
                .generate_chat_streamed(&attempt, &bounded, cancellation, &mut report)
                .await
            {
                // A model that is not loaded yet (local servers load it on first use, which can take
                // longer than the request timeout) is usually ready by now; one more attempt spares
                // the user from pressing the button twice. Only before any fragment was shown.
                Err(LlmProfileError::Timeout)
                    if !cancellation.is_cancelled() && !emitted.load(Ordering::Relaxed) =>
                {
                    self.transport
                        .generate_chat_streamed(&attempt, &bounded, cancellation, &mut report)
                        .await?
                }
                other => other?,
            };
            let ChatOutcome::Generated {
                text,
                reasoning_seen,
            } = outcome
            else {
                continue;
            };
            if capable {
                let learned = if reasoning_seen {
                    switch.after_ineffective()
                } else {
                    switch
                };
                self.remember_reasoning_switch(id, learned);
            }
            let answer = strip_reasoning(&text);
            return if answer.is_empty() {
                Err(LlmProfileError::MalformedResponse)
            } else {
                Ok(answer)
            };
        }
        Err(LlmProfileError::Transport)
    }

    /// Stores the thinking-off variant a server accepted so the next generation starts with it.
    /// Best effort: a failed save only costs a retry next time.
    fn remember_reasoning_switch(&self, id: Uuid, switch: ReasoningSwitch) {
        let Ok(mut document) = self.lock() else {
            return;
        };
        let Some(stored) = document
            .profiles
            .iter_mut()
            .find(|profile| profile.id == id)
        else {
            return;
        };
        if stored.reasoning_switch == Some(switch) {
            return;
        }
        stored.reasoning_switch = Some(switch);
        let _ = self.persist(&document);
    }

    /// Whether the role's resolved model reads images. Ollama reports vision per model, so it is
    /// asked; other endpoints disclose nothing reliable, so the person's `Vision` switch decides.
    pub(crate) async fn role_accepts_images(&self, role: LlmRole) -> Result<bool, LlmProfileError> {
        let resolved = self.resolve_role(role)?;
        let profile = LlmProfile {
            model: resolved.model,
            ..resolved.profile
        };
        let endpoint = normalize_endpoint(&profile.base_url)?;
        self.enforce_policy(&endpoint, profile.advanced.tls_policy)?;
        let request = self.probe_request(&profile).await?;
        if request.ollama_generation_url.is_some() {
            let cancellation = CancellationToken::new();
            return Ok(self
                .transport
                .model_capabilities(&request, &cancellation)
                .await?
                .vision);
        }
        Ok(profile.capabilities.contains(&LlmApiCapability::Vision))
    }

    pub(crate) async fn model_capabilities(
        &self,
        id: Uuid,
        cancellation: &CancellationToken,
    ) -> Result<LlmModelCapabilities, LlmProfileError> {
        let profile = self.profile(id)?;
        let endpoint = normalize_endpoint(&profile.base_url)?;
        self.enforce_policy(&endpoint, profile.advanced.tls_policy)?;
        let request = self.probe_request(&profile).await?;
        self.transport
            .model_capabilities(&request, cancellation)
            .await
    }

    fn profile(&self, id: Uuid) -> Result<LlmProfile, LlmProfileError> {
        self.lock()?
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
            .ok_or(LlmProfileError::NotFound)
    }

    pub(crate) fn generation_profile(&self, id: Uuid) -> Result<LlmProfile, LlmProfileError> {
        let mut profile = self.profile(id)?;
        profile.advanced.context_window = profile
            .advanced
            .context_window
            .min(MAX_GENERATION_CONTEXT_WINDOW);
        profile.advanced.maximum_answer_tokens = profile
            .advanced
            .maximum_answer_tokens
            .min(MAX_GENERATION_ANSWER_TOKENS);
        Ok(profile)
    }

    pub(crate) async fn summary_context_window(
        &self,
        id: Uuid,
        cancellation: &CancellationToken,
    ) -> Result<(u32, bool), LlmProfileError> {
        let profile = self.generation_profile(id)?;
        let endpoint = normalize_endpoint(&profile.base_url)?;
        self.enforce_policy(&endpoint, profile.advanced.tls_policy)?;
        if endpoint.locality == EndpointLocality::Cloud
            && profile.consented_host.as_deref() != Some(endpoint.base_url.as_str())
        {
            return Err(LlmProfileError::ConsentRequired(endpoint.host));
        }
        let request = self.probe_request(&profile).await?;
        let reported = self
            .transport
            .discover_model_limits(&request, cancellation)
            .await?
            .context_window;
        if profile.preset == LlmPreset::Ollama && reported.is_none() {
            return Err(LlmProfileError::InvalidConfiguration);
        }
        Ok((
            reported.map_or(profile.advanced.context_window, |limit| {
                profile.advanced.context_window.min(limit)
            }),
            reported.is_some(),
        ))
    }

    async fn store_api_key(
        &self,
        name: &str,
        api_key: Option<String>,
    ) -> Result<Option<CredentialRef>, LlmProfileError> {
        match api_key {
            None => Ok(None),
            Some(key) if key.trim().is_empty() => Err(LlmProfileError::InvalidConfiguration),
            Some(key) => self
                .credentials
                .store(StoreCredentialRequest::new(
                    format!("LLM profile: {name}"),
                    SecretMaterial::password(key),
                ))
                .await
                .map(Some)
                .map_err(|_| LlmProfileError::Credential),
        }
    }

    async fn probe_request(
        &self,
        profile: &LlmProfile,
    ) -> Result<LlmProbeRequest, LlmProfileError> {
        let endpoint = normalize_endpoint(&profile.base_url)?;
        let api_key = self.api_key_for(profile).await?;
        Ok(LlmProbeRequest {
            url: chat_completions_url(profile, &endpoint)?,
            ollama_generation_url: (profile.preset == LlmPreset::Ollama)
                .then(|| ollama_chat_url(&endpoint)),
            model: profile.model.clone(),
            api_key,
            api_key_header: api_key_header(profile),
            headers: profile.advanced.custom_headers.clone(),
            timeout: Duration::from_secs(profile.advanced.timeout_seconds),
            reasoning_switch: ReasoningSwitch::Plain,
        })
    }

    /// Resolves the profile's stored key in the shape its provider wants. Secrets stay inside this
    /// capability: callers get a value for one bounded request, never a stored reference.
    async fn api_key_for(&self, profile: &LlmProfile) -> Result<Option<String>, LlmProfileError> {
        let reference = match &profile.credential_ref {
            None => return Ok(None),
            Some(reference) => reference,
        };
        let resolved = self
            .credentials
            .resolve(reference)
            .await
            .map_err(|_| LlmProfileError::Credential)?;
        match resolved.secret {
            SecretMaterial::Password { password } => {
                let key = password.to_string();
                Ok(Some(if profile.preset == LlmPreset::AzureOpenAi {
                    key
                } else {
                    format!("Bearer {key}")
                }))
            }
            _ => Err(LlmProfileError::Credential),
        }
    }

    /// Where and how to reach the endpoint the image role points at (task 0325). Like
    /// [`Self::resolve_role`] it never prompts: a missing consent or a cloud endpoint without the
    /// role's cloud permission is reported for the caller to surface. The profile's chat timeout is
    /// raised to the floor image generation needs, because a batch takes far longer than a reply.
    pub async fn image_request(
        &self,
        role: LlmRole,
    ) -> Result<ImageEndpointRequest, LlmProfileError> {
        if role != LlmRole::Image {
            return Err(LlmProfileError::InvalidConfiguration);
        }
        let resolved = self.resolve_role(role)?;
        let profile = resolved.profile;
        let endpoint = normalize_endpoint(&profile.base_url)?;
        let api_key = self.api_key_for(&profile).await?;
        // Which contract the endpoint answers is what the profile says it answers; a profile saved
        // before the task API existed keeps using the OpenAI-style one.
        let api = if profile
            .capabilities
            .contains(&LlmApiCapability::DashscopeTasks)
        {
            if profile.preset == LlmPreset::AzureOpenAi {
                // Azure has no task API; the pairing would only produce a request nobody answers.
                return Err(LlmProfileError::InvalidConfiguration);
            }
            ImageApiStyle::DashscopeTasks {
                task_url_prefix: model_studio_prefix(&endpoint) + "/tasks",
            }
        } else {
            ImageApiStyle::OpenAiImages
        };
        let url = match &api {
            ImageApiStyle::OpenAiImages => images_url(&profile, &endpoint)?,
            ImageApiStyle::DashscopeTasks { .. } => {
                model_studio_prefix(&endpoint) + "/services/aigc/text2image/image-synthesis"
            }
        };
        Ok(ImageEndpointRequest {
            url,
            model: resolved.model,
            api_key,
            api_key_header: api_key_header(&profile),
            headers: profile.advanced.custom_headers.clone(),
            timeout: Duration::from_secs(
                profile
                    .advanced
                    .timeout_seconds
                    .max(crate::image_generation::MIN_IMAGE_TIMEOUT_SECONDS),
            ),
            api,
        })
    }

    fn enforce_policy(
        &self,
        endpoint: &NormalizedEndpoint,
        tls: LlmTlsPolicy,
    ) -> Result<(), LlmProfileError> {
        if tls == LlmTlsPolicy::RequireHttps && !endpoint.base_url.starts_with("https://") {
            return Err(LlmProfileError::Tls);
        }
        self.host_policy
            .permits(endpoint)
            .then_some(())
            .ok_or(LlmProfileError::PolicyDenied)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, LlmProfilesDocument>, LlmProfileError> {
        self.profiles
            .lock()
            .map_err(|_| LlmProfileError::Persistence)
    }

    fn persist(&self, document: &LlmProfilesDocument) -> Result<(), LlmProfileError> {
        self.settings
            .save_document(document)
            .map_err(|_| LlmProfileError::Persistence)
    }
}

fn preset_draft(preset: LlmPreset, name: &str, base_url: &str) -> LlmProfileDraft {
    LlmProfileDraft {
        name: name.to_owned(),
        preset,
        base_url: base_url.to_owned(),
        deployment: None,
        api_version: (preset == LlmPreset::AzureOpenAi).then(|| "2024-10-21".to_owned()),
        model: String::new(),
        api_key: None,
        advanced: LlmAdvancedSettings::default(),
        capabilities: BTreeSet::from([
            LlmApiCapability::ChatCompletions,
            LlmApiCapability::ModelDiscovery,
        ]),
        redact_filenames: preset == LlmPreset::AzureOpenAi || preset == LlmPreset::OpenAiCompatible,
    }
}

fn profile_from_draft(
    id: Uuid,
    draft: LlmProfileDraft,
    credential_ref: Option<CredentialRef>,
) -> LlmProfile {
    LlmProfile {
        id,
        name: draft.name,
        preset: draft.preset,
        base_url: draft.base_url,
        deployment: draft.deployment,
        api_version: draft.api_version,
        model: draft.model,
        credential_ref,
        advanced: draft.advanced,
        capabilities: draft.capabilities,
        consented_host: None,
        redact_filenames: draft.redact_filenames,
        reasoning_switch: None,
    }
}

fn validate_profile(profile: &LlmProfile) -> Result<(), LlmProfileError> {
    if profile.name.trim().is_empty()
        || profile.model.trim().is_empty()
        || profile.advanced.context_window == 0
        || profile.advanced.maximum_answer_tokens == 0
        || profile.advanced.maximum_answer_tokens > profile.advanced.context_window
        || profile.advanced.context_window > MAX_GENERATION_CONTEXT_WINDOW
        || profile.advanced.maximum_answer_tokens > MAX_GENERATION_ANSWER_TOKENS
        || !profile.advanced.temperature.is_finite()
        || !(0.0..=2.0).contains(&profile.advanced.temperature)
        || profile.advanced.timeout_seconds == 0
        || profile.advanced.timeout_seconds > 600
        || profile.capabilities.is_empty()
        // A profile has to answer something this app asks of it. Chat needs the completions API;
        // an endpoint that only speaks one of the images contracts is a legitimate image endpoint.
        || !(profile
            .capabilities
            .contains(&LlmApiCapability::ChatCompletions)
            || profile
                .capabilities
                .contains(&LlmApiCapability::ImageGenerations)
            || profile
                .capabilities
                .contains(&LlmApiCapability::DashscopeTasks))
    {
        return Err(LlmProfileError::InvalidConfiguration);
    }
    normalize_endpoint(&profile.base_url)?;
    if profile.preset == LlmPreset::AzureOpenAi
        && (profile.deployment.as_deref().is_none_or(str::is_empty)
            || profile.api_version.as_deref().is_none_or(str::is_empty))
    {
        return Err(LlmProfileError::InvalidConfiguration);
    }
    for (name, value) in &profile.advanced.custom_headers {
        if !ALLOWED_CUSTOM_HEADERS.contains(&name.to_ascii_lowercase().as_str())
            || value.contains(['\r', '\n'])
        {
            return Err(LlmProfileError::InvalidConfiguration);
        }
    }
    Ok(())
}

fn normalize_endpoint(value: &str) -> Result<NormalizedEndpoint, LlmProfileError> {
    let mut url = Url::parse(value).map_err(|_| LlmProfileError::InvalidConfiguration)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(LlmProfileError::InvalidConfiguration);
    }

    let host = url.host().ok_or(LlmProfileError::InvalidConfiguration)?;
    let loopback = match host {
        Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
    };
    let private_lan = match host {
        Host::Ipv4(address) => address.is_private() || address.is_link_local(),
        Host::Ipv6(address) => {
            let first = address.segments()[0];
            // Unique-local fc00::/7 and link-local fe80::/10.
            (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
        Host::Domain(_) => false,
    };
    let locality = if loopback || private_lan {
        EndpointLocality::Loopback
    } else {
        EndpointLocality::Cloud
    };
    let host = url
        .host_str()
        .ok_or(LlmProfileError::InvalidConfiguration)?
        .to_ascii_lowercase();
    let normalized_path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&normalized_path);
    Ok(NormalizedEndpoint {
        base_url: url.to_string().trim_end_matches('/').to_owned(),
        host,
        locality,
        loopback,
    })
}

pub(crate) fn normalize_endpoint_locality(
    value: &str,
) -> Result<EndpointLocality, LlmProfileError> {
    normalize_endpoint(value).map(|endpoint| endpoint.locality)
}

/// Classifies an endpoint URL for the editor, so the UI never re-implements the rule.
pub fn classify_endpoint(value: &str) -> Result<EndpointLocality, LlmProfileError> {
    normalize_endpoint_locality(value)
}

fn chat_completions_url(
    profile: &LlmProfile,
    endpoint: &NormalizedEndpoint,
) -> Result<String, LlmProfileError> {
    if profile.preset == LlmPreset::AzureOpenAi {
        let deployment = profile
            .deployment
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or(LlmProfileError::InvalidConfiguration)?;
        let api_version = profile
            .api_version
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or(LlmProfileError::InvalidConfiguration)?;
        return Ok(format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            endpoint.base_url,
            percent_encoding::utf8_percent_encode(deployment, percent_encoding::NON_ALPHANUMERIC),
            percent_encoding::utf8_percent_encode(api_version, percent_encoding::NON_ALPHANUMERIC)
        ));
    }
    let base = endpoint
        .base_url
        .strip_suffix("/v1")
        .unwrap_or(&endpoint.base_url);
    Ok(format!("{base}/v1/chat/completions"))
}

fn models_url(chat_url: &str) -> Option<String> {
    chat_url
        .strip_suffix("/chat/completions")
        .map(|base| format!("{base}/models"))
}

/// The header a profile's key travels in.
fn api_key_header(profile: &LlmProfile) -> &'static str {
    if profile.preset == LlmPreset::AzureOpenAi {
        "api-key"
    } else {
        "authorization"
    }
}

/// The images endpoint of the same OpenAI-style provider (task 0325). Azure addresses a deployment
/// directly; everything else, local servers included, answers under `/v1/images/generations`.
fn images_url(
    profile: &LlmProfile,
    endpoint: &NormalizedEndpoint,
) -> Result<String, LlmProfileError> {
    if profile.preset == LlmPreset::AzureOpenAi {
        let deployment = profile
            .deployment
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or(LlmProfileError::InvalidConfiguration)?;
        let api_version = profile
            .api_version
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or(LlmProfileError::InvalidConfiguration)?;
        return Ok(format!(
            "{}/openai/deployments/{}/images/generations?api-version={}",
            endpoint.base_url,
            percent_encoding::utf8_percent_encode(deployment, percent_encoding::NON_ALPHANUMERIC),
            percent_encoding::utf8_percent_encode(api_version, percent_encoding::NON_ALPHANUMERIC)
        ));
    }
    let base = endpoint
        .base_url
        .strip_suffix("/v1")
        .unwrap_or(&endpoint.base_url);
    Ok(format!("{base}/v1/images/generations"))
}

/// The `{origin}/api/v1` prefix a Model Studio endpoint is reached under, whether the person typed
/// the origin or the versioned prefix itself.
fn model_studio_prefix(endpoint: &NormalizedEndpoint) -> String {
    let trimmed = endpoint.base_url.trim_end_matches('/');
    let origin = trimmed.strip_suffix("/api/v1").unwrap_or(trimmed);
    format!("{origin}/api/v1")
}

/// Removes the chain-of-thought a reasoning model may put inline in its answer
/// (`<think>…</think>`, as emitted by Qwen, DeepSeek-R1, oMLX and LM Studio). A block that is never
/// closed means the model ran out of tokens while thinking, so nothing after it is an answer.
pub(crate) fn strip_reasoning(text: &str) -> String {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";
    let mut rest = text;
    let mut answer = String::with_capacity(text.len());
    while let Some(start) = rest.find(OPEN) {
        answer.push_str(&rest[..start]);
        let after = &rest[start + OPEN.len()..];
        match after.find(CLOSE) {
            Some(end) => rest = &after[end + CLOSE.len()..],
            None => return answer.trim().to_owned(),
        }
    }
    answer.push_str(rest);
    // Some templates open the block in the prompt, so only the closing tag appears in the output.
    let answer = answer
        .rsplit_once(CLOSE)
        .map_or(answer.as_str(), |(_, tail)| tail);
    answer.trim().to_owned()
}

/// Body of an OpenAI-compatible streaming chat request, carrying the thinking-off parameters the
/// given [`ReasoningSwitch`] stands for.
fn openai_generation_body(
    request: &LlmProbeRequest,
    generation: &LlmChatGeneration,
    switch: ReasoningSwitch,
) -> Value {
    let mut body = json!({
        "model": request.model,
        "messages": [
            {"role": "system", "content": generation.system_prompt},
            {"role": "user", "content": openai_user_content(generation)}
        ],
        "stream": true,
        "max_tokens": generation.maximum_tokens,
    });
    if let Some(temperature) = generation.temperature {
        body["temperature"] = json!(temperature);
    }
    if matches!(
        switch,
        ReasoningSwitch::Both | ReasoningSwitch::TemplateKwargs
    ) {
        body["chat_template_kwargs"] = json!({"enable_thinking": false});
    }
    if matches!(switch, ReasoningSwitch::Both | ReasoningSwitch::EffortNone) {
        body["reasoning_effort"] = json!("none");
    }
    body
}

/// The user message: plain text, or a content array with each image as a data URL after the text.
fn openai_user_content(generation: &LlmChatGeneration) -> Value {
    use base64::Engine as _;

    if generation.images.is_empty() {
        return Value::from(generation.user_prompt.as_str());
    }
    let mut parts = vec![json!({"type": "text", "text": generation.user_prompt})];
    parts.extend(generation.images.iter().map(|image| {
        let encoded = base64::engine::general_purpose::STANDARD.encode(&image.data);
        json!({
            "type": "image_url",
            "image_url": {"url": format!("data:{};base64,{encoded}", image.media_type)}
        })
    }));
    Value::Array(parts)
}

fn ollama_chat_url(endpoint: &NormalizedEndpoint) -> String {
    let base = endpoint
        .base_url
        .strip_suffix("/v1")
        .unwrap_or(&endpoint.base_url);
    format!("{base}/api/chat")
}

fn ollama_generation_body(request: &LlmProbeRequest, generation: &LlmChatGeneration) -> Value {
    use base64::Engine as _;

    let images = generation
        .images
        .iter()
        .map(|image| base64::engine::general_purpose::STANDARD.encode(&image.data))
        .collect::<Vec<_>>();
    let mut options = json!({"num_predict": generation.maximum_tokens});
    if let Some(temperature) = generation.temperature {
        options["temperature"] = json!(temperature);
    }
    if let Some(context_window) = generation.context_window {
        options["num_ctx"] = Value::from(context_window);
    }
    json!({
        "model": request.model,
        "messages": [
            {"role": "system", "content": generation.system_prompt},
            {"role": "user", "content": generation.user_prompt, "images": images}
        ],
        "stream": false,
        "think": false,
        "format": "json",
        "options": options
    })
}

fn parse_ollama_model_capabilities(body: &[u8]) -> Result<LlmModelCapabilities, LlmProfileError> {
    let value: Value =
        serde_json::from_slice(body).map_err(|_| LlmProfileError::MalformedResponse)?;
    let capabilities = value
        .get("capabilities")
        .and_then(Value::as_array)
        .ok_or(LlmProfileError::MalformedResponse)?;
    Ok(LlmModelCapabilities {
        vision: capabilities
            .iter()
            .any(|value| value.as_str() == Some("vision")),
    })
}

fn positive_limit(value: &Value, keys: &[&str]) -> Option<u32> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
    })
}

fn cap_model_limits(limits: LlmModelLimits) -> LlmModelLimits {
    LlmModelLimits {
        context_window: limits
            .context_window
            .map(|value| value.min(MAX_GENERATION_CONTEXT_WINDOW)),
        maximum_answer_tokens: limits
            .maximum_answer_tokens
            .map(|value| value.min(MAX_GENERATION_ANSWER_TOKENS)),
    }
}

fn parse_openai_model_limits(value: &Value, model_id: &str) -> LlmModelLimits {
    let model = value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|models| {
            models
                .iter()
                .find(|model| model.get("id").and_then(Value::as_str) == Some(model_id))
        });
    let Some(model) = model else {
        return LlmModelLimits::default();
    };
    LlmModelLimits {
        context_window: positive_limit(
            model,
            &[
                "context_length",
                "max_context_length",
                "context_window",
                "max_model_len",
            ],
        ),
        maximum_answer_tokens: positive_limit(
            model,
            &["max_output_tokens", "max_completion_tokens", "max_tokens"],
        ),
    }
}

fn parse_ollama_model_limits(value: &Value) -> LlmModelLimits {
    let context_window = value
        .get("model_info")
        .and_then(Value::as_object)
        .and_then(|info| {
            info.get("general.context_length")
                .and_then(Value::as_u64)
                .or_else(|| {
                    info.iter()
                        .find(|(key, _)| key.ends_with(".context_length"))
                        .and_then(|(_, value)| value.as_u64())
                })
        })
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0);
    LlmModelLimits {
        context_window,
        maximum_answer_tokens: None,
    }
}

fn validate_streaming_chat(bytes: &[u8]) -> Result<(), LlmProfileError> {
    parse_streaming_chat(bytes).map(|_| ())
}

fn parse_streaming_chat(bytes: &[u8]) -> Result<String, LlmProfileError> {
    parse_streaming_chat_detailed(bytes).map(|(text, _)| text)
}

/// Parses a streamed completion into its content and whether the model also streamed reasoning
/// (`reasoning_content`/`reasoning` deltas, or inline `<think>` tags).
fn parse_streaming_chat_detailed(bytes: &[u8]) -> Result<(String, bool), LlmProfileError> {
    let text = std::str::from_utf8(bytes).map_err(|_| LlmProfileError::MalformedResponse)?;
    let mut valid_event = false;
    let mut terminated = false;
    let mut truncated = false;
    let mut content = String::new();
    let mut reasoning_seen = false;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            terminated = true;
            continue;
        }
        let value: Value =
            serde_json::from_str(data).map_err(|_| LlmProfileError::MalformedResponse)?;
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .ok_or(LlmProfileError::MalformedResponse)?;
        valid_event = true;
        for choice in choices {
            reasoning_seen |= ["reasoning_content", "reasoning"].iter().any(|key| {
                choice
                    .get("delta")
                    .and_then(|delta| delta.get(*key))
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.is_empty())
            });
            if let Some(delta) = choice
                .get("delta")
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
            {
                content.push_str(delta);
                if content.len() > MAX_STREAM_BYTES {
                    return Err(LlmProfileError::MalformedResponse);
                }
            }
        }
        for choice in choices {
            if let Some(reason) = choice
                .get("finish_reason")
                .filter(|reason| !reason.is_null())
            {
                terminated = true;
                truncated |= reason.as_str() == Some("length");
            }
        }
    }
    if valid_event && terminated && !truncated {
        reasoning_seen |= content.contains("<think>") || content.contains("</think>");
        Ok((content, reasoning_seen))
    } else {
        Err(LlmProfileError::MalformedResponse)
    }
}

fn parse_ollama_chat(bytes: &[u8]) -> Result<String, LlmProfileError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| LlmProfileError::MalformedResponse)?;
    if value.get("done").and_then(Value::as_bool) != Some(true)
        || value.get("done_reason").and_then(Value::as_str) == Some("length")
    {
        return Err(LlmProfileError::MalformedResponse);
    }
    value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .filter(|content| !content.trim().is_empty())
        .map(str::to_owned)
        .ok_or(LlmProfileError::MalformedResponse)
}

fn classify_http_status(status: u16) -> Result<(), LlmProfileError> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(LlmProfileError::Authentication),
        404 => Err(LlmProfileError::ModelUnavailable),
        _ => Err(LlmProfileError::Transport),
    }
}

fn map_reqwest(error: reqwest::Error) -> LlmProfileError {
    if error.is_timeout() {
        LlmProfileError::Timeout
    } else if error.is_connect()
        && error
            .source()
            .is_some_and(|source| source.to_string().to_ascii_lowercase().contains("tls"))
    {
        LlmProfileError::Tls
    } else {
        LlmProfileError::Transport
    }
}

fn test_result(
    profile: &LlmProfile,
    locality: EndpointLocality,
    started: Instant,
    category: Option<LlmTestErrorCategory>,
    model_available: Option<bool>,
    available_models: Option<Vec<String>>,
) -> LlmProfileTestResult {
    LlmProfileTestResult {
        profile_id: profile.id,
        provider: profile.preset,
        locality,
        success: category.is_none(),
        category,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        model_available,
        available_models,
        capabilities: profile.capabilities.clone(),
    }
}

fn bounded_model_ids(models: Vec<String>) -> Vec<String> {
    let mut models = models
        .into_iter()
        .filter(|model| !model.trim().is_empty() && model.len() <= 512)
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models.truncate(256);
    models
}

/// Typed generation-profile failure with no response or secret content.
#[derive(Debug, Error, PartialEq)]
pub enum LlmProfileError {
    #[error("invalid LLM profile configuration")]
    InvalidConfiguration,
    #[error("LLM profile not found")]
    NotFound,
    #[error("LLM credential operation failed")]
    Credential,
    #[error("LLM profile persistence failed")]
    Persistence,
    #[error("LLM endpoint denied by administrator policy")]
    PolicyDenied,
    #[error("cloud consent required for host {0}")]
    ConsentRequired(String),
    #[error("no endpoint is assigned to this role")]
    RoleNotAssigned,
    #[error("this role may not use a cloud endpoint until cloud use is allowed")]
    CloudEmbeddingNotAllowed,
    #[error("the image role may not use a cloud endpoint until cloud use is allowed for it")]
    CloudImageNotAllowed,
    #[error("LLM authentication failed")]
    Authentication,
    #[error("LLM model is unavailable")]
    ModelUnavailable,
    #[error("LLM request timed out")]
    Timeout,
    #[error("LLM TLS policy failed")]
    Tls,
    #[error("LLM streaming response was malformed")]
    MalformedResponse,
    #[error("LLM request was cancelled")]
    Cancelled,
    #[error("LLM transport failed")]
    Transport,
}

impl LlmProfileError {
    #[must_use]
    pub const fn category(&self) -> LlmTestErrorCategory {
        match self {
            Self::InvalidConfiguration | Self::NotFound | Self::Credential | Self::Persistence => {
                LlmTestErrorCategory::InvalidConfiguration
            }
            Self::PolicyDenied | Self::CloudEmbeddingNotAllowed | Self::CloudImageNotAllowed => {
                LlmTestErrorCategory::PolicyDenied
            }
            Self::ConsentRequired(_) => LlmTestErrorCategory::ConsentRequired,
            Self::RoleNotAssigned => LlmTestErrorCategory::InvalidConfiguration,
            Self::Authentication => LlmTestErrorCategory::Authentication,
            Self::ModelUnavailable => LlmTestErrorCategory::ModelUnavailable,
            Self::Timeout => LlmTestErrorCategory::Timeout,
            Self::Tls => LlmTestErrorCategory::Tls,
            Self::MalformedResponse => LlmTestErrorCategory::MalformedResponse,
            Self::Cancelled => LlmTestErrorCategory::Cancelled,
            Self::Transport => LlmTestErrorCategory::Transport,
        }
    }
}

// --- Conversions (folded from `llm_profile_mapping`) ---

fn assignment_to_dto(assignment: RoleAssignment) -> LlmRoleAssignmentDto {
    LlmRoleAssignmentDto {
        profile_id: assignment.profile_id,
        model: assignment.model,
        allow_cloud: assignment.allow_cloud,
    }
}

pub(crate) fn roles_to_dto(roles: LlmRoles) -> LlmRolesDto {
    LlmRolesDto {
        chat: roles.chat.map(assignment_to_dto),
        embeddings: roles.embeddings.map(assignment_to_dto),
        image: roles.image.map(assignment_to_dto),
    }
}

pub(crate) fn role_from_dto(role: LlmRoleDto) -> LlmRole {
    match role {
        LlmRoleDto::Chat => LlmRole::Chat,
        LlmRoleDto::Embeddings => LlmRole::Embeddings,
        LlmRoleDto::Image => LlmRole::Image,
    }
}

/// The refusal for a role that may not reach a cloud endpoint without the person's flag.
const fn cloud_role_denied(role: LlmRole) -> LlmProfileError {
    match role {
        LlmRole::Embeddings => LlmProfileError::CloudEmbeddingNotAllowed,
        LlmRole::Image => LlmProfileError::CloudImageNotAllowed,
        LlmRole::Chat => LlmProfileError::PolicyDenied,
    }
}

pub(crate) fn role_input_from_dto(request: SaveLlmRoleRequestDto) -> RoleAssignmentInput {
    RoleAssignmentInput {
        profile_id: request.profile_id,
        model: request.model,
        allow_cloud: request.allow_cloud,
    }
}

pub(crate) fn model_limits_to_dto(limits: LlmModelLimits) -> LlmModelLimitsDto {
    LlmModelLimitsDto {
        context_window: limits.context_window,
        maximum_answer_tokens: limits.maximum_answer_tokens,
        context_window_cap: MAX_GENERATION_CONTEXT_WINDOW,
        maximum_answer_tokens_cap: MAX_GENERATION_ANSWER_TOKENS,
    }
}

pub(crate) fn draft_from_dto(request: SaveLlmProfileRequestDto) -> LlmProfileDraft {
    LlmProfileDraft {
        name: request.name,
        preset: preset_from_dto(request.preset),
        base_url: request.base_url,
        deployment: request.deployment,
        api_version: request.api_version,
        model: request.model,
        api_key: request.credential.map(|credential| credential.api_key),
        advanced: advanced_from_dto(request.advanced),
        capabilities: request
            .capabilities
            .into_iter()
            .map(capability_from_dto)
            .collect(),
        redact_filenames: request.redact_filenames,
    }
}

pub(crate) fn profile_to_dto(profile: LlmProfile) -> Result<LlmProfileDto, LlmProfileError> {
    let locality = normalize_endpoint_locality(&profile.base_url)?;
    Ok(LlmProfileDto {
        id: profile.id,
        name: profile.name,
        preset: preset_to_dto(profile.preset),
        base_url: profile.base_url,
        deployment: profile.deployment,
        api_version: profile.api_version,
        model: profile.model,
        has_credential: profile.credential_ref.is_some(),
        advanced: advanced_to_dto(profile.advanced),
        capabilities: profile
            .capabilities
            .into_iter()
            .map(capability_to_dto)
            .collect(),
        locality: locality_to_dto(locality),
        consented_host: profile.consented_host,
        redact_filenames: profile.redact_filenames,
    })
}

pub(crate) fn preset_to_profile_dto(draft: LlmProfileDraft) -> LlmProfilePresetDto {
    LlmProfilePresetDto {
        name: draft.name,
        preset: preset_to_dto(draft.preset),
        base_url: draft.base_url,
        deployment: draft.deployment,
        api_version: draft.api_version,
        model: draft.model,
        advanced: advanced_to_dto(draft.advanced),
        capabilities: draft
            .capabilities
            .into_iter()
            .map(capability_to_dto)
            .collect(),
        redact_filenames: draft.redact_filenames,
    }
}

pub(crate) fn profile_to_export_dto(profile: LlmProfile) -> LlmProfileExportDto {
    LlmProfileExportDto {
        name: profile.name,
        preset: preset_to_dto(profile.preset),
        base_url: profile.base_url,
        deployment: profile.deployment,
        api_version: profile.api_version,
        model: profile.model,
        advanced: advanced_to_dto(profile.advanced),
        capabilities: profile
            .capabilities
            .into_iter()
            .map(capability_to_dto)
            .collect(),
        redact_filenames: profile.redact_filenames,
    }
}

pub(crate) fn test_result_to_dto(result: LlmProfileTestResult) -> LlmProfileTestResultDto {
    LlmProfileTestResultDto {
        profile_id: result.profile_id,
        provider: preset_to_dto(result.provider),
        locality: locality_to_dto(result.locality),
        success: result.success,
        category: result.category.map(test_category_to_dto),
        duration_ms: result.duration_ms,
        model_available: result.model_available,
        available_models: result.available_models,
        capabilities: result
            .capabilities
            .into_iter()
            .map(capability_to_dto)
            .collect(),
    }
}

pub(crate) fn disposition_from_dto(
    disposition: OrphanLlmCredentialDispositionDto,
) -> OrphanCredentialDisposition {
    match disposition {
        OrphanLlmCredentialDispositionDto::Delete => OrphanCredentialDisposition::Delete,
        OrphanLlmCredentialDispositionDto::Retain => OrphanCredentialDisposition::Retain,
    }
}

fn preset_from_dto(value: LlmPresetDto) -> LlmPreset {
    match value {
        LlmPresetDto::Ollama => LlmPreset::Ollama,
        LlmPresetDto::LmStudio => LlmPreset::LmStudio,
        LlmPresetDto::Vllm => LlmPreset::Vllm,
        LlmPresetDto::Sglang => LlmPreset::Sglang,
        LlmPresetDto::Omlx => LlmPreset::Omlx,
        LlmPresetDto::MlxServe => LlmPreset::MlxServe,
        LlmPresetDto::Unsloth => LlmPreset::Unsloth,
        LlmPresetDto::OpenAiCompatible => LlmPreset::OpenAiCompatible,
        LlmPresetDto::AzureOpenAi => LlmPreset::AzureOpenAi,
        LlmPresetDto::ModelStudio => LlmPreset::ModelStudio,
        LlmPresetDto::QwenImage => LlmPreset::QwenImage,
    }
}

fn preset_to_dto(value: LlmPreset) -> LlmPresetDto {
    match value {
        LlmPreset::Ollama => LlmPresetDto::Ollama,
        LlmPreset::LmStudio => LlmPresetDto::LmStudio,
        LlmPreset::Vllm => LlmPresetDto::Vllm,
        LlmPreset::Sglang => LlmPresetDto::Sglang,
        LlmPreset::Omlx => LlmPresetDto::Omlx,
        LlmPreset::MlxServe => LlmPresetDto::MlxServe,
        LlmPreset::Unsloth => LlmPresetDto::Unsloth,
        LlmPreset::OpenAiCompatible => LlmPresetDto::OpenAiCompatible,
        LlmPreset::AzureOpenAi => LlmPresetDto::AzureOpenAi,
        LlmPreset::ModelStudio => LlmPresetDto::ModelStudio,
        LlmPreset::QwenImage => LlmPresetDto::QwenImage,
    }
}

fn capability_from_dto(value: LlmApiCapabilityDto) -> LlmApiCapability {
    match value {
        LlmApiCapabilityDto::ChatCompletions => LlmApiCapability::ChatCompletions,
        LlmApiCapabilityDto::Responses => LlmApiCapability::Responses,
        LlmApiCapabilityDto::ModelDiscovery => LlmApiCapability::ModelDiscovery,
        LlmApiCapabilityDto::ImageGenerations => LlmApiCapability::ImageGenerations,
        LlmApiCapabilityDto::DashscopeTasks => LlmApiCapability::DashscopeTasks,
        LlmApiCapabilityDto::Vision => LlmApiCapability::Vision,
    }
}

fn capability_to_dto(value: LlmApiCapability) -> LlmApiCapabilityDto {
    match value {
        LlmApiCapability::ChatCompletions => LlmApiCapabilityDto::ChatCompletions,
        LlmApiCapability::Responses => LlmApiCapabilityDto::Responses,
        LlmApiCapability::ModelDiscovery => LlmApiCapabilityDto::ModelDiscovery,
        LlmApiCapability::ImageGenerations => LlmApiCapabilityDto::ImageGenerations,
        LlmApiCapability::DashscopeTasks => LlmApiCapabilityDto::DashscopeTasks,
        LlmApiCapability::Vision => LlmApiCapabilityDto::Vision,
    }
}

fn advanced_from_dto(value: LlmAdvancedSettingsDto) -> LlmAdvancedSettings {
    LlmAdvancedSettings {
        context_window: value.context_window,
        maximum_answer_tokens: value.maximum_answer_tokens,
        temperature: value.temperature,
        timeout_seconds: value.timeout_seconds,
        tls_policy: match value.tls_policy {
            LlmTlsPolicyDto::RequireValidCertificate => LlmTlsPolicy::RequireValidCertificate,
            LlmTlsPolicyDto::RequireHttps => LlmTlsPolicy::RequireHttps,
        },
        custom_headers: value.custom_headers,
    }
}

fn advanced_to_dto(value: LlmAdvancedSettings) -> LlmAdvancedSettingsDto {
    LlmAdvancedSettingsDto {
        context_window: value.context_window,
        maximum_answer_tokens: value.maximum_answer_tokens,
        temperature: value.temperature,
        timeout_seconds: value.timeout_seconds,
        tls_policy: match value.tls_policy {
            LlmTlsPolicy::RequireValidCertificate => LlmTlsPolicyDto::RequireValidCertificate,
            LlmTlsPolicy::RequireHttps => LlmTlsPolicyDto::RequireHttps,
        },
        custom_headers: value.custom_headers,
    }
}

pub(crate) fn locality_to_dto(value: EndpointLocality) -> LlmEndpointLocalityDto {
    match value {
        EndpointLocality::Loopback => LlmEndpointLocalityDto::Loopback,
        EndpointLocality::Cloud => LlmEndpointLocalityDto::Cloud,
    }
}

fn test_category_to_dto(value: LlmTestErrorCategory) -> LlmTestErrorCategoryDto {
    match value {
        LlmTestErrorCategory::InvalidConfiguration => LlmTestErrorCategoryDto::InvalidConfiguration,
        LlmTestErrorCategory::PolicyDenied => LlmTestErrorCategoryDto::PolicyDenied,
        LlmTestErrorCategory::ConsentRequired => LlmTestErrorCategoryDto::ConsentRequired,
        LlmTestErrorCategory::Authentication => LlmTestErrorCategoryDto::Authentication,
        LlmTestErrorCategory::ModelUnavailable => LlmTestErrorCategoryDto::ModelUnavailable,
        LlmTestErrorCategory::Timeout => LlmTestErrorCategoryDto::Timeout,
        LlmTestErrorCategory::Tls => LlmTestErrorCategoryDto::Tls,
        LlmTestErrorCategory::MalformedResponse => LlmTestErrorCategoryDto::MalformedResponse,
        LlmTestErrorCategory::Cancelled => LlmTestErrorCategoryDto::Cancelled,
        LlmTestErrorCategory::Transport => LlmTestErrorCategoryDto::Transport,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_servers_are_asked_to_skip_thinking_only_when_enabled() {
        let request = LlmProbeRequest {
            url: "http://localhost:8000/v1/chat/completions".into(),
            ollama_generation_url: None,
            model: "m".into(),
            api_key: None,
            api_key_header: "authorization",
            headers: BTreeMap::new(),
            timeout: Duration::from_secs(30),
            reasoning_switch: ReasoningSwitch::Both,
        };
        let generation = LlmChatGeneration {
            system_prompt: "s".into(),
            user_prompt: "u".into(),
            images: Vec::new(),
            maximum_tokens: 10,
            context_window: None,
            temperature: Some(0.0),
        };
        let off = openai_generation_body(&request, &generation, ReasoningSwitch::Both);
        assert_eq!(off["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(off["reasoning_effort"], "none");
        let plain = openai_generation_body(&request, &generation, ReasoningSwitch::Plain);
        assert!(plain.get("chat_template_kwargs").is_none());
        assert!(plain.get("reasoning_effort").is_none());
    }

    #[test]
    fn reasoning_blocks_are_removed_from_generated_text() {
        assert_eq!(strip_reasoning("<think>hmm</think>{\"a\":1}"), "{\"a\":1}");
        assert_eq!(
            strip_reasoning("x<think>a</think>y<think>b</think>z"),
            "xyz"
        );
        assert_eq!(
            strip_reasoning("reasoning only</think>\n{\"a\":1}"),
            "{\"a\":1}"
        );
        assert_eq!(strip_reasoning("<think>never closed {\"a\":1}"), "");
        assert_eq!(strip_reasoning("plain"), "plain");
    }

    use super::*;
    use fm_credentials::InMemoryCredentialStore;
    use tempfile::tempdir;

    struct FakeTransport {
        models: Option<Vec<String>>,
        response: LlmProbeResponse,
        captured: Mutex<Vec<LlmProbeRequest>>,
        limits: Mutex<LlmModelLimits>,
        /// Failures `generate_chat` returns, front first, before it starts succeeding.
        generate_failures: Mutex<std::collections::VecDeque<LlmProfileError>>,
        generate_calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl LlmProbeTransport for FakeTransport {
        async fn discover_model_limits(
            &self,
            request: &LlmProbeRequest,
            _cancellation: &CancellationToken,
        ) -> Result<LlmModelLimits, LlmProfileError> {
            self.captured.lock().unwrap().push(request.clone());
            Ok(*self.limits.lock().unwrap())
        }

        async fn discover_models(
            &self,
            request: &LlmProbeRequest,
            _cancellation: &CancellationToken,
        ) -> Result<Option<Vec<String>>, LlmProfileError> {
            self.captured.lock().unwrap().push(request.clone());
            Ok(self.models.clone())
        }

        async fn stream_chat(
            &self,
            request: &LlmProbeRequest,
            cancellation: &CancellationToken,
        ) -> Result<LlmProbeResponse, LlmProfileError> {
            if cancellation.is_cancelled() {
                return Err(LlmProfileError::Cancelled);
            }
            self.captured.lock().unwrap().push(request.clone());
            Ok(self.response.clone())
        }

        async fn generate_chat(
            &self,
            request: &LlmProbeRequest,
            _generation: &LlmChatGeneration,
            cancellation: &CancellationToken,
        ) -> Result<String, LlmProfileError> {
            if cancellation.is_cancelled() {
                return Err(LlmProfileError::Cancelled);
            }
            self.generate_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if let Some(failure) = self.generate_failures.lock().unwrap().pop_front() {
                return Err(failure);
            }
            self.captured.lock().unwrap().push(request.clone());
            parse_streaming_chat(&self.response.body)
        }
    }

    fn service(
        policy: LlmHostPolicy,
    ) -> (
        LlmProfileService,
        Arc<InMemoryCredentialStore>,
        Arc<FakeTransport>,
    ) {
        let directory = tempdir().unwrap().keep();
        let credentials = Arc::new(InMemoryCredentialStore::new());
        let transport = Arc::new(FakeTransport {
            models: Some(vec!["model-a".to_owned()]),
            response: LlmProbeResponse {
                status: 200,
                body: b"data: {\"choices\":[{\"delta\":{\"content\":\"OK\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n".to_vec(),
            },
            captured: Mutex::new(Vec::new()),
            limits: Mutex::new(LlmModelLimits {
                context_window: Some(128_000),
                maximum_answer_tokens: Some(32_000),
            }),
            generate_failures: Mutex::new(std::collections::VecDeque::new()),
            generate_calls: std::sync::atomic::AtomicUsize::new(0),
        });
        (
            LlmProfileService::new(
                SettingsStore::new(directory),
                credentials.clone(),
                transport.clone(),
                policy,
            )
            .unwrap(),
            credentials,
            transport,
        )
    }

    fn draft(preset: LlmPreset, base_url: &str) -> LlmProfileDraft {
        let mut draft = preset_draft(preset, "Test", base_url);
        draft.model = "model-a".to_owned();
        draft
    }

    #[test]
    fn every_preset_has_safe_explicit_defaults() {
        let presets = LlmProfileService::presets();
        assert_eq!(presets.len(), 11);
        assert_eq!(presets[0].base_url, "http://127.0.0.1:11434");
        assert_eq!(presets[1].base_url, "http://127.0.0.1:1234");
        assert_eq!(presets[2].base_url, "http://127.0.0.1:8000");
        assert_eq!(presets[3].base_url, "http://127.0.0.1:30000");
        assert_eq!(presets[4].base_url, "http://127.0.0.1:8000");
        assert_eq!(presets[5].preset, LlmPreset::MlxServe);
        assert_eq!(presets[5].base_url, "http://127.0.0.1:11234");
        assert!(
            presets[5]
                .capabilities
                .contains(&LlmApiCapability::ModelDiscovery)
        );
        // Unsloth Studio is a local server on the Studio port; its API is the OpenAI-style one.
        assert_eq!(presets[6].preset, LlmPreset::Unsloth);
        assert_eq!(presets[6].base_url, "http://127.0.0.1:8888");
        assert_eq!(presets[6].name, "Unsloth Studio");
        assert!(
            !supports_reasoning_switch(LlmPreset::Unsloth),
            "the Studio server is not documented to take the thinking-off switches"
        );
        assert!(presets[7].redact_filenames);
        assert_eq!(presets[8].api_version.as_deref(), Some("2024-10-21"));

        // An image-only endpoint is a preset of its own, so the image role is one choice away.
        let qwen = presets
            .iter()
            .find(|preset| preset.preset == LlmPreset::QwenImage)
            .expect("Qwen-Image is offered as a preset");
        assert_eq!(qwen.base_url, "https://dashscope.aliyuncs.com/api/v1");
        assert_eq!(qwen.model, "qwen-image");
        assert_eq!(
            qwen.capabilities,
            std::collections::BTreeSet::from([LlmApiCapability::DashscopeTasks])
        );
        assert!(
            qwen.redact_filenames,
            "prompts leave the machine either way"
        );
    }

    #[test]
    fn provider_model_limits_are_model_specific_and_bounded() {
        let value = json!({"data": [
            {"id": "other", "context_length": 64, "max_output_tokens": 32},
            {"id": "selected", "max_model_len": 2_000_000, "max_completion_tokens": 50_000}
        ]});
        assert_eq!(
            cap_model_limits(parse_openai_model_limits(&value, "selected")),
            LlmModelLimits {
                context_window: Some(MAX_GENERATION_CONTEXT_WINDOW),
                maximum_answer_tokens: Some(MAX_GENERATION_ANSWER_TOKENS),
            }
        );
        assert_eq!(
            parse_openai_model_limits(&value, "missing"),
            LlmModelLimits::default()
        );
        assert_eq!(
            parse_openai_model_limits(
                &json!({"data": [{"id": "selected", "context_length": 0, "max_tokens": -1}]}),
                "selected"
            ),
            LlmModelLimits::default()
        );
    }

    #[test]
    fn ollama_model_info_reports_context_but_not_unadvertised_output() {
        let value = json!({"model_info": {"llama.context_length": 131072, "general.parameter_count": 7_000_000_000u64}});
        assert_eq!(
            parse_ollama_model_limits(&value),
            LlmModelLimits {
                context_window: Some(131_072),
                maximum_answer_tokens: None,
            }
        );
    }

    #[test]
    fn configured_generation_limits_have_hard_caps() {
        let mut profile = profile_from_draft(
            Uuid::nil(),
            draft(LlmPreset::Ollama, "http://127.0.0.1:11434"),
            None,
        );
        profile.advanced.context_window = MAX_GENERATION_CONTEXT_WINDOW;
        profile.advanced.maximum_answer_tokens = MAX_GENERATION_ANSWER_TOKENS;
        assert!(validate_profile(&profile).is_ok());
        profile.advanced.context_window += 1;
        assert!(matches!(
            validate_profile(&profile),
            Err(LlmProfileError::InvalidConfiguration)
        ));
        profile.advanced.context_window = MAX_GENERATION_CONTEXT_WINDOW;
        profile.advanced.maximum_answer_tokens += 1;
        assert!(matches!(
            validate_profile(&profile),
            Err(LlmProfileError::InvalidConfiguration)
        ));
    }

    #[tokio::test]
    async fn draft_limit_discovery_uses_selected_model_without_persisting_it() {
        let (service, _credentials, transport) = service(LlmHostPolicy::desktop());
        let mut profile = draft(LlmPreset::MlxServe, "http://127.0.0.1:11234");
        profile.model = "selected".to_owned();
        profile.api_key = Some("draft-test-key".to_owned());
        let limits = service
            .discover_draft_model_limits(profile, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(limits.context_window, Some(128_000));
        assert_eq!(
            limits.maximum_answer_tokens,
            Some(MAX_GENERATION_ANSWER_TOKENS)
        );
        let captured = transport.captured.lock().unwrap();
        assert_eq!(captured[0].model, "selected");
        assert_eq!(
            captured[0].api_key.as_deref(),
            Some("Bearer draft-test-key")
        );
        assert_eq!(captured[0].api_key_header, "authorization");
        let models_request = ReqwestLlmProbeTransport::new()
            .request(
                reqwest::Method::GET,
                &captured[0],
                &models_url(&captured[0].url).unwrap(),
            )
            .build()
            .unwrap();
        assert_eq!(models_request.url().path(), "/v1/models");
        assert_eq!(
            models_request
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .unwrap(),
            "Bearer draft-test-key"
        );
        drop(captured);
        let response = serde_json::to_string(&model_limits_to_dto(limits)).unwrap();
        assert!(!response.contains("draft-test-key"));
        assert!(service.list().unwrap().is_empty());
    }

    #[tokio::test]
    async fn draft_limit_discovery_rejects_denied_host_before_using_credential() {
        let (service, _credentials, transport) = service(LlmHostPolicy::server([]));
        let mut profile = draft(LlmPreset::MlxServe, "http://127.0.0.1:11234");
        profile.api_key = Some("draft-test-key".to_owned());
        assert!(matches!(
            service
                .discover_draft_model_limits(profile, &CancellationToken::new())
                .await,
            Err(LlmProfileError::PolicyDenied)
        ));
        assert!(transport.captured.lock().unwrap().is_empty());
    }

    #[test]
    fn discovered_model_ids_are_bounded_sorted_and_deduplicated() {
        let mut models = (0..300)
            .rev()
            .map(|index| format!("model-{index:03}"))
            .collect::<Vec<_>>();
        models.extend([
            "model-100".to_owned(),
            String::new(),
            " ".to_owned(),
            "x".repeat(513),
        ]);

        let bounded = bounded_model_ids(models);

        assert_eq!(bounded.len(), 256);
        assert_eq!(bounded.first().map(String::as_str), Some("model-000"));
        assert_eq!(bounded.last().map(String::as_str), Some("model-255"));
        assert_eq!(
            bounded
                .iter()
                .filter(|model| model.as_str() == "model-100")
                .count(),
            1
        );
    }

    #[test]
    fn azure_uses_deployment_url_api_version_and_api_key_header() {
        let mut profile = profile_from_draft(
            Uuid::new_v4(),
            draft(LlmPreset::AzureOpenAi, "https://tenant.openai.azure.com"),
            None,
        );
        profile.deployment = Some("gpt 4".to_owned());
        profile.api_version = Some("2024-10-21".to_owned());
        let endpoint = normalize_endpoint(&profile.base_url).unwrap();
        assert_eq!(
            chat_completions_url(&profile, &endpoint).unwrap(),
            "https://tenant.openai.azure.com/openai/deployments/gpt%204/chat/completions?api-version=2024%2D10%2D21"
        );
    }

    #[tokio::test]
    async fn secret_is_only_persisted_in_credential_store_and_export_is_redacted() {
        let (service, credentials, _) = service(LlmHostPolicy::desktop());
        let mut draft = draft(LlmPreset::Ollama, "http://localhost:11434");
        draft.api_key = Some("super-secret-key".to_owned());
        let profile = service.create(draft).await.unwrap();
        let reference = profile.credential_ref.expect("credential ref");
        assert!(
            service
                .export_profile(profile.id)
                .unwrap()
                .credential_ref
                .is_none()
        );
        assert_eq!(
            credentials.resolve(&reference).await.unwrap().secret,
            SecretMaterial::password("super-secret-key")
        );
        let persisted =
            std::fs::read_to_string(service.settings.document_path::<LlmProfilesDocument>())
                .unwrap();
        assert!(!persisted.contains("super-secret-key"));
        let request = service.probe_request(&profile).await.unwrap();
        assert_eq!(request.api_key_header, "authorization");
        assert_eq!(request.api_key.as_deref(), Some("Bearer super-secret-key"));
        assert!(!format!("{request:?}").contains("super-secret-key"));
    }

    #[test]
    fn version_one_profiles_migrate_without_inventing_consent() {
        let profile = profile_from_draft(
            Uuid::new_v4(),
            draft(LlmPreset::Ollama, "http://localhost:11434"),
            None,
        );
        let mut value = serde_json::to_value(LlmProfilesDocument {
            schema_version: 1,
            active_profile_id: None,
            profiles: vec![profile],
            roles: LlmRoles::default(),
        })
        .unwrap();
        let legacy = value["profiles"][0].as_object_mut().unwrap();
        legacy.remove("capabilities");
        legacy.remove("redactFilenames");
        legacy.remove("consentedHost");

        let migrated = LlmProfilesDocument::migrate(value, 1).unwrap();
        let document: LlmProfilesDocument = serde_json::from_value(migrated).unwrap();

        document.validate().unwrap();
        assert_eq!(document.schema_version, PROFILE_SCHEMA_VERSION);
        assert_eq!(document.active_profile_id, None);
        assert_eq!(document.profiles[0].consented_host, None);
        assert!(
            document.profiles[0]
                .capabilities
                .contains(&LlmApiCapability::ChatCompletions)
        );
    }

    #[test]
    fn version_two_profiles_migrate_without_changing_profile_order() {
        let first = profile_from_draft(
            Uuid::new_v4(),
            draft(LlmPreset::Ollama, "http://localhost:11434"),
            None,
        );
        let mut second = first.clone();
        second.id = Uuid::new_v4();
        second.name = "Second".to_owned();
        let mut value = serde_json::to_value(LlmProfilesDocument {
            schema_version: 2,
            active_profile_id: None,
            profiles: vec![first.clone(), second.clone()],
            roles: LlmRoles::default(),
        })
        .unwrap();
        value.as_object_mut().unwrap().remove("activeProfileId");

        let migrated = LlmProfilesDocument::migrate(value, 2).unwrap();
        let document: LlmProfilesDocument = serde_json::from_value(migrated).unwrap();

        document.validate().unwrap();
        assert_eq!(document.active_profile_id, None);
        assert_eq!(document.profiles, vec![first, second]);
    }

    #[tokio::test]
    async fn activating_a_profile_makes_it_the_shared_default() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let first = service
            .create(draft(LlmPreset::Ollama, "http://localhost:11434"))
            .await
            .unwrap();
        let mut second_draft = draft(LlmPreset::Ollama, "http://localhost:11434");
        second_draft.name = "Second".to_owned();
        let second = service.create(second_draft).await.unwrap();

        assert_eq!(service.list().unwrap()[0].id, first.id);
        service.activate(second.id, false).unwrap();
        assert_eq!(service.list().unwrap()[0].id, second.id);
    }

    #[test]
    fn ollama_capabilities_and_generation_preserve_bounded_images() {
        assert!(
            parse_ollama_model_capabilities(br#"{"capabilities":["completion","tools","vision"]}"#)
                .unwrap()
                .vision
        );
        assert!(
            !parse_ollama_model_capabilities(br#"{"capabilities":["completion"]}"#)
                .unwrap()
                .vision
        );
        let request = LlmProbeRequest {
            url: "http://localhost:11434/v1/chat/completions".into(),
            ollama_generation_url: Some("http://localhost:11434/api/chat".into()),
            model: "vision-model".into(),
            api_key: None,
            api_key_header: "authorization",
            headers: BTreeMap::new(),
            timeout: Duration::from_secs(30),
            reasoning_switch: ReasoningSwitch::Plain,
        };
        let body = ollama_generation_body(
            &request,
            &LlmChatGeneration {
                system_prompt: "Summarize.".into(),
                user_prompt: "Evidence".into(),
                images: vec![LlmImageAttachment {
                    media_type: "image/png".into(),
                    data: vec![1, 2, 3],
                }],
                maximum_tokens: 512,
                context_window: Some(8_192),
                temperature: Some(0.1),
            },
        );

        assert_eq!(body["messages"][1]["images"][0], "AQID");
        assert_eq!(body["format"], "json");
        assert_eq!(body["options"]["num_ctx"], 8_192);
    }

    #[test]
    fn an_unset_temperature_is_left_to_the_model_default() {
        let request = LlmProbeRequest {
            url: "http://127.0.0.1:8000/v1/chat/completions".into(),
            ollama_generation_url: None,
            model: "m".into(),
            api_key: None,
            api_key_header: "Authorization",
            headers: BTreeMap::new(),
            timeout: Duration::from_secs(30),
            reasoning_switch: ReasoningSwitch::Plain,
        };
        let generation = |temperature| LlmChatGeneration {
            system_prompt: "s".into(),
            user_prompt: "u".into(),
            images: Vec::new(),
            maximum_tokens: 8,
            context_window: None,
            temperature,
        };
        let unset = openai_generation_body(&request, &generation(None), ReasoningSwitch::Plain);
        assert!(unset.get("temperature").is_none());
        let set = openai_generation_body(&request, &generation(Some(0.2)), ReasoningSwitch::Plain);
        assert!((set["temperature"].as_f64().unwrap() - 0.2).abs() < 1e-6);
    }

    #[test]
    fn openai_user_content_puts_each_image_after_the_text() {
        let text_only = openai_user_content(&LlmChatGeneration {
            system_prompt: "s".into(),
            user_prompt: "Describe".into(),
            images: Vec::new(),
            maximum_tokens: 8,
            context_window: None,
            temperature: Some(0.0),
        });
        assert_eq!(text_only, json!("Describe"));

        let with_image = openai_user_content(&LlmChatGeneration {
            system_prompt: "s".into(),
            user_prompt: "Describe".into(),
            images: vec![LlmImageAttachment {
                media_type: "image/png".into(),
                data: vec![1, 2, 3],
            }],
            maximum_tokens: 8,
            context_window: None,
            temperature: Some(0.0),
        });
        assert_eq!(with_image[0], json!({"type": "text", "text": "Describe"}));
        assert_eq!(
            with_image[1]["image_url"]["url"],
            "data:image/png;base64,AQID"
        );
    }

    #[test]
    fn version_four_active_profile_migrates_to_chat_and_embeddings_roles() {
        let profile = profile_from_draft(
            Uuid::new_v4(),
            draft(LlmPreset::Ollama, "http://localhost:11434"),
            None,
        );
        let mut value = serde_json::to_value(LlmProfilesDocument {
            schema_version: 4,
            active_profile_id: Some(profile.id),
            profiles: vec![profile.clone()],
            roles: LlmRoles::default(),
        })
        .unwrap();
        value.as_object_mut().unwrap().remove("roles");

        let migrated = LlmProfilesDocument::migrate(value, 4).unwrap();
        let document: LlmProfilesDocument = serde_json::from_value(migrated).unwrap();

        document.validate().unwrap();
        let expected = RoleAssignment {
            profile_id: profile.id,
            model: profile.model.clone(),
            allow_cloud: false,
        };
        assert_eq!(document.roles.chat, Some(expected.clone()));
        assert_eq!(document.roles.embeddings, Some(expected));
    }

    #[test]
    fn version_four_without_active_profile_migrates_to_unassigned_roles() {
        let profile = profile_from_draft(
            Uuid::new_v4(),
            draft(LlmPreset::Ollama, "http://localhost:11434"),
            None,
        );
        let mut value = serde_json::to_value(LlmProfilesDocument {
            schema_version: 4,
            active_profile_id: None,
            profiles: vec![profile],
            roles: LlmRoles::default(),
        })
        .unwrap();
        value.as_object_mut().unwrap().remove("roles");

        let migrated = LlmProfilesDocument::migrate(value, 4).unwrap();
        let document: LlmProfilesDocument = serde_json::from_value(migrated).unwrap();

        assert_eq!(document.roles, LlmRoles::default());
    }

    #[tokio::test]
    async fn cloud_embeddings_role_requires_the_allow_cloud_flag() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(
                LlmPreset::OpenAiCompatible,
                "https://api.example.test",
            ))
            .await
            .unwrap();
        service
            .grant_endpoint_consent(profile.id)
            .expect("consent for the saved cloud host");

        let blocked = service.set_role(
            LlmRole::Embeddings,
            Some(RoleAssignmentInput {
                profile_id: profile.id,
                model: "embed-model".to_owned(),
                allow_cloud: false,
            }),
        );
        assert_eq!(blocked, Err(LlmProfileError::CloudEmbeddingNotAllowed));

        service
            .set_role(
                LlmRole::Embeddings,
                Some(RoleAssignmentInput {
                    profile_id: profile.id,
                    model: "embed-model".to_owned(),
                    allow_cloud: true,
                }),
            )
            .unwrap();
        let resolved = service.resolve_role(LlmRole::Embeddings).unwrap();
        assert_eq!(resolved.model, "embed-model");
        assert_eq!(resolved.profile.id, profile.id);
    }

    /// Creates one endpoint with a key, ready to be assigned to a role.
    async fn endpoint(
        service: &LlmProfileService,
        preset: LlmPreset,
        base_url: &str,
    ) -> LlmProfile {
        let mut draft = draft(preset, base_url);
        draft.api_key = Some("sk-test-key".to_owned());
        service.create(draft).await.expect("created")
    }

    #[tokio::test]
    async fn an_image_endpoint_uses_the_contract_its_capabilities_claim() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let local = endpoint(&service, LlmPreset::Vllm, "http://127.0.0.1:8000").await;
        service
            .set_role(
                LlmRole::Image,
                Some(RoleAssignmentInput {
                    profile_id: local.id,
                    model: "flux".to_owned(),
                    allow_cloud: false,
                }),
            )
            .unwrap();
        let request = service.image_request(LlmRole::Image).await.unwrap();
        assert_eq!(request.api, ImageApiStyle::OpenAiImages);
        assert_eq!(request.url, "http://127.0.0.1:8000/v1/images/generations");

        // A Model Studio endpoint (qwen-image) says so, and then both URLs are its own: the task is
        // submitted to the synthesis route and polled under the task route.
        let mut draft = draft(
            LlmPreset::ModelStudio,
            "https://dashscope.aliyuncs.com/api/v1",
        );
        draft.capabilities = std::collections::BTreeSet::from([LlmApiCapability::DashscopeTasks]);
        draft.api_key = Some("sk-test-key".to_owned());
        let studio = service.create(draft).await.expect("created");
        service
            .grant_endpoint_consent(studio.id)
            .expect("consented");
        service
            .set_role(
                LlmRole::Image,
                Some(RoleAssignmentInput {
                    profile_id: studio.id,
                    model: "qwen-image".to_owned(),
                    allow_cloud: true,
                }),
            )
            .unwrap();
        let request = service.image_request(LlmRole::Image).await.unwrap();
        assert_eq!(
            request.api,
            ImageApiStyle::DashscopeTasks {
                task_url_prefix: "https://dashscope.aliyuncs.com/api/v1/tasks".to_owned(),
            }
        );
        assert_eq!(
            request.url,
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/text2image/image-synthesis"
        );
        assert_eq!(
            request.timeout,
            Duration::from_secs(crate::image_generation::MIN_IMAGE_TIMEOUT_SECONDS),
            "a queued task needs far more time than a chat reply"
        );
    }

    #[tokio::test]
    async fn an_image_only_endpoint_serves_images_and_refuses_the_chat_role() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let mut draft = draft(
            LlmPreset::ModelStudio,
            "https://dashscope.aliyuncs.com/api/v1",
        );
        draft.capabilities = std::collections::BTreeSet::from([LlmApiCapability::DashscopeTasks]);
        draft.api_key = Some("sk-test-key".to_owned());
        let studio = service.create(draft).await.expect("created");
        service
            .grant_endpoint_consent(studio.id)
            .expect("consented");

        // The same endpoint cannot answer both roles; the chat role needs the completions API.
        assert_eq!(
            service.set_role(
                LlmRole::Chat,
                Some(RoleAssignmentInput {
                    profile_id: studio.id,
                    model: "qwen-plus".to_owned(),
                    allow_cloud: true,
                }),
            ),
            Err(LlmProfileError::InvalidConfiguration)
        );
        service
            .set_role(
                LlmRole::Image,
                Some(RoleAssignmentInput {
                    profile_id: studio.id,
                    model: "qwen-image".to_owned(),
                    allow_cloud: true,
                }),
            )
            .unwrap();
        assert_eq!(
            service
                .image_request(LlmRole::Image)
                .await
                .expect("image endpoint")
                .model,
            "qwen-image"
        );
    }

    #[tokio::test]
    async fn a_task_api_endpoint_typed_by_its_origin_still_reaches_the_versioned_routes() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        // The person typed the origin rather than the versioned prefix the preset carries.
        let mut draft = draft(
            LlmPreset::OpenAiCompatible,
            "https://dashscope.aliyuncs.com",
        );
        draft.capabilities = std::collections::BTreeSet::from([LlmApiCapability::DashscopeTasks]);
        draft.api_key = Some("sk-test-key".to_owned());
        let profile = service.create(draft).await.expect("created");
        service
            .grant_endpoint_consent(profile.id)
            .expect("consented");
        service
            .set_role(
                LlmRole::Image,
                Some(RoleAssignmentInput {
                    profile_id: profile.id,
                    model: "wan2.5-t2i-preview".to_owned(),
                    allow_cloud: true,
                }),
            )
            .unwrap();
        let request = service.image_request(LlmRole::Image).await.unwrap();
        assert_eq!(
            request.url,
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/text2image/image-synthesis"
        );
        let ImageApiStyle::DashscopeTasks { task_url_prefix } = request.api else {
            panic!("the capability selects the task API");
        };
        assert_eq!(
            task_url_prefix,
            "https://dashscope.aliyuncs.com/api/v1/tasks"
        );
    }

    #[tokio::test]
    async fn the_image_role_is_a_binding_of_its_own() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let chat = endpoint(&service, LlmPreset::Ollama, "http://127.0.0.1:11434").await;
        let image = endpoint(&service, LlmPreset::Vllm, "http://127.0.0.1:8000").await;
        service
            .set_role(
                LlmRole::Chat,
                Some(RoleAssignmentInput {
                    profile_id: chat.id,
                    model: "chat-model".to_owned(),
                    allow_cloud: false,
                }),
            )
            .unwrap();
        service
            .set_role(
                LlmRole::Image,
                Some(RoleAssignmentInput {
                    profile_id: image.id,
                    model: "image-model".to_owned(),
                    allow_cloud: false,
                }),
            )
            .unwrap();
        let roles = service.roles().unwrap();
        assert_eq!(roles.chat.expect("chat").profile_id, chat.id);
        assert_eq!(roles.image.expect("image").profile_id, image.id);
        assert!(
            roles.embeddings.is_none(),
            "assigning the image role leaves the others alone"
        );
        let request = service.image_request(LlmRole::Image).await.unwrap();
        assert_eq!(
            request.model, "image-model",
            "the role's model wins over the one saved on the profile"
        );

        // Removing the endpoint leaves the role unassigned rather than dangling.
        service
            .delete(image.id, OrphanCredentialDisposition::Retain)
            .await
            .unwrap();
        assert!(service.roles().unwrap().image.is_none());
        assert_eq!(
            service
                .image_request(LlmRole::Image)
                .await
                .expect_err("unassigned"),
            LlmProfileError::RoleNotAssigned
        );
        assert_eq!(
            service
                .image_request(LlmRole::Chat)
                .await
                .expect_err("not an image role"),
            LlmProfileError::InvalidConfiguration,
            "only the image role has an images endpoint"
        );
    }

    #[tokio::test]
    async fn a_cloud_image_endpoint_needs_the_cloud_flag_and_the_saved_consent() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let profile = endpoint(
            &service,
            LlmPreset::OpenAiCompatible,
            "https://api.example.test",
        )
        .await;
        let input = |allow_cloud| RoleAssignmentInput {
            profile_id: profile.id,
            model: "image-model".to_owned(),
            allow_cloud,
        };
        assert_eq!(
            service.set_role(LlmRole::Image, Some(input(false))),
            Err(LlmProfileError::CloudImageNotAllowed)
        );
        service
            .set_role(LlmRole::Image, Some(input(true)))
            .expect("the person allowed cloud use for this role");
        assert_eq!(
            service
                .image_request(LlmRole::Image)
                .await
                .expect_err("no consent yet"),
            LlmProfileError::ConsentRequired("api.example.test".to_owned()),
            "the pane never prompts; consent lives in Settings"
        );
        service.grant_endpoint_consent(profile.id).expect("consent");
        let request = service.image_request(LlmRole::Image).await.unwrap();
        assert_eq!(
            request.url,
            "https://api.example.test/v1/images/generations"
        );
    }

    #[tokio::test]
    async fn an_image_request_addresses_the_images_endpoint_within_a_long_enough_window() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let profile = endpoint(&service, LlmPreset::Ollama, "http://127.0.0.1:11434/v1").await;
        assert_eq!(
            profile.advanced.timeout_seconds, 30,
            "the profile keeps the chat timeout"
        );
        service
            .set_role(
                LlmRole::Image,
                Some(RoleAssignmentInput {
                    profile_id: profile.id,
                    model: "image-model".to_owned(),
                    allow_cloud: false,
                }),
            )
            .unwrap();
        let request = service.image_request(LlmRole::Image).await.unwrap();
        assert_eq!(
            request.url, "http://127.0.0.1:11434/v1/images/generations",
            "a base url that already names /v1 is not doubled"
        );
        assert_eq!(request.api_key.as_deref(), Some("Bearer sk-test-key"));
        assert_eq!(request.api_key_header, "authorization");
        assert!(
            request.timeout
                >= Duration::from_secs(crate::image_generation::MIN_IMAGE_TIMEOUT_SECONDS),
            "a batch outlives a chat reply"
        );
        assert!(
            !format!("{request:?}").contains("sk-test-key"),
            "the key is not a debug field"
        );
    }

    #[tokio::test]
    async fn an_azure_image_request_addresses_its_deployment() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let mut profile_draft = draft(LlmPreset::AzureOpenAi, "https://example.openai.azure.com");
        profile_draft.deployment = Some("flux".to_owned());
        profile_draft.api_key = Some("sk-test-key".to_owned());
        let profile = service.create(profile_draft).await.expect("created");
        service
            .set_role(
                LlmRole::Image,
                Some(RoleAssignmentInput {
                    profile_id: profile.id,
                    model: "flux".to_owned(),
                    allow_cloud: true,
                }),
            )
            .unwrap();
        service.grant_endpoint_consent(profile.id).expect("consent");
        let request = service.image_request(LlmRole::Image).await.unwrap();
        assert_eq!(
            request.url,
            "https://example.openai.azure.com/openai/deployments/flux/images/generations?api-version=2024%2D10%2D21"
        );
        assert_eq!(request.api_key.as_deref(), Some("sk-test-key"));
        assert_eq!(request.api_key_header, "api-key");
    }

    #[tokio::test]
    async fn cloud_chat_role_needs_consent_for_the_saved_host() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(
                LlmPreset::OpenAiCompatible,
                "https://api.example.test",
            ))
            .await
            .unwrap();
        service
            .set_role(
                LlmRole::Chat,
                Some(RoleAssignmentInput {
                    profile_id: profile.id,
                    model: profile.model.clone(),
                    allow_cloud: false,
                }),
            )
            .unwrap();

        assert_eq!(
            service.resolve_role(LlmRole::Chat),
            Err(LlmProfileError::ConsentRequired(
                "api.example.test".to_owned()
            ))
        );
        service.grant_endpoint_consent(profile.id).unwrap();
        assert_eq!(
            service.resolve_role(LlmRole::Chat).unwrap().profile.id,
            profile.id
        );
    }

    #[tokio::test]
    async fn local_chat_role_needs_no_consent_and_unset_role_is_reported() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        assert_eq!(
            service.resolve_role(LlmRole::Chat),
            Err(LlmProfileError::RoleNotAssigned)
        );
        let profile = service
            .create(draft(LlmPreset::Ollama, "http://localhost:11434"))
            .await
            .unwrap();
        service
            .set_role(
                LlmRole::Chat,
                Some(RoleAssignmentInput {
                    profile_id: profile.id,
                    model: profile.model.clone(),
                    allow_cloud: false,
                }),
            )
            .unwrap();
        assert_eq!(
            service.resolve_role(LlmRole::Chat).unwrap().profile.id,
            profile.id
        );
    }

    #[test]
    fn endpoint_classification_is_local_for_loopback_and_private_lan_only() {
        for url in [
            "http://localhost:11434",
            "http://127.0.0.1:1234",
            "http://[::1]:8000",
            "http://192.168.1.20:8000",
            "http://10.0.0.5",
            "http://172.20.1.1",
            "http://169.254.10.1",
            "http://[fd00::1]:8000",
            "http://[fe80::1]:8000",
        ] {
            assert_eq!(
                classify_endpoint(url),
                Ok(EndpointLocality::Loopback),
                "{url}"
            );
        }
        for url in [
            "https://api.example.test",
            "http://printer.local",
            "http://172.32.0.1",
            "https://8.8.8.8",
            "http://[2001:db8::1]",
        ] {
            assert_eq!(classify_endpoint(url), Ok(EndpointLocality::Cloud), "{url}");
        }
        assert!(classify_endpoint("not a url").is_err());
    }

    #[test]
    fn server_policy_keeps_private_lan_under_the_host_allow_list() {
        let policy = LlmHostPolicy::server(["192.168.1.20".to_owned()]);
        let lan = normalize_endpoint("http://192.168.1.20:8000").unwrap();
        let other_lan = normalize_endpoint("http://192.168.1.21:8000").unwrap();
        let loopback = normalize_endpoint("http://127.0.0.1:8000").unwrap();

        assert!(policy.permits(&lan));
        assert!(!policy.permits(&other_lan));
        assert!(!policy.permits(&loopback));
    }

    #[test]
    fn dangling_role_is_pruned_without_losing_the_profile_document() {
        let profile = profile_from_draft(
            Uuid::new_v4(),
            draft(LlmPreset::Ollama, "http://localhost:11434"),
            None,
        );
        let mut document = LlmProfilesDocument {
            schema_version: PROFILE_SCHEMA_VERSION,
            active_profile_id: Some(profile.id),
            profiles: vec![profile.clone()],
            roles: LlmRoles {
                image: None,
                chat: Some(RoleAssignment {
                    profile_id: Uuid::new_v4(),
                    model: "gone".to_owned(),
                    allow_cloud: false,
                }),
                embeddings: None,
            },
        };

        document.validate().unwrap();
        document.prune_dangling_roles();

        document.validate().unwrap();
        assert_eq!(document.roles.chat, None);
        assert_eq!(document.profiles, vec![profile]);
    }

    #[tokio::test]
    async fn host_change_invalidates_role_consent() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(
                LlmPreset::OpenAiCompatible,
                "https://api.example.test",
            ))
            .await
            .unwrap();
        service
            .set_role(
                LlmRole::Chat,
                Some(RoleAssignmentInput {
                    profile_id: profile.id,
                    model: profile.model.clone(),
                    allow_cloud: false,
                }),
            )
            .unwrap();
        service.grant_endpoint_consent(profile.id).unwrap();

        let mut changed = draft(LlmPreset::OpenAiCompatible, "https://other.example.test");
        changed.name = profile.name.clone();
        service.update(profile.id, changed).await.unwrap();

        assert_eq!(
            service.resolve_role(LlmRole::Chat),
            Err(LlmProfileError::ConsentRequired(
                "other.example.test".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn host_change_invalidates_cloud_consent() {
        let (service, _, _) = service(LlmHostPolicy::server([
            "one.example".to_owned(),
            "two.example".to_owned(),
        ]));
        let profile = service
            .create(draft(LlmPreset::OpenAiCompatible, "https://one.example"))
            .await
            .unwrap();
        assert!(matches!(
            service.activate(profile.id, false),
            Err(LlmProfileError::ConsentRequired(host)) if host == "one.example"
        ));
        assert_eq!(
            service
                .activate(profile.id, true)
                .unwrap()
                .consented_host
                .as_deref(),
            Some("https://one.example")
        );
        let changed = service
            .update(
                profile.id,
                draft(LlmPreset::OpenAiCompatible, "https://two.example"),
            )
            .await
            .unwrap();
        assert_eq!(changed.consented_host, None);
    }

    #[tokio::test]
    async fn changing_a_cloud_endpoint_path_or_port_invalidates_consent() {
        let (service, _, _) = service(LlmHostPolicy::server(["one.example".to_owned()]));
        let profile = service
            .create(draft(
                LlmPreset::OpenAiCompatible,
                "https://one.example:8443/first",
            ))
            .await
            .unwrap();
        assert_eq!(
            service
                .activate(profile.id, true)
                .unwrap()
                .consented_host
                .as_deref(),
            Some("https://one.example:8443/first")
        );
        let changed_path = service
            .update(
                profile.id,
                draft(
                    LlmPreset::OpenAiCompatible,
                    "https://one.example:8443/second",
                ),
            )
            .await
            .unwrap();
        assert_eq!(changed_path.consented_host, None);
        let changed_port = service
            .update(
                profile.id,
                draft(
                    LlmPreset::OpenAiCompatible,
                    "https://one.example:9443/second",
                ),
            )
            .await
            .unwrap();
        assert_eq!(changed_port.consented_host, None);
    }

    #[test]
    fn legacy_consent_survives_only_for_the_current_configured_host() {
        let mut profile = profile_from_draft(
            Uuid::new_v4(),
            draft(
                LlmPreset::OpenAiCompatible,
                "https://one.example:8443/current",
            ),
            None,
        );
        profile.consented_host = Some("one.example".into());
        let value = serde_json::to_value(LlmProfilesDocument {
            schema_version: 3,
            active_profile_id: Some(profile.id),
            profiles: vec![profile],
            roles: LlmRoles::default(),
        })
        .unwrap();
        let migrated = LlmProfilesDocument::migrate(value, 3).unwrap();
        let document: LlmProfilesDocument = serde_json::from_value(migrated).unwrap();
        document.validate().unwrap();
        assert_eq!(document.active_profile_id, Some(document.profiles[0].id));
        assert_eq!(
            document.profiles[0].consented_host.as_deref(),
            Some("https://one.example:8443/current")
        );

        let mut value = serde_json::to_value(document).unwrap();
        value["schemaVersion"] = Value::from(3);
        value["profiles"][0]["consentedHost"] = Value::String("elsewhere.example".into());
        let migrated = LlmProfilesDocument::migrate(value, 3).unwrap();
        let document: LlmProfilesDocument = serde_json::from_value(migrated).unwrap();
        assert_eq!(document.profiles[0].consented_host, None);
    }

    #[tokio::test]
    async fn server_policy_blocks_loopback_and_unlisted_cloud_hosts() {
        let (service, _, _) = service(LlmHostPolicy::server(["allowed.example".to_owned()]));
        let local = service
            .create(draft(LlmPreset::Ollama, "http://127.0.0.1:11434"))
            .await
            .unwrap();
        assert_eq!(
            service
                .test(local.id, &CancellationToken::new())
                .await
                .unwrap()
                .category,
            Some(LlmTestErrorCategory::PolicyDenied)
        );
        let cloud = service
            .create(draft(
                LlmPreset::OpenAiCompatible,
                "https://metadata.invalid",
            ))
            .await
            .unwrap();
        assert_eq!(
            service
                .test(cloud.id, &CancellationToken::new())
                .await
                .unwrap()
                .category,
            Some(LlmTestErrorCategory::PolicyDenied)
        );
    }

    #[tokio::test]
    async fn https_requirement_is_reported_as_a_normalized_tls_failure() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let mut insecure = draft(LlmPreset::OpenAiCompatible, "http://example.test");
        insecure.advanced.tls_policy = LlmTlsPolicy::RequireHttps;
        let profile = service.create(insecure).await.unwrap();

        let result = service
            .test(profile.id, &CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(result.category, Some(LlmTestErrorCategory::Tls));
        assert!(!result.success);
    }

    #[tokio::test]
    async fn test_validates_model_and_stream_without_retaining_content() {
        let (service, _, transport) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(LlmPreset::Ollama, "http://localhost:11434"))
            .await
            .unwrap();
        let result = service
            .test(profile.id, &CancellationToken::new())
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.model_available, Some(true));
        assert_eq!(result.available_models, Some(vec!["model-a".to_owned()]));
        let captured = transport.captured.lock().unwrap();
        assert!(
            captured
                .iter()
                .all(|request| !request.url.contains(TEST_PROMPT))
        );
    }

    #[tokio::test]
    async fn test_returns_discovered_models_when_the_configured_model_is_unavailable() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let mut profile = draft(LlmPreset::Ollama, "http://localhost:11434");
        profile.model = "missing-model".to_owned();
        let profile = service.create(profile).await.unwrap();

        let result = service
            .test(profile.id, &CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(
            result.category,
            Some(LlmTestErrorCategory::ModelUnavailable)
        );
        assert_eq!(result.model_available, Some(false));
        assert_eq!(result.available_models, Some(vec!["model-a".to_owned()]));
    }

    #[tokio::test]
    async fn model_discovery_does_not_send_a_chat_probe() {
        let (service, _, transport) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(LlmPreset::Ollama, "http://localhost:11434"))
            .await
            .unwrap();

        let models = service
            .discover_models(profile.id, &CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(models, vec!["model-a".to_owned()]);
        let captured = transport.captured.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert!(captured[0].url.ends_with("/v1/chat/completions"));
    }

    #[tokio::test]
    async fn draft_model_discovery_accepts_an_empty_model_without_persisting_a_profile() {
        let (service, _, transport) = service(LlmHostPolicy::desktop());
        let mut profile = draft(LlmPreset::Ollama, "http://localhost:11434");
        profile.model.clear();
        profile.api_key = Some("draft-secret".to_owned());

        let models = service
            .discover_draft_models(profile, &CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(models, vec!["model-a".to_owned()]);
        assert!(service.list().unwrap().is_empty());
        let captured = transport.captured.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert!(captured[0].url.ends_with("/v1/chat/completions"));
        assert_eq!(captured[0].api_key.as_deref(), Some("Bearer draft-secret"));
    }

    #[test]
    fn malformed_streams_are_rejected_and_valid_streams_need_termination() {
        assert!(matches!(
            validate_streaming_chat(b"data: not-json\n\n"),
            Err(LlmProfileError::MalformedResponse)
        ));
        assert!(matches!(
            validate_streaming_chat(b"data: {\"choices\":[]}\n\n"),
            Err(LlmProfileError::MalformedResponse)
        ));
        assert!(
            validate_streaming_chat(
                b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n"
            )
            .is_ok()
        );
        assert!(matches!(
            parse_streaming_chat(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"An incomplete answer\"},\"finish_reason\":\"length\"}]}\n\n"
            ),
            Err(LlmProfileError::MalformedResponse)
        ));
        assert_eq!(
            parse_ollama_chat(
                br#"{"message":{"role":"assistant","content":"A complete answer."},"done":true,"done_reason":"stop"}"#
            )
            .unwrap(),
            "A complete answer."
        );
        assert!(matches!(
            parse_ollama_chat(
                br#"{"message":{"role":"assistant","content":"An incomplete answer"},"done":true,"done_reason":"length"}"#
            ),
            Err(LlmProfileError::MalformedResponse)
        ));
        let request = LlmProbeRequest {
            url: "http://localhost:11434/v1/chat/completions".into(),
            ollama_generation_url: Some("http://localhost:11434/api/chat".into()),
            model: "reasoning-model".into(),
            api_key: None,
            api_key_header: "authorization",
            headers: BTreeMap::new(),
            timeout: Duration::from_secs(30),
            reasoning_switch: ReasoningSwitch::Plain,
        };
        let body = ollama_generation_body(
            &request,
            &LlmChatGeneration {
                system_prompt: "Answer from evidence.".into(),
                user_prompt: "Question".into(),
                images: Vec::new(),
                maximum_tokens: 1_024,
                context_window: None,
                temperature: Some(0.2),
            },
        );
        assert_eq!(body["think"], false);
        assert_eq!(body["stream"], false);
        assert_eq!(body["format"], "json");
        assert_eq!(body["options"]["num_predict"], 1_024);
        assert_eq!(
            classify_http_status(401).unwrap_err().category(),
            LlmTestErrorCategory::Authentication
        );
        assert_eq!(
            classify_http_status(404).unwrap_err().category(),
            LlmTestErrorCategory::ModelUnavailable
        );
        assert_eq!(
            classify_http_status(500).unwrap_err().category(),
            LlmTestErrorCategory::Transport
        );
    }

    #[tokio::test]
    async fn summary_context_respects_advertised_caps_and_requires_ollama_report() {
        let (service, _, transport) = service(LlmHostPolicy::desktop());
        let mut ollama_draft = draft(LlmPreset::Ollama, "http://localhost:11434");
        ollama_draft.advanced.context_window = 8_192;
        let profile = service.create(ollama_draft).await.unwrap();
        let cancellation = CancellationToken::new();
        transport.limits.lock().unwrap().context_window = Some(4_096);
        assert_eq!(
            service
                .summary_context_window(profile.id, &cancellation)
                .await
                .unwrap(),
            (4_096, true)
        );
        transport.limits.lock().unwrap().context_window = None;
        assert!(matches!(
            service
                .summary_context_window(profile.id, &cancellation)
                .await,
            Err(LlmProfileError::InvalidConfiguration)
        ));

        let remote = service
            .create(draft(LlmPreset::OpenAiCompatible, "https://models.example"))
            .await
            .unwrap();
        service.activate(remote.id, true).unwrap();
        assert_eq!(
            service
                .summary_context_window(remote.id, &cancellation)
                .await
                .unwrap(),
            (remote.advanced.context_window, false)
        );
    }

    #[tokio::test]
    async fn cancellation_and_explicit_orphan_deletion_are_enforced() {
        let (service, credentials, _) = service(LlmHostPolicy::desktop());
        let mut draft = draft(LlmPreset::Ollama, "http://localhost:11434");
        draft.api_key = Some("key".to_owned());
        let profile = service.create(draft).await.unwrap();
        let reference = profile.credential_ref.unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            service
                .test(profile.id, &cancellation)
                .await
                .unwrap()
                .category,
            Some(LlmTestErrorCategory::Cancelled)
        );
        service
            .delete(profile.id, OrphanCredentialDisposition::Delete)
            .await
            .unwrap();
        assert!(credentials.resolve(&reference).await.is_err());
    }

    #[tokio::test]
    async fn generation_requires_cloud_consent_and_returns_only_stream_content() {
        let (service, _, _) = service(LlmHostPolicy::server(["allowed.example".to_owned()]));
        let profile = service
            .create(draft(
                LlmPreset::OpenAiCompatible,
                "https://allowed.example",
            ))
            .await
            .unwrap();
        let generation = LlmChatGeneration {
            system_prompt: "Summarize grounded evidence.".into(),
            user_prompt: "Evidence".into(),
            images: Vec::new(),
            maximum_tokens: 512,
            context_window: None,
            temperature: Some(0.1),
        };
        assert!(matches!(
            service
                .generate(
                    profile.id,
                    generation.clone(),
                    &CancellationToken::new()
                )
                .await,
            Err(LlmProfileError::ConsentRequired(host)) if host == "allowed.example"
        ));
        service.activate(profile.id, true).unwrap();

        assert_eq!(
            service
                .generate(profile.id, generation, &CancellationToken::new())
                .await
                .unwrap(),
            "OK"
        );
    }

    #[tokio::test]
    async fn streamed_generation_reports_the_answer_through_the_callback() {
        let (service, _, _) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(LlmPreset::OpenAiCompatible, "http://127.0.0.1:8000"))
            .await
            .unwrap();
        let mut seen = Vec::new();
        let answer = service
            .generate_streamed(
                profile.id,
                tiny_generation(),
                &CancellationToken::new(),
                &mut |fragment: &str| seen.push(fragment.to_owned()),
            )
            .await
            .unwrap();
        assert_eq!(answer, "OK");
        assert_eq!(seen, vec!["OK".to_owned()]);
    }

    #[test]
    fn streamed_content_reports_complete_lines_across_chunks() {
        let mut fragments = Vec::new();
        let mut sink = |fragment: &str| fragments.push(fragment.to_owned());
        let mut stream = StreamedContent {
            pending: Vec::new(),
            on_delta: &mut sink,
        };
        stream.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"He\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"reasoning_content\":\"x\"}}]}\n\nda");
        stream.push(b"ta: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}\r\n");
        stream.push(b"data: not-json\n");
        assert_eq!(fragments, vec!["He".to_owned(), "llo".to_owned()]);
    }

    fn tiny_generation() -> LlmChatGeneration {
        LlmChatGeneration {
            system_prompt: "Answer.".into(),
            user_prompt: "Question".into(),
            images: Vec::new(),
            maximum_tokens: 16,
            context_window: None,
            temperature: Some(0.0),
        }
    }

    #[tokio::test]
    async fn a_timeout_while_the_model_loads_is_retried_once() {
        let (service, _, transport) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(LlmPreset::OpenAiCompatible, "http://127.0.0.1:8000"))
            .await
            .unwrap();
        transport
            .generate_failures
            .lock()
            .unwrap()
            .push_back(LlmProfileError::Timeout);

        let answer = service
            .generate(profile.id, tiny_generation(), &CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(answer, "OK");
        assert_eq!(
            transport
                .generate_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            2
        );
    }

    #[tokio::test]
    async fn timeouts_are_retried_only_once_and_other_failures_not_at_all() {
        let (service, _, transport) = service(LlmHostPolicy::desktop());
        let profile = service
            .create(draft(LlmPreset::OpenAiCompatible, "http://127.0.0.1:8000"))
            .await
            .unwrap();
        {
            let mut failures = transport.generate_failures.lock().unwrap();
            failures.push_back(LlmProfileError::Timeout);
            failures.push_back(LlmProfileError::Timeout);
        }
        let error = service
            .generate(profile.id, tiny_generation(), &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(error, LlmProfileError::Timeout));
        assert_eq!(
            transport
                .generate_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            2
        );

        transport
            .generate_failures
            .lock()
            .unwrap()
            .push_back(LlmProfileError::ModelUnavailable);
        let error = service
            .generate(profile.id, tiny_generation(), &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(error, LlmProfileError::ModelUnavailable));
        assert_eq!(
            transport
                .generate_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            3
        );
    }

    /// Accepts only the listed switches (others are rejected like a strict server would) and says
    /// whether the model reasoned anyway.
    struct SwitchTransport {
        accepted: Vec<ReasoningSwitch>,
        reasons_anyway: bool,
        attempts: Mutex<Vec<ReasoningSwitch>>,
    }

    #[async_trait]
    impl LlmProbeTransport for SwitchTransport {
        async fn discover_models(
            &self,
            _request: &LlmProbeRequest,
            _cancellation: &CancellationToken,
        ) -> Result<Option<Vec<String>>, LlmProfileError> {
            Ok(None)
        }

        async fn stream_chat(
            &self,
            _request: &LlmProbeRequest,
            _cancellation: &CancellationToken,
        ) -> Result<LlmProbeResponse, LlmProfileError> {
            Err(LlmProfileError::Transport)
        }

        async fn generate_chat(
            &self,
            _request: &LlmProbeRequest,
            _generation: &LlmChatGeneration,
            _cancellation: &CancellationToken,
        ) -> Result<String, LlmProfileError> {
            Err(LlmProfileError::Transport)
        }

        async fn generate_chat_adaptive(
            &self,
            request: &LlmProbeRequest,
            _generation: &LlmChatGeneration,
            _cancellation: &CancellationToken,
        ) -> Result<ChatOutcome, LlmProfileError> {
            self.attempts.lock().unwrap().push(request.reasoning_switch);
            if self.accepted.contains(&request.reasoning_switch) {
                Ok(ChatOutcome::Generated {
                    text: "{}".into(),
                    reasoning_seen: self.reasons_anyway
                        && request.reasoning_switch != ReasoningSwitch::Plain,
                })
            } else {
                Ok(ChatOutcome::Rejected)
            }
        }
    }

    fn switch_service(transport: Arc<SwitchTransport>) -> LlmProfileService {
        LlmProfileService::new(
            SettingsStore::new(tempdir().unwrap().keep()),
            Arc::new(InMemoryCredentialStore::new()),
            transport,
            LlmHostPolicy::desktop(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn the_accepted_thinking_switch_is_learned_and_tried_first_next_time() {
        let transport = Arc::new(SwitchTransport {
            accepted: vec![ReasoningSwitch::EffortNone, ReasoningSwitch::Plain],
            reasons_anyway: false,
            attempts: Mutex::new(Vec::new()),
        });
        let service = switch_service(transport.clone());
        let profile = service
            .create(draft(LlmPreset::LmStudio, "http://127.0.0.1:1234"))
            .await
            .unwrap();
        let cancel = CancellationToken::new();

        service
            .generate(profile.id, tiny_generation(), &cancel)
            .await
            .unwrap();
        assert_eq!(
            *transport.attempts.lock().unwrap(),
            [
                ReasoningSwitch::Both,
                ReasoningSwitch::TemplateKwargs,
                ReasoningSwitch::EffortNone
            ]
        );

        transport.attempts.lock().unwrap().clear();
        service
            .generate(profile.id, tiny_generation(), &cancel)
            .await
            .unwrap();
        assert_eq!(
            *transport.attempts.lock().unwrap(),
            [ReasoningSwitch::EffortNone]
        );
        assert_eq!(
            service.profile(profile.id).unwrap().reasoning_switch,
            Some(ReasoningSwitch::EffortNone)
        );

        // Changing the model forgets what was learned for the old one.
        let mut changed = draft(LlmPreset::LmStudio, "http://127.0.0.1:1234");
        changed.model = "other-model".into();
        let updated = service.update(profile.id, changed).await.unwrap();
        assert_eq!(updated.reasoning_switch, None);
    }

    #[tokio::test]
    async fn a_switch_the_model_ignores_is_dropped_for_the_next_request() {
        let transport = Arc::new(SwitchTransport {
            accepted: ReasoningSwitch::LADDER.to_vec(),
            reasons_anyway: true,
            attempts: Mutex::new(Vec::new()),
        });
        let service = switch_service(transport.clone());
        let profile = service
            .create(draft(LlmPreset::Omlx, "http://127.0.0.1:8000"))
            .await
            .unwrap();
        service
            .generate(profile.id, tiny_generation(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            service.profile(profile.id).unwrap().reasoning_switch,
            Some(ReasoningSwitch::Plain)
        );
    }

    #[tokio::test]
    async fn cloud_style_profiles_never_receive_thinking_parameters() {
        let transport = Arc::new(SwitchTransport {
            accepted: vec![ReasoningSwitch::Plain],
            reasons_anyway: false,
            attempts: Mutex::new(Vec::new()),
        });
        let service = switch_service(transport.clone());
        let profile = service
            .create(draft(LlmPreset::OpenAiCompatible, "http://127.0.0.1:8000"))
            .await
            .unwrap();
        service
            .generate(profile.id, tiny_generation(), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            *transport.attempts.lock().unwrap(),
            [ReasoningSwitch::Plain]
        );
    }
}
